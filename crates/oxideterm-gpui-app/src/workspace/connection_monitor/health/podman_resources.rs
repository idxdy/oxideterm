//! Owns the Podman images/volumes/networks sub-views, their one-shot capture
//! lifecycle, and the Podman input/text dialogs.

use super::*;

use oxideterm_gpui_ui::button::ButtonVariant;

impl WorkspaceApp {
    pub(in crate::workspace) fn render_host_podman_input_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let target = WorkspaceImeTarget::HostPodmanDialog;
        let (kind, submit_disabled, input_control) = {
            let host_tools = self.host_tools.read(cx);
            let ui = &host_tools.ui;
            let dialog = ui.host_podman_input_dialog.as_ref()?;
            let input_control = self
                .workspace_text_input(
                    target,
                    TextInputView {
                        value: dialog.value.as_str(),
                        placeholder: self.i18n.t(host_podman_input_placeholder_key(&dialog.kind)),
                        focused: ui.input_is_focused(HostToolsTextInput::PodmanDialog),
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
                dialog.value.trim().is_empty(),
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
                            host_tools.ui.focus_input(HostToolsTextInput::PodmanDialog);
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
        let submit_label = self.i18n.t(host_podman_input_submit_key(&kind));
        let cancel_action = self.workspace_confirm_footer_action_button(
            self.i18n.t("sidebar.host_podman.confirm.cancel"),
            ButtonVariant::Secondary,
            ConfirmDialogAction::Cancel,
            false,
            None,
            |this, _event, _window, cx| {
                this.host_tools.update(cx, |host_tools, cx| {
                    host_tools.dismiss_podman_input_dialog(cx);
                });
                this.ime_marked_text = None;
                this.clear_ime_selection();
                cx.notify();
            },
            cx,
        );
        let submit_action = self.workspace_confirm_footer_action_button(
            submit_label,
            ButtonVariant::Default,
            ConfirmDialogAction::Confirm,
            submit_disabled,
            None,
            |this, _event, window, cx| {
                this.submit_host_podman_input_dialog(window, cx);
            },
            cx,
        );
        let dialog = {
            let host_tools = self.host_tools.read(cx);
            let ui = &host_tools.ui;
            let dialog = ui.host_podman_input_dialog.as_ref()?;
            let theme = self.tokens.ui;
            div()
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
                                .child(self.i18n.t(host_podman_input_title_key(&dialog.kind))),
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
                )
        };

        Some(
            oxideterm_gpui_ui::modal::dismissible_dialog_backdrop()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event, _window, cx| {
                        this.host_tools.update(cx, |host_tools, cx| {
                            host_tools.dismiss_podman_input_dialog(cx);
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

    pub(in crate::workspace) fn handle_host_podman_input_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self
            .host_tools
            .read(cx)
            .ui
            .host_podman_input_dialog
            .is_none()
        {
            return false;
        }
        match event.keystroke.key.as_str() {
            "escape" if !event.keystroke.modifiers.platform => {
                self.host_tools.update(cx, |host_tools, cx| {
                    host_tools.dismiss_podman_input_dialog(cx);
                });
                self.ime_marked_text = None;
                self.clear_ime_selection();
                cx.notify();
                true
            }
            "enter" => {
                self.submit_host_podman_input_dialog(window, cx);
                true
            }
            _ => false,
        }
    }

    pub(in crate::workspace) fn render_host_podman_capture_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.host_tools.read(cx).podman_text_dialog()?.clone();
        let theme = self.tokens.ui;
        let content = if dialog.loading {
            div()
                .p_4()
                .text_color(rgb(theme.text_muted))
                .child(self.i18n.t("sidebar.host_podman.captures.loading"))
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
            let mut rendered_lines = 0;
            if let Some(output) = output {
                for (index, line) in output.lines().enumerate() {
                    // Inspect/history output can embed secrets; lines render
                    // as bounded non-wrapping rows and never reach notices.
                    let display_line = if line.is_empty() {
                        " ".to_string()
                    } else {
                        line.chars().take(512).collect::<String>()
                    };
                    lines = lines.child(
                        div()
                            .id(("host-podman-capture-line", index))
                            .flex_none()
                            .whitespace_nowrap()
                            .child(display_line),
                    );
                    rendered_lines += 1;
                }
            }
            // An empty successful capture must not collapse the dialog.
            if rendered_lines == 0 {
                lines =
                    lines.child(div().child(self.i18n.t("sidebar.host_podman.resources_empty")));
            }
            lines.into_any_element()
        };

        Some(
            oxideterm_gpui_ui::modal::dismissible_dialog_backdrop()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event, _window, cx| {
                        this.host_tools.update(cx, |host_tools, cx| {
                            host_tools.dismiss_podman_text_dialog(cx);
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
                                        .truncate()
                                        .text_size(px(14.0))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(rgb(theme.text))
                                        .child(dialog.title.clone()),
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
                                    self.i18n.t("sidebar.host_podman.captures.close"),
                                    "host-podman-capture-close",
                                    true,
                                    cx.listener(|this, _event, _window, cx| {
                                        this.host_tools.update(cx, |host_tools, cx| {
                                            host_tools.dismiss_podman_text_dialog(cx);
                                        });
                                        cx.stop_propagation();
                                    }),
                                )),
                        )
                        .child(
                            div()
                                .id("host-podman-capture-scroll")
                                .h(px(420.0))
                                .overflow_y_scroll()
                                .track_scroll(&dialog.scroll_handle)
                                .vertical_scrollbar(&dialog.scroll_handle)
                                .overflow_x_scroll()
                                .child(content),
                        ),
                ))
                .into_any_element(),
        )
    }
}

impl HostToolsEntity {
    // ---- capture lifecycle ----

    pub(in crate::workspace::connection_monitor) fn request_podman_capture(
        &mut self,
        connection_id: String,
        kind: HostPodmanCaptureKind,
        cx: &mut Context<Self>,
    ) {
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            return;
        };
        let Some(os_type) = self.connection_os_type(&connection_id) else {
            return;
        };
        let command = match &kind {
            HostPodmanCaptureKind::Resources => {
                podman_resources_snapshot_command(&os_type).to_string()
            }
            HostPodmanCaptureKind::Stats => podman_stats_command(&os_type).to_string(),
            HostPodmanCaptureKind::Search { term } => match build_podman_search_command(term) {
                Ok(command) => command,
                Err(_) => return,
            },
            HostPodmanCaptureKind::Inspect { reference } => {
                match build_podman_inspect_command(&os_type, reference) {
                    Ok(command) => command.command,
                    Err(_) => return,
                }
            }
            HostPodmanCaptureKind::History { image_id } => {
                match build_podman_history_command(&os_type, image_id) {
                    Ok(command) => command.command,
                    Err(_) => return,
                }
            }
            HostPodmanCaptureKind::Top { container_id } => {
                match build_podman_top_command(&os_type, container_id) {
                    Ok(command) => command.command,
                    Err(_) => return,
                }
            }
            HostPodmanCaptureKind::Diff { container_id } => {
                match build_podman_diff_command(&os_type, container_id) {
                    Ok(command) => command.command,
                    Err(_) => return,
                }
            }
            HostPodmanCaptureKind::VolumeInspect { name } => {
                match build_podman_volume_inspect_command(&os_type, name) {
                    Ok(command) => command.command,
                    Err(_) => return,
                }
            }
            HostPodmanCaptureKind::NetworkInspect { name } => {
                match build_podman_network_inspect_command(&os_type, name) {
                    Ok(command) => command.command,
                    Err(_) => return,
                }
            }
            HostPodmanCaptureKind::VolumeUsage { name } => {
                match build_podman_volume_usage_command(&os_type, name) {
                    Ok(command) => command.command,
                    Err(_) => return,
                }
            }
        };
        let failure_fallback = self
            .messages
            .as_ref()
            .map(|messages| messages.podman_capture_failed.clone())
            .unwrap_or_else(|| "Podman capture failed.".to_string());
        let request = HostPodmanCaptureRequest {
            connection_id: connection_id.clone(),
            kind,
            failure_fallback,
        };
        let spawned = self.spawn_podman_capture(
            command,
            request.clone(),
            Duration::from_secs(30),
            256 * 1024,
            runtime,
        );
        if spawned {
            // Text-capture kinds fill the open dialog on delivery; without
            // registering the request the dialog would stay loading forever.
            if !matches!(
                request.kind,
                HostPodmanCaptureKind::Resources
                    | HostPodmanCaptureKind::Stats
                    | HostPodmanCaptureKind::Search { .. }
            ) {
                self.host_podman_resources.text_dialog_running = Some(request.clone());
            }
            match &request.kind {
                HostPodmanCaptureKind::Resources => {
                    if self.host_podman_resources.snapshot_running {
                        self.host_podman_resources.snapshot_pending = Some(request);
                        return;
                    }
                    self.host_podman_resources.snapshot_running = true;
                    self.host_podman_resources.resources_error = None;
                    self.host_podman_resources.snapshot_connection_id = Some(connection_id);
                }
                HostPodmanCaptureKind::Stats => self.host_podman_resources.stats_in_flight = true,
                HostPodmanCaptureKind::Search { .. } => {
                    self.host_podman_resources.search_in_flight = true
                }
                _ => {}
            }
            cx.notify();
        }
    }

    pub(in crate::workspace::connection_monitor) fn finish_host_podman_capture(
        &mut self,
        delivery: HostPodmanCaptureDelivery,
        cx: &mut Context<Self>,
    ) {
        let succeeded_output = match delivery.result {
            Ok(mut output) if output.exit_code.unwrap_or(0) == 0 => {
                let text = std::mem::take(&mut output.stdout);
                zeroize::Zeroize::zeroize(&mut output.stderr);
                text
            }
            Ok(mut output) => {
                // Keep a bounded stderr hint for the dialog; podman reports
                // lookup failures (missing volume/network) here.
                let stderr_hint =
                    podman_action_failure_message("", &output.stderr, output.exit_code);
                let stderr_hint = stderr_hint.trim();
                let stderr_hint = (!stderr_hint.is_empty()).then(|| stderr_hint.to_string());
                zeroize::Zeroize::zeroize(&mut output.stdout);
                zeroize::Zeroize::zeroize(&mut output.stderr);
                self.host_podman_resources.snapshot_running = false;
                self.host_podman_resources.snapshot_pending = None;
                self.host_podman_resources.resources_error =
                    Some(delivery.request.failure_fallback.clone());
                self.host_podman_resources.stats_in_flight = false;
                self.host_podman_resources.search_in_flight = false;
                // Resolve the open text dialog unconditionally: the running
                // marker is an optimization, never a delivery filter.
                if let Some(dialog) = self.host_podman_resources.text_dialog.as_mut() {
                    dialog.loading = false;
                    dialog.error = Some(match stderr_hint {
                        Some(hint) => format!(
                            "{} (exit {}, {})",
                            delivery.request.failure_fallback,
                            output.exit_code.unwrap_or(-1),
                            hint
                        ),
                        None => format!(
                            "{} (exit {})",
                            delivery.request.failure_fallback,
                            output.exit_code.unwrap_or(-1)
                        ),
                    });
                }
                self.host_podman_resources.text_dialog_running = None;
                cx.notify();
                return;
            }
            Err(()) => {
                self.host_podman_resources.snapshot_running = false;
                self.host_podman_resources.snapshot_pending = None;
                self.host_podman_resources.resources_error =
                    Some(delivery.request.failure_fallback.clone());
                self.host_podman_resources.stats_in_flight = false;
                self.host_podman_resources.search_in_flight = false;
                if let Some(dialog) = self.host_podman_resources.text_dialog.as_mut() {
                    dialog.loading = false;
                    dialog.error = Some(delivery.request.failure_fallback.clone());
                }
                self.host_podman_resources.text_dialog_running = None;
                cx.notify();
                return;
            }
        };
        match &delivery.request.kind {
            HostPodmanCaptureKind::Resources => {
                let (images, volumes, networks) =
                    parse_podman_resources_snapshot(&succeeded_output);
                self.host_podman_resources.images = Some(images);
                self.host_podman_resources.volumes = Some(volumes);
                self.host_podman_resources.networks = Some(networks);
                self.host_podman_resources.snapshot_connection_id =
                    Some(delivery.request.connection_id.clone());
                self.host_podman_resources.snapshot_running = false;
                self.host_podman_resources.resources_error = None;
                // Re-run a request that arrived while this capture was in
                // flight; dropping it would leave the panel sampling forever.
                if let Some(pending) = self.host_podman_resources.snapshot_pending.take() {
                    self.request_podman_capture(pending.connection_id, pending.kind, cx);
                }
                return;
            }
            HostPodmanCaptureKind::Stats => {
                self.host_podman_resources.stats =
                    Some(parse_podman_stats_snapshot(&succeeded_output));
                self.host_podman_resources.stats_in_flight = false;
            }
            HostPodmanCaptureKind::Search { .. } => {
                self.host_podman_resources.search_in_flight = false;
                let results = parse_podman_search_results(&succeeded_output);
                if let Some(dialog) = self.host_podman_resources.text_dialog.as_mut() {
                    dialog.loading = false;
                    dialog.output = Some(Arc::new(zeroize::Zeroizing::new(
                        results
                            .into_iter()
                            .map(|result| {
                                format!(
                                    "{} ★{} — {}",
                                    result.name, result.stars, result.description
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )));
                    dialog.error = None;
                }
            }
            HostPodmanCaptureKind::Inspect { .. }
            | HostPodmanCaptureKind::History { .. }
            | HostPodmanCaptureKind::Top { .. }
            | HostPodmanCaptureKind::Diff { .. }
            | HostPodmanCaptureKind::VolumeInspect { .. }
            | HostPodmanCaptureKind::NetworkInspect { .. }
            | HostPodmanCaptureKind::VolumeUsage { .. } => {
                if let Some(dialog) = self.host_podman_resources.text_dialog.as_mut() {
                    dialog.loading = false;
                    dialog.output = Some(Arc::new(zeroize::Zeroizing::new(succeeded_output)));
                    dialog.error = None;
                }
                self.host_podman_resources.text_dialog_running = None;
            }
        }
        cx.notify();
    }

    pub(super) fn podman_text_dialog(&self) -> Option<HostPodmanTextDialog> {
        self.host_podman_resources.text_dialog.clone()
    }

    pub(in crate::workspace::connection_monitor) fn dismiss_podman_text_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.host_podman_resources.text_dialog.take().is_some() {
            cx.notify();
        }
    }

    /// Opens the shared text-capture dialog and starts its bounded capture.
    pub(in crate::workspace::connection_monitor) fn open_podman_text_capture(
        &mut self,
        kind: HostPodmanCaptureKind,
        title: String,
        cx: &mut Context<Self>,
    ) {
        let Some(connection_id) = self.selected_connection_id_owned() else {
            return;
        };
        self.host_podman_resources.text_dialog = Some(HostPodmanTextDialog {
            title,
            output: None,
            error: None,
            loading: true,
            scroll_handle: gpui::ScrollHandle::new(),
        });
        self.request_podman_capture(connection_id, kind, cx);
    }

    pub(in crate::workspace::connection_monitor) fn open_podman_search(
        &mut self,
        term: String,
        cx: &mut Context<Self>,
    ) {
        let Some(connection_id) = self.selected_connection_id_owned() else {
            return;
        };
        self.host_podman_resources.text_dialog = Some(HostPodmanTextDialog {
            title: "Podman search".to_string(),
            output: None,
            error: None,
            loading: true,
            scroll_handle: gpui::ScrollHandle::new(),
        });
        self.request_podman_capture(connection_id, HostPodmanCaptureKind::Search { term }, cx);
    }

    // ---- resource actions ----

    pub(in crate::workspace::connection_monitor) fn request_podman_resource_action(
        &mut self,
        action: PodmanResourceAction,
        description: String,
        destructive: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(connection_id) = self.selected_connection_id_owned() else {
            return;
        };
        let request = HostPodmanResourceActionRequest {
            connection_id,
            description,
            action,
        };
        if destructive {
            if self.host_podman_resources.resource_action_running {
                cx.emit(HostToolsEvent::ShowNotice(
                    HostToolsNotice::PodmanActionAlreadyRunning,
                ));
                return;
            }
            HostToolConfirmState::open(&mut self.host_podman_resources.resource_confirm, request);
            cx.notify();
        } else {
            for notice in self.start_podman_resource_action(request, cx) {
                cx.emit(HostToolsEvent::ShowNotice(notice));
            }
        }
    }

    pub(in crate::workspace::connection_monitor) fn start_podman_resource_action(
        &mut self,
        request: HostPodmanResourceActionRequest,
        cx: &mut Context<Self>,
    ) -> Vec<HostToolsNotice> {
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            return vec![HostToolsNotice::PodmanConnectionMissing];
        };
        let Some(os_type) = self.connection_os_type(&request.connection_id) else {
            return vec![HostToolsNotice::PodmanConnectionMissing];
        };
        let command = match build_podman_resource_action_command(&os_type, request.action.clone()) {
            Ok(command) => command,
            Err(_) => return vec![HostToolsNotice::PodmanActionFailed],
        };
        self.host_podman_resources.resource_action_running = true;
        let spawned = self.spawn_podman_resource_action(
            command.command,
            request,
            Duration::from_secs(20),
            4096,
            runtime,
        );
        if !spawned {
            self.host_podman_resources.resource_action_running = false;
            return vec![HostToolsNotice::PodmanConnectionMissing];
        }
        cx.notify();
        Vec::new()
    }

    pub(in crate::workspace::connection_monitor) fn finish_host_podman_resource_action(
        &mut self,
        delivery: HostPodmanResourceActionDelivery,
        cx: &mut Context<Self>,
    ) {
        self.host_podman_resources.resource_action_running = false;
        cx.emit(HostToolsEvent::ShowNotice(
            HostToolsNotice::PodmanResourceActionFinished {
                description: delivery.request.description,
                succeeded: delivery.result.unwrap_or(false),
            },
        ));
        // Refresh the affected inventory so the panel never knowingly lies.
        let connection_id = delivery.request.connection_id;
        self.host_podman_resources.snapshot_running = false;
        self.request_podman_capture(connection_id, HostPodmanCaptureKind::Resources, cx);
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn open_podman_resource_confirm(
        &mut self,
        request: HostPodmanResourceActionRequest,
        cx: &mut Context<Self>,
    ) {
        if self.host_podman_resources.resource_action_running {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::PodmanActionAlreadyRunning,
            ));
            return;
        }
        HostToolConfirmState::open(&mut self.host_podman_resources.resource_confirm, request);
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn podman_resource_confirm_view(
        &self,
    ) -> Option<(
        HostPodmanResourceActionRequest,
        oxideterm_gpui_ui::motion::ExitPhase,
    )> {
        self.host_podman_resources
            .resource_confirm
            .as_ref()
            .map(|state| (state.request.clone(), state.presence.phase()))
    }

    pub(in crate::workspace::connection_monitor) fn dismiss_podman_resource_confirm(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.host_podman_resources.resource_confirm.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn begin_podman_resource_confirm_exit(
        &mut self,
        delay: Duration,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(generation) = self
            .host_podman_resources
            .resource_confirm
            .as_mut()
            .and_then(|state| state.presence.begin_exit())
        else {
            return false;
        };
        if delay.is_zero() {
            self.host_podman_resources.resource_confirm = None;
            cx.notify();
            return true;
        }
        cx.spawn(async move |weak, cx| {
            Timer::after(delay).await;
            let _ = weak.update(cx, |entity, cx| {
                if entity
                    .host_podman_resources
                    .resource_confirm
                    .as_ref()
                    .is_some_and(|state| state.presence.finish_exit(generation))
                {
                    entity.host_podman_resources.resource_confirm = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
        true
    }

    pub(super) fn confirm_podman_resource_action(
        &mut self,
        delay: Duration,
        cx: &mut Context<Self>,
    ) {
        let Some(request) = self
            .host_podman_resources
            .resource_confirm
            .as_ref()
            .map(|state| state.request.clone())
        else {
            return;
        };
        if !self.begin_podman_resource_confirm_exit(delay, cx) {
            return;
        }
        for notice in self.start_podman_resource_action(request, cx) {
            cx.emit(HostToolsEvent::ShowNotice(notice));
        }
    }

    // ---- input dialog ----

    pub(in crate::workspace::connection_monitor) fn open_podman_input_dialog(
        &mut self,
        kind: HostPodmanInputDialogKind,
        prefill: &str,
        cx: &mut Context<Self>,
    ) {
        self.ui.host_podman_input_dialog = Some(HostPodmanInputDialog {
            kind,
            value: zeroize::Zeroizing::new(prefill.to_string()),
        });
        self.ui.focus_input(HostToolsTextInput::PodmanDialog);
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn dismiss_podman_input_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.ui.host_podman_input_dialog.take().is_some() {
            self.ui.clear_input_focus();
            cx.notify();
        }
    }

    /// Routes a submitted dialog to a terminal dispatch or a direct action.
    /// Composed command lines never reach audit or notices; the terminal
    /// session opened for the user owns the stream.
    pub(in crate::workspace::connection_monitor) fn confirm_podman_input_dialog(
        &mut self,
        kind: HostPodmanInputDialogKind,
        value: String,
        opened_notice: String,
        missing_notice: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(connection_id) = self.selected_connection_id_owned() else {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::PodmanConnectionMissing,
            ));
            return;
        };
        let mut dispatch_terminal = |this: &Self,
                                     target_connection_id: &str,
                                     command: String,
                                     title: String,
                                     cx: &mut Context<Self>| {
            window.dispatch_action(
                Box::new(HostToolsWindowRequest::new(
                    HostToolsWindowIntent::OpenExistingNodeTerminal {
                        connection_id: target_connection_id.to_string(),
                        command,
                        title,
                        opened_notice: opened_notice.clone(),
                        missing_notice: missing_notice.clone(),
                    },
                )),
                cx,
            );
        };
        let direct = |this: &mut Self,
                      target_connection_id: &str,
                      description: String,
                      action: PodmanResourceAction,
                      cx: &mut Context<Self>| {
            for notice in this.start_podman_resource_action(
                HostPodmanResourceActionRequest {
                    connection_id: target_connection_id.to_string(),
                    description,
                    action,
                },
                cx,
            ) {
                cx.emit(HostToolsEvent::ShowNotice(notice));
            }
        };
        match kind {
            HostPodmanInputDialogKind::RunContainer => {
                match build_podman_terminal_command(PodmanTerminalOp::Run { args: value }) {
                    Ok(command) => dispatch_terminal(
                        self,
                        &connection_id,
                        command,
                        "podman run".to_string(),
                        cx,
                    ),
                    Err(_) => cx.emit(HostToolsEvent::ShowNotice(
                        HostToolsNotice::PodmanActionFailed,
                    )),
                }
            }
            HostPodmanInputDialogKind::RenameContainer { container_id } => {
                direct(
                    self,
                    &connection_id,
                    value.clone(),
                    PodmanResourceAction::RenameContainer {
                        container_id,
                        new_name: value,
                    },
                    cx,
                );
            }
            HostPodmanInputDialogKind::PullImage => {
                match build_podman_terminal_command(PodmanTerminalOp::Pull { reference: value }) {
                    Ok(command) => dispatch_terminal(
                        self,
                        &connection_id,
                        command,
                        "podman pull".to_string(),
                        cx,
                    ),
                    Err(_) => cx.emit(HostToolsEvent::ShowNotice(
                        HostToolsNotice::PodmanActionFailed,
                    )),
                }
            }
            HostPodmanInputDialogKind::PushImage => {
                match build_podman_terminal_command(PodmanTerminalOp::Push { reference: value }) {
                    Ok(command) => dispatch_terminal(
                        self,
                        &connection_id,
                        command,
                        "podman push".to_string(),
                        cx,
                    ),
                    Err(_) => cx.emit(HostToolsEvent::ShowNotice(
                        HostToolsNotice::PodmanActionFailed,
                    )),
                }
            }
            HostPodmanInputDialogKind::BuildImage => {
                match build_podman_terminal_command(PodmanTerminalOp::Build { args: value }) {
                    Ok(command) => dispatch_terminal(
                        self,
                        &connection_id,
                        command,
                        "podman build".to_string(),
                        cx,
                    ),
                    Err(_) => cx.emit(HostToolsEvent::ShowNotice(
                        HostToolsNotice::PodmanActionFailed,
                    )),
                }
            }
            HostPodmanInputDialogKind::TagImage { image_id } => {
                direct(
                    self,
                    &connection_id,
                    value.clone(),
                    PodmanResourceAction::TagImage {
                        image_id,
                        reference: value,
                    },
                    cx,
                );
            }
            HostPodmanInputDialogKind::SaveImage { image_id } => {
                match build_podman_terminal_command(PodmanTerminalOp::Save {
                    path: value,
                    reference: image_id,
                }) {
                    Ok(command) => dispatch_terminal(
                        self,
                        &connection_id,
                        command,
                        "podman save".to_string(),
                        cx,
                    ),
                    Err(_) => cx.emit(HostToolsEvent::ShowNotice(
                        HostToolsNotice::PodmanActionFailed,
                    )),
                }
            }
            HostPodmanInputDialogKind::LoadImage => {
                match build_podman_terminal_command(PodmanTerminalOp::Load { path: value }) {
                    Ok(command) => dispatch_terminal(
                        self,
                        &connection_id,
                        command,
                        "podman load".to_string(),
                        cx,
                    ),
                    Err(_) => cx.emit(HostToolsEvent::ShowNotice(
                        HostToolsNotice::PodmanActionFailed,
                    )),
                }
            }
            HostPodmanInputDialogKind::ImportImage => {
                let mut parts = value.split_whitespace();
                let path = parts.next().unwrap_or_default().to_string();
                let reference = parts.next().unwrap_or_default().to_string();
                match build_podman_terminal_command(PodmanTerminalOp::Import { path, reference }) {
                    Ok(command) => dispatch_terminal(
                        self,
                        &connection_id,
                        command,
                        "podman import".to_string(),
                        cx,
                    ),
                    Err(_) => cx.emit(HostToolsEvent::ShowNotice(
                        HostToolsNotice::PodmanActionFailed,
                    )),
                }
            }
            HostPodmanInputDialogKind::ExportContainer { container_id } => {
                match build_podman_terminal_command(PodmanTerminalOp::Export {
                    path: value,
                    container_id,
                }) {
                    Ok(command) => dispatch_terminal(
                        self,
                        &connection_id,
                        command,
                        "podman export".to_string(),
                        cx,
                    ),
                    Err(_) => cx.emit(HostToolsEvent::ShowNotice(
                        HostToolsNotice::PodmanActionFailed,
                    )),
                }
            }
            HostPodmanInputDialogKind::CopyFiles { container_id } => {
                let mut parts = value.split_whitespace();
                let source = parts.next().unwrap_or_default().to_string();
                let destination = parts.next().unwrap_or_default().to_string();
                match build_podman_terminal_command(PodmanTerminalOp::Copy {
                    source: format!("{container_id}:{source}"),
                    destination,
                }) {
                    Ok(command) => dispatch_terminal(
                        self,
                        &connection_id,
                        command,
                        "podman cp".to_string(),
                        cx,
                    ),
                    Err(_) => cx.emit(HostToolsEvent::ShowNotice(
                        HostToolsNotice::PodmanActionFailed,
                    )),
                }
            }
            HostPodmanInputDialogKind::CreateVolume => {
                direct(
                    self,
                    &connection_id,
                    value.clone(),
                    PodmanResourceAction::CreateVolume { name: value },
                    cx,
                );
            }
            HostPodmanInputDialogKind::CreateNetwork => {
                let mut parts = value.split_whitespace();
                let name = parts.next().unwrap_or_default().to_string();
                let driver = parts.next().unwrap_or_default().to_string();
                direct(
                    self,
                    &connection_id,
                    name.clone(),
                    PodmanResourceAction::CreateNetwork { name, driver },
                    cx,
                );
            }
            HostPodmanInputDialogKind::SearchImages => {
                self.open_podman_search(value, cx);
            }
            HostPodmanInputDialogKind::Login => {
                match build_podman_terminal_command(PodmanTerminalOp::Login { registry: value }) {
                    Ok(command) => dispatch_terminal(
                        self,
                        &connection_id,
                        command,
                        "podman login".to_string(),
                        cx,
                    ),
                    Err(_) => cx.emit(HostToolsEvent::ShowNotice(
                        HostToolsNotice::PodmanActionFailed,
                    )),
                }
            }
        }
        cx.notify();
    }

    // ---- tab switching ----

    pub(in crate::workspace::connection_monitor) fn select_podman_tab(
        &mut self,
        tab: HostPodmanTab,
        cx: &mut Context<Self>,
    ) {
        if self.ui.host_podman_tab == tab {
            return;
        }
        self.ui.host_podman_tab = tab;
        self.ui.host_podman_stats_visible = false;
        if matches!(
            tab,
            HostPodmanTab::Images | HostPodmanTab::Volumes | HostPodmanTab::Networks
        ) && !self.host_podman_resources.snapshot_running
            && self.host_podman_resources.images.is_none()
            && let Some(connection_id) = self.selected_connection_id_owned()
        {
            self.request_podman_capture(connection_id, HostPodmanCaptureKind::Resources, cx);
        }
        cx.notify();
    }
}

impl HostToolsEntity {
    pub(in crate::workspace::connection_monitor) fn render_podman_resources_body(
        &self,
        tab: HostPodmanTab,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match tab {
            HostPodmanTab::Images => {
                self.render_podman_images_panel(tokens, i18n, mono_font_family, selectable_text, cx)
            }
            HostPodmanTab::Volumes => self.render_podman_volumes_panel(
                tokens,
                i18n,
                mono_font_family,
                selectable_text,
                cx,
            ),
            HostPodmanTab::Networks => self.render_podman_networks_panel(
                tokens,
                i18n,
                mono_font_family,
                selectable_text,
                cx,
            ),
            HostPodmanTab::Containers => div().into_any_element(),
        }
    }

    fn podman_resources_stale(&self, connection_id: &str) -> bool {
        self.host_podman_resources.snapshot_connection_id.as_deref() != Some(connection_id)
    }

    pub(in crate::workspace::connection_monitor) fn request_podman_resources_if_needed(
        &mut self,
        connection_id: String,
        cx: &mut Context<Self>,
    ) {
        if self.podman_resources_stale(&connection_id)
            && !self.host_podman_resources.snapshot_running
        {
            self.request_podman_capture(connection_id, HostPodmanCaptureKind::Resources, cx);
        }
    }

    fn render_podman_resources_status_row(
        &self,
        count: usize,
        count_suffix_key: &'static str,
        buttons: Vec<AnyElement>,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let mut row = div()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .min_w_0()
            .text_size(px(11.0))
            .text_color(rgb(theme.text_muted))
            .child(
                div()
                    .flex_none()
                    .child(format!("{} {}", count, i18n.t(count_suffix_key))),
            );
        for button in buttons {
            row = row.child(button);
        }
        row.into_any_element()
    }

    fn podman_status_button(
        &self,
        icon: LucideIcon,
        label_key: &'static str,
        element_id: &'static str,
        disabled: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        on_click: impl Fn(&mut HostToolsEntity, &mut Context<HostToolsEntity>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        host_tools_tooltip_icon_button(
            tokens,
            icon,
            13.0,
            rgb(theme.text),
            oxideterm_gpui_ui::button::IconButtonOptions {
                size: 24.0,
                disabled,
                has_background: true,
                background: Some(rgb(theme.bg_hover)),
                hover_background: Some(rgb(theme.bg_panel)),
                idle_opacity: 1.0,
                ..oxideterm_gpui_ui::button::IconButtonOptions::compact(24.0)
            },
            i18n.t(label_key),
            element_id,
            true,
            cx.listener(move |host_tools, _event, _window, cx| {
                on_click(host_tools, cx);
                cx.stop_propagation();
            }),
        )
    }

    fn render_podman_images_panel(
        &self,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let Some(connection_id) = self.selected_connection_id_owned() else {
            return host_tools_center_state(
                LucideIcon::WifiOff,
                theme.text_muted,
                i18n.t("profiler.panel.no_connection"),
                selectable_text,
                cx,
            );
        };
        let stale = self.podman_resources_stale(&connection_id);
        let Some(snapshot) = self
            .host_podman_resources
            .images
            .as_ref()
            .filter(|_| !stale)
        else {
            if let Some(error) = self.host_podman_resources.resources_error.as_ref() {
                return host_tools_center_state(
                    LucideIcon::AlertTriangle,
                    MONITOR_RED,
                    error.clone(),
                    selectable_text,
                    cx,
                );
            }
            return host_tools_center_state(
                LucideIcon::Container,
                theme.text_muted,
                i18n.t("sidebar.host_podman.sampling"),
                selectable_text,
                cx,
            );
        };
        if snapshot.unavailable {
            return host_tools_center_state(
                LucideIcon::Container,
                theme.text_muted,
                i18n.t("sidebar.host_podman.unavailable"),
                selectable_text,
                cx,
            );
        }
        let status_row = self.render_podman_resources_status_row(
            snapshot.images.len(),
            "sidebar.host_podman.images.count_suffix",
            vec![
                self.podman_status_button(
                    LucideIcon::Scissors,
                    "sidebar.host_podman.actions.prune_dangling",
                    "host-podman-image-prune-dangling",
                    self.host_podman_resources.resource_action_running,
                    tokens,
                    i18n,
                    |host_tools, cx| {
                        host_tools.open_podman_resource_confirm(
                            HostPodmanResourceActionRequest {
                                connection_id: host_tools
                                    .selected_connection_id_owned()
                                    .unwrap_or_default(),
                                description: String::new(),
                                action: PodmanResourceAction::PruneDanglingImages,
                            },
                            cx,
                        );
                    },
                    cx,
                ),
                self.podman_status_button(
                    LucideIcon::Trash2,
                    "sidebar.host_podman.actions.prune_unused",
                    "host-podman-image-prune-unused",
                    self.host_podman_resources.resource_action_running,
                    tokens,
                    i18n,
                    |host_tools, cx| {
                        host_tools.open_podman_resource_confirm(
                            HostPodmanResourceActionRequest {
                                connection_id: host_tools
                                    .selected_connection_id_owned()
                                    .unwrap_or_default(),
                                description: String::new(),
                                action: PodmanResourceAction::PruneAllUnusedImages,
                            },
                            cx,
                        );
                    },
                    cx,
                ),
                self.podman_status_button(
                    LucideIcon::Download,
                    "sidebar.host_podman.actions.pull",
                    "host-podman-pull",
                    false,
                    tokens,
                    i18n,
                    |host_tools, cx| {
                        host_tools.open_podman_input_dialog(
                            HostPodmanInputDialogKind::PullImage,
                            "",
                            cx,
                        );
                    },
                    cx,
                ),
                self.podman_status_button(
                    LucideIcon::Upload,
                    "sidebar.host_podman.actions.push",
                    "host-podman-push",
                    false,
                    tokens,
                    i18n,
                    |host_tools, cx| {
                        host_tools.open_podman_input_dialog(
                            HostPodmanInputDialogKind::PushImage,
                            "",
                            cx,
                        );
                    },
                    cx,
                ),
                self.podman_status_button(
                    LucideIcon::Wrench,
                    "sidebar.host_podman.actions.build",
                    "host-podman-build",
                    false,
                    tokens,
                    i18n,
                    |host_tools, cx| {
                        host_tools.open_podman_input_dialog(
                            HostPodmanInputDialogKind::BuildImage,
                            "",
                            cx,
                        );
                    },
                    cx,
                ),
                self.podman_status_button(
                    LucideIcon::Search,
                    "sidebar.host_podman.actions.search",
                    "host-podman-search",
                    false,
                    tokens,
                    i18n,
                    |host_tools, cx| {
                        host_tools.open_podman_input_dialog(
                            HostPodmanInputDialogKind::SearchImages,
                            "",
                            cx,
                        );
                    },
                    cx,
                ),
                self.podman_status_button(
                    LucideIcon::RefreshCw,
                    "sidebar.host_podman.actions.refresh",
                    "host-podman-images-refresh",
                    self.host_podman_resources.snapshot_running,
                    tokens,
                    i18n,
                    |host_tools, cx| {
                        if let Some(connection_id) = host_tools.selected_connection_id_owned() {
                            host_tools.request_podman_capture(
                                connection_id,
                                HostPodmanCaptureKind::Resources,
                                cx,
                            );
                        }
                    },
                    cx,
                ),
            ],
            tokens,
            i18n,
            cx,
        );
        let rows = div()
            .id("host-podman-images-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .children(snapshot.images.iter().map(|image| {
                let image_id = image.image_id.clone();
                div()
                    .w_full()
                    .min_w_0()
                    .border_b_1()
                    .border_color(rgba((theme.border << 8) | MONITOR_BORDER_ALPHA))
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .px_3()
                            .pt_2()
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
                                    .font_family(mono_font_family.clone())
                                    .child(format!("{}:{}", image.repository, image.tag)),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(HOST_PROCESS_TABLE_VALUE_TEXT_SIZE))
                                    .text_color(rgb(theme.text_muted))
                                    .font_family(mono_font_family.clone())
                                    .child(image.size.clone()),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .px_3()
                            .pb_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .text_size(px(HOST_PROCESS_TABLE_META_TEXT_SIZE))
                                    .text_color(rgb(theme.text_muted))
                                    .font_family(mono_font_family.clone())
                                    .child(format!("{} · {}", image.image_id, image.created)),
                            )
                            .child(self.render_podman_image_actions(&image_id, tokens, i18n, cx)),
                    )
            }));
        let list = if snapshot.images.is_empty() {
            host_tools_center_state(
                LucideIcon::Container,
                theme.text_muted,
                i18n.t("sidebar.host_podman.images.empty"),
                selectable_text,
                cx,
            )
        } else {
            rows.into_any_element()
        };
        div()
            .w_full()
            .min_w_0()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .px_3()
            .pt_2()
            .child(status_row)
            .child(list)
            .into_any_element()
    }

    fn render_podman_image_actions(
        &self,
        image_id: &str,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let tag_id = image_id.to_string();
        let history_id = image_id.to_string();
        let inspect_id = image_id.to_string();
        let save_id = image_id.to_string();
        let remove_id = image_id.to_string();
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.0))
            .child(self.podman_inline_action_button(
                tokens,
                i18n,
                LucideIcon::Hash,
                "sidebar.host_podman.actions.tag",
                "host-podman-image-tag",
                move |host_tools, cx| {
                    let kind = HostPodmanInputDialogKind::TagImage {
                        image_id: tag_id.clone(),
                    };
                    host_tools.open_podman_input_dialog(kind, "", cx);
                },
                false,
                cx,
            ))
            .child(self.podman_inline_action_button(
                tokens,
                i18n,
                LucideIcon::History,
                "sidebar.host_podman.actions.history",
                "host-podman-image-history",
                move |host_tools, cx| {
                    let kind = HostPodmanCaptureKind::History {
                        image_id: history_id.clone(),
                    };
                    host_tools.open_podman_text_capture(kind, "podman history".to_string(), cx);
                },
                false,
                cx,
            ))
            .child(self.podman_inline_action_button(
                tokens,
                i18n,
                LucideIcon::Info,
                "sidebar.host_podman.actions.inspect",
                "host-podman-image-inspect",
                move |host_tools, cx| {
                    let kind = HostPodmanCaptureKind::Inspect {
                        reference: inspect_id.clone(),
                    };
                    host_tools.open_podman_text_capture(kind, "podman inspect".to_string(), cx);
                },
                false,
                cx,
            ))
            .child(self.podman_inline_action_button(
                tokens,
                i18n,
                LucideIcon::Save,
                "sidebar.host_podman.actions.save",
                "host-podman-image-save",
                move |host_tools, cx| {
                    let kind = HostPodmanInputDialogKind::SaveImage {
                        image_id: save_id.clone(),
                    };
                    host_tools.open_podman_input_dialog(kind, "", cx);
                },
                false,
                cx,
            ))
            .child(self.podman_inline_action_button(
                tokens,
                i18n,
                LucideIcon::Trash2,
                "sidebar.host_podman.actions.remove",
                "host-podman-image-remove",
                move |host_tools, cx| {
                    let action = PodmanResourceAction::RemoveImage {
                        image_id: remove_id.clone(),
                    };
                    host_tools.request_podman_resource_action(action, remove_id.clone(), true, cx);
                },
                true,
                cx,
            ))
            .into_any_element()
    }
}

