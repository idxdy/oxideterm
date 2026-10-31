//! Owns the herdr Host Tool UI: snapshot lifecycle, workspace/pane tree with
//! agent status chips, action confirms, the input dialog, and pane reads.

use super::*;

use chrono::TimeZone;

use oxideterm_connection_monitor::{
    HERDR_SNAPSHOT_MAX_OUTPUT_SIZE, HERDR_SNAPSHOT_TIMEOUT, HerdrActionKind, HerdrAttachTarget,
    HerdrPane, HerdrSessionDetail, HerdrWorkspace, herdr_agent_matches_query,
    herdr_pane_matches_query, herdr_session_matches_query, herdr_tab_matches_query,
    herdr_workspace_matches_query,
};
use oxideterm_gpui_ui::button::ButtonVariant;

const HOST_HERDR_ACTION_TIMEOUT: Duration = Duration::from_secs(15);
const HOST_HERDR_ACTION_MAX_OUTPUT_SIZE: usize = 64 * 1024;
const HOST_HERDR_READ_TIMEOUT: Duration = Duration::from_secs(10);
const HOST_HERDR_READ_MAX_OUTPUT_SIZE: usize = 256 * 1024;
const HOST_HERDR_DETAIL_TIMEOUT: Duration = Duration::from_secs(12);
const HOST_HERDR_DETAIL_MAX_OUTPUT_SIZE: usize = 512 * 1024;

/// herdr's own level names (`tab`, `pane`) stay untranslated; only the plural
/// marker is formed here so counts read naturally.
fn herdr_count_label(count: u32, unit: &str) -> String {
    if count == 1 {
        format!("{count} {unit}")
    } else {
        format!("{count} {unit}s")
    }
}

fn herdr_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn format_herdr_clock(ms: u64) -> String {
    chrono::Local
        .timestamp_millis_opt(ms as i64)
        .single()
        .map(|time| time.format("%H:%M:%S").to_string())
        .unwrap_or_default()
}

impl WorkspaceApp {
    pub(super) fn render_host_herdr_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens;
        let i18n = &self.i18n;
        let mono_font_family = settings_mono_font_family(self.settings_store.settings());
        let selectable_text = self.selectable_text_render_state(cx);
        let connections = self.monitor_connections(cx);
        let selected_connection_id = self.host_tools.read(cx).selected_connection_id_owned();
        let selected_connection_id = selected_connection_id.as_deref().or_else(|| {
            connections
                .first()
                .map(|connection| connection.connection_id.as_str())
        });
        let terminal_available = selected_connection_id
            .is_some_and(|connection_id| self.host_tools_terminal_available(connection_id));
        let search_ime = self
            .host_tools_plain_text_ime_frame(HostToolsTextInput::HerdrSearch, cx)
            .expect("herdr search is a non-secret Host Tools input");
        self.host_tools.update(cx, |host_tools, cx| {
            host_tools.render_host_herdr_panel(
                &search_ime,
                terminal_available,
                &tokens,
                i18n,
                mono_font_family,
                &selectable_text,
                cx,
            )
        })
    }

    pub(in crate::workspace) fn handle_host_herdr_confirm_key(
        &mut self,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.host_tools.read(cx).herdr_confirm_view().is_none() {
            return false;
        }
        match self.handle_standard_confirm_key(event, cx) {
            Some(ConfirmKeyboardAction::Cancel) => {
                self.begin_host_herdr_confirm_exit(cx);
                true
            }
            Some(ConfirmKeyboardAction::Confirm) => {
                self.confirm_host_herdr_action(cx);
                true
            }
            Some(ConfirmKeyboardAction::Handled) => true,
            None => false,
        }
    }

    pub(in crate::workspace) fn handle_host_herdr_input_dialog_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self
            .host_tools
            .read(cx)
            .ui
            .host_herdr_input_dialog
            .is_none()
        {
            return false;
        }
        if event.keystroke.modifiers.platform {
            return false;
        }
        match event.keystroke.key.as_str() {
            "escape" => {
                self.host_tools.update(cx, |host_tools, cx| {
                    host_tools.dismiss_herdr_input_dialog(cx);
                });
                self.ime_marked_text = None;
                self.clear_ime_selection();
                cx.notify();
                true
            }
            "enter" => {
                self.submit_host_herdr_input_dialog(window, cx);
                true
            }
            _ => false,
        }
    }

    pub(in crate::workspace) fn handle_host_herdr_search_key(
        &mut self,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self
            .host_tools
            .read(cx)
            .ui
            .input_is_focused(HostToolsTextInput::HerdrSearch)
        {
            return false;
        }
        if event.keystroke.key.as_str() == "escape" && !event.keystroke.modifiers.platform {
            self.host_tools.update(cx, |host_tools, _cx| {
                host_tools.ui.clear_input_focus();
            });
            self.ime_marked_text = None;
            self.clear_ime_selection();
            cx.notify();
            return true;
        }
        false
    }

    pub(super) fn confirm_host_herdr_action(&mut self, cx: &mut Context<Self>) {
        self.clear_standard_confirm_focus();
        let delay = oxideterm_gpui_ui::motion::duration(
            &self.tokens,
            oxideterm_gpui_ui::motion::MotionDuration::Control,
        );
        self.host_tools.update(cx, |host_tools, cx| {
            host_tools.confirm_herdr_action_from_view(delay, cx);
        });
    }

    /// Keeps the request mounted until the current exit generation completes.
    fn begin_host_herdr_confirm_exit(&mut self, cx: &mut Context<Self>) -> bool {
        self.clear_standard_confirm_focus();
        let delay = oxideterm_gpui_ui::motion::duration(
            &self.tokens,
            oxideterm_gpui_ui::motion::MotionDuration::Control,
        );
        self.host_tools.update(cx, |host_tools, cx| {
            host_tools.begin_herdr_confirm_exit(delay, cx)
        })
    }

    pub(super) fn submit_host_herdr_input_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A new session is created-or-attached by typing `herdr --session <name>`
        // into a terminal, so this one kind is dispatched here where the window
        // and the localized notices live; every other kind is a captured action.
        let new_session = self
            .host_tools
            .read(cx)
            .ui
            .host_herdr_input_dialog
            .as_ref()
            .filter(|dialog| matches!(dialog.kind, HostHerdrInputDialogKind::NewSession))
            .map(|dialog| dialog.value.trim().to_string());
        if let Some(name) = new_session {
            if name.is_empty() {
                return;
            }
            let connection_id = self.host_tools.read(cx).selected_connection_id_owned();
            let Some(connection_id) = connection_id else {
                self.host_tools.update(cx, |host_tools, cx| {
                    host_tools.dismiss_herdr_input_dialog(cx);
                    cx.emit(HostToolsEvent::ShowNotice(
                        HostToolsNotice::HerdrConnectionMissing,
                    ));
                });
                self.ime_marked_text = None;
                self.clear_ime_selection();
                return;
            };
            let title = self.i18n.t("sidebar.host_herdr.attach_title");
            let opened = self.i18n.t("sidebar.host_herdr.toast.attach_opened");
            let missing = self.i18n.t("sidebar.host_herdr.toast.connection_missing");
            self.host_tools.update(cx, |host_tools, cx| {
                host_tools.dismiss_herdr_input_dialog(cx);
                host_tools.dispatch_herdr_terminal(
                    connection_id,
                    HerdrAttachTarget::NewSession { name },
                    title,
                    opened,
                    missing,
                    window,
                    cx,
                );
            });
        } else {
            self.host_tools.update(cx, |host_tools, cx| {
                host_tools.submit_herdr_input_from_view(cx);
            });
        }
        self.ime_marked_text = None;
        self.clear_ime_selection();
    }

    pub(in crate::workspace) fn render_host_herdr_confirm_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let tokens = self.tokens;
        let i18n = &self.i18n;
        let focused_action = self.standard_confirm_focus();
        self.host_tools.update(cx, |host_tools, cx| {
            host_tools.render_herdr_confirm_view(&tokens, i18n, focused_action, cx)
        })
    }

    pub(in crate::workspace) fn render_host_herdr_input_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let target = WorkspaceImeTarget::HostHerdrDialog;
        let (kind, submit_disabled, input_control) = {
            let host_tools = self.host_tools.read(cx);
            let ui = &host_tools.ui;
            let dialog = ui.host_herdr_input_dialog.as_ref()?;
            let input_control = self
                .workspace_text_input(
                    target,
                    TextInputView {
                        value: dialog.value.as_str(),
                        placeholder: self.i18n.t(herdr_input_placeholder_key(&dialog.kind)),
                        focused: ui.input_is_focused(HostToolsTextInput::HerdrDialog),
                        caret_visible: self.input_caret.visible(),
                        secret: false,
                        selected_all: false,
                        selected_range: self.ime_selected_range_for_target(target, cx),
                        marked_text: self.marked_text_for_target(target, cx),
                    },
                )
                .h(px(34.0))
                .cursor(CursorStyle::IBeam);
            (
                dialog.kind.clone(),
                dialog.value.trim().is_empty() || host_tools.herdr_action_running(),
                input_control,
            )
        };
        let workspace = cx.entity();
        let input_control = text_input_anchor_probe(
            target.anchor_id(),
            input_control
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        this.host_tools.update(cx, |host_tools, _cx| {
                            host_tools.ui.focus_input(HostToolsTextInput::HerdrDialog);
                        });
                        this.ime_marked_text = None;
                        this.show_active_input_caret(cx);
                        window.focus(&this.focus_handle, cx);
                        this.begin_ime_selection_from_mouse_down(target, event, window, cx);
                        cx.stop_propagation();
                    }),
                )
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                    this.update_ime_selection_drag_from_mouse_move(event, window, cx);
                })),
            move |anchor, _window, cx| {
                let _ = workspace.update(cx, |this, cx| {
                    this.update_text_input_anchor(anchor, cx);
                });
            },
        )
        .into_any_element();
        let cancel_action = self.workspace_confirm_footer_action_button(
            self.i18n.t("sidebar.host_herdr.confirm.cancel"),
            ButtonVariant::Secondary,
            ConfirmDialogAction::Cancel,
            false,
            None,
            |this, _event, _window, cx| {
                this.host_tools.update(cx, |host_tools, cx| {
                    host_tools.dismiss_herdr_input_dialog(cx);
                });
                this.ime_marked_text = None;
                this.clear_ime_selection();
                cx.notify();
            },
            cx,
        );
        let submit_label = match kind {
            HostHerdrInputDialogKind::RunPane { .. } => {
                self.i18n.t("sidebar.host_herdr.actions.send")
            }
            _ => self.i18n.t("sidebar.host_herdr.confirm.run"),
        };
        let submit_action = self.workspace_confirm_footer_action_button(
            submit_label,
            ButtonVariant::Default,
            ConfirmDialogAction::Confirm,
            submit_disabled,
            None,
            |this, _event, window, cx| {
                this.submit_host_herdr_input_dialog(window, cx);
            },
            cx,
        );
        let theme = self.tokens.ui;
        let dialog = div()
            .w(px(560.0))
            .child(
                div()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(theme.border))
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(rgb(theme.text))
                            .child(self.i18n.t(herdr_input_title_key(&kind))),
                    ),
            )
            .child(div().p_4().flex().flex_col().gap_2().child(input_control))
            .child(
                div()
                    .px_4()
                    .py_3()
                    .border_t_1()
                    .border_color(rgb(theme.border))
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(cancel_action)
                    .child(submit_action),
            );

        Some(
            oxideterm_gpui_ui::modal::dismissible_dialog_backdrop()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event, _window, cx| {
                        this.host_tools.update(cx, |host_tools, cx| {
                            host_tools.dismiss_herdr_input_dialog(cx);
                        });
                        this.ime_marked_text = None;
                        this.clear_ime_selection();
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .child(oxideterm_gpui_ui::modal::overlay_content_boundary(dialog))
                .into_any_element(),
        )
    }

    pub(in crate::workspace) fn render_host_herdr_read_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.host_tools.read(cx).herdr_read_dialog()?;
        let theme = self.tokens.ui;
        let content = if dialog.loading {
            div()
                .p_4()
                .text_color(rgb(theme.text_muted))
                .child(self.i18n.t("sidebar.host_herdr.read.loading"))
                .into_any_element()
        } else if let Some(error) = dialog.error.as_ref() {
            div()
                .p_4()
                .text_color(rgb(MONITOR_RED))
                .child(error.clone())
                .into_any_element()
        } else {
            let output = dialog.output.as_deref();
            let mut lines = div()
                .p_3()
                .flex()
                .flex_col()
                .gap(px(1.0))
                .font_family(settings_mono_font_family(self.settings_store.settings()))
                .text_size(px(11.0))
                .text_color(rgb(theme.text));
            let mut rendered = 0;
            if let Some(output) = output {
                for (index, line) in output.lines().enumerate() {
                    let display_line = if line.is_empty() {
                        " ".to_string()
                    } else {
                        line.chars().take(512).collect::<String>()
                    };
                    lines = lines.child(
                        div()
                            .id(("host-herdr-read-line", index))
                            .flex_none()
                            .whitespace_nowrap()
                            .child(display_line),
                    );
                    rendered += 1;
                }
            }
            if rendered == 0 {
                lines = lines.child(div().child(self.i18n.t("sidebar.host_herdr.read.empty")));
            }
            lines.into_any_element()
        };

        Some(
            oxideterm_gpui_ui::modal::dismissible_dialog_backdrop()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event, _window, cx| {
                        this.host_tools.update(cx, |host_tools, cx| {
                            host_tools.dismiss_herdr_read_dialog(cx);
                        });
                        cx.stop_propagation();
                    }),
                )
                .child(oxideterm_gpui_ui::modal::overlay_content_boundary(
                    oxideterm_gpui_ui::modal::dialog_content(&self.tokens)
                        .w(px(760.0))
                        .max_h(px(520.0))
                        .child(
                            div()
                                .flex_none()
                                .px_4()
                                .py_3()
                                .border_b_1()
                                .border_color(rgb(theme.border))
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            div()
                                                .truncate()
                                                .text_size(px(14.0))
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .text_color(rgb(theme.text))
                                                .child(
                                                    self.i18n
                                                        .t("sidebar.host_herdr.read.title")
                                                        .replace(
                                                            "{{pane}}",
                                                            &dialog.request.cwd_label,
                                                        ),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .truncate()
                                                .text_size(px(11.0))
                                                .text_color(rgb(theme.text_muted))
                                                .child(dialog.request.pane_id.clone()),
                                        ),
                                )
                                .child(host_tools_tooltip_icon_button(
                                    &self.tokens,
                                    LucideIcon::X,
                                    14.0,
                                    rgb(theme.text_muted),
                                    oxideterm_gpui_ui::button::IconButtonOptions {
                                        size: 24.0,
                                        has_background: true,
                                        background: Some(rgb(theme.bg_hover)),
                                        hover_background: Some(rgb(theme.bg_panel)),
                                        idle_opacity: 1.0,
                                        ..oxideterm_gpui_ui::button::IconButtonOptions::compact(
                                            24.0,
                                        )
                                    },
                                    self.i18n.t("sidebar.host_herdr.read.close"),
                                    "host-herdr-read-close",
                                    true,
                                    cx.listener(|this, _event, _window, cx| {
                                        this.host_tools.update(cx, |host_tools, cx| {
                                            host_tools.dismiss_herdr_read_dialog(cx);
                                        });
                                        cx.stop_propagation();
                                    }),
                                )),
                        )
                        .child(
                            div()
                                .id("host-herdr-read-scroll")
                                .min_h(px(120.0))
                                .max_h(px(436.0))
                                .overflow_y_scroll()
                                .overflow_x_scroll()
                                .child(content),
                        ),
                ))
                .into_any_element(),
        )
    }
}

