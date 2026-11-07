//! Unified Container tool: engine detection, instance switching, the shared
//! inventory lists and the confirm/capture dialogs.
//!
//! Engine and transport details live in `oxideterm-container`; this module only
//! renders the unified inventory and forwards composed commands.

use super::*;

use oxideterm_connection_monitor::container::{
    ContainerCapture, ContainerInstance, ContainerStatus, container_action_availability,
    pod_action_availability,
};
use oxideterm_gpui_ui::button::{ButtonVariant, IconButtonOptions};

const HOST_CONTAINER_ACTION_TIMEOUT: Duration = Duration::from_secs(20);
const HOST_CONTAINER_ACTION_MAX_OUTPUT: usize = 4096;
const HOST_CONTAINER_CAPTURE_TIMEOUT: Duration = Duration::from_secs(30);
const HOST_CONTAINER_CAPTURE_MAX_OUTPUT: usize = 2 * 1024 * 1024;

/// Confirmed image prunes carry their scope in the request's target: an empty
/// target prunes dangling images only, this one also drops unused tagged ones.
const IMAGE_PRUNE_ALL_SCOPE: &str = "all";

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

/// Image sizes arrive as raw bytes from every engine; show them humanized and
/// keep an already-formatted engine string as the fallback.
fn image_size_label(image: &oxideterm_connection_monitor::container::Image) -> String {
    match image.size_bytes {
        Some(bytes) => format_bytes(bytes),
        None => image.size.clone(),
    }
}

/// Podman reports creation as unix seconds; render a local timestamp instead
/// of the raw number. Engines that already print a date keep it.
fn image_created_label(image: &oxideterm_connection_monitor::container::Image) -> String {
    image
        .created_at
        .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
        .map(|value| {
            value
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| image.created.clone())
}

/// Three-state cycle for the image sort buttons: descending → ascending →
/// engine order, then back to descending.
fn next_image_sort(
    active: Option<oxideterm_connection_monitor::container::ImageSort>,
    descending: bool,
    requested: oxideterm_connection_monitor::container::ImageSort,
) -> (
    Option<oxideterm_connection_monitor::container::ImageSort>,
    bool,
) {
    match active {
        Some(current) if current == requested && !descending => (None, true),
        Some(current) if current == requested => (Some(current), false),
        _ => (Some(requested), true),
    }
}

/// Containers' sort cycle: ascending, descending, then the engine's order.
/// Switching keys always starts an ascending pass.
fn next_container_sort(
    active: Option<ContainerSort>,
    requested: ContainerSortKey,
) -> Option<ContainerSort> {
    match active {
        Some(sort) if sort.key != requested => Some(ContainerSort {
            key: requested,
            descending: false,
        }),
        Some(sort) if !sort.descending => Some(ContainerSort {
            key: requested,
            descending: true,
        }),
        Some(_) => None,
        None => Some(ContainerSort {
            key: requested,
            descending: false,
        }),
    }
}

/// Stats ids are 12-character abbreviations while inventory ids are full
/// digests (`--no-trunc`), so either side may be the shorter form.
fn container_stat_for<'a>(
    stats: &'a [oxideterm_connection_monitor::container::ContainerStat],
    container_id: &str,
) -> Option<&'a oxideterm_connection_monitor::container::ContainerStat> {
    stats.iter().find(|stat| {
        let stat_id = stat.id.trim();
        let container_id = container_id.trim();
        stat_id.eq_ignore_ascii_case(container_id)
            || (stat_id.len() >= 12
                && container_id.len() >= 12
                && (stat_id.starts_with(container_id) || container_id.starts_with(stat_id)))
    })
}

fn container_sort_value(
    container: &oxideterm_connection_monitor::container::Container,
    sort: ContainerSortKey,
    stats: &[oxideterm_connection_monitor::container::ContainerStat],
) -> Option<f64> {
    let stat = container_stat_for(stats, &container.id)?;
    match sort {
        ContainerSortKey::Cpu => Some(stat.cpu_percent_value),
        ContainerSortKey::Memory => Some(stat.mem_usage_bytes as f64),
    }
}