impl HostToolsEntity {
    #[allow(clippy::too_many_arguments)]
    fn podman_inline_action_button<F>(
        &self,
        tokens: &ThemeTokens,
        i18n: &I18n,
        icon: LucideIcon,
        label_key: &'static str,
        element_id: &'static str,
        on_click: F,
        destructive: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement
    where
        F: Fn(&mut HostToolsEntity, &mut Context<HostToolsEntity>) + 'static,
    {
        let theme = tokens.ui;
        host_tools_tooltip_icon_button(
            tokens,
            icon,
            13.0,
            rgb(if destructive { MONITOR_RED } else { theme.text }),
            oxideterm_gpui_ui::button::IconButtonOptions {
                size: 22.0,
                has_background: true,
                background: Some(rgb(theme.bg_hover)),
                hover_background: Some(rgb(theme.bg_panel)),
                idle_opacity: 1.0,
                ..oxideterm_gpui_ui::button::IconButtonOptions::compact(22.0)
            },
            i18n.t(label_key),
            element_id,
            true,
            cx.listener(move |host_tools, _event, _window, cx| {
                on_click(host_tools, cx);
                cx.stop_propagation();
            }),
        )
    }

    fn podman_resource_status_row(
        &self,
        count: usize,
        count_suffix_key: &'static str,
        buttons: Vec<AnyElement>,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let mut row = div()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .min_w_0()
            .text_size(px(11.0))
            .text_color(rgb(theme.text_muted))
            .child(
                div()
                    .flex_none()
                    .child(format!("{} {}", count, i18n.t(count_suffix_key))),
            );
        for button in buttons {
            row = row.child(button);
        }
        row.into_any_element()
    }

    fn podman_action_button(
        &self,
        icon: LucideIcon,
        label_key: &'static str,
        element_id: &'static str,
        disabled: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        on_click: impl Fn(&mut HostToolsEntity, &mut Context<HostToolsEntity>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        host_tools_tooltip_icon_button(
            tokens,
            icon,
            13.0,
            rgb(theme.text),
            oxideterm_gpui_ui::button::IconButtonOptions {
                size: 24.0,
                disabled,
                has_background: true,
                background: Some(rgb(theme.bg_hover)),
                hover_background: Some(rgb(theme.bg_panel)),
                idle_opacity: 1.0,
                ..oxideterm_gpui_ui::button::IconButtonOptions::compact(24.0)
            },
            i18n.t(label_key),
            element_id,
            true,
            cx.listener(move |host_tools, _event, _window, cx| {
                on_click(host_tools, cx);
                cx.stop_propagation();
            }),
        )
    }

    fn render_podman_resources_header(
        &self,
        count: usize,
        count_suffix_key: &'static str,
        buttons: Vec<AnyElement>,
        tokens: &ThemeTokens,
        i18n: &I18n,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        div()
            .w_full()
            .min_w_0()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .px_3()
            .pt_2()
            .child(self.podman_resource_status_row(
                count,
                count_suffix_key,
                buttons,
                tokens,
                i18n,
                cx,
            ))
            .when(count == 0, |section| {
                section.child(host_tools_center_state(
                    LucideIcon::Container,
                    theme.text_muted,
                    i18n.t("sidebar.host_podman.resources_empty"),
                    selectable_text,
                    cx,
                ))
            })
            .into_any_element()
    }

    fn render_podman_volumes_panel(
        &self,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let Some(connection_id) = self.selected_connection_id_owned() else {
            return host_tools_center_state(
                LucideIcon::WifiOff,
                theme.text_muted,
                i18n.t("profiler.panel.no_connection"),
                selectable_text,
                cx,
            );
        };
        let stale = self.podman_resources_stale(&connection_id);
        let Some(snapshot) = self
            .host_podman_resources
            .volumes
            .as_ref()
            .filter(|_| !stale)
        else {
            if let Some(error) = self.host_podman_resources.resources_error.as_ref() {
                return host_tools_center_state(
                    LucideIcon::AlertTriangle,
                    MONITOR_RED,
                    error.clone(),
                    selectable_text,
                    cx,
                );
            }
            return host_tools_center_state(
                LucideIcon::Container,
                theme.text_muted,
                i18n.t("sidebar.host_podman.sampling"),
                selectable_text,
                cx,
            );
        };
        if snapshot.unavailable {
            return host_tools_center_state(
                LucideIcon::Container,
                theme.text_muted,
                i18n.t("sidebar.host_podman.unavailable"),
                selectable_text,
                cx,
            );
        }
        let create_id = String::new();
        let prune = String::new();
        let refresh = connection_id.clone();
        let status_row = self.podman_resource_status_row(
            snapshot.volumes.len(),
            "sidebar.host_podman.volumes.count_suffix",
            vec![
                self.podman_action_button(
                    LucideIcon::Plus,
                    "sidebar.host_podman.actions.create",
                    "host-podman-volume-create",
                    false,
                    tokens,
                    i18n,
                    move |host_tools, cx| {
                        let _ = create_id;
                        host_tools.open_podman_input_dialog(
                            HostPodmanInputDialogKind::CreateVolume,
                            "",
                            cx,
                        );
                    },
                    cx,
                ),
                self.podman_action_button(
                    LucideIcon::Trash2,
                    "sidebar.host_podman.actions.prune",
                    "host-podman-volume-prune",
                    self.host_podman_resources.resource_action_running,
                    tokens,
                    i18n,
                    move |host_tools, cx| {
                        let _ = prune;
                        host_tools.open_podman_resource_confirm(
                            HostPodmanResourceActionRequest {
                                connection_id: host_tools
                                    .selected_connection_id_owned()
                                    .unwrap_or_default(),
                                description: String::new(),
                                action: PodmanResourceAction::PruneVolumes,
                            },
                            cx,
                        );
                    },
                    cx,
                ),
                self.podman_action_button(
                    LucideIcon::RefreshCw,
                    "sidebar.host_podman.actions.refresh",
                    "host-podman-volumes-refresh",
                    self.host_podman_resources.snapshot_running,
                    tokens,
                    i18n,
                    move |host_tools, cx| {
                        let _ = refresh;
                        if let Some(connection_id) = host_tools.selected_connection_id_owned() {
                            host_tools.request_podman_capture(
                                connection_id,
                                HostPodmanCaptureKind::Resources,
                                cx,
                            );
                        }
                    },
                    cx,
                ),
            ],
            tokens,
            i18n,
            cx,
        );
        let rows = snapshot.volumes.iter().map(|volume| {
            let volume_name = volume.name.clone();
            div()
                .w_full()
                .min_w_0()
                .border_b_1()
                .border_color(rgba((theme.border << 8) | MONITOR_BORDER_ALPHA))
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .px_3()
                        .pt_2()
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
                                .font_family(mono_font_family.clone())
                                .child(volume.name.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(HOST_PROCESS_TABLE_VALUE_TEXT_SIZE))
                                .text_color(rgb(theme.text_muted))
                                .child(volume.driver.clone()),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .px_3()
                        .pb_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .text_size(px(HOST_PROCESS_TABLE_META_TEXT_SIZE))
                                .text_color(rgb(theme.text_muted))
                                .font_family(mono_font_family.clone())
                                .child(volume.mount_point.clone()),
                        )
                        .child(self.podman_volume_actions(&volume_name, tokens, i18n, cx)),
                )
        });
        let list = div()
            .id("host-podman-volumes-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .children(rows);
        div()
            .w_full()
            .min_w_0()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(status_row)
            .child(list)
            .into_any_element()
    }

    fn podman_volume_actions(
        &self,
        volume_name: &str,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let inspect_name = volume_name.to_string();
        let usage_name = volume_name.to_string();
        let remove_name = volume_name.to_string();
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.0))
            .child(self.podman_inline_action_button(
                tokens,
                i18n,
                LucideIcon::Info,
                "sidebar.host_podman.actions.inspect",
                "host-podman-volume-inspect",
                move |host_tools, cx| {
                    let kind = HostPodmanCaptureKind::VolumeInspect {
                        name: inspect_name.clone(),
                    };
                    host_tools.open_podman_text_capture(
                        kind,
                        "podman volume inspect".to_string(),
                        cx,
                    );
                },
                false,
                cx,
            ))
            .child(self.podman_inline_action_button(
                tokens,
                i18n,
                LucideIcon::ListTree,
                "sidebar.host_podman.actions.usage",
                "host-podman-volume-usage",
                move |host_tools, cx| {
                    let kind = HostPodmanCaptureKind::VolumeUsage {
                        name: usage_name.clone(),
                    };
                    host_tools.open_podman_text_capture(kind, "podman ps -a".to_string(), cx);
                },
                false,
                cx,
            ))
            .child(self.podman_inline_action_button(
                tokens,
                i18n,
                LucideIcon::Trash2,
                "sidebar.host_podman.actions.remove",
                "host-podman-volume-remove",
                move |host_tools, cx| {
                    let action = PodmanResourceAction::RemoveVolume {
                        name: remove_name.clone(),
                    };
                    host_tools.request_podman_resource_action(
                        action,
                        remove_name.clone(),
                        true,
                        cx,
                    );
                },
                true,
                cx,
            ))
            .into_any_element()
    }

    fn render_podman_networks_panel(
        &self,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let Some(connection_id) = self.selected_connection_id_owned() else {
            return host_tools_center_state(
                LucideIcon::WifiOff,
                theme.text_muted,
                i18n.t("profiler.panel.no_connection"),
                selectable_text,
                cx,
            );
        };
        let stale = self.podman_resources_stale(&connection_id);
        let Some(snapshot) = self
            .host_podman_resources
            .networks
            .as_ref()
            .filter(|_| !stale)
        else {
            if let Some(error) = self.host_podman_resources.resources_error.as_ref() {
                return host_tools_center_state(
                    LucideIcon::AlertTriangle,
                    MONITOR_RED,
                    error.clone(),
                    selectable_text,
                    cx,
                );
            }
            return host_tools_center_state(
                LucideIcon::Container,
                theme.text_muted,
                i18n.t("sidebar.host_podman.sampling"),
                selectable_text,
                cx,
            );
        };
        if snapshot.unavailable {
            return host_tools_center_state(
                LucideIcon::Container,
                theme.text_muted,
                i18n.t("sidebar.host_podman.unavailable"),
                selectable_text,
                cx,
            );
        }
        let refresh = connection_id.clone();
        let status_row = self.podman_resource_status_row(
            snapshot.networks.len(),
            "sidebar.host_podman.networks.count_suffix",
            vec![
                self.podman_action_button(
                    LucideIcon::Plus,
                    "sidebar.host_podman.actions.create",
                    "host-podman-network-create",
                    false,
                    tokens,
                    i18n,
                    |host_tools, cx| {
                        host_tools.open_podman_input_dialog(
                            HostPodmanInputDialogKind::CreateNetwork,
                            "",
                            cx,
                        );
                    },
                    cx,
                ),
                self.podman_action_button(
                    LucideIcon::RefreshCw,
                    "sidebar.host_podman.actions.refresh",
                    "host-podman-networks-refresh",
                    self.host_podman_resources.snapshot_running,
                    tokens,
                    i18n,
                    move |host_tools, cx| {
                        let _ = refresh;
                        if let Some(connection_id) = host_tools.selected_connection_id_owned() {
                            host_tools.request_podman_capture(
                                connection_id,
                                HostPodmanCaptureKind::Resources,
                                cx,
                            );
                        }
                    },
                    cx,
                ),
            ],
            tokens,
            i18n,
            cx,
        );
        let rows = snapshot.networks.iter().map(|network| {
            let network_name = network.name.clone();
            let subnets = if network.subnets.is_empty() {
                "-".to_string()
            } else {
                network.subnets.join(", ")
            };
            div()
                .w_full()
                .min_w_0()
                .border_b_1()
                .border_color(rgba((theme.border << 8) | MONITOR_BORDER_ALPHA))
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .px_3()
                        .pt_2()
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
                                .font_family(mono_font_family.clone())
                                .child(network.name.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(HOST_PROCESS_TABLE_VALUE_TEXT_SIZE))
                                .text_color(rgb(theme.text_muted))
                                .font_family(mono_font_family.clone())
                                .child(format!("{} · {}", network.driver, network.interface)),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .px_3()
                        .pb_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .text_size(px(HOST_PROCESS_TABLE_META_TEXT_SIZE))
                                .text_color(rgb(theme.text_muted))
                                .font_family(mono_font_family.clone())
                                .child(subnets),
                        )
                        .child(self.podman_network_actions(&network_name, tokens, i18n, cx)),
                )
        });
        let list = div()
            .id("host-podman-networks-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .children(rows);
        div()
            .w_full()
            .min_w_0()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(status_row)
            .child(list)
            .into_any_element()
    }

    fn podman_network_actions(
        &self,
        network_name: &str,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let inspect_name = network_name.to_string();
        let remove_name = network_name.to_string();
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.0))
            .child(self.podman_inline_action_button(
                tokens,
                i18n,
                LucideIcon::Info,
                "sidebar.host_podman.actions.inspect",
                "host-podman-network-inspect",
                move |host_tools, cx| {
                    let kind = HostPodmanCaptureKind::NetworkInspect {
                        name: inspect_name.clone(),
                    };
                    host_tools.open_podman_text_capture(
                        kind,
                        "podman network inspect".to_string(),
                        cx,
                    );
                },
                false,
                cx,
            ))
            .child(self.podman_inline_action_button(
                tokens,
                i18n,
                LucideIcon::Trash2,
                "sidebar.host_podman.actions.remove",
                "host-podman-network-remove",
                move |host_tools, cx| {
                    let action = PodmanResourceAction::RemoveNetwork {
                        name: remove_name.clone(),
                    };
                    host_tools.request_podman_resource_action(
                        action,
                        remove_name.clone(),
                        true,
                        cx,
                    );
                },
                true,
                cx,
            ))
            .into_any_element()
    }

    pub(in crate::workspace::connection_monitor) fn render_podman_stats_view(
        &self,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let status_row = self.podman_resource_status_row(
            self.host_podman_resources
                .stats
                .as_ref()
                .map(|snapshot| snapshot.stats.len())
                .unwrap_or_default(),
            "sidebar.host_podman.stats.count_suffix",
            vec![self.podman_action_button(
                LucideIcon::RefreshCw,
                "sidebar.host_podman.actions.refresh",
                "host-podman-stats-refresh",
                self.host_podman_resources.stats_in_flight,
                tokens,
                i18n,
                |host_tools, cx| {
                    if let Some(connection_id) = host_tools.selected_connection_id_owned() {
                        host_tools.request_podman_capture(
                            connection_id,
                            HostPodmanCaptureKind::Stats,
                            cx,
                        );
                    }
                },
                cx,
            )],
            tokens,
            i18n,
            cx,
        );
        let Some(snapshot) = self.host_podman_resources.stats.as_ref() else {
            return div()
                .w_full()
                .min_w_0()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .overflow_hidden()
                .px_3()
                .pt_2()
                .child(status_row)
                .child(host_tools_center_state(
                    LucideIcon::Activity,
                    theme.text_muted,
                    i18n.t("sidebar.host_podman.sampling"),
                    selectable_text,
                    cx,
                ))
                .into_any_element();
        };
        let rows = div()
            .id("host-podman-stats-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .children(snapshot.stats.iter().map(|stat| {
                div()
                    .w_full()
                    .min_w_0()
                    .border_b_1()
                    .border_color(rgba((theme.border << 8) | MONITOR_BORDER_ALPHA))
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .px_3()
                            .py_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_none()
                                    .w(px(110.0))
                                    .truncate()
                                    .text_size(px(HOST_PROCESS_TABLE_COMMAND_TEXT_SIZE))
                                    .text_color(rgb(theme.text))
                                    .font_family(mono_font_family.clone())
                                    .child(stat.name.clone()),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .text_size(px(HOST_PROCESS_TABLE_VALUE_TEXT_SIZE))
                                    .text_color(rgb(theme.text_muted))
                                    .font_family(mono_font_family.clone())
                                    .child(format!(
                                        "CPU {} · MEM {} ({} ) · NET {} · IO {}",
                                        stat.cpu_percent,
                                        stat.mem_usage,
                                        stat.mem_percent,
                                        stat.net_io,
                                        stat.block_io
                                    )),
                            ),
                    )
            }));
        div()
            .w_full()
            .min_w_0()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(status_row)
            .child(rows)
            .into_any_element()
    }
}

