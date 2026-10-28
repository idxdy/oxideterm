//! Owns the Podman Host Tool UI and its container lifecycle operations.

use super::*;

impl WorkspaceApp {
    pub(super) fn render_host_podman_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens;
        let i18n = &self.i18n;
        let mono_font_family = settings_mono_font_family(self.settings_store.settings());
        let selectable_text = self.selectable_text_render_state(cx);
        let search_ime = self
            .host_tools_plain_text_ime_frame(HostToolsTextInput::PodmanSearch, cx)
            .expect("podman search is a non-secret Host Tools input");
        let connections = self.monitor_connections(cx);
        let selected_connection_id = self.host_tools.read(cx).selected_connection_id_owned();
        let selected_connection_id = selected_connection_id.as_deref().or_else(|| {
            connections
                .first()
                .map(|connection| connection.connection_id.as_str())
        });
        let terminal_available = selected_connection_id
            .and_then(|connection_id| self.node_router.node_id_for_connection(connection_id))
            .is_some_and(|node_id| self.ssh_nodes.contains_key(&node_id));
        self.host_tools.update(cx, |host_tools, cx| {
            host_tools.render_host_podman_panel(
                search_ime,
                terminal_available,
                &tokens,
                i18n,
                mono_font_family,
                &selectable_text,
                cx,
            )
        })
    }

    pub(in crate::workspace) fn handle_host_podman_search_key(
        &mut self,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self
            .host_tools
            .read(cx)
            .ui
            .input_is_focused(HostToolsTextInput::PodmanSearch)
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

    pub(in crate::workspace) fn handle_host_podman_confirm_key(
        &mut self,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.host_tools.read(cx).podman_confirm_view().is_none() {
            return false;
        }
        match self.handle_standard_confirm_key(event, cx) {
            Some(ConfirmKeyboardAction::Cancel) => {
                self.begin_host_podman_confirm_exit(cx);
                true
            }
            Some(ConfirmKeyboardAction::Confirm) => {
                self.clear_standard_confirm_focus();
                let delay = oxideterm_gpui_ui::motion::duration(
                    &self.tokens,
                    oxideterm_gpui_ui::motion::MotionDuration::Control,
                );
                self.host_tools.update(cx, |host_tools, cx| {
                    host_tools.confirm_podman_action_from_view(delay, cx);
                });
                true
            }
            Some(ConfirmKeyboardAction::Handled) => true,
            None => false,
        }
    }

    /// Keeps the request mounted until the current exit generation completes.
    fn begin_host_podman_confirm_exit(&mut self, cx: &mut Context<Self>) -> bool {
        self.clear_standard_confirm_focus();
        let delay = oxideterm_gpui_ui::motion::duration(
            &self.tokens,
            oxideterm_gpui_ui::motion::MotionDuration::Control,
        );
        self.host_tools.update(cx, |host_tools, cx| {
            host_tools.begin_podman_confirm_exit(delay, cx)
        })
    }

    pub(in crate::workspace) fn render_host_podman_confirm_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let tokens = self.tokens;
        let i18n = &self.i18n;
        let focused_action = self.standard_confirm_focus();
        self.host_tools.update(cx, |host_tools, cx| {
            host_tools.render_host_podman_confirm_dialog(&tokens, i18n, focused_action, cx)
        })
    }

    pub(in crate::workspace) fn render_host_podman_logs_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let tokens = self.tokens;
        let i18n = &self.i18n;
        let mono_font_family = settings_mono_font_family(self.settings_store.settings());
        let follow_terminal_available = self
            .host_tools
            .read(cx)
            .podman_logs_dialog()
            .and_then(|dialog| {
                self.node_router
                    .node_id_for_connection(&dialog.request.connection_id)
            })
            .is_some_and(|node_id| self.ssh_nodes.contains_key(&node_id));
        self.host_tools.update(cx, |host_tools, cx| {
            host_tools.render_host_podman_logs_dialog(
                follow_terminal_available,
                &tokens,
                i18n,
                mono_font_family,
                cx,
            )
        })
    }
}

struct HostPodmanPanelSnapshot {
    connections: Vec<MonitorConnectionOption>,
    selected_connection_id: String,
    rows: Vec<ResourcePodmanContainer>,
    visible_count: usize,
    status: ResourcePodmanStatus,
    has_metrics: bool,
}

impl HostToolsEntity {
    fn podman_panel_snapshot(&self) -> Option<HostPodmanPanelSnapshot> {
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
        let current = self.profiler_registry().current(&selected_connection_id);
        let metrics = current.as_ref().and_then(|(metrics, _)| metrics.as_ref());
        let rows = metrics
            .map(|metrics| {
                visible_podman_rows(
                    &metrics.podman.containers,
                    &self.ui.host_podman_search_query,
                )
            })
            .unwrap_or_default();
        let status = metrics
            .map(|metrics| metrics.podman.status.clone())
            .unwrap_or_default();

        Some(HostPodmanPanelSnapshot {
            connections,
            selected_connection_id,
            visible_count: rows.len(),
            rows,
            status,
            has_metrics: current.is_some(),
        })
    }