/// Rows without a stats sample keep the engine's relative order at the end,
/// so a half-finished capture cannot shake the list.
fn sort_containers(
    containers: &mut [oxideterm_connection_monitor::container::Container],
    sort: ContainerSort,
    stats: &[oxideterm_connection_monitor::container::ContainerStat],
) {
    containers.sort_by(|left, right| {
        let left_value = container_sort_value(left, sort.key, stats);
        let right_value = container_sort_value(right, sort.key, stats);
        match (left_value, right_value) {
            (Some(left), Some(right)) => {
                let order = left
                    .partial_cmp(&right)
                    .unwrap_or(std::cmp::Ordering::Equal);
                if sort.descending {
                    order.reverse()
                } else {
                    order
                }
            }
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
}

/// Row identity for an image row. The engine's image ID is the config digest,
/// which several tags share; the reference is what the table actually lists,
/// so selection and list keys must include it.
fn image_row_identity(image: &oxideterm_connection_monitor::container::Image) -> String {
    format!("{}\u{1}{}\u{1}{}", image.id, image.repository, image.tag)
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
    use oxideterm_connection_monitor::container;
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
    Container(oxideterm_connection_monitor::container::Container),
    Image(oxideterm_connection_monitor::container::Image),
    Volume(oxideterm_connection_monitor::container::Volume),
    Network(oxideterm_connection_monitor::container::Network),
    Pod(oxideterm_connection_monitor::container::Pod),
}

/// Header action for the current inventory tab.
#[derive(Clone)]
enum ContainerCountAction {
    Input(HostContainerInputKind),
    /// `true` also removes unused tagged images.
    PruneImages(bool),
    Confirm(HostContainerConfirmKind),
}

/// Whether the row's action strip is expanded. Expanding changes the row
/// height, so the virtual list must see it in the signature.
fn container_row_is_selected(row: &ContainerRow, selected_row: Option<&str>) -> bool {
    let Some(selected) = selected_row else {
        return false;
    };
    match row {
        ContainerRow::Container(container) => container.id == selected,
        ContainerRow::Image(image) => image_row_identity(image) == selected,
        // Volumes and networks always show their actions.
        ContainerRow::Volume(_) | ContainerRow::Network(_) => false,
        ContainerRow::Pod(pod) => pod.id == selected,
    }
}

/// Row identity for the virtual list: sampling updates must not reset scroll,
/// but selection changes the row height and must remeasure it.
fn container_row_signature(row: &ContainerRow, selected: bool) -> u64 {
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
            // Images sharing a digest stay distinct by repository and tag.
            image_row_identity(image).hash(&mut hasher);
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
    selected.hash(&mut hasher);
    hasher.finish()
}

/// Keeps the virtual list's item count in step with the rendered rows; without
/// this the list stays empty while the count label already shows the total.
fn sync_container_list_state(
    state: &ListState,
    cache: &mut VirtualListSignatureCache,
    identity: &str,
    rows: &[ContainerRow],
    selected_row: Option<&str>,
) {
    let signatures = rows
        .iter()
        .map(|row| container_row_signature(row, container_row_is_selected(row, selected_row)))
        .collect::<Vec<_>>();
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
    /// Why discovery could not run at all (for example a missing Git Bash),
    /// kept apart from an empty engine list.
    discovery_error: Option<String>,
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

    /// Escape closes and Enter submits the container input dialog. IME
    /// composition owns Enter before this runs (`defer_active_ime_key`), so a
    /// candidate commit never submits.
    pub(in crate::workspace) fn handle_host_container_input_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self
            .host_tools
            .read(cx)
            .ui
            .host_container_input_dialog
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
                    host_tools.dismiss_container_input_dialog(cx);
                });
                self.ime_marked_text = None;
                self.clear_ime_selection();
                cx.notify();
                true
            }
            "enter" => {
                self.submit_host_container_input_dialog(window, cx);
                true
            }
            _ => false,
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
        // The two image prune controls share one action; the scope selects the
        // copy so "dangling" never describes an unused-image prune.
        let description_key = if matches!(
            request.kind,
            HostContainerConfirmKind::Image(
                oxideterm_connection_monitor::container::ImageAction::Prune
            )
        ) && request.target_id == IMAGE_PRUNE_ALL_SCOPE
        {
            "sidebar.host_container.confirms.prune_all_images"
        } else {
            confirm_description_key(request.kind)
        };
        let description = self
            .i18n
            .t(description_key)
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
        // The capture body is read-only text, but users must be able to select
        // and copy an inspect line without reopening the shell.
        let text_group_id = crate::workspace::selectable_text::selectable_text_id(
            "host-container-capture",
            dialog.title.as_str(),
        );
        let mono_font_family = settings_mono_font_family(self.settings_store.settings());
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
                .font_family(mono_font_family.clone())
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
                            .child(self.render_row_safe_selectable_display_text_in_group(
                                text_group_id,
                                "host-container-capture-line",
                                index,
                                index,
                                display_line,
                                theme.text,
                                Some(mono_font_family.clone()),
                                cx,
                            )),
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
/// Pull, push and search name a registry; every other input is one line.
fn container_input_uses_registry(kind: &HostContainerInputKind) -> bool {
    matches!(
        kind,
        HostContainerInputKind::Pull
            | HostContainerInputKind::Push
            | HostContainerInputKind::Search
    )
}

/// Registry value usable as a reference prefix. Docker stores Hub credentials
/// under `https://index.docker.io/v1/`, which is not a pullable host.
fn registry_host(raw: &str) -> String {
    let host = oxideterm_connection_monitor::container::normalized_registry(raw);
    let host = host.trim_end_matches("/v1");
    if matches!(
        host.to_ascii_lowercase().as_str(),
        "index.docker.io" | "registry-1.docker.io" | "docker.io"
    ) {
        oxideterm_connection_monitor::container::DEFAULT_REGISTRY.to_string()
    } else {
        host.to_string()
    }
}

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
    /// One text field of the container input dialog, wired to the shared IME
    /// so the registry and image lines edit exactly like every other input.
    fn render_container_dialog_text_field(
        &self,
        target: WorkspaceImeTarget,
        input: HostToolsTextInput,
        value: &str,
        placeholder: String,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let control = self
            .workspace_text_input(
                target,
                TextInputView {
                    value,
                    placeholder,
                    focused,
                    caret_visible: self.input_caret.visible(),
                    secret: false,
                    selected_all: false,
                    selected_range: self.ime_selected_range_for_target(target, cx),
                    marked_text: self.marked_text_for_target(target, cx),
                },
            )
            .h(px(34.0))
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.host_tools.update(cx, |host_tools, _cx| {
                        host_tools.ui.focus_input(input);
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
            }));
        let workspace = cx.entity();
        text_input_anchor_probe(target.anchor_id(), control, move |anchor, _window, cx| {
            let _ = workspace.update(cx, |this, cx| {
                this.update_text_input_anchor(anchor, cx);
            });
        })
        .into_any_element()
    }

    /// Public and signed-in registries offered under the registry field.
    fn container_registry_picks(&self, cx: &App) -> Vec<String> {
        let mut picks = vec![
            oxideterm_connection_monitor::container::DEFAULT_REGISTRY.to_string(),
            "quay.io".to_string(),
        ];
        if let Some(registries) = &self.host_tools.read(cx).host_container.registries {
            for registry in registries {
                let host = registry_host(&registry.registry);
                if !host.is_empty() && !picks.iter().any(|pick| pick.eq_ignore_ascii_case(&host)) {
                    picks.push(host);
                }
            }
        }
        picks
    }

    pub(in crate::workspace) fn render_host_container_input_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        // Copy the dialog's fields out of the entity first: the text-field
        // helpers need `&mut Context`, which cannot overlap the read borrow.
        let (kind, uses_registry, registry_value, value, registry_focused, value_focused) = {
            let host_tools = self.host_tools.read(cx);
            let ui = &host_tools.ui;
            let dialog = ui.host_container_input_dialog.as_ref()?;
            (
                dialog.kind.clone(),
                container_input_uses_registry(&dialog.kind),
                dialog.registry.clone(),
                dialog.value.clone(),
                ui.input_is_focused(HostToolsTextInput::ContainerDialogRegistry),
                ui.input_is_focused(HostToolsTextInput::ContainerDialog),
            )
        };
        let registry_control = uses_registry.then(|| {
            self.render_container_dialog_text_field(
                WorkspaceImeTarget::HostContainerDialogRegistry,
                HostToolsTextInput::ContainerDialogRegistry,
                registry_value.as_str(),
                self.i18n
                    .t("sidebar.host_container.inputs.registry_placeholder"),
                registry_focused,
                cx,
            )
        });
        let input_control = self.render_container_dialog_text_field(
            WorkspaceImeTarget::HostContainerDialog,
            HostToolsTextInput::ContainerDialog,
            value.as_str(),
            self.i18n.t(container_input_placeholder_key(&kind)),
            value_focused,
            cx,
        );
        let submit_disabled = value.trim().is_empty();
        let registry_picks = if uses_registry {
            self.container_registry_picks(cx)
        } else {
            Vec::new()
        };
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
                        .child(
                            div()
                                .p_4()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .when_some(registry_control, |body, registry_control| {
                                    body.child(registry_control).child(
                                        div()
                                            .flex()
                                            .flex_wrap()
                                            .items_center()
                                            .gap_1()
                                            .children(registry_picks.iter().enumerate().map(
                                                |(index, pick)| {
                                                    let pick_value = pick.clone();
                                                    div()
                                                        .id(("host-container-registry-pick", index))
                                                        .px_2()
                                                        .py(px(2.0))
                                                        .rounded(px(self.tokens.radii.sm))
                                                        .cursor_pointer()
                                                        .text_size(px(10.0))
                                                        .text_color(rgb(theme.text_muted))
                                                        .bg(rgb(theme.bg_hover))
                                                        .hover(move |chip| {
                                                            chip.bg(rgb(theme.bg_panel))
                                                        })
                                                        .child(pick.clone())
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            cx.listener(
                                                                move |this,
                                                                      _event,
                                                                      _window,
                                                                      cx| {
                                                                    this.host_tools.update(
                                                                        cx,
                                                                        |host_tools, cx| {
                                                                            if let Some(dialog) = host_tools
                                                                                .ui
                                                                                .host_container_input_dialog
                                                                                .as_mut()
                                                                            {
                                                                                dialog.registry =
                                                                                    pick_value
                                                                                        .clone();
                                                                            }
                                                                            cx.notify();
                                                                        },
                                                                    );
                                                                    cx.stop_propagation();
                                                                },
                                                            ),
                                                        )
                                                        .into_any_element()
                                                },
                                            )),
                                    )
                                })
                                .child(input_control),
                        )
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
        let (kind, registry, value) = {
            let host_tools = self.host_tools.read(cx);
            let Some(dialog) = host_tools.ui.host_container_input_dialog.as_ref() else {
                return;
            };
            (
                dialog.kind.clone(),
                dialog.registry.trim().to_string(),
                dialog.value.trim().to_string(),
            )
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
                registry,
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
        oxideterm_connection_monitor::container::preferred_instance(instances).cloned()
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
                HostContainerTab::Containers => {
                    let mut containers = snapshot
                        .containers
                        .iter()
                        .filter(|container| container.matches_query(&query))
                        .cloned()
                        .collect::<Vec<_>>();
                    if let Some(sort) = self.host_container.container_sort {
                        let stats = self.host_container.stats.as_deref().unwrap_or_default();
                        sort_containers(&mut containers, sort, stats);
                    }
                    rows.extend(containers.into_iter().map(ContainerRow::Container));
                }
                HostContainerTab::Images => {
                    let mut images = snapshot
                        .images
                        .iter()
                        .filter(|image| {
                            matches(&image.repository) || matches(&image.tag) || matches(&image.id)
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    if let Some(sort) = self.host_container.image_sort {
                        oxideterm_connection_monitor::container::sort_images(
                            &mut images,
                            sort,
                            self.host_container.image_sort_descending,
                        );
                    }
                    rows.extend(images.into_iter().map(ContainerRow::Image));
                }
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
                // Registries are captured on demand and rendered from their own
                // snapshot, not from the inventory list.
                HostContainerTab::Registries => {}
            }
        }
        let discovery_error = self
            .host_container
            .discovery
            .as_ref()
            .and_then(|discovery| discovery.error.as_ref())
            .map(|error| error.message().to_string());
        Some(HostContainerPanelSnapshot {
            connections: self.monitor_connections(),
            selected_connection_id,
            instances,
            instance,
            discovery_error,
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
        self.host_container.text_dialog_resize = Some(HostToolsDialogResizeDrag {
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
        if self.host_container.invalidate_container_text_dialog() {
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
        if panel
            .instance
            .as_ref()
            .is_some_and(|instance| instance.engine.supports_registry_auth())
        {
            tabs.push((
                HostContainerTab::Registries,
                "sidebar.host_container.tabs.registries",
            ));
        }
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
                        .child(div().min_w_0().truncate().child(i18n.t(*label_key)))
                        .into_any_element(),
                )
                // Seven possible tabs share one rail: trim the default item
                // padding so every label keeps a readable slice.
                .px(px(4.0))
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
        // Fill shares the panel width; a fixed width clipped the trailing
        // tabs (Pods/Registries) outside the visible panel.
        oxideterm_gpui_ui::segmented_control(
            tokens,
            "host-container-tab-switcher",
            oxideterm_gpui_ui::SegmentedControlOptions::new(active_index, active_index, tabs.len()),
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
            return match panel.discovery_error.as_deref() {
                Some(error) => host_tools_center_state(
                    LucideIcon::AlertTriangle,
                    MONITOR_RED,
                    i18n.t("sidebar.host_container.discovery_failed")
                        .replace("{{error}}", error),
                    selectable_text,
                    cx,
                ),
                None => host_tools_center_state(
                    LucideIcon::Layers,
                    tokens.ui.text_muted,
                    i18n.t("sidebar.host_container.empty.no_engine"),
                    selectable_text,
                    cx,
                ),
            };
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
        if panel.tab == HostContainerTab::Registries {
            return self.render_container_registries(
                tokens,
                i18n,
                mono_font_family,
                selectable_text,
                cx,
            );
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

    fn render_container_registries(
        &self,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        if self.host_container.registries_running {
            return host_tools_center_state(
                LucideIcon::KeyRound,
                theme.text_muted,
                i18n.t("sidebar.host_container.loading"),
                selectable_text,
                cx,
            );
        }
        let registries = self
            .host_container
            .registries
            .as_deref()
            .unwrap_or_default();
        if let Some(error) = self.host_container.registries_error.as_deref() {
            return host_tools_center_state(
                LucideIcon::AlertTriangle,
                MONITOR_RED,
                error.to_string(),
                selectable_text,
                cx,
            );
        }
        if registries.is_empty() {
            return host_tools_center_state(
                LucideIcon::KeyRound,
                theme.text_muted,
                i18n.t("sidebar.host_container.empty.registries"),
                selectable_text,
                cx,
            );
        }
        let rows = registries
            .iter()
            .map(|registry| {
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
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(mono_font_family.clone())
                            .text_size(px(12.0))
                            .text_color(rgb(theme.text))
                            .child(registry.registry.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(10.0))
                            .text_color(rgb(theme.text_muted))
                            .child(i18n.t(registry.source.label_key())),
                    )
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
            .child(
                div()
                    .id("host-container-registries-scroll")
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
            HostContainerTab::Registries => "sidebar.host_container.counts.registries",
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
                            oxideterm_connection_monitor::container::VolumeAction::Prune,
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
                            oxideterm_connection_monitor::container::NetworkAction::Prune,
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
            HostContainerTab::Registries => {}
        }
        let prune_label = i18n.t(suffix_key);
        let buttons = actions
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
                                .request_container_prune(
                                    HostContainerConfirmKind::Image(
                                        oxideterm_connection_monitor::container::ImageAction::Prune,
                                    ),
                                    if *all {
                                        IMAGE_PRUNE_ALL_SCOPE.to_string()
                                    } else {
                                        String::new()
                                    },
                                    prune_label.clone(),
                                    cx,
                                ),
                            ContainerCountAction::Confirm(kind) => host_tools
                                .request_container_prune(
                                    *kind,
                                    String::new(),
                                    prune_label.clone(),
                                    cx,
                                ),
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
                    .when(panel.tab == HostContainerTab::Containers, |row| {
                        row.child(self.render_container_metric_sort_button(
                            ContainerSortKey::Cpu,
                            "sidebar.host_container.sort.cpu",
                            tokens,
                            i18n,
                            cx,
                        ))
                        .child(self.render_container_metric_sort_button(
                            ContainerSortKey::Memory,
                            "sidebar.host_container.sort.memory",
                            tokens,
                            i18n,
                            cx,
                        ))
                    })
                    .when(panel.tab == HostContainerTab::Images, |row| {
                        row.child(self.render_container_sort_button(
                            oxideterm_connection_monitor::container::ImageSort::Size,
                            "sidebar.host_container.sort.size",
                            tokens,
                            i18n,
                            cx,
                        ))
                        .child(self.render_container_sort_button(
                            oxideterm_connection_monitor::container::ImageSort::Created,
                            "sidebar.host_container.sort.time",
                            tokens,
                            i18n,
                            cx,
                        ))
                    })
                    .children(buttons),
            )
            .into_any_element()
    }

    fn render_container_sort_button(
        &self,
        sort: oxideterm_connection_monitor::container::ImageSort,
        label_key: &'static str,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self.host_container.image_sort == Some(sort);
        let descending = self.host_container.image_sort_descending;
        let theme = tokens.ui;
        let mut label = i18n.t(label_key);
        if active {
            label.push_str(if descending { " ↓" } else { " ↑" });
        }
        div()
            .flex_none()
            .px_1p5()
            .h(px(22.0))
            .flex()
            .items_center()
            .rounded(px(tokens.radii.sm))
            .cursor_pointer()
            .bg(if active {
                rgb(theme.bg_hover)
            } else {
                rgba(0x00000000)
            })
            .text_color(if active {
                rgb(theme.text)
            } else {
                rgb(theme.text_muted)
            })
            .hover(move |button| button.bg(rgb(theme.bg_hover)))
            .child(label)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |host_tools, _event, _window, cx| {
                    let state = &mut host_tools.host_container;
                    let next = next_image_sort(state.image_sort, state.image_sort_descending, sort);
                    state.image_sort = next.0;
                    state.image_sort_descending = next.1;
                    cx.notify();
                    cx.stop_propagation();
                }),
            )
            .into_any_element()
    }

    fn render_container_metric_sort_button(
        &self,
        key: ContainerSortKey,
        label_key: &'static str,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self
            .host_container
            .container_sort
            .filter(|sort| sort.key == key);
        let theme = tokens.ui;
        let mut label = i18n.t(label_key);
        if let Some(sort) = active {
            label.push_str(if sort.descending { " ↓" } else { " ↑" });
        }
        div()
            .flex_none()
            .px_1p5()
            .h(px(22.0))
            .flex()
            .items_center()
            .rounded(px(tokens.radii.sm))
            .cursor_pointer()
            .bg(if active.is_some() {
                rgb(theme.bg_hover)
            } else {
                rgba(0x00000000)
            })
            .text_color(if active.is_some() {
                rgb(theme.text)
            } else {
                rgb(theme.text_muted)
            })
            .hover(move |button| button.bg(rgb(theme.bg_hover)))
            .child(label)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |host_tools, _event, _window, cx| {
                    let state = &mut host_tools.host_container;
                    state.container_sort = next_container_sort(state.container_sort, key);
                    cx.notify();
                    cx.stop_propagation();
                }),
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
                HostContainerTab::Registries => "sidebar.host_container.empty.registries",
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
            "host-container:{}:{:?}:{}:{:?}:{:?}:{}",
            panel.selected_connection_id,
            panel.tab,
            self.ui.host_container_search_query,
            self.host_container.container_sort,
            self.host_container.image_sort,
            self.host_container.image_sort_descending
        );
        sync_container_list_state(
            &self.host_container.list_state,
            &mut self.host_container.list_cache.borrow_mut(),
            &identity,
            &panel.rows,
            self.host_container.selected_row.as_deref(),
        );
        let rows = Arc::new(panel.rows.clone());
        let instance = panel.instance.clone();
        let connection_id = Arc::new(panel.selected_connection_id.clone());
        let list_state = self.host_container.list_state.clone();
        // Containers carry the sampled CPU/memory/PID line, so their estimate
        // is one text line taller than the other inventories.
        let row_height = if panel.tab == HostContainerTab::Containers {
            104.0
        } else {
            88.0
        };
        let list_spec = TauriVirtualListSpec::new(px(row_height), 6);
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
        element_id: gpui::ElementId,
        enabled: bool,
        cx: &mut Context<Self>,
        listener: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let theme = tokens.ui;
        let options = IconButtonOptions {
            size: 20.0,
            has_background: true,
            background: Some(rgb(theme.bg_hover)),
            hover_background: Some(rgb(theme.bg_panel)),
            idle_opacity: if enabled { 1.0 } else { 0.35 },
            disabled: !enabled,
            ..IconButtonOptions::compact(20.0)
        };
        *row = std::mem::replace(row, div()).child(host_tools_scoped_tooltip_icon_button(
            tokens,
            icon,
            11.0,
            rgb(theme.text),
            options,
            tooltip,
            element_id,
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
        let terminal_os_type = self.connection_os_type(connection_id).unwrap_or_default();
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
                let selected =
                    self.host_container.selected_row.as_deref() == Some(container.id.as_str());
                let id = container.id.clone();
                let name = container.name.clone();
                let connection_id_owned = connection_id.to_string();
                let mut actions = div()
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_end()
                    .gap(px(1.0));
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
                    host_tools_row_id(
                        "host-container-inspect",
                        &connection_id_owned,
                        container.id.clone(),
                    ),
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
                        host_tools_row_id(
                            "host-container-logs",
                            &connection_id_owned,
                            container.id.clone(),
                        ),
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
                    let Ok(command) = oxideterm_connection_monitor::container::follow_logs_command(
                        &terminal_os_type,
                        &instance,
                        &id,
                    ) else {
                        return shell.into_any_element();
                    };
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::Radio,
                        i18n.t("sidebar.host_container.actions.follow_logs"),
                        host_tools_row_id(
                            "host-container-follow-logs",
                            &connection_id_owned,
                            container.id.clone(),
                        ),
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
                    let Ok(command) = oxideterm_connection_monitor::container::exec_shell_command(
                        &terminal_os_type,
                        &instance,
                        &id,
                    ) else {
                        return shell.into_any_element();
                    };
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::Terminal,
                        i18n.t("sidebar.host_container.actions.exec"),
                        host_tools_row_id(
                            "host-container-exec",
                            &connection_id_owned,
                            container.id.clone(),
                        ),
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
                    if let Ok(command) = oxideterm_connection_monitor::container::terminal_command(
                        &terminal_os_type,
                        &instance,
                        oxideterm_connection_monitor::container::ContainerTerminalOp::Attach {
                            container_id: id,
                        },
                    ) {
                        self.render_row_action(
                            &mut actions,
                            tokens,
                            LucideIcon::Cable,
                            i18n.t("sidebar.host_container.actions.attach"),
                            host_tools_row_id(
                                "host-container-attach",
                                &connection_id_owned,
                                container.id.clone(),
                            ),
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
                        host_tools_row_id(
                            "host-container-export",
                            &connection_id_owned,
                            container.id.clone(),
                        ),
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
                        host_tools_row_id(
                            "host-container-copy",
                            &connection_id_owned,
                            container.id.clone(),
                        ),
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
                        host_tools_row_id(
                            "host-container-rename",
                            &connection_id_owned,
                            container.id.clone(),
                        ),
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
                        oxideterm_connection_monitor::container::ContainerAction::Start,
                        LucideIcon::Play,
                        availability.can_start,
                    ),
                    (
                        oxideterm_connection_monitor::container::ContainerAction::Stop,
                        LucideIcon::StopCircle,
                        availability.can_stop,
                    ),
                    (
                        oxideterm_connection_monitor::container::ContainerAction::Restart,
                        LucideIcon::RefreshCw,
                        availability.can_restart,
                    ),
                ] {
                    podman_actions.push((action, icon, enabled));
                }
                if instance.engine.supports_pause() {
                    podman_actions.push((
                        oxideterm_connection_monitor::container::ContainerAction::Pause,
                        LucideIcon::Pause,
                        availability.can_pause,
                    ));
                    podman_actions.push((
                        oxideterm_connection_monitor::container::ContainerAction::Unpause,
                        LucideIcon::Play,
                        availability.can_unpause,
                    ));
                }
                podman_actions.push((
                    oxideterm_connection_monitor::container::ContainerAction::Remove,
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
                        host_tools_row_id(
                            match action {
                                oxideterm_connection_monitor::container::ContainerAction::Start => {
                                    "host-container-start"
                                }
                                oxideterm_connection_monitor::container::ContainerAction::Stop => "host-container-stop",
                                oxideterm_connection_monitor::container::ContainerAction::Restart => {
                                    "host-container-restart"
                                }
                                oxideterm_connection_monitor::container::ContainerAction::Pause => {
                                    "host-container-pause"
                                }
                                oxideterm_connection_monitor::container::ContainerAction::Unpause => {
                                    "host-container-unpause"
                                }
                                oxideterm_connection_monitor::container::ContainerAction::Remove => {
                                    "host-container-remove"
                                }
                            },
                            &connection_id_owned,
                            container.id.clone(),
                        ),
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
                    i18n.t(
                        oxideterm_connection_monitor::container::container_state_label_key(
                            &container.state,
                        ),
                    )
                    .to_string()
                } else {
                    container.status.clone()
                };
                // CPU, memory and the host PID share one muted line: they are
                // sampled, so a separate column per value would read as a table
                // that also moves the rows on every refresh.
                let stat = container_stat_for(
                    self.host_container.stats.as_deref().unwrap_or_default(),
                    &container.id,
                );
                // A failed sample read keeps its reason in the row instead of
                // showing dashes that would read like "not running".
                let (stats_line, stats_color) = match self.host_container.stats_error.as_deref() {
                    Some(error) => (error.to_string(), MONITOR_RED),
                    None => (
                        format!(
                            "{} {} · {} {} · {} {}",
                            i18n.t("sidebar.host_container.columns.cpu"),
                            stat.map(|stat| stat.cpu_percent.as_str())
                                .filter(|value| !value.is_empty())
                                .unwrap_or("—"),
                            i18n.t("sidebar.host_container.columns.memory"),
                            stat.map(|stat| stat.mem_usage.as_str())
                                .filter(|value| !value.is_empty())
                                .unwrap_or("—"),
                            i18n.t("sidebar.host_container.columns.pid"),
                            container
                                .pid
                                .map(|pid| pid.to_string())
                                .unwrap_or_else(|| "—".to_string()),
                        ),
                        theme.text_muted,
                    ),
                };
                // Info and actions are separate lines: a single row let the
                // action strip squeeze the container name out of the panel.
                shell
                    .when(selected, |row| row.bg(rgb(theme.bg_hover)))
                    .flex_col()
                    .items_stretch()
                    .gap(px(3.0))
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_2()
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
                                    .truncate()
                                    .text_size(px(12.0))
                                    .text_color(rgb(theme.text))
                                    .child(container.name.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .max_w(px(110.0))
                                    .truncate()
                                    .text_size(px(10.0))
                                    .text_color(rgb(theme.text_muted))
                                    .child(status),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(mono_font_family.clone())
                                    .text_size(px(10.0))
                                    .text_color(rgb(theme.text_muted))
                                    .child(container.image.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .max_w(px(120.0))
                                    .truncate()
                                    .text_size(px(10.0))
                                    .text_color(rgb(theme.text_muted))
                                    .child(container.ports.clone().unwrap_or_default()),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .truncate()
                            .text_size(px(10.0))
                            .text_color(rgb(stats_color))
                            .child(stats_line),
                    )
                    .when(selected, |row| row.child(actions))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener({
                            let row_id = container.id.clone();
                            move |host_tools, _event, _window, cx| {
                                host_tools.toggle_container_row_selection(row_id.clone(), cx);
                                cx.stop_propagation();
                            }
                        }),
                    )
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
                let row_identity = image_row_identity(&image);
                let selected =
                    self.host_container.selected_row.as_deref() == Some(row_identity.as_str());
                let connection_id_owned = connection_id.to_string();
                let mut actions = div()
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_end()
                    .gap(px(1.0));
                {
                    let image_id = image.id.clone();
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::Copy,
                        i18n.t("sidebar.host_container.actions.copy_id"),
                        host_tools_row_id(
                            "host-container-image-copy-id",
                            &connection_id_owned,
                            row_identity.clone(),
                        ),
                        true,
                        cx,
                        move |host_tools, _window, cx| {
                            host_tools.copy_container_row_id(image_id.clone(), cx);
                        },
                    );
                }
                {
                    let image_id = image.id.clone();
                    self.render_row_action(
                        &mut actions,
                        tokens,
                        LucideIcon::Hash,
                        i18n.t("sidebar.host_container.actions.tag"),
                        host_tools_row_id(
                            "host-container-image-tag",
                            &connection_id_owned,
                            row_identity.clone(),
                        ),
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
                        host_tools_row_id(
                            "host-container-image-inspect",
                            &connection_id_owned,
                            row_identity.clone(),
                        ),
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
                        host_tools_row_id(
                            "host-container-image-history",
                            &connection_id_owned,
                            row_identity.clone(),
                        ),
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
                        host_tools_row_id(
                            "host-container-image-save",
                            &connection_id_owned,
                            row_identity.clone(),
                        ),
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
                        host_tools_row_id(
                            "host-container-image-remove",
                            &connection_id_owned,
                            row_identity.clone(),
                        ),
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
                                        oxideterm_connection_monitor::container::ImageAction::Remove,
                                    ),
                                },
                                cx,
                            );
                        },
                    );
                }
                // Tag and ID are the long fields; size and creation time sit at
                // the end of each line so they stay readable next to them.
                shell
                    .when(selected, |row| row.bg(rgb(theme.bg_hover)))
                    .flex_col()
                    .items_stretch()
                    .gap(px(3.0))
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.0))
                                    .text_color(rgb(theme.text))
                                    .child(reference),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(10.0))
                                    .text_color(rgb(theme.text_muted))
                                    .child(image_size_label(&image)),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(mono_font_family.clone())
                                    .text_size(px(10.0))
                                    .text_color(rgb(theme.text_muted))
                                    .child(image.id.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(10.0))
                                    .text_color(rgb(theme.text_muted))
                                    .child(image_created_label(&image)),
                            ),
                    )
                    .when(selected, |row| row.child(actions))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener({
                            let row_id = row_identity.clone();
                            move |host_tools, _event, _window, cx| {
                                host_tools.toggle_container_row_selection(row_id.clone(), cx);
                                cx.stop_propagation();
                            }
                        }),
                    )
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
                        host_tools_row_id(
                            "host-container-volume-inspect",
                            &connection_id_owned,
                            volume.name.clone(),
                        ),
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
                        host_tools_row_id(
                            "host-container-volume-usage",
                            &connection_id_owned,
                            volume.name.clone(),
                        ),
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
                        host_tools_row_id(
                            "host-container-volume-remove",
                            &connection_id_owned,
                            volume.name.clone(),
                        ),
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
                                        oxideterm_connection_monitor::container::VolumeAction::Remove,
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
                        host_tools_row_id(
                            "host-container-network-inspect",
                            &connection_id_owned,
                            network.name.clone(),
                        ),
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
                        host_tools_row_id(
                            "host-container-network-remove",
                            &connection_id_owned,
                            network.name.clone(),
                        ),
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
                                        oxideterm_connection_monitor::container::NetworkAction::Remove,
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
                let selected = self.host_container.selected_row.as_deref() == Some(pod.id.as_str());
                let connection_id_owned = connection_id.to_string();
                let mut actions = div()
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_end()
                    .gap(px(1.0));
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
                        host_tools_row_id(
                            "host-container-pod-inspect",
                            &connection_id_owned,
                            pod.id.clone(),
                        ),
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
                        oxideterm_connection_monitor::container::PodAction::Start,
                        LucideIcon::Play,
                        availability.can_start,
                    ),
                    (
                        oxideterm_connection_monitor::container::PodAction::Stop,
                        LucideIcon::StopCircle,
                        availability.can_stop,
                    ),
                    (
                        oxideterm_connection_monitor::container::PodAction::Restart,
                        LucideIcon::RefreshCw,
                        availability.can_restart,
                    ),
                    (
                        oxideterm_connection_monitor::container::PodAction::Pause,
                        LucideIcon::Pause,
                        availability.can_pause,
                    ),
                    (
                        oxideterm_connection_monitor::container::PodAction::Unpause,
                        LucideIcon::Play,
                        availability.can_unpause,
                    ),
                    (
                        oxideterm_connection_monitor::container::PodAction::Remove,
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
                        host_tools_row_id(
                            match action {
                                oxideterm_connection_monitor::container::PodAction::Start => {
                                    "host-container-pod-start"
                                }
                                oxideterm_connection_monitor::container::PodAction::Stop => {
                                    "host-container-pod-stop"
                                }
                                oxideterm_connection_monitor::container::PodAction::Restart => {
                                    "host-container-pod-restart"
                                }
                                oxideterm_connection_monitor::container::PodAction::Pause => {
                                    "host-container-pod-pause"
                                }
                                oxideterm_connection_monitor::container::PodAction::Unpause => {
                                    "host-container-pod-unpause"
                                }
                                oxideterm_connection_monitor::container::PodAction::Remove => {
                                    "host-container-pod-remove"
                                }
                            },
                            &connection_id_owned,
                            pod.id.clone(),
                        ),
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
                    .when(selected, |row| row.bg(rgb(theme.bg_hover)))
                    .flex_col()
                    .items_stretch()
                    .gap(px(3.0))
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_2()
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
                                    .text_color(rgb(theme.text))
                                    .child(pod.name.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(10.0))
                                    .text_color(rgb(theme.text_muted))
                                    .child(
                                        i18n.t("sidebar.host_container.pod_containers")
                                            .replace("{{count}}", &pod.containers.to_string()),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .max_w(px(110.0))
                                    .truncate()
                                    .text_size(px(10.0))
                                    .text_color(rgb(theme.text_muted))
                                    .child(
                                        i18n.t(oxideterm_connection_monitor::container::pod_state_label_key(
                                            &pod.status,
                                        )),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(mono_font_family.clone())
                                    .text_size(px(10.0))
                                    .text_color(rgb(theme.text_muted))
                                    .child(pod.id.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(10.0))
                                    .text_color(rgb(theme.text_muted))
                                    .child(pod.created.clone().unwrap_or_default()),
                            ),
                    )
                    .when(selected, |row| row.child(actions))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener({
                            let row_id = pod.id.clone();
                            move |host_tools, _event, _window, cx| {
                                host_tools.toggle_container_row_selection(row_id.clone(), cx);
                                cx.stop_propagation();
                            }
                        }),
                    )
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
            self.host_container.selected_row = None;
            self.host_container.list_state = tauri_virtual_list_state(
                0,
                ListAlignment::Top,
                TauriVirtualListSpec::new(px(64.0), 6),
            );
            // The Containers tab renders the live CPU/memory/PID line, so the
            // stats capture follows the tab instead of owning one.
            if tab == HostContainerTab::Containers
                && let Some(connection_id) = self.selected_connection_id_owned()
            {
                self.request_container_stats(connection_id, cx);
            }
            if tab == HostContainerTab::Registries
                && let Some(connection_id) = self.selected_connection_id_owned()
            {
                self.request_container_registries(connection_id, cx);
            }
            cx.notify();
        }
    }

    /// Image IDs are long and cannot be selected from the row, so the action
    /// copies the exact value the engine reported.
    pub(in crate::workspace::connection_monitor) fn copy_container_row_id(
        &mut self,
        id: String,
        cx: &mut Context<Self>,
    ) {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(id.clone()));
        cx.emit(HostToolsEvent::ShowNotice(
            HostToolsNotice::ContainerCopied { label: id },
        ));
    }

    pub(in crate::workspace::connection_monitor) fn toggle_container_row_selection(
        &mut self,
        row_id: String,
        cx: &mut Context<Self>,
    ) {
        self.host_container.selected_row =
            if self.host_container.selected_row.as_deref() == Some(row_id.as_str()) {
                None
            } else {
                Some(row_id)
            };
        cx.notify();
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
            // Stats and registries belong to the engine that produced them.
            // A capture still running for the old engine is retired too: its
            // delivery is dropped by the request's instance guard and must not
            // keep the new engine's read from starting.
            self.host_container.stats = None;
            self.host_container.stats_error = None;
            self.host_container.stats_running = false;
            self.host_container.registries = None;
            self.host_container.registries_error = None;
            self.host_container.registries_running = false;
            // Pods only exist on Podman and registry auth is absent from the
            // WSL container CLI; drop those tabs when the engine changes.
            if !self.host_container.tab.supported_by(instance.engine) {
                self.host_container.tab = HostContainerTab::Containers;
                self.host_container.selected_row = None;
            }
            if let Some(connection_id) = self.selected_connection_id_owned() {
                self.request_container_snapshot(connection_id.clone(), instance, cx);
                // The visible capture tab describes the engine that is gone.
                match self.host_container.tab {
                    HostContainerTab::Containers => {
                        self.request_container_stats(connection_id, cx);
                    }
                    HostContainerTab::Registries => {
                        self.request_container_registries(connection_id, cx);
                    }
                    _ => {}
                }
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
            instance_id: None,
            failure_fallback: self
                .messages
                .as_ref()
                .map(|messages| messages.container_capture_failed.clone())
                .unwrap_or_else(|| "Container detection failed.".to_string()),
        };
        let command = oxideterm_connection_monitor::container::discovery_command(&os_type);
        // A superseding probe aborts the previous host's capture instead of
        // letting it hold an SSH route for the rest of its 40s timeout.
        self.cancel_container_discovery();
        self.host_container.discovery_task = self.spawn_container_capture_handle(
            command,
            request,
            Duration::from_secs(40),
            128 * 1024,
            runtime,
        );
        if self.host_container.discovery_task.is_some() {
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
            instance_id: Some(instance.id()),
            failure_fallback: self
                .messages
                .as_ref()
                .map(|messages| messages.container_capture_failed.clone())
                .unwrap_or_else(|| "Container snapshot failed.".to_string()),
        };
        let command =
            oxideterm_connection_monitor::container::snapshot_command(&os_type, &instance);
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
        // A new dialog epoch retires every capture started for the prior one.
        let generation = self.host_container.begin_container_text_dialog();
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
            instance_id: Some(instance.id()),
            failure_fallback: self
                .messages
                .as_ref()
                .map(|messages| messages.container_capture_failed.clone())
                .unwrap_or_else(|| "Container capture failed.".to_string()),
        };
        let command = match oxideterm_connection_monitor::container::capture_command(
            &os_type, &instance, &capture,
        ) {
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
            self.host_container.text_dialog_running_generation = Some(generation);
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
        // A capture can finish after the user switched hosts or engines; its
        // output belongs to the old selection and must not be installed on the
        // current one. Stats and registry reads also carry the engine they were
        // issued for, so a slower old engine cannot overwrite the new one.
        let connection_stale = self.selected_connection_id_owned().as_deref()
            != Some(delivery.request.connection_id.as_str());
        let engine_stale = matches!(
            &delivery.request.kind,
            HostContainerCaptureKind::Stats | HostContainerCaptureKind::Registries
        ) && delivery.request.instance_id.as_deref().is_some_and(
            |instance_id| self.host_container.selected_instance_id.as_deref() != Some(instance_id),
        );
        let stale = connection_stale || engine_stale;
        match delivery.request.kind {
            HostContainerCaptureKind::Discovery => {
                if stale {
                    // The probe was superseded; only clear the marker it owned
                    // so a newer probe's handle and running state survive.
                    if self.host_container.discovery_connection_id.as_deref()
                        == Some(delivery.request.connection_id.as_str())
                    {
                        self.host_container.discovery_task = None;
                        self.host_container.discovery_running = false;
                    }
                    cx.notify();
                    return;
                }
                self.host_container.discovery_task = None;
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
                let discovery = oxideterm_connection_monitor::container::parse_discovery(&text);
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
                // A refresh can drop the engine that owned the current tab.
                if let Some(instance) = self.selected_container_instance()
                    && !self.host_container.tab.supported_by(instance.engine)
                {
                    self.host_container.tab = HostContainerTab::Containers;
                    self.host_container.selected_row = None;
                }
                if let Some(instance) = self.selected_container_instance() {
                    self.host_container.selected_instance_id = Some(instance.id());
                    self.request_container_snapshot(connection_id, instance, cx);
                } else {
                    self.host_container.snapshot = None;
                    self.host_container.snapshot_connection_id = None;
                }
                cx.notify();
            }
            HostContainerCaptureKind::Registries => {
                self.host_container.registries_running = false;
                if stale {
                    cx.notify();
                    return;
                }
                let failure = delivery.request.failure_fallback.clone();
                let (registries, error) = match delivery.result {
                    Ok(mut output) if output.exit_code.unwrap_or(0) == 0 => {
                        let text = std::mem::take(&mut output.stdout);
                        zeroize::Zeroize::zeroize(&mut output.stderr);
                        (
                            oxideterm_connection_monitor::container::parse_registries(&text),
                            None,
                        )
                    }
                    Ok(mut output) => {
                        zeroize::Zeroize::zeroize(&mut output.stdout);
                        zeroize::Zeroize::zeroize(&mut output.stderr);
                        // An unreachable engine must not look like "no registries"
                        // once the notice is gone; the body keeps the error.
                        cx.emit(HostToolsEvent::ShowNotice(
                            HostToolsNotice::ContainerCaptureFailed,
                        ));
                        (Vec::new(), Some(failure))
                    }
                    Err(_) => {
                        cx.emit(HostToolsEvent::ShowNotice(
                            HostToolsNotice::ContainerCaptureFailed,
                        ));
                        (Vec::new(), Some(failure))
                    }
                };
                self.host_container.registries = Some(registries);
                self.host_container.registries_error = error;
                cx.notify();
            }
            HostContainerCaptureKind::Stats => {
                self.host_container.stats_running = false;
                if stale {
                    cx.notify();
                    return;
                }
                let failure = delivery.request.failure_fallback.clone();
                let (stats, error) = match delivery.result {
                    Ok(mut output) if output.exit_code.unwrap_or(0) == 0 => {
                        let text = std::mem::take(&mut output.stdout);
                        zeroize::Zeroize::zeroize(&mut output.stderr);
                        match oxideterm_connection_monitor::container::parse_stats(&text) {
                            Ok(stats) => (stats, None),
                            Err(_) => {
                                // A failed read is not an empty table.
                                cx.emit(HostToolsEvent::ShowNotice(
                                    HostToolsNotice::ContainerCaptureFailed,
                                ));
                                (Vec::new(), Some(failure))
                            }
                        }
                    }
                    Ok(mut output) => {
                        zeroize::Zeroize::zeroize(&mut output.stdout);
                        zeroize::Zeroize::zeroize(&mut output.stderr);
                        cx.emit(HostToolsEvent::ShowNotice(
                            HostToolsNotice::ContainerCaptureFailed,
                        ));
                        (Vec::new(), Some(failure))
                    }
                    Err(_) => {
                        cx.emit(HostToolsEvent::ShowNotice(
                            HostToolsNotice::ContainerCaptureFailed,
                        ));
                        (Vec::new(), Some(failure))
                    }
                };
                self.host_container.stats = Some(stats);
                self.host_container.stats_error = error;
                cx.notify();
            }
            HostContainerCaptureKind::Snapshot(instance) => {
                self.host_container.snapshot_running = false;
                if stale {
                    cx.notify();
                    return;
                }
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
                        let mut parsed =
                            oxideterm_connection_monitor::container::parse_snapshot(&text);
                        // A capture without any recognised section never
                        // produced a snapshot; surface it instead of an empty list.
                        if matches!(parsed.status, ContainerStatus::Unknown) {
                            parsed.status = ContainerStatus::Error {
                                message: delivery.request.failure_fallback.clone(),
                            };
                        }
                        parsed
                    }
                    Ok(mut output) => {
                        let hint = first_stderr_line(&output.stderr)
                            .unwrap_or_else(|| delivery.request.failure_fallback.clone());
                        zeroize::Zeroize::zeroize(&mut output.stdout);
                        zeroize::Zeroize::zeroize(&mut output.stderr);
                        oxideterm_connection_monitor::container::ContainerSnapshot {
                            status: ContainerStatus::Error { message: hint },
                            ..oxideterm_connection_monitor::container::ContainerSnapshot::default()
                        }
                    }
                    Err(_) => oxideterm_connection_monitor::container::ContainerSnapshot {
                        status: ContainerStatus::Error {
                            message: delivery.request.failure_fallback.clone(),
                        },
                        ..oxideterm_connection_monitor::container::ContainerSnapshot::default()
                    },
                };
                self.host_container.snapshot_connection_id =
                    Some(delivery.request.connection_id.clone());
                self.host_container.selected_instance_id = Some(instance.id());
                self.host_container.snapshot = Some(snapshot);
                // The rows now show sampled CPU/memory, so a fresh inventory
                // must also refresh the samples that belong to it.
                if self.host_container.tab == HostContainerTab::Containers {
                    self.request_container_stats(delivery.request.connection_id.clone(), cx);
                }
                cx.notify();
            }
            HostContainerCaptureKind::Text { .. } => {
                // The dialog may have been replaced by another capture while
                // this one ran; only the request opened for the current dialog
                // generation may fill it.
                let matches_dialog = self.host_container.text_dialog_running_generation
                    == Some(self.host_container.text_dialog_generation)
                    && self
                        .host_container
                        .text_dialog_running
                        .as_ref()
                        .is_some_and(|running| {
                            running.connection_id == delivery.request.connection_id
                                && same_container_capture(&running.kind, &delivery.request.kind)
                        });
                if !matches_dialog {
                    // A superseded capture must not clear the newer request's
                    // marker, or its dialog would stay loading forever.
                    cx.notify();
                    return;
                }
                self.host_container.text_dialog_running = None;
                self.host_container.text_dialog_running_generation = None;
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
                oxideterm_connection_monitor::container::container_action_command(
                    &os_type,
                    &request.instance,
                    *action,
                    target,
                )
            }
            HostContainerConfirmKind::Image(
                oxideterm_connection_monitor::container::ImageAction::Prune,
            ) => {
                // The header's "prune unused" control widens the same confirmed
                // action instead of running an unconfirmed command.
                oxideterm_connection_monitor::container::prune_images_command(
                    &os_type,
                    &request.instance,
                    target == IMAGE_PRUNE_ALL_SCOPE,
                )
            }
            HostContainerConfirmKind::Image(action) => {
                oxideterm_connection_monitor::container::image_action_command(
                    &os_type,
                    &request.instance,
                    *action,
                    Some(target),
                )
            }
            HostContainerConfirmKind::Volume(action) => {
                oxideterm_connection_monitor::container::volume_action_command(
                    &os_type,
                    &request.instance,
                    *action,
                    Some(target),
                )
            }
            HostContainerConfirmKind::Network(action) => {
                oxideterm_connection_monitor::container::network_action_command(
                    &os_type,
                    &request.instance,
                    *action,
                    Some(target),
                )
            }
            HostContainerConfirmKind::Pod(action) => {
                oxideterm_connection_monitor::container::pod_action_command(
                    &os_type,
                    &request.instance,
                    *action,
                    target,
                )
            }
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

    pub(in crate::workspace::connection_monitor) fn request_container_registries(
        &mut self,
        connection_id: String,
        cx: &mut Context<Self>,
    ) {
        if self.host_container.registries_running {
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
            kind: HostContainerCaptureKind::Registries,
            instance_id: Some(instance.id()),
            failure_fallback: self
                .messages
                .as_ref()
                .map(|messages| messages.container_capture_failed.clone())
                .unwrap_or_else(|| "Container registry read failed.".to_string()),
        };
        let command =
            oxideterm_connection_monitor::container::registries_command(&os_type, &instance);
        if self.spawn_container_capture(
            command,
            request,
            HOST_CONTAINER_CAPTURE_TIMEOUT,
            128 * 1024,
            runtime,
        ) {
            self.host_container.registries_running = true;
            self.host_container.registries = None;
            self.host_container.registries_error = None;
            cx.notify();
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
            instance_id: Some(instance.id()),
            failure_fallback: self
                .messages
                .as_ref()
                .map(|messages| messages.container_capture_failed.clone())
                .unwrap_or_else(|| "Container stats failed.".to_string()),
        };
        let command = oxideterm_connection_monitor::container::stats_command(&os_type, &instance);
        if self.spawn_container_capture(
            command,
            request,
            Duration::from_secs(20),
            HOST_CONTAINER_CAPTURE_MAX_OUTPUT,
            runtime,
        ) {
            self.host_container.stats_running = true;
            self.host_container.stats_error = None;
            cx.notify();
        }
    }

    pub(in crate::workspace::connection_monitor) fn open_container_input_dialog(
        &mut self,
        kind: HostContainerInputKind,
        initial: &str,
        cx: &mut Context<Self>,
    ) {
        let uses_registry = container_input_uses_registry(&kind);
        self.ui.host_container_input_dialog = Some(HostContainerInputDialog {
            kind,
            registry: String::new(),
            value: initial.to_string(),
        });
        self.ui.focus_input(HostToolsTextInput::ContainerDialog);
        // Signed-in registries are offered as picks; load them once in the
        // background so the dialog opens without waiting on the engine.
        if uses_registry
            && self.host_container.registries.is_none()
            && !self.host_container.registries_running
            && let Some(connection_id) = self.selected_connection_id_owned()
        {
            self.request_container_registries(connection_id, cx);
        }
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
        registry: String,
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
                match oxideterm_connection_monitor::container::terminal_command(
                    &os_type,
                    &instance,
                    oxideterm_connection_monitor::container::ContainerTerminalOp::Run {
                        args: value,
                    },
                ) {
                    Ok(command) => dispatch_terminal(self, command, "run".to_string(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Pull => {
                let reference = oxideterm_connection_monitor::container::qualified_image_reference(
                    &registry, &value,
                );
                match oxideterm_connection_monitor::container::terminal_command(
                    &os_type,
                    &instance,
                    oxideterm_connection_monitor::container::ContainerTerminalOp::Pull {
                        reference,
                    },
                ) {
                    Ok(command) => dispatch_terminal(self, command, "pull".to_string(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Push => {
                let reference = oxideterm_connection_monitor::container::qualified_image_reference(
                    &registry, &value,
                );
                match oxideterm_connection_monitor::container::terminal_command(
                    &os_type,
                    &instance,
                    oxideterm_connection_monitor::container::ContainerTerminalOp::Push {
                        reference,
                    },
                ) {
                    Ok(command) => dispatch_terminal(self, command, "push".to_string(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Build => {
                match oxideterm_connection_monitor::container::terminal_command(
                    &os_type,
                    &instance,
                    oxideterm_connection_monitor::container::ContainerTerminalOp::Build {
                        args: value,
                    },
                ) {
                    Ok(command) => dispatch_terminal(self, command, "build".to_string(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Login => {
                match oxideterm_connection_monitor::container::terminal_command(
                    &os_type,
                    &instance,
                    oxideterm_connection_monitor::container::ContainerTerminalOp::Login {
                        registry: value,
                    },
                ) {
                    Ok(command) => dispatch_terminal(self, command, "login".to_string(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Load => {
                match oxideterm_connection_monitor::container::terminal_command(
                    &os_type,
                    &instance,
                    oxideterm_connection_monitor::container::ContainerTerminalOp::Load {
                        path: value,
                    },
                ) {
                    Ok(command) => dispatch_terminal(self, command, "load".to_string(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Import => {
                let mut parts = value.split_whitespace();
                let path = parts.next().unwrap_or_default().to_string();
                let reference = parts.next().unwrap_or_default().to_string();
                match oxideterm_connection_monitor::container::terminal_command(
                    &os_type,
                    &instance,
                    oxideterm_connection_monitor::container::ContainerTerminalOp::Import {
                        path,
                        reference,
                    },
                ) {
                    Ok(command) => dispatch_terminal(self, command, "import".to_string(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Save { image_id } => {
                match oxideterm_connection_monitor::container::terminal_command(
                    &os_type,
                    &instance,
                    oxideterm_connection_monitor::container::ContainerTerminalOp::Save {
                        reference: image_id,
                        path: value,
                    },
                ) {
                    Ok(command) => dispatch_terminal(self, command, "save".to_string(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Export { container_id } => {
                match oxideterm_connection_monitor::container::terminal_command(
                    &os_type,
                    &instance,
                    oxideterm_connection_monitor::container::ContainerTerminalOp::Export {
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
                match oxideterm_connection_monitor::container::terminal_command(
                    &os_type,
                    &instance,
                    oxideterm_connection_monitor::container::ContainerTerminalOp::Copy {
                        source: format!("{container_id}:{source}"),
                        destination,
                    },
                ) {
                    Ok(command) => dispatch_terminal(self, command, "cp".to_string(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Tag { image_id } => {
                match oxideterm_connection_monitor::container::tag_image_command(
                    &os_type, &instance, &image_id, &value,
                ) {
                    Ok(command) => self.start_container_raw_action(command, value.clone(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Rename { container_id } => {
                match oxideterm_connection_monitor::container::rename_container_command(
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
                match oxideterm_connection_monitor::container::create_volume_command(
                    &os_type, &instance, &value,
                ) {
                    Ok(command) => self.start_container_raw_action(command, value.clone(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::CreateNetwork => {
                let mut parts = value.split_whitespace();
                let name = parts.next().unwrap_or_default().to_string();
                let driver = parts.next().map(str::to_string);
                match oxideterm_connection_monitor::container::create_network_command(
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
                match oxideterm_connection_monitor::container::create_pod_command(
                    &os_type, &instance, &value,
                ) {
                    Ok(command) => self.start_container_raw_action(command, value.clone(), cx),
                    Err(_) => failed(cx),
                }
            }
            HostContainerInputKind::Search => {
                self.open_container_text_capture(
                    ContainerCapture::Search {
                        registry,
                        term: value.clone(),
                    },
                    value.clone(),
                    cx,
                );
            }
        }
        cx.notify();
    }

    /// Confirms a prune before it reaches the engine. `target_id` is the
    /// action's scope: image prune uses [`IMAGE_PRUNE_ALL_SCOPE`] to widen from
    /// dangling to unused images; volume/network prunes have no scope.
    pub(in crate::workspace::connection_monitor) fn request_container_prune(
        &mut self,
        kind: HostContainerConfirmKind,
        target_id: String,
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
                target_id,
                label,
                kind,
            },
            cx,
        );
    }
}

/// Whether a delivered text capture is the one the open dialog is waiting for.
fn same_container_capture(
    left: &HostContainerCaptureKind,
    right: &HostContainerCaptureKind,
) -> bool {
    match (left, right) {
        (
            HostContainerCaptureKind::Text {
                capture: left_capture,
                title: left_title,
            },
            HostContainerCaptureKind::Text {
                capture: right_capture,
                title: right_title,
            },
        ) => left_capture == right_capture && left_title == right_title,
        (HostContainerCaptureKind::Stats, HostContainerCaptureKind::Stats) => true,
        (HostContainerCaptureKind::Registries, HostContainerCaptureKind::Registries) => true,
        (HostContainerCaptureKind::Discovery, HostContainerCaptureKind::Discovery) => true,
        (HostContainerCaptureKind::Snapshot(left), HostContainerCaptureKind::Snapshot(right)) => {
            left.id() == right.id()
        }
        _ => false,
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
    fn image_sort_buttons_cycle_through_both_directions_and_off() {
        use oxideterm_connection_monitor::container::ImageSort;
        let (sort, descending) = next_image_sort(None, true, ImageSort::Size);
        assert_eq!((sort, descending), (Some(ImageSort::Size), true));
        let (sort, descending) = next_image_sort(sort, descending, ImageSort::Size);
        assert_eq!((sort, descending), (Some(ImageSort::Size), false));
        let (sort, descending) = next_image_sort(sort, descending, ImageSort::Size);
        assert_eq!((sort, descending), (None, true));
        // Switching to the other column starts at its descending order.
        let (sort, descending) = next_image_sort(None, true, ImageSort::Created);
        assert_eq!((sort, descending), (Some(ImageSort::Created), true));
        let (sort, descending) = next_image_sort(sort, descending, ImageSort::Size);
        assert_eq!((sort, descending), (Some(ImageSort::Size), true));
    }

    #[test]
    fn container_sort_buttons_cycle_ascending_descending_and_off() {
        let sort = next_container_sort(None, ContainerSortKey::Cpu);
        assert_eq!(
            sort,
            Some(ContainerSort {
                key: ContainerSortKey::Cpu,
                descending: false
            })
        );
        let sort = next_container_sort(sort, ContainerSortKey::Cpu);
        assert_eq!(
            sort,
            Some(ContainerSort {
                key: ContainerSortKey::Cpu,
                descending: true
            })
        );
        assert_eq!(next_container_sort(sort, ContainerSortKey::Cpu), None);
        // A different column restarts from ascending.
        assert_eq!(
            next_container_sort(None, ContainerSortKey::Memory),
            Some(ContainerSort {
                key: ContainerSortKey::Memory,
                descending: false
            })
        );
    }

    #[test]
    fn container_rows_sort_by_cpu_and_memory_samples() {
        use oxideterm_connection_monitor::container::{Container, ContainerStat};
        let container = |id: &str, name: &str| Container {
            id: id.to_string(),
            name: name.to_string(),
            ..Container::default()
        };
        let stat = |id: &str, cpu: f64, mem: u64| ContainerStat {
            id: id.to_string(),
            cpu_percent_value: cpu,
            mem_usage_bytes: mem,
            ..ContainerStat::default()
        };
        let stats = vec![
            // Stats abbreviate the id, inventory carries the full digest.
            stat("aaaaaaaaaaaa", 0.5, 200),
            stat("bbbbbbbbbbbb", 12.5, 900),
        ];
        let mut containers = vec![
            container("aaaaaaaaaaaaaaaaaaaa", "web"),
            container("bbbbbbbbbbbbbbbbbbbb", "api"),
            container("cccccccccccccccccccc", "no-stats"),
        ];
        sort_containers(
            &mut containers,
            ContainerSort {
                key: ContainerSortKey::Memory,
                descending: false,
            },
            &stats,
        );
        let names: Vec<&str> = containers.iter().map(|c| c.name.as_str()).collect();
        // Unknown samples stay last in both directions.
        assert_eq!(names, ["web", "api", "no-stats"]);

        sort_containers(
            &mut containers,
            ContainerSort {
                key: ContainerSortKey::Cpu,
                descending: true,
            },
            &stats,
        );
        let names: Vec<&str> = containers.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["api", "web", "no-stats"]);
    }

    #[test]
    fn engine_capabilities_gate_inventory_tabs() {
        use oxideterm_connection_monitor::container::ContainerEngine;
        assert!(HostContainerTab::Pods.supported_by(ContainerEngine::Podman));
        assert!(!HostContainerTab::Pods.supported_by(ContainerEngine::Wslc));
        assert!(HostContainerTab::Registries.supported_by(ContainerEngine::Podman));
        assert!(HostContainerTab::Registries.supported_by(ContainerEngine::Docker));
        assert!(!HostContainerTab::Registries.supported_by(ContainerEngine::Wslc));
        assert!(HostContainerTab::Images.supported_by(ContainerEngine::Wslc));
    }

    #[gpui::test]
    fn engine_switch_leaves_tabs_the_new_engine_cannot_fill(cx: &mut gpui::TestAppContext) {
        let (profiler_tx, profiler_rx) = tokio::sync::mpsc::channel(1);
        let entity = cx.new(|cx| {
            HostToolsEntity::new(
                profiler_tx,
                profiler_rx,
                oxideterm_ssh::SshConnectionRegistry::default(),
                cx,
            )
        });
        entity.update(cx, |entity, cx| {
            entity.host_container.discovery = Some(
                oxideterm_connection_monitor::container::ContainerDiscovery {
                    instances: vec![
                        oxideterm_connection_monitor::container::ContainerInstance::new(
                            oxideterm_connection_monitor::container::ContainerEngine::Podman,
                            oxideterm_connection_monitor::container::ContainerTransport::Cli,
                            None,
                        ),
                        oxideterm_connection_monitor::container::ContainerInstance::new(
                            oxideterm_connection_monitor::container::ContainerEngine::Wslc,
                            oxideterm_connection_monitor::container::ContainerTransport::Cli,
                            None,
                        ),
                    ],
                    unavailable: Vec::new(),
                    error: None,
                },
            );
            entity.host_container.discovery_connection_id = Some("local".to_string());
            entity.host_container.selected_instance_id = Some("podman:cli".to_string());

            // The registry tab disappears for the WSL container CLI.
            entity.host_container.tab = HostContainerTab::Registries;
            entity.select_container_instance("wslc:cli".to_string(), cx);
            assert_eq!(entity.host_container.tab, HostContainerTab::Containers);

            // Pods only exist on Podman as well.
            entity.select_container_instance("podman:cli".to_string(), cx);
            entity.host_container.tab = HostContainerTab::Pods;
            entity.select_container_instance("wslc:cli".to_string(), cx);
            assert_eq!(entity.host_container.tab, HostContainerTab::Containers);
        });
    }

    #[test]
    fn signed_in_registry_hosts_are_usable_as_reference_prefixes() {
        assert_eq!(registry_host("https://index.docker.io/v1/"), "docker.io");
        assert_eq!(registry_host("registry-1.docker.io"), "docker.io");
        assert_eq!(registry_host("ghcr.io"), "ghcr.io");
        assert_eq!(
            registry_host("harbor.example.com/project"),
            "harbor.example.com/project"
        );
    }

    #[test]
    fn image_rows_sharing_a_digest_keep_distinct_identities() {
        let image = |tag: &str| oxideterm_connection_monitor::container::Image {
            id: "sha256:same-config".to_string(),
            repository: "registry.example.com/app".to_string(),
            tag: tag.to_string(),
            ..oxideterm_connection_monitor::container::Image::default()
        };
        let latest = image("latest");
        let stable = image("stable");
        assert_ne!(image_row_identity(&latest), image_row_identity(&stable));
        assert_eq!(
            image_row_identity(&latest),
            image_row_identity(&image("latest"))
        );
        // Virtual-list signatures must follow the same identity, otherwise a
        // shared digest would collapse two rows into one entry.
        assert_ne!(
            container_row_signature(&ContainerRow::Image(latest), false),
            container_row_signature(&ContainerRow::Image(stable), false)
        );
    }

    #[test]
    fn image_labels_humanize_bytes_and_unix_creation_time() {
        let podman = oxideterm_connection_monitor::container::Image {
            id: "abc".to_string(),
            size: "39875426".to_string(),
            size_bytes: Some(39_875_426),
            created: "1793032819".to_string(),
            created_at: Some(1_793_032_819),
            ..oxideterm_connection_monitor::container::Image::default()
        };
        assert_eq!(image_size_label(&podman), "38.0 MB");
        let created = image_created_label(&podman);
        assert_ne!(created, podman.created);
        // Podman's unix seconds become a "YYYY-MM-DD HH:MM" local timestamp.
        assert_eq!(created.len(), 16, "unexpected timestamp shape: {created}");
        assert_eq!(&created[4..5], "-");
        assert_eq!(&created[7..8], "-");
        assert_eq!(&created[10..11], " ");
        assert_eq!(&created[13..14], ":");
        assert!(created.starts_with("2026-"), "unexpected year: {created}");

        // Docker already formats both fields; nothing is reinterpreted.
        let docker = oxideterm_connection_monitor::container::Image {
            id: "def".to_string(),
            size: "41 MB".to_string(),
            created: "2026-10-22 10:11:12 +0800".to_string(),
            ..oxideterm_connection_monitor::container::Image::default()
        };
        assert_eq!(image_size_label(&docker), "41 MB");
        assert_eq!(image_created_label(&docker), "2026-10-22 10:11:12 +0800");
    }

    #[test]
    fn list_state_follows_the_rendered_row_count() {
        let state = tauri_virtual_list_state(
            0,
            ListAlignment::Top,
            TauriVirtualListSpec::new(px(64.0), 6),
        );
        let mut cache = VirtualListSignatureCache::default();
        let pod = |id: &str| {
            ContainerRow::Pod(oxideterm_connection_monitor::container::Pod {
                id: id.to_string(),
                name: id.to_string(),
                ..oxideterm_connection_monitor::container::Pod::default()
            })
        };
        let rows = vec![pod("a"), pod("b"), pod("c")];
        sync_container_list_state(&state, &mut cache, "rows", &rows, None);
        assert_eq!(state.item_count(), 3);
        // Filtering shrinks the same list instead of leaving stale rows behind.
        sync_container_list_state(&state, &mut cache, "rows", &rows[..1], None);
        assert_eq!(state.item_count(), 1);
    }

    #[test]
    fn expanding_a_row_changes_its_list_signature() {
        let pod = ContainerRow::Pod(oxideterm_connection_monitor::container::Pod {
            id: "pod-a".to_string(),
            name: "pod-a".to_string(),
            ..oxideterm_connection_monitor::container::Pod::default()
        });
        assert_ne!(
            container_row_signature(&pod, false),
            container_row_signature(&pod, true)
        );
        assert!(container_row_is_selected(&pod, Some("pod-a")));
        assert!(!container_row_is_selected(&pod, Some("pod-b")));
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
