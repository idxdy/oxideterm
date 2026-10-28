//! Owns the Redis Host Tool: endpoint discovery, optional password entry, and
//! the per-database key tree.
//!
//! Discovery lists the loopback server and every running Redis-family
//! container. Browsing reads one endpoint through `docker exec` or the host
//! `redis-cli`; a password typed in the panel reaches the remote process on the
//! exec channel's stdin, never through the command line.

use super::*;

use oxideterm_gpui_ui::button::ButtonVariant;

impl WorkspaceApp {
    pub(super) fn render_host_redis_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens;
        let i18n = &self.i18n;
        let mono_font_family = settings_mono_font_family(self.settings_store.settings());
        let selectable_text = self.selectable_text_render_state(cx);
        let password_ime = self
            .host_tools_plain_text_ime_frame(HostToolsTextInput::RedisPassword, cx)
            .expect("redis password is a registered Host Tools input");
        self.host_tools.update(cx, |host_tools, cx| {
            host_tools.render_host_redis_panel(
                password_ime,
                &tokens,
                i18n,
                mono_font_family,
                &selectable_text,
                cx,
            )
        })
    }

    pub(in crate::workspace) fn handle_host_redis_key(
        &mut self,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let key = event.keystroke.key.as_str();
        let escape = key == "escape" && !event.keystroke.modifiers.platform;
        let submit = matches!(key, "enter" | "return");

        if self.host_tools.read(cx).redis_delete_open() {
            return match self.handle_standard_confirm_key(event, cx) {
                Some(ConfirmKeyboardAction::Cancel) => {
                    self.host_tools.update(cx, |host_tools, cx| {
                        host_tools.dismiss_redis_delete(cx);
                    });
                    true
                }
                Some(ConfirmKeyboardAction::Confirm) => {
                    self.clear_standard_confirm_focus();
                    self.host_tools.update(cx, |host_tools, cx| {
                        host_tools.confirm_redis_delete(cx);
                    });
                    true
                }
                Some(ConfirmKeyboardAction::Handled) => true,
                None => false,
            };
        }

        let host_tools = self.host_tools.read(cx);
        let create_key_focused = host_tools
            .ui
            .input_is_focused(HostToolsTextInput::RedisCreateKeyName)
            || host_tools
                .ui
                .input_is_focused(HostToolsTextInput::RedisCreateKeyValue)
            || host_tools
                .ui
                .input_is_focused(HostToolsTextInput::RedisCreateKeyField)
            || host_tools
                .ui
                .input_is_focused(HostToolsTextInput::RedisCreateKeyTtl);
        let set_value_focused = host_tools
            .ui
            .input_is_focused(HostToolsTextInput::RedisSetValue);
        let rename_focused = host_tools
            .ui
            .input_is_focused(HostToolsTextInput::RedisRename);
        let password_focused = host_tools
            .ui
            .input_is_focused(HostToolsTextInput::RedisPassword);
        let value_open = host_tools.redis_value_dialog_open();
        let rename_open = host_tools.redis_rename_open();
        let set_value_open = host_tools.redis_set_value_open();
        let create_key_open = host_tools.redis_create_key_open();

        if create_key_focused {
            if escape {
                self.host_tools.update(cx, |host_tools, cx| {
                    host_tools.dismiss_redis_create_key(cx);
                });
                self.ime_marked_text = None;
                self.clear_ime_selection();
                cx.notify();
                return true;
            }
            if submit {
                self.host_tools.update(cx, |host_tools, cx| {
                    host_tools.submit_redis_create_key(cx);
                });
                return true;
            }
            return false;
        }

        if set_value_focused {
            if escape {
                self.host_tools.update(cx, |host_tools, cx| {
                    host_tools.dismiss_redis_set_value(cx);
                });
                self.ime_marked_text = None;
                self.clear_ime_selection();
                cx.notify();
                return true;
            }
            if submit {
                self.host_tools.update(cx, |host_tools, cx| {
                    host_tools.submit_redis_set_value(cx);
                });
                return true;
            }
            return false;
        }

        if rename_focused {
            if escape {
                self.host_tools.update(cx, |host_tools, cx| {
                    host_tools.dismiss_redis_rename(cx);
                });
                self.ime_marked_text = None;
                self.clear_ime_selection();
                cx.notify();
                return true;
            }
            if submit {
                self.host_tools.update(cx, |host_tools, cx| {
                    host_tools.submit_redis_rename(cx);
                });
                return true;
            }
            return false;
        }
        if password_focused {
            if escape {
                self.host_tools.update(cx, |host_tools, _cx| {
                    host_tools.ui.clear_input_focus();
                });
                self.ime_marked_text = None;
                self.clear_ime_selection();
                cx.notify();
                return true;
            }
            if submit {
                // Enter re-reads the selected endpoint with the password
                // currently in the field, so typing a password does not
                // require a pointer.
                self.host_tools.update(cx, |host_tools, cx| {
                    host_tools.request_redis_keys_for_selected(cx);
                });
                return true;
            }
            return false;
        }
        if escape && create_key_open {
            self.host_tools.update(cx, |host_tools, cx| {
                host_tools.dismiss_redis_create_key(cx);
            });
            return true;
        }
        if escape && set_value_open {
            self.host_tools.update(cx, |host_tools, cx| {
                host_tools.dismiss_redis_set_value(cx);
            });
            return true;
        }
        if escape && rename_open {
            self.host_tools.update(cx, |host_tools, cx| {
                host_tools.dismiss_redis_rename(cx);
            });
            return true;
        }
        if escape && value_open {
            self.host_tools.update(cx, |host_tools, cx| {
                host_tools.dismiss_redis_value(cx);
            });
            return true;
        }
        false
    }
}

impl HostToolsEntity {
    pub(in crate::workspace::connection_monitor) fn request_redis_snapshot(
        &mut self,
        connection_id: String,
        runtime: tokio::runtime::Handle,
        connection_fallback: String,
        failure_fallback: String,
        cx: &mut Context<Self>,
    ) {
        let request = HostRedisSnapshotRequest {
            connection_id,
            connection_fallback,
            failure_fallback,
        };
        if self.host_redis.snapshot_in_flight {
            // Keep only the newest selection or refresh while the current
            // bounded probe completes.
            self.host_redis.snapshot_pending = Some(HostRedisSnapshotPending { request, runtime });
            return;
        }
        self.start_redis_snapshot(request, runtime, cx);
    }

