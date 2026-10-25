//! Owns the Redis Host Tool UI and its one-shot unauthenticated probe lifecycle.

use super::*;

impl WorkspaceApp {
    pub(super) fn render_host_redis_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens;
        let i18n = &self.i18n;
        let mono_font_family = settings_mono_font_family(self.settings_store.settings());
        let selectable_text = self.selectable_text_render_state(cx);
        self.host_tools.update(cx, |host_tools, cx| {
            host_tools.render_host_redis_panel(
                &tokens,
                i18n,
                mono_font_family,
                &selectable_text,
                cx,
            )
        })
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
            });
            cx.notify();
            return;
        };
        let command = redis_sample_command(&os_type).to_string();
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
                }
            }
            Err(()) => ResourceRedisSnapshot {
                status: ResourceRedisStatus::Error {
                    message: delivery.request.failure_fallback.clone(),
                },
            },
        };
        self.host_redis.snapshot_connection_id = Some(delivery.request.connection_id);
        self.host_redis.snapshot = Some(snapshot);
        let pending = self.host_redis.snapshot_pending.take();
        if let Some(pending) = pending {
            self.start_redis_snapshot(pending.request, pending.runtime, cx);
        }
        cx.notify();
    }

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
        let snapshot = (self.host_redis.snapshot_connection_id.as_deref()
            == Some(selected_connection_id.as_str()))
        .then(|| self.host_redis.snapshot.clone())
        .flatten();

        Some(HostRedisPanelSnapshot {
            connections,
            selected_connection_id,
            snapshot,
            in_flight: self.host_redis.snapshot_in_flight,
        })
    }

    fn render_host_redis_panel(
        &self,
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
        let status_row = self.render_host_redis_status_row(snapshot.in_flight, tokens, i18n, cx);
        let body = self.render_host_redis_status(
            snapshot
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.status.clone()),
            snapshot.in_flight,
            tokens,
            i18n,
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
                        mono_font_family,
                        selectable_text,
                        cx,
                    ))
                    .child(status_row),
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

    fn render_host_redis_status_row(
        &self,
        in_flight: bool,
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
                div()
                    .flex_none()
                    .child(i18n.t("sidebar.host_redis.probe_target")),
            )
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
                    host_tools.request_active_tool_snapshot(HostSnapshotFeedback::Toast, cx);
                    cx.stop_propagation();
                }),
            ))
            .into_any_element()
    }

    fn render_host_redis_status(
        &self,
        status: Option<ResourceRedisStatus>,
        in_flight: bool,
        tokens: &ThemeTokens,
        i18n: &I18n,
        selectable_text: &SelectableTextRenderState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if in_flight {
            return host_tools_center_state(
                LucideIcon::Database,
                tokens.ui.text_muted,
                i18n.t("sidebar.host_redis.loading"),
                selectable_text,
                cx,
            );
        }
        let Some(status) = status else {
            return host_tools_center_state(
                LucideIcon::Database,
                tokens.ui.text_muted,
                i18n.t("sidebar.host_redis.loading"),
                selectable_text,
                cx,
            );
        };
        match status {
            ResourceRedisStatus::Available => host_tools_center_state(
                LucideIcon::CheckCircle,
                tokens.ui.text_muted,
                i18n.t("sidebar.host_redis.reachable"),
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
                i18n.t("sidebar.host_redis.no_server"),
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
                    .replace("{{error}}", &message),
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
}

struct HostRedisPanelSnapshot {
    connections: Vec<MonitorConnectionOption>,
    selected_connection_id: String,
    snapshot: Option<ResourceRedisSnapshot>,
    in_flight: bool,
}