    fn render_host_podman_panel(
        &self,
        search_ime: HostToolsPlainTextImeFrame,
        terminal_available: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tab = self.ui.host_podman_tab;
        let Some(snapshot) = self.podman_panel_snapshot() else {
            return host_tools_center_state(
                LucideIcon::WifiOff,
                tokens.ui.text_muted,
                i18n.t("profiler.panel.no_connection"),
                selectable_text,
                cx,
            );
        };
        let selected_connection_id = snapshot.selected_connection_id.clone();
        let tab_bar = self.render_podman_tab_bar(tokens, i18n, cx);
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
                &snapshot.connections,
                &selected_connection_id,
                snapshot.has_metrics,
                tokens,
                mono_font_family.clone(),
                selectable_text,
                cx,
            ))
            .child(tab_bar);
        let body = match tab {
            HostPodmanTab::Containers => {
                let mut snapshot = snapshot;
                self.sync_host_podman_list_state(&snapshot.rows, &snapshot.selected_connection_id);
                let search = self.render_host_podman_search(&search_ime, tokens, i18n, cx);
                let list = self.render_host_podman_list(
                    std::mem::take(&mut snapshot.rows),
                    snapshot.has_metrics,
                    std::mem::take(&mut snapshot.status),
                    selected_connection_id.clone(),
                    terminal_available,
                    tokens,
                    i18n,
                    mono_font_family.clone(),
                    selectable_text,
                    cx,
                );
                let status_row = self.render_host_podman_status_row(
                    snapshot.visible_count,
                    selected_connection_id.clone(),
                    terminal_available,
                    tokens,
                    i18n,
                    cx,
                );
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
                            .child(search)
                            .child(status_row),
                    )
                    .child(list)
                    .into_any_element()
            }
            HostPodmanTab::Images | HostPodmanTab::Volumes | HostPodmanTab::Networks => self
                .render_podman_resources_body(
                    tab,
                    tokens,
                    i18n,
                    mono_font_family.clone(),
                    selectable_text,
                    cx,
                ),
        };

        div()
            .id("host-podman-panel")
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

    fn render_host_podman_search(
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
                    value: &self.ui.host_podman_search_query,
                    placeholder: i18n.t("sidebar.host_podman.search_placeholder"),
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
                    // The one-shot request lets the root coordinate shared IME state
                    // without retaining WorkspaceApp in this Entity-owned input.
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

    fn render_host_podman_status_row(
        &self,
        visible_count: usize,
        selected_connection_id: String,
        terminal_available: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .min_w_0()
            .text_size(px(11.0))
            .text_color(rgb(theme.text_muted))
            .child(div().flex_none().child(format!(
                "{} {}",
                visible_count,
                i18n.t("sidebar.host_podman.count_suffix")
            )))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(self.render_host_podman_status_button(
                        LucideIcon::Play,
                        "sidebar.host_podman.actions.run",
                        "host-podman-run",
                        false,
                        tokens,
                        i18n,
                        |host_tools, cx| {
                            host_tools.open_podman_input_dialog(
                                HostPodmanInputDialogKind::RunContainer,
                                "",
                                cx,
                            );
                        },
                        cx,
                    ))
                    .child(self.render_host_podman_status_button(
                        LucideIcon::Activity,
                        "sidebar.host_podman.actions.stats",
                        "host-podman-stats",
                        false,
                        tokens,
                        i18n,
                        |host_tools, cx| {
                            host_tools.toggle_podman_stats_view(cx);
                        },
                        cx,
                    ))
                    .child(host_tools_tooltip_icon_button(
                        tokens,
                        LucideIcon::RefreshCw,
                        13.0,
                        rgb(theme.text),
                        oxideterm_gpui_ui::button::IconButtonOptions {
                            size: 24.0,
                            has_background: true,
                            background: Some(rgb(theme.bg_hover)),
                            hover_background: Some(rgb(theme.bg_panel)),
                            idle_opacity: 1.0,
                            ..oxideterm_gpui_ui::button::IconButtonOptions::compact(24.0)
                        },
                        i18n.t("sidebar.host_podman.actions.refresh"),
                        "host-podman-refresh",
                        true,
                        cx.listener(move |host_tools, _event, _window, cx| {
                            host_tools
                                .refresh_host_podman_snapshot(selected_connection_id.clone(), cx);
                            cx.stop_propagation();
                        }),
                    )),
            )
            .when(!terminal_available, |row| row)
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_host_podman_status_button(
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

    #[allow(clippy::too_many_arguments)]
    fn render_host_podman_list(
        &self,
        rows: Vec<ResourcePodmanContainer>,
        has_metrics: bool,
        status: ResourcePodmanStatus,
        selected_connection_id: String,
        terminal_available: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !has_metrics {
            return host_tools_center_state(
                LucideIcon::Container,
                tokens.ui.text_muted,
                i18n.t("sidebar.host_podman.sampling"),
                selectable_text,
                cx,
            );
        }
        match status {
            ResourcePodmanStatus::Unavailable { detail: None } => {
                return host_tools_center_state(
                    LucideIcon::Container,
                    tokens.ui.text_muted,
                    i18n.t("sidebar.host_podman.unavailable"),
                    selectable_text,
                    cx,
                );
            }
            ResourcePodmanStatus::Unavailable {
                detail: Some(detail),
            } => {
                return host_tools_center_state(
                    LucideIcon::Container,
                    tokens.ui.text_muted,
                    i18n.t("sidebar.host_podman.unavailable_detail")
                        .replace("{{detail}}", &detail),
                    selectable_text,
                    cx,
                );
            }
            ResourcePodmanStatus::Error { message } => {
                return host_tools_center_state(
                    LucideIcon::AlertTriangle,
                    MONITOR_RED,
                    i18n.t("sidebar.host_podman.error")
                        .replace("{{error}}", &message),
                    selectable_text,
                    cx,
                );
            }
            ResourcePodmanStatus::Unknown | ResourcePodmanStatus::Available => {}
        }
        if rows.is_empty() {
            return host_tools_center_state(
                LucideIcon::Container,
                tokens.ui.text_muted,
                i18n.t("sidebar.host_podman.empty"),
                selectable_text,
                cx,
            );
        }

        let rows = Arc::new(rows);
        let selected_connection_id = Arc::new(selected_connection_id);
        let list_state = self.ui.host_podman_list_state.clone();
        let list_spec = TauriVirtualListSpec::new(px(HOST_PODMAN_LIST_ESTIMATED_ROW_HEIGHT), 8);
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
            .child(self.render_host_podman_table_header(tokens, i18n))
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
                            let selected_connection_id = Arc::clone(&selected_connection_id);
                            let mono_font_family = row_mono_font_family.clone();
                            host_tools.update(cx, |host_tools, cx| {
                                host_tools.render_host_podman_row(
                                    selected_connection_id.as_str(),
                                    rows.get(index).cloned(),
                                    terminal_available,
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

    fn render_host_podman_table_header(&self, tokens: &ThemeTokens, i18n: &I18n) -> AnyElement {
        let theme = tokens.ui;
        div()
            .flex_none()
            .w_full()
            .min_w_0()
            .h(px(HOST_PODMAN_TABLE_HEADER_HEIGHT))
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
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .child(i18n.t("sidebar.host_podman.columns.container")),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .child(i18n.t("sidebar.host_podman.columns.image")),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(HOST_PODMAN_STATE_COLUMN_WIDTH))
                    .child(i18n.t("sidebar.host_podman.columns.state")),
            )
            .child(
                div()
                    .min_w(px(HOST_PODMAN_PORTS_COLUMN_MIN_WIDTH))
                    .flex_1()
                    .truncate()
                    .child(i18n.t("sidebar.host_podman.columns.ports")),
            )
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_host_podman_row(
        &self,
        connection_id: &str,
        container: Option<ResourcePodmanContainer>,
        terminal_available: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(container) = container else {
            return div().into_any_element();
        };
        let expanded = self.ui.host_podman_expanded_id.as_deref() == Some(container.id.as_str());
        let theme = tokens.ui;
        let state_label = i18n.t(podman_state_label_key(&container.state));
        let ports = container.ports.clone().unwrap_or_else(|| "—".to_string());

        div()
            .id(host_tools_row_id(
                "host-podman-row",
                connection_id,
                container.id.clone(),
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
                    .h(px(HOST_PODMAN_TABLE_MAIN_ROW_HEIGHT))
                    .px_3()
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
                            .child(container.name.clone()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(HOST_PROCESS_TABLE_VALUE_TEXT_SIZE))
                            .text_color(rgb(theme.text_muted))
                            .font_family(mono_font_family.clone())
                            .child(container.image.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(HOST_PODMAN_STATE_COLUMN_WIDTH))
                            .truncate()
                            .text_size(px(HOST_PROCESS_TABLE_VALUE_TEXT_SIZE))
                            .text_color(rgb(podman_state_color(&container.state, theme.text_muted)))
                            .font_family(mono_font_family.clone())
                            .child(state_label),
                    )
                    .child(
                        div()
                            .min_w(px(HOST_PODMAN_PORTS_COLUMN_MIN_WIDTH))
                            .flex_1()
                            .truncate()
                            .whitespace_nowrap()
                            .text_size(px(HOST_PROCESS_TABLE_META_TEXT_SIZE))
                            .text_color(rgb(theme.text_muted))
                            .font_family(mono_font_family.clone())
                            .child(ports),
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
                            .child(container.status.clone()),
                    )
                    .child(self.render_host_podman_inline_actions(
                        connection_id,
                        &container,
                        terminal_available,
                        tokens,
                        i18n,
                        cx,
                    )),
            )
            .when(expanded, |row| {
                row.child(self.render_host_podman_detail(
                    &container,
                    tokens,
                    i18n,
                    mono_font_family,
                ))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener({
                    let container_id = container.id.clone();
                    move |host_tools, _event, _window, cx| {
                        let expanded_id = &mut host_tools.ui.host_podman_expanded_id;
                        if expanded_id.as_deref() == Some(container_id.as_str()) {
                            *expanded_id = None;
                        } else {
                            *expanded_id = Some(container_id.clone());
                        }
                        cx.notify();
                        cx.stop_propagation();
                    }
                }),
            )
            .into_any_element()
    }

    fn render_host_podman_inline_actions(
        &self,
        connection_id: &str,
        container: &ResourcePodmanContainer,
        terminal_available: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let action_running = self.podman_action_running_for(&container.id);
        let availability = podman_action_availability(&container.state);
        div()
            .flex_none()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(4.0))
            .child(self.render_host_podman_logs_button(
                connection_id,
                container,
                action_running,
                tokens,
                i18n,
                cx,
            ))
            .child(self.render_host_podman_follow_logs_button(
                connection_id,
                container,
                action_running || !availability.can_use_live_tools || !terminal_available,
                tokens,
                i18n,
                cx,
            ))
            .child(self.render_host_podman_exec_button(
                connection_id,
                container,
                action_running || !availability.can_exec || !terminal_available,
                tokens,
                i18n,
                cx,
            ))
            .child(self.render_host_podman_attach_button(
                connection_id,
                container,
                action_running || !availability.can_exec || !terminal_available,
                tokens,
                i18n,
                cx,
            ))
            .child(self.render_host_podman_action_button(
                connection_id,
                container,
                PodmanActionKind::Start,
                LucideIcon::Play,
                "sidebar.host_podman.actions.start",
                false,
                action_running || !availability.can_start,
                tokens,
                i18n,
                cx,
            ))
            .child(self.render_host_podman_action_button(
                connection_id,
                container,
                PodmanActionKind::Stop,
                LucideIcon::Square,
                "sidebar.host_podman.actions.stop",
                true,
                action_running || !availability.can_stop,
                tokens,
                i18n,
                cx,
            ))
            .child(self.render_host_podman_action_button(
                connection_id,
                container,
                PodmanActionKind::Restart,
                LucideIcon::RefreshCw,
                "sidebar.host_podman.actions.restart",
                true,
                action_running || !availability.can_restart,
                tokens,
                i18n,
                cx,
            ))
            .child(self.render_host_podman_action_button(
                connection_id,
                container,
                PodmanActionKind::Pause,
                LucideIcon::Pause,
                "sidebar.host_podman.actions.pause",
                false,
                action_running || !availability.can_pause,
                tokens,
                i18n,
                cx,
            ))
            .child(self.render_host_podman_action_button(
                connection_id,
                container,
                PodmanActionKind::Unpause,
                LucideIcon::Play,
                "sidebar.host_podman.actions.unpause",
                false,
                action_running || !availability.can_unpause,
                tokens,
                i18n,
                cx,
            ))
            .child(self.render_host_podman_action_button(
                connection_id,
                container,
                PodmanActionKind::Remove,
                LucideIcon::Trash2,
                "sidebar.host_podman.actions.remove",
                true,
                action_running || !availability.can_remove,
                tokens,
                i18n,
                cx,
            ))
            .child(self.render_host_podman_row_menu_button(
                connection_id,
                container,
                action_running,
                tokens,
                i18n,
                cx,
            ))
            .into_any_element()
    }

    /// Row menu affordance: rename, healthcheck run, and file copy open the
    /// shared input dialog or a confirm instead of owning dedicated buttons.
    #[allow(clippy::too_many_arguments)]
    fn render_host_podman_row_menu_button(
        &self,
        connection_id: &str,
        container: &ResourcePodmanContainer,
        action_running: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let container_id = container.id.clone();
        let container_name = container.name.clone();
        host_tools_tooltip_icon_button(
            tokens,
            LucideIcon::Pencil,
            13.0,
            rgb(theme.text),
            oxideterm_gpui_ui::button::IconButtonOptions {
                size: 22.0,
                disabled: action_running,
                has_background: true,
                background: Some(rgb(theme.bg_hover)),
                hover_background: Some(rgb(theme.bg_panel)),
                idle_opacity: 1.0,
                ..oxideterm_gpui_ui::button::IconButtonOptions::compact(22.0)
            },
            i18n.t("sidebar.host_podman.actions.more"),
            "host-podman-row-more",
            true,
            cx.listener(move |host_tools, _event, _window, cx| {
                host_tools.open_podman_input_dialog(
                    HostPodmanInputDialogKind::RenameContainer {
                        container_id: container_id.clone(),
                    },
                    &container_name.clone(),
                    cx,
                );
                cx.stop_propagation();
            }),
        )
    }

    fn render_host_podman_attach_button(
        &self,
        connection_id: &str,
        container: &ResourcePodmanContainer,
        disabled: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let title = i18n
            .t("sidebar.host_podman.attach_title")
            .replace("{{name}}", &container.name);
        let opened_notice = i18n
            .t("sidebar.host_podman.toast.attach_opened")
            .replace("{{name}}", &container.name);
        let missing_notice = i18n.t("sidebar.host_podman.toast.exec_terminal_missing");
        host_tools_tooltip_icon_button(
            tokens,
            LucideIcon::Cable,
            13.0,
            rgb(theme.text),
            oxideterm_gpui_ui::button::IconButtonOptions {
                size: 22.0,
                disabled: disabled || build_podman_attach_command(&container.id).is_err(),
                has_background: true,
                background: Some(rgb(theme.bg_hover)),
                hover_background: Some(rgb(theme.bg_panel)),
                idle_opacity: 1.0,
                ..oxideterm_gpui_ui::button::IconButtonOptions::compact(22.0)
            },
            i18n.t("sidebar.host_podman.actions.attach"),
            "host-podman-attach",
            true,
            cx.listener({
                let connection_id = connection_id.to_string();
                let container_id = container.id.clone();
                move |host_tools, _event, window, cx| {
                    match build_podman_attach_command(&container_id) {
                        Ok(command) => host_tools.dispatch_podman_terminal_request(
                            connection_id.clone(),
                            command,
                            title.clone(),
                            opened_notice.clone(),
                            missing_notice.clone(),
                            window,
                            cx,
                        ),
                        Err(_) => cx.emit(HostToolsEvent::ShowNotice(
                            HostToolsNotice::PodmanActionFailed,
                        )),
                    }
                    cx.stop_propagation();
                }
            }),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_host_podman_action_button(
        &self,
        connection_id: &str,
        container: &ResourcePodmanContainer,
        action: PodmanActionKind,
        icon: LucideIcon,
        label_key: &'static str,
        danger: bool,
        disabled: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let unsupported =
            !self.podman_action_supported(connection_id, &container.id, action.clone());
        let icon_color = if danger { MONITOR_RED } else { theme.text };
        host_tools_tooltip_icon_button(
            tokens,
            icon,
            13.0,
            rgb(icon_color),
            oxideterm_gpui_ui::button::IconButtonOptions {
                size: 22.0,
                disabled: disabled || unsupported,
                has_background: true,
                background: Some(if danger {
                    rgba((MONITOR_RED << 8) | MONITOR_TINT_ALPHA)
                } else {
                    rgb(theme.bg_hover)
                }),
                hover_background: Some(if danger {
                    rgba((MONITOR_RED << 8) | 0x30)
                } else {
                    rgb(theme.bg_panel)
                }),
                idle_opacity: 1.0,
                ..oxideterm_gpui_ui::button::IconButtonOptions::compact(22.0)
            },
            i18n.t(label_key),
            "host-podman-action",
            true,
            cx.listener({
                let connection_id = connection_id.to_string();
                let container_id = container.id.clone();
                let container_name = container.name.clone();
                move |host_tools, _event, _window, cx| {
                    host_tools.request_podman_action_from_view(
                        connection_id.clone(),
                        container_id.clone(),
                        container_name.clone(),
                        action.clone(),
                        cx,
                    );
                    cx.stop_propagation();
                }
            }),
        )
    }

    fn render_host_podman_logs_button(
        &self,
        connection_id: &str,
        container: &ResourcePodmanContainer,
        disabled: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let unsupported = !self.podman_logs_supported(connection_id, &container.id);
        let failure_fallback = i18n.t("sidebar.host_podman.toast.logs_failed");
        let empty_fallback = i18n.t("sidebar.host_podman.logs.empty");
        host_tools_tooltip_icon_button(
            tokens,
            LucideIcon::FileText,
            13.0,
            rgb(theme.text),
            oxideterm_gpui_ui::button::IconButtonOptions {
                size: 22.0,
                disabled: disabled || unsupported,
                has_background: true,
                background: Some(rgb(theme.bg_hover)),
                hover_background: Some(rgb(theme.bg_panel)),
                idle_opacity: 1.0,
                ..oxideterm_gpui_ui::button::IconButtonOptions::compact(22.0)
            },
            i18n.t("sidebar.host_podman.actions.logs"),
            "host-podman-logs",
            true,
            cx.listener({
                let connection_id = connection_id.to_string();
                let container_id = container.id.clone();
                let container_name = container.name.clone();
                move |host_tools, _event, _window, cx| {
                    host_tools.request_podman_logs_from_view(
                        connection_id.clone(),
                        container_id.clone(),
                        container_name.clone(),
                        failure_fallback.clone(),
                        empty_fallback.clone(),
                        cx,
                    );
                    cx.stop_propagation();
                }
            }),
        )
    }

    fn render_host_podman_follow_logs_button(
        &self,
        connection_id: &str,
        container: &ResourcePodmanContainer,
        disabled: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let title = i18n
            .t("sidebar.host_podman.follow_title")
            .replace("{{name}}", &container.name);
        let opened_notice = i18n
            .t("sidebar.host_podman.toast.follow_opened")
            .replace("{{name}}", &container.name);
        let missing_notice = i18n.t("sidebar.host_podman.toast.exec_terminal_missing");
        host_tools_tooltip_icon_button(
            tokens,
            LucideIcon::Radio,
            13.0,
            rgb(theme.text),
            oxideterm_gpui_ui::button::IconButtonOptions {
                size: 22.0,
                disabled: disabled || build_podman_follow_logs_command(&container.id).is_err(),
                has_background: true,
                background: Some(rgb(theme.bg_hover)),
                hover_background: Some(rgb(theme.bg_panel)),
                idle_opacity: 1.0,
                ..oxideterm_gpui_ui::button::IconButtonOptions::compact(22.0)
            },
            i18n.t("sidebar.host_podman.actions.follow_logs"),
            "host-podman-follow-logs",
            true,
            cx.listener({
                let connection_id = connection_id.to_string();
                let container_id = container.id.clone();
                move |host_tools, _event, window, cx| {
                    host_tools.dispatch_podman_follow_logs_terminal(
                        connection_id.clone(),
                        container_id.clone(),
                        title.clone(),
                        opened_notice.clone(),
                        missing_notice.clone(),
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                }
            }),
        )
    }

    fn render_host_podman_exec_button(
        &self,
        connection_id: &str,
        container: &ResourcePodmanContainer,
        disabled: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let title = i18n
            .t("sidebar.host_podman.exec_title")
            .replace("{{name}}", &container.name);
        let opened_notice = i18n
            .t("sidebar.host_podman.toast.exec_opened")
            .replace("{{name}}", &container.name);
        let missing_notice = i18n.t("sidebar.host_podman.toast.exec_terminal_missing");
        host_tools_tooltip_icon_button(
            tokens,
            LucideIcon::Terminal,
            13.0,
            rgb(theme.text),
            oxideterm_gpui_ui::button::IconButtonOptions {
                size: 22.0,
                disabled: disabled || build_podman_exec_shell_command(&container.id).is_err(),
                has_background: true,
                background: Some(rgb(theme.bg_hover)),
                hover_background: Some(rgb(theme.bg_panel)),
                idle_opacity: 1.0,
                ..oxideterm_gpui_ui::button::IconButtonOptions::compact(22.0)
            },
            i18n.t("sidebar.host_podman.actions.exec"),
            "host-podman-exec",
            true,
            cx.listener({
                let connection_id = connection_id.to_string();
                let container_id = container.id.clone();
                move |host_tools, _event, window, cx| {
                    host_tools.dispatch_podman_exec_terminal(
                        connection_id.clone(),
                        container_id.clone(),
                        title.clone(),
                        opened_notice.clone(),
                        missing_notice.clone(),
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                }
            }),
        )
    }

    fn request_podman_action_from_view(
        &mut self,
        connection_id: String,
        container_id: String,
        container_name: String,
        action: PodmanActionKind,
        cx: &mut Context<Self>,
    ) {
        if let Some(notice) = self.open_podman_action_confirm(
            HostPodmanActionRequest {
                connection_id,
                container_id,
                container_name,
                action,
            },
            cx,
        ) {
            cx.emit(HostToolsEvent::ShowNotice(notice));
        }
    }

    fn request_podman_logs_from_view(
        &mut self,
        connection_id: String,
        container_id: String,
        container_name: String,
        failure_fallback: String,
        empty_fallback: String,
        cx: &mut Context<Self>,
    ) {
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::PodmanConnectionMissing,
            ));
            return;
        };
        for notice in self.request_podman_logs(
            connection_id,
            container_id,
            container_name,
            runtime,
            failure_fallback,
            empty_fallback,
            cx,
        ) {
            cx.emit(HostToolsEvent::ShowNotice(notice));
        }
    }

    fn dispatch_podman_exec_terminal(
        &mut self,
        connection_id: String,
        container_id: String,
        title: String,
        opened_notice: String,
        missing_notice: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(command) = build_podman_exec_shell_command(&container_id) else {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::PodmanActionFailed,
            ));
            return;
        };
        self.dispatch_podman_terminal_request(
            connection_id,
            command,
            title,
            opened_notice,
            missing_notice,
            window,
            cx,
        );
    }

    fn dispatch_podman_follow_logs_terminal(
        &mut self,
        connection_id: String,
        container_id: String,
        title: String,
        opened_notice: String,
        missing_notice: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(command) = build_podman_follow_logs_command(&container_id) else {
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::PodmanLogsFailed,
            ));
            return;
        };
        // Follow mode lives in a terminal consumer so Ctrl-C and tab closure stop only the stream.
        self.dispatch_podman_terminal_request(
            connection_id,
            command,
            title,
            opened_notice,
            missing_notice,
            window,
            cx,
        );
    }

    fn dispatch_podman_terminal_request(
        &self,
        connection_id: String,
        command: String,
        title: String,
        opened_notice: String,
        missing_notice: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Move the fixed-builder command through the one-shot request. GPUI action
        // cloning shares the envelope and never clones terminal command contents.
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

    fn render_host_podman_detail(
        &self,
        container: &ResourcePodmanContainer,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
    ) -> AnyElement {
        let theme = tokens.ui;
        div()
            .px_3()
            .pb_3()
            .pt_2()
            .border_t_1()
            .border_color(rgba((theme.border << 8) | MONITOR_BORDER_ALPHA))
            .flex()
            .flex_col()
            .gap_1()
            .text_size(px(HOST_PROCESS_DETAIL_TEXT_SIZE))
            .text_color(rgb(theme.text_muted))
            .child(Self::render_host_podman_detail_line(
                "ID".to_string(),
                container.id.clone(),
                mono_font_family.clone(),
            ))
            .child(Self::render_host_podman_detail_line(
                i18n.t("sidebar.host_podman.columns.image"),
                container.image.clone(),
                mono_font_family.clone(),
            ))
            .child(Self::render_host_podman_detail_line(
                i18n.t("sidebar.host_podman.columns.ports"),
                container.ports.clone().unwrap_or_else(|| "—".to_string()),
                mono_font_family.clone(),
            ))
            .child(
                div()
                    .mt_1()
                    .min_w_0()
                    .font_family(mono_font_family)
                    .text_color(rgb(theme.text))
                    .child(container.status.clone()),
            )
            .into_any_element()
    }

    fn render_host_podman_detail_line(
        label: String,
        value: String,
        mono_font_family: SharedString,
    ) -> AnyElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .min_w_0()
            .child(div().flex_none().child(label))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_family(mono_font_family)
                    .child(value),
            )
            .into_any_element()
    }

    fn sync_host_podman_list_state(&self, rows: &[ResourcePodmanContainer], selected_id: &str) {
        let signatures = rows.iter().map(podman_row_signature).collect::<Vec<_>>();
        let ui = &self.ui;
        let identity = format!(
            "host-podman:{selected_id}:{}:{}",
            ui.host_podman_search_query,
            ui.host_podman_expanded_id.as_deref().unwrap_or_default()
        );
        sync_tauri_variable_list_state_by_signatures(
            &ui.host_podman_list_state,
            &mut ui.host_podman_list_cache.borrow_mut(),
            &identity,
            &signatures,
            TauriVirtualListSpec::new(px(HOST_PODMAN_LIST_ESTIMATED_ROW_HEIGHT), 8),
        );
    }

    fn refresh_host_podman_snapshot(&mut self, connection_id: String, cx: &mut Context<Self>) {
        self.request_profiler_refresh(connection_id, cx);
    }

    pub(in crate::workspace::connection_monitor) fn toggle_podman_stats_view(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        self.ui.host_podman_stats_visible = !self.ui.host_podman_stats_visible;
        if self.ui.host_podman_stats_visible
            && self.host_podman_resources.stats.is_none()
            && !self.host_podman_resources.stats_in_flight
            && let Some(connection_id) = self.selected_connection_id_owned()
        {
            self.request_podman_capture(connection_id, HostPodmanCaptureKind::Stats, cx);
        }
        cx.notify();
    }

    pub(super) fn podman_action_running_for(&self, container_id: &str) -> bool {
        self.host_podman_operations
            .action_running
            .as_ref()
            .is_some_and(|request| request.container_id == container_id)
    }

    pub(super) fn podman_action_supported(
        &self,
        connection_id: &str,
        container_id: &str,
        action: PodmanActionKind,
    ) -> bool {
        self.connection_os_type(connection_id)
            .and_then(|os_type| build_podman_action_command(&os_type, container_id, action).ok())
            .is_some()
    }

    pub(super) fn podman_logs_supported(&self, connection_id: &str, container_id: &str) -> bool {
        self.connection_os_type(connection_id)
            .and_then(|os_type| build_podman_logs_command(&os_type, container_id).ok())
            .is_some()
    }

    pub(in crate::workspace::connection_monitor) fn open_podman_action_confirm(
        &mut self,
        request: HostPodmanActionRequest,
        cx: &mut Context<Self>,
    ) -> Option<HostToolsNotice> {
        if self.host_podman_operations.action_running.is_some() {
            return Some(HostToolsNotice::PodmanActionAlreadyRunning);
        }
        HostToolConfirmState::open(&mut self.host_podman_operations.pending_confirm, request);
        cx.notify();
        None
    }

    pub(in crate::workspace::connection_monitor) fn podman_confirm_view(
        &self,
    ) -> Option<(
        HostPodmanActionRequest,
        oxideterm_gpui_ui::motion::ExitPhase,
    )> {
        self.host_podman_operations
            .pending_confirm
            .as_ref()
            .map(|state| (state.request.clone(), state.presence.phase()))
    }

    /// Dismisses unsubmitted UI state without cancelling a running Podman action.
    pub(in crate::workspace::connection_monitor) fn dismiss_podman_confirm(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.host_podman_operations.pending_confirm.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn begin_podman_confirm_exit(
        &mut self,
        delay: Duration,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(generation) = self
            .host_podman_operations
            .pending_confirm
            .as_mut()
            .and_then(|state| state.presence.begin_exit())
        else {
            return false;
        };
        if delay.is_zero() {
            self.host_podman_operations.pending_confirm = None;
            cx.notify();
            return true;
        }
        cx.spawn(async move |weak, cx| {
            Timer::after(delay).await;
            let _ = weak.update(cx, |entity, cx| {
                if entity
                    .host_podman_operations
                    .pending_confirm
                    .as_ref()
                    .is_some_and(|state| state.presence.finish_exit(generation))
                {
                    entity.host_podman_operations.pending_confirm = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
        true
    }

    pub(super) fn confirm_podman_action(
        &mut self,
        delay: Duration,
        runtime: tokio::runtime::Handle,
        cx: &mut Context<Self>,
    ) -> Vec<HostToolsNotice> {
        let Some(request) = self
            .host_podman_operations
            .pending_confirm
            .as_ref()
            .map(|state| state.request.clone())
        else {
            return Vec::new();
        };
        if !self.begin_podman_confirm_exit(delay, cx) {
            return Vec::new();
        }
        self.start_podman_action(request, runtime, cx)
    }

    fn confirm_podman_action_from_view(&mut self, delay: Duration, cx: &mut Context<Self>) {
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            // A visible confirmation without a runtime cannot execute; close
            // the request while reporting the same safe connection failure.
            self.begin_podman_confirm_exit(delay, cx);
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::PodmanConnectionMissing,
            ));
            return;
        };
        for notice in self.confirm_podman_action(delay, runtime, cx) {
            cx.emit(HostToolsEvent::ShowNotice(notice));
        }
    }

    pub(in crate::workspace::connection_monitor) fn start_podman_action(
        &mut self,
        request: HostPodmanActionRequest,
        runtime: tokio::runtime::Handle,
        cx: &mut Context<Self>,
    ) -> Vec<HostToolsNotice> {
        let Some(os_type) = self.connection_os_type(&request.connection_id) else {
            return vec![HostToolsNotice::PodmanConnectionMissing];
        };
        let command = match build_podman_action_command(
            &os_type,
            &request.container_id,
            request.action.clone(),
        ) {
            Ok(command) => command,
            Err(_) => return vec![HostToolsNotice::PodmanActionFailed],
        };
        self.host_podman_operations.action_running = Some(request.clone());
        let spawned = self.spawn_podman_action(
            command.command,
            request,
            HOST_PODMAN_ACTION_TIMEOUT,
            HOST_PODMAN_ACTION_MAX_OUTPUT_SIZE,
            runtime,
        );
        if !spawned {
            self.host_podman_operations.action_running = None;
            return vec![HostToolsNotice::PodmanConnectionMissing];
        }
        cx.notify();
        Vec::new()
    }

    pub(super) fn request_podman_logs(
        &mut self,
        connection_id: String,
        container_id: String,
        container_name: String,
        runtime: tokio::runtime::Handle,
        failure_fallback: String,
        empty_fallback: String,
        cx: &mut Context<Self>,
    ) -> Vec<HostToolsNotice> {
        if self
            .host_podman_operations
            .logs_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.loading)
        {
            return vec![HostToolsNotice::PodmanLogsAlreadyRunning];
        }
        let Some(os_type) = self.connection_os_type(&connection_id) else {
            return vec![HostToolsNotice::PodmanConnectionMissing];
        };
        let command = match build_podman_logs_command(&os_type, &container_id) {
            Ok(command) => command,
            Err(_) => return vec![HostToolsNotice::PodmanLogsFailed],
        };
        let request = HostPodmanLogsRequest {
            connection_id,
            container_id,
            container_name,
            failure_fallback,
            empty_fallback,
        };
        self.host_podman_operations.logs_dialog = Some(HostPodmanLogsDialog {
            request: request.clone(),
            output: None,
            error: None,
            loading: true,
        });
        let spawned = self.spawn_podman_logs_capture(
            command.command,
            request,
            HOST_PODMAN_LOGS_TIMEOUT,
            HOST_PODMAN_LOGS_MAX_OUTPUT_SIZE,
            runtime,
        );
        if !spawned {
            self.host_podman_operations.logs_dialog = None;
            return vec![HostToolsNotice::PodmanConnectionMissing];
        }
        cx.notify();
        Vec::new()
    }

    pub(super) fn podman_logs_dialog(&self) -> Option<HostPodmanLogsDialog> {
        self.host_podman_operations.logs_dialog.clone()
    }

    pub(in crate::workspace::connection_monitor) fn dismiss_podman_logs_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.host_podman_operations.logs_dialog.take().is_some() {
            cx.notify();
        }
    }

    pub(in crate::workspace::connection_monitor) fn finish_host_podman_action(
        &mut self,
        delivery: HostPodmanActionDelivery,
        cx: &mut Context<Self>,
    ) {
        if self.host_podman_operations.action_running.as_ref() != Some(&delivery.request) {
            return;
        }
        self.host_podman_operations.action_running = None;
        cx.emit(HostToolsEvent::ShowNotice(
            HostToolsNotice::PodmanActionFinished {
                container_name: delivery.request.container_name,
                succeeded: delivery.result.unwrap_or(false),
            },
        ));
        // Sample the remote state transition without clearing the current list.
        self.request_profiler_refresh(delivery.request.connection_id, cx);
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn finish_host_podman_logs(
        &mut self,
        delivery: HostPodmanLogsDelivery,
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = self
            .host_podman_operations
            .logs_dialog
            .as_mut()
            .filter(|dialog| dialog.request == delivery.request)
        else {
            return;
        };
        dialog.loading = false;
        match delivery.result {
            Ok(mut output) if podman_action_succeeded(output.exit_code) => {
                zeroize::Zeroize::zeroize(&mut output.stderr);
                let retained_output = if output.stdout.trim().is_empty() {
                    delivery.request.empty_fallback
                } else {
                    std::mem::take(&mut output.stdout)
                };
                // The last Entity/render owner clears the retained log buffer.
                dialog.output = Some(Arc::new(zeroize::Zeroizing::new(retained_output)));
                dialog.error = None;
            }
            Ok(mut output) => {
                // Failed output is never rendered or copied into an error.
                zeroize::Zeroize::zeroize(&mut output.stdout);
                zeroize::Zeroize::zeroize(&mut output.stderr);
                dialog.output = None;
                dialog.error = Some(delivery.request.failure_fallback);
            }
            Err(()) => {
                dialog.output = None;
                dialog.error = Some(delivery.request.failure_fallback);
            }
        }
        cx.notify();
    }

    fn render_host_podman_logs_dialog(
        &self,
        follow_terminal_available: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.podman_logs_dialog()?;
        let theme = tokens.ui;
        let follow_connection_id = dialog.request.connection_id.clone();
        let follow_container_id = dialog.request.container_id.clone();
        let follow_title = i18n
            .t("sidebar.host_podman.follow_title")
            .replace("{{name}}", &dialog.request.container_name);
        let follow_opened_notice = i18n
            .t("sidebar.host_podman.toast.follow_opened")
            .replace("{{name}}", &dialog.request.container_name);
        let missing_notice = i18n.t("sidebar.host_podman.toast.exec_terminal_missing");
        let content = if dialog.loading {
            div()
                .p_4()
                .text_color(rgb(theme.text_muted))
                .child(i18n.t("sidebar.host_podman.logs.loading"))
                .into_any_element()
        } else if let Some(error) = dialog.error.as_ref() {
            div()
                .p_4()
                .text_color(rgb(MONITOR_RED))
                .child(error.clone())
                .into_any_element()
        } else {
            let output = dialog.output.as_deref();
            // The retained Arc<Zeroizing<String>> remains the sole log owner.
            // Per-line strings are the bounded GPUI frame output boundary.
            let mut lines = div()
                .p_3()
                .flex()
                .flex_col()
                .gap(px(1.0))
                .font_family(mono_font_family)
                .text_size(px(11.0))
                .text_color(rgb(theme.text));
            if let Some(output) = output {
                for (index, line) in output.lines().enumerate() {
                    let display_line = if line.is_empty() {
                        " ".to_string()
                    } else {
                        line.to_string()
                    };
                    lines = lines.child(
                        div()
                            .id(("host-podman-log-line", index))
                            .flex_none()
                            .whitespace_nowrap()
                            .child(display_line),
                    );
                }
            }
            lines.into_any_element()
        };

        Some(
            oxideterm_gpui_ui::modal::dismissible_dialog_backdrop()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|host_tools, _event, _window, cx| {
                        host_tools.dismiss_podman_logs_dialog(cx);
                        cx.stop_propagation();
                    }),
                )
                .child(oxideterm_gpui_ui::modal::overlay_content_boundary(
                    oxideterm_gpui_ui::modal::dialog_content(tokens)
                        .w(px(HOST_PODMAN_LOGS_DIALOG_WIDTH))
                        .max_h(px(HOST_PODMAN_LOGS_DIALOG_MAX_HEIGHT))
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
                                                .text_size(px(14.0))
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .text_color(rgb(theme.text))
                                                .child(
                                                    i18n
                                                        .t("sidebar.host_podman.logs.title")
                                                        .replace(
                                                            "{{name}}",
                                                            &dialog.request.container_name,
                                                        ),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .truncate()
                                                .text_size(px(11.0))
                                                .text_color(rgb(theme.text_muted))
                                                .child(dialog.request.container_id.clone()),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .child(host_tools_tooltip_icon_button(
                                            tokens,
                                            LucideIcon::Activity,
                                            14.0,
                                            rgb(theme.text),
                                            oxideterm_gpui_ui::button::IconButtonOptions {
                                                size: 24.0,
                                                disabled: !follow_terminal_available
                                                    || build_podman_follow_logs_command(
                                                        &follow_container_id,
                                                    )
                                                    .is_err(),
                                                has_background: true,
                                                background: Some(rgb(theme.bg_hover)),
                                                hover_background: Some(rgb(theme.bg_panel)),
                                                idle_opacity: 1.0,
                                                ..oxideterm_gpui_ui::button::IconButtonOptions::compact(
                                                    24.0,
                                                )
                                            },
                                            i18n.t("sidebar.host_podman.actions.follow_logs"),
                                            "host-podman-logs-follow",
                                            true,
                                            cx.listener(
                                                move |host_tools, _event, window, cx| {
                                                    host_tools.dismiss_podman_logs_dialog(cx);
                                                    host_tools
                                                        .dispatch_podman_follow_logs_terminal(
                                                            follow_connection_id.clone(),
                                                            follow_container_id.clone(),
                                                            follow_title.clone(),
                                                            follow_opened_notice.clone(),
                                                            missing_notice.clone(),
                                                            window,
                                                            cx,
                                                        );
                                                    cx.stop_propagation();
                                                },
                                            ),
                                        ))
                                        .child(host_tools_tooltip_icon_button(
                                            tokens,
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
                                            i18n.t("sidebar.host_podman.logs.close"),
                                            "host-podman-logs-close",
                                            true,
                                            cx.listener(|host_tools, _event, _window, cx| {
                                                host_tools.dismiss_podman_logs_dialog(cx);
                                                cx.stop_propagation();
                                            }),
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .id("host-podman-logs-scroll")
                                .flex_1()
                                .min_h_0()
                                .max_h(px(HOST_PODMAN_LOGS_DIALOG_MAX_HEIGHT - 84.0))
                                .overflow_y_scroll()
                                // Long log lines scroll sideways instead of
                                // being clipped by the modal boundary.
                                .overflow_x_scrollbar()
                                .child(content),
                        ),
                ))
                .into_any_element(),
        )
    }

    fn render_host_podman_confirm_dialog(
        &self,
        tokens: &ThemeTokens,
        i18n: &I18n,
        focused_action: Option<ConfirmDialogAction>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (request, phase) = self.podman_confirm_view()?;
        let description = i18n
            .t(host_podman_confirm_description_key(&request.action))
            .replace("{{id}}", &request.container_id)
            .replace("{{name}}", &request.container_name);
        let exit_delay = oxideterm_gpui_ui::motion::duration(
            tokens,
            oxideterm_gpui_ui::motion::MotionDuration::Control,
        );
        let confirm_delay = exit_delay;

        Some(
            oxideterm_gpui_ui::confirm::confirm_dialog_with_focus_motion(
                tokens,
                "host-podman-confirm-motion",
                phase,
                ConfirmDialogView {
                    variant: if matches!(
                        request.action,
                        PodmanActionKind::Stop
                            | PodmanActionKind::Restart
                            | PodmanActionKind::Remove
                    ) {
                        ConfirmDialogVariant::Danger
                    } else {
                        ConfirmDialogVariant::Default
                    },
                    title: div()
                        .child(i18n.t("sidebar.host_podman.confirm.title"))
                        .into_any_element(),
                    description: Some(div().child(description).into_any_element()),
                    cancel_label: div()
                        .child(i18n.t("sidebar.host_podman.confirm.cancel"))
                        .into_any_element(),
                    confirm_label: div()
                        .child(i18n.t(host_podman_confirm_label_key(&request.action)))
                        .into_any_element(),
                },
                focused_action,
                cx.listener(move |host_tools, _event, _window, cx| {
                    // The global keyboard-focus marker is transient render input;
                    // the confirmation lifecycle itself belongs to Host Tools.
                    host_tools.begin_podman_confirm_exit(exit_delay, cx);
                }),
                cx.listener(move |host_tools, _event, _window, cx| {
                    host_tools.confirm_podman_action_from_view(confirm_delay, cx);
                }),
            )
            .into_any_element(),
        )
    }
}

fn host_podman_confirm_description_key(action: &PodmanActionKind) -> &'static str {
    match action {
        PodmanActionKind::Start => "sidebar.host_podman.confirm.start_desc",
        PodmanActionKind::Stop => "sidebar.host_podman.confirm.stop_desc",
        PodmanActionKind::Restart => "sidebar.host_podman.confirm.restart_desc",
        PodmanActionKind::Pause => "sidebar.host_podman.confirm.pause_desc",
        PodmanActionKind::Unpause => "sidebar.host_podman.confirm.unpause_desc",
        PodmanActionKind::Remove => "sidebar.host_podman.confirm.remove_desc",
    }
}

fn host_podman_confirm_label_key(action: &PodmanActionKind) -> &'static str {
    match action {
        PodmanActionKind::Start => "sidebar.host_podman.actions.start",
        PodmanActionKind::Stop => "sidebar.host_podman.actions.stop",
        PodmanActionKind::Restart => "sidebar.host_podman.actions.restart",
        PodmanActionKind::Pause => "sidebar.host_podman.actions.pause",
        PodmanActionKind::Unpause => "sidebar.host_podman.actions.unpause",
        PodmanActionKind::Remove => "sidebar.host_podman.actions.remove",
    }
}

fn podman_state_color(state: &str, muted_color: u32) -> u32 {
    match state.trim().to_lowercase().as_str() {
        "running" => MONITOR_EMERALD,
        "created" | "configured" | "paused" | "restarting" => MONITOR_AMBER,
        "dead" | "removing" => MONITOR_RED,
        "exited" | "stopped" => muted_color,
        _ => muted_color,
    }
}