    fn start_redis_snapshot(
        &mut self,
        request: HostRedisSnapshotRequest,
        runtime: tokio::runtime::Handle,
        cx: &mut Context<Self>,
    ) {
        let Some(os_type) = self.connection_os_type(&request.connection_id) else {
            self.host_redis.snapshot_connection_id = Some(request.connection_id);
            self.host_redis.snapshot = Some(ResourceRedisSnapshot {
                status: ResourceRedisStatus::Error {
                    message: request.connection_fallback,
                },
                endpoints: Vec::new(),
            });
            cx.notify();
            return;
        };
        let command = redis_discovery_command(&os_type).to_string();
        if self.host_redis.snapshot_connection_id.as_deref() != Some(request.connection_id.as_str())
        {
            self.host_redis.snapshot = None;
        }
        self.host_redis.snapshot_connection_id = Some(request.connection_id.clone());
        self.host_redis.snapshot_running = Some(request.clone());
        self.host_redis.snapshot_in_flight = true;
        let spawned = self.spawn_redis_snapshot_capture(
            command,
            request.clone(),
            HOST_REDIS_PROBE_TIMEOUT,
            HOST_REDIS_PROBE_MAX_OUTPUT_SIZE,
            runtime,
        );
        if !spawned {
            self.host_redis.snapshot_running = None;
            self.host_redis.snapshot_in_flight = false;
            self.host_redis.snapshot = Some(ResourceRedisSnapshot {
                status: ResourceRedisStatus::Error {
                    message: request.connection_fallback,
                },
                endpoints: Vec::new(),
            });
        }
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn finish_host_redis_snapshot(
        &mut self,
        delivery: HostRedisSnapshotDelivery,
        cx: &mut Context<Self>,
    ) {
        if self.host_redis.snapshot_running.as_ref() != Some(&delivery.request) {
            return;
        }
        self.host_redis.snapshot_in_flight = false;
        self.host_redis.snapshot_running = None;
        let snapshot = match delivery.result {
            Ok(mut output) if output.exit_code.unwrap_or(0) == 0 => {
                let snapshot = parse_redis_snapshot(&output.stdout);
                zeroize::Zeroize::zeroize(&mut output.stdout);
                zeroize::Zeroize::zeroize(&mut output.stderr);
                snapshot
            }
            Ok(mut output) => {
                // Probe failures use a localized safe fallback, never the
                // captured remote output.
                zeroize::Zeroize::zeroize(&mut output.stdout);
                zeroize::Zeroize::zeroize(&mut output.stderr);
                ResourceRedisSnapshot {
                    status: ResourceRedisStatus::Error {
                        message: delivery.request.failure_fallback.clone(),
                    },
                    endpoints: Vec::new(),
                }
            }
            Err(()) => ResourceRedisSnapshot {
                status: ResourceRedisStatus::Error {
                    message: delivery.request.failure_fallback.clone(),
                },
                endpoints: Vec::new(),
            },
        };
        self.host_redis.snapshot_connection_id = Some(delivery.request.connection_id.clone());
        self.host_redis.snapshot = Some(snapshot);
        self.sync_redis_endpoint_selection(
            &delivery.request.connection_id,
            delivery.request.failure_fallback,
            cx,
        );
        let pending = self.host_redis.snapshot_pending.take();
        if let Some(pending) = pending {
            self.start_redis_snapshot(pending.request, pending.runtime, cx);
        }
        cx.notify();
    }

    /// Keeps the browsed endpoint inside the freshly discovered list, then reads
    /// it so the tree recovers after a refresh without extra clicks.
    fn sync_redis_endpoint_selection(
        &mut self,
        connection_id: &str,
        failure_fallback: String,
        cx: &mut Context<Self>,
    ) {
        let endpoints = self
            .host_redis
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.endpoints.clone())
            .unwrap_or_default();
        let selected_is_live = self
            .host_redis
            .keys_endpoint
            .as_ref()
            .is_some_and(|selected| endpoints.contains(selected));
        if !selected_is_live {
            self.host_redis.keys_endpoint = endpoints.first().cloned();
            self.host_redis.keys = None;
            self.host_redis.keys_connection_id = None;
            self.host_redis.collapsed_databases.clear();
            self.host_redis.expanded_segments.clear();
        }
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            return;
        };
        self.request_redis_keys(connection_id.to_string(), runtime, failure_fallback, cx);
    }

    pub(in crate::workspace::connection_monitor) fn select_redis_endpoint(
        &mut self,
        endpoint: ResourceRedisEndpoint,
        cx: &mut Context<Self>,
    ) {
        let changed = self.host_redis.keys_endpoint.as_ref() != Some(&endpoint);
        self.host_redis.keys_endpoint = Some(endpoint);
        if changed {
            self.host_redis.keys = None;
            self.host_redis.collapsed_databases.clear();
            self.host_redis.expanded_segments.clear();
        }
        self.request_redis_keys_for_selected(cx);
    }

    pub(in crate::workspace::connection_monitor) fn request_redis_keys_for_selected(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let Some(connection_id) = self
            .selected_connection_id_owned()
            .or_else(|| self.host_redis.snapshot_connection_id.clone())
        else {
            return;
        };
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            return;
        };
        let Some(messages) = self.messages.clone() else {
            return;
        };
        self.request_redis_keys(connection_id, runtime, messages.redis_probe_failed, cx);
    }

    fn request_redis_keys(
        &mut self,
        connection_id: String,
        runtime: tokio::runtime::Handle,
        failure_fallback: String,
        cx: &mut Context<Self>,
    ) {
        let Some(endpoint) = self.host_redis.keys_endpoint.clone() else {
            return;
        };
        let request = HostRedisKeysRequest {
            connection_id,
            endpoint,
            password: self.ui.host_redis_password.clone(),
            failure_fallback,
        };
        if self.host_redis.keys_in_flight {
            // Only the newest endpoint or password replaces the running read.
            self.host_redis.keys_pending = Some(HostRedisKeysPending { request, runtime });
            cx.notify();
            return;
        }
        self.start_redis_keys(request, runtime, cx);
    }

    fn start_redis_keys(
        &mut self,
        request: HostRedisKeysRequest,
        runtime: tokio::runtime::Handle,
        cx: &mut Context<Self>,
    ) {
        let os_type = self.connection_os_type(&request.connection_id);
        let command = os_type
            .as_deref()
            .and_then(|os_type| redis_keys_command(&request.endpoint, os_type));
        let Some(command) = command else {
            // Unsupported host shells and rejected container names land here
            // without spawning a command.
            self.host_redis.keys_connection_id = Some(request.connection_id);
            self.host_redis.keys = Some(ResourceRedisKeys {
                status: ResourceRedisStatus::Error {
                    message: request.failure_fallback,
                },
                databases: Vec::new(),
            });
            cx.notify();
            return;
        };
        if self.host_redis.keys_connection_id.as_deref() != Some(request.connection_id.as_str()) {
            self.host_redis.keys = None;
        }
        self.host_redis.keys_connection_id = Some(request.connection_id.clone());
        self.host_redis.keys_running = Some(request.clone());
        self.host_redis.keys_in_flight = true;
        let spawned = self.spawn_redis_keys_capture(
            command,
            request.clone(),
            HOST_REDIS_KEYS_TIMEOUT,
            HOST_REDIS_KEYS_MAX_OUTPUT_SIZE,
            runtime,
        );
        if !spawned {
            self.host_redis.keys_running = None;
            self.host_redis.keys_in_flight = false;
            self.host_redis.keys = Some(ResourceRedisKeys {
                status: ResourceRedisStatus::Error {
                    message: request.failure_fallback,
                },
                databases: Vec::new(),
            });
        }
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn finish_host_redis_keys(
        &mut self,
        delivery: HostRedisKeysDelivery,
        cx: &mut Context<Self>,
    ) {
        let is_current = self
            .host_redis
            .keys_running
            .as_ref()
            .is_some_and(|running| running.same_target(&delivery.request));
        if !is_current {
            return;
        }
        self.host_redis.keys_in_flight = false;
        self.host_redis.keys_running = None;
        let keys = match delivery.result {
            Ok(output) if output.exit_code.unwrap_or(0) == 0 => {
                let text = String::from_utf8_lossy(&output.stdout);
                parse_redis_keys(&text)
            }
            Ok(_) => ResourceRedisKeys {
                status: ResourceRedisStatus::Error {
                    message: delivery.request.failure_fallback.clone(),
                },
                databases: Vec::new(),
            },
            Err(()) => ResourceRedisKeys {
                status: ResourceRedisStatus::Error {
                    message: delivery.request.failure_fallback.clone(),
                },
                databases: Vec::new(),
            },
        };
        self.host_redis.keys = Some(keys);
        let pending = self.host_redis.keys_pending.take();
        if let Some(pending) = pending {
            self.start_redis_keys(pending.request, pending.runtime, cx);
        }
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn toggle_redis_tree_row(
        &mut self,
        path: String,
        is_database: bool,
        cx: &mut Context<Self>,
    ) {
        let target = if is_database {
            &mut self.host_redis.collapsed_databases
        } else {
            &mut self.host_redis.expanded_segments
        };
        if !target.remove(&path) {
            target.insert(path);
        }
        cx.notify();
    }
}

struct HostRedisPanelSnapshot {
    connections: Vec<MonitorConnectionOption>,
    selected_connection_id: String,
    snapshot: Option<ResourceRedisSnapshot>,
    endpoints: Vec<ResourceRedisEndpoint>,
    selected_endpoint: Option<ResourceRedisEndpoint>,
    keys: Option<ResourceRedisKeys>,
    in_flight: bool,
}

struct HostRedisTreeRow {
    path: String,
    label: String,
    depth: usize,
    key_count: usize,
    expandable: bool,
    expanded: bool,
    is_database: bool,
    is_key: bool,
    /// Full key name for rows that address one key.
    key: Option<String>,
    /// Database the row belongs to; keys are only unique within one database.
    database: u32,
    /// Synthetic leaf standing for a prefix that is itself a key.
    is_current_key: bool,
}

impl HostToolsEntity {
    fn redis_panel_snapshot(&self) -> Option<HostRedisPanelSnapshot> {
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
        let same_connection = self.host_redis.snapshot_connection_id.as_deref()
            == Some(selected_connection_id.as_str());
        let snapshot = same_connection
            .then(|| self.host_redis.snapshot.clone())
            .flatten();
        let endpoints = snapshot
            .as_ref()
            .map(|snapshot| snapshot.endpoints.clone())
            .unwrap_or_default();
        let keys = (self.host_redis.keys_connection_id.as_deref()
            == Some(selected_connection_id.as_str()))
        .then(|| self.host_redis.keys.clone())
        .flatten();

        Some(HostRedisPanelSnapshot {
            connections,
            selected_connection_id,
            snapshot,
            endpoints,
            selected_endpoint: self.host_redis.keys_endpoint.clone(),
            keys,
            in_flight: self.host_redis.snapshot_in_flight || self.host_redis.keys_in_flight,
        })
    }

    fn redis_tree_rows(&self, databases: &[ResourceRedisDatabase]) -> Vec<HostRedisTreeRow> {
        host_redis_tree_rows(
            databases,
            self.ui.host_redis_show_all_databases,
            &self.host_redis.collapsed_databases,
            &self.host_redis.expanded_segments,
        )
    }

    pub(in crate::workspace::connection_monitor) fn toggle_redis_database_filter(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        self.ui.host_redis_show_all_databases = !self.ui.host_redis_show_all_databases;
        cx.notify();
    }

    fn sync_redis_render_rows(&self, rows: &[HostRedisTreeRow], selected_connection_id: &str) {
        let signatures = rows
            .iter()
            .map(|row| {
                let mut hasher = std::hash::DefaultHasher::new();
                std::hash::Hash::hash(&row.path, &mut hasher);
                std::hash::Hash::hash(&row.expanded, &mut hasher);
                std::hash::Hash::hash(&row.key_count, &mut hasher);
                std::hash::Hash::hash(&row.key, &mut hasher);
                std::hash::Hasher::finish(&hasher)
            })
            .collect::<Vec<_>>();
        let identity = format!("host-redis:{selected_connection_id}");
        sync_tauri_variable_list_state_by_signatures(
            &self.host_redis.list_state,
            &mut self.host_redis.list_cache.borrow_mut(),
            &identity,
            &signatures,
            TauriVirtualListSpec::new(px(HOST_REDIS_TREE_ROW_HEIGHT), 8),
        );
    }

    fn render_host_redis_panel(
        &self,
        password_ime: HostToolsPlainTextImeFrame,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(snapshot) = self.redis_panel_snapshot() else {
            return host_tools_center_state(
                LucideIcon::WifiOff,
                tokens.ui.text_muted,
                i18n.t("profiler.panel.no_connection"),
                selectable_text,
                cx,
            );
        };
        let selected_connection_id = snapshot.selected_connection_id.clone();
        let rows = std::sync::Arc::new(
            snapshot
                .keys
                .as_ref()
                .map(|keys| self.redis_tree_rows(&keys.databases))
                .unwrap_or_default(),
        );
        self.sync_redis_render_rows(&rows, &selected_connection_id);
        let body = self.render_host_redis_body(
            snapshot.keys.as_ref(),
            rows.clone(),
            snapshot
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.status.clone()),
            snapshot.endpoints.is_empty(),
            snapshot.in_flight,
            tokens,
            i18n,
            mono_font_family.clone(),
            selectable_text,
            cx,
        );

        div()
            .id("host-redis-panel")
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
                        &snapshot.connections,
                        &selected_connection_id,
                        snapshot.snapshot.is_some(),
                        tokens,
                        mono_font_family.clone(),
                        selectable_text,
                        cx,
                    ))
                    .child(self.render_host_redis_endpoints(
                        &snapshot.endpoints,
                        snapshot.selected_endpoint.as_ref(),
                        tokens,
                        i18n,
                        cx,
                    ))
                    .child(self.render_host_redis_password(&password_ime, tokens, i18n, cx))
                    .child(self.render_host_redis_status_row(
                        rows.len(),
                        snapshot.in_flight,
                        self.ui.host_redis_show_all_databases,
                        tokens,
                        i18n,
                        cx,
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(body),
            )
            .into_any_element()
    }

    fn render_host_redis_endpoints(
        &self,
        endpoints: &[ResourceRedisEndpoint],
        selected: Option<&ResourceRedisEndpoint>,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if endpoints.is_empty() {
            return div().into_any_element();
        }
        let theme = tokens.ui;
        let mut row = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_wrap()
            .gap_1()
            .children(endpoints.iter().map(|endpoint| {
                let active = selected == Some(endpoint);
                let endpoint = endpoint.clone();
                let icon = match endpoint.kind {
                    ResourceRedisEndpointKind::Docker => LucideIcon::Container,
                    ResourceRedisEndpointKind::Host => LucideIcon::Server,
                };
                div()
                    .id(host_tools_row_id(
                        "host-redis-endpoint",
                        &endpoint.target,
                        endpoint.target.clone(),
                    ))
                    .h(px(HOST_REDIS_ENDPOINT_CHIP_HEIGHT))
                    .px_2()
                    .min_w_0()
                    .max_w_full()
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded(px(tokens.radii.md))
                    .border_1()
                    .border_color(rgba(if active {
                        (theme.accent << 8) | MONITOR_BORDER_ALPHA
                    } else {
                        (theme.border << 8) | MONITOR_BORDER_ALPHA
                    }))
                    .when(active, |chip| {
                        chip.bg(rgba((theme.bg_panel << 8) | MONITOR_TINT_ALPHA))
                    })
                    .cursor_pointer()
                    .hover(|chip| chip.bg(rgb(theme.bg_hover)))
                    .child(WorkspaceApp::render_lucide_icon(
                        icon,
                        12.0,
                        rgb(if active {
                            theme.accent
                        } else {
                            theme.text_muted
                        }),
                    ))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.0))
                            .text_color(rgb(if active { theme.text } else { theme.text_muted }))
                            .child(endpoint.target.clone()),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |host_tools, _event, _window, cx| {
                            host_tools.select_redis_endpoint(endpoint.clone(), cx);
                            cx.stop_propagation();
                        }),
                    )
            }));
        row = row.child(
            div()
                .h(px(HOST_REDIS_ENDPOINT_CHIP_HEIGHT))
                .flex()
                .items_center()
                .px_1()
                .text_size(px(10.0))
                .text_color(rgb(theme.text_muted))
                .child(i18n.t("sidebar.host_redis.endpoint_hint")),
        );
        row.into_any_element()
    }

    fn render_host_redis_password(
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
                    value: self.ui.host_redis_password.as_str(),
                    placeholder: i18n.t("sidebar.host_redis.password_placeholder"),
                    focused: self.ui.input_is_focused(input),
                    caret_visible: ime.caret_visible(),
                    // A Redis password is a credential: only the mask is painted.
                    secret: true,
                    selected_all: false,
                    selected_range: ime.selected_range(),
                    marked_text: ime.marked_text(),
                },
            )
            .h(px(30.0))
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

    fn render_host_redis_status_row(
        &self,
        visible_rows: usize,
        in_flight: bool,
        show_all_databases: bool,
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
            .child(
                div().min_w_0().flex_1().truncate().child(
                    i18n.t("sidebar.host_redis.tree_hint")
                        .replace("{{rows}}", &visible_rows.to_string()),
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
                        LucideIcon::ListTree,
                        13.0,
                        if show_all_databases {
                            rgb(theme.accent)
                        } else {
                            rgb(theme.text)
                        },
                        oxideterm_gpui_ui::button::IconButtonOptions {
                            size: 24.0,
                            has_background: true,
                            background: Some(rgb(if show_all_databases {
                                theme.bg_panel
                            } else {
                                theme.bg_hover
                            })),
                            hover_background: Some(rgb(theme.bg_panel)),
                            idle_opacity: 1.0,
                            ..oxideterm_gpui_ui::button::IconButtonOptions::compact(24.0)
                        },
                        if show_all_databases {
                            i18n.t("sidebar.host_redis.filter_only_with_keys")
                        } else {
                            i18n.t("sidebar.host_redis.filter_all_databases")
                        },
                        "host-redis-database-filter",
                        true,
                        cx.listener(move |host_tools, _event, _window, cx| {
                            host_tools.toggle_redis_database_filter(cx);
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
                            disabled: in_flight,
                            has_background: true,
                            background: Some(rgb(theme.bg_hover)),
                            hover_background: Some(rgb(theme.bg_panel)),
                            idle_opacity: 1.0,
                            ..oxideterm_gpui_ui::button::IconButtonOptions::compact(24.0)
                        },
                        i18n.t("sidebar.host_redis.actions.refresh"),
                        "host-redis-refresh",
                        true,
                        cx.listener(move |host_tools, _event, _window, cx| {
                            host_tools
                                .request_active_tool_snapshot(HostSnapshotFeedback::Toast, cx);
                            cx.stop_propagation();
                        }),
                    )),
            )
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_host_redis_body(
        &self,
        keys: Option<&ResourceRedisKeys>,
        rows: std::sync::Arc<Vec<HostRedisTreeRow>>,
        discovery_status: Option<ResourceRedisStatus>,
        has_endpoints: bool,
        in_flight: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(keys) = keys {
            match &keys.status {
                ResourceRedisStatus::Available if !rows.is_empty() => {
                    return self.render_host_redis_tree(rows, tokens, i18n, mono_font_family, cx);
                }
                ResourceRedisStatus::Available => {
                    return host_tools_center_state(
                        LucideIcon::Database,
                        tokens.ui.text_muted,
                        i18n.t("sidebar.host_redis.keys_empty"),
                        selectable_text,
                        cx,
                    );
                }
                status if !in_flight => {
                    return self.render_host_redis_state(status, tokens, i18n, selectable_text, cx);
                }
                _ => {}
            }
        }
        if in_flight {
            return host_tools_center_state(
                LucideIcon::Database,
                tokens.ui.text_muted,
                i18n.t("sidebar.host_redis.loading"),
                selectable_text,
                cx,
            );
        }
        if discovery_status == Some(ResourceRedisStatus::Available) && !has_endpoints {
            // Only the availability probe ran: this host shell cannot list
            // endpoints or browse keys.
            return host_tools_center_state(
                LucideIcon::Lock,
                tokens.ui.text_muted,
                i18n.t("sidebar.host_redis.browse_unsupported"),
                selectable_text,
                cx,
            );
        }
        self.render_host_redis_state(
            &discovery_status.unwrap_or_default(),
            tokens,
            i18n,
            selectable_text,
            cx,
        )
    }

    fn render_host_redis_state(
        &self,
        status: &ResourceRedisStatus,
        tokens: &ThemeTokens,
        i18n: &I18n,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match status {
            ResourceRedisStatus::Available => host_tools_center_state(
                LucideIcon::CheckCircle,
                tokens.ui.text_muted,
                i18n.t("sidebar.host_redis.keys_empty"),
                selectable_text,
                cx,
            ),
            ResourceRedisStatus::Unavailable => host_tools_center_state(
                LucideIcon::Database,
                tokens.ui.text_muted,
                i18n.t("sidebar.host_redis.unavailable"),
                selectable_text,
                cx,
            ),
            ResourceRedisStatus::NoServer => host_tools_center_state(
                LucideIcon::Database,
                tokens.ui.text_muted,
                i18n.t("sidebar.host_redis.no_endpoint"),
                selectable_text,
                cx,
            ),
            ResourceRedisStatus::AuthRequired => host_tools_center_state(
                LucideIcon::Lock,
                tokens.ui.text_muted,
                i18n.t("sidebar.host_redis.auth_required"),
                selectable_text,
                cx,
            ),
            ResourceRedisStatus::Error { message } => host_tools_center_state(
                LucideIcon::AlertTriangle,
                MONITOR_RED,
                i18n.t("sidebar.host_redis.error")
                    .replace("{{error}}", message),
                selectable_text,
                cx,
            ),
            ResourceRedisStatus::Unknown => host_tools_center_state(
                LucideIcon::Database,
                tokens.ui.text_muted,
                i18n.t("sidebar.host_redis.loading"),
                selectable_text,
                cx,
            ),
        }
    }

    fn render_host_redis_tree(
        &self,
        rows: std::sync::Arc<Vec<HostRedisTreeRow>>,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = self.host_redis.list_state.clone();
        let spec = TauriVirtualListSpec::new(px(HOST_REDIS_TREE_ROW_HEIGHT), 8);
        let host_tools = cx.entity();
        let tokens = *tokens;
        let i18n = i18n.clone();
        div()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .child(tauri_virtual_list(
                state,
                spec,
                move |index, _window, cx| {
                    let rows = rows.clone();
                    let tokens = tokens;
                    let i18n = i18n.clone();
                    let mono_font_family = mono_font_family.clone();
                    host_tools.update(cx, |host_tools, cx| {
                        host_tools.render_host_redis_row(
                            index,
                            rows.as_ref(),
                            &tokens,
                            &i18n,
                            mono_font_family,
                            cx,
                        )
                    })
                },
            ))
            .into_any_element()
    }

    fn render_host_redis_row(
        &self,
        index: usize,
        rows: &[HostRedisTreeRow],
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(row) = rows.get(index) else {
            return div().into_any_element();
        };
        let theme = tokens.ui;
        let indent = 12.0 + row.depth as f32 * 14.0;
        let icon = if row.is_database {
            LucideIcon::Database
        } else if row.is_key {
            LucideIcon::Key
        } else {
            LucideIcon::Folder
        };
        let chevron = if !row.expandable {
            None
        } else if row.expanded {
            Some(LucideIcon::ChevronDown)
        } else {
            Some(LucideIcon::ChevronRight)
        };
        let path = row.path.clone();
        let is_database = row.is_database;
        let row_key = row.key.clone();
        let row_database = row.database;
        let count_label = i18n
            .t("sidebar.host_redis.key_count")
            .replace("{{count}}", &row.key_count.to_string());

        let mut element = div()
            .id(host_tools_row_id("host-redis-row", &row.path, path.clone()))
            .w_full()
            .min_w_0()
            .h(px(HOST_REDIS_TREE_ROW_HEIGHT))
            .pl(px(indent))
            .pr_3()
            .flex()
            .items_center()
            .gap_1()
            .border_b_1()
            .border_color(rgba((theme.border << 8) | MONITOR_BORDER_ALPHA))
            .when(row.expandable || row.is_key, |row| row.cursor_pointer())
            .hover(|row| row.bg(rgb(theme.bg_hover)))
            .when_some(chevron, |element, chevron| {
                let toggle_path = path.clone();
                element.child(
                    div()
                        .id(host_tools_row_id(
                            "host-redis-chevron",
                            &toggle_path,
                            toggle_path.clone(),
                        ))
                        .flex_none()
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .child(WorkspaceApp::render_lucide_icon(
                            chevron,
                            12.0,
                            rgb(theme.text_muted),
                        ))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |host_tools, _event, _window, cx| {
                                host_tools.toggle_redis_tree_row(
                                    toggle_path.clone(),
                                    is_database,
                                    cx,
                                );
                                cx.stop_propagation();
                            }),
                        ),
                )
            })
            .when(chevron.is_none(), |element| {
                element.child(div().w(px(12.0)))
            })
            .child(WorkspaceApp::render_lucide_icon(
                icon,
                12.0,
                rgb(if row.is_key {
                    theme.text
                } else {
                    theme.text_muted
                }),
            ))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_size(px(11.0))
                    .font_family(mono_font_family)
                    .when(row.is_database, |label| {
                        label.font_weight(gpui::FontWeight::SEMIBOLD)
                    })
                    .text_color(rgb(theme.text))
                    .child(if row.is_current_key {
                        i18n.t("sidebar.host_redis.tree_current")
                    } else {
                        row.label.clone()
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(10.0))
                    .text_color(rgb(theme.text_muted))
                    .child(count_label),
            );

        if row.is_database {
            let create_database = row.database;
            element = element.child(host_tools_tooltip_icon_button(
                tokens,
                LucideIcon::Plus,
                13.0,
                rgb(theme.text_muted),
                oxideterm_gpui_ui::button::IconButtonOptions {
                    size: 20.0,
                    has_background: false,
                    idle_opacity: 1.0,
                    ..oxideterm_gpui_ui::button::IconButtonOptions::compact(20.0)
                },
                i18n.t("sidebar.host_redis.actions.new_key"),
                "host-redis-create-key",
                true,
                cx.listener(move |host_tools, _event, _window, cx| {
                    host_tools.open_redis_create_key(create_database, cx);
                    cx.stop_propagation();
                }),
            ));
        }

        if let Some(key) = row_key {
            let copy_key = key.clone();
            let copy_value = key.clone();
            let delete_key = key.clone();
            element = element
                .child(host_tools_tooltip_icon_button(
                    tokens,
                    LucideIcon::Copy,
                    12.0,
                    rgb(theme.text_muted),
                    oxideterm_gpui_ui::button::IconButtonOptions {
                        size: 20.0,
                        has_background: false,
                        idle_opacity: 1.0,
                        ..oxideterm_gpui_ui::button::IconButtonOptions::compact(20.0)
                    },
                    i18n.t("sidebar.host_redis.actions.copy_key"),
                    "host-redis-copy-key",
                    true,
                    cx.listener(move |host_tools, _event, _window, cx| {
                        host_tools.copy_redis_key(copy_key.clone(), cx);
                        cx.stop_propagation();
                    }),
                ))
                .child(host_tools_tooltip_icon_button(
                    tokens,
                    LucideIcon::FileText,
                    12.0,
                    rgb(theme.text_muted),
                    oxideterm_gpui_ui::button::IconButtonOptions {
                        size: 20.0,
                        has_background: false,
                        idle_opacity: 1.0,
                        ..oxideterm_gpui_ui::button::IconButtonOptions::compact(20.0)
                    },
                    i18n.t("sidebar.host_redis.actions.copy_value"),
                    "host-redis-copy-value",
                    true,
                    cx.listener(move |host_tools, _event, _window, cx| {
                        host_tools.copy_redis_value(row_database, copy_value.clone(), cx);
                        cx.stop_propagation();
                    }),
                ))
                .child(host_tools_tooltip_icon_button(
                    tokens,
                    LucideIcon::Trash2,
                    12.0,
                    rgb(MONITOR_RED),
                    oxideterm_gpui_ui::button::IconButtonOptions {
                        size: 20.0,
                        has_background: false,
                        idle_opacity: 1.0,
                        ..oxideterm_gpui_ui::button::IconButtonOptions::compact(20.0)
                    },
                    i18n.t("sidebar.host_redis.actions.delete"),
                    "host-redis-delete-key",
                    true,
                    cx.listener(move |host_tools, _event, _window, cx| {
                        host_tools.request_redis_delete(row_database, delete_key.clone(), cx);
                        cx.stop_propagation();
                    }),
                ));
        }

        // A key row opens the value viewer; prefix rows only expand.
        if let Some(key) = row.key.clone() {
            element = element.on_mouse_down(
                MouseButton::Left,
                cx.listener(move |host_tools, _event, _window, cx| {
                    host_tools.open_redis_value(row_database, key.clone(), cx);
                    cx.stop_propagation();
                }),
            );
        } else if row.expandable {
            element = element.on_mouse_down(
                MouseButton::Left,
                cx.listener(move |host_tools, _event, _window, cx| {
                    host_tools.toggle_redis_tree_row(path.clone(), is_database, cx);
                    cx.stop_propagation();
                }),
            );
        }

        element.into_any_element()
    }
}

