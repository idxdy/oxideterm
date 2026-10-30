//! Owns the eCapture Host Tool UI and request lifecycle.
//!
//! eCapture runs detached on the Linux host and writes its TLS plaintext event
//! stream to a per-connection file. This module only schedules bounded probe,
//! poll, start, and stop commands over the SSH exec channel, so the tool keeps
//! working on hosts where only TCP/22 is reachable and forwarding is disabled.

use super::*;

use oxideterm_gpui_ui::badge::{StatusPillOptions, StatusTone, status_pill};
use oxideterm_gpui_ui::button::{
    ButtonOptions, ButtonRadius, ButtonSize, ButtonVariant, button_with,
};
use oxideterm_gpui_ui::confirm::{ConfirmDialogVariant, ConfirmDialogView};

impl WorkspaceApp {
    pub(super) fn render_host_ecapture_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens;
        let i18n = &self.i18n;
        let mono_font_family = settings_mono_font_family(self.settings_store.settings());
        let selectable_text = self.selectable_text_render_state(cx);
        let search_ime = self
            .host_tools_plain_text_ime_frame(HostToolsTextInput::EcaptureSearch, cx)
            .expect("eCapture search is a non-secret Host Tools input");
        let target_ime = self
            .host_tools_plain_text_ime_frame(HostToolsTextInput::EcaptureTarget, cx)
            .expect("eCapture target is a non-secret Host Tools input");
        let pid_ime = self
            .host_tools_plain_text_ime_frame(HostToolsTextInput::EcapturePid, cx)
            .expect("eCapture PID is a non-secret Host Tools input");
        let sidebar_width = self.ai_entity.read(cx).chat_ui().sidebar_width;
        self.host_tools.update(cx, |host_tools, cx| {
            host_tools.render_host_ecapture_panel(
                search_ime,
                target_ime,
                pid_ime,
                sidebar_width,
                &tokens,
                i18n,
                mono_font_family,
                &selectable_text,
                cx,
            )
        })
    }

    pub(in crate::workspace) fn handle_host_ecapture_key(
        &mut self,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let focused = self.host_tools.read(cx).ui.focused_input;
        if !matches!(
            focused,
            Some(
                HostToolsTextInput::EcaptureSearch
                    | HostToolsTextInput::EcaptureTarget
                    | HostToolsTextInput::EcapturePid
            )
        ) {
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

    pub(in crate::workspace) fn handle_host_ecapture_confirm_key(
        &mut self,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.host_tools.read(cx).ecapture_confirm_view().is_none() {
            return false;
        }
        match self.handle_standard_confirm_key(event, cx) {
            Some(ConfirmKeyboardAction::Cancel) => {
                self.begin_host_ecapture_confirm_exit(cx);
                true
            }
            Some(ConfirmKeyboardAction::Confirm) => {
                self.confirm_host_ecapture_action(cx);
                true
            }
            Some(ConfirmKeyboardAction::Handled) => true,
            None => false,
        }
    }

    pub(super) fn confirm_host_ecapture_action(&mut self, cx: &mut Context<Self>) {
        self.clear_standard_confirm_focus();
        let delay = oxideterm_gpui_ui::motion::duration(
            &self.tokens,
            oxideterm_gpui_ui::motion::MotionDuration::Control,
        );
        self.host_tools.update(cx, |host_tools, cx| {
            host_tools.confirm_ecapture_action_from_view(delay, cx);
        });
    }

    /// Keeps the request mounted until the current exit generation completes.
    fn begin_host_ecapture_confirm_exit(&mut self, cx: &mut Context<Self>) -> bool {
        self.clear_standard_confirm_focus();
        let delay = oxideterm_gpui_ui::motion::duration(
            &self.tokens,
            oxideterm_gpui_ui::motion::MotionDuration::Control,
        );
        self.host_tools.update(cx, |host_tools, cx| {
            host_tools.begin_ecapture_confirm_exit(delay, cx)
        })
    }

    pub(in crate::workspace) fn render_host_ecapture_confirm_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let mono_font_family = settings_mono_font_family(self.settings_store.settings());
        let (phase, view) = self.host_tools.read(cx).render_host_ecapture_confirm_view(
            &self.i18n,
            &self.tokens,
            mono_font_family,
        )?;
        Some(
            oxideterm_gpui_ui::confirm::confirm_dialog_with_focus_motion(
                &self.tokens,
                "host-ecapture-confirm-motion",
                phase,
                view,
                self.standard_confirm_focus(),
                cx.listener(|this, _event, _window, cx| {
                    this.begin_host_ecapture_confirm_exit(cx);
                }),
                cx.listener(|this, _event, _window, cx| {
                    this.confirm_host_ecapture_action(cx);
                }),
            )
            .into_any_element(),
        )
    }
}

