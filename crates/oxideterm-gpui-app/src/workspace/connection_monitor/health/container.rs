//! Unified Container tool: engine detection, instance switching, the shared
//! inventory lists and the confirm/capture dialogs.
//!
//! Engine and transport details live in `oxideterm-container`; this module only
//! renders the unified inventory and forwards composed commands.

use super::*;

use oxideterm_container::{
    ContainerCapture, ContainerInstance, ContainerStatus, container_action_availability,
    pod_action_availability,
};
use oxideterm_gpui_ui::button::{ButtonVariant, IconButtonOptions};

const HOST_CONTAINER_ACTION_TIMEOUT: Duration = Duration::from_secs(20);
const HOST_CONTAINER_ACTION_MAX_OUTPUT: usize = 4096;
const HOST_CONTAINER_CAPTURE_TIMEOUT: Duration = Duration::from_secs(30);
const HOST_CONTAINER_CAPTURE_MAX_OUTPUT: usize = 256 * 1024;

/// Clamped resize for the capture dialog's corner handle.
pub(super) fn resized_container_text_dialog_size(
    start: (f32, f32),
    delta: (f32, f32),
) -> (f32, f32) {
    (
        (start.0 + delta.0).clamp(
            HOST_CONTAINER_CAPTURE_DIALOG_MIN_WIDTH,
            HOST_CONTAINER_CAPTURE_DIALOG_MAX_WIDTH,
        ),
        (start.1 + delta.1).clamp(
            HOST_CONTAINER_CAPTURE_DIALOG_MIN_HEIGHT,
            HOST_CONTAINER_CAPTURE_DIALOG_MAX_HEIGHT,
        ),
    )
}

fn container_state_color(tokens: &ThemeTokens, state: &str) -> u32 {
    match state.trim().to_ascii_lowercase().as_str() {
        "running" => MONITOR_EMERALD,
        "paused" | "restarting" | "degraded" => MONITOR_AMBER,
        "dead" | "removing" => MONITOR_RED,
        _ => tokens.ui.text_muted,
    }
}

fn kind_is_destructive(kind: HostContainerConfirmKind) -> bool {
    match kind {
        HostContainerConfirmKind::Container(action) => action.is_destructive(),
        HostContainerConfirmKind::Image(action) => action.is_destructive(),
        HostContainerConfirmKind::Volume(action) => action.is_destructive(),
        HostContainerConfirmKind::Network(action) => action.is_destructive(),
        HostContainerConfirmKind::Pod(action) => action.is_destructive(),
        HostContainerConfirmKind::Raw => false,
    }
}

fn confirm_description_key(kind: HostContainerConfirmKind) -> &'static str {
    use oxideterm_container as container;
    match kind {
        HostContainerConfirmKind::Container(action) => match action {
            container::ContainerAction::Start => "sidebar.host_container.confirms.start",
            container::ContainerAction::Stop => "sidebar.host_container.confirms.stop",
            container::ContainerAction::Restart => "sidebar.host_container.confirms.restart",
            container::ContainerAction::Pause => "sidebar.host_container.confirms.pause",
            container::ContainerAction::Unpause => "sidebar.host_container.confirms.unpause",
            container::ContainerAction::Remove => "sidebar.host_container.confirms.remove",
        },
        HostContainerConfirmKind::Image(action) => match action {
            container::ImageAction::Remove => "sidebar.host_container.confirms.remove_image",
            container::ImageAction::Prune => "sidebar.host_container.confirms.prune_images",
        },
        HostContainerConfirmKind::Volume(action) => match action {
            container::VolumeAction::Remove => "sidebar.host_container.confirms.remove_volume",
            container::VolumeAction::Prune => "sidebar.host_container.confirms.prune_volumes",
        },
        HostContainerConfirmKind::Network(action) => match action {
            container::NetworkAction::Remove => "sidebar.host_container.confirms.remove_network",
            container::NetworkAction::Prune => "sidebar.host_container.confirms.prune_networks",
        },
        HostContainerConfirmKind::Pod(action) => match action {
            container::PodAction::Start => "sidebar.host_container.confirms.pod_start",
            container::PodAction::Stop => "sidebar.host_container.confirms.pod_stop",
            container::PodAction::Restart => "sidebar.host_container.confirms.pod_restart",
            container::PodAction::Pause => "sidebar.host_container.confirms.pod_pause",
            container::PodAction::Unpause => "sidebar.host_container.confirms.pod_unpause",
            container::PodAction::Remove => "sidebar.host_container.confirms.pod_remove",
        },
        HostContainerConfirmKind::Raw => "sidebar.host_container.confirm.title",
    }
}

fn confirm_label_key(kind: HostContainerConfirmKind) -> &'static str {
    match kind {
        HostContainerConfirmKind::Container(action) => action.label_key(),
        HostContainerConfirmKind::Image(action) => action.label_key(),
        HostContainerConfirmKind::Volume(action) => action.label_key(),
        HostContainerConfirmKind::Network(action) => action.label_key(),
        HostContainerConfirmKind::Pod(action) => action.label_key(),
        HostContainerConfirmKind::Raw => "sidebar.host_container.confirm.title",
    }
}

/// A row in the shared virtual list. One list serves every inventory tab.
#[derive(Clone)]
enum ContainerRow {
    Container(oxideterm_container::Container),
    Image(oxideterm_container::Image),
    Volume(oxideterm_container::Volume),
    Network(oxideterm_container::Network),
    Pod(oxideterm_container::Pod),
}

/// Header action for the current inventory tab.
#[derive(Clone)]
enum ContainerCountAction {
    Input(HostContainerInputKind),
    /// `true` also removes unused tagged images.
    PruneImages(bool),
    Confirm(HostContainerConfirmKind),
}

/// Row identity for the virtual list: sampling updates must not reset scroll.
fn container_row_signature(row: &ContainerRow) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    match row {
        ContainerRow::Container(container) => {
            0u8.hash(&mut hasher);
            container.id.hash(&mut hasher);
        }
        ContainerRow::Image(image) => {
            1u8.hash(&mut hasher);
            image.id.hash(&mut hasher);
        }
        ContainerRow::Volume(volume) => {
            2u8.hash(&mut hasher);
            volume.name.hash(&mut hasher);
        }
        ContainerRow::Network(network) => {
            3u8.hash(&mut hasher);
            network.name.hash(&mut hasher);
        }
        ContainerRow::Pod(pod) => {
            4u8.hash(&mut hasher);
            pod.id.hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Keeps the virtual list's item count in step with the rendered rows; without
/// this the list stays empty while the count label already shows the total.
fn sync_container_list_state(
    state: &ListState,
    cache: &mut VirtualListSignatureCache,
    identity: &str,
    rows: &[ContainerRow],
) {
    let signatures = rows.iter().map(container_row_signature).collect::<Vec<_>>();
    sync_tauri_variable_list_state_by_signatures(
        state,
        cache,
        identity,
        &signatures,
        TauriVirtualListSpec::new(px(64.0), 6),
    );
}

struct HostContainerPanelSnapshot {
    connections: Vec<MonitorConnectionOption>,
    selected_connection_id: String,
    instances: Vec<ContainerInstance>,
    instance: Option<ContainerInstance>,
    status: ContainerStatus,
    tab: HostContainerTab,
    rows: Vec<ContainerRow>,
}

/// Read-only view of the capture dialog so rendering never touches the Entity
/// while the dialog closure borrows it.
pub(in crate::workspace::connection_monitor) struct HostContainerTextDialogView {
    pub(in crate::workspace::connection_monitor) title: String,
    pub(in crate::workspace::connection_monitor) output: Option<Arc<zeroize::Zeroizing<String>>>,
    pub(in crate::workspace::connection_monitor) error: Option<String>,
    pub(in crate::workspace::connection_monitor) loading: bool,
    pub(in crate::workspace::connection_monitor) scroll_handle: gpui::ScrollHandle,
}

impl WorkspaceApp {
    pub(super) fn render_host_container_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens;
        let i18n = &self.i18n;
        let mono_font_family = settings_mono_font_family(self.settings_store.settings());
        let selectable_text = self.selectable_text_render_state(cx);
        let search_ime = self
            .host_tools_plain_text_ime_frame(HostToolsTextInput::ContainerSearch, cx)
            .expect("container search is a non-secret Host Tools input");
        self.host_tools.update(cx, |host_tools, cx| {
            host_tools.render_host_container_panel(
                search_ime,
                &tokens,
                i18n,
                mono_font_family,
                &selectable_text,
                cx,
            )
        })
    }

    pub(in crate::workspace) fn handle_host_container_search_key(
        &mut self,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self
            .host_tools
            .read(cx)
            .ui
            .input_is_focused(HostToolsTextInput::ContainerSearch)
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

    pub(in crate::workspace) fn handle_host_container_confirm_key(
        &mut self,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.host_tools.read(cx).container_confirm_view().is_none() {
            return false;
        }
        match self.handle_standard_confirm_key(event, cx) {
            Some(ConfirmKeyboardAction::Cancel) => {
                self.begin_host_container_confirm_exit(cx);
                true
            }
            Some(ConfirmKeyboardAction::Confirm) => {
                self.clear_standard_confirm_focus();
                let delay = oxideterm_gpui_ui::motion::duration(
                    &self.tokens,
                    oxideterm_gpui_ui::motion::MotionDuration::Control,
                );
                self.host_tools.update(cx, |host_tools, cx| {
                    host_tools.confirm_container_action_from_view(delay, cx);
                });
                true
            }
            Some(ConfirmKeyboardAction::Handled) => true,
            None => false,
        }
    }

    fn begin_host_container_confirm_exit(&mut self, cx: &mut Context<Self>) -> bool {
        self.clear_standard_confirm_focus();
        let delay = oxideterm_gpui_ui::motion::duration(
            &self.tokens,
            oxideterm_gpui_ui::motion::MotionDuration::Control,
        );
        self.host_tools.update(cx, |host_tools, cx| {
            host_tools.begin_container_confirm_exit(delay, cx)
        })
    }

    pub(in crate::workspace) fn render_host_container_confirm_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (request, phase) = self.host_tools.read(cx).container_confirm_view()?;
        let description = self
            .i18n
            .t(confirm_description_key(request.kind))
            .replace("{{name}}", &request.label);
        let destructive = kind_is_destructive(request.kind);
        Some(
            oxideterm_gpui_ui::confirm::confirm_dialog_with_focus_motion(
                &self.tokens,
                "host-container-confirm-motion",
                phase,
                ConfirmDialogView {
                    variant: if destructive {
                        ConfirmDialogVariant::Danger
                    } else {
                        ConfirmDialogVariant::Default
                    },
                    title: div()
                        .child(self.i18n.t("sidebar.host_container.confirm.title"))
                        .into_any_element(),
                    description: Some(div().child(description).into_any_element()),
                    cancel_label: div()
                        .child(self.i18n.t("sidebar.host_container.confirm.cancel"))
                        .into_any_element(),
                    confirm_label: div()
                        .child(self.i18n.t(confirm_label_key(request.kind)))
                        .into_any_element(),
                },
                self.standard_confirm_focus(),
                cx.listener(|this, _event, _window, cx| {
                    this.begin_host_container_confirm_exit(cx);
                }),
                cx.listener(|this, _event, _window, cx| {
                    let delay = oxideterm_gpui_ui::motion::duration(
                        &this.tokens,
                        oxideterm_gpui_ui::motion::MotionDuration::Control,
                    );
                    this.host_tools.update(cx, |host_tools, cx| {
                        host_tools.confirm_container_action_from_view(delay, cx);
                    });
                }),
            )
            .into_any_element(),
        )
    }

    pub(in crate::workspace) fn render_host_container_text_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.host_tools.read(cx).container_text_dialog()?;
        let (dialog_width, dialog_height) = self.host_tools.read(cx).container_text_dialog_size();
        let theme = self.tokens.ui;
        let content = if dialog.loading {
            div()
                .p_4()
                .text_color(rgb(theme.text_muted))
                .child(self.i18n.t("sidebar.host_container.captures.loading"))
                .into_any_element()
        } else if let Some(error) = dialog.error.as_ref() {
            div()
                .p_4()
                .text_color(rgb(MONITOR_RED))
                .child(error.clone())
                .into_any_element()
        } else {
            let mut lines = div()
                .p_3()
                .flex()
                .flex_col()
                .gap(px(1.0))
                .font_family(settings_mono_font_family(self.settings_store.settings()))
                .text_size(px(11.0))
                .text_color(rgb(theme.text));
            let mut rendered_lines = 0;
            if let Some(output) = dialog.output.as_deref() {
                for (index, line) in output.lines().enumerate() {
                    // Inspect/log output can embed secrets; lines stay bounded,
                    // non-wrapping rows and never reach notices.
                    let display_line = if line.is_empty() {
                        " ".to_string()
                    } else {
                        line.chars().take(512).collect::<String>()
                    };
                    lines = lines.child(
                        div()
                            .id(("host-container-capture-line", index))
                            .flex_none()
                            .whitespace_nowrap()
                            .child(display_line),
                    );
                    rendered_lines += 1;
                }
            }
            if rendered_lines == 0 {
                lines =
                    lines.child(div().child(self.i18n.t("sidebar.host_container.captures.empty")));
            }
            lines.into_any_element()
        };

        Some(
            oxideterm_gpui_ui::modal::dismissible_dialog_backdrop()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event, _window, cx| {
                        this.host_tools.update(cx, |host_tools, cx| {
                            host_tools.dismiss_container_text_dialog(cx);
                        });
                        cx.stop_propagation();
                    }),
                )
                // Resize continuation lives on the backdrop so the corner drag
                // never depends on the pointer staying inside the handle.
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                    this.host_tools.update(cx, |host_tools, cx| {
                        host_tools.update_container_text_dialog_resize(event, cx);
                    });
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _event, _window, cx| {
                        this.host_tools.update(cx, |host_tools, cx| {
                            host_tools.finish_container_text_dialog_resize(cx);
                        });
                    }),
                )
                .child(oxideterm_gpui_ui::modal::overlay_content_boundary(
                    oxideterm_gpui_ui::modal::dialog_content(&self.tokens)
                        .relative()
                        .w(px(dialog_width))
                        .max_h(px(dialog_height + 96.0))
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
                                    IconButtonOptions {
                                        size: 24.0,
                                        has_background: true,
                                        background: Some(rgb(theme.bg_hover)),
                                        hover_background: Some(rgb(theme.bg_panel)),
                                        idle_opacity: 1.0,
                                        ..IconButtonOptions::compact(24.0)
                                    },
                                    self.i18n.t("sidebar.host_container.captures.close"),
                                    "host-container-capture-close",
                                    true,
                                    cx.listener(|this, _event, _window, cx| {
                                        this.host_tools.update(cx, |host_tools, cx| {
                                            host_tools.dismiss_container_text_dialog(cx);
                                        });
                                        cx.stop_propagation();
                                    }),
                                )),
                        )
                        .child(
                            div()
                                .id("host-container-capture-scroll")
                                .h(px(dialog_height))
                                .overflow_y_scroll()
                                .track_scroll(&dialog.scroll_handle)
                                .vertical_scrollbar(&dialog.scroll_handle)
                                .overflow_x_scroll()
                                .child(content),
                        )
                        .child(
                            // Invisible corner hotzone: drag resizes, double
                            // click restores the default size.
                            div()
                                .id("host-container-capture-resize")
                                .absolute()
                                .right(px(2.0))
                                .bottom(px(2.0))
                                .w(px(14.0))
                                .h(px(14.0))
                                .border_r_2()
                                .border_b_2()
                                .border_color(rgba((theme.text_muted << 8) | 0x66))
                                .cursor(CursorStyle::ResizeUpLeftDownRight)
                                .occlude()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                                        this.host_tools.update(cx, |host_tools, cx| {
                                            host_tools
                                                .start_container_text_dialog_resize(event, cx);
                                        });
                                        cx.stop_propagation();
                                    }),
                                ),
                        ),
                ))
                .into_any_element(),
        )
    }
}