impl WorkspaceApp {
    pub(in crate::workspace) fn render_host_redis_value_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let tokens = self.tokens;
        let i18n = &self.i18n;
        let mono_font_family = settings_mono_font_family(self.settings_store.settings());
        self.host_tools.update(cx, |host_tools, cx| {
            host_tools.render_host_redis_value_dialog(&tokens, i18n, mono_font_family, cx)
        })
    }

    pub(in crate::workspace) fn render_host_redis_delete_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let tokens = self.tokens;
        let i18n = &self.i18n;
        let focused_action = self.standard_confirm_focus();
        self.host_tools.update(cx, |host_tools, cx| {
            host_tools.render_host_redis_delete_dialog(&tokens, i18n, focused_action, cx)
        })
    }

    pub(in crate::workspace) fn render_host_redis_rename_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let target = WorkspaceImeTarget::HostRedisRename;
        let (key, submit_disabled, input_control) = {
            let host_tools = self.host_tools.read(cx);
            let ui = &host_tools.ui;
            let dialog = ui.host_redis_rename_dialog.as_ref()?;
            let input_control = self
                .workspace_text_input(
                    target,
                    TextInputView {
                        value: dialog.value.as_str(),
                        placeholder: self.i18n.t("sidebar.host_redis.rename.placeholder"),
                        focused: ui.input_is_focused(HostToolsTextInput::RedisRename),
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
                dialog.key.clone(),
                dialog.value.trim().is_empty() || dialog.loading,
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
                            host_tools.ui.focus_input(HostToolsTextInput::RedisRename);
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
            self.i18n.t("sidebar.host_redis.rename.cancel"),
            ButtonVariant::Secondary,
            ConfirmDialogAction::Cancel,
            false,
            None,
            |this, _event, _window, cx| {
                this.host_tools.update(cx, |host_tools, cx| {
                    host_tools.dismiss_redis_rename(cx);
                });
                this.ime_marked_text = None;
                this.clear_ime_selection();
                cx.notify();
            },
            cx,
        );
        let submit_action = self.workspace_confirm_footer_action_button(
            self.i18n.t("sidebar.host_redis.rename.confirm"),
            ButtonVariant::Default,
            ConfirmDialogAction::Confirm,
            submit_disabled,
            None,
            |this, _event, _window, cx| {
                this.host_tools.update(cx, |host_tools, cx| {
                    host_tools.submit_redis_rename(cx);
                });
                cx.notify();
            },
            cx,
        );
        let error = {
            let host_tools = self.host_tools.read(cx);
            host_tools
                .ui
                .host_redis_rename_dialog
                .as_ref()?
                .error
                .clone()
        };
        let theme = self.tokens.ui;
        let body = div()
            .p_4()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(rgb(theme.text_muted))
                    .child(
                        self.i18n
                            .t("sidebar.host_redis.rename.current")
                            .replace("{{key}}", &key),
                    ),
            )
            .child(input_control)
            .when_some(error, |body, error| {
                body.child(
                    div()
                        .text_size(px(11.0))
                        .text_color(rgb(MONITOR_RED))
                        .child(error),
                )
            });
        Some(
            oxideterm_gpui_ui::modal::dismissible_dialog_backdrop()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event, _window, cx| {
                        this.host_tools.update(cx, |host_tools, cx| {
                            host_tools.dismiss_redis_rename(cx);
                        });
                        this.ime_marked_text = None;
                        this.clear_ime_selection();
                        cx.stop_propagation();
                    }),
                )
                .child(oxideterm_gpui_ui::modal::overlay_content_boundary(
                    oxideterm_gpui_ui::modal::dialog_content(&self.tokens)
                        .w(px(480.0))
                        .child(
                            div()
                                .px_4()
                                .py_3()
                                .border_b_1()
                                .border_color(rgb(theme.border))
                                .text_size(px(14.0))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgb(theme.text))
                                .child(self.i18n.t("sidebar.host_redis.rename.title")),
                        )
                        .child(body)
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
}