impl WorkspaceApp {
    pub(in crate::workspace) fn submit_host_podman_input_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (kind, value) = {
            let host_tools = self.host_tools.read(cx);
            let Some(dialog) = host_tools.ui.host_podman_input_dialog.as_ref() else {
                return;
            };
            (dialog.kind.clone(), dialog.value.to_string())
        };
        let opened_notice = self.i18n.t("sidebar.host_podman.toast.terminal_opened");
        let missing_notice = self.i18n.t("sidebar.host_podman.toast.connection_missing");
        self.ime_marked_text = None;
        self.clear_ime_selection();
        self.host_tools.update(cx, |host_tools, cx| {
            host_tools.dismiss_podman_input_dialog(cx);
            host_tools.confirm_podman_input_dialog(
                kind,
                value,
                opened_notice,
                missing_notice,
                window,
                cx,
            );
        });
    }

    pub(in crate::workspace) fn render_host_podman_resource_confirm_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (request, phase) = self.host_tools.read(cx).podman_resource_confirm_view()?;
        let description = self
            .i18n
            .t(host_podman_resource_confirm_description_key(
                &request.action,
            ))
            .replace("{{name}}", &request.description);
        let exit_delay = oxideterm_gpui_ui::motion::duration(
            &self.tokens,
            oxideterm_gpui_ui::motion::MotionDuration::Control,
        );
        let confirm_delay = exit_delay;
        Some(
            oxideterm_gpui_ui::confirm::confirm_dialog_with_focus_motion(
                &self.tokens,
                "host-podman-resource-confirm-motion",
                phase,
                ConfirmDialogView {
                    variant: ConfirmDialogVariant::Danger,
                    title: div()
                        .child(self.i18n.t("sidebar.host_podman.confirm.title"))
                        .into_any_element(),
                    description: Some(div().child(description).into_any_element()),
                    cancel_label: div()
                        .child(self.i18n.t("sidebar.host_podman.confirm.cancel"))
                        .into_any_element(),
                    confirm_label: div()
                        .child(
                            self.i18n
                                .t(host_podman_resource_confirm_label_key(&request.action)),
                        )
                        .into_any_element(),
                },
                self.standard_confirm_focus(),
                cx.listener(|this, _event, _window, cx| {
                    let delay = oxideterm_gpui_ui::motion::duration(
                        &this.tokens,
                        oxideterm_gpui_ui::motion::MotionDuration::Control,
                    );
                    this.host_tools.update(cx, |host_tools, cx| {
                        host_tools.begin_podman_resource_confirm_exit(delay, cx);
                    });
                }),
                cx.listener(|this, _event, _window, cx| {
                    let delay = oxideterm_gpui_ui::motion::duration(
                        &this.tokens,
                        oxideterm_gpui_ui::motion::MotionDuration::Control,
                    );
                    this.host_tools.update(cx, |host_tools, cx| {
                        host_tools.confirm_podman_resource_action(delay, cx);
                    });
                }),
            )
            .into_any_element(),
        )
    }
}