impl HostToolsEntity {
    #[allow(clippy::too_many_arguments)]
    fn render_host_ecapture_panel(
        &self,
        search_ime: HostToolsPlainTextImeFrame,
        target_ime: HostToolsPlainTextImeFrame,
        pid_ime: HostToolsPlainTextImeFrame,
        sidebar_width: f32,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let connections = self.monitor_connections();
        if connections.is_empty() {
            return host_tools_center_state(
                LucideIcon::WifiOff,
                tokens.ui.text_muted,
                i18n.t("profiler.panel.no_connection"),
                selectable_text,
                cx,
            );
        }

        let selected_connection_id = self.selected_connection_id_owned();
        let selected_id = selected_connection_id
            .as_deref()
            .unwrap_or(connections[0].connection_id.as_str());
        let probe = self.ecapture_probe_for(selected_id);
        let session = self.ecapture_session(selected_id);
        let sequences = session
            .map(|session| {
                session.visible_sequences(&self.ui.host_ecapture_search, self.host_ecapture.filter)
            })
            .unwrap_or_default();
        self.sync_ecapture_render_rows(&sequences, selected_id);

        div()
            .id("host-ecapture-panel")
            .w_full()
            .min_w_0()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
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
                        &connections,
                        selected_id,
                        !self.ecapture_busy(),
                        tokens,
                        mono_font_family.clone(),
                        selectable_text,
                        cx,
                    ))
                    .child(self.render_ecapture_status_row(probe, session, tokens, i18n, cx))
                    .child(self.render_ecapture_template_row(tokens, i18n, cx))
                    .child(self.render_ecapture_field(
                        &target_ime,
                        self.host_ecapture.template.target_label_key(),
                        self.host_ecapture.template.target_placeholder_key(),
                        &self.ui.host_ecapture_target,
                        self.host_ecapture.template.target_required(),
                        tokens,
                        i18n,
                        cx,
                    ))
                    .child(self.render_ecapture_field(
                        &pid_ime,
                        Some("sidebar.host_ecapture.fields.pid"),
                        Some("sidebar.host_ecapture.fields.pid_placeholder"),
                        &self.ui.host_ecapture_pid,
                        false,
                        tokens,
                        i18n,
                        cx,
                    ))
                    .child(self.render_ecapture_actions_row(session, probe, tokens, i18n, cx))
                    .child(self.render_ecapture_search(&search_ime, tokens, i18n, cx))
                    .child(self.render_ecapture_filter_row(tokens, i18n, cx))
                    .when_some(self.render_ecapture_form_error(i18n), |column, error| {
                        column.child(error)
                    }),
            )
            .child(self.render_host_ecapture_list(
                sequences,
                selected_id,
                probe,
                session,
                tokens,
                i18n,
                mono_font_family,
                selectable_text,
                cx,
            ))
            .into_any_element()
    }

    fn render_ecapture_status_row(
        &self,
        probe: Option<&ResourceEcaptureProbe>,
        session: Option<&EcaptureSession>,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let status = probe.map(|probe| probe.status.clone()).unwrap_or_default();
        let (tone, label) = match &status {
            ResourceEcaptureStatus::Available {
                capability: EcaptureCommandCapability::Full,
                ..
            } => (
                StatusTone::Success,
                i18n.t("sidebar.host_ecapture.capability.full"),
            ),
            ResourceEcaptureStatus::Available { .. } => (
                StatusTone::Warning,
                i18n.t("sidebar.host_ecapture.capability.partial"),
            ),
            ResourceEcaptureStatus::Unavailable => (
                StatusTone::Error,
                i18n.t("sidebar.host_ecapture.unavailable"),
            ),
            ResourceEcaptureStatus::Error { .. } => (
                StatusTone::Error,
                i18n.t("sidebar.host_ecapture.status.probe_failed"),
            ),
            ResourceEcaptureStatus::Unknown => (
                StatusTone::Neutral,
                i18n.t("sidebar.host_ecapture.status.checking"),
            ),
        };
        let running = session.is_some_and(EcaptureSession::running);
        let capture_label = if running {
            i18n.t("sidebar.host_ecapture.status.running")
        } else {
            i18n.t("sidebar.host_ecapture.status.stopped")
        };
        let detail = match (&status, session) {
            (ResourceEcaptureStatus::Available { .. }, Some(session)) => {
                let mut detail = format!(
                    "{} {} · {} {}",
                    session.event_count(),
                    i18n.t("sidebar.host_ecapture.events"),
                    session.dropped_events(),
                    i18n.t("sidebar.host_ecapture.dropped"),
                );
                if session.skipped_lines() > 0 {
                    detail.push_str(&format!(
                        " · {} {}",
                        session.skipped_lines(),
                        i18n.t("sidebar.host_ecapture.skipped")
                    ));
                }
                if let Some(line) = session.runtime_log().back() {
                    detail = format!("{detail} · {line}");
                }
                detail
            }
            (ResourceEcaptureStatus::Available { .. }, None) => probe
                .map(|probe| format!("{} · {}", probe.version, probe.kernel))
                .unwrap_or_default(),
            (ResourceEcaptureStatus::Error { message }, _) => message.clone(),
            (ResourceEcaptureStatus::Unavailable, _) => {
                i18n.t("sidebar.host_ecapture.unavailable_hint")
            }
            (ResourceEcaptureStatus::Unknown, _) => i18n.t("sidebar.host_ecapture.status.checking"),
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .min_w_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(status_pill(
                        tokens,
                        label,
                        StatusPillOptions::new(tone).compact(),
                    ))
                    .child(status_pill(
                        tokens,
                        capture_label,
                        StatusPillOptions::new(if running {
                            StatusTone::Accent
                        } else {
                            StatusTone::Neutral
                        })
                        .compact(),
                    ))
                    .child(div().flex_1())
                    .child(host_tools_tooltip_icon_button(
                        tokens,
                        LucideIcon::RefreshCw,
                        13.0,
                        rgb(theme.text),
                        oxideterm_gpui_ui::button::IconButtonOptions {
                            size: 24.0,
                            disabled: self.ecapture_busy(),
                            has_background: true,
                            background: Some(rgb(theme.bg_hover)),
                            hover_background: Some(rgb(theme.bg_panel)),
                            idle_opacity: 1.0,
                            ..oxideterm_gpui_ui::button::IconButtonOptions::compact(24.0)
                        },
                        i18n.t("sidebar.host_ecapture.actions.refresh"),
                        "host-ecapture-refresh",
                        true,
                        cx.listener(move |host_tools, _event, _window, cx| {
                            host_tools
                                .request_active_tool_snapshot(HostSnapshotFeedback::Toast, cx);
                            cx.stop_propagation();
                        }),
                    )),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(10.0))
                    .text_color(rgb(theme.text_muted))
                    .child(detail),
            )
            .into_any_element()
    }

    fn render_ecapture_template_row(
        &self,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut row = div()
            .id("host-ecapture-templates")
            .flex()
            .items_center()
            .gap_1()
            .overflow_x_scrollbar();
        for template in EcaptureTemplate::ALL {
            let active = self.host_ecapture.template == template;
            row = row.child(
                host_ecapture_chip(active, tokens)
                    .child(i18n.t(ecapture_template_label_key(template)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _event, _window, cx| {
                            this.select_ecapture_template(template, cx);
                            cx.stop_propagation();
                        }),
                    ),
            );
        }
        row.into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_ecapture_field(
        &self,
        ime: &HostToolsPlainTextImeFrame,
        label_key: Option<&'static str>,
        placeholder_key: Option<&'static str>,
        value: &str,
        required: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let Some(label_key) = label_key else {
            return div().into_any_element();
        };
        let input = ime.input();
        let anchor_frame = ime.clone();
        let input_control = ime
            .text_input(
                tokens,
                TextInputView {
                    value,
                    placeholder: placeholder_key.map(|key| i18n.t(key)).unwrap_or_default(),
                    focused: self.ui.input_is_focused(input),
                    caret_visible: ime.caret_visible(),
                    secret: false,
                    selected_all: false,
                    selected_range: ime.selected_range(),
                    marked_text: ime.marked_text(),
                },
            )
            .h(px(HOST_ECAPTURE_INPUT_HEIGHT))
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |host_tools, event: &MouseDownEvent, window, cx| {
                    host_tools.ui.focus_input(input);
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
            );
        div()
            .flex()
            .flex_col()
            .gap_1()
            .min_w_0()
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(rgb(theme.text_muted))
                    .child(if required {
                        format!("{} *", i18n.t(label_key))
                    } else {
                        i18n.t(label_key)
                    }),
            )
            .child(text_input_anchor_probe(
                ime.anchor_id(),
                input_control,
                move |anchor, _window, _cx| anchor_frame.update_anchor(anchor),
            ))
            .into_any_element()
    }

    fn render_ecapture_actions_row(
        &self,
        session: Option<&EcaptureSession>,
        probe: Option<&ResourceEcaptureProbe>,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let running = session.is_some_and(EcaptureSession::running);
        let busy = self.ecapture_busy();
        let can_start = !busy
            && !running
            && matches!(
                probe.map(|probe| &probe.status),
                Some(ResourceEcaptureStatus::Available { .. })
            );
        let can_clear = !busy && !running && session.is_some_and(|session| !session.absent());
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div().flex_1().child(
                    button_with(
                        tokens,
                        i18n.t("sidebar.host_ecapture.actions.start"),
                        ButtonOptions {
                            variant: ButtonVariant::Default,
                            size: ButtonSize::Sm,
                            radius: ButtonRadius::Md,
                            disabled: !can_start,
                        },
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |host_tools, _event, _window, cx| {
                            if let Some(notice) = host_tools.open_ecapture_start_confirm(cx) {
                                cx.emit(HostToolsEvent::ShowNotice(notice));
                            }
                            cx.stop_propagation();
                        }),
                    ),
                ),
            )
            .child(
                div().flex_1().child(
                    button_with(
                        tokens,
                        i18n.t("sidebar.host_ecapture.actions.stop"),
                        ButtonOptions {
                            variant: ButtonVariant::Outline,
                            size: ButtonSize::Sm,
                            radius: ButtonRadius::Md,
                            disabled: !running || busy,
                        },
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |host_tools, _event, _window, cx| {
                            if let Some(notice) = host_tools.request_ecapture_stop_from_view(cx) {
                                cx.emit(HostToolsEvent::ShowNotice(notice));
                            }
                            cx.stop_propagation();
                        }),
                    ),
                ),
            )
            .child(
                div().flex_1().child(
                    button_with(
                        tokens,
                        i18n.t("sidebar.host_ecapture.actions.clear"),
                        ButtonOptions {
                            variant: ButtonVariant::Outline,
                            size: ButtonSize::Sm,
                            radius: ButtonRadius::Md,
                            disabled: !can_clear,
                        },
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |host_tools, _event, _window, cx| {
                            if let Some(notice) = host_tools.open_ecapture_clear_confirm(cx) {
                                cx.emit(HostToolsEvent::ShowNotice(notice));
                            }
                            cx.stop_propagation();
                        }),
                    ),
                ),
            )
            .into_any_element()
    }

    fn render_ecapture_search(
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
                    value: &self.ui.host_ecapture_search,
                    placeholder: i18n.t("sidebar.host_ecapture.search_placeholder"),
                    focused: self.ui.input_is_focused(input),
                    caret_visible: ime.caret_visible(),
                    secret: false,
                    selected_all: false,
                    selected_range: ime.selected_range(),
                    marked_text: ime.marked_text(),
                },
            )
            .h(px(HOST_ECAPTURE_INPUT_HEIGHT))
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |host_tools, event: &MouseDownEvent, window, cx| {
                    host_tools.ui.focus_input(input);
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

    fn render_ecapture_filter_row(
        &self,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut row = div()
            .id("host-ecapture-filters")
            .flex()
            .items_center()
            .gap_1()
            .overflow_x_scrollbar();
        for filter in [
            EcaptureFilter::All,
            EcaptureFilter::Requests,
            EcaptureFilter::Responses,
        ] {
            let active = self.host_ecapture.filter == filter;
            row = row.child(
                host_ecapture_chip(active, tokens)
                    .child(i18n.t(ecapture_filter_label_key(filter)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _event, _window, cx| {
                            this.select_ecapture_filter(filter, cx);
                            cx.stop_propagation();
                        }),
                    ),
            );
        }
        row.into_any_element()
    }

    fn render_ecapture_form_error(&self, i18n: &I18n) -> Option<AnyElement> {
        if let Some(key) = self.host_ecapture.form_error_key {
            return Some(
                div()
                    .text_size(px(10.5))
                    .text_color(rgb(MONITOR_RED))
                    .child(i18n.t(key))
                    .into_any_element(),
            );
        }
        self.host_ecapture.error_text.as_ref().map(|message| {
            div()
                .text_size(px(10.5))
                .text_color(rgb(MONITOR_RED))
                .child(message.clone())
                .into_any_element()
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn render_host_ecapture_list(
        &self,
        sequences: Vec<u64>,
        selected_id: &str,
        probe: Option<&ResourceEcaptureProbe>,
        session: Option<&EcaptureSession>,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(session) = session else {
            return host_tools_center_state(
                LucideIcon::KeyRound,
                tokens.ui.text_muted,
                i18n.t("sidebar.host_ecapture.empty_idle"),
                selectable_text,
                cx,
            );
        };
        if session.absent() {
            return host_tools_center_state(
                LucideIcon::KeyRound,
                tokens.ui.text_muted,
                i18n.t("sidebar.host_ecapture.empty_idle"),
                selectable_text,
                cx,
            );
        }
        if matches!(
            probe.map(|probe| &probe.status),
            Some(ResourceEcaptureStatus::Unavailable)
        ) {
            return host_tools_center_state(
                LucideIcon::ShieldOff,
                tokens.ui.text_muted,
                i18n.t("sidebar.host_ecapture.unavailable"),
                selectable_text,
                cx,
            );
        }
        if matches!(
            probe.map(|probe| &probe.status),
            Some(ResourceEcaptureStatus::Error { .. })
        ) {
            let message = match probe.map(|probe| &probe.status) {
                Some(ResourceEcaptureStatus::Error { message }) => message.clone(),
                _ => String::new(),
            };
            return host_tools_center_state(
                LucideIcon::AlertTriangle,
                MONITOR_RED,
                message,
                selectable_text,
                cx,
            );
        }
        if sequences.is_empty() {
            let label = if session.running() {
                i18n.t("sidebar.host_ecapture.waiting")
            } else {
                i18n.t("sidebar.host_ecapture.empty")
            };
            return host_tools_center_state(
                LucideIcon::KeyRound,
                tokens.ui.text_muted,
                label,
                selectable_text,
                cx,
            );
        }

        let rows = Arc::new(sequences);
        let selected_id = Arc::new(selected_id.to_string());
        let state = self.ecapture_list_state();
        let spec = TauriVirtualListSpec::new(px(HOST_ECAPTURE_LIST_ESTIMATED_ROW_HEIGHT), 8);
        let host_tools = cx.entity();
        let row_tokens = *tokens;
        let row_i18n = i18n.clone();
        let row_mono_font_family = mono_font_family;
        div()
            .w_full()
            .min_w_0()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(Self::render_host_ecapture_table_header(tokens, i18n))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(tauri_virtual_list(
                        state,
                        spec,
                        move |index, _window, cx| {
                            let rows = rows.clone();
                            let selected_id = selected_id.clone();
                            host_tools.update(cx, |host_tools, cx| {
                                host_tools.render_host_ecapture_row(
                                    index,
                                    rows.get(index).copied(),
                                    selected_id.as_str(),
                                    &row_tokens,
                                    &row_i18n,
                                    row_mono_font_family.clone(),
                                    cx,
                                )
                            })
                        },
                    )),
            )
            .into_any_element()
    }

    fn render_host_ecapture_table_header(tokens: &ThemeTokens, i18n: &I18n) -> AnyElement {
        let theme = tokens.ui;
        div()
            .flex_none()
            .w_full()
            .min_w_0()
            .h(px(HOST_LOG_TABLE_HEADER_HEIGHT))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(rgba((theme.border << 8) | MONITOR_BORDER_ALPHA))
            .bg(rgb(theme.bg))
            .text_size(px(HOST_PROCESS_TABLE_HEADER_TEXT_SIZE))
            .text_color(rgb(theme.text_muted))
            .child(
                div()
                    .flex_none()
                    .w(px(52.0))
                    .child(i18n.t("sidebar.host_ecapture.columns.direction")),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .child(i18n.t("sidebar.host_ecapture.columns.event")),
            )
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_host_ecapture_row(
        &self,
        _index: usize,
        sequence: Option<u64>,
        connection_id: &str,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(sequence) = sequence else {
            return div().into_any_element();
        };
        let Some(event) = self
            .host_ecapture
            .sessions
            .get(connection_id)
            .and_then(|session| session.event(sequence))
        else {
            return div().into_any_element();
        };
        let theme = tokens.ui;
        let expanded = self.host_ecapture.expanded_sequence == Some(sequence);
        let direction_label = i18n.t(ecapture_direction_label_key(event.direction));
        let direction_color = match event.direction {
            EcaptureDirection::Read => MONITOR_BLUE,
            EcaptureDirection::Write => MONITOR_EMERALD,
            EcaptureDirection::Unknown => theme.text_muted,
        };
        let meta = format!(
            "{} · {} {} · {}",
            host_ecapture_blank(&event.comm),
            i18n.t("sidebar.host_ecapture.columns.pid"),
            event.pid,
            i18n.t(ecapture_kind_label_key(event.kind)),
        );
        let summary = if event.summary.is_empty() {
            format!("{} B", event.length)
        } else {
            event.summary.clone()
        };
        let details = expanded
            .then(|| self.render_host_ecapture_detail(event, tokens, i18n, mono_font_family));
        let sequence_for_toggle = sequence;
        div()
            .id(host_tools_row_id(
                "host-ecapture-row",
                connection_id,
                sequence.to_string(),
            ))
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .border_b_1()
            .border_color(rgba((theme.border << 8) | MONITOR_BORDER_ALPHA))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |host_tools, _event, _window, cx| {
                    host_tools.toggle_ecapture_expanded(sequence_for_toggle, cx);
                    cx.stop_propagation();
                }),
            )
            .child(
                div()
                    .px_3()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .text_size(px(12.0))
                    .child(
                        div()
                            .flex_none()
                            .w(px(44.0))
                            .text_size(px(10.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(rgb(direction_color))
                            .child(direction_label),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.0))
                                    .text_color(rgb(theme.text))
                                    .child(summary),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(10.0))
                                    .text_color(rgb(theme.text_muted))
                                    .child(meta),
                            ),
                    )
                    .when(event.payload_truncated, |row| {
                        row.child(status_pill(
                            tokens,
                            i18n.t("sidebar.host_ecapture.truncated"),
                            StatusPillOptions::new(StatusTone::Warning).compact(),
                        ))
                    }),
            )
            .when_some(details, |row, details| row.child(details))
            .into_any_element()
    }

    fn render_host_ecapture_detail(
        &self,
        event: &EcaptureEvent,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
    ) -> AnyElement {
        let theme = tokens.ui;
        let mut payload = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(1.0))
            .font_family(mono_font_family)
            .text_size(px(10.5))
            .text_color(rgb(theme.text))
            .overflow_x_scrollbar();
        let total_lines = event.payload.lines().count();
        for (index, line) in event
            .payload
            .lines()
            .take(HOST_ECAPTURE_PAYLOAD_PREVIEW_LINES)
            .enumerate()
        {
            payload = payload.child(
                div()
                    .id(("host-ecapture-line", index))
                    .whitespace_nowrap()
                    .child(if line.is_empty() {
                        " ".to_string()
                    } else {
                        line.to_string()
                    }),
            );
        }
        if total_lines > HOST_ECAPTURE_PAYLOAD_PREVIEW_LINES {
            payload = payload.child(
                div()
                    .whitespace_nowrap()
                    .text_color(rgb(theme.text_muted))
                    .child(format!("… {}", i18n.t("sidebar.host_ecapture.truncated"))),
            );
        }
        let mut meta = div()
            .flex()
            .flex_col()
            .gap(px(1.0))
            .text_size(px(10.0))
            .text_color(rgb(theme.text_muted))
            .child(format!(
                "{}: {}",
                i18n.t("sidebar.host_ecapture.columns.length"),
                event.length
            ));
        if !event.timestamp.is_empty() {
            meta = meta.child(format!(
                "{}: {}",
                i18n.t("sidebar.host_ecapture.columns.time"),
                event.timestamp
            ));
        }
        if !event.tuple.is_empty() {
            meta = meta.child(format!(
                "{}: {}",
                i18n.t("sidebar.host_ecapture.columns.connection"),
                event.tuple
            ));
        }
        meta = meta.child(format!(
            "{}: {}",
            i18n.t("sidebar.host_ecapture.columns.fd"),
            event.fd
        ));
        div()
            .px_3()
            .pb_2()
            .flex()
            .flex_col()
            .gap_1()
            .min_w_0()
            .child(meta)
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .p_2()
                    .rounded(px(tokens.radii.md))
                    .border_1()
                    .border_color(rgba((theme.border << 8) | MONITOR_BORDER_ALPHA))
                    .bg(rgb(theme.bg_panel))
                    .overflow_hidden()
                    .child(payload),
            )
            .into_any_element()
    }

    fn sync_ecapture_render_rows(&self, sequences: &[u64], connection_id: &str) {
        let signatures = sequences
            .iter()
            .filter_map(|sequence| {
                self.host_ecapture
                    .sessions
                    .get(connection_id)
                    .and_then(|session| session.event(*sequence))
                    .map(|event| {
                        ecapture_event_row_signature(
                            event,
                            self.host_ecapture.expanded_sequence == Some(event.sequence),
                        )
                    })
            })
            .collect::<Vec<_>>();
        let identity = format!(
            "host-ecapture:{connection_id}:{}:{}:{:?}",
            self.ui.host_ecapture_search,
            self.host_ecapture.filter as u8,
            self.host_ecapture.expanded_sequence
        );
        sync_tauri_variable_list_state_by_signatures(
            &self.host_ecapture.list_state,
            &mut self.host_ecapture.list_cache.borrow_mut(),
            &identity,
            &signatures,
            TauriVirtualListSpec::new(px(HOST_ECAPTURE_LIST_ESTIMATED_ROW_HEIGHT), 8),
        );
    }

    fn ecapture_probe_for(&self, connection_id: &str) -> Option<&ResourceEcaptureProbe> {
        self.host_ecapture
            .probe
            .as_ref()
            .filter(|_| self.host_ecapture.probe_connection_id.as_deref() == Some(connection_id))
    }

    fn ecapture_session(&self, connection_id: &str) -> Option<&EcaptureSession> {
        self.host_ecapture.sessions.get(connection_id)
    }

    fn ecapture_busy(&self) -> bool {
        self.host_ecapture.start_running.is_some()
            || self.host_ecapture.action_running.is_some()
            || self.host_ecapture.pending_confirm.is_some()
    }

    fn ecapture_list_state(&self) -> ListState {
        self.host_ecapture.list_state.clone()
    }

    pub(in crate::workspace::connection_monitor) fn ecapture_confirm_view(
        &self,
    ) -> Option<(
        HostEcaptureConfirmRequest,
        oxideterm_gpui_ui::motion::ExitPhase,
    )> {
        self.host_ecapture
            .pending_confirm
            .as_ref()
            .map(|state| (state.request.clone(), state.presence.phase()))
    }

    fn render_host_ecapture_confirm_view(
        &self,
        i18n: &I18n,
        tokens: &ThemeTokens,
        mono_font_family: SharedString,
    ) -> Option<(oxideterm_gpui_ui::motion::ExitPhase, ConfirmDialogView)> {
        let (request, phase) = self.ecapture_confirm_view()?;
        let (title_key, description, confirm_key, variant) = match &request.action {
            HostEcaptureConfirmAction::Start { .. } => (
                "sidebar.host_ecapture.confirm.start_title",
                "sidebar.host_ecapture.confirm.start_desc",
                "sidebar.host_ecapture.confirm.start_action",
                ConfirmDialogVariant::Default,
            ),
            HostEcaptureConfirmAction::Clear { .. } => (
                "sidebar.host_ecapture.confirm.clear_title",
                "sidebar.host_ecapture.confirm.clear_desc",
                "sidebar.host_ecapture.confirm.clear_action",
                ConfirmDialogVariant::Danger,
            ),
        };
        let mut description_column = div().flex().flex_col().gap_2().child(i18n.t(description));
        if matches!(request.action, HostEcaptureConfirmAction::Start { .. }) {
            description_column = description_column.child(
                div()
                    .max_h(px(150.0))
                    .w_full()
                    .min_w_0()
                    .overflow_y_scrollbar()
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(rgb(tokens.ui.border))
                    .p_2()
                    .font_family(mono_font_family)
                    .text_size(px(10.0))
                    .whitespace_normal()
                    .child(request.preview.clone()),
            );
        }
        Some((
            phase,
            ConfirmDialogView {
                variant,
                title: div().child(i18n.t(title_key)).into_any_element(),
                description: Some(description_column.into_any_element()),
                cancel_label: div()
                    .child(i18n.t("sidebar.host_ecapture.confirm.cancel"))
                    .into_any_element(),
                confirm_label: div().child(i18n.t(confirm_key)).into_any_element(),
            },
        ))
    }

    pub(in crate::workspace::connection_monitor) fn dismiss_ecapture_confirm(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.host_ecapture.pending_confirm.take().is_some() {
            cx.notify();
        }
    }

    pub(in crate::workspace::connection_monitor) fn begin_ecapture_confirm_exit(
        &mut self,
        delay: Duration,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(generation) = self
            .host_ecapture
            .pending_confirm
            .as_mut()
            .and_then(|state| state.presence.begin_exit())
        else {
            return false;
        };
        if delay.is_zero() {
            self.host_ecapture.pending_confirm = None;
            cx.notify();
            return true;
        }
        cx.spawn(async move |weak, cx| {
            Timer::after(delay).await;
            let _ = weak.update(cx, |entity, cx| {
                if entity
                    .host_ecapture
                    .pending_confirm
                    .as_ref()
                    .is_some_and(|state| state.presence.finish_exit(generation))
                {
                    entity.host_ecapture.pending_confirm = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
        true
    }

    pub(in crate::workspace::connection_monitor) fn confirm_ecapture_action_from_view(
        &mut self,
        delay: Duration,
        cx: &mut Context<Self>,
    ) {
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::EcaptureConnectionMissing,
            ));
            return;
        };
        for notice in self.confirm_ecapture_action(delay, runtime, cx) {
            cx.emit(HostToolsEvent::ShowNotice(notice));
        }
    }

    fn confirm_ecapture_action(
        &mut self,
        delay: Duration,
        runtime: tokio::runtime::Handle,
        cx: &mut Context<Self>,
    ) -> Vec<HostToolsNotice> {
        let Some(request) = self
            .host_ecapture
            .pending_confirm
            .as_ref()
            .map(|state| state.request.clone())
        else {
            return Vec::new();
        };
        if !self.begin_ecapture_confirm_exit(delay, cx) {
            return Vec::new();
        }
        match request.action {
            HostEcaptureConfirmAction::Start {
                launch,
                spec,
                binary,
            } => self.start_ecapture(request.connection_id, launch, spec, binary, runtime, cx),
            HostEcaptureConfirmAction::Clear { run_id } => self
                .request_ecapture_action_with_run_id(
                    HostEcaptureActionKind::Clear,
                    request.connection_id,
                    run_id,
                    runtime,
                    cx,
                ),
        }
    }

    pub(in crate::workspace::connection_monitor) fn open_ecapture_start_confirm(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<HostToolsNotice> {
        if self.ecapture_busy() {
            return Some(HostToolsNotice::EcaptureBusy);
        }
        let Some(connection_id) = self.selected_connection_id_owned() else {
            return Some(HostToolsNotice::EcaptureConnectionMissing);
        };
        let Some(os_type) = self.connection_os_type(&connection_id) else {
            return Some(HostToolsNotice::EcaptureConnectionMissing);
        };
        if self
            .ecapture_session(&connection_id)
            .is_some_and(EcaptureSession::running)
            || self.host_ecapture.poll_running.is_some()
        {
            return Some(HostToolsNotice::EcaptureBusy);
        }
        let launch = EcaptureLaunch {
            template: self.host_ecapture.template,
            target: self.ui.host_ecapture_target.clone(),
            pid: self.ui.host_ecapture_pid.clone(),
        };
        let spec = match validate_ecapture_launch(&launch) {
            Ok(spec) => spec,
            Err(error) => {
                self.host_ecapture.form_error_key = Some(error.label_key());
                cx.notify();
                return None;
            }
        };
        self.host_ecapture.form_error_key = None;
        let probe = self.ecapture_probe_for(&connection_id).cloned();
        let binary = match probe.as_ref().map(|probe| &probe.status) {
            Some(ResourceEcaptureStatus::Available {
                capability: EcaptureCommandCapability::Full,
                ..
            }) => probe
                .as_ref()
                .map(|probe| probe.binary.clone())
                .filter(|binary| !binary.is_empty()),
            Some(ResourceEcaptureStatus::Available { .. }) => {
                self.host_ecapture.form_error_key =
                    Some("sidebar.host_ecapture.validation.privilege_required");
                cx.notify();
                return None;
            }
            Some(ResourceEcaptureStatus::Unavailable)
            | Some(ResourceEcaptureStatus::Error { .. }) => {
                self.host_ecapture.form_error_key =
                    Some("sidebar.host_ecapture.validation.binary_missing");
                cx.notify();
                return None;
            }
            Some(ResourceEcaptureStatus::Unknown) | None => {
                self.host_ecapture.form_error_key =
                    Some("sidebar.host_ecapture.validation.probe_pending");
                cx.notify();
                return None;
            }
        };
        let run_id = ecapture_run_id(&connection_id);
        let preview =
            match build_ecapture_start_command(&os_type, &run_id, &spec, binary.as_deref()) {
                Ok(command) => command.command,
                Err(message) => {
                    self.host_ecapture.error_text = Some(message);
                    cx.notify();
                    return None;
                }
            };
        HostToolConfirmState::open(
            &mut self.host_ecapture.pending_confirm,
            HostEcaptureConfirmRequest {
                connection_id,
                action: HostEcaptureConfirmAction::Start {
                    launch,
                    spec,
                    binary,
                },
                preview,
            },
        );
        self.host_ecapture.error_text = None;
        cx.notify();
        None
    }

    pub(in crate::workspace::connection_monitor) fn open_ecapture_clear_confirm(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<HostToolsNotice> {
        if self.ecapture_busy() {
            return Some(HostToolsNotice::EcaptureBusy);
        }
        let Some(connection_id) = self.selected_connection_id_owned() else {
            return Some(HostToolsNotice::EcaptureConnectionMissing);
        };
        if self.host_ecapture.poll_running.is_some() {
            return Some(HostToolsNotice::EcaptureBusy);
        }
        let Some(session) = self.ecapture_session(&connection_id) else {
            return None;
        };
        if session.running() || session.absent() {
            return None;
        }
        let run_id = ecapture_run_id(&connection_id);
        HostToolConfirmState::open(
            &mut self.host_ecapture.pending_confirm,
            HostEcaptureConfirmRequest {
                connection_id,
                action: HostEcaptureConfirmAction::Clear {
                    run_id: run_id.clone(),
                },
                preview: format!("/tmp/{run_id}"),
            },
        );
        cx.notify();
        None
    }

    fn start_ecapture(
        &mut self,
        connection_id: String,
        launch: EcaptureLaunch,
        spec: EcaptureSpec,
        binary: Option<String>,
        runtime: tokio::runtime::Handle,
        cx: &mut Context<Self>,
    ) -> Vec<HostToolsNotice> {
        if self.host_ecapture.start_running.is_some() {
            return vec![HostToolsNotice::EcaptureBusy];
        }
        let Some(os_type) = self.connection_os_type(&connection_id) else {
            return vec![HostToolsNotice::EcaptureConnectionMissing];
        };
        let run_id = ecapture_run_id(&connection_id);
        let command =
            match build_ecapture_start_command(&os_type, &run_id, &spec, binary.as_deref()) {
                Ok(command) => command,
                Err(message) => {
                    self.host_ecapture.error_text = Some(message);
                    cx.notify();
                    return Vec::new();
                }
            };
        let failure_fallback = self
            .messages
            .as_ref()
            .map(|messages| messages.ecapture_unknown_error.clone())
            .unwrap_or_default();
        let request = HostEcaptureStartRequest {
            connection_id: connection_id.clone(),
            run_id: run_id.clone(),
            launch,
            spec,
            binary,
            failure_fallback,
        };
        // The start script wipes the per-connection run directory, so the
        // local cursor and event ring restart with it.
        self.host_ecapture
            .sessions
            .insert(connection_id, EcaptureSession::new(run_id));
        self.host_ecapture.start_running = Some(request.clone());
        self.host_ecapture.error_text = None;
        self.host_ecapture.form_error_key = None;
        let spawned = self.spawn_ecapture_start(
            zeroize::Zeroizing::new(command.command),
            request,
            HOST_ECAPTURE_START_TIMEOUT,
            HOST_ECAPTURE_START_MAX_OUTPUT_SIZE,
            runtime,
        );
        if !spawned {
            self.host_ecapture.start_running = None;
            return vec![HostToolsNotice::EcaptureConnectionMissing];
        }
        cx.notify();
        Vec::new()
    }

    pub(in crate::workspace::connection_monitor) fn finish_host_ecapture_start(
        &mut self,
        delivery: HostEcaptureStartDelivery,
        cx: &mut Context<Self>,
    ) {
        if self.host_ecapture.start_running.as_ref() != Some(&delivery.request) {
            return;
        }
        let connection_id = delivery.request.connection_id.clone();
        self.host_ecapture.start_running = None;
        match delivery.result {
            Ok(outcome) if outcome.started => {
                self.host_ecapture.error_text = None;
                cx.emit(HostToolsEvent::ShowNotice(HostToolsNotice::EcaptureStarted));
            }
            Ok(outcome) => {
                self.host_ecapture.error_text = Some(
                    outcome
                        .failure
                        .unwrap_or_else(|| self.ecapture_unknown_error()),
                );
                cx.emit(HostToolsEvent::ShowNotice(
                    HostToolsNotice::EcaptureStartFailed,
                ));
            }
            Err(message) => {
                self.host_ecapture.error_text = Some(message);
                cx.emit(HostToolsEvent::ShowNotice(
                    HostToolsNotice::EcaptureStartFailed,
                ));
            }
        }
        let failure_fallback = self.ecapture_unknown_error();
        if let Some(runtime) = self.lifecycle_runtime.clone() {
            // Startup failures are visible in the runtime log, so read the new
            // session immediately instead of waiting for a user refresh.
            let notices = self.request_ecapture_poll(
                connection_id,
                HostSnapshotFeedback::Silent,
                runtime,
                failure_fallback,
                cx,
            );
            debug_assert!(notices.is_empty());
        }
        cx.notify();
    }

    fn request_ecapture_stop_from_view(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<HostToolsNotice> {
        let Some(connection_id) = self.selected_connection_id_owned() else {
            return Some(HostToolsNotice::EcaptureConnectionMissing);
        };
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            return Some(HostToolsNotice::EcaptureConnectionMissing);
        };
        let mut notices =
            self.request_ecapture_action(HostEcaptureActionKind::Stop, connection_id, runtime, cx);
        notices.pop()
    }

    fn request_ecapture_action(
        &mut self,
        kind: HostEcaptureActionKind,
        connection_id: String,
        runtime: tokio::runtime::Handle,
        cx: &mut Context<Self>,
    ) -> Vec<HostToolsNotice> {
        let run_id = ecapture_run_id(&connection_id);
        self.request_ecapture_action_with_run_id(kind, connection_id, run_id, runtime, cx)
    }

    fn request_ecapture_action_with_run_id(
        &mut self,
        kind: HostEcaptureActionKind,
        connection_id: String,
        run_id: String,
        runtime: tokio::runtime::Handle,
        cx: &mut Context<Self>,
    ) -> Vec<HostToolsNotice> {
        if self.host_ecapture.action_running.is_some() || self.host_ecapture.start_running.is_some()
        {
            return vec![HostToolsNotice::EcaptureBusy];
        }
        let Some(os_type) = self.connection_os_type(&connection_id) else {
            return vec![HostToolsNotice::EcaptureConnectionMissing];
        };
        let command = match kind {
            HostEcaptureActionKind::Stop => build_ecapture_stop_command(&os_type, &run_id),
            HostEcaptureActionKind::Clear => build_ecapture_clear_command(&os_type, &run_id),
        };
        let command = match command {
            Ok(command) => command,
            Err(message) => {
                self.host_ecapture.error_text = Some(message);
                cx.notify();
                return Vec::new();
            }
        };
        let request = HostEcaptureActionRequest {
            connection_id,
            run_id,
            kind,
        };
        self.host_ecapture.action_running = Some(request.clone());
        let spawned = self.spawn_ecapture_action(
            zeroize::Zeroizing::new(command.command),
            request,
            HOST_ECAPTURE_ACTION_TIMEOUT,
            HOST_ECAPTURE_ACTION_MAX_OUTPUT_SIZE,
            runtime,
        );
        if !spawned {
            self.host_ecapture.action_running = None;
            return vec![HostToolsNotice::EcaptureConnectionMissing];
        }
        self.host_ecapture.error_text = None;
        cx.notify();
        Vec::new()
    }

    pub(in crate::workspace::connection_monitor) fn finish_host_ecapture_action(
        &mut self,
        delivery: HostEcaptureActionDelivery,
        cx: &mut Context<Self>,
    ) {
        if self.host_ecapture.action_running.as_ref() != Some(&delivery.request) {
            return;
        }
        let request = delivery.request.clone();
        self.host_ecapture.action_running = None;
        let succeeded = delivery.result.unwrap_or(false);
        match request.kind {
            HostEcaptureActionKind::Stop => {
                cx.emit(HostToolsEvent::ShowNotice(
                    HostToolsNotice::EcaptureStopped { succeeded },
                ));
            }
            HostEcaptureActionKind::Clear => {
                cx.emit(HostToolsEvent::ShowNotice(
                    HostToolsNotice::EcaptureCleared { succeeded },
                ));
                if succeeded {
                    self.host_ecapture.sessions.insert(
                        request.connection_id.clone(),
                        EcaptureSession::new(ecapture_run_id(&request.connection_id)),
                    );
                }
            }
        }
        let failure_fallback = self.ecapture_unknown_error();
        if let Some(runtime) = self.lifecycle_runtime.clone() {
            let notices = self.request_ecapture_poll(
                request.connection_id,
                HostSnapshotFeedback::Silent,
                runtime,
                failure_fallback,
                cx,
            );
            debug_assert!(notices.is_empty());
        }
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn request_ecapture_snapshot(
        &mut self,
        connection_id: String,
        feedback: HostSnapshotFeedback,
        runtime: tokio::runtime::Handle,
        failure_fallback: String,
        cx: &mut Context<Self>,
    ) -> Vec<HostToolsNotice> {
        let mut notices = self.request_ecapture_probe(
            connection_id.clone(),
            feedback,
            runtime.clone(),
            failure_fallback.clone(),
            cx,
        );
        notices.extend(self.request_ecapture_poll(
            connection_id,
            feedback,
            runtime,
            failure_fallback,
            cx,
        ));
        notices
    }

    fn request_ecapture_probe(
        &mut self,
        connection_id: String,
        feedback: HostSnapshotFeedback,
        runtime: tokio::runtime::Handle,
        failure_fallback: String,
        cx: &mut Context<Self>,
    ) -> Vec<HostToolsNotice> {
        if self.host_ecapture.probe_running.is_some() {
            return feedback
                .should_toast()
                .then_some(HostToolsNotice::EcaptureBusy)
                .into_iter()
                .collect();
        }
        let Some(os_type) = self.connection_os_type(&connection_id) else {
            return feedback
                .should_toast()
                .then_some(HostToolsNotice::EcaptureConnectionMissing)
                .into_iter()
                .collect();
        };
        let command = match build_ecapture_probe_command(&os_type) {
            Ok(command) => command,
            Err(message) => {
                self.host_ecapture.probe_connection_id = Some(connection_id);
                self.host_ecapture.probe = Some(ResourceEcaptureProbe {
                    status: ResourceEcaptureStatus::Error { message },
                    ..Default::default()
                });
                cx.notify();
                return Vec::new();
            }
        };
        let request = HostEcaptureProbeRequest {
            connection_id: connection_id.clone(),
            feedback,
            failure_fallback,
        };
        self.host_ecapture.probe_connection_id = Some(connection_id);
        self.host_ecapture.probe_running = Some(request.clone());
        let spawned = self.spawn_ecapture_probe(
            command.command,
            request,
            HOST_ECAPTURE_PROBE_TIMEOUT,
            HOST_ECAPTURE_PROBE_MAX_OUTPUT_SIZE,
            runtime,
        );
        if !spawned {
            self.host_ecapture.probe_running = None;
            return feedback
                .should_toast()
                .then_some(HostToolsNotice::EcaptureConnectionMissing)
                .into_iter()
                .collect();
        }
        cx.notify();
        Vec::new()
    }

    pub(in crate::workspace::connection_monitor) fn finish_host_ecapture_probe(
        &mut self,
        delivery: HostEcaptureProbeDelivery,
        cx: &mut Context<Self>,
    ) {
        if self.host_ecapture.probe_running.as_ref() != Some(&delivery.request) {
            return;
        }
        let feedback = delivery.request.feedback;
        self.host_ecapture.probe_running = None;
        let probe = match delivery.result {
            Ok(mut probe) => {
                if matches!(probe.status, ResourceEcaptureStatus::Unknown) {
                    probe.status = ResourceEcaptureStatus::Error {
                        message: delivery.request.failure_fallback.clone(),
                    };
                }
                probe
            }
            Err(message) => ResourceEcaptureProbe {
                status: ResourceEcaptureStatus::Error { message },
                ..Default::default()
            },
        };
        let notice = feedback.should_toast().then(|| match &probe.status {
            ResourceEcaptureStatus::Unavailable => Some(HostToolsNotice::EcaptureUnavailable),
            ResourceEcaptureStatus::Error { .. } => Some(HostToolsNotice::EcaptureProbeFailed),
            _ => None,
        });
        self.host_ecapture.probe_connection_id = Some(delivery.request.connection_id);
        self.host_ecapture.probe = Some(probe);
        if let Some(Some(notice)) = notice {
            cx.emit(HostToolsEvent::ShowNotice(notice));
        }
        cx.notify();
    }

    fn request_ecapture_poll(
        &mut self,
        connection_id: String,
        feedback: HostSnapshotFeedback,
        runtime: tokio::runtime::Handle,
        failure_fallback: String,
        cx: &mut Context<Self>,
    ) -> Vec<HostToolsNotice> {
        if self.host_ecapture.poll_running.is_some() {
            // The in-flight poll schedules the next tick when it finishes.
            return Vec::new();
        }
        let Some(os_type) = self.connection_os_type(&connection_id) else {
            return feedback
                .should_toast()
                .then_some(HostToolsNotice::EcaptureConnectionMissing)
                .into_iter()
                .collect();
        };
        let run_id = ecapture_run_id(&connection_id);
        let start_line = self
            .host_ecapture
            .sessions
            .entry(connection_id.clone())
            .or_insert_with(|| EcaptureSession::new(run_id.clone()))
            .next_line();
        let command = match build_ecapture_poll_command(&os_type, &run_id, start_line) {
            Ok(command) => command,
            Err(message) => {
                self.host_ecapture.error_text = Some(message);
                cx.notify();
                return Vec::new();
            }
        };
        let request = HostEcapturePollRequest {
            connection_id,
            run_id,
            start_line,
            feedback,
            failure_fallback,
        };
        self.host_ecapture.poll_running = Some(request.clone());
        let spawned = self.spawn_ecapture_poll(
            command.command,
            request,
            HOST_ECAPTURE_POLL_TIMEOUT,
            HOST_ECAPTURE_POLL_MAX_OUTPUT_SIZE,
            runtime,
        );
        if !spawned {
            self.host_ecapture.poll_running = None;
            return feedback
                .should_toast()
                .then_some(HostToolsNotice::EcaptureConnectionMissing)
                .into_iter()
                .collect();
        }
        cx.notify();
        Vec::new()
    }

    pub(in crate::workspace::connection_monitor) fn finish_host_ecapture_poll(
        &mut self,
        delivery: HostEcapturePollDelivery,
        cx: &mut Context<Self>,
    ) {
        if self.host_ecapture.poll_running.as_ref() != Some(&delivery.request) {
            return;
        }
        let request = delivery.request.clone();
        self.host_ecapture.poll_running = None;
        match delivery.result {
            Ok(poll) => {
                let still_running = poll.running;
                if let Some(session) = self.host_ecapture.sessions.get_mut(&request.connection_id)
                    // A start or clear replaces the cursor; a poll that was
                    // already in flight must not splice the old stream into it.
                    && session.next_line() == request.start_line
                {
                    session.apply_poll(poll);
                }
                if still_running {
                    // A stopped session keeps the last start failure visible;
                    // the runtime log alone does not explain it.
                    self.host_ecapture.error_text = None;
                }
            }
            Err(message) => {
                self.host_ecapture.error_text = Some(message);
                if request.feedback.should_toast() {
                    cx.emit(HostToolsEvent::ShowNotice(
                        HostToolsNotice::EcaptureSnapshotFailed,
                    ));
                }
            }
        }
        let stream_too_large =
            self.ecapture_session(&request.connection_id)
                .is_some_and(|session| {
                    session.running() && session.file_bytes() > ECAPTURE_MAX_STREAM_BYTES
                });
        if stream_too_large {
            // Bound disk growth: stop the root process and let the user start a
            // fresh capture with a new run directory.
            if let Some(runtime) = self.lifecycle_runtime.clone() {
                let notices = self.request_ecapture_action(
                    HostEcaptureActionKind::Stop,
                    request.connection_id.clone(),
                    runtime,
                    cx,
                );
                for notice in notices {
                    cx.emit(HostToolsEvent::ShowNotice(notice));
                }
            }
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::EcaptureStreamLimit,
            ));
        } else {
            self.schedule_ecapture_poll_tick(request.connection_id.clone());
        }
        cx.notify();
    }

    fn schedule_ecapture_poll_tick(&self, connection_id: String) {
        let Some(session) = self.ecapture_session(&connection_id) else {
            return;
        };
        if !(session.running() || session.has_backlog()) {
            return;
        }
        if self.host_ecapture.poll_running.is_some() {
            return;
        }
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            return;
        };
        let delivery_tx = self.reliable_delivery_tx.clone();
        runtime.spawn(async move {
            tokio::time::sleep(HOST_ECAPTURE_POLL_INTERVAL).await;
            let _ = delivery_tx
                .send(super::delivery::HostToolsReliableDelivery::EcapturePollTick(connection_id));
        });
    }

    pub(in crate::workspace::connection_monitor) fn handle_host_ecapture_poll_tick(
        &mut self,
        connection_id: String,
        cx: &mut Context<Self>,
    ) {
        if self.active_tool() != ContextSidebarTool::Ecapture
            || !self.visibility.sidebar_is_visible()
            || !self.monitoring.ecapture_enabled
            || self.selected_connection_id_owned().as_deref() != Some(connection_id.as_str())
        {
            return;
        }
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            return;
        };
        let Some(messages) = self.messages.as_ref() else {
            return;
        };
        let failure_fallback = messages.ecapture_unknown_error.clone();
        let notices = self.request_ecapture_poll(
            connection_id,
            HostSnapshotFeedback::Silent,
            runtime,
            failure_fallback,
            cx,
        );
        debug_assert!(notices.is_empty());
    }

    fn ecapture_unknown_error(&self) -> String {
        self.messages
            .as_ref()
            .map(|messages| messages.ecapture_unknown_error.clone())
            .unwrap_or_default()
    }

    pub(in crate::workspace::connection_monitor) fn select_ecapture_template(
        &mut self,
        template: EcaptureTemplate,
        cx: &mut Context<Self>,
    ) {
        if self.host_ecapture.template == template {
            return;
        }
        self.host_ecapture.template = template;
        self.host_ecapture.form_error_key = None;
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn select_ecapture_filter(
        &mut self,
        filter: EcaptureFilter,
        cx: &mut Context<Self>,
    ) {
        if self.host_ecapture.filter == filter {
            return;
        }
        self.host_ecapture.filter = filter;
        self.host_ecapture.expanded_sequence = None;
        cx.notify();
    }

    fn toggle_ecapture_expanded(&mut self, sequence: u64, cx: &mut Context<Self>) {
        self.host_ecapture.expanded_sequence =
            (self.host_ecapture.expanded_sequence != Some(sequence)).then_some(sequence);
        cx.notify();
    }
}

fn host_ecapture_chip(active: bool, tokens: &ThemeTokens) -> Div {
    let theme = tokens.ui;
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(24.0))
        .px_2()
        .rounded(px(tokens.radii.md))
        .text_size(px(10.5))
        .cursor_pointer()
        .bg(rgb(if active {
            theme.bg_hover
        } else {
            theme.bg_panel
        }))
        .text_color(rgb(if active { theme.text } else { theme.text_muted }))
        .when(active, |chip| {
            chip.border_1()
                .border_color(rgba((theme.accent << 8) | MONITOR_BORDER_ALPHA))
        })
}

fn host_ecapture_blank(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "—".to_string()
    } else {
        trimmed.to_string()
    }
}