impl WorkspaceApp {
    pub(in crate::workspace) fn render_host_redis_set_value_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let target = WorkspaceImeTarget::HostRedisSetValue;
        let (key, database, json_only, submit_disabled, input_control) = {
            let host_tools = self.host_tools.read(cx);
            let ui = &host_tools.ui;
            let dialog = ui.host_redis_set_value_dialog.as_ref()?;
            let input_control = self
                .workspace_text_input(
                    target,
                    TextInputView {
                        value: dialog.value.as_str(),
                        placeholder: self.i18n.t("sidebar.host_redis.set_value.placeholder"),
                        focused: ui.input_is_focused(HostToolsTextInput::RedisSetValue),
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
                dialog.key.clone(),
                dialog.database,
                dialog.json_only,
                dialog.loading,
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
                            host_tools.ui.focus_input(HostToolsTextInput::RedisSetValue);
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
            self.i18n.t("sidebar.host_redis.set_value.cancel"),
            ButtonVariant::Secondary,
            ConfirmDialogAction::Cancel,
            false,
            None,
            |this, _event, _window, cx| {
                this.host_tools.update(cx, |host_tools, cx| {
                    host_tools.dismiss_redis_set_value(cx);
                });
                this.ime_marked_text = None;
                this.clear_ime_selection();
                cx.notify();
            },
            cx,
        );
        let submit_action = self.workspace_confirm_footer_action_button(
            self.i18n.t("sidebar.host_redis.set_value.confirm"),
            ButtonVariant::Default,
            ConfirmDialogAction::Confirm,
            submit_disabled,
            None,
            |this, _event, _window, cx| {
                this.host_tools.update(cx, |host_tools, cx| {
                    host_tools.submit_redis_set_value(cx);
                });
                cx.notify();
            },
            cx,
        );
        let error = {
            let host_tools = self.host_tools.read(cx);
            host_tools
                .ui
                .host_redis_set_value_dialog
                .as_ref()?
                .error
                .clone()
        };
        let theme = self.tokens.ui;
        let hint = if json_only {
            self.i18n.t("sidebar.host_redis.set_value.json_hint")
        } else {
            self.i18n.t("sidebar.host_redis.set_value.text_hint")
        };
        let body = div()
            .p_4()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(rgb(theme.text_muted))
                    .child(
                        self.i18n
                            .t("sidebar.host_redis.set_value.key")
                            .replace("{{db}}", &format!("db{database}"))
                            .replace("{{key}}", &key),
                    ),
            )
            .child(input_control)
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(rgb(theme.text_muted))
                    .child(hint),
            )
            .when_some(error, |body, error| {
                body.child(
                    div()
                        .text_size(px(11.0))
                        .text_color(rgb(MONITOR_RED))
                        .child(error),
                )
            });
        Some(
            oxideterm_gpui_ui::modal::dismissible_dialog_backdrop()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event, _window, cx| {
                        this.host_tools.update(cx, |host_tools, cx| {
                            host_tools.dismiss_redis_set_value(cx);
                        });
                        this.ime_marked_text = None;
                        this.clear_ime_selection();
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
                                .text_size(px(14.0))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgb(theme.text))
                                .child(self.i18n.t("sidebar.host_redis.set_value.title")),
                        )
                        .child(body)
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
}

impl WorkspaceApp {
    /// Shared single-line input for the create-key form.
    fn render_host_redis_create_input(
        &self,
        target: WorkspaceImeTarget,
        input: HostToolsTextInput,
        value: String,
        placeholder: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let focused = self.host_tools.read(cx).ui.input_is_focused(input);
        let workspace = cx.entity();
        let control = self
            .workspace_text_input(
                target,
                TextInputView {
                    value: value.as_str(),
                    placeholder,
                    focused,
                    caret_visible: self.input_caret.visible(),
                    secret: false,
                    selected_all: false,
                    selected_range: self.ime_selected_range_for_target(target, cx),
                    marked_text: self.marked_text_for_target(target, cx),
                },
            )
            .h(px(32.0))
            .cursor(CursorStyle::IBeam);
        text_input_anchor_probe(
            target.anchor_id(),
            control
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
                })),
            move |anchor, _window, cx| {
                let _ = workspace.update(cx, |this, cx| {
                    this.update_text_input_anchor(anchor, cx);
                });
            },
        )
        .into_any_element()
    }

    pub(in crate::workspace) fn render_host_redis_create_key_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (name, kind, ttl_never, ttl, value, field, entries, database, error, loading) = {
            let host_tools = self.host_tools.read(cx);
            let dialog = host_tools.ui.host_redis_create_key_dialog.as_ref()?;
            (
                dialog.name.to_string(),
                dialog.kind,
                dialog.ttl_never,
                dialog.ttl.to_string(),
                dialog.value.to_string(),
                dialog.field.to_string(),
                dialog
                    .entries
                    .iter()
                    .map(|entry| entry.to_string())
                    .collect::<Vec<_>>(),
                dialog.database,
                dialog.error.clone(),
                dialog.loading,
            )
        };
        let theme = self.tokens.ui;
        let i18n = &self.i18n;
        let mut body = div()
            .p_4()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(rgb(theme.text_muted))
                    .child(format!("db{database}")),
            )
            .child(self.render_host_redis_create_input(
                WorkspaceImeTarget::HostRedisCreateKeyName,
                HostToolsTextInput::RedisCreateKeyName,
                name,
                i18n.t("sidebar.host_redis.create_key.name_placeholder"),
                cx,
            ))
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(rgb(theme.text_muted))
                    .child(i18n.t("sidebar.host_redis.create_key.type")),
            );

        // Type selector: one chip per writable shape.
        let mut type_row = div().flex().flex_wrap().gap_1();
        for (candidate, label_key) in [
            (
                RedisCreateKind::String,
                "sidebar.host_redis.create_key.types.string",
            ),
            (
                RedisCreateKind::Hash,
                "sidebar.host_redis.create_key.types.hash",
            ),
            (
                RedisCreateKind::List,
                "sidebar.host_redis.create_key.types.list",
            ),
            (
                RedisCreateKind::Set,
                "sidebar.host_redis.create_key.types.set",
            ),
            (
                RedisCreateKind::SortedSet,
                "sidebar.host_redis.create_key.types.sorted_set",
            ),
            (
                RedisCreateKind::Json,
                "sidebar.host_redis.create_key.types.json",
            ),
        ] {
            let active = candidate == kind;
            type_row = type_row.child(
                div()
                    .id(("host-redis-create-kind", candidate as usize))
                    .h(px(24.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .rounded(px(self.tokens.radii.md))
                    .cursor_pointer()
                    .text_size(px(10.0))
                    .when(active, |chip| {
                        chip.bg(rgb(theme.bg_panel))
                            .border_1()
                            .border_color(rgba((theme.accent << 8) | MONITOR_BORDER_ALPHA))
                            .text_color(rgb(theme.text))
                    })
                    .when(!active, |chip| {
                        chip.text_color(rgb(theme.text_muted))
                            .hover(|chip| chip.bg(rgb(theme.bg_hover)))
                    })
                    .child(i18n.t(label_key))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _event, _window, cx| {
                            this.host_tools.update(cx, |host_tools, cx| {
                                host_tools.set_redis_create_kind(candidate, cx);
                            });
                            cx.stop_propagation();
                        }),
                    ),
            );
        }
        body = body.child(type_row);

        // TTL: "never expires" keeps the field hidden.
        let ttl_chip = div()
            .id("host-redis-create-ttl-never")
            .h(px(24.0))
            .px_2()
            .flex()
            .items_center()
            .rounded(px(self.tokens.radii.md))
            .cursor_pointer()
            .text_size(px(10.0))
            .when(ttl_never, |chip| {
                chip.bg(rgb(theme.bg_panel))
                    .border_1()
                    .border_color(rgba((theme.accent << 8) | MONITOR_BORDER_ALPHA))
                    .text_color(rgb(theme.text))
            })
            .when(!ttl_never, |chip| {
                chip.text_color(rgb(theme.text_muted))
                    .hover(|chip| chip.bg(rgb(theme.bg_hover)))
            })
            .child(i18n.t("sidebar.host_redis.create_key.ttl_never"))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event, _window, cx| {
                    this.host_tools.update(cx, |host_tools, cx| {
                        host_tools.toggle_redis_create_ttl(cx);
                    });
                    cx.stop_propagation();
                }),
            );
        let mut ttl_row = div().flex().items_center().gap_2().child(ttl_chip);
        if !ttl_never {
            ttl_row = ttl_row.child(div().flex_1().min_w_0().child(
                self.render_host_redis_create_input(
                    WorkspaceImeTarget::HostRedisCreateKeyTtl,
                    HostToolsTextInput::RedisCreateKeyTtl,
                    ttl,
                    i18n.t("sidebar.host_redis.create_key.ttl_placeholder"),
                    cx,
                ),
            ));
        }
        body = body.child(ttl_row);

        // Value editor per shape.
        match kind {
            RedisCreateKind::String | RedisCreateKind::Json => {
                body = body.child(self.render_host_redis_create_input(
                    WorkspaceImeTarget::HostRedisCreateKeyValue,
                    HostToolsTextInput::RedisCreateKeyValue,
                    value,
                    i18n.t("sidebar.host_redis.create_key.value_placeholder"),
                    cx,
                ));
                body = body.child(
                    div()
                        .text_size(px(10.0))
                        .text_color(rgb(theme.text_muted))
                        .child(if kind == RedisCreateKind::Json {
                            i18n.t("sidebar.host_redis.create_key.json_hint")
                        } else {
                            i18n.t("sidebar.host_redis.create_key.value_hint")
                        }),
                );
            }
            RedisCreateKind::Hash
            | RedisCreateKind::List
            | RedisCreateKind::Set
            | RedisCreateKind::SortedSet => {
                let mut entry_row = div().flex().items_center().gap_2();
                if kind == RedisCreateKind::Hash {
                    entry_row = entry_row.child(div().flex_1().min_w_0().child(
                        self.render_host_redis_create_input(
                            WorkspaceImeTarget::HostRedisCreateKeyField,
                            HostToolsTextInput::RedisCreateKeyField,
                            field,
                            i18n.t("sidebar.host_redis.create_key.field_placeholder"),
                            cx,
                        ),
                    ));
                }
                entry_row = entry_row.child(div().flex_1().min_w_0().child(
                    self.render_host_redis_create_input(
                        WorkspaceImeTarget::HostRedisCreateKeyValue,
                        HostToolsTextInput::RedisCreateKeyValue,
                        value,
                        i18n.t("sidebar.host_redis.create_key.element_placeholder"),
                        cx,
                    ),
                ));
                entry_row = entry_row.child(
                    div()
                        .id("host-redis-create-add-entry")
                        .h(px(32.0))
                        .px_3()
                        .flex()
                        .items_center()
                        .rounded(px(self.tokens.radii.md))
                        .cursor_pointer()
                        .text_size(px(11.0))
                        .text_color(rgb(theme.text))
                        .bg(rgb(theme.bg_hover))
                        .hover(|button| button.bg(rgb(theme.bg_panel)))
                        .child(i18n.t("sidebar.host_redis.create_key.add_entry"))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _event, _window, cx| {
                                this.host_tools.update(cx, |host_tools, cx| {
                                    host_tools.add_redis_create_entry(cx);
                                });
                                cx.stop_propagation();
                            }),
                        ),
                );
                body = body.child(entry_row);
                body = body.child(
                    div()
                        .text_size(px(10.0))
                        .text_color(rgb(theme.text_muted))
                        .child(match kind {
                            RedisCreateKind::Hash => {
                                i18n.t("sidebar.host_redis.create_key.hash_hint")
                            }
                            RedisCreateKind::List => {
                                i18n.t("sidebar.host_redis.create_key.list_hint")
                            }
                            RedisCreateKind::Set => {
                                i18n.t("sidebar.host_redis.create_key.set_hint")
                            }
                            _ => i18n.t("sidebar.host_redis.create_key.zset_hint"),
                        }),
                );
                if entries.is_empty() {
                    body = body.child(
                        div()
                            .text_size(px(10.0))
                            .text_color(rgb(theme.text_muted))
                            .child(i18n.t("sidebar.host_redis.create_key.entries_empty")),
                    );
                } else {
                    let mut list = div()
                        .id("host-redis-create-entries")
                        .max_h(px(140.0))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_1();
                    for (index, entry) in entries.iter().enumerate() {
                        let display = entry.replace(
                            '\t',
                            if kind == RedisCreateKind::Hash {
                                " = "
                            } else {
                                " · "
                            },
                        );
                        list = list.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .px_2()
                                .py_1()
                                .rounded(px(self.tokens.radii.md))
                                .bg(rgb(theme.bg_hover))
                                .text_size(px(11.0))
                                .text_color(rgb(theme.text))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .font_family(settings_mono_font_family(
                                            self.settings_store.settings(),
                                        ))
                                        .child(display),
                                )
                                .child(
                                    div()
                                        .id(("host-redis-create-entry-remove", index))
                                        .flex_none()
                                        .cursor_pointer()
                                        .text_size(px(10.0))
                                        .text_color(rgb(theme.text_muted))
                                        .hover(|button| button.text_color(rgb(MONITOR_RED)))
                                        .child(i18n.t("sidebar.host_redis.create_key.remove_entry"))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |this, _event, _window, cx| {
                                                this.host_tools.update(cx, |host_tools, cx| {
                                                    host_tools.remove_redis_create_entry(index, cx);
                                                });
                                                cx.stop_propagation();
                                            }),
                                        ),
                                ),
                        );
                    }
                    body = body.child(list);
                }
            }
        }

        body = body.when_some(error, |body, error| {
            body.child(
                div()
                    .text_size(px(11.0))
                    .text_color(rgb(MONITOR_RED))
                    .child(error),
            )
        });

        let cancel_action = self.workspace_confirm_footer_action_button(
            i18n.t("sidebar.host_redis.create_key.cancel"),
            ButtonVariant::Secondary,
            ConfirmDialogAction::Cancel,
            false,
            None,
            |this, _event, _window, cx| {
                this.host_tools.update(cx, |host_tools, cx| {
                    host_tools.dismiss_redis_create_key(cx);
                });
                this.ime_marked_text = None;
                this.clear_ime_selection();
                cx.notify();
            },
            cx,
        );
        let submit_action = self.workspace_confirm_footer_action_button(
            i18n.t("sidebar.host_redis.create_key.confirm"),
            ButtonVariant::Default,
            ConfirmDialogAction::Confirm,
            loading,
            None,
            |this, _event, _window, cx| {
                this.host_tools.update(cx, |host_tools, cx| {
                    host_tools.submit_redis_create_key(cx);
                });
                cx.notify();
            },
            cx,
        );
        Some(
            oxideterm_gpui_ui::modal::dismissible_dialog_backdrop()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event, _window, cx| {
                        this.host_tools.update(cx, |host_tools, cx| {
                            host_tools.dismiss_redis_create_key(cx);
                        });
                        this.ime_marked_text = None;
                        this.clear_ime_selection();
                        cx.stop_propagation();
                    }),
                )
                .child(oxideterm_gpui_ui::modal::overlay_content_boundary(
                    oxideterm_gpui_ui::modal::dialog_content(&self.tokens)
                        .w(px(560.0))
                        .max_h(px(620.0))
                        .child(
                            div()
                                .px_4()
                                .py_3()
                                .border_b_1()
                                .border_color(rgb(theme.border))
                                .text_size(px(14.0))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgb(theme.text))
                                .child(i18n.t("sidebar.host_redis.create_key.title")),
                        )
                        .child(body)
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
}