fn host_podman_resource_confirm_description_key(action: &PodmanResourceAction) -> &'static str {
    match action {
        PodmanResourceAction::RemoveImage { .. } => "sidebar.host_podman.confirms.remove_image",
        PodmanResourceAction::PruneDanglingImages => {
            "sidebar.host_podman.confirms.prune_dangling_images"
        }
        PodmanResourceAction::PruneAllUnusedImages => {
            "sidebar.host_podman.confirms.prune_unused_images"
        }
        PodmanResourceAction::TagImage { .. } => "sidebar.host_podman.confirms.tag_image",
        PodmanResourceAction::RenameContainer { .. } => "sidebar.host_podman.confirms.rename",
        PodmanResourceAction::RunHealthcheck { .. } => "sidebar.host_podman.confirms.healthcheck",
        PodmanResourceAction::CreateVolume { .. } => "sidebar.host_podman.confirms.create_volume",
        PodmanResourceAction::RemoveVolume { .. } => "sidebar.host_podman.confirms.remove_volume",
        PodmanResourceAction::PruneVolumes => "sidebar.host_podman.confirms.prune_volumes",
        PodmanResourceAction::CreateNetwork { .. } => "sidebar.host_podman.confirms.create_network",
        PodmanResourceAction::RemoveNetwork { .. } => "sidebar.host_podman.confirms.remove_network",
        PodmanResourceAction::Logout { .. } => "sidebar.host_podman.confirms.logout",
    }
}