struct HostHerdrPanelSnapshot {
    connections: Vec<MonitorConnectionOption>,
    selected_connection_id: String,
    snapshot: Option<HerdrSnapshot>,
    last_refreshed_ms: Option<u64>,
}

impl HostToolsEntity {
    // ---- snapshot lifecycle ----

    pub(in crate::workspace::connection_monitor) fn request_herdr_snapshot(
        &mut self,
        connection_id: String,
        feedback: HostSnapshotFeedback,
        failure_fallback: String,
        runtime: tokio::runtime::Handle,
        cx: &mut Context<Self>,
    ) {
        let request = HostHerdrSnapshotRequest {
            connection_id,
            feedback,
            failure_fallback,
        };
        // Several triggers (tool selection, sidebar becoming visible, retry)
        // can ask for the same sample; capture it once.
        if self.host_herdr.snapshot_running.as_ref() == Some(&request)
            || self.host_herdr.snapshot_pending.as_ref() == Some(&request)
        {
            return;
        }
        if self.host_herdr.snapshot_in_flight {
            // Keep only the newest request while the bounded snapshot runs.
            if feedback.should_toast() {
                cx.emit(HostToolsEvent::ShowNotice(
                    HostToolsNotice::HerdrSnapshotAlreadyRunning,
                ));
            }
            self.host_herdr.snapshot_pending = Some(request);
            return;
        }
        self.start_herdr_snapshot(request, runtime, cx);
    }

    fn start_herdr_snapshot(
        &mut self,
        request: HostHerdrSnapshotRequest,
        runtime: tokio::runtime::Handle,
        cx: &mut Context<Self>,
    ) {
        let Some(os_type) = self.connection_os_type(&request.connection_id) else {
            self.host_herdr.snapshot_connection_id = Some(request.connection_id);
            self.host_herdr.snapshot = Some(HerdrSnapshot {
                status: HerdrStatus::Error {
                    message: request.failure_fallback,
                },
                ..HerdrSnapshot::default()
            });
            cx.notify();
            return;
        };
        let command = build_herdr_snapshot_command(&os_type);
        if self.host_herdr.snapshot_connection_id.as_deref() != Some(request.connection_id.as_str())
        {
            self.host_herdr.snapshot = None;
            // Subtrees belong to one node's sessions; a different connection
            // must not show them.
            self.host_herdr.session_details.clear();
            self.host_herdr.session_detail_running = None;
            self.host_herdr.expanded_session = None;
            self.ui.host_herdr_expanded_workspace = None;
            self.ui.host_herdr_expanded_tab = None;
        }
        self.host_herdr.snapshot_connection_id = Some(request.connection_id.clone());
        self.host_herdr.snapshot_running = Some(request.clone());
        self.host_herdr.snapshot_in_flight = true;
        let spawned = self.spawn_herdr_snapshot_capture(
            command.command,
            request.clone(),
            HERDR_SNAPSHOT_TIMEOUT,
            HERDR_SNAPSHOT_MAX_OUTPUT_SIZE,
            runtime,
        );
        if !spawned {
            self.host_herdr.snapshot_running = None;
            self.host_herdr.snapshot_in_flight = false;
            // The capture never started (no live SSH route); end the loading
            // state so the panel shows a failure the user can retry.
            self.host_herdr.snapshot = Some(HerdrSnapshot {
                status: HerdrStatus::Error {
                    message: request.failure_fallback,
                },
                ..HerdrSnapshot::default()
            });
        }
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn finish_host_herdr_snapshot(
        &mut self,
        delivery: HostHerdrSnapshotDelivery,
        cx: &mut Context<Self>,
    ) {
        if self.host_herdr.snapshot_running.as_ref() != Some(&delivery.request) {
            return;
        }
        self.host_herdr.snapshot_in_flight = false;
        self.host_herdr.snapshot_running = None;
        let snapshot = match delivery.result {
            Ok(mut output) if output.exit_code.unwrap_or(0) == 0 => {
                let parsed = parse_herdr_snapshot(&output.stdout);
                zeroize::Zeroize::zeroize(&mut output.stdout);
                zeroize::Zeroize::zeroize(&mut output.stderr);
                parsed
            }
            Ok(mut output) => {
                // Snapshot failures use a localized safe fallback, never the
                // captured remote output.
                zeroize::Zeroize::zeroize(&mut output.stdout);
                zeroize::Zeroize::zeroize(&mut output.stderr);
                HerdrSnapshot {
                    status: HerdrStatus::Error {
                        message: delivery.request.failure_fallback.clone(),
                    },
                    ..HerdrSnapshot::default()
                }
            }
            Err(()) => HerdrSnapshot {
                status: HerdrStatus::Error {
                    message: delivery.request.failure_fallback.clone(),
                },
                ..HerdrSnapshot::default()
            },
        };
        let connection_id = delivery.request.connection_id;
        self.host_herdr.snapshot_connection_id = Some(connection_id.clone());
        self.host_herdr.snapshot = Some(snapshot);
        self.host_herdr.last_refreshed_ms = Some(herdr_now_ms());
        // The session list may have changed, so every cached subtree is stale.
        self.host_herdr.session_details.clear();
        let expanded_removed = self.host_herdr.snapshot.as_ref().is_some_and(|snapshot| {
            matches!(snapshot.status, HerdrStatus::Available)
                && self
                    .host_herdr
                    .expanded_session
                    .as_deref()
                    .is_some_and(|expanded| {
                        !snapshot
                            .sessions
                            .iter()
                            .any(|session| session.name == expanded)
                    })
        });
        if expanded_removed {
            self.host_herdr.expanded_session = None;
        }
        // A request that arrived mid-flight reuses the same bounded channel;
        // dropping it would leave the panel stale after a switch.
        let pending = self.host_herdr.snapshot_pending.take();
        if let Some(pending) = pending
            && let Some(runtime) = self.lifecycle_runtime.clone()
        {
            self.start_herdr_snapshot(pending, runtime, cx);
        } else if let Some(runtime) = self.lifecycle_runtime.clone()
            && let Some(session) = self.host_herdr.expanded_session.clone()
        {
            self.request_herdr_session_detail(connection_id, session, runtime, cx);
        }
        cx.notify();
    }

    // ---- session subtrees ----

    /// Expanding a session loads its workspaces from that session's socket;
    /// collapsing keeps the cache so re-opening is free.
    fn toggle_herdr_session(&mut self, session: &str, cx: &mut Context<Self>) {
        if self.host_herdr.expanded_session.as_deref() == Some(session) {
            self.host_herdr.expanded_session = None;
            cx.notify();
            return;
        }
        self.host_herdr.expanded_session = Some(session.to_string());
        self.ui.host_herdr_expanded_workspace = None;
        self.ui.host_herdr_expanded_tab = None;
        cx.notify();
        if self.host_herdr.session_details.contains_key(session) {
            return;
        }
        let Some(connection_id) = self.host_herdr.snapshot_connection_id.clone() else {
            return;
        };
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            return;
        };
        self.request_herdr_session_detail(connection_id, session.to_string(), runtime, cx);
    }