impl HostToolsEntity {
    fn render_host_redis_value_dialog(
        &self,
        tokens: &ThemeTokens,
        i18n: &I18n,
        mono_font_family: SharedString,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.host_redis.value_dialog.as_ref()?;
        let theme = tokens.ui;
        let key = dialog.key.clone();
        let database = dialog.database;
        let database_label = format!("db{database}");
        let loading = dialog.loading;
        let error = dialog.error.clone();
        let json_view = dialog.json_view;

        let metadata = dialog.value.as_ref().map(|value| {
            let kind_label = match value.kind {
                ResourceRedisValueKind::String => "STRING",
                ResourceRedisValueKind::Hash => "HASH",
                ResourceRedisValueKind::List => "LIST",
                ResourceRedisValueKind::Set => "SET",
                ResourceRedisValueKind::SortedSet => "ZSET",
                ResourceRedisValueKind::Stream => "STREAM",
                ResourceRedisValueKind::Other => "OTHER",
            };
            let size = value
                .memory_usage
                .map(format_bytes)
                .or_else(|| value.length.map(|length| length.to_string()))
                .unwrap_or_else(|| "-".to_string());
            let ttl = match value.ttl {
                ResourceRedisTtl::Never => i18n.t("sidebar.host_redis.value.ttl_never"),
                ResourceRedisTtl::Missing => i18n.t("sidebar.host_redis.value.ttl_missing"),
                ResourceRedisTtl::Seconds(seconds) => i18n
                    .t("sidebar.host_redis.value.ttl_seconds")
                    .replace("{{seconds}}", &seconds.to_string()),
                ResourceRedisTtl::Unknown => "-".to_string(),
            };
            format!(
                "{database_label}  ·  {} {kind_label}  ·  {} {size}  ·  {} {ttl}",
                i18n.t("sidebar.host_redis.value.type"),
                i18n.t("sidebar.host_redis.value.size"),
                i18n.t("sidebar.host_redis.value.ttl"),
            )
        });

        let body = if loading {
            div()
                .p_4()
                .text_color(rgb(theme.text_muted))
                .child(i18n.t("sidebar.host_redis.value.loading"))
                .into_any_element()
        } else if let Some(error) = error {
            div()
                .p_4()
                .text_color(rgb(MONITOR_RED))
                .child(error)
                .into_any_element()
        } else if let Some(value) = dialog.value.as_ref() {
            let structured = matches!(
                value.kind,
                ResourceRedisValueKind::Stream | ResourceRedisValueKind::Other
            );
            if structured {
                div()
                    .p_4()
                    .text_color(rgb(theme.text_muted))
                    .child(
                        i18n.t("sidebar.host_redis.value.unsupported")
                            .replace("{{type}}", value.kind.label()),
                    )
                    .into_any_element()
            } else {
                let text = if json_view {
                    redis_value_json(value).unwrap_or_default()
                } else {
                    redis_value_text(value)
                };
                if text.is_empty() {
                    div()
                        .p_4()
                        .text_color(rgb(theme.text_muted))
                        .child(i18n.t("sidebar.host_redis.value.empty"))
                        .into_any_element()
                } else {
                    // Bound the per-frame line count; the dialog is a viewer,
                    // not a pager over a 512 KiB capture.
                    let mut lines = div()
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .font_family(mono_font_family.clone())
                        .text_size(px(11.0))
                        .text_color(rgb(theme.text));
                    for (index, line) in text.lines().take(HOST_REDIS_VALUE_MAX_LINES).enumerate() {
                        lines = lines.child(
                            div()
                                .id(("host-redis-value-line", index))
                                .flex_none()
                                .whitespace_nowrap()
                                .child(if line.is_empty() {
                                    " ".to_string()
                                } else {
                                    line.to_string()
                                }),
                        );
                    }
                    div()
                        .id("host-redis-value-scroll")
                        .max_h(px(HOST_REDIS_VALUE_DIALOG_MAX_HEIGHT - 150.0))
                        .overflow_y_scroll()
                        .overflow_x_scrollbar()
                        .child(lines)
                        .into_any_element()
                }
            }
        } else {
            div()
                .p_4()
                .text_color(rgb(theme.text_muted))
                .child(i18n.t("sidebar.host_redis.value.loading"))
                .into_any_element()
        };

        let copy_key = key.clone();
        let rename_key = key.clone();
        let set_value_key = key.clone();
        let delete_key = key.clone();
        // Only a string value can be replaced wholesale; collections need their
        // own per-element commands.
        let set_value_available = dialog
            .value
            .as_ref()
            .is_some_and(|value| value.kind == ResourceRedisValueKind::String);
        let refresh_disabled = loading;
        let json_available = dialog
            .value
            .as_ref()
            .is_some_and(|value| redis_value_json(value).is_some());

        Some(
            oxideterm_gpui_ui::modal::dismissible_dialog_backdrop()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|host_tools, _event, _window, cx| {
                        host_tools.dismiss_redis_value(cx);
                        cx.stop_propagation();
                    }),
                )
                .child(oxideterm_gpui_ui::modal::overlay_content_boundary(
                    oxideterm_gpui_ui::modal::dialog_content(tokens)
                        .w(px(HOST_REDIS_VALUE_DIALOG_WIDTH))
                        .max_h(px(HOST_REDIS_VALUE_DIALOG_MAX_HEIGHT))
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
                                                .min_w_0()
                                                .truncate()
                                                .font_family(mono_font_family.clone())
                                                .text_size(px(13.0))
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .text_color(rgb(theme.text))
                                                .child(key.clone()),
                                        )
                                        .child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .text_size(px(10.0))
                                                .text_color(rgb(theme.text_muted))
                                                .child(metadata.unwrap_or_default()),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .when(json_available, |row| {
                                            row.child(self.render_host_redis_view_toggle(
                                                json_view,
                                                tokens,
                                                i18n,
                                                cx,
                                            ))
                                        })
                                        .child(host_tools_tooltip_icon_button(
                                            tokens,
                                            LucideIcon::Copy,
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
                                            i18n.t("sidebar.host_redis.actions.copy_key"),
                                            "host-redis-value-copy-key",
                                            true,
                                            cx.listener(move |host_tools, _event, _window, cx| {
                                                host_tools.copy_redis_key(copy_key.clone(), cx);
                                                cx.stop_propagation();
                                            }),
                                        ))
                                        .child(host_tools_tooltip_icon_button(
                                            tokens,
                                            LucideIcon::PenLine,
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
                                            i18n.t("sidebar.host_redis.actions.rename"),
                                            "host-redis-value-rename",
                                            true,
                                            cx.listener(move |host_tools, _event, _window, cx| {
                                                host_tools.open_redis_rename(
                                                    database,
                                                    rename_key.clone(),
                                                    cx,
                                                );
                                                cx.stop_propagation();
                                            }),
                                        )).child(host_tools_tooltip_icon_button(
                                            tokens,
                                            LucideIcon::Pencil,
                                            13.0,
                                            rgb(theme.text),
                                            oxideterm_gpui_ui::button::IconButtonOptions {
                                                size: 24.0,
                                                disabled: !set_value_available,
                                                has_background: true,
                                                background: Some(rgb(theme.bg_hover)),
                                                hover_background: Some(rgb(theme.bg_panel)),
                                                idle_opacity: 1.0,
                                                ..oxideterm_gpui_ui::button::IconButtonOptions::compact(24.0)
                                            },
                                            if set_value_available {
                                                i18n.t("sidebar.host_redis.actions.set_value")
                                            } else {
                                                i18n.t("sidebar.host_redis.set_value.string_only")
                                            },
                                            "host-redis-value-set",
                                            true,
                                            cx.listener(move |host_tools, _event, _window, cx| {
                                                host_tools.open_redis_set_value(
                                                    database,
                                                    set_value_key.clone(),
                                                    json_view,
                                                    cx,
                                                );
                                                cx.stop_propagation();
                                            }),
                                        ))
                                        .child(host_tools_tooltip_icon_button(
                                            tokens,
                                            LucideIcon::Trash2,
                                            13.0,
                                            rgb(MONITOR_RED),
                                            oxideterm_gpui_ui::button::IconButtonOptions {
                                                size: 24.0,
                                                has_background: true,
                                                background: Some(rgb(theme.bg_hover)),
                                                hover_background: Some(rgb(theme.bg_panel)),
                                                idle_opacity: 1.0,
                                                ..oxideterm_gpui_ui::button::IconButtonOptions::compact(24.0)
                                            },
                                            i18n.t("sidebar.host_redis.actions.delete"),
                                            "host-redis-value-delete",
                                            true,
                                            cx.listener(move |host_tools, _event, _window, cx| {
                                                host_tools.request_redis_delete(
                                                    database,
                                                    delete_key.clone(),
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
                                                disabled: refresh_disabled,
                                                has_background: true,
                                                background: Some(rgb(theme.bg_hover)),
                                                hover_background: Some(rgb(theme.bg_panel)),
                                                idle_opacity: 1.0,
                                                ..oxideterm_gpui_ui::button::IconButtonOptions::compact(24.0)
                                            },
                                            i18n.t("sidebar.host_redis.actions.refresh"),
                                            "host-redis-value-refresh",
                                            true,
                                            cx.listener(|host_tools, _event, _window, cx| {
                                                host_tools.refresh_redis_value(cx);
                                                cx.stop_propagation();
                                            }),
                                        )),
                                ),
                        )
                        .child(body)
                        .child(
                            div()
                                .px_4()
                                .py_3()
                                .border_t_1()
                                .border_color(rgb(theme.border))
                                .flex()
                                .justify_end()
                                .child(
                                    div()
                                        .id("host-redis-value-close")
                                        .cursor_pointer()
                                        .px_3()
                                        .py_1()
                                        .rounded(px(tokens.radii.md))
                                        .hover(|button| button.bg(rgb(theme.bg_hover)))
                                        .text_size(px(11.0))
                                        .text_color(rgb(theme.text))
                                        .child(i18n.t("sidebar.host_redis.actions.close"))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|host_tools, _event, _window, cx| {
                                                host_tools.dismiss_redis_value(cx);
                                                cx.stop_propagation();
                                            }),
                                        ),
                                ),
                        ),
                ))
                .into_any_element(),
        )
    }

    fn render_host_redis_view_toggle(
        &self,
        json_view: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = tokens.ui;
        let mut row = div().flex_none().flex().items_center().gap_1().mr_1();
        for (json, label_key) in [
            (false, "sidebar.host_redis.value.view_text"),
            (true, "sidebar.host_redis.value.view_json"),
        ] {
            let active = json == json_view;
            row = row.child(
                div()
                    .id(("host-redis-view-toggle", usize::from(json)))
                    .px_2()
                    .h(px(22.0))
                    .flex()
                    .items_center()
                    .rounded(px(tokens.radii.md))
                    .cursor_pointer()
                    .text_size(px(10.0))
                    .when(active, |tab| {
                        tab.bg(rgb(theme.bg_panel)).text_color(rgb(theme.text))
                    })
                    .when(!active, |tab| tab.text_color(rgb(theme.text_muted)))
                    .hover(|tab| tab.bg(rgb(theme.bg_hover)))
                    .child(i18n.t(label_key))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |host_tools, _event, _window, cx| {
                            host_tools.set_redis_value_view(json, cx);
                            cx.stop_propagation();
                        }),
                    ),
            );
        }
        row.into_any_element()
    }

    fn render_host_redis_delete_dialog(
        &self,
        tokens: &ThemeTokens,
        i18n: &I18n,
        focused_action: Option<ConfirmDialogAction>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let confirm = self.host_redis.delete_confirm.as_ref()?;
        let key = confirm.key.clone();
        let description = i18n
            .t("sidebar.host_redis.delete.description")
            .replace("{{key}}", &key);
        Some(
            oxideterm_gpui_ui::confirm::confirm_dialog_with_focus_motion(
                tokens,
                "host-redis-delete-confirm",
                confirm.presence.phase(),
                ConfirmDialogView {
                    variant: ConfirmDialogVariant::Danger,
                    title: div()
                        .child(i18n.t("sidebar.host_redis.delete.title"))
                        .into_any_element(),
                    description: Some(div().child(description).into_any_element()),
                    cancel_label: div()
                        .child(i18n.t("sidebar.host_redis.delete.cancel"))
                        .into_any_element(),
                    confirm_label: div()
                        .child(i18n.t("sidebar.host_redis.delete.confirm"))
                        .into_any_element(),
                },
                focused_action,
                cx.listener(|host_tools, _event, _window, cx| {
                    host_tools.dismiss_redis_delete(cx);
                }),
                cx.listener(|host_tools, _event, _window, cx| {
                    host_tools.confirm_redis_delete(cx);
                }),
            )
            .into_any_element(),
        )
    }
}