/// Placeholder/title/submit keys for the container input dialogs.
fn container_input_title_key(kind: &HostContainerInputKind) -> &'static str {
    match kind {
        HostContainerInputKind::Run => "sidebar.host_container.inputs.run_title",
        HostContainerInputKind::Pull => "sidebar.host_container.inputs.pull_title",
        HostContainerInputKind::Push => "sidebar.host_container.inputs.push_title",
        HostContainerInputKind::Build => "sidebar.host_container.inputs.build_title",
        HostContainerInputKind::Tag { .. } => "sidebar.host_container.inputs.tag_title",
        HostContainerInputKind::Save { .. } => "sidebar.host_container.inputs.save_title",
        HostContainerInputKind::Load => "sidebar.host_container.inputs.load_title",
        HostContainerInputKind::Import => "sidebar.host_container.inputs.import_title",
        HostContainerInputKind::Export { .. } => "sidebar.host_container.inputs.export_title",
        HostContainerInputKind::Copy { .. } => "sidebar.host_container.inputs.copy_title",
        HostContainerInputKind::CreateVolume => "sidebar.host_container.inputs.volume_title",
        HostContainerInputKind::CreateNetwork => "sidebar.host_container.inputs.network_title",
        HostContainerInputKind::CreatePod => "sidebar.host_container.inputs.pod_title",
        HostContainerInputKind::Rename { .. } => "sidebar.host_container.inputs.rename_title",
        HostContainerInputKind::Search => "sidebar.host_container.inputs.search_title",
        HostContainerInputKind::Login => "sidebar.host_container.inputs.login_title",
    }
}

fn container_input_placeholder_key(kind: &HostContainerInputKind) -> &'static str {
    match kind {
        HostContainerInputKind::Run => "sidebar.host_container.inputs.run_placeholder",
        HostContainerInputKind::Pull => "sidebar.host_container.inputs.pull_placeholder",
        HostContainerInputKind::Push => "sidebar.host_container.inputs.push_placeholder",
        HostContainerInputKind::Build => "sidebar.host_container.inputs.build_placeholder",
        HostContainerInputKind::Tag { .. } => "sidebar.host_container.inputs.tag_placeholder",
        HostContainerInputKind::Save { .. } => "sidebar.host_container.inputs.save_placeholder",
        HostContainerInputKind::Load => "sidebar.host_container.inputs.load_placeholder",
        HostContainerInputKind::Import => "sidebar.host_container.inputs.import_placeholder",
        HostContainerInputKind::Export { .. } => "sidebar.host_container.inputs.export_placeholder",
        HostContainerInputKind::Copy { .. } => "sidebar.host_container.inputs.copy_placeholder",
        HostContainerInputKind::CreateVolume => "sidebar.host_container.inputs.volume_placeholder",
        HostContainerInputKind::CreateNetwork => {
            "sidebar.host_container.inputs.network_placeholder"
        }
        HostContainerInputKind::CreatePod => "sidebar.host_container.inputs.pod_placeholder",
        HostContainerInputKind::Rename { .. } => "sidebar.host_container.inputs.rename_placeholder",
        HostContainerInputKind::Search => "sidebar.host_container.inputs.search_placeholder",
        HostContainerInputKind::Login => "sidebar.host_container.inputs.login_placeholder",
    }
}

fn container_input_submit_key(kind: &HostContainerInputKind) -> &'static str {
    match kind {
        HostContainerInputKind::Run => "sidebar.host_container.actions.run",
        HostContainerInputKind::Pull => "sidebar.host_container.actions.pull",
        HostContainerInputKind::Push => "sidebar.host_container.actions.push",
        HostContainerInputKind::Build => "sidebar.host_container.actions.build",
        HostContainerInputKind::Tag { .. } => "sidebar.host_container.actions.tag",
        HostContainerInputKind::Save { .. } => "sidebar.host_container.actions.save",
        HostContainerInputKind::Load => "sidebar.host_container.actions.load",
        HostContainerInputKind::Import => "sidebar.host_container.actions.import",
        HostContainerInputKind::Export { .. } => "sidebar.host_container.actions.export",
        HostContainerInputKind::Copy { .. } => "sidebar.host_container.actions.copy",
        HostContainerInputKind::CreateVolume | HostContainerInputKind::CreateNetwork => {
            "sidebar.host_container.actions.create"
        }
        HostContainerInputKind::CreatePod => "sidebar.host_container.actions.create",
        HostContainerInputKind::Rename { .. } => "sidebar.host_container.actions.rename",
        HostContainerInputKind::Search => "sidebar.host_container.actions.search",
        HostContainerInputKind::Login => "sidebar.host_container.actions.login",
    }
}

impl WorkspaceApp {
    pub(in crate::workspace) fn render_host_container_input_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let target = WorkspaceImeTarget::HostContainerDialog;
        let (kind, submit_disabled, input_control) = {
            let host_tools = self.host_tools.read(cx);
            let ui = &host_tools.ui;
            let dialog = ui.host_container_input_dialog.as_ref()?;
            let input_control = self
                .workspace_text_input(
                    target,
                    TextInputView {
                        value: dialog.value.as_str(),
                        placeholder: self.i18n.t(container_input_placeholder_key(&dialog.kind)),
                        focused: ui.input_is_focused(HostToolsTextInput::ContainerDialog),
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
                            host_tools
                                .ui
                                .focus_input(HostToolsTextInput::ContainerDialog);
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
            self.i18n.t("sidebar.host_container.confirm.cancel"),
            ButtonVariant::Secondary,
            ConfirmDialogAction::Cancel,
            false,
            None,
            |this, _event, _window, cx| {
                this.host_tools.update(cx, |host_tools, cx| {
                    host_tools.dismiss_container_input_dialog(cx);
                });
                this.ime_marked_text = None;
                this.clear_ime_selection();
                cx.notify();
            },
            cx,
        );
        let submit_action = self.workspace_confirm_footer_action_button(
            self.i18n.t(container_input_submit_key(&kind)),
            ButtonVariant::Default,
            ConfirmDialogAction::Confirm,
            submit_disabled,
            None,
            |this, _event, window, cx| {
                this.submit_host_container_input_dialog(window, cx);
            },
            cx,
        );
        let theme = self.tokens.ui;
        Some(
            oxideterm_gpui_ui::modal::dismissible_dialog_backdrop()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event, _window, cx| {
                        this.host_tools.update(cx, |host_tools, cx| {
                            host_tools.dismiss_container_input_dialog(cx);
                        });
                        cx.stop_propagation();
                    }),
                )
                .child(oxideterm_gpui_ui::modal::overlay_content_boundary(
                    oxideterm_gpui_ui::modal::dialog_content(&self.tokens)
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
                                        .child(self.i18n.t(container_input_title_key(&kind))),
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
                        ),
                ))
                .into_any_element(),
        )
    }

    pub(in crate::workspace) fn submit_host_container_input_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (kind, value) = {
            let host_tools = self.host_tools.read(cx);
            let Some(dialog) = host_tools.ui.host_container_input_dialog.as_ref() else {
                return;
            };
            (dialog.kind.clone(), dialog.value.trim().to_string())
        };
        if value.is_empty() {
            return;
        }
        let opened_notice = self.i18n.t("sidebar.host_container.toast.terminal_opened");
        let missing_notice = self
            .i18n
            .t("sidebar.host_container.toast.connection_missing");
        self.ime_marked_text = None;
        self.clear_ime_selection();
        self.host_tools.update(cx, |host_tools, cx| {
            host_tools.dismiss_container_input_dialog(cx);
            host_tools.confirm_container_input_dialog(
                kind,
                value,
                opened_notice,
                missing_notice,
                window,
                cx,
            );
        });
    }
}

impl HostToolsEntity {
    // ---- state ----

    fn selected_container_instance(&self) -> Option<ContainerInstance> {
        let instances = self.host_container.discovery.as_ref()?.instances.as_slice();
        if let Some(selected) = self.host_container.selected_instance_id.as_deref()
            && let Some(instance) = instances.iter().find(|instance| instance.id() == selected)
        {
            return Some(instance.clone());
        }
        oxideterm_container::preferred_instance(instances).cloned()
    }