    pub(in crate::workspace::connection_monitor) fn request_herdr_session_detail(
        &mut self,
        connection_id: String,
        session: String,
        runtime: tokio::runtime::Handle,
        cx: &mut Context<Self>,
    ) {
        if self
            .host_herdr
            .session_detail_running
            .as_ref()
            .is_some_and(|running| running.session == session)
        {
            return;
        }
        let Some(os_type) = self.connection_os_type(&connection_id) else {
            return;
        };
        let command = match build_herdr_session_detail_command(&os_type, &session) {
            Ok(command) => command,
            Err(_) => return,
        };
        let failure_fallback = self
            .messages
            .as_ref()
            .map(|messages| messages.herdr_unknown_error.clone())
            .unwrap_or_default();
        let request = HostHerdrSessionDetailRequest {
            connection_id,
            session,
            failure_fallback,
        };
        self.host_herdr.session_detail_running = Some(request.clone());
        let spawned = self.spawn_herdr_session_detail(
            command.command,
            request,
            HOST_HERDR_DETAIL_TIMEOUT,
            HOST_HERDR_DETAIL_MAX_OUTPUT_SIZE,
            runtime,
        );
        if !spawned {
            self.host_herdr.session_detail_running = None;
        }
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn finish_host_herdr_session_detail(
        &mut self,
        delivery: HostHerdrSessionDetailDelivery,
        cx: &mut Context<Self>,
    ) {
        if self.host_herdr.session_detail_running.as_ref() != Some(&delivery.request) {
            return;
        }
        self.host_herdr.session_detail_running = None;
        let HostHerdrSessionDetailRequest {
            session,
            failure_fallback,
            ..
        } = delivery.request;
        let detail = match delivery.result {
            Ok(mut output) if output.exit_code.unwrap_or(0) == 0 => {
                let detail = parse_herdr_session_detail(&output.stdout);
                zeroize::Zeroize::zeroize(&mut output.stdout);
                zeroize::Zeroize::zeroize(&mut output.stderr);
                detail
            }
            Ok(mut output) => {
                // Failures use a localized fallback, never captured output.
                zeroize::Zeroize::zeroize(&mut output.stdout);
                zeroize::Zeroize::zeroize(&mut output.stderr);
                HerdrSessionDetail {
                    status: HerdrStatus::Error {
                        message: failure_fallback,
                    },
                    ..HerdrSessionDetail::default()
                }
            }
            Err(()) => HerdrSessionDetail {
                status: HerdrStatus::Error {
                    message: failure_fallback,
                },
                ..HerdrSessionDetail::default()
            },
        };
        self.host_herdr.session_details.insert(session, detail);
        cx.notify();
    }

    // ---- destructive actions (confirm) ----

    pub(in crate::workspace::connection_monitor) fn open_herdr_action_confirm(
        &mut self,
        request: HostHerdrActionRequest,
        cx: &mut Context<Self>,
    ) -> Option<HostToolsNotice> {
        if self.host_herdr.action_running.is_some() {
            return Some(HostToolsNotice::HerdrActionAlreadyRunning);
        }
        HostToolConfirmState::open(&mut self.host_herdr.pending_confirm, request);
        cx.notify();
        None
    }

    pub(in crate::workspace::connection_monitor) fn herdr_confirm_view(
        &self,
    ) -> Option<(HostHerdrActionRequest, oxideterm_gpui_ui::motion::ExitPhase)> {
        self.host_herdr
            .pending_confirm
            .as_ref()
            .map(|state| (state.request.clone(), state.presence.phase()))
    }

    pub(in crate::workspace::connection_monitor) fn dismiss_herdr_confirm(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.host_herdr.pending_confirm.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn begin_herdr_confirm_exit(
        &mut self,
        delay: Duration,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(generation) = self
            .host_herdr
            .pending_confirm
            .as_mut()
            .and_then(|state| state.presence.begin_exit())
        else {
            return false;
        };
        if delay.is_zero() {
            self.host_herdr.pending_confirm = None;
            cx.notify();
            return true;
        }
        cx.spawn(async move |weak, cx| {
            Timer::after(delay).await;
            let _ = weak.update(cx, |entity, cx| {
                if entity
                    .host_herdr
                    .pending_confirm
                    .as_ref()
                    .is_some_and(|state| state.presence.finish_exit(generation))
                {
                    entity.host_herdr.pending_confirm = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
        true
    }

    pub(super) fn confirm_herdr_action_from_view(
        &mut self,
        delay: Duration,
        cx: &mut Context<Self>,
    ) {
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            self.begin_herdr_confirm_exit(delay, cx);
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::HerdrConnectionMissing,
            ));
            return;
        };
        for notice in self.confirm_herdr_action(delay, runtime, cx) {
            cx.emit(HostToolsEvent::ShowNotice(notice));
        }
    }

    pub(super) fn confirm_herdr_action(
        &mut self,
        delay: Duration,
        runtime: tokio::runtime::Handle,
        cx: &mut Context<Self>,
    ) -> Vec<HostToolsNotice> {
        let Some(request) = self
            .host_herdr
            .pending_confirm
            .as_ref()
            .map(|state| state.request.clone())
        else {
            return Vec::new();
        };
        if !self.begin_herdr_confirm_exit(delay, cx) {
            return Vec::new();
        }
        self.start_herdr_destructive_action(request, runtime, cx)
    }

    fn start_herdr_destructive_action(
        &mut self,
        request: HostHerdrActionRequest,
        runtime: tokio::runtime::Handle,
        cx: &mut Context<Self>,
    ) -> Vec<HostToolsNotice> {
        let Some(os_type) = self.connection_os_type(&request.connection_id) else {
            return vec![HostToolsNotice::HerdrConnectionMissing];
        };
        let action_kind = match &request.action {
            HostHerdrDestructiveAction::CloseWorkspace { workspace_id } => {
                HerdrActionKind::CloseWorkspace {
                    workspace_id: workspace_id.clone(),
                }
            }
            HostHerdrDestructiveAction::ClosePane { pane_id } => HerdrActionKind::ClosePane {
                pane_id: pane_id.clone(),
            },
            HostHerdrDestructiveAction::CloseTab { tab_id } => HerdrActionKind::CloseTab {
                tab_id: tab_id.clone(),
            },
            HostHerdrDestructiveAction::StopSession { name } => {
                HerdrActionKind::StopSession { name: name.clone() }
            }
            HostHerdrDestructiveAction::DeleteSession { name } => {
                HerdrActionKind::DeleteSession { name: name.clone() }
            }
        };
        let command = match build_herdr_action_command(&os_type, &request.session, &action_kind) {
            Ok(command) => command,
            Err(_) => return vec![HostToolsNotice::HerdrActionFailed],
        };
        let run = HostHerdrActionRun {
            connection_id: request.connection_id,
            target_label: request.target_label,
        };
        self.host_herdr.action_running = Some(run.clone());
        let spawned = self.spawn_herdr_action(
            zeroize::Zeroizing::new(command.command),
            run,
            HOST_HERDR_ACTION_TIMEOUT,
            HOST_HERDR_ACTION_MAX_OUTPUT_SIZE,
            runtime,
        );
        if !spawned {
            self.host_herdr.action_running = None;
            return vec![HostToolsNotice::HerdrConnectionMissing];
        }
        cx.notify();
        Vec::new()
    }

    pub(in crate::workspace::connection_monitor) fn finish_host_herdr_action(
        &mut self,
        delivery: HostHerdrActionDelivery,
        cx: &mut Context<Self>,
    ) {
        if self.host_herdr.action_running.as_ref() != Some(&delivery.request) {
            return;
        }
        self.host_herdr.action_running = None;
        let HostHerdrActionDelivery { request, result } = delivery;
        let HostHerdrActionRun {
            connection_id,
            target_label,
        } = request;
        cx.emit(HostToolsEvent::ShowNotice(
            HostToolsNotice::HerdrActionFinished {
                target_label,
                succeeded: result.unwrap_or(false),
            },
        ));
        // Sample the mutated connection only while this tool is what the user
        // is looking at; a hidden or switched panel is refreshed on entry.
        if self.monitoring.herdr_enabled
            && self.visibility.sidebar_is_visible()
            && self.active_tool() == ContextSidebarTool::Herdr
            && let Some(runtime) = self.lifecycle_runtime.clone()
        {
            let failure_fallback = self
                .messages
                .clone()
                .map(|messages| messages.herdr_unknown_error)
                .unwrap_or_default();
            self.request_herdr_snapshot(
                connection_id,
                HostSnapshotFeedback::Silent,
                failure_fallback,
                runtime,
                cx,
            );
        }
        cx.notify();
    }

    // ---- input dialog (non-destructive actions) ----

    fn herdr_action_running(&self) -> bool {
        self.host_herdr.action_running.is_some()
    }

    /// Session that toolbar actions (new workspace) apply to: the expanded one,
    /// otherwise the first listed session.
    fn herdr_action_session(&self) -> Option<String> {
        self.host_herdr.expanded_session.clone().or_else(|| {
            self.host_herdr.snapshot.as_ref().and_then(|snapshot| {
                snapshot
                    .sessions
                    .first()
                    .map(|session| session.name.clone())
            })
        })
    }

    pub(in crate::workspace::connection_monitor) fn open_herdr_input_dialog(
        &mut self,
        kind: HostHerdrInputDialogKind,
        session: &str,
        prefill: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ui.host_herdr_input_dialog = Some(HostHerdrInputDialog {
            kind,
            session: session.to_string(),
            value: zeroize::Zeroizing::new(prefill.to_string()),
        });
        self.ui.focus_input(HostToolsTextInput::HerdrDialog);
        // Sidebar clicks do not move keyboard focus; without this handoff the
        // typed run command would keep going to the focused terminal.
        window.dispatch_action(
            Box::new(HostToolsWindowRequest::new(
                HostToolsWindowIntent::PrepareHerdrInputDialog,
            )),
            cx,
        );
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn dismiss_herdr_input_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.ui.host_herdr_input_dialog.take().is_some() {
            self.ui.clear_input_focus();
            cx.notify();
        }
    }

    pub(super) fn submit_herdr_input_from_view(&mut self, cx: &mut Context<Self>) {
        // Enter reaches this path while the submit button is disabled; keep the
        // dialog so the typed value is not lost to a rejected command.
        if self
            .ui
            .host_herdr_input_dialog
            .as_ref()
            .is_none_or(|dialog| dialog.value.trim().is_empty())
        {
            return;
        }
        if self.host_herdr.action_running.is_some() {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::HerdrActionAlreadyRunning,
            ));
            cx.notify();
            return;
        }
        let Some(dialog) = self.ui.host_herdr_input_dialog.take() else {
            return;
        };
        self.ui.clear_input_focus();
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::HerdrConnectionMissing,
            ));
            cx.notify();
            return;
        };
        let Some(connection_id) = self.selected_connection_id_owned() else {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::HerdrConnectionMissing,
            ));
            cx.notify();
            return;
        };
        let (action_kind, label) = match &dialog.kind {
            HostHerdrInputDialogKind::NewWorkspace => {
                // "label path" packs both fields into one input line.
                let mut parts = dialog.value.splitn(2, char::is_whitespace);
                let label = parts.next().unwrap_or_default().to_string();
                let cwd = parts.next().unwrap_or_default().trim().to_string();
                (
                    HerdrActionKind::NewWorkspace {
                        label: label.clone(),
                        cwd,
                    },
                    label,
                )
            }
            HostHerdrInputDialogKind::RenameWorkspace { workspace_id } => {
                let label = dialog.value.trim().to_string();
                (
                    HerdrActionKind::RenameWorkspace {
                        workspace_id: workspace_id.clone(),
                        label: label.clone(),
                    },
                    label,
                )
            }
            HostHerdrInputDialogKind::RenameTab { tab_id } => {
                let label = dialog.value.trim().to_string();
                (
                    HerdrActionKind::RenameTab {
                        tab_id: tab_id.clone(),
                        label: label.clone(),
                    },
                    label,
                )
            }
            HostHerdrInputDialogKind::RenamePane { pane_id } => {
                let label = dialog.value.trim().to_string();
                (
                    HerdrActionKind::RenamePane {
                        pane_id: pane_id.clone(),
                        label: label.clone(),
                    },
                    label,
                )
            }
            HostHerdrInputDialogKind::RunPane { pane_id } => (
                HerdrActionKind::RunPane {
                    pane_id: pane_id.clone(),
                    command: dialog.value.trim().to_string(),
                },
                pane_id.clone(),
            ),
            // A new session is a terminal command, dispatched by the view.
            HostHerdrInputDialogKind::NewSession => return,
        };
        for notice in self.start_herdr_built_action(
            connection_id,
            dialog.session,
            label,
            action_kind,
            runtime,
            cx,
        ) {
            cx.emit(HostToolsEvent::ShowNotice(notice));
        }
        cx.notify();
    }

    fn start_herdr_built_action(
        &mut self,
        connection_id: String,
        session: String,
        target_label: String,
        action_kind: HerdrActionKind,
        runtime: tokio::runtime::Handle,
        cx: &mut Context<Self>,
    ) -> Vec<HostToolsNotice> {
        // One mutating herdr command at a time; the completion toast and the
        // follow-up refresh are matched on this single slot.
        if self.host_herdr.action_running.is_some() {
            return vec![HostToolsNotice::HerdrActionAlreadyRunning];
        }
        let Some(os_type) = self.connection_os_type(&connection_id) else {
            return vec![HostToolsNotice::HerdrConnectionMissing];
        };
        let command = match build_herdr_action_command(&os_type, &session, &action_kind) {
            Ok(command) => command,
            Err(_) => return vec![HostToolsNotice::HerdrActionFailed],
        };
        let run = HostHerdrActionRun {
            connection_id,
            target_label,
        };
        self.host_herdr.action_running = Some(run.clone());
        let spawned = self.spawn_herdr_action(
            zeroize::Zeroizing::new(command.command),
            run,
            HOST_HERDR_ACTION_TIMEOUT,
            HOST_HERDR_ACTION_MAX_OUTPUT_SIZE,
            runtime,
        );
        if !spawned {
            self.host_herdr.action_running = None;
            return vec![HostToolsNotice::HerdrConnectionMissing];
        }
        cx.notify();
        Vec::new()
    }

    // ---- pane reads ----

    pub(in crate::workspace::connection_monitor) fn request_herdr_read(
        &mut self,
        session: String,
        pane_id: String,
        cwd_label: String,
        cx: &mut Context<Self>,
    ) {
        if self
            .host_herdr
            .read_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.loading)
        {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::HerdrReadAlreadyRunning,
            ));
            return;
        }
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::HerdrConnectionMissing,
            ));
            return;
        };
        let Some(connection_id) = self.selected_connection_id_owned() else {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::HerdrConnectionMissing,
            ));
            return;
        };
        let Some(os_type) = self.connection_os_type(&connection_id) else {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::HerdrConnectionMissing,
            ));
            return;
        };
        let failure_fallback = self
            .messages
            .clone()
            .map(|messages| messages.herdr_unknown_error)
            .unwrap_or_default();
        let command = match build_herdr_read_command(&os_type, &session, &pane_id) {
            Ok(command) => command,
            Err(_) => {
                cx.emit(HostToolsEvent::ShowNotice(HostToolsNotice::HerdrReadFailed));
                return;
            }
        };
        let request = HostHerdrReadRequest {
            connection_id,
            session,
            pane_id,
            cwd_label,
            failure_fallback,
        };
        self.host_herdr.read_dialog = Some(HostHerdrReadDialog {
            request: request.clone(),
            output: None,
            error: None,
            loading: true,
        });
        let spawned = self.spawn_herdr_read(
            command.command,
            request,
            HOST_HERDR_READ_TIMEOUT,
            HOST_HERDR_READ_MAX_OUTPUT_SIZE,
            runtime,
        );
        if !spawned {
            self.host_herdr.read_dialog = None;
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::HerdrConnectionMissing,
            ));
        }
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn dismiss_herdr_read_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.host_herdr.read_dialog.take().is_some() {
            cx.notify();
        }
    }

    pub(in crate::workspace::connection_monitor) fn finish_host_herdr_read(
        &mut self,
        delivery: HostHerdrReadDelivery,
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = self
            .host_herdr
            .read_dialog
            .as_mut()
            .filter(|dialog| dialog.request == delivery.request)
        else {
            return;
        };
        dialog.loading = false;
        match delivery.result {
            Ok(mut output) if output.exit_code.unwrap_or(0) == 0 => {
                zeroize::Zeroize::zeroize(&mut output.stderr);
                // The last Entity/render owner clears the retained buffer.
                dialog.output = Some(Arc::new(zeroize::Zeroizing::new(std::mem::take(
                    &mut output.stdout,
                ))));
                dialog.error = None;
            }
            Ok(mut output) => {
                // Failed output is never rendered or copied into an error.
                zeroize::Zeroize::zeroize(&mut output.stdout);
                zeroize::Zeroize::zeroize(&mut output.stderr);
                dialog.output = None;
                dialog.error = Some(delivery.request.failure_fallback.clone());
            }
            Err(()) => {
                dialog.output = None;
                dialog.error = Some(delivery.request.failure_fallback.clone());
            }
        }
        cx.notify();
    }

    fn herdr_read_dialog(&self) -> Option<HostHerdrReadDialog> {
        self.host_herdr.read_dialog.clone()
    }

    fn refresh_herdr_snapshot_from_view(
        &mut self,
        feedback: HostSnapshotFeedback,
        cx: &mut Context<Self>,
    ) {
        if let Some(runtime) = self.lifecycle_runtime.clone()
            && let Some(connection_id) = self.selected_connection_id_owned()
        {
            let failure_fallback = self
                .messages
                .clone()
                .map(|messages| messages.herdr_unknown_error)
                .unwrap_or_default();
            self.request_herdr_snapshot(connection_id, feedback, failure_fallback, runtime, cx);
        }
    }

    // ---- terminal dispatch ----

    fn dispatch_herdr_terminal(
        &mut self,
        connection_id: String,
        target: HerdrAttachTarget,
        title: String,
        opened_notice: String,
        missing_notice: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let command = match build_herdr_attach_command(&target) {
            Ok(command) => command,
            Err(_) => {
                cx.emit(HostToolsEvent::ShowNotice(
                    HostToolsNotice::HerdrActionFailed,
                ));
                return;
            }
        };
        // The builder is the only source of terminal commands for this action.
        window.dispatch_action(
            Box::new(HostToolsWindowRequest::new(
                HostToolsWindowIntent::OpenExistingNodeTerminal {
                    connection_id,
                    command,
                    title,
                    opened_notice,
                    missing_notice,
                },
            )),
            cx,
        );
    }

    // ---- panel ----

    fn herdr_panel_snapshot(&self) -> Option<HostHerdrPanelSnapshot> {
        let connections = self.monitor_connections();
        let fallback_connection_id = connections.first()?.connection_id.clone();
        let selected_connection_id = self
            .selected_connection_id_owned()
            .filter(|selected_id| {
                connections
                    .iter()
                    .any(|connection| connection.connection_id == *selected_id)
            })
            .unwrap_or(fallback_connection_id);
        let same_connection = self.host_herdr.snapshot_connection_id.as_deref()
            == Some(selected_connection_id.as_str());
        let snapshot = same_connection
            .then(|| self.host_herdr.snapshot.clone())
            .flatten();
        let last_refreshed_ms = same_connection
            .then_some(self.host_herdr.last_refreshed_ms)
            .flatten();

        Some(HostHerdrPanelSnapshot {
            connections,
            selected_connection_id,
            snapshot,
            last_refreshed_ms,
        })
    }

    fn render_host_herdr_panel(
        &self,
        search_ime: &HostToolsPlainTextImeFrame,
        terminal_available: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(panel) = self.herdr_panel_snapshot() else {
            return host_tools_center_state(
                LucideIcon::WifiOff,
                tokens.ui.text_muted,
                i18n.t("profiler.panel.no_connection"),
                selectable_text,
                cx,
            );
        };
        let selected_connection_id = panel.selected_connection_id.clone();
        let query = self.ui.host_herdr_search_query.trim().to_lowercase();
        let visible_sessions = panel
            .snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .sessions
                    .iter()
                    .filter(|session| {
                        herdr_session_matches_query(
                            session,
                            self.host_herdr.session_details.get(&session.name),
                            &query,
                        )
                    })
                    .count()
            })
            .unwrap_or(0);
        let header = div()
            .flex_none()
            .w_full()
            .min_w_0()
            .px_3()
            .pt_3()
            .pb_2()
            .flex()
            .flex_col()
            .gap_2()
            .border_b_1()
            .border_color(rgba((tokens.ui.border << 8) | MONITOR_BORDER_ALPHA))
            .child(self.render_connection_switcher(
                &panel.connections,
                &selected_connection_id,
                panel.snapshot.is_some(),
                tokens,
                mono_font_family.clone(),
                selectable_text,
                cx,
            ))
            .child(self.render_herdr_search(search_ime, tokens, i18n, cx))
            .child(self.render_herdr_status_row(
                visible_sessions,
                terminal_available,
                panel.last_refreshed_ms,
                tokens,
                i18n,
                cx,
            ));
        let body = self.render_herdr_body(
            panel.snapshot.as_ref(),
            &query,
            terminal_available,
            tokens,
            i18n,
            mono_font_family,
            selectable_text,
            cx,
        );

        div()
            .id("host-herdr-panel")
            .w_full()
            .min_w_0()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(header)
            .child(body)
            .into_any_element()
    }

    fn render_herdr_search(
        &self,
        ime: &HostToolsPlainTextImeFrame,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input = ime.input();
        let anchor_frame = ime.clone();
        text_input_anchor_probe(
            ime.anchor_id(),
            ime.text_input(
                tokens,
                TextInputView {
                    value: &self.ui.host_herdr_search_query,
                    placeholder: i18n.t("sidebar.host_herdr.search_placeholder"),
                    focused: self.ui.input_is_focused(input),
                    caret_visible: ime.caret_visible(),
                    secret: false,
                    selected_all: false,
                    selected_range: ime.selected_range(),
                    marked_text: ime.marked_text(),
                },
            )
            .h(px(34.0))
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |host_tools, event: &MouseDownEvent, window, cx| {
                    host_tools.ui.focus_input(input);
                    // Only pointer metadata crosses to the workspace IME coordinator.
                    window.dispatch_action(
                        Box::new(HostToolsWindowRequest::new(
                            HostToolsWindowIntent::BeginPlainTextImeSelection {
                                input,
                                event: event.clone(),
                            },
                        )),
                        cx,
                    );
                    cx.stop_propagation();
                }),
            ),
            move |anchor, _window, _cx| {
                anchor_frame.update_anchor(anchor);
            },
        )
        .into_any_element()
    }

    fn render_herdr_status_row(
        &self,
        visible_sessions: usize,
        terminal_available: bool,
        last_refreshed_ms: Option<u64>,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let left = match last_refreshed_ms {
            Some(ms) => format!(
                "{} {} · {} {}",
                visible_sessions,
                i18n.t("sidebar.host_herdr.sessions_title"),
                i18n.t("sidebar.host_herdr.last_refreshed"),
                format_herdr_clock(ms),
            ),
            None => format!(
                "{} {}",
                visible_sessions,
                i18n.t("sidebar.host_herdr.sessions_title")
            ),
        };
        let attach_disabled = !terminal_available;
        // Terminal-dispatch copy is resolved at render time; the Entity never
        // retains an i18n handle.
        let attach_title = i18n.t("sidebar.host_herdr.attach_title");
        let attach_opened = i18n.t("sidebar.host_herdr.toast.attach_opened");
        let attach_missing = i18n.t("sidebar.host_herdr.toast.connection_missing");
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .min_w_0()
            .text_size(px(11.0))
            .text_color(rgb(theme.text_muted))
            .child(div().flex_none().child(left))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(host_tools_tooltip_icon_button(
                        tokens,
                        LucideIcon::Plus,
                        13.0,
                        rgb(theme.text),
                        oxideterm_gpui_ui::button::IconButtonOptions {
                            size: 24.0,
                            disabled: self.herdr_action_running(),
                            has_background: true,
                            background: Some(rgb(theme.bg_hover)),
                            hover_background: Some(rgb(theme.bg_panel)),
                            idle_opacity: 1.0,
                            ..oxideterm_gpui_ui::button::IconButtonOptions::compact(24.0)
                        },
                        i18n.t("sidebar.host_herdr.actions.new_workspace"),
                        "host-herdr-new-workspace",
                        true,
                        cx.listener(|host_tools, _event, window, cx| {
                            let Some(session) = host_tools.herdr_action_session() else {
                                cx.emit(HostToolsEvent::ShowNotice(
                                    HostToolsNotice::HerdrConnectionMissing,
                                ));
                                return;
                            };
                            host_tools.open_herdr_input_dialog(
                                HostHerdrInputDialogKind::NewWorkspace,
                                &session,
                                "",
                                window,
                                cx,
                            );
                            cx.stop_propagation();
                        }),
                    ))
                    .child(host_tools_tooltip_icon_button(
                        tokens,
                        LucideIcon::Terminal,
                        13.0,
                        rgb(theme.text),
                        oxideterm_gpui_ui::button::IconButtonOptions {
                            size: 24.0,
                            disabled: attach_disabled,
                            has_background: true,
                            background: Some(rgb(theme.bg_hover)),
                            hover_background: Some(rgb(theme.bg_panel)),
                            idle_opacity: 1.0,
                            ..oxideterm_gpui_ui::button::IconButtonOptions::compact(24.0)
                        },
                        i18n.t("sidebar.host_herdr.actions.attach"),
                        "host-herdr-attach",
                        true,
                        cx.listener(move |host_tools, _event, window, cx| {
                            let Some(connection_id) = host_tools.selected_connection_id_owned()
                            else {
                                return;
                            };
                            host_tools.dispatch_herdr_terminal(
                                connection_id,
                                HerdrAttachTarget::Default,
                                attach_title.clone(),
                                attach_opened.clone(),
                                attach_missing.clone(),
                                window,
                                cx,
                            );
                            cx.stop_propagation();
                        }),
                    ))
                    .child(host_tools_tooltip_icon_button(
                        tokens,
                        LucideIcon::RefreshCw,
                        13.0,
                        rgb(theme.text),
                        oxideterm_gpui_ui::button::IconButtonOptions {
                            size: 24.0,
                            disabled: self.host_herdr.snapshot_in_flight,
                            has_background: true,
                            background: Some(rgb(theme.bg_hover)),
                            hover_background: Some(rgb(theme.bg_panel)),
                            idle_opacity: 1.0,
                            ..oxideterm_gpui_ui::button::IconButtonOptions::compact(24.0)
                        },
                        i18n.t("sidebar.host_herdr.actions.refresh"),
                        "host-herdr-refresh",
                        true,
                        cx.listener(|host_tools, _event, _window, cx| {
                            host_tools
                                .refresh_herdr_snapshot_from_view(HostSnapshotFeedback::Toast, cx);
                            cx.stop_propagation();
                        }),
                    )),
            )
            .into_any_element()
    }

    fn render_herdr_body(
        &self,
        snapshot: Option<&HerdrSnapshot>,
        query: &str,
        terminal_available: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let Some(snapshot) = snapshot else {
            return host_tools_center_state(
                LucideIcon::Bot,
                theme.text_muted,
                i18n.t("sidebar.host_herdr.sampling"),
                selectable_text,
                cx,
            );
        };
        match &snapshot.status {
            HerdrStatus::Unknown | HerdrStatus::Available => {}
            HerdrStatus::Unavailable => {
                return host_tools_center_state(
                    LucideIcon::Bot,
                    theme.text_muted,
                    i18n.t("sidebar.host_herdr.unavailable"),
                    selectable_text,
                    cx,
                );
            }
            HerdrStatus::ServerNotRunning => {
                return host_tools_center_state(
                    LucideIcon::Bot,
                    theme.text_muted,
                    i18n.t("sidebar.host_herdr.server_not_running"),
                    selectable_text,
                    cx,
                );
            }
            HerdrStatus::Error { message } => {
                return host_tools_center_state(
                    LucideIcon::AlertTriangle,
                    MONITOR_RED,
                    i18n.t("sidebar.host_herdr.error")
                        .replace("{{error}}", message),
                    selectable_text,
                    cx,
                );
            }
        }
        if snapshot.sessions.is_empty() {
            return host_tools_center_state(
                LucideIcon::Bot,
                theme.text_muted,
                i18n.t("sidebar.host_herdr.empty"),
                selectable_text,
                cx,
            );
        }
        let sessions: Vec<&HerdrSession> = snapshot
            .sessions
            .iter()
            .filter(|session| {
                herdr_session_matches_query(
                    session,
                    self.host_herdr.session_details.get(&session.name),
                    query,
                )
            })
            .collect();
        if sessions.is_empty() {
            // A query that hides every session must not read as "no sessions
            // exist", so the empty state names the search instead.
            let empty_key = if query.is_empty() {
                "sidebar.host_herdr.empty"
            } else {
                "sidebar.host_herdr.search_empty"
            };
            return host_tools_center_state(
                LucideIcon::Bot,
                theme.text_muted,
                i18n.t(empty_key),
                selectable_text,
                cx,
            );
        }

        let mut list = div()
            .id("host-herdr-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(self.render_herdr_sessions_header(tokens, i18n, cx));
        for session in sessions {
            list = list.child(self.render_herdr_session_row(
                session,
                query,
                terminal_available,
                tokens,
                i18n,
                mono_font_family.clone(),
                cx,
            ));
        }
        list.into_any_element()
    }

    fn render_herdr_sessions_header(
        &self,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        // Create-or-attach: `herdr --session <name>` starts a named session and
        // attaches to it when it already exists.
        let new_session = i18n.t("sidebar.host_herdr.actions.new_session");
        div()
            .w_full()
            .px_3()
            .pt_3()
            .pb_1()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .text_size(px(10.0))
                    .text_color(rgb(theme.text_muted))
                    .child(i18n.t("sidebar.host_herdr.sessions_title")),
            )
            .child(host_tools_tooltip_icon_button(
                tokens,
                LucideIcon::Plus,
                11.0,
                rgb(theme.text),
                oxideterm_gpui_ui::button::IconButtonOptions {
                    size: 18.0,
                    has_background: true,
                    background: Some(rgb(theme.bg_hover)),
                    hover_background: Some(rgb(theme.bg_panel)),
                    idle_opacity: 1.0,
                    ..oxideterm_gpui_ui::button::IconButtonOptions::compact(18.0)
                },
                new_session,
                "host-herdr-new-session",
                true,
                cx.listener(|host_tools, _event, window, cx| {
                    // A new session defines its own name; no context session.
                    host_tools.open_herdr_input_dialog(
                        HostHerdrInputDialogKind::NewSession,
                        "",
                        "",
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                }),
            ))
            .into_any_element()
    }

    fn render_herdr_session_row(
        &self,
        session: &HerdrSession,
        query: &str,
        terminal_available: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let detail = self.host_herdr.session_details.get(&session.name);
        // A search reveals the branches it matched without touching the
        // click-managed expansion state.
        let descendant_match = !query.is_empty()
            && detail.is_some_and(|detail| {
                detail
                    .workspaces
                    .iter()
                    .any(|workspace| herdr_workspace_matches_query(detail, workspace, query))
                    || detail
                        .agents
                        .iter()
                        .any(|agent| herdr_agent_matches_query(agent, query))
            });
        let expanded = self.host_herdr.expanded_session.as_deref() == Some(session.name.as_str())
            || descendant_match;
        let attach_name = session.name.clone();
        let stop_name = session.name.clone();
        let delete_name = session.name.clone();
        // Terminal-dispatch copy is resolved at render time; the Entity never
        // retains an i18n handle.
        let attach_title = i18n.t("sidebar.host_herdr.actions.session_attach");
        let attach_opened = i18n.t("sidebar.host_herdr.toast.attach_opened");
        let attach_missing = i18n.t("sidebar.host_herdr.toast.connection_missing");
        // `session attach` starts a stopped session, so attach stays enabled
        // while stop is disabled; only the name dims to read as inactive.
        let name_color = if session.running {
            theme.text
        } else {
            theme.text_muted
        };
        let header = div()
            .id(host_tools_row_id(
                "host-herdr-session",
                "",
                session.name.clone(),
            ))
            .w_full()
            .min_w_0()
            .border_b_1()
            .border_color(rgba((theme.border << 8) | MONITOR_BORDER_ALPHA))
            .cursor_pointer()
            .hover(|row| row.bg(rgb(theme.bg_hover)))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .h(px(34.0))
                    .pl(px(10.0))
                    .pr_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div().flex_none().child(
                            svg()
                                .path(if expanded {
                                    LucideIcon::ChevronDown.path()
                                } else {
                                    LucideIcon::ChevronRight.path()
                                })
                                .size(px(11.0))
                                .text_color(rgb(theme.text_muted)),
                        ),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(HOST_PROCESS_TABLE_COMMAND_TEXT_SIZE))
                            .text_color(rgb(name_color))
                            .child(session.name.clone()),
                    )
                    .child(host_tools_tooltip_icon_button(
                        tokens,
                        LucideIcon::Cable,
                        12.0,
                        rgb(theme.text),
                        oxideterm_gpui_ui::button::IconButtonOptions {
                            size: 22.0,
                            disabled: !terminal_available,
                            has_background: true,
                            background: Some(rgb(theme.bg_hover)),
                            hover_background: Some(rgb(theme.bg_panel)),
                            idle_opacity: 1.0,
                            ..oxideterm_gpui_ui::button::IconButtonOptions::compact(22.0)
                        },
                        attach_title.clone(),
                        "host-herdr-session-attach",
                        true,
                        cx.listener(move |host_tools, _event, window, cx| {
                            let Some(connection_id) = host_tools.selected_connection_id_owned()
                            else {
                                return;
                            };
                            host_tools.dispatch_herdr_terminal(
                                connection_id,
                                HerdrAttachTarget::Session {
                                    name: attach_name.clone(),
                                },
                                attach_title.clone(),
                                attach_opened.clone(),
                                attach_missing.clone(),
                                window,
                                cx,
                            );
                            cx.stop_propagation();
                        }),
                    ))
                    .child(host_tools_tooltip_icon_button(
                        tokens,
                        LucideIcon::X,
                        12.0,
                        rgb(MONITOR_RED),
                        oxideterm_gpui_ui::button::IconButtonOptions {
                            size: 22.0,
                            disabled: !session.running || self.herdr_action_running(),
                            has_background: true,
                            background: Some(rgba((MONITOR_RED << 8) | MONITOR_TINT_ALPHA)),
                            hover_background: Some(rgba((MONITOR_RED << 8) | 0x30)),
                            idle_opacity: 1.0,
                            ..oxideterm_gpui_ui::button::IconButtonOptions::compact(22.0)
                        },
                        i18n.t("sidebar.host_herdr.actions.session_stop"),
                        "host-herdr-session-stop",
                        true,
                        cx.listener(move |host_tools, _event, _window, cx| {
                            let Some(connection_id) = host_tools.selected_connection_id_owned()
                            else {
                                return;
                            };
                            if let Some(notice) = host_tools.open_herdr_action_confirm(
                                HostHerdrActionRequest {
                                    connection_id,
                                    session: stop_name.clone(),
                                    target_label: stop_name.clone(),
                                    action: HostHerdrDestructiveAction::StopSession {
                                        name: stop_name.clone(),
                                    },
                                },
                                cx,
                            ) {
                                cx.emit(HostToolsEvent::ShowNotice(notice));
                            }
                            cx.stop_propagation();
                        }),
                    ))
                    .child(host_tools_tooltip_icon_button(
                        tokens,
                        LucideIcon::Trash2,
                        12.0,
                        rgb(theme.text_muted),
                        oxideterm_gpui_ui::button::IconButtonOptions {
                            size: 22.0,
                            disabled: session.running || self.herdr_action_running(),
                            has_background: true,
                            background: Some(rgb(theme.bg_hover)),
                            hover_background: Some(rgb(theme.bg_panel)),
                            idle_opacity: 1.0,
                            ..oxideterm_gpui_ui::button::IconButtonOptions::compact(22.0)
                        },
                        i18n.t("sidebar.host_herdr.actions.session_delete"),
                        "host-herdr-session-delete",
                        true,
                        cx.listener(move |host_tools, _event, _window, cx| {
                            let Some(connection_id) = host_tools.selected_connection_id_owned()
                            else {
                                return;
                            };
                            if let Some(notice) = host_tools.open_herdr_action_confirm(
                                HostHerdrActionRequest {
                                    connection_id,
                                    session: delete_name.clone(),
                                    target_label: delete_name.clone(),
                                    action: HostHerdrDestructiveAction::DeleteSession {
                                        name: delete_name.clone(),
                                    },
                                },
                                cx,
                            ) {
                                cx.emit(HostToolsEvent::ShowNotice(notice));
                            }
                            cx.stop_propagation();
                        }),
                    )),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener({
                    let session_name = session.name.clone();
                    move |host_tools, _event, _window, cx| {
                        host_tools.toggle_herdr_session(&session_name, cx);
                        cx.stop_propagation();
                    }
                }),
            );

        let mut container = div().w_full().min_w_0().flex().flex_col().child(header);
        if expanded {
            container = container.children(self.render_herdr_session_children(
                session,
                query,
                tokens,
                i18n,
                mono_font_family,
                cx,
            ));
        }
        container.into_any_element()
    }

    fn render_herdr_session_children(
        &self,
        session: &HerdrSession,
        query: &str,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = tokens.ui;
        let muted_child = |text: String| {
            div()
                .w_full()
                .min_w_0()
                .pl(px(45.0))
                .pr_3()
                .py(px(6.0))
                .text_size(px(HOST_PROCESS_TABLE_VALUE_TEXT_SIZE))
                .text_color(rgb(theme.text_muted))
                .child(text)
                .into_any_element()
        };
        let Some(detail) = self.host_herdr.session_details.get(&session.name) else {
            // Expanding requests the subtree, and a refresh re-requests the
            // session that is still open.
            return vec![muted_child(i18n.t("sidebar.host_herdr.sampling"))];
        };
        match &detail.status {
            HerdrStatus::Unknown | HerdrStatus::Available => {}
            HerdrStatus::Unavailable => {
                return vec![muted_child(i18n.t("sidebar.host_herdr.unavailable"))];
            }
            HerdrStatus::ServerNotRunning => {
                return vec![muted_child(i18n.t("sidebar.host_herdr.server_not_running"))];
            }
            HerdrStatus::Error { message } => {
                return vec![muted_child(
                    i18n.t("sidebar.host_herdr.error")
                        .replace("{{error}}", message),
                )];
            }
        }
        let orphan_agents: Vec<_> = detail
            .orphan_agents()
            .into_iter()
            .filter(|agent| herdr_agent_matches_query(agent, query))
            .collect();
        let workspaces: Vec<&HerdrWorkspace> = detail
            .workspaces
            .iter()
            .filter(|workspace| herdr_workspace_matches_query(detail, workspace, query))
            .collect();
        if workspaces.is_empty() && orphan_agents.is_empty() {
            // Nothing matched below a session the query only reached by name.
            return if query.is_empty() {
                vec![muted_child(i18n.t("sidebar.host_herdr.workspace_empty"))]
            } else {
                Vec::new()
            };
        }
        let mut rows = Vec::new();
        for workspace in workspaces {
            rows.push(self.render_herdr_workspace_row(
                &session.name,
                workspace,
                query,
                detail,
                tokens,
                i18n,
                mono_font_family.clone(),
                cx,
            ));
        }
        for (index, agent) in orphan_agents.iter().enumerate() {
            let label = agent.label.clone().unwrap_or_else(|| agent.pane_id.clone());
            rows.push(self.render_herdr_agent_chip_row(
                &session.name,
                &label,
                agent.status.as_deref().unwrap_or("unknown"),
                agent.pane_id.clone(),
                index,
                tokens,
                i18n,
                cx,
            ));
        }
        rows
    }

    fn render_herdr_workspace_row(
        &self,
        session: &str,
        workspace: &HerdrWorkspace,
        query: &str,
        detail: &HerdrSessionDetail,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let descendant_match = !query.is_empty()
            && detail
                .tabs_for_workspace(&workspace.workspace_id)
                .iter()
                .any(|tab| herdr_tab_matches_query(detail, tab, query));
        let expanded = self.ui.host_herdr_expanded_workspace.as_deref()
            == Some(workspace.workspace_id.as_str())
            || descendant_match;
        let workspace_label = workspace.label.clone();
        let workspace_id = workspace.workspace_id.clone();
        let header = div()
            .id(host_tools_row_id(
                "host-herdr-workspace",
                "",
                workspace.workspace_id.clone(),
            ))
            .w_full()
            .min_w_0()
            .border_b_1()
            .border_color(rgba((theme.border << 8) | MONITOR_BORDER_ALPHA))
            .cursor_pointer()
            .hover(|row| row.bg(rgb(theme.bg_hover)))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .h(px(36.0))
                    .pl(px(26.0))
                    .pr_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div().flex_none().child(
                            svg()
                                .path(if expanded {
                                    LucideIcon::ChevronDown.path()
                                } else {
                                    LucideIcon::ChevronRight.path()
                                })
                                .size(px(11.0))
                                .text_color(rgb(theme.text_muted)),
                        ),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(HOST_PROCESS_TABLE_COMMAND_TEXT_SIZE))
                            .text_color(rgb(theme.text))
                            .child(workspace.label.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(10.0))
                            .text_color(rgb(theme.text_muted))
                            .child(i18n.t("sidebar.host_herdr.row_space")),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(HOST_PROCESS_TABLE_VALUE_TEXT_SIZE))
                            .text_color(rgb(theme.text_muted))
                            .child(herdr_count_label(
                                workspace.tab_count,
                                &i18n.t("sidebar.host_herdr.tabs_suffix"),
                            )),
                    )
                    .when_some(
                        self.render_herdr_status_chip(
                            workspace.agent_status.as_deref().unwrap_or("unknown"),
                            i18n,
                        ),
                        |row, chip| row.child(chip),
                    )
                    .when(workspace.focused, |row| {
                        row.child(
                            div()
                                .flex_none()
                                .text_size(px(10.0))
                                .text_color(rgb(theme.accent))
                                .child(i18n.t("sidebar.host_herdr.focused")),
                        )
                    })
                    .child(self.render_herdr_workspace_actions(
                        session,
                        &workspace_id,
                        &workspace_label,
                        tokens,
                        i18n,
                        cx,
                    )),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener({
                    let workspace_id = workspace.workspace_id.clone();
                    move |host_tools, _event, _window, cx| {
                        let expanded_id = &mut host_tools.ui.host_herdr_expanded_workspace;
                        if expanded_id.as_deref() == Some(workspace_id.as_str()) {
                            *expanded_id = None;
                        } else {
                            *expanded_id = Some(workspace_id.clone());
                        }
                        // A tab belongs to one workspace; keep the tree honest.
                        host_tools.ui.host_herdr_expanded_tab = None;
                        cx.notify();
                        cx.stop_propagation();
                    }
                }),
            );

        let mut container = div().w_full().min_w_0().flex().flex_col().child(header);
        if expanded {
            for tab in detail.tabs_for_workspace(&workspace.workspace_id) {
                if !herdr_tab_matches_query(detail, tab, query) {
                    continue;
                }
                container = container.child(self.render_herdr_tab_row(
                    session,
                    tab,
                    query,
                    detail,
                    tokens,
                    i18n,
                    mono_font_family.clone(),
                    cx,
                ));
            }
        }
        container.into_any_element()
    }

    fn render_herdr_tab_row(
        &self,
        session: &str,
        tab: &HerdrTab,
        query: &str,
        detail: &HerdrSessionDetail,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let descendant_match = !query.is_empty()
            && detail
                .panes_for_tab(&tab.tab_id)
                .iter()
                .any(|pane| herdr_pane_matches_query(pane, query));
        let expanded = self.ui.host_herdr_expanded_tab.as_deref() == Some(tab.tab_id.as_str())
            || descendant_match;
        let label = tab
            .label
            .clone()
            .unwrap_or_else(|| format!("#{}", tab.number));
        let rename_id = tab.tab_id.clone();
        let rename_label = label.clone();
        let rename_session = session.to_string();
        let close_id = tab.tab_id.clone();
        let close_label = label.clone();
        let close_session = session.to_string();
        let header = div()
            .id(host_tools_row_id("host-herdr-tab", "", tab.tab_id.clone()))
            .w_full()
            .min_w_0()
            .border_b_1()
            .border_color(rgba((theme.border << 8) | MONITOR_BORDER_ALPHA))
            .cursor_pointer()
            .hover(|row| row.bg(rgb(theme.bg_hover)))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .h(px(32.0))
                    .pl(px(42.0))
                    .pr_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div().flex_none().child(
                            svg()
                                .path(if expanded {
                                    LucideIcon::ChevronDown.path()
                                } else {
                                    LucideIcon::ChevronRight.path()
                                })
                                .size(px(11.0))
                                .text_color(rgb(theme.text_muted)),
                        ),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(HOST_PROCESS_TABLE_COMMAND_TEXT_SIZE))
                            .text_color(rgb(theme.text))
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(10.0))
                            .text_color(rgb(theme.text_muted))
                            .child(i18n.t("sidebar.host_herdr.row_tab")),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(HOST_PROCESS_TABLE_VALUE_TEXT_SIZE))
                            .text_color(rgb(theme.text_muted))
                            .child(herdr_count_label(
                                tab.pane_count,
                                &i18n.t("sidebar.host_herdr.panes_suffix"),
                            )),
                    )
                    .when_some(
                        self.render_herdr_status_chip(
                            tab.agent_status.as_deref().unwrap_or("unknown"),
                            i18n,
                        ),
                        |row, chip| row.child(chip),
                    )
                    .when(tab.focused, |row| {
                        row.child(
                            div()
                                .flex_none()
                                .text_size(px(10.0))
                                .text_color(rgb(theme.accent))
                                .child(i18n.t("sidebar.host_herdr.focused")),
                        )
                    })
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .child(host_tools_tooltip_icon_button(
                                tokens,
                                LucideIcon::Pencil,
                                13.0,
                                rgb(theme.text),
                                oxideterm_gpui_ui::button::IconButtonOptions {
                                    size: 22.0,
                                    disabled: self.herdr_action_running(),
                                    has_background: true,
                                    background: Some(rgb(theme.bg_hover)),
                                    hover_background: Some(rgb(theme.bg_panel)),
                                    idle_opacity: 1.0,
                                    ..oxideterm_gpui_ui::button::IconButtonOptions::compact(22.0)
                                },
                                i18n.t("sidebar.host_herdr.actions.rename"),
                                "host-herdr-tab-rename",
                                true,
                                cx.listener(move |host_tools, _event, window, cx| {
                                    host_tools.open_herdr_input_dialog(
                                        HostHerdrInputDialogKind::RenameTab {
                                            tab_id: rename_id.clone(),
                                        },
                                        &rename_session,
                                        &rename_label,
                                        window,
                                        cx,
                                    );
                                    cx.stop_propagation();
                                }),
                            ))
                            .child(host_tools_tooltip_icon_button(
                                tokens,
                                LucideIcon::X,
                                13.0,
                                rgb(MONITOR_RED),
                                oxideterm_gpui_ui::button::IconButtonOptions {
                                    size: 22.0,
                                    disabled: self.herdr_action_running(),
                                    has_background: true,
                                    background: Some(rgba((MONITOR_RED << 8) | MONITOR_TINT_ALPHA)),
                                    hover_background: Some(rgba((MONITOR_RED << 8) | 0x30)),
                                    idle_opacity: 1.0,
                                    ..oxideterm_gpui_ui::button::IconButtonOptions::compact(22.0)
                                },
                                i18n.t("sidebar.host_herdr.actions.close"),
                                "host-herdr-tab-close",
                                true,
                                cx.listener(move |host_tools, _event, _window, cx| {
                                    let Some(connection_id) =
                                        host_tools.selected_connection_id_owned()
                                    else {
                                        return;
                                    };
                                    if let Some(notice) = host_tools.open_herdr_action_confirm(
                                        HostHerdrActionRequest {
                                            connection_id,
                                            session: close_session.clone(),
                                            target_label: close_label.clone(),
                                            action: HostHerdrDestructiveAction::CloseTab {
                                                tab_id: close_id.clone(),
                                            },
                                        },
                                        cx,
                                    ) {
                                        cx.emit(HostToolsEvent::ShowNotice(notice));
                                    }
                                    cx.stop_propagation();
                                }),
                            )),
                    ),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener({
                    let tab_id = tab.tab_id.clone();
                    move |host_tools, _event, _window, cx| {
                        let expanded_id = &mut host_tools.ui.host_herdr_expanded_tab;
                        if expanded_id.as_deref() == Some(tab_id.as_str()) {
                            *expanded_id = None;
                        } else {
                            *expanded_id = Some(tab_id.clone());
                        }
                        cx.notify();
                        cx.stop_propagation();
                    }
                }),
            );

        let mut container = div().w_full().min_w_0().flex().flex_col().child(header);
        if expanded {
            for pane in detail.panes_for_tab(&tab.tab_id) {
                if !herdr_pane_matches_query(pane, query) {
                    continue;
                }
                container = container.child(self.render_herdr_pane_row(
                    session,
                    pane,
                    detail,
                    tokens,
                    i18n,
                    mono_font_family.clone(),
                    cx,
                ));
            }
        }
        container.into_any_element()
    }

    fn render_herdr_workspace_actions(
        &self,
        session: &str,
        workspace_id: &str,
        workspace_label: &str,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let rename_id = workspace_id.to_string();
        let rename_label = workspace_label.to_string();
        let rename_session = session.to_string();
        let close_id = workspace_id.to_string();
        let close_label = workspace_label.to_string();
        let close_session = session.to_string();
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.0))
            .child(host_tools_tooltip_icon_button(
                tokens,
                LucideIcon::Pencil,
                13.0,
                rgb(theme.text),
                oxideterm_gpui_ui::button::IconButtonOptions {
                    size: 22.0,
                    disabled: self.herdr_action_running(),
                    has_background: true,
                    background: Some(rgb(theme.bg_hover)),
                    hover_background: Some(rgb(theme.bg_panel)),
                    idle_opacity: 1.0,
                    ..oxideterm_gpui_ui::button::IconButtonOptions::compact(22.0)
                },
                i18n.t("sidebar.host_herdr.actions.rename"),
                "host-herdr-workspace-rename",
                true,
                cx.listener(move |host_tools, _event, window, cx| {
                    host_tools.open_herdr_input_dialog(
                        HostHerdrInputDialogKind::RenameWorkspace {
                            workspace_id: rename_id.clone(),
                        },
                        &rename_session,
                        &rename_label,
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                }),
            ))
            .child(host_tools_tooltip_icon_button(
                tokens,
                LucideIcon::X,
                13.0,
                rgb(MONITOR_RED),
                oxideterm_gpui_ui::button::IconButtonOptions {
                    size: 22.0,
                    disabled: self.herdr_action_running(),
                    has_background: true,
                    background: Some(rgba((MONITOR_RED << 8) | MONITOR_TINT_ALPHA)),
                    hover_background: Some(rgba((MONITOR_RED << 8) | 0x30)),
                    idle_opacity: 1.0,
                    ..oxideterm_gpui_ui::button::IconButtonOptions::compact(22.0)
                },
                i18n.t("sidebar.host_herdr.actions.close"),
                "host-herdr-workspace-close",
                true,
                cx.listener(move |host_tools, _event, _window, cx| {
                    let Some(connection_id) = host_tools.selected_connection_id_owned() else {
                        return;
                    };
                    if let Some(notice) = host_tools.open_herdr_action_confirm(
                        HostHerdrActionRequest {
                            connection_id,
                            session: close_session.clone(),
                            target_label: close_label.clone(),
                            action: HostHerdrDestructiveAction::CloseWorkspace {
                                workspace_id: close_id.clone(),
                            },
                        },
                        cx,
                    ) {
                        cx.emit(HostToolsEvent::ShowNotice(notice));
                    }
                    cx.stop_propagation();
                }),
            ))
            .into_any_element()
    }

    fn render_herdr_pane_row(
        &self,
        session: &str,
        pane: &HerdrPane,
        detail: &HerdrSessionDetail,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let agent = detail.agent_for_pane(&pane.pane_id);
        // `pane rename` writes `label`; the OSC terminal title is only a
        // fallback because shells overwrite it with their own prompt text.
        let pane_label = pane
            .label
            .clone()
            .or_else(|| pane.terminal_title.clone())
            .or_else(|| agent.and_then(|agent| agent.label.clone()))
            .unwrap_or_else(|| pane.cwd.clone().unwrap_or_else(|| pane.pane_id.clone()));
        // The cwd stays visible as a hint unless it already is the label.
        let cwd_hint = pane
            .cwd
            .clone()
            .filter(|cwd| cwd.as_str() != pane_label.as_str());
        let status = agent
            .and_then(|agent| agent.status.clone())
            .or_else(|| pane.agent_status.clone())
            .unwrap_or_else(|| "unknown".to_string());
        let restore_warning = pane.restore_failed.then(|| {
            div()
                .flex_none()
                .text_size(px(10.0))
                .text_color(rgb(MONITOR_RED))
                .child(i18n.t("sidebar.host_herdr.restore_error"))
        });
        let read_id = pane.pane_id.clone();
        let read_label = pane_label.clone();
        let read_session = session.to_string();
        let run_id = pane.pane_id.clone();
        let run_session = session.to_string();
        let rename_id = pane.pane_id.clone();
        // Prefill only the explicit name so confirming never renames the pane
        // to a shell-generated terminal title.
        let rename_prefill = pane.label.clone().unwrap_or_default();
        let rename_session = session.to_string();
        let close_id = pane.pane_id.clone();
        let close_label = pane_label.clone();
        let close_session = session.to_string();

        div()
            .id(host_tools_row_id(
                "host-herdr-pane",
                "",
                pane.pane_id.clone(),
            ))
            .w_full()
            .min_w_0()
            .border_b_1()
            .border_color(rgba((theme.border << 8) | MONITOR_BORDER_ALPHA))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .pl(px(68.0))
                    .pr_3()
                    .py(px(6.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(HOST_PROCESS_TABLE_COMMAND_TEXT_SIZE))
                            .text_color(rgb(theme.text))
                            .font_family(mono_font_family)
                            .child(pane_label),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(10.0))
                            .text_color(rgb(theme.text_muted))
                            .child(i18n.t("sidebar.host_herdr.row_pane")),
                    )
                    .when_some(cwd_hint, |row, cwd| {
                        row.child(
                            div()
                                .flex_none()
                                .min_w_0()
                                .max_w(px(220.0))
                                .truncate()
                                .text_size(px(10.0))
                                .text_color(rgb(theme.text_muted))
                                .child(cwd),
                        )
                    })
                    .children(restore_warning)
                    .when_some(self.render_herdr_status_chip(&status, i18n), |row, chip| {
                        row.child(chip)
                    })
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .child(host_tools_tooltip_icon_button(
                                tokens,
                                LucideIcon::FileText,
                                12.0,
                                rgb(theme.text),
                                oxideterm_gpui_ui::button::IconButtonOptions {
                                    size: 22.0,
                                    has_background: true,
                                    background: Some(rgb(theme.bg_hover)),
                                    hover_background: Some(rgb(theme.bg_panel)),
                                    idle_opacity: 1.0,
                                    ..oxideterm_gpui_ui::button::IconButtonOptions::compact(22.0)
                                },
                                i18n.t("sidebar.host_herdr.actions.read"),
                                "host-herdr-pane-read",
                                true,
                                cx.listener(move |host_tools, _event, _window, cx| {
                                    host_tools.request_herdr_read(
                                        read_session.clone(),
                                        read_id.clone(),
                                        read_label.clone(),
                                        cx,
                                    );
                                    cx.stop_propagation();
                                }),
                            ))
                            .child(host_tools_tooltip_icon_button(
                                tokens,
                                LucideIcon::Play,
                                12.0,
                                rgb(theme.text),
                                oxideterm_gpui_ui::button::IconButtonOptions {
                                    size: 22.0,
                                    disabled: self.herdr_action_running(),
                                    has_background: true,
                                    background: Some(rgb(theme.bg_hover)),
                                    hover_background: Some(rgb(theme.bg_panel)),
                                    idle_opacity: 1.0,
                                    ..oxideterm_gpui_ui::button::IconButtonOptions::compact(22.0)
                                },
                                i18n.t("sidebar.host_herdr.actions.send"),
                                "host-herdr-pane-run",
                                true,
                                cx.listener(move |host_tools, _event, window, cx| {
                                    host_tools.open_herdr_input_dialog(
                                        HostHerdrInputDialogKind::RunPane {
                                            pane_id: run_id.clone(),
                                        },
                                        &run_session,
                                        "",
                                        window,
                                        cx,
                                    );
                                    cx.stop_propagation();
                                }),
                            ))
                            .child(host_tools_tooltip_icon_button(
                                tokens,
                                LucideIcon::Pencil,
                                12.0,
                                rgb(theme.text),
                                oxideterm_gpui_ui::button::IconButtonOptions {
                                    size: 22.0,
                                    disabled: self.herdr_action_running(),
                                    has_background: true,
                                    background: Some(rgb(theme.bg_hover)),
                                    hover_background: Some(rgb(theme.bg_panel)),
                                    idle_opacity: 1.0,
                                    ..oxideterm_gpui_ui::button::IconButtonOptions::compact(22.0)
                                },
                                i18n.t("sidebar.host_herdr.actions.rename"),
                                "host-herdr-pane-rename",
                                true,
                                cx.listener(move |host_tools, _event, window, cx| {
                                    host_tools.open_herdr_input_dialog(
                                        HostHerdrInputDialogKind::RenamePane {
                                            pane_id: rename_id.clone(),
                                        },
                                        &rename_session,
                                        &rename_prefill,
                                        window,
                                        cx,
                                    );
                                    cx.stop_propagation();
                                }),
                            ))
                            .child(host_tools_tooltip_icon_button(
                                tokens,
                                LucideIcon::X,
                                12.0,
                                rgb(MONITOR_RED),
                                oxideterm_gpui_ui::button::IconButtonOptions {
                                    size: 22.0,
                                    disabled: self.herdr_action_running(),
                                    has_background: true,
                                    background: Some(rgba((MONITOR_RED << 8) | MONITOR_TINT_ALPHA)),
                                    hover_background: Some(rgba((MONITOR_RED << 8) | 0x30)),
                                    idle_opacity: 1.0,
                                    ..oxideterm_gpui_ui::button::IconButtonOptions::compact(22.0)
                                },
                                i18n.t("sidebar.host_herdr.actions.close"),
                                "host-herdr-pane-close",
                                true,
                                cx.listener(move |host_tools, _event, _window, cx| {
                                    let Some(connection_id) =
                                        host_tools.selected_connection_id_owned()
                                    else {
                                        return;
                                    };
                                    if let Some(notice) = host_tools.open_herdr_action_confirm(
                                        HostHerdrActionRequest {
                                            connection_id,
                                            session: close_session.clone(),
                                            target_label: close_label.clone(),
                                            action: HostHerdrDestructiveAction::ClosePane {
                                                pane_id: close_id.clone(),
                                            },
                                        },
                                        cx,
                                    ) {
                                        cx.emit(HostToolsEvent::ShowNotice(notice));
                                    }
                                    cx.stop_propagation();
                                }),
                            )),
                    ),
            )
            .into_any_element()
    }

    fn render_herdr_agent_chip_row(
        &self,
        session: &str,
        label: &str,
        status: &str,
        pane_id: String,
        row_index: usize,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let attach_pane = pane_id.clone();
        let attach_session = session.to_string();
        // Terminal-dispatch copy is resolved at render time; the Entity never
        // retains an i18n handle.
        let attach_title = i18n.t("sidebar.host_herdr.attach_title");
        let attach_opened = i18n.t("sidebar.host_herdr.toast.attach_opened");
        let attach_missing = i18n.t("sidebar.host_herdr.toast.connection_missing");
        div()
            .id(host_tools_row_id(
                "host-herdr-orphan-agent",
                "",
                format!("{pane_id}#{row_index}"),
            ))
            .w_full()
            .min_w_0()
            .border_b_1()
            .border_color(rgba((theme.border << 8) | MONITOR_BORDER_ALPHA))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .pl(px(68.0))
                    .pr_3()
                    .py(px(6.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(HOST_PROCESS_TABLE_COMMAND_TEXT_SIZE))
                            .text_color(rgb(theme.text))
                            .child(label.to_string()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(10.0))
                            .text_color(rgb(theme.text_muted))
                            .child(i18n.t("sidebar.host_herdr.orphan_agent")),
                    )
                    .when_some(self.render_herdr_status_chip(status, i18n), |row, chip| {
                        row.child(chip)
                    })
                    .child(host_tools_tooltip_icon_button(
                        tokens,
                        LucideIcon::Cable,
                        12.0,
                        rgb(theme.text),
                        oxideterm_gpui_ui::button::IconButtonOptions {
                            size: 22.0,
                            has_background: true,
                            background: Some(rgb(theme.bg_hover)),
                            hover_background: Some(rgb(theme.bg_panel)),
                            idle_opacity: 1.0,
                            ..oxideterm_gpui_ui::button::IconButtonOptions::compact(22.0)
                        },
                        i18n.t("sidebar.host_herdr.actions.agent_attach"),
                        "host-herdr-agent-attach",
                        true,
                        cx.listener(move |host_tools, _event, window, cx| {
                            let Some(connection_id) = host_tools.selected_connection_id_owned()
                            else {
                                return;
                            };
                            host_tools.dispatch_herdr_terminal(
                                connection_id,
                                HerdrAttachTarget::Agent {
                                    session: attach_session.clone(),
                                    pane_id: attach_pane.clone(),
                                },
                                attach_title.clone(),
                                attach_opened.clone(),
                                attach_missing.clone(),
                                window,
                                cx,
                            );
                            cx.stop_propagation();
                        }),
                    )),
            )
            .into_any_element()
    }

    /// A chip exists only for a reported agent state; "unknown" is the
    /// absence of one and would otherwise repeat on every row.
    fn render_herdr_status_chip(&self, status: &str, i18n: &I18n) -> Option<Div> {
        let color = match status {
            "working" => MONITOR_EMERALD,
            "blocked" => MONITOR_RED,
            "idle" | "done" => MONITOR_AMBER,
            _ => return None,
        };
        Some(
            div()
                .flex_none()
                .text_size(px(10.0))
                .text_color(rgb(color))
                .child(i18n.t(herdr_status_label_key(status))),
        )
    }

    fn render_herdr_confirm_view(
        &self,
        tokens: &ThemeTokens,
        i18n: &I18n,
        focused_action: Option<ConfirmDialogAction>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (request, phase) = self.herdr_confirm_view()?;
        let description = i18n
            .t(herdr_confirm_description_key(&request.action))
            .replace("{{name}}", &request.target_label);
        let exit_delay = oxideterm_gpui_ui::motion::duration(
            tokens,
            oxideterm_gpui_ui::motion::MotionDuration::Control,
        );
        let confirm_delay = exit_delay;
        Some(
            oxideterm_gpui_ui::confirm::confirm_dialog_with_focus_motion(
                tokens,
                "host-herdr-confirm-motion",
                phase,
                ConfirmDialogView {
                    variant: ConfirmDialogVariant::Danger,
                    title: div()
                        .child(i18n.t("sidebar.host_herdr.confirm.title"))
                        .into_any_element(),
                    description: Some(div().child(description).into_any_element()),
                    cancel_label: div()
                        .child(i18n.t("sidebar.host_herdr.confirm.cancel"))
                        .into_any_element(),
                    confirm_label: div()
                        .child(i18n.t("sidebar.host_herdr.confirm.run"))
                        .into_any_element(),
                },
                focused_action,
                cx.listener(move |host_tools, _event, _window, cx| {
                    // The global keyboard-focus marker is transient render
                    // input; the confirmation lifecycle belongs to Host Tools.
                    host_tools.begin_herdr_confirm_exit(exit_delay, cx);
                }),
                cx.listener(move |host_tools, _event, _window, cx| {
                    host_tools.confirm_herdr_action_from_view(confirm_delay, cx);
                }),
            )
            .into_any_element(),
        )
    }
}