/// Expansion paths address nodes by index instead of by label, so a key segment
/// that itself contains a separator cannot collide with a sibling level.
/// Flattens databases and their key trees into render rows.
///
/// Databases start expanded and deeper segments stay collapsed, so browsing a
/// large database shows its first-level prefixes instead of every key.
fn host_redis_tree_rows(
    databases: &[ResourceRedisDatabase],
    show_empty_databases: bool,
    collapsed_databases: &std::collections::BTreeSet<String>,
    expanded_segments: &std::collections::BTreeSet<String>,
) -> Vec<HostRedisTreeRow> {
    let mut rows = Vec::new();
    for database in databases {
        if !show_empty_databases && database.key_count == 0 && database.keys.is_empty() {
            continue;
        }
        let path = format!("db:{}", database.index);
        let expanded = !collapsed_databases.contains(&path);
        rows.push(HostRedisTreeRow {
            path,
            label: format!("db{}", database.index),
            depth: 0,
            key_count: usize::try_from(database.key_count).unwrap_or(usize::MAX),
            expandable: !database.keys.is_empty(),
            expanded,
            is_database: true,
            is_key: false,
            key: None,
            database: database.index,
            is_current_key: false,
        });
        if expanded {
            let tree = redis_key_tree(&database.keys);
            push_redis_tree_rows(
                &tree,
                &format!("db:{}:", database.index),
                database.index,
                1,
                expanded_segments,
                &mut rows,
            );
        }
    }
    rows
}

impl HostToolsEntity {
    fn redis_value_target(&self) -> Option<(String, ResourceRedisEndpoint)> {
        let connection_id = self
            .selected_connection_id_owned()
            .or_else(|| self.host_redis.snapshot_connection_id.clone())?;
        let endpoint = self.host_redis.keys_endpoint.clone()?;
        Some((connection_id, endpoint))
    }

    pub(in crate::workspace::connection_monitor) fn open_redis_value(
        &mut self,
        database: u32,
        key: String,
        cx: &mut Context<Self>,
    ) {
        let Some((connection_id, endpoint)) = self.redis_value_target() else {
            return;
        };
        self.host_redis.value_dialog = Some(HostRedisValueDialog {
            connection_id: connection_id.clone(),
            endpoint,
            database,
            key: key.clone(),
            value: None,
            loading: true,
            error: None,
            json_view: false,
        });
        self.request_redis_value(
            connection_id,
            database,
            key,
            HostRedisValuePurpose::Open,
            cx,
        );
    }

    pub(in crate::workspace::connection_monitor) fn refresh_redis_value(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = self.host_redis.value_dialog.as_mut() else {
            return;
        };
        dialog.loading = true;
        dialog.error = None;
        let connection_id = dialog.connection_id.clone();
        let database = dialog.database;
        let key = dialog.key.clone();
        self.request_redis_value(
            connection_id,
            database,
            key,
            HostRedisValuePurpose::Open,
            cx,
        );
    }

    pub(in crate::workspace) fn redis_value_dialog_open(&self) -> bool {
        self.host_redis.value_dialog.is_some()
    }

    pub(in crate::workspace) fn redis_delete_open(&self) -> bool {
        self.host_redis.delete_confirm.is_some()
    }

    pub(in crate::workspace) fn redis_rename_open(&self) -> bool {
        self.ui.host_redis_rename_dialog.is_some()
    }

    pub(in crate::workspace::connection_monitor) fn dismiss_redis_value(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.host_redis.value_dialog.take().is_some() {
            cx.notify();
        }
    }

    pub(in crate::workspace::connection_monitor) fn set_redis_value_view(
        &mut self,
        json_view: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(dialog) = self.host_redis.value_dialog.as_mut() {
            dialog.json_view = json_view;
            cx.notify();
        }
    }

    pub(in crate::workspace) fn redis_create_key_open(&self) -> bool {
        self.ui.host_redis_create_key_dialog.is_some()
    }