fn host_podman_resource_confirm_label_key(action: &PodmanResourceAction) -> &'static str {
    match action {
        PodmanResourceAction::RemoveImage { .. } => "sidebar.host_podman.actions.remove",
        PodmanResourceAction::PruneDanglingImages => "sidebar.host_podman.actions.prune_dangling",
        PodmanResourceAction::PruneAllUnusedImages => "sidebar.host_podman.actions.prune_unused",
        PodmanResourceAction::TagImage { .. } => "sidebar.host_podman.actions.tag",
        PodmanResourceAction::RenameContainer { .. } => "sidebar.host_podman.actions.rename",
        PodmanResourceAction::RunHealthcheck { .. } => "sidebar.host_podman.actions.healthcheck",
        PodmanResourceAction::CreateVolume { .. } => "sidebar.host_podman.actions.create",
        PodmanResourceAction::RemoveVolume { .. } => "sidebar.host_podman.actions.remove",
        PodmanResourceAction::PruneVolumes => "sidebar.host_podman.actions.prune",
        PodmanResourceAction::CreateNetwork { .. } => "sidebar.host_podman.actions.create",
        PodmanResourceAction::RemoveNetwork { .. } => "sidebar.host_podman.actions.remove",
        PodmanResourceAction::Logout { .. } => "sidebar.host_podman.inputs.login_title",
    }
}