fn herdr_confirm_description_key(action: &HostHerdrDestructiveAction) -> &'static str {
    match action {
        HostHerdrDestructiveAction::CloseWorkspace { .. } => {
            "sidebar.host_herdr.confirm.close_workspace"
        }
        HostHerdrDestructiveAction::ClosePane { .. } => "sidebar.host_herdr.confirm.close_pane",
        HostHerdrDestructiveAction::CloseTab { .. } => "sidebar.host_herdr.confirm.close_tab",
        HostHerdrDestructiveAction::StopSession { .. } => "sidebar.host_herdr.confirm.stop_session",
        HostHerdrDestructiveAction::DeleteSession { .. } => {
            "sidebar.host_herdr.confirm.delete_session"
        }
    }
}

fn herdr_input_title_key(kind: &HostHerdrInputDialogKind) -> &'static str {
    match kind {
        HostHerdrInputDialogKind::NewSession => "sidebar.host_herdr.inputs.new_session",
        HostHerdrInputDialogKind::NewWorkspace => "sidebar.host_herdr.inputs.new_workspace",
        HostHerdrInputDialogKind::RenameWorkspace { .. } => {
            "sidebar.host_herdr.inputs.rename_workspace"
        }
        HostHerdrInputDialogKind::RenameTab { .. } => "sidebar.host_herdr.inputs.rename_tab",
        HostHerdrInputDialogKind::RenamePane { .. } => "sidebar.host_herdr.inputs.rename_pane",
        HostHerdrInputDialogKind::RunPane { .. } => "sidebar.host_herdr.inputs.run_pane",
    }
}

fn herdr_input_placeholder_key(kind: &HostHerdrInputDialogKind) -> &'static str {
    match kind {
        HostHerdrInputDialogKind::NewSession => "sidebar.host_herdr.inputs.new_session_hint",
        HostHerdrInputDialogKind::NewWorkspace => "sidebar.host_herdr.inputs.new_workspace_hint",
        HostHerdrInputDialogKind::RenameWorkspace { .. } => "sidebar.host_herdr.inputs.rename_hint",
        HostHerdrInputDialogKind::RenameTab { .. } => "sidebar.host_herdr.inputs.rename_hint",
        HostHerdrInputDialogKind::RenamePane { .. } => "sidebar.host_herdr.inputs.rename_hint",
        HostHerdrInputDialogKind::RunPane { .. } => "sidebar.host_herdr.inputs.run_pane_hint",
    }
}