    pub(in crate::workspace::connection_monitor) fn open_redis_create_key(
        &mut self,
        database: u32,
        cx: &mut Context<Self>,
    ) {
        let Some((connection_id, endpoint)) = self.redis_value_target() else {
            return;
        };
        self.ui.host_redis_create_key_dialog = Some(HostRedisCreateKeyDialog {
            connection_id,
            endpoint,
            database,
            name: zeroize::Zeroizing::new(String::new()),
            kind: RedisCreateKind::String,
            ttl_never: true,
            ttl: zeroize::Zeroizing::new(String::new()),
            value: zeroize::Zeroizing::new(String::new()),
            field: zeroize::Zeroizing::new(String::new()),
            entries: Vec::new(),
            error: None,
            loading: false,
        });
        self.ui.focus_input(HostToolsTextInput::RedisCreateKeyName);
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn dismiss_redis_create_key(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.ui.host_redis_create_key_dialog.take().is_some() {
            self.ui.clear_input_focus();
            cx.notify();
        }
    }

    pub(in crate::workspace::connection_monitor) fn set_redis_create_kind(
        &mut self,
        kind: RedisCreateKind,
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = self.ui.host_redis_create_key_dialog.as_mut() else {
            return;
        };
        if dialog.kind == kind {
            return;
        }
        dialog.kind = kind;
        // Entries and pending inputs have a different shape per type.
        dialog.entries.clear();
        dialog.field = zeroize::Zeroizing::new(String::new());
        dialog.value = zeroize::Zeroizing::new(String::new());
        dialog.error = None;
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn toggle_redis_create_ttl(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if let Some(dialog) = self.ui.host_redis_create_key_dialog.as_mut() {
            dialog.ttl_never = !dialog.ttl_never;
            dialog.error = None;
            cx.notify();
        }
    }

    pub(in crate::workspace::connection_monitor) fn add_redis_create_entry(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let messages = self.messages.clone();
        let Some(dialog) = self.ui.host_redis_create_key_dialog.as_mut() else {
            return;
        };
        let entry = match dialog.kind {
            RedisCreateKind::Hash => {
                let field = dialog.field.trim().to_string();
                if field.is_empty() || field.contains(['\t', '\n']) {
                    dialog.error = messages
                        .as_ref()
                        .map(|messages| messages.redis_create_key_invalid_field.clone());
                    cx.notify();
                    return;
                }
                let value = dialog.value.trim_matches(['\n', '\r']).to_string();
                dialog.field = zeroize::Zeroizing::new(String::new());
                dialog.value = zeroize::Zeroizing::new(String::new());
                format!("{field}\t{value}")
            }
            RedisCreateKind::List | RedisCreateKind::Set | RedisCreateKind::SortedSet => {
                let value = dialog.value.trim().to_string();
                if value.is_empty() {
                    return;
                }
                dialog.value = zeroize::Zeroizing::new(String::new());
                if dialog.kind == RedisCreateKind::SortedSet {
                    // `member score`; a missing score defaults to 0.
                    match value.split_once([' ', '\t']) {
                        Some((member, score)) if score.trim().parse::<f64>().is_ok() => {
                            format!("{}\t{}", member.trim(), score.trim())
                        }
                        Some((member, _)) => format!("{}\t0", member.trim()),
                        None => format!("{value}\t0"),
                    }
                } else {
                    value
                }
            }
            RedisCreateKind::String | RedisCreateKind::Json => return,
        };
        dialog.entries.push(zeroize::Zeroizing::new(entry));
        dialog.error = None;
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn remove_redis_create_entry(
        &mut self,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(dialog) = self.ui.host_redis_create_key_dialog.as_mut()
            && index < dialog.entries.len()
        {
            dialog.entries.remove(index);
            cx.notify();
        }
    }

    pub(in crate::workspace::connection_monitor) fn submit_redis_create_key(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let messages = self.messages.clone();
        let Some(dialog) = self.ui.host_redis_create_key_dialog.as_mut() else {
            return;
        };
        if dialog.loading {
            return;
        }
        let error = |messages: &Option<HostToolsMessages>,
                     pick: fn(&HostToolsMessages) -> &String| {
            messages.as_ref().map(pick).cloned()
        };
        let name = dialog.name.trim().to_string();
        if name.is_empty() || name.contains(['\n', '\r']) {
            dialog.error = error(&messages, |messages| {
                &messages.redis_create_key_invalid_name
            });
            cx.notify();
            return;
        }
        let ttl_seconds = if dialog.ttl_never {
            None
        } else {
            match dialog.ttl.trim().parse::<u64>() {
                Ok(seconds) if seconds > 0 => Some(seconds),
                _ => {
                    dialog.error =
                        error(&messages, |messages| &messages.redis_create_key_invalid_ttl);
                    cx.notify();
                    return;
                }
            }
        };
        let body = match dialog.kind {
            RedisCreateKind::String => dialog.value.as_str().to_string(),
            RedisCreateKind::Json => {
                // A JSON key is validated before it is written.
                match serde_json::from_str::<serde_json::Value>(dialog.value.trim()) {
                    Ok(_) => dialog.value.trim().to_string(),
                    Err(parse_error) => {
                        let template = self
                            .messages
                            .as_ref()
                            .map(|messages| messages.redis_value_set_invalid_json.clone())
                            .unwrap_or_default();
                        if let Some(dialog) = self.ui.host_redis_create_key_dialog.as_mut() {
                            dialog.error =
                                Some(template.replace("{{error}}", &parse_error.to_string()));
                            cx.notify();
                        }
                        return;
                    }
                }
            }
            RedisCreateKind::Hash
            | RedisCreateKind::List
            | RedisCreateKind::Set
            | RedisCreateKind::SortedSet => {
                if dialog.entries.is_empty() {
                    dialog.error = error(&messages, |messages| {
                        &messages.redis_create_key_empty_entries
                    });
                    cx.notify();
                    return;
                }
                dialog
                    .entries
                    .iter()
                    .map(|entry| entry.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        };
        let request = HostRedisCreateKeyRequest {
            connection_id: dialog.connection_id.clone(),
            endpoint: dialog.endpoint.clone(),
            database: dialog.database,
            key: name,
            password: self.ui.host_redis_password.clone(),
            value: zeroize::Zeroizing::new(body),
            failure_fallback: messages
                .as_ref()
                .map(|messages| messages.redis_key_action_failed.clone())
                .unwrap_or_default(),
        };
        let kind = dialog.kind;
        dialog.loading = true;
        dialog.error = None;
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            return;
        };
        let os_type = self.connection_os_type(&request.connection_id);
        let command = os_type.as_deref().and_then(|os_type| {
            redis_create_key_command(&request.endpoint, os_type, kind, ttl_seconds)
        });
        let Some(command) = command else {
            if let Some(dialog) = self.ui.host_redis_create_key_dialog.as_mut() {
                dialog.loading = false;
                dialog.error = Some(request.failure_fallback.clone());
            }
            cx.notify();
            return;
        };
        self.host_redis.create_key_running = Some(request.clone());
        let spawned = self.spawn_redis_create_key_capture(
            command,
            request.clone(),
            HOST_REDIS_ACTION_TIMEOUT,
            HOST_REDIS_VALUE_MAX_OUTPUT_SIZE,
            runtime,
        );
        if !spawned {
            self.host_redis.create_key_running = None;
            if let Some(dialog) = self.ui.host_redis_create_key_dialog.as_mut() {
                dialog.loading = false;
                dialog.error = Some(request.failure_fallback.clone());
            }
        }
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn finish_host_redis_create_key(
        &mut self,
        delivery: HostRedisCreateKeyDelivery,
        cx: &mut Context<Self>,
    ) {
        let is_current = self
            .host_redis
            .create_key_running
            .as_ref()
            .is_some_and(|running| {
                running.connection_id == delivery.request.connection_id
                    && running.endpoint == delivery.request.endpoint
                    && running.database == delivery.request.database
                    && running.key == delivery.request.key
            });
        if !is_current {
            return;
        }
        self.host_redis.create_key_running = None;
        let exists_message = self
            .messages
            .as_ref()
            .map(|messages| messages.redis_create_key_exists.clone());
        let (succeeded, message) = match delivery.result {
            Ok(output) if output.exit_code.unwrap_or(0) == 0 => {
                let text = String::from_utf8_lossy(&output.stdout);
                match parse_redis_key_mutation(&text) {
                    ResourceRedisStatus::Available => (true, String::new()),
                    ResourceRedisStatus::Error { message } => (
                        false,
                        if message == REDIS_KEY_EXISTS_SENTINEL {
                            exists_message.clone().unwrap_or_else(|| message.clone())
                        } else {
                            message
                        },
                    ),
                    _ => (false, delivery.request.failure_fallback.clone()),
                }
            }
            _ => (false, delivery.request.failure_fallback.clone()),
        };
        if succeeded {
            self.ui.host_redis_create_key_dialog = None;
            self.ui.clear_input_focus();
            self.request_redis_keys_for_selected(cx);
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::RedisKeyCreated {
                    key: delivery.request.key.clone(),
                },
            ));
        } else if let Some(dialog) = self.ui.host_redis_create_key_dialog.as_mut() {
            dialog.loading = false;
            dialog.error = Some(message.clone());
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::RedisKeyActionFailed { message },
            ));
        }
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn open_redis_set_value(
        &mut self,
        database: u32,
        key: String,
        json_only: bool,
        cx: &mut Context<Self>,
    ) {
        let Some((connection_id, endpoint)) = self.redis_value_target() else {
            return;
        };
        // Prefill with what the user is looking at so an edit starts from the
        // current value instead of an empty field.
        let prefill = self
            .host_redis
            .value_dialog
            .as_ref()
            .filter(|dialog| dialog.database == database && dialog.key == key)
            .and_then(|dialog| dialog.value.as_ref())
            .map(|value| {
                if json_only {
                    redis_value_json(value).unwrap_or_default()
                } else {
                    redis_value_text(value)
                }
            })
            .unwrap_or_default();
        self.ui.host_redis_set_value_dialog = Some(HostRedisSetValueDialog {
            connection_id,
            endpoint,
            database,
            key,
            value: zeroize::Zeroizing::new(prefill),
            json_only,
            error: None,
            loading: false,
        });
        self.ui.focus_input(HostToolsTextInput::RedisSetValue);
        cx.notify();
    }

    pub(in crate::workspace) fn redis_set_value_open(&self) -> bool {
        self.ui.host_redis_set_value_dialog.is_some()
    }

    pub(in crate::workspace::connection_monitor) fn dismiss_redis_set_value(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.ui.host_redis_set_value_dialog.take().is_some() {
            self.ui.clear_input_focus();
            cx.notify();
        }
    }

    pub(in crate::workspace::connection_monitor) fn submit_redis_set_value(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let json_error_key = self
            .messages
            .as_ref()
            .map(|messages| messages.redis_value_set_invalid_json.clone())
            .unwrap_or_default();
        let Some(dialog) = self.ui.host_redis_set_value_dialog.as_mut() else {
            return;
        };
        if dialog.loading {
            return;
        }
        // JSON view constrains the payload: an unparsable document would be
        // stored verbatim and then fail to render as JSON.
        if dialog.json_only
            && let Err(error) = serde_json::from_str::<serde_json::Value>(&dialog.value)
        {
            dialog.error = Some(json_error_key.replace("{{error}}", &error.to_string()));
            cx.notify();
            return;
        }
        let request = HostRedisSetValueRequest {
            connection_id: dialog.connection_id.clone(),
            endpoint: dialog.endpoint.clone(),
            database: dialog.database,
            key: dialog.key.clone(),
            password: self.ui.host_redis_password.clone(),
            value: dialog.value.clone(),
            failure_fallback: self
                .messages
                .as_ref()
                .map(|messages| messages.redis_key_action_failed.clone())
                .unwrap_or_default(),
        };
        dialog.loading = true;
        dialog.error = None;
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            return;
        };
        let os_type = self.connection_os_type(&request.connection_id);
        let command = os_type
            .as_deref()
            .and_then(|os_type| redis_set_value_command(&request.endpoint, os_type));
        let Some(command) = command else {
            if let Some(dialog) = self.ui.host_redis_set_value_dialog.as_mut() {
                dialog.loading = false;
                dialog.error = Some(request.failure_fallback.clone());
            }
            cx.notify();
            return;
        };
        self.host_redis.set_value_running = Some(request.clone());
        let spawned = self.spawn_redis_set_value_capture(
            command,
            request.clone(),
            HOST_REDIS_ACTION_TIMEOUT,
            HOST_REDIS_VALUE_MAX_OUTPUT_SIZE,
            runtime,
        );
        if !spawned {
            self.host_redis.set_value_running = None;
            if let Some(dialog) = self.ui.host_redis_set_value_dialog.as_mut() {
                dialog.loading = false;
                dialog.error = Some(request.failure_fallback.clone());
            }
        }
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn finish_host_redis_set_value(
        &mut self,
        delivery: HostRedisSetValueDelivery,
        cx: &mut Context<Self>,
    ) {
        let is_current = self
            .host_redis
            .set_value_running
            .as_ref()
            .is_some_and(|running| {
                running.connection_id == delivery.request.connection_id
                    && running.endpoint == delivery.request.endpoint
                    && running.database == delivery.request.database
                    && running.key == delivery.request.key
            });
        if !is_current {
            return;
        }
        self.host_redis.set_value_running = None;
        let (succeeded, message) = match delivery.result {
            Ok(output) if output.exit_code.unwrap_or(0) == 0 => {
                let text = String::from_utf8_lossy(&output.stdout);
                match parse_redis_key_mutation(&text) {
                    ResourceRedisStatus::Available => (true, String::new()),
                    ResourceRedisStatus::Error { message } => (false, message),
                    _ => (false, delivery.request.failure_fallback.clone()),
                }
            }
            _ => (false, delivery.request.failure_fallback.clone()),
        };
        if succeeded {
            self.ui.host_redis_set_value_dialog = None;
            self.ui.clear_input_focus();
            if let Some(dialog) = self.host_redis.value_dialog.as_mut()
                && dialog.database == delivery.request.database
                && dialog.key == delivery.request.key
            {
                dialog.loading = true;
                dialog.error = None;
                self.refresh_redis_value(cx);
            }
            cx.emit(HostToolsEvent::ShowNotice(HostToolsNotice::RedisValueSet {
                key: delivery.request.key.clone(),
            }));
        } else if let Some(dialog) = self.ui.host_redis_set_value_dialog.as_mut() {
            dialog.loading = false;
            dialog.error = Some(message.clone());
            cx.emit(HostToolsEvent::ShowNotice(
                HostToolsNotice::RedisKeyActionFailed { message },
            ));
        }
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn copy_redis_key(
        &mut self,
        key: String,
        cx: &mut Context<Self>,
    ) {
        cx.write_to_clipboard(ClipboardItem::new_string(key.clone()));
        cx.emit(HostToolsEvent::ShowNotice(
            HostToolsNotice::RedisKeyCopied { key },
        ));
    }

    pub(in crate::workspace::connection_monitor) fn copy_redis_value(
        &mut self,
        database: u32,
        key: String,
        cx: &mut Context<Self>,
    ) {
        let Some((connection_id, _endpoint)) = self.redis_value_target() else {
            return;
        };
        self.request_redis_value(
            connection_id,
            database,
            key,
            HostRedisValuePurpose::Copy,
            cx,
        );
    }

    pub(in crate::workspace::connection_monitor) fn open_redis_rename(
        &mut self,
        database: u32,
        key: String,
        cx: &mut Context<Self>,
    ) {
        let Some((connection_id, endpoint)) = self.redis_value_target() else {
            return;
        };
        self.ui.host_redis_rename_dialog = Some(HostRedisRenameDialog {
            connection_id,
            endpoint,
            database,
            key,
            value: zeroize::Zeroizing::new(String::new()),
            error: None,
            loading: false,
        });
        self.ui.focus_input(HostToolsTextInput::RedisRename);
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn dismiss_redis_rename(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.ui.host_redis_rename_dialog.take().is_some() {
            self.ui.clear_input_focus();
            cx.notify();
        }
    }

    pub(in crate::workspace::connection_monitor) fn submit_redis_rename(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let invalid_message = self
            .messages
            .as_ref()
            .map(|messages| messages.redis_rename_invalid.clone())
            .unwrap_or_default();
        let Some(dialog) = self.ui.host_redis_rename_dialog.as_mut() else {
            return;
        };
        if dialog.loading {
            return;
        }
        let new_key = dialog.value.trim().to_string();
        if new_key.is_empty() || new_key == dialog.key {
            dialog.error = Some(invalid_message);
            cx.notify();
            return;
        }
        let connection_id = dialog.connection_id.clone();
        let database = dialog.database;
        let key = dialog.key.clone();
        dialog.loading = true;
        dialog.error = None;
        self.request_redis_mutation(
            connection_id,
            database,
            HostRedisMutationKind::Rename,
            key,
            Some(new_key),
            cx,
        );
    }

    pub(in crate::workspace::connection_monitor) fn request_redis_delete(
        &mut self,
        database: u32,
        key: String,
        cx: &mut Context<Self>,
    ) {
        let Some((connection_id, endpoint)) = self.redis_value_target() else {
            return;
        };
        self.host_redis.delete_confirm = Some(HostRedisDeleteConfirm {
            connection_id,
            endpoint,
            database,
            key,
            presence: oxideterm_gpui_ui::motion::ExitPresence::visible(),
        });
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn dismiss_redis_delete(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.host_redis.delete_confirm.take().is_some() {
            cx.notify();
        }
    }

    pub(in crate::workspace::connection_monitor) fn confirm_redis_delete(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let Some(confirm) = self.host_redis.delete_confirm.take() else {
            return;
        };
        self.ui.clear_input_focus();
        self.request_redis_mutation(
            confirm.connection_id,
            confirm.database,
            HostRedisMutationKind::Delete,
            confirm.key,
            None,
            cx,
        );
        cx.notify();
    }

    fn request_redis_value(
        &mut self,
        connection_id: String,
        database: u32,
        key: String,
        purpose: HostRedisValuePurpose,
        cx: &mut Context<Self>,
    ) {
        let Some(endpoint) = self.host_redis.keys_endpoint.clone() else {
            return;
        };
        let Some(messages) = self.messages.clone() else {
            return;
        };
        let request = HostRedisValueRequest {
            connection_id,
            endpoint,
            database,
            key,
            password: self.ui.host_redis_password.clone(),
            purpose,
            failure_fallback: messages.redis_probe_failed,
        };
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            return;
        };
        if self.host_redis.value_in_flight {
            self.host_redis.value_pending = Some(HostRedisValuePending { request, runtime });
            cx.notify();
            return;
        }
        self.start_redis_value(request, runtime, cx);
    }

    fn start_redis_value(
        &mut self,
        request: HostRedisValueRequest,
        runtime: tokio::runtime::Handle,
        cx: &mut Context<Self>,
    ) {
        let Some(os_type) = self.connection_os_type(&request.connection_id) else {
            self.host_redis.value_in_flight = false;
            if request.purpose == HostRedisValuePurpose::Open {
                self.fail_redis_value_dialog(&request.key, request.failure_fallback);
            }
            cx.notify();
            return;
        };
        let Some(command) = redis_value_command(&request.endpoint, &os_type) else {
            self.host_redis.value_in_flight = false;
            if request.purpose == HostRedisValuePurpose::Open {
                self.fail_redis_value_dialog(&request.key, request.failure_fallback);
            }
            cx.notify();
            return;
        };
        self.host_redis.value_running = Some(request.clone());
        self.host_redis.value_in_flight = true;
        let spawned = self.spawn_redis_value_capture(
            command,
            request.clone(),
            HOST_REDIS_VALUE_TIMEOUT,
            HOST_REDIS_VALUE_MAX_OUTPUT_SIZE,
            runtime,
        );
        if !spawned {
            self.host_redis.value_running = None;
            self.host_redis.value_in_flight = false;
            if request.purpose == HostRedisValuePurpose::Open {
                self.fail_redis_value_dialog(&request.key, request.failure_fallback);
            }
        }
        cx.notify();
    }

    fn fail_redis_value_dialog(&mut self, key: &str, message: String) {
        if let Some(dialog) = self.host_redis.value_dialog.as_mut()
            && dialog.key == key
        {
            dialog.loading = false;
            dialog.error = Some(message);
        }
    }

    pub(in crate::workspace::connection_monitor) fn finish_host_redis_value(
        &mut self,
        delivery: HostRedisValueDelivery,
        cx: &mut Context<Self>,
    ) {
        let is_current = self
            .host_redis
            .value_running
            .as_ref()
            .is_some_and(|running| running.same_target(&delivery.request));
        if !is_current {
            return;
        }
        self.host_redis.value_in_flight = false;
        self.host_redis.value_running = None;
        let value = match delivery.result {
            Ok(output) if output.exit_code.unwrap_or(0) == 0 => {
                let text = String::from_utf8_lossy(&output.stdout);
                Some(parse_redis_value(&text))
            }
            _ => None,
        };
        match delivery.request.purpose {
            HostRedisValuePurpose::Open => {
                let messages = self.messages.clone();
                if let Some(dialog) = self.host_redis.value_dialog.as_mut()
                    && dialog.key == delivery.request.key
                {
                    dialog.loading = false;
                    dialog.value = None;
                    dialog.error = None;
                    match value {
                        Some(value) => match &value.status {
                            ResourceRedisStatus::Available => {
                                // Types without a JSON shape fall back to the text view.
                                if dialog.json_view && redis_value_json(&value).is_none() {
                                    dialog.json_view = false;
                                }
                                dialog.value = Some(value);
                            }
                            ResourceRedisStatus::AuthRequired => {
                                dialog.error = messages
                                    .as_ref()
                                    .map(|messages| messages.redis_auth_required.clone());
                            }
                            ResourceRedisStatus::NoServer => {
                                dialog.error = messages
                                    .as_ref()
                                    .map(|messages| messages.redis_key_missing.clone());
                            }
                            ResourceRedisStatus::Error { message } => {
                                dialog.error = Some(message.clone());
                            }
                            _ => {
                                dialog.error = Some(delivery.request.failure_fallback.clone());
                            }
                        },
                        None => {
                            dialog.error = Some(delivery.request.failure_fallback.clone());
                        }
                    }
                }
            }
            HostRedisValuePurpose::Copy => {
                let Some(value) = value else {
                    cx.emit(HostToolsEvent::ShowNotice(
                        HostToolsNotice::RedisValueCopyFailed {
                            key: delivery.request.key,
                        },
                    ));
                    cx.notify();
                    return;
                };
                let rendered = redis_value_json(&value).unwrap_or_else(|| redis_value_text(&value));
                cx.write_to_clipboard(ClipboardItem::new_string(rendered));
                cx.emit(HostToolsEvent::ShowNotice(
                    HostToolsNotice::RedisValueCopied {
                        key: delivery.request.key,
                    },
                ));
            }
        }
        let pending = self.host_redis.value_pending.take();
        if let Some(pending) = pending {
            self.start_redis_value(pending.request, pending.runtime, cx);
        }
        cx.notify();
    }

    fn request_redis_mutation(
        &mut self,
        connection_id: String,
        database: u32,
        kind: HostRedisMutationKind,
        key: String,
        new_key: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(endpoint) = self.host_redis.keys_endpoint.clone() else {
            return;
        };
        let Some(messages) = self.messages.clone() else {
            return;
        };
        let request = HostRedisMutationRequest {
            connection_id,
            endpoint,
            database,
            kind,
            key,
            new_key,
            password: self.ui.host_redis_password.clone(),
            failure_fallback: messages.redis_key_action_failed,
        };
        let Some(runtime) = self.lifecycle_runtime.clone() else {
            return;
        };
        let os_type = self.connection_os_type(&request.connection_id);
        let command = os_type.as_deref().and_then(|os_type| {
            redis_key_mutation_command(
                &request.endpoint,
                os_type,
                match request.kind {
                    HostRedisMutationKind::Delete => RedisKeyMutation::Delete,
                    HostRedisMutationKind::Rename => RedisKeyMutation::Rename,
                },
            )
        });
        let Some(command) = command else {
            let message = request.failure_fallback.clone();
            self.finish_redis_mutation_ui(&request, false, message, cx);
            return;
        };
        self.host_redis.mutation_running = Some(request.clone());
        self.host_redis.value_in_flight = false;
        let spawned = self.spawn_redis_mutation_capture(
            command,
            request.clone(),
            HOST_REDIS_ACTION_TIMEOUT,
            HOST_REDIS_ACTION_MAX_OUTPUT_SIZE,
            runtime,
        );
        if !spawned {
            self.host_redis.mutation_running = None;
            let message = request.failure_fallback.clone();
            self.finish_redis_mutation_ui(&request, false, message, cx);
        }
        cx.notify();
    }

    pub(in crate::workspace::connection_monitor) fn finish_host_redis_mutation(
        &mut self,
        delivery: HostRedisMutationDelivery,
        cx: &mut Context<Self>,
    ) {
        let is_current = self
            .host_redis
            .mutation_running
            .as_ref()
            .is_some_and(|running| {
                running.connection_id == delivery.request.connection_id
                    && running.database == delivery.request.database
                    && running.kind == delivery.request.kind
                    && running.key == delivery.request.key
                    && running.new_key == delivery.request.new_key
            });
        if !is_current {
            return;
        }
        self.host_redis.mutation_running = None;
        let (succeeded, message) = match delivery.result {
            Ok(output) if output.exit_code.unwrap_or(0) == 0 => {
                let text = String::from_utf8_lossy(&output.stdout);
                match parse_redis_key_mutation(&text) {
                    ResourceRedisStatus::Available => (true, String::new()),
                    ResourceRedisStatus::Error { message } => (false, message),
                    _ => (false, delivery.request.failure_fallback.clone()),
                }
            }
            _ => (false, delivery.request.failure_fallback.clone()),
        };
        self.finish_redis_mutation_ui(&delivery.request, succeeded, message, cx);
    }

    fn finish_redis_mutation_ui(
        &mut self,
        request: &HostRedisMutationRequest,
        succeeded: bool,
        message: String,
        cx: &mut Context<Self>,
    ) {
        if succeeded {
            self.ui.host_redis_rename_dialog = None;
            self.ui.clear_input_focus();
            // The browsed keys changed shape: re-read the endpoint and refresh
            // an open value dialog for the renamed key.
            if request.kind == HostRedisMutationKind::Rename {
                if let Some(new_key) = request.new_key.clone() {
                    self.host_redis.value_dialog = None;
                    self.open_redis_value(request.database, new_key, cx);
                }
            } else {
                self.host_redis.value_dialog = None;
            }
            self.request_redis_keys_for_selected(cx);
        } else if let Some(dialog) = self.ui.host_redis_rename_dialog.as_mut() {
            dialog.loading = false;
            dialog.error = Some(message.clone());
        }
        cx.emit(HostToolsEvent::ShowNotice(
            match (request.kind, succeeded) {
                (HostRedisMutationKind::Delete, true) => HostToolsNotice::RedisKeyDeleted {
                    key: request.key.clone(),
                },
                (HostRedisMutationKind::Rename, true) => HostToolsNotice::RedisKeyRenamed {
                    key: request.new_key.clone().unwrap_or_default(),
                },
                _ => HostToolsNotice::RedisKeyActionFailed { message },
            },
        ));
        cx.notify();
    }
}

fn push_redis_tree_rows(
    nodes: &[ResourceRedisKeyNode],
    prefix: &str,
    database_index: u32,
    depth: usize,
    expanded_segments: &std::collections::BTreeSet<String>,
    rows: &mut Vec<HostRedisTreeRow>,
) {
    for (index, node) in nodes.iter().enumerate() {
        let path = format!("{prefix}{index}");
        let expandable = !node.children.is_empty();
        let expanded = expandable && expanded_segments.contains(&path);
        rows.push(HostRedisTreeRow {
            path: path.clone(),
            label: node.label.clone(),
            depth,
            key_count: node.key_count,
            expandable,
            expanded,
            is_database: false,
            is_key: node.is_key,
            key: node.is_key.then(|| node.path.clone()),
            database: database_index,
            is_current_key: false,
        });
        if expanded {
            // A prefix that is itself a key (`config` and `config:version`)
            // renders as its own leaf inside the prefix, otherwise the key's
            // value would be unreachable from the tree.
            if node.is_key {
                rows.push(HostRedisTreeRow {
                    path: format!("{path}:self"),
                    label: node.path.clone(),
                    depth: depth + 1,
                    key_count: 1,
                    expandable: false,
                    expanded: false,
                    is_database: false,
                    is_key: true,
                    key: Some(node.path.clone()),
                    database: database_index,
                    is_current_key: true,
                });
            }
            push_redis_tree_rows(
                &node.children,
                &format!("{path}:"),
                database_index,
                depth + 1,
                expanded_segments,
                rows,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database(index: u32, keys: &[&str]) -> ResourceRedisDatabase {
        ResourceRedisDatabase {
            index,
            key_count: keys.len() as u64,
            keys: keys.iter().map(|key| key.to_string()).collect(),
        }
    }

    fn rows(
        databases: &[ResourceRedisDatabase],
        show_empty_databases: bool,
        collapsed: &[&str],
        expanded: &[&str],
    ) -> Vec<String> {
        host_redis_tree_rows(
            databases,
            show_empty_databases,
            &collapsed.iter().map(|path| path.to_string()).collect(),
            &expanded.iter().map(|path| path.to_string()).collect(),
        )
        .into_iter()
        .map(|row| format!("{}{}", "  ".repeat(row.depth), row.label))
        .collect()
    }

    #[test]
    fn databases_start_expanded_and_segments_start_collapsed() {
        let databases = [
            database(0, &["user:1", "user:2", "session:9"]),
            database(3, &["cache:a"]),
        ];
        // Only first-level prefixes show until the user expands a segment.
        assert_eq!(
            rows(&databases, false, &[], &[]),
            vec!["db0", "  session", "  user", "db3", "  cache"]
        );
        // Expanding `user` (index 1 inside db0) reveals its children.
        assert_eq!(
            rows(&databases, false, &[], &["db:0:1"]),
            vec![
                "db0",
                "  session",
                "  user",
                "    1",
                "    2",
                "db3",
                "  cache"
            ]
        );
        // A collapsed database hides its whole subtree.
        assert_eq!(
            rows(&databases, false, &["db:0"], &["db:0:1"]),
            vec!["db0", "db3", "  cache"]
        );
    }

    #[test]
    fn empty_databases_are_hidden_unless_the_filter_shows_all() {
        let databases = [database(0, &["user:1"]), database(1, &[])];
        assert_eq!(rows(&databases, false, &[], &[]), vec!["db0", "  user"]);
        assert_eq!(
            rows(&databases, true, &[], &[]),
            vec!["db0", "  user", "db1"]
        );
    }

    #[test]
    fn a_prefix_that_is_also_a_key_lists_its_own_leaf() {
        // Reported case: `jsm-config` and `jsm-config:version` both exist.
        let databases = [database(
            0,
            &[
                "helphub:config:jsm-config",
                "helphub:config:jsm-config:version",
            ],
        )];
        let expanded = host_redis_tree_rows(
            &databases,
            false,
            &Default::default(),
            &[
                "db:0:0".to_string(),
                "db:0:0:0".to_string(),
                "db:0:0:0:0".to_string(),
            ]
            .into_iter()
            .collect(),
        );
        let labels = expanded
            .iter()
            .map(|row| (row.depth, row.label.as_str(), row.is_key))
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            vec![
                (0, "db0", false),
                (1, "helphub", false),
                (2, "config", false),
                (3, "jsm-config", true),
                // The key itself is its own leaf next to the deeper key.
                (4, "helphub:config:jsm-config", true),
                (4, "version", true),
            ]
        );
        // The prefix reports both keys, and expanding it shows both.
        let jsm = &expanded[3];
        assert_eq!(jsm.key_count, 2);
        assert_eq!(expanded[4..].len(), 2);
        // The prefix's own value renders as a `{current}` marker row, not as a
        // repeated full path; the real key name stays on the row for actions.
        assert!(expanded[4].is_current_key);
        assert_eq!(
            expanded[4].key.as_deref(),
            Some("helphub:config:jsm-config")
        );
        assert!(!expanded[5].is_current_key);
    }

    #[test]
    fn key_rows_carry_the_database_they_came_from() {
        // A key name is only unique inside one database; a row that lost its
        // index would read db0 and report "key no longer exists".
        let databases = [database(0, &["a"]), database(9, &["b:1"])];
        let rows = host_redis_tree_rows(
            &databases,
            false,
            &Default::default(),
            &["db:9:0".to_string()].into_iter().collect(),
        );
        let db0_key = rows
            .iter()
            .find(|row| row.key.as_deref() == Some("a"))
            .expect("db0 key row");
        assert_eq!(db0_key.database, 0);
        let db9_key = rows
            .iter()
            .find(|row| row.key.as_deref() == Some("b:1"))
            .expect("db9 key row");
        assert_eq!(db9_key.database, 9);
    }

    #[test]
    fn tree_rows_carry_subtree_counts_and_key_markers() {
        let databases = [database(0, &["user:1", "user:profile:2", "user"])];
        let rows = host_redis_tree_rows(
            &databases,
            false,
            &Default::default(),
            &["db:0:0".to_string()].into_iter().collect(),
        );
        assert_eq!(rows[0].key_count, 3);
        assert!(rows[0].is_database);
        // `user` is both a key and a parent, so it keeps its own leaf inside.
        assert_eq!(rows[1].label, "user");
        assert!(rows[1].is_key);
        assert_eq!(rows[1].key_count, 3);
        assert_eq!(
            rows.iter()
                .map(|row| row.label.as_str())
                .collect::<Vec<_>>(),
            vec!["db0", "user", "user", "1", "profile"]
        );
        assert!(rows[2].is_key && !rows[2].expandable);
        assert!(rows[2].is_current_key);
        assert!(!rows[1].is_current_key);
    }
}