pub(in crate::workspace::connection_monitor) fn host_podman_input_title_key(
    kind: &HostPodmanInputDialogKind,
) -> &'static str {
    match kind {
        HostPodmanInputDialogKind::RunContainer => "sidebar.host_podman.inputs.run_title",
        HostPodmanInputDialogKind::RenameContainer { .. } => {
            "sidebar.host_podman.inputs.rename_title"
        }
        HostPodmanInputDialogKind::PullImage => "sidebar.host_podman.inputs.pull_title",
        HostPodmanInputDialogKind::PushImage => "sidebar.host_podman.inputs.push_title",
        HostPodmanInputDialogKind::BuildImage => "sidebar.host_podman.inputs.build_title",
        HostPodmanInputDialogKind::TagImage { .. } => "sidebar.host_podman.inputs.tag_title",
        HostPodmanInputDialogKind::SaveImage { .. } => "sidebar.host_podman.inputs.save_title",
        HostPodmanInputDialogKind::LoadImage => "sidebar.host_podman.inputs.load_title",
        HostPodmanInputDialogKind::ImportImage => "sidebar.host_podman.inputs.import_title",
        HostPodmanInputDialogKind::ExportContainer { .. } => {
            "sidebar.host_podman.inputs.export_title"
        }
        HostPodmanInputDialogKind::CopyFiles { .. } => "sidebar.host_podman.inputs.copy_title",
        HostPodmanInputDialogKind::CreateVolume => "sidebar.host_podman.inputs.volume_title",
        HostPodmanInputDialogKind::CreateNetwork => "sidebar.host_podman.inputs.network_title",
        HostPodmanInputDialogKind::SearchImages => "sidebar.host_podman.inputs.search_title",
        HostPodmanInputDialogKind::Login => "sidebar.host_podman.inputs.login_title",
    }
}