    fn container_panel_snapshot(&self) -> Option<HostContainerPanelSnapshot> {
        let selected_connection_id = self.selected_connection_id_owned()?;
        let discovery_matches = self.host_container.discovery_connection_id.as_deref()
            == Some(selected_connection_id.as_str());
        let instances = self
            .host_container
            .discovery
            .as_ref()
            .filter(|_| discovery_matches)
            .map(|discovery| discovery.instances.clone())
            .unwrap_or_default();
        let instance = self
            .selected_container_instance()
            .filter(|_| discovery_matches);
        let snapshot_matches = self.host_container.snapshot_connection_id.as_deref()
            == Some(selected_connection_id.as_str());
        let snapshot = self
            .host_container
            .snapshot
            .as_ref()
            .filter(|_| snapshot_matches);
        let status = snapshot
            .map(|snapshot| snapshot.status.clone())
            .unwrap_or_default();
        let query = self.ui.host_container_search_query.trim().to_lowercase();
        let tab = self.host_container.tab;
        let mut rows = Vec::new();
        if let Some(snapshot) = snapshot {
            let matches = |value: &str| query.is_empty() || value.to_lowercase().contains(&query);
            match tab {
                HostContainerTab::Containers => rows.extend(
                    snapshot
                        .containers
                        .iter()
                        .filter(|container| container.matches_query(&query))
                        .cloned()
                        .map(ContainerRow::Container),
                ),
                HostContainerTab::Images => rows.extend(
                    snapshot
                        .images
                        .iter()
                        .filter(|image| {
                            matches(&image.repository) || matches(&image.tag) || matches(&image.id)
                        })
                        .cloned()
                        .map(ContainerRow::Image),
                ),
                HostContainerTab::Volumes => rows.extend(
                    snapshot
                        .volumes
                        .iter()
                        .filter(|volume| matches(&volume.name))
                        .cloned()
                        .map(ContainerRow::Volume),
                ),
                HostContainerTab::Networks => rows.extend(
                    snapshot
                        .networks
                        .iter()
                        .filter(|network| matches(&network.name))
                        .cloned()
                        .map(ContainerRow::Network),
                ),
                HostContainerTab::Pods => rows.extend(
                    snapshot
                        .pods
                        .iter()
                        .filter(|pod| pod.matches_query(&query))
                        .cloned()
                        .map(ContainerRow::Pod),
                ),
                // Stats are captured on demand and rendered from their own
                // snapshot, not from the inventory list.
                HostContainerTab::Stats => {}
            }
        }
        Some(HostContainerPanelSnapshot {
            connections: self.monitor_connections(),
            selected_connection_id,
            instances,
            instance,
            status,
            tab,
            rows,
        })
    }

    pub(in crate::workspace::connection_monitor) fn container_text_dialog(
        &self,
    ) -> Option<HostContainerTextDialogView> {
        self.host_container
            .text_dialog
            .as_ref()
            .map(|dialog| HostContainerTextDialogView {
                title: dialog.title.clone(),
                output: dialog.output.clone(),
                error: dialog.error.clone(),
                loading: dialog.loading,
                scroll_handle: dialog.scroll_handle.clone(),
            })
    }

    pub(in crate::workspace::connection_monitor) fn container_text_dialog_size(
        &self,
    ) -> (f32, f32) {
        self.host_container.text_dialog_size
    }

    pub(in crate::workspace::connection_monitor) fn update_container_text_dialog_resize(
        &mut self,
        event: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.host_container.text_dialog_resize else {
            return;
        };
        let next = resized_container_text_dialog_size(
            (drag.start_width, drag.start_height),
            (
                f32::from(event.position.x - drag.start_cursor.x),
                f32::from(event.position.y - drag.start_cursor.y),
            ),
        );
        if self.host_container.text_dialog_size != next {
            self.host_container.text_dialog_size = next;
            cx.notify();
        }
    }

    pub(in crate::workspace::connection_monitor) fn finish_container_text_dialog_resize(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.host_container.text_dialog_resize.take().is_some() {
            cx.notify();
        }
    }

    pub(in crate::workspace::connection_monitor) fn start_container_text_dialog_resize(
        &mut self,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        if event.click_count >= 2 {
            self.host_container.text_dialog_size = (
                HOST_CONTAINER_CAPTURE_DIALOG_DEFAULT_WIDTH,
                HOST_CONTAINER_CAPTURE_DIALOG_DEFAULT_HEIGHT,
            );
            self.host_container.text_dialog_resize = None;
            cx.notify();
            return;
        }
        let (start_width, start_height) = self.host_container.text_dialog_size;
        self.host_container.text_dialog_resize = Some(HostContainerDialogResizeDrag {
            start_cursor: event.position,
            start_width,
            start_height,
        });
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn dismiss_container_text_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.host_container.text_dialog.take().is_some() {
            cx.notify();
        }
    }

    // ---- panel rendering ----

    fn render_host_container_panel(
        &self,
        search_ime: HostToolsPlainTextImeFrame,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(panel) = self.container_panel_snapshot() else {
            return host_tools_center_state(
                LucideIcon::WifiOff,
                tokens.ui.text_muted,
                i18n.t("profiler.panel.no_connection"),
                selectable_text,
                cx,
            );
        };
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
                &panel.selected_connection_id,
                self.host_container.snapshot.is_some(),
                tokens,
                mono_font_family.clone(),
                selectable_text,
                cx,
            ))
            .child(self.render_container_instance_row(&panel, tokens, i18n, cx))
            .child(self.render_container_tab_bar(&panel, tokens, i18n, cx));
        let body = self.render_container_body(
            &panel,
            &search_ime,
            tokens,
            i18n,
            mono_font_family,
            selectable_text,
            cx,
        );
        div()
            .id("host-container-panel")
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

    fn render_container_instance_row(
        &self,
        panel: &HostContainerPanelSnapshot,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let connection_id = panel.selected_connection_id.clone();
        let refresh = host_tools_tooltip_icon_button(
            tokens,
            LucideIcon::RefreshCw,
            13.0,
            rgb(theme.text),
            IconButtonOptions {
                size: 24.0,
                has_background: true,
                background: Some(rgb(theme.bg_hover)),
                hover_background: Some(rgb(theme.bg_panel)),
                idle_opacity: 1.0,
                ..IconButtonOptions::compact(24.0)
            },
            i18n.t("sidebar.host_container.actions.refresh"),
            "host-container-refresh",
            true,
            cx.listener(move |host_tools, _event, _window, cx| {
                host_tools.refresh_host_container(connection_id.clone(), cx);
                cx.stop_propagation();
            }),
        );

        let selector: AnyElement = if panel.instances.len() > 1 {
            let active_index = panel
                .instance
                .as_ref()
                .and_then(|selected| {
                    panel
                        .instances
                        .iter()
                        .position(|instance| instance.id() == selected.id())
                })
                .unwrap_or_default();
            let items = panel
                .instances
                .iter()
                .map(|instance| {
                    let mut label = i18n.t(instance.engine.label_key());
                    if let Some(version) = instance.server_version.as_deref() {
                        label.push_str(" · ");
                        label.push_str(version);
                    }
                    let active = panel
                        .instance
                        .as_ref()
                        .is_some_and(|selected| selected.id() == instance.id());
                    let instance_id = instance.id();
                    oxideterm_gpui_ui::segmented_control_item_content(
                        tokens,
                        active,
                        div()
                            .w_full()
                            .py(px(2.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(label)
                            .into_any_element(),
                    )
                    .text_size(px(11.0))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |host_tools, _event, _window, cx| {
                            host_tools.select_container_instance(instance_id.clone(), cx);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .into_any_element()
                })
                .collect::<Vec<_>>();
            oxideterm_gpui_ui::segmented_control(
                tokens,
                "host-container-instance-switcher",
                oxideterm_gpui_ui::SegmentedControlOptions::new(
                    active_index,
                    active_index,
                    panel.instances.len(),
                )
                .compact(300.0),
                items,
            )
            .into_any_element()
        } else {
            let label = match panel.instance.as_ref() {
                Some(instance) => {
                    let mut label = i18n.t(instance.engine.label_key());
                    if let Some(version) = instance.server_version.as_deref() {
                        label.push_str(" · ");
                        label.push_str(version);
                    }
                    label.push_str(" · ");
                    label.push_str(&i18n.t(instance.transport.kind().label_key()));
                    label
                }
                None => i18n.t("sidebar.host_container.empty.no_engine"),
            };
            div()
                .min_w_0()
                .truncate()
                .text_size(px(11.0))
                .text_color(rgb(theme.text_muted))
                .child(label)
                .into_any_element()
        };

        div()
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .child(div().min_w_0().flex_1().child(selector))
            .child(refresh)
            .into_any_element()
    }

    fn render_container_tab_bar(
        &self,
        panel: &HostContainerPanelSnapshot,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut tabs = vec![
            (
                HostContainerTab::Containers,
                "sidebar.host_container.tabs.containers",
            ),
            (
                HostContainerTab::Images,
                "sidebar.host_container.tabs.images",
            ),
            (
                HostContainerTab::Volumes,
                "sidebar.host_container.tabs.volumes",
            ),
            (
                HostContainerTab::Networks,
                "sidebar.host_container.tabs.networks",
            ),
        ];
        if panel
            .instance
            .as_ref()
            .is_some_and(|instance| instance.engine.supports_pods())
        {
            tabs.push((HostContainerTab::Pods, "sidebar.host_container.tabs.pods"));
        }
        tabs.push((HostContainerTab::Stats, "sidebar.host_container.tabs.stats"));
        let active_index = tabs
            .iter()
            .position(|(tab, _)| *tab == panel.tab)
            .unwrap_or_default();
        let items = tabs
            .iter()
            .map(|(tab, label_key)| {
                let active = *tab == panel.tab;
                let target = *tab;
                oxideterm_gpui_ui::segmented_control_item_content(
                    tokens,
                    active,
                    div()
                        .w_full()
                        .py(px(2.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(i18n.t(*label_key))
                        .into_any_element(),
                )
                .text_size(px(11.0))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |host_tools, _event, _window, cx| {
                        host_tools.select_container_tab(target, cx);
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .into_any_element()
            })
            .collect::<Vec<_>>();
        oxideterm_gpui_ui::segmented_control(
            tokens,
            "host-container-tab-switcher",
            oxideterm_gpui_ui::SegmentedControlOptions::new(active_index, active_index, tabs.len())
                .compact(320.0),
            items,
        )
        .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_container_body(
        &self,
        panel: &HostContainerPanelSnapshot,
        search_ime: &HostToolsPlainTextImeFrame,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if panel.instance.is_none() {
            return host_tools_center_state(
                LucideIcon::Layers,
                tokens.ui.text_muted,
                i18n.t("sidebar.host_container.empty.no_engine"),
                selectable_text,
                cx,
            );
        }
        match &panel.status {
            ContainerStatus::Unavailable { detail } => {
                let label = match detail {
                    Some(detail) => i18n
                        .t("sidebar.host_container.unavailable_detail")
                        .replace("{{detail}}", detail),
                    None => i18n.t("sidebar.host_container.unavailable"),
                };
                return host_tools_center_state(
                    LucideIcon::Layers,
                    tokens.ui.text_muted,
                    label,
                    selectable_text,
                    cx,
                );
            }
            ContainerStatus::Error { message } => {
                return host_tools_center_state(
                    LucideIcon::AlertTriangle,
                    MONITOR_RED,
                    i18n.t("sidebar.host_container.error")
                        .replace("{{error}}", message),
                    selectable_text,
                    cx,
                );
            }
            ContainerStatus::Unknown | ContainerStatus::Available => {}
        }
        if panel.tab == HostContainerTab::Stats {
            return self.render_container_stats(tokens, i18n, selectable_text, cx);
        }
        // A missing snapshot means detection/first capture has not resolved
        // yet; showing an empty inventory would wrongly imply the host is empty.
        if self.host_container.snapshot.is_none()
            || (panel.rows.is_empty()
                && (self.host_container.snapshot_running || self.host_container.discovery_running))
        {
            return host_tools_center_state(
                LucideIcon::Layers,
                tokens.ui.text_muted,
                i18n.t("sidebar.host_container.loading"),
                selectable_text,
                cx,
            );
        }
        div()
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
                    .pt_2()
                    .pb_2()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(self.render_container_search(search_ime, tokens, i18n, cx))
                    .child(self.render_container_count_row(panel, tokens, i18n, cx)),
            )
            .child(self.render_container_table(
                panel,
                tokens,
                i18n,
                mono_font_family,
                selectable_text,
                cx,
            ))
            .into_any_element()
    }

    fn render_container_stats(
        &self,
        tokens: &ThemeTokens,
        i18n: &I18n,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        if self.host_container.stats_running {
            return host_tools_center_state(
                LucideIcon::Activity,
                theme.text_muted,
                i18n.t("sidebar.host_container.loading"),
                selectable_text,
                cx,
            );
        }
        let stats = self.host_container.stats.as_deref().unwrap_or_default();
        if stats.is_empty() {
            return host_tools_center_state(
                LucideIcon::Activity,
                theme.text_muted,
                i18n.t("sidebar.host_container.empty.stats"),
                selectable_text,
                cx,
            );
        }
        let header = div()
            .flex_none()
            .w_full()
            .min_w_0()
            .h(px(28.0))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(rgba((theme.border << 8) | MONITOR_BORDER_ALPHA))
            .text_size(px(10.0))
            .text_color(rgb(theme.text_muted))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(i18n.t("sidebar.host_container.columns.name")),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(64.0))
                    .child(i18n.t("sidebar.host_container.columns.cpu")),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(132.0))
                    .truncate()
                    .child(i18n.t("sidebar.host_container.columns.memory")),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(120.0))
                    .truncate()
                    .child(i18n.t("sidebar.host_container.columns.network")),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(120.0))
                    .truncate()
                    .child(i18n.t("sidebar.host_container.columns.block")),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(40.0))
                    .child(i18n.t("sidebar.host_container.columns.pids")),
            );
        let rows = stats
            .iter()
            .map(|stat| {
                div()
                    .w_full()
                    .min_w_0()
                    .px_3()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgba((theme.border << 8) | MONITOR_BORDER_ALPHA))
                    .text_size(px(11.0))
                    .text_color(rgb(theme.text))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(if stat.name.is_empty() {
                                stat.id.clone()
                            } else {
                                stat.name.clone()
                            }),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(64.0))
                            .child(stat.cpu_percent.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(132.0))
                            .truncate()
                            .child(stat.mem_usage.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(120.0))
                            .truncate()
                            .child(stat.net_io.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(120.0))
                            .truncate()
                            .child(stat.block_io.clone()),
                    )
                    .child(div().flex_none().w(px(40.0)).child(stat.pids.clone()))
            })
            .collect::<Vec<_>>();
        div()
            .w_full()
            .min_w_0()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(header)
            .child(
                div()
                    .id("host-container-stats-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows),
            )
            .into_any_element()
    }

    fn render_container_search(
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
                    value: &self.ui.host_container_search_query,
                    placeholder: i18n.t("sidebar.host_container.search_placeholder"),
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

    fn render_container_count_row(
        &self,
        panel: &HostContainerPanelSnapshot,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let suffix_key = match panel.tab {
            HostContainerTab::Containers => "sidebar.host_container.counts.containers",
            HostContainerTab::Images => "sidebar.host_container.counts.images",
            HostContainerTab::Volumes => "sidebar.host_container.counts.volumes",
            HostContainerTab::Networks => "sidebar.host_container.counts.networks",
            HostContainerTab::Pods => "sidebar.host_container.counts.pods",
            HostContainerTab::Stats => "sidebar.host_container.counts.containers",
        };
        let mut actions: Vec<(LucideIcon, &'static str, &'static str, ContainerCountAction)> =
            Vec::new();
        match panel.tab {
            HostContainerTab::Containers => actions.push((
                LucideIcon::Play,
                "sidebar.host_container.actions.run",
                "host-container-run",
                ContainerCountAction::Input(HostContainerInputKind::Run),
            )),
            HostContainerTab::Images => {
                actions.extend([
                    (
                        LucideIcon::Scissors,
                        "sidebar.host_container.actions.prune",
                        "host-container-prune-images",
                        ContainerCountAction::PruneImages(false),
                    ),
                    (
                        LucideIcon::Archive,
                        "sidebar.host_container.actions.prune_unused",
                        "host-container-prune-unused-images",
                        ContainerCountAction::PruneImages(true),
                    ),
                    (
                        LucideIcon::Download,
                        "sidebar.host_container.actions.pull",
                        "host-container-pull",
                        ContainerCountAction::Input(HostContainerInputKind::Pull),
                    ),
                    (
                        LucideIcon::Upload,
                        "sidebar.host_container.actions.push",
                        "host-container-push",
                        ContainerCountAction::Input(HostContainerInputKind::Push),
                    ),
                    (
                        LucideIcon::Wrench,
                        "sidebar.host_container.actions.build",
                        "host-container-build",
                        ContainerCountAction::Input(HostContainerInputKind::Build),
                    ),
                    (
                        LucideIcon::Search,
                        "sidebar.host_container.actions.search",
                        "host-container-search",
                        ContainerCountAction::Input(HostContainerInputKind::Search),
                    ),
                    (
                        LucideIcon::KeyRound,
                        "sidebar.host_container.actions.login",
                        "host-container-login",
                        ContainerCountAction::Input(HostContainerInputKind::Login),
                    ),
                ]);
            }
            HostContainerTab::Volumes => {
                actions.extend([
                    (
                        LucideIcon::Plus,
                        "sidebar.host_container.actions.create",
                        "host-container-create-volume",
                        ContainerCountAction::Input(HostContainerInputKind::CreateVolume),
                    ),
                    (
                        LucideIcon::Trash2,
                        "sidebar.host_container.actions.prune",
                        "host-container-prune-volumes",
                        ContainerCountAction::Confirm(HostContainerConfirmKind::Volume(
                            oxideterm_container::VolumeAction::Prune,
                        )),
                    ),
                ]);
            }
            HostContainerTab::Networks => {
                actions.extend([
                    (
                        LucideIcon::Plus,
                        "sidebar.host_container.actions.create",
                        "host-container-create-network",
                        ContainerCountAction::Input(HostContainerInputKind::CreateNetwork),
                    ),
                    (
                        LucideIcon::Trash2,
                        "sidebar.host_container.actions.prune",
                        "host-container-prune-networks",
                        ContainerCountAction::Confirm(HostContainerConfirmKind::Network(
                            oxideterm_container::NetworkAction::Prune,
                        )),
                    ),
                ]);
            }
            HostContainerTab::Pods => actions.push((
                LucideIcon::Plus,
                "sidebar.host_container.actions.create",
                "host-container-create-pod",
                ContainerCountAction::Input(HostContainerInputKind::CreatePod),
            )),
            HostContainerTab::Stats => {}
        }
        let prune_label = i18n.t(suffix_key);
        let buttons =
            actions
                .into_iter()
                .map(|(icon, tooltip_key, id, action)| {
                    let tooltip = i18n.t(tooltip_key);
                    let prune_label = prune_label.clone();
                    host_tools_tooltip_icon_button(
                        tokens,
                        icon,
                        13.0,
                        rgb(theme.text),
                        IconButtonOptions {
                            size: 24.0,
                            has_background: true,
                            background: Some(rgb(theme.bg_hover)),
                            hover_background: Some(rgb(theme.bg_panel)),
                            idle_opacity: 1.0,
                            ..IconButtonOptions::compact(24.0)
                        },
                        tooltip,
                        id,
                        true,
                        cx.listener(move |host_tools, _event, _window, cx| {
                            match &action {
                                ContainerCountAction::Input(kind) => {
                                    host_tools.open_container_input_dialog(kind.clone(), "", cx)
                                }
                                ContainerCountAction::PruneImages(all) => host_tools
                                    .run_container_prune_images(*all, prune_label.clone(), cx),
                                ContainerCountAction::Confirm(kind) => host_tools
                                    .request_container_prune(*kind, prune_label.clone(), cx),
                            }
                            cx.stop_propagation();
                        }),
                    )
                })
                .collect::<Vec<_>>();
        div()
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
                    .child(format!("{} {}", panel.rows.len(), i18n.t(suffix_key))),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(3.0))
                    .children(buttons),
            )
            .into_any_element()
    }

    fn render_container_table(
        &self,
        panel: &HostContainerPanelSnapshot,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        if panel.rows.is_empty() {
            let empty_key = match panel.tab {
                HostContainerTab::Containers => "sidebar.host_container.empty.containers",
                HostContainerTab::Images => "sidebar.host_container.empty.images",
                HostContainerTab::Volumes => "sidebar.host_container.empty.volumes",
                HostContainerTab::Networks => "sidebar.host_container.empty.networks",
                HostContainerTab::Pods => "sidebar.host_container.empty.pods",
                HostContainerTab::Stats => "sidebar.host_container.empty.stats",
            };
            return host_tools_center_state(
                LucideIcon::Layers,
                theme.text_muted,
                i18n.t(empty_key),
                selectable_text,
                cx,
            );
        }
        let identity = format!(
            "host-container:{}:{:?}:{}",
            panel.selected_connection_id, panel.tab, self.ui.host_container_search_query
        );
        sync_container_list_state(
            &self.host_container.list_state,
            &mut self.host_container.list_cache.borrow_mut(),
            &identity,
            &panel.rows,
        );
        let rows = Arc::new(panel.rows.clone());
        let instance = panel.instance.clone();
        let connection_id = Arc::new(panel.selected_connection_id.clone());
        let list_state = self.host_container.list_state.clone();
        let list_spec = TauriVirtualListSpec::new(px(64.0), 6);
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
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(tauri_virtual_list(
                        list_state,
                        list_spec,
                        move |index, _window, cx| {
                            let rows = Arc::clone(&rows);
                            let instance = instance.clone();
                            let connection_id = Arc::clone(&connection_id);
                            let mono_font_family = row_mono_font_family.clone();
                            host_tools.update(cx, |host_tools, cx| {
                                host_tools.render_container_row(
                                    rows.get(index).cloned(),
                                    instance.clone(),
                                    connection_id.as_str(),
                                    &row_tokens,
                                    &row_i18n,
                                    mono_font_family,
                                    cx,
                                )
                            })
                        },
                    )),
            )
            .into_any_element()
    }

    fn render_row_action(
        &self,
        row: &mut gpui::Div,
        tokens: &ThemeTokens,
        icon: LucideIcon,
        tooltip: String,
        id: &'static str,
        enabled: bool,
        cx: &mut Context<Self>,
        listener: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let theme = tokens.ui;
        let options = IconButtonOptions {
            size: 22.0,
            has_background: true,
            background: Some(rgb(theme.bg_hover)),
            hover_background: Some(rgb(theme.bg_panel)),
            idle_opacity: if enabled { 1.0 } else { 0.35 },
            disabled: !enabled,
            ..IconButtonOptions::compact(22.0)
        };
        *row = std::mem::replace(row, div()).child(host_tools_tooltip_icon_button(
            tokens,
            icon,
            12.0,
            rgb(theme.text),
            options,
            tooltip,
            id,
            true,
            cx.listener(move |host_tools, _event, window, cx| {
                listener(host_tools, window, cx);
                cx.stop_propagation();
            }),
        ));
    }

    #[allow(clippy::too_many_arguments)]
    fn render_container_row(
        &self,
        row: Option<ContainerRow>,
        instance: Option<ContainerInstance>,
        connection_id: &str,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let Some(row) = row else {
            return div().h(px(0.0)).into_any_element();
        };
        let shell = div()
            .w_full()
            .min_w_0()
            .px_3()
            .py_2()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(rgba((theme.border << 8) | MONITOR_BORDER_ALPHA))
            .hover(|style| style.bg(rgb(theme.bg_hover)));
        match row {
            ContainerRow::Container(container) => {
                let Some(instance) = instance else {
                    return shell.into_any_element();
                };
                let availability = container_action_availability(&container.state);
                let id = container.id.clone();
                let name = container.name.clone();
                let connection_id_owned = connection_id.to_string();
                let mut actions = div().flex_none().flex().items_center().gap(px(2.0));
                let inspect_title = format!(
                    "{} · {name}",
                    i18n.t("sidebar.host_container.actions.inspect")
                );
                let logs_title =
                    format!("{} · {name}", i18n.t("sidebar.host_container.actions.logs"));
                self.render_row_action(
                    &mut actions,
                    tokens,
                    LucideIcon::Info,
                    i18n.t("sidebar.host_container.actions.inspect"),
                    "host-container-inspect",
                    true,
                    cx,
                    move |host_tools, _window, cx| {
                        host_tools.open_container_text_capture(
                            ContainerCapture::ContainerInspect {
                                container_id: id.clone(),
                            },
                            inspect_title.clone(),
                            cx,
                        );
                    },
                );
                {
                    let id = container.id.clone();
                    let connection_id = connection_id_owned.clone();
                    let title = logs_title.clone();
                    let instance = instance.clone();
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::FileText,
                        i18n.t("sidebar.host_container.actions.logs"),
                        "host-container-logs",
                        availability.can_use_live_tools,
                        cx,
                        move |host_tools, _window, cx| {
                            host_tools.open_container_text_capture(
                                ContainerCapture::ContainerLogs {
                                    container_id: id.clone(),
                                },
                                title.clone(),
                                cx,
                            );
                            let _ = (connection_id.clone(), instance.clone());
                        },
                    );
                }
                {
                    let id = container.id.clone();
                    let connection_id = connection_id_owned.clone();
                    let title = format!(
                        "{} · {name}",
                        i18n.t("sidebar.host_container.actions.follow_logs")
                    );
                    let Ok(command) = oxideterm_container::follow_logs_command(&instance, &id)
                    else {
                        return shell.into_any_element();
                    };
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::Radio,
                        i18n.t("sidebar.host_container.actions.follow_logs"),
                        "host-container-follow-logs",
                        availability.can_use_live_tools,
                        cx,
                        move |host_tools, window, cx| {
                            host_tools.dispatch_container_terminal(
                                connection_id.clone(),
                                command.clone(),
                                title.clone(),
                                cx,
                                window,
                            );
                        },
                    );
                }
                {
                    let id = container.id.clone();
                    let connection_id = connection_id_owned.clone();
                    let title =
                        format!("{} · {name}", i18n.t("sidebar.host_container.actions.exec"));
                    let Ok(command) = oxideterm_container::exec_shell_command(&instance, &id)
                    else {
                        return shell.into_any_element();
                    };
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::Terminal,
                        i18n.t("sidebar.host_container.actions.exec"),
                        "host-container-exec",
                        availability.can_exec,
                        cx,
                        move |host_tools, window, cx| {
                            host_tools.dispatch_container_terminal(
                                connection_id.clone(),
                                command.clone(),
                                title.clone(),
                                cx,
                                window,
                            );
                        },
                    );
                }
                {
                    let id = container.id.clone();
                    let connection_id = connection_id_owned.clone();
                    let title = format!(
                        "{} · {name}",
                        i18n.t("sidebar.host_container.actions.attach")
                    );
                    if let Ok(command) = oxideterm_container::terminal_command(
                        &instance,
                        oxideterm_container::ContainerTerminalOp::Attach { container_id: id },
                    ) {
                        self.render_row_action(
                            &mut actions,
                            tokens,
                            LucideIcon::Cable,
                            i18n.t("sidebar.host_container.actions.attach"),
                            "host-container-attach",
                            availability.can_use_live_tools,
                            cx,
                            move |host_tools, window, cx| {
                                host_tools.dispatch_container_terminal(
                                    connection_id.clone(),
                                    command.clone(),
                                    title.clone(),
                                    cx,
                                    window,
                                );
                            },
                        );
                    }
                }
                {
                    let id = container.id.clone();
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::Download,
                        i18n.t("sidebar.host_container.actions.export"),
                        "host-container-export",
                        true,
                        cx,
                        move |host_tools, _window, cx| {
                            host_tools.open_container_input_dialog(
                                HostContainerInputKind::Export {
                                    container_id: id.clone(),
                                },
                                "",
                                cx,
                            );
                        },
                    );
                }
                {
                    let id = container.id.clone();
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::Copy,
                        i18n.t("sidebar.host_container.actions.copy"),
                        "host-container-copy",
                        true,
                        cx,
                        move |host_tools, _window, cx| {
                            host_tools.open_container_input_dialog(
                                HostContainerInputKind::Copy {
                                    container_id: id.clone(),
                                },
                                "",
                                cx,
                            );
                        },
                    );
                }
                {
                    let id = container.id.clone();
                    let initial = container.name.clone();
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::Pencil,
                        i18n.t("sidebar.host_container.actions.rename"),
                        "host-container-rename",
                        instance.engine.supports_container_maintenance(),
                        cx,
                        move |host_tools, _window, cx| {
                            host_tools.open_container_input_dialog(
                                HostContainerInputKind::Rename {
                                    container_id: id.clone(),
                                },
                                &initial,
                                cx,
                            );
                        },
                    );
                }
                let mut podman_actions = Vec::new();
                for (action, icon, enabled) in [
                    (
                        oxideterm_container::ContainerAction::Start,
                        LucideIcon::Play,
                        availability.can_start,
                    ),
                    (
                        oxideterm_container::ContainerAction::Stop,
                        LucideIcon::StopCircle,
                        availability.can_stop,
                    ),
                    (
                        oxideterm_container::ContainerAction::Restart,
                        LucideIcon::RefreshCw,
                        availability.can_restart,
                    ),
                ] {
                    podman_actions.push((action, icon, enabled));
                }
                if instance.engine.supports_pause() {
                    podman_actions.push((
                        oxideterm_container::ContainerAction::Pause,
                        LucideIcon::Pause,
                        availability.can_pause,
                    ));
                    podman_actions.push((
                        oxideterm_container::ContainerAction::Unpause,
                        LucideIcon::Play,
                        availability.can_unpause,
                    ));
                }
                podman_actions.push((
                    oxideterm_container::ContainerAction::Remove,
                    LucideIcon::Trash2,
                    availability.can_remove,
                ));
                for (action, icon, enabled) in podman_actions {
                    let target = container.id.clone();
                    let label = container.name.clone();
                    let instance = instance.clone();
                    let connection_id = connection_id_owned.clone();
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        icon,
                        i18n.t(action.label_key()),
                        match action {
                            oxideterm_container::ContainerAction::Start => "host-container-start",
                            oxideterm_container::ContainerAction::Stop => "host-container-stop",
                            oxideterm_container::ContainerAction::Restart => {
                                "host-container-restart"
                            }
                            oxideterm_container::ContainerAction::Pause => "host-container-pause",
                            oxideterm_container::ContainerAction::Unpause => {
                                "host-container-unpause"
                            }
                            oxideterm_container::ContainerAction::Remove => "host-container-remove",
                        },
                        enabled,
                        cx,
                        move |host_tools, _window, cx| {
                            host_tools.open_container_confirm(
                                HostContainerConfirmRequest {
                                    connection_id: connection_id.clone(),
                                    instance: instance.clone(),
                                    target_id: target.clone(),
                                    label: label.clone(),
                                    kind: HostContainerConfirmKind::Container(action),
                                },
                                cx,
                            );
                        },
                    );
                }
                let status = if container.status.is_empty() {
                    i18n.t(oxideterm_container::container_state_label_key(
                        &container.state,
                    ))
                    .to_string()
                } else {
                    container.status.clone()
                };
                shell
                    .child(
                        div()
                            .flex_none()
                            .size(px(7.0))
                            .rounded_full()
                            .bg(rgb(container_state_color(tokens, &container.state))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(1.0))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.0))
                                    .text_color(rgb(theme.text))
                                    .child(container.name.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .font_family(mono_font_family)
                                    .text_size(px(10.0))
                                    .text_color(rgb(theme.text_muted))
                                    .child(container.image.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(92.0))
                            .truncate()
                            .text_size(px(10.0))
                            .text_color(rgb(theme.text_muted))
                            .child(status),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(88.0))
                            .truncate()
                            .text_size(px(10.0))
                            .text_color(rgb(theme.text_muted))
                            .child(container.ports.clone().unwrap_or_default()),
                    )
                    .child(actions)
                    .into_any_element()
            }
            ContainerRow::Image(image) => {
                let Some(instance) = instance else {
                    return shell.into_any_element();
                };
                let reference = if image.tag.is_empty() {
                    image.repository.clone()
                } else {
                    format!("{}:{}", image.repository, image.tag)
                };
                let connection_id_owned = connection_id.to_string();
                let mut actions = div().flex_none().flex().items_center().gap(px(2.0));
                {
                    let image_id = image.id.clone();
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::Hash,
                        i18n.t("sidebar.host_container.actions.tag"),
                        "host-container-image-tag",
                        true,
                        cx,
                        move |host_tools, _window, cx| {
                            host_tools.open_container_input_dialog(
                                HostContainerInputKind::Tag {
                                    image_id: image_id.clone(),
                                },
                                "",
                                cx,
                            );
                        },
                    );
                }
                {
                    let image_id = image.id.clone();
                    let title = format!(
                        "{} · {reference}",
                        i18n.t("sidebar.host_container.actions.inspect")
                    );
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::Info,
                        i18n.t("sidebar.host_container.actions.inspect"),
                        "host-container-image-inspect",
                        true,
                        cx,
                        move |host_tools, _window, cx| {
                            host_tools.open_container_text_capture(
                                ContainerCapture::ImageInspect {
                                    image_id: image_id.clone(),
                                },
                                title.clone(),
                                cx,
                            );
                        },
                    );
                }
                if instance.engine.supports_image_history() {
                    let image_id = image.id.clone();
                    let title = format!(
                        "{} · {reference}",
                        i18n.t("sidebar.host_container.actions.history")
                    );
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::History,
                        i18n.t("sidebar.host_container.actions.history"),
                        "host-container-image-history",
                        true,
                        cx,
                        move |host_tools, _window, cx| {
                            host_tools.open_container_text_capture(
                                ContainerCapture::ImageHistory {
                                    image_id: image_id.clone(),
                                },
                                title.clone(),
                                cx,
                            );
                        },
                    );
                }
                {
                    let image_id = image.id.clone();
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::Save,
                        i18n.t("sidebar.host_container.actions.save"),
                        "host-container-image-save",
                        true,
                        cx,
                        move |host_tools, _window, cx| {
                            host_tools.open_container_input_dialog(
                                HostContainerInputKind::Save {
                                    image_id: image_id.clone(),
                                },
                                "",
                                cx,
                            );
                        },
                    );
                }
                {
                    let target = image.id.clone();
                    let label = reference.clone();
                    let instance = instance.clone();
                    let connection_id = connection_id_owned.clone();
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::Trash2,
                        i18n.t("sidebar.host_container.actions.remove"),
                        "host-container-image-remove",
                        true,
                        cx,
                        move |host_tools, _window, cx| {
                            host_tools.open_container_confirm(
                                HostContainerConfirmRequest {
                                    connection_id: connection_id.clone(),
                                    instance: instance.clone(),
                                    target_id: target.clone(),
                                    label: label.clone(),
                                    kind: HostContainerConfirmKind::Image(
                                        oxideterm_container::ImageAction::Remove,
                                    ),
                                },
                                cx,
                            );
                        },
                    );
                }
                shell
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(1.0))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.0))
                                    .text_color(rgb(theme.text))
                                    .child(reference),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .font_family(mono_font_family)
                                    .text_size(px(10.0))
                                    .text_color(rgb(theme.text_muted))
                                    .child(image.id.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(84.0))
                            .truncate()
                            .text_size(px(10.0))
                            .text_color(rgb(theme.text_muted))
                            .child(image.size.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(96.0))
                            .truncate()
                            .text_size(px(10.0))
                            .text_color(rgb(theme.text_muted))
                            .child(image.created.clone()),
                    )
                    .child(actions)
                    .into_any_element()
            }
            ContainerRow::Volume(volume) => {
                let Some(instance) = instance else {
                    return shell.into_any_element();
                };
                let connection_id_owned = connection_id.to_string();
                let mut actions = div().flex_none().flex().items_center().gap(px(2.0));
                {
                    let name = volume.name.clone();
                    let title = format!(
                        "{} · {name}",
                        i18n.t("sidebar.host_container.actions.inspect")
                    );
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::Info,
                        i18n.t("sidebar.host_container.actions.inspect"),
                        "host-container-volume-inspect",
                        true,
                        cx,
                        move |host_tools, _window, cx| {
                            host_tools.open_container_text_capture(
                                ContainerCapture::VolumeInspect { name: name.clone() },
                                title.clone(),
                                cx,
                            );
                        },
                    );
                }
                {
                    let name = volume.name.clone();
                    let title = format!(
                        "{} · {name}",
                        i18n.t("sidebar.host_container.actions.usage")
                    );
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::ListTree,
                        i18n.t("sidebar.host_container.actions.usage"),
                        "host-container-volume-usage",
                        true,
                        cx,
                        move |host_tools, _window, cx| {
                            host_tools.open_container_text_capture(
                                ContainerCapture::VolumeUsage { name: name.clone() },
                                title.clone(),
                                cx,
                            );
                        },
                    );
                }
                {
                    let target = volume.name.clone();
                    let label = volume.name.clone();
                    let instance = instance.clone();
                    let connection_id = connection_id_owned.clone();
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::Trash2,
                        i18n.t("sidebar.host_container.actions.remove"),
                        "host-container-volume-remove",
                        true,
                        cx,
                        move |host_tools, _window, cx| {
                            host_tools.open_container_confirm(
                                HostContainerConfirmRequest {
                                    connection_id: connection_id.clone(),
                                    instance: instance.clone(),
                                    target_id: target.clone(),
                                    label: label.clone(),
                                    kind: HostContainerConfirmKind::Volume(
                                        oxideterm_container::VolumeAction::Remove,
                                    ),
                                },
                                cx,
                            );
                        },
                    );
                }
                shell
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(1.0))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.0))
                                    .text_color(rgb(theme.text))
                                    .child(volume.name.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .font_family(mono_font_family)
                                    .text_size(px(10.0))
                                    .text_color(rgb(theme.text_muted))
                                    .child(volume.mount_point.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(84.0))
                            .truncate()
                            .text_size(px(10.0))
                            .text_color(rgb(theme.text_muted))
                            .child(volume.driver.clone()),
                    )
                    .child(actions)
                    .into_any_element()
            }
            ContainerRow::Network(network) => {
                let Some(instance) = instance else {
                    return shell.into_any_element();
                };
                let connection_id_owned = connection_id.to_string();
                let mut actions = div().flex_none().flex().items_center().gap(px(2.0));
                {
                    let name = network.name.clone();
                    let title = format!(
                        "{} · {name}",
                        i18n.t("sidebar.host_container.actions.inspect")
                    );
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::Info,
                        i18n.t("sidebar.host_container.actions.inspect"),
                        "host-container-network-inspect",
                        true,
                        cx,
                        move |host_tools, _window, cx| {
                            host_tools.open_container_text_capture(
                                ContainerCapture::NetworkInspect { name: name.clone() },
                                title.clone(),
                                cx,
                            );
                        },
                    );
                }
                {
                    let target = network.name.clone();
                    let label = network.name.clone();
                    let instance = instance.clone();
                    let connection_id = connection_id_owned.clone();
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::Trash2,
                        i18n.t("sidebar.host_container.actions.remove"),
                        "host-container-network-remove",
                        true,
                        cx,
                        move |host_tools, _window, cx| {
                            host_tools.open_container_confirm(
                                HostContainerConfirmRequest {
                                    connection_id: connection_id.clone(),
                                    instance: instance.clone(),
                                    target_id: target.clone(),
                                    label: label.clone(),
                                    kind: HostContainerConfirmKind::Network(
                                        oxideterm_container::NetworkAction::Remove,
                                    ),
                                },
                                cx,
                            );
                        },
                    );
                }
                shell
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(1.0))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.0))
                                    .text_color(rgb(theme.text))
                                    .child(network.name.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .font_family(mono_font_family)
                                    .text_size(px(10.0))
                                    .text_color(rgb(theme.text_muted))
                                    .child(if network.subnets.is_empty() {
                                        network.driver.clone()
                                    } else {
                                        format!(
                                            "{} · {}",
                                            network.driver,
                                            network.subnets.join(", ")
                                        )
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(84.0))
                            .truncate()
                            .text_size(px(10.0))
                            .text_color(rgb(theme.text_muted))
                            .child(network.scope.clone()),
                    )
                    .child(actions)
                    .into_any_element()
            }
            ContainerRow::Pod(pod) => {
                let Some(instance) = instance else {
                    return shell.into_any_element();
                };
                let availability = pod_action_availability(&pod.status);
                let connection_id_owned = connection_id.to_string();
                let mut actions = div().flex_none().flex().items_center().gap(px(2.0));
                {
                    let pod_id = pod.id.clone();
                    let title = format!(
                        "{} · {}",
                        i18n.t("sidebar.host_container.actions.inspect"),
                        pod.name
                    );
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::Info,
                        i18n.t("sidebar.host_container.actions.inspect"),
                        "host-container-pod-inspect",
                        true,
                        cx,
                        move |host_tools, _window, cx| {
                            host_tools.open_container_text_capture(
                                ContainerCapture::PodInspect {
                                    pod_id: pod_id.clone(),
                                },
                                title.clone(),
                                cx,
                            );
                        },
                    );
                }
                for (action, icon, enabled) in [
                    (
                        oxideterm_container::PodAction::Start,
                        LucideIcon::Play,
                        availability.can_start,
                    ),
                    (
                        oxideterm_container::PodAction::Stop,
                        LucideIcon::StopCircle,
                        availability.can_stop,
                    ),
                    (
                        oxideterm_container::PodAction::Restart,
                        LucideIcon::RefreshCw,
                        availability.can_restart,
                    ),
                    (
                        oxideterm_container::PodAction::Pause,
                        LucideIcon::Pause,
                        availability.can_pause,
                    ),
                    (
                        oxideterm_container::PodAction::Unpause,
                        LucideIcon::Play,
                        availability.can_unpause,
                    ),
                    (
                        oxideterm_container::PodAction::Remove,
                        LucideIcon::Trash2,
                        availability.can_remove,
                    ),
                ] {
                    let target = pod.id.clone();
                    let label = pod.name.clone();
                    let instance = instance.clone();
                    let connection_id = connection_id_owned.clone();
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        icon,
                        i18n.t(action.label_key()),
                        match action {
                            oxideterm_container::PodAction::Start => "host-container-pod-start",
                            oxideterm_container::PodAction::Stop => "host-container-pod-stop",
                            oxideterm_container::PodAction::Restart => "host-container-pod-restart",
                            oxideterm_container::PodAction::Pause => "host-container-pod-pause",
                            oxideterm_container::PodAction::Unpause => "host-container-pod-unpause",
                            oxideterm_container::PodAction::Remove => "host-container-pod-remove",
                        },
                        enabled,
                        cx,
                        move |host_tools, _window, cx| {
                            host_tools.open_container_confirm(
                                HostContainerConfirmRequest {
                                    connection_id: connection_id.clone(),
                                    instance: instance.clone(),
                                    target_id: target.clone(),
                                    label: label.clone(),
                                    kind: HostContainerConfirmKind::Pod(action),
                                },
                                cx,
                            );
                        },
                    );
                }
                shell
                    .child(
                        div()
                            .flex_none()
                            .size(px(7.0))
                            .rounded_full()
                            .bg(rgb(container_state_color(tokens, &pod.status))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.0))
                            .child(pod.name.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(72.0))
                            .text_size(px(10.0))
                            .text_color(rgb(theme.text_muted))
                            .child(pod.containers.to_string()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(92.0))
                            .truncate()
                            .text_size(px(10.0))
                            .text_color(rgb(theme.text_muted))
                            .child(i18n.t(oxideterm_container::pod_state_label_key(&pod.status))),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(84.0))
                            .truncate()
                            .text_size(px(10.0))
                            .text_color(rgb(theme.text_muted))
                            .child(pod.created.clone().unwrap_or_default()),
                    )
                    .child(actions)
                    .into_any_element()
            }
        }
    }

    // ---- state transitions ----

    pub(in crate::workspace::connection_monitor) fn select_container_tab(
        &mut self,
        tab: HostContainerTab,
        cx: &mut Context<Self>,
    ) {
        if self.host_container.tab != tab {
            self.host_container.tab = tab;
            self.host_container.list_state = tauri_virtual_list_state(
                0,
                ListAlignment::Top,
                TauriVirtualListSpec::new(px(64.0), 6),
            );
            if tab == HostContainerTab::Stats
                && let Some(connection_id) = self.selected_connection_id_owned()
            {
                self.request_container_stats(connection_id, cx);
            }
            cx.notify();
        }
    }

    pub(in crate::workspace::connection_monitor) fn select_container_instance(
        &mut self,
        instance_id: String,
        cx: &mut Context<Self>,
    ) {
        let Some(instance) = self
            .host_container
            .discovery
            .as_ref()
            .and_then(|discovery| {
                discovery
                    .instances
                    .iter()
                    .find(|instance| instance.id() == instance_id)
            })
            .cloned()
        else {
            return;
        };
        let changed =
            self.host_container.selected_instance_id.as_deref() != Some(instance_id.as_str());
        self.host_container.selected_instance_id = Some(instance_id);
        if changed {
            self.host_container.snapshot = None;
            self.host_container.snapshot_connection_id = None;
            if self.host_container.tab == HostContainerTab::Pods && !instance.engine.supports_pods()
            {
                self.host_container.tab = HostContainerTab::Containers;
            }
            if let Some(connection_id) = self.selected_connection_id_owned() {
                self.request_container_snapshot(connection_id, instance, cx);
            }
            cx.notify();
        }
    }

    pub(in crate::workspace::connection_monitor) fn request_container_discovery_if_needed(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let Some(connection_id) = self.selected_connection_id_owned() else {
            return;
        };
        let same_connection =
            self.host_container.discovery_connection_id.as_deref() == Some(connection_id.as_str());
        if same_connection
            && (self.host_container.discovery.is_some() || self.host_container.discovery_running)
        {
            if let Some(instance) = self.selected_container_instance()
                && self.host_container.snapshot_connection_id.as_deref()
                    != Some(connection_id.as_str())
                && !self.host_container.snapshot_running
            {
                self.request_container_snapshot(connection_id, instance, cx);
            }
            return;
        }
        self.request_container_discovery(connection_id, HostSnapshotFeedback::Silent, cx);
    }

    pub(in crate::workspace::connection_monitor) fn refresh_host_container(
        &mut self,
        connection_id: String,
        cx: &mut Context<Self>,
    ) {
        self.request_container_discovery(connection_id, HostSnapshotFeedback::Toast, cx);
    }

    pub(in crate::workspace::connection_monitor) fn request_container_discovery(
        &mut self,
        connection_id: String,
        feedback: HostSnapshotFeedback,
        cx: &mut Context<Self>,
    ) {
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            return;
        };
        let Some(os_type) = self.connection_os_type(&connection_id) else {
            return;
        };
        if feedback == HostSnapshotFeedback::Toast {
            self.host_container.discovery = None;
        }
        if self.host_container.discovery_running
            && self.host_container.discovery_connection_id.as_deref()
                == Some(connection_id.as_str())
        {
            return;
        }
        let request = HostContainerCaptureRequest {
            connection_id: connection_id.clone(),
            kind: HostContainerCaptureKind::Discovery,
            failure_fallback: self
                .messages
                .as_ref()
                .map(|messages| messages.podman_capture_failed.clone())
                .unwrap_or_else(|| "Container detection failed.".to_string()),
        };
        let command = oxideterm_container::discovery_command(&os_type);
        if self.spawn_container_capture(
            command,
            request,
            Duration::from_secs(40),
            128 * 1024,
            runtime,
        ) {
            self.host_container.discovery_running = true;
            self.host_container.discovery_connection_id = Some(connection_id);
            cx.notify();
        }
    }

    pub(in crate::workspace::connection_monitor) fn request_container_snapshot(
        &mut self,
        connection_id: String,
        instance: ContainerInstance,
        cx: &mut Context<Self>,
    ) {
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            return;
        };
        let Some(os_type) = self.connection_os_type(&connection_id) else {
            return;
        };
        let request = HostContainerCaptureRequest {
            connection_id: connection_id.clone(),
            kind: HostContainerCaptureKind::Snapshot(instance.clone()),
            failure_fallback: self
                .messages
                .as_ref()
                .map(|messages| messages.podman_capture_failed.clone())
                .unwrap_or_else(|| "Container snapshot failed.".to_string()),
        };
        let command = oxideterm_container::snapshot_command(&os_type, &instance);
        if self.spawn_container_capture(
            command,
            request,
            Duration::from_secs(30),
            HOST_CONTAINER_CAPTURE_MAX_OUTPUT,
            runtime,
        ) {
            self.host_container.snapshot_running = true;
            cx.notify();
        }
    }

    pub(in crate::workspace::connection_monitor) fn open_container_text_capture(
        &mut self,
        capture: ContainerCapture,
        title: String,
        cx: &mut Context<Self>,
    ) {
        let Some(connection_id) = self.selected_connection_id_owned() else {
            return;
        };
        let Some(instance) = self.selected_container_instance() else {
            return;
        };
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            return;
        };
        let Some(os_type) = self.connection_os_type(&connection_id) else {
            return;
        };
        self.host_container.text_dialog = Some(HostContainerTextDialog {
            title: title.clone(),
            output: None,
            error: None,
            loading: true,
            scroll_handle: gpui::ScrollHandle::new(),
        });
        let request = HostContainerCaptureRequest {
            connection_id,
            kind: HostContainerCaptureKind::Text {
                capture: capture.clone(),
                title,
            },
            failure_fallback: self
                .messages
                .as_ref()
                .map(|messages| messages.podman_capture_failed.clone())
                .unwrap_or_else(|| "Container capture failed.".to_string()),
        };
        let command = match oxideterm_container::capture_command(&os_type, &instance, &capture) {
            Ok(command) => command,
            Err(_) => {
                if let Some(dialog) = self.host_container.text_dialog.as_mut() {
                    dialog.loading = false;
                    dialog.error = Some(request.failure_fallback.clone());
                }
                cx.notify();
                return;
            }
        };
        if self.spawn_container_capture(
            command,
            request.clone(),
            HOST_CONTAINER_CAPTURE_TIMEOUT,
            HOST_CONTAINER_CAPTURE_MAX_OUTPUT,
            runtime,
        ) {
            self.host_container.text_dialog_running = Some(request);
        } else if let Some(dialog) = self.host_container.text_dialog.as_mut() {
            dialog.loading = false;
            dialog.error = Some(request.failure_fallback);
        }
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn dispatch_container_terminal(
        &mut self,
        connection_id: String,
        command: String,
        title: String,
        cx: &mut Context<Self>,
        window: &mut Window,
    ) {
        window.dispatch_action(
            Box::new(HostToolsWindowRequest::new(
                HostToolsWindowIntent::OpenExistingNodeTerminal {
                    connection_id,
                    command,
                    title,
                    opened_notice: self
                        .messages
                        .as_ref()
                        .map(|messages| messages.container_terminal_opened.clone())
                        .unwrap_or_default(),
                    missing_notice: self
                        .messages
                        .as_ref()
                        .map(|messages| messages.container_terminal_missing.clone())
                        .unwrap_or_default(),
                },
            )),
            cx,
        );
    }

    pub(in crate::workspace::connection_monitor) fn finish_host_container_capture(
        &mut self,
        delivery: HostContainerCaptureDelivery,
        cx: &mut Context<Self>,
    ) {
        let succeeded =
            matches!(&delivery.result, Ok(output) if output.exit_code.unwrap_or(0) == 0);
        match delivery.request.kind {
            HostContainerCaptureKind::Discovery => {
                self.host_container.discovery_running = false;
                let Ok(mut output) = delivery.result else {
                    cx.emit(HostToolsEvent::ShowNotice(
                        HostToolsNotice::ContainerCaptureFailed,
                    ));
                    cx.notify();
                    return;
                };
                let text = std::mem::take(&mut output.stdout);
                zeroize::Zeroize::zeroize(&mut output.stderr);
                if !succeeded {
                    cx.emit(HostToolsEvent::ShowNotice(
                        HostToolsNotice::ContainerCaptureFailed,
                    ));
                    cx.notify();
                    return;
                }
                let discovery = oxideterm_container::parse_discovery(&text);
                let connection_id = delivery.request.connection_id.clone();
                self.host_container.discovery_connection_id = Some(connection_id.clone());
                // Drop a selection that no longer exists so the panel always
                // shows the engine its switcher highlights.
                if let Some(selected) = self.host_container.selected_instance_id.as_deref()
                    && !discovery
                        .instances
                        .iter()
                        .any(|instance| instance.id() == selected)
                {
                    self.host_container.selected_instance_id = None;
                }
                self.host_container.discovery = Some(discovery);
                if let Some(instance) = self.selected_container_instance() {
                    self.host_container.selected_instance_id = Some(instance.id());
                    self.request_container_snapshot(connection_id, instance, cx);
                } else {
                    self.host_container.snapshot = None;
                    self.host_container.snapshot_connection_id = None;
                }
                cx.notify();
            }
            HostContainerCaptureKind::Stats => {
                self.host_container.stats_running = false;
                let stats = match delivery.result {
                    Ok(mut output) if output.exit_code.unwrap_or(0) == 0 => {
                        let text = std::mem::take(&mut output.stdout);
                        zeroize::Zeroize::zeroize(&mut output.stderr);
                        oxideterm_container::parse_stats(&text)
                    }
                    Ok(mut output) => {
                        zeroize::Zeroize::zeroize(&mut output.stdout);
                        zeroize::Zeroize::zeroize(&mut output.stderr);
                        Vec::new()
                    }
                    Err(_) => Vec::new(),
                };
                self.host_container.stats = Some(stats);
                cx.notify();
            }
            HostContainerCaptureKind::Snapshot(instance) => {
                self.host_container.snapshot_running = false;
                // A faster engine switch may have already selected another
                // instance; the stale delivery must not override it.
                if let Some(selected) = self.host_container.selected_instance_id.as_deref()
                    && selected != instance.id()
                {
                    cx.notify();
                    return;
                }
                let snapshot = match delivery.result {
                    Ok(mut output) if output.exit_code.unwrap_or(0) == 0 => {
                        let text = std::mem::take(&mut output.stdout);
                        zeroize::Zeroize::zeroize(&mut output.stderr);
                        oxideterm_container::parse_snapshot(&text)
                    }
                    Ok(mut output) => {
                        let hint = first_stderr_line(&output.stderr)
                            .unwrap_or_else(|| delivery.request.failure_fallback.clone());
                        zeroize::Zeroize::zeroize(&mut output.stdout);
                        zeroize::Zeroize::zeroize(&mut output.stderr);
                        oxideterm_container::ContainerSnapshot {
                            status: ContainerStatus::Error { message: hint },
                            ..oxideterm_container::ContainerSnapshot::default()
                        }
                    }
                    Err(_) => oxideterm_container::ContainerSnapshot {
                        status: ContainerStatus::Error {
                            message: delivery.request.failure_fallback.clone(),
                        },
                        ..oxideterm_container::ContainerSnapshot::default()
                    },
                };
                self.host_container.snapshot_connection_id =
                    Some(delivery.request.connection_id.clone());
                self.host_container.selected_instance_id = Some(instance.id());
                self.host_container.snapshot = Some(snapshot);
                cx.notify();
            }
            HostContainerCaptureKind::Text { .. } => {
                self.host_container.text_dialog_running = None;
                let Some(dialog) = self.host_container.text_dialog.as_mut() else {
                    cx.notify();
                    return;
                };
                dialog.loading = false;
                match delivery.result {
                    Ok(mut output) if output.exit_code.unwrap_or(0) == 0 => {
                        let text = std::mem::take(&mut output.stdout);
                        zeroize::Zeroize::zeroize(&mut output.stderr);
                        dialog.output = Some(Arc::new(zeroize::Zeroizing::new(text)));
                        dialog.error = None;
                    }
                    Ok(mut output) => {
                        let hint = first_stderr_line(&output.stderr);
                        zeroize::Zeroize::zeroize(&mut output.stdout);
                        zeroize::Zeroize::zeroize(&mut output.stderr);
                        dialog.error = Some(match hint {
                            Some(hint) => format!("{} ({hint})", delivery.request.failure_fallback),
                            None => delivery.request.failure_fallback.clone(),
                        });
                    }
                    Err(_) => {
                        dialog.error = Some(delivery.request.failure_fallback.clone());
                    }
                }
                cx.notify();
            }
        }
    }

    pub(in crate::workspace::connection_monitor) fn open_container_confirm(
        &mut self,
        request: HostContainerConfirmRequest,
        cx: &mut Context<Self>,
    ) {
        if self.host_container.action_running {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::ContainerActionAlreadyRunning,
            ));
            return;
        }
        HostToolConfirmState::open(&mut self.host_container.confirm, request);
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn container_confirm_view(
        &self,
    ) -> Option<(
        HostContainerConfirmRequest,
        oxideterm_gpui_ui::motion::ExitPhase,
    )> {
        self.host_container
            .confirm
            .as_ref()
            .map(|state| (state.request.clone(), state.presence.phase()))
    }

    pub(in crate::workspace::connection_monitor) fn dismiss_container_confirm(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.host_container.confirm.take().is_some() {
            cx.notify();
        }
    }

    pub(in crate::workspace::connection_monitor) fn begin_container_confirm_exit(
        &mut self,
        delay: Duration,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(generation) = self
            .host_container
            .confirm
            .as_mut()
            .and_then(|state| state.presence.begin_exit())
        else {
            return false;
        };
        if delay.is_zero() {
            self.host_container.confirm = None;
            cx.notify();
            return true;
        }
        cx.spawn(async move |weak, cx| {
            Timer::after(delay).await;
            let _ = weak.update(cx, |entity, cx| {
                if entity
                    .host_container
                    .confirm
                    .as_ref()
                    .is_some_and(|state| state.presence.finish_exit(generation))
                {
                    entity.host_container.confirm = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
        true
    }

    pub(in crate::workspace::connection_monitor) fn confirm_container_action_from_view(
        &mut self,
        delay: Duration,
        cx: &mut Context<Self>,
    ) {
        let Some(request) = self
            .host_container
            .confirm
            .as_ref()
            .map(|state| state.request.clone())
        else {
            return;
        };
        if !self.begin_container_confirm_exit(delay, cx) {
            return;
        }
        self.start_container_action(request, cx);
    }

    pub(in crate::workspace::connection_monitor) fn start_container_action(
        &mut self,
        request: HostContainerConfirmRequest,
        cx: &mut Context<Self>,
    ) {
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::ContainerConnectionMissing,
            ));
            return;
        };
        let Some(os_type) = self.connection_os_type(&request.connection_id) else {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::ContainerConnectionMissing,
            ));
            return;
        };
        let target = request.target_id.as_str();
        let command = match &request.kind {
            HostContainerConfirmKind::Container(action) => {
                oxideterm_container::container_action_command(
                    &os_type,
                    &request.instance,
                    *action,
                    target,
                )
            }
            HostContainerConfirmKind::Image(action) => oxideterm_container::image_action_command(
                &os_type,
                &request.instance,
                *action,
                Some(target),
            ),
            HostContainerConfirmKind::Volume(action) => oxideterm_container::volume_action_command(
                &os_type,
                &request.instance,
                *action,
                Some(target),
            ),
            HostContainerConfirmKind::Network(action) => {
                oxideterm_container::network_action_command(
                    &os_type,
                    &request.instance,
                    *action,
                    Some(target),
                )
            }
            HostContainerConfirmKind::Pod(action) => oxideterm_container::pod_action_command(
                &os_type,
                &request.instance,
                *action,
                target,
            ),
            HostContainerConfirmKind::Raw => Err("Raw commands are spawned directly.".to_string()),
        };
        let Ok(command) = command else {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::ContainerActionFailed,
            ));
            return;
        };
        self.host_container.action_running = true;
        if !self.spawn_container_action(
            command,
            request,
            HOST_CONTAINER_ACTION_TIMEOUT,
            HOST_CONTAINER_ACTION_MAX_OUTPUT,
            runtime,
        ) {
            self.host_container.action_running = false;
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::ContainerConnectionMissing,
            ));
        }
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn finish_host_container_action(
        &mut self,
        delivery: HostContainerConfirmDelivery,
        cx: &mut Context<Self>,
    ) {
        self.host_container.action_running = false;
        let succeeded =
            matches!(&delivery.result, Ok(output) if output.exit_code.unwrap_or(0) == 0);
        cx.emit(HostToolsEvent::ShowNotice(
            HostToolsNotice::ContainerActionFinished {
                description: delivery.request.label.clone(),
                succeeded,
            },
        ));
        // Refresh the inventory so the panel never knowingly lies after a
        // succeeded or partially applied mutation.
        if let Some(instance) = self.selected_container_instance() {
            self.request_container_snapshot(delivery.request.connection_id, instance, cx);
        }
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn run_container_prune_images(
        &mut self,
        all: bool,
        label: String,
        cx: &mut Context<Self>,
    ) {
        let Some(instance) = self.selected_container_instance() else {
            return;
        };
        let Some(os_type) = self
            .selected_connection_id_owned()
            .and_then(|connection_id| self.connection_os_type(&connection_id))
        else {
            return;
        };
        match oxideterm_container::prune_images_command(&os_type, &instance, all) {
            Ok(command) => self.start_container_raw_action(command, label, cx),
            Err(_) => cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::ContainerActionFailed,
            )),
        }
    }

    pub(in crate::workspace::connection_monitor) fn request_container_stats(
        &mut self,
        connection_id: String,
        cx: &mut Context<Self>,
    ) {
        if self.host_container.stats_running {
            return;
        }
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            return;
        };
        let Some(os_type) = self.connection_os_type(&connection_id) else {
            return;
        };
        let Some(instance) = self.selected_container_instance() else {
            return;
        };
        let request = HostContainerCaptureRequest {
            connection_id,
            kind: HostContainerCaptureKind::Stats,
            failure_fallback: self
                .messages
                .as_ref()
                .map(|messages| messages.podman_capture_failed.clone())
                .unwrap_or_else(|| "Container stats failed.".to_string()),
        };
        let command = oxideterm_container::stats_command(&os_type, &instance);
        if self.spawn_container_capture(
            command,
            request,
            Duration::from_secs(20),
            HOST_CONTAINER_CAPTURE_MAX_OUTPUT,
            runtime,
        ) {
            self.host_container.stats_running = true;
            self.host_container.stats = None;
            cx.notify();
        }
    }

    pub(in crate::workspace::connection_monitor) fn open_container_input_dialog(
        &mut self,
        kind: HostContainerInputKind,
        initial: &str,
        cx: &mut Context<Self>,
    ) {
        self.ui.host_container_input_dialog = Some(HostContainerInputDialog {
            kind,
            value: initial.to_string(),
        });
        self.ui.focus_input(HostToolsTextInput::ContainerDialog);
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn dismiss_container_input_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.ui.host_container_input_dialog.take().is_some() {
            self.ui.clear_input_focus();
            cx.notify();
        }
    }

    /// Runs a composed command as a direct (non-confirmed) mutation.
    pub(in crate::workspace::connection_monitor) fn start_container_raw_action(
        &mut self,
        command: String,
        description: String,
        cx: &mut Context<Self>,
    ) {
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::ContainerConnectionMissing,
            ));
            return;
        };
        let Some(connection_id) = self.selected_connection_id_owned() else {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::ContainerConnectionMissing,
            ));
            return;
        };
        let Some(instance) = self.selected_container_instance() else {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::ContainerConnectionMissing,
            ));
            return;
        };
        self.host_container.action_running = true;
        if !self.spawn_container_action(
            command,
            HostContainerConfirmRequest {
                connection_id,
                instance,
                target_id: String::new(),
                label: description,
                kind: HostContainerConfirmKind::Raw,
            },
            HOST_CONTAINER_ACTION_TIMEOUT,
            HOST_CONTAINER_ACTION_MAX_OUTPUT,
            runtime,
        ) {
            self.host_container.action_running = false;
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::ContainerConnectionMissing,
            ));
        }
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn confirm_container_input_dialog(
        &mut self,
        kind: HostContainerInputKind,
        value: String,
        opened_notice: String,
        missing_notice: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(connection_id) = self.selected_connection_id_owned() else {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::ContainerConnectionMissing,
            ));
            return;
        };
        let Some(instance) = self.selected_container_instance() else {
            return;
        };
        let Some(os_type) = self.connection_os_type(&connection_id) else {
            return;
        };
        let mut dispatch_terminal =
            |this: &Self, command: String, title: String, cx: &mut Context<Self>| {
                window.dispatch_action(
                    Box::new(HostToolsWindowRequest::new(
                        HostToolsWindowIntent::OpenExistingNodeTerminal {
                            connection_id: connection_id.clone(),
                            command,
                            title,
                            opened_notice: opened_notice.clone(),
                            missing_notice: missing_notice.clone(),
                        },
                    )),
                    cx,
                );
                let _ = this;
            };
        let failed = |cx: &mut Context<Self>| {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::ContainerActionFailed,
            ))
        };
        match kind {
            HostContainerInputKind::Run => {
                match oxideterm_container::terminal_command(
                    &instance,
                    oxideterm_container::ContainerTerminalOp::Run { args: value },
                ) {
                    Ok(command) => dispatch_terminal(self, command, "run".to_string(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Pull => {
                match oxideterm_container::terminal_command(
                    &instance,
                    oxideterm_container::ContainerTerminalOp::Pull { reference: value },
                ) {
                    Ok(command) => dispatch_terminal(self, command, "pull".to_string(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Push => {
                match oxideterm_container::terminal_command(
                    &instance,
                    oxideterm_container::ContainerTerminalOp::Push { reference: value },
                ) {
                    Ok(command) => dispatch_terminal(self, command, "push".to_string(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Build => {
                match oxideterm_container::terminal_command(
                    &instance,
                    oxideterm_container::ContainerTerminalOp::Build { args: value },
                ) {
                    Ok(command) => dispatch_terminal(self, command, "build".to_string(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Login => {
                match oxideterm_container::terminal_command(
                    &instance,
                    oxideterm_container::ContainerTerminalOp::Login { registry: value },
                ) {
                    Ok(command) => dispatch_terminal(self, command, "login".to_string(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Load => {
                match oxideterm_container::terminal_command(
                    &instance,
                    oxideterm_container::ContainerTerminalOp::Load { path: value },
                ) {
                    Ok(command) => dispatch_terminal(self, command, "load".to_string(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Import => {
                let mut parts = value.split_whitespace();
                let path = parts.next().unwrap_or_default().to_string();
                let reference = parts.next().unwrap_or_default().to_string();
                match oxideterm_container::terminal_command(
                    &instance,
                    oxideterm_container::ContainerTerminalOp::Import { path, reference },
                ) {
                    Ok(command) => dispatch_terminal(self, command, "import".to_string(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Save { image_id } => {
                match oxideterm_container::terminal_command(
                    &instance,
                    oxideterm_container::ContainerTerminalOp::Save {
                        reference: image_id,
                        path: value,
                    },
                ) {
                    Ok(command) => dispatch_terminal(self, command, "save".to_string(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Export { container_id } => {
                match oxideterm_container::terminal_command(
                    &instance,
                    oxideterm_container::ContainerTerminalOp::Export {
                        container_id,
                        path: value,
                    },
                ) {
                    Ok(command) => dispatch_terminal(self, command, "export".to_string(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Copy { container_id } => {
                let mut parts = value.split_whitespace();
                let source = parts.next().unwrap_or_default().to_string();
                let destination = parts.next().unwrap_or_default().to_string();
                match oxideterm_container::terminal_command(
                    &instance,
                    oxideterm_container::ContainerTerminalOp::Copy {
                        source: format!("{container_id}:{source}"),
                        destination,
                    },
                ) {
                    Ok(command) => dispatch_terminal(self, command, "cp".to_string(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Tag { image_id } => {
                match oxideterm_container::tag_image_command(&os_type, &instance, &image_id, &value)
                {
                    Ok(command) => self.start_container_raw_action(command, value.clone(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Rename { container_id } => {
                match oxideterm_container::rename_container_command(
                    &os_type,
                    &instance,
                    &container_id,
                    &value,
                ) {
                    Ok(command) => self.start_container_raw_action(command, value.clone(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::CreateVolume => {
                match oxideterm_container::create_volume_command(&os_type, &instance, &value) {
                    Ok(command) => self.start_container_raw_action(command, value.clone(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::CreateNetwork => {
                let mut parts = value.split_whitespace();
                let name = parts.next().unwrap_or_default().to_string();
                let driver = parts.next().map(str::to_string);
                match oxideterm_container::create_network_command(
                    &os_type,
                    &instance,
                    &name,
                    driver.as_deref(),
                ) {
                    Ok(command) => self.start_container_raw_action(command, name.clone(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::CreatePod => {
                match oxideterm_container::create_pod_command(&os_type, &instance, &value) {
                    Ok(command) => self.start_container_raw_action(command, value.clone(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Search => {
                self.open_container_text_capture(
                    ContainerCapture::Search {
                        term: value.clone(),
                    },
                    format!("search · {value}"),
                    cx,
                );
            }
        }
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn request_container_prune(
        &mut self,
        kind: HostContainerConfirmKind,
        label: String,
        cx: &mut Context<Self>,
    ) {
        let Some(connection_id) = self.selected_connection_id_owned() else {
            return;
        };
        let Some(instance) = self.selected_container_instance() else {
            return;
        };
        self.open_container_confirm(
            HostContainerConfirmRequest {
                connection_id,
                instance,
                target_id: String::new(),
                label,
                kind,
            },
            cx,
        );
    }
}

fn first_stderr_line(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(200).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_state_follows_the_rendered_row_count() {
        let state = tauri_virtual_list_state(
            0,
            ListAlignment::Top,
            TauriVirtualListSpec::new(px(64.0), 6),
        );
        let mut cache = VirtualListSignatureCache::default();
        let pod = |id: &str| {
            ContainerRow::Pod(oxideterm_container::Pod {
                id: id.to_string(),
                name: id.to_string(),
                ..oxideterm_container::Pod::default()
            })
        };
        let rows = vec![pod("a"), pod("b"), pod("c")];
        sync_container_list_state(&state, &mut cache, "rows", &rows);
        assert_eq!(state.item_count(), 3);
        // Filtering shrinks the same list instead of leaving stale rows behind.
        sync_container_list_state(&state, &mut cache, "rows", &rows[..1]);
        assert_eq!(state.item_count(), 1);
    }

    #[test]
    fn capture_dialog_drag_tracks_and_clamps() {
        let start = (
            HOST_CONTAINER_CAPTURE_DIALOG_DEFAULT_WIDTH,
            HOST_CONTAINER_CAPTURE_DIALOG_DEFAULT_HEIGHT,
        );
        assert_eq!(
            resized_container_text_dialog_size(start, (100.0, 60.0)),
            (start.0 + 100.0, start.1 + 60.0)
        );
        assert_eq!(
            resized_container_text_dialog_size(start, (-5000.0, -5000.0)),
            (
                HOST_CONTAINER_CAPTURE_DIALOG_MIN_WIDTH,
                HOST_CONTAINER_CAPTURE_DIALOG_MIN_HEIGHT
            )
        );
        assert_eq!(
            resized_container_text_dialog_size(start, (5000.0, 5000.0)),
            (
                HOST_CONTAINER_CAPTURE_DIALOG_MAX_WIDTH,
                HOST_CONTAINER_CAPTURE_DIALOG_MAX_HEIGHT
            )
        );
    }
}