pub(in crate::workspace::connection_monitor) fn host_podman_input_placeholder_key(
    kind: &HostPodmanInputDialogKind,
) -> &'static str {
    match kind {
        HostPodmanInputDialogKind::RunContainer => "sidebar.host_podman.inputs.run_placeholder",
        HostPodmanInputDialogKind::RenameContainer { .. } => {
            "sidebar.host_podman.inputs.rename_placeholder"
        }
        HostPodmanInputDialogKind::PullImage => "sidebar.host_podman.inputs.pull_placeholder",
        HostPodmanInputDialogKind::PushImage => "sidebar.host_podman.inputs.push_placeholder",
        HostPodmanInputDialogKind::BuildImage => "sidebar.host_podman.inputs.build_placeholder",
        HostPodmanInputDialogKind::TagImage { .. } => "sidebar.host_podman.inputs.tag_placeholder",
        HostPodmanInputDialogKind::SaveImage { .. } => {
            "sidebar.host_podman.inputs.save_placeholder"
        }
        HostPodmanInputDialogKind::LoadImage => "sidebar.host_podman.inputs.load_placeholder",
        HostPodmanInputDialogKind::ImportImage => "sidebar.host_podman.inputs.import_placeholder",
        HostPodmanInputDialogKind::ExportContainer { .. } => {
            "sidebar.host_podman.inputs.export_placeholder"
        }
        HostPodmanInputDialogKind::CopyFiles { .. } => {
            "sidebar.host_podman.inputs.copy_placeholder"
        }
        HostPodmanInputDialogKind::CreateVolume => "sidebar.host_podman.inputs.volume_placeholder",
        HostPodmanInputDialogKind::CreateNetwork => {
            "sidebar.host_podman.inputs.network_placeholder"
        }
        HostPodmanInputDialogKind::SearchImages => "sidebar.host_podman.inputs.search_placeholder",
        HostPodmanInputDialogKind::Login => "sidebar.host_podman.inputs.login_placeholder",
    }
}

pub(in crate::workspace::connection_monitor) fn host_podman_input_submit_key(
    kind: &HostPodmanInputDialogKind,
) -> &'static str {
    match kind {
        HostPodmanInputDialogKind::SearchImages => "sidebar.host_podman.actions.search",
        _ => "sidebar.host_podman.confirm.run",
    }
}

impl HostToolsEntity {
    pub(in crate::workspace) fn render_podman_tab_bar(
        &self,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tab = self.ui.host_podman_tab;
        let tabs = [
            (
                HostPodmanTab::Containers,
                "sidebar.host_podman.tabs.containers",
            ),
            (HostPodmanTab::Images, "sidebar.host_podman.tabs.images"),
            (HostPodmanTab::Volumes, "sidebar.host_podman.tabs.volumes"),
            (HostPodmanTab::Networks, "sidebar.host_podman.tabs.networks"),
        ];
        let active_index = tabs
            .iter()
            .position(|(candidate, _)| *candidate == tab)
            .unwrap_or_default();
        let items = tabs
            .iter()
            .map(|(candidate, label_key)| {
                self.render_podman_tab_item(tokens, i18n, *candidate, label_key, cx)
            })
            .collect::<Vec<_>>();
        oxideterm_gpui_ui::segmented_control(
            tokens,
            "host-podman-tab-switcher",
            oxideterm_gpui_ui::SegmentedControlOptions::new(active_index, active_index, tabs.len())
                .compact(240.0),
            items,
        )
        .into_any_element()
    }

    fn render_podman_tab_item(
        &self,
        tokens: &ThemeTokens,
        i18n: &I18n,
        tab: HostPodmanTab,
        label_key: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self.ui.host_podman_tab == tab;
        let content = div()
            .w_full()
            .py(px(2.0))
            .flex()
            .items_center()
            .justify_center()
            .child(i18n.t(label_key));
        oxideterm_gpui_ui::segmented_control_item_content(
            tokens,
            active,
            content.into_any_element(),
        )
        .text_size(px(11.0))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |host_tools, _event, _window, cx| {
                host_tools.select_podman_tab(tab, cx);
                cx.stop_propagation();
                cx.notify();
            }),
        )
        .into_any_element()
    }
}
