use std::time::Duration;

use gpui::{Rgba, rgb, rgba};
use oxideterm_gpui_ui::motion::ExitPresence;
use oxideterm_ssh::{SshCommandOutput, SshSecretCommandOutput};
use oxideterm_topology::TopologyViewStatus;
use zeroize::Zeroize;

use super::*;

pub(super) const HOST_PROCESS_LIST_ESTIMATED_ROW_HEIGHT: f32 = 64.0;
pub(super) const HOST_GPU_LIST_ESTIMATED_ROW_HEIGHT: f32 = 72.0;
pub(super) const HOST_GPU_TABLE_HEADER_HEIGHT: f32 = 28.0;
pub(super) const HOST_GPU_UTILIZATION_COLUMN_WIDTH: f32 = 58.0;
pub(super) const HOST_GPU_MEMORY_COLUMN_WIDTH: f32 = 92.0;
pub(super) const HOST_PROCESS_TABLE_HEADER_HEIGHT: f32 = 28.0;
pub(super) const HOST_PROCESS_TABLE_MAIN_ROW_HEIGHT: f32 = 36.0;
pub(super) const HOST_PROCESS_USER_COLUMN_WIDTH: f32 = 64.0;
pub(super) const HOST_PROCESS_PID_COLUMN_WIDTH: f32 = 54.0;
pub(super) const HOST_PROCESS_CPU_COLUMN_WIDTH: f32 = 44.0;
pub(super) const HOST_PROCESS_MEMORY_COLUMN_WIDTH: f32 = 48.0;
pub(super) const HOST_PROCESS_SEPARATE_USER_COLUMN_MIN_WIDTH: f32 = 620.0;
pub(super) const HOST_PROCESS_TABLE_HEADER_TEXT_SIZE: f32 = 10.0;
pub(super) const HOST_PROCESS_TABLE_COMMAND_TEXT_SIZE: f32 = 12.0;
pub(super) const HOST_PROCESS_TABLE_META_TEXT_SIZE: f32 = 10.0;
pub(super) const HOST_PROCESS_TABLE_VALUE_TEXT_SIZE: f32 = 11.0;
pub(super) const HOST_PROCESS_DETAIL_TEXT_SIZE: f32 = 11.0;
pub(super) const HOST_PROCESS_ACTION_TIMEOUT: Duration = Duration::from_secs(8);
pub(super) const HOST_PROCESS_ACTION_MAX_OUTPUT_SIZE: usize = 4096;
pub(super) const HOST_DOCKER_LIST_ESTIMATED_ROW_HEIGHT: f32 = 64.0;
pub(super) const HOST_DOCKER_TABLE_HEADER_HEIGHT: f32 = 28.0;
pub(super) const HOST_DOCKER_TABLE_MAIN_ROW_HEIGHT: f32 = 36.0;
pub(super) const HOST_DOCKER_STATE_COLUMN_WIDTH: f32 = 72.0;
pub(super) const HOST_PODMAN_LIST_ESTIMATED_ROW_HEIGHT: f32 = 64.0;
pub(super) const HOST_PODMAN_TABLE_HEADER_HEIGHT: f32 = 28.0;
pub(super) const HOST_PODMAN_TABLE_MAIN_ROW_HEIGHT: f32 = 36.0;
pub(super) const HOST_PODMAN_STATE_COLUMN_WIDTH: f32 = 72.0;
pub(super) const HOST_PODMAN_PORTS_COLUMN_MIN_WIDTH: f32 = 92.0;
pub(super) const HOST_PODMAN_ACTION_TIMEOUT: Duration = Duration::from_secs(12);
pub(super) const HOST_PODMAN_ACTION_MAX_OUTPUT_SIZE: usize = 4096;
pub(super) const HOST_PODMAN_LOGS_TIMEOUT: Duration = Duration::from_secs(8);
pub(super) const HOST_PODMAN_LOGS_MAX_OUTPUT_SIZE: usize = 128 * 1024;
pub(super) const HOST_PODMAN_LOGS_DIALOG_WIDTH: f32 = 760.0;
pub(super) const HOST_PODMAN_LOGS_DIALOG_MAX_HEIGHT: f32 = 520.0;
// Inspect/search capture dialogs are user-resizable; the body height is the
// dragged dimension and the width follows the corner grip.
pub(super) const HOST_PODMAN_CAPTURE_DIALOG_DEFAULT_WIDTH: f32 = 760.0;
pub(super) const HOST_PODMAN_CAPTURE_DIALOG_DEFAULT_HEIGHT: f32 = 420.0;
pub(super) const HOST_PODMAN_CAPTURE_DIALOG_MIN_WIDTH: f32 = 420.0;
pub(super) const HOST_PODMAN_CAPTURE_DIALOG_MIN_HEIGHT: f32 = 200.0;
pub(super) const HOST_PODMAN_CAPTURE_DIALOG_MAX_WIDTH: f32 = 1200.0;
pub(super) const HOST_PODMAN_CAPTURE_DIALOG_MAX_HEIGHT: f32 = 800.0;
pub(super) const HOST_DOCKER_PORTS_COLUMN_MIN_WIDTH: f32 = 92.0;
pub(super) const HOST_DOCKER_ACTION_TIMEOUT: Duration = Duration::from_secs(12);
pub(super) const HOST_DOCKER_ACTION_MAX_OUTPUT_SIZE: usize = 4096;
pub(super) const HOST_DOCKER_LOGS_TIMEOUT: Duration = Duration::from_secs(8);
pub(super) const HOST_DOCKER_LOGS_MAX_OUTPUT_SIZE: usize = 128 * 1024;
pub(super) const HOST_DOCKER_LOGS_DIALOG_WIDTH: f32 = 760.0;
pub(super) const HOST_DOCKER_LOGS_DIALOG_MAX_HEIGHT: f32 = 520.0;
pub(super) const HOST_SERVICE_LIST_ESTIMATED_ROW_HEIGHT: f32 = 64.0;
pub(super) const HOST_SERVICE_TABLE_HEADER_HEIGHT: f32 = 28.0;
pub(super) const HOST_SERVICE_TABLE_MAIN_ROW_HEIGHT: f32 = 36.0;
pub(super) const HOST_SERVICE_STATE_COLUMN_WIDTH: f32 = 78.0;
pub(super) const HOST_SERVICE_ENABLED_COLUMN_WIDTH: f32 = 70.0;
pub(super) const HOST_SERVICE_PID_COLUMN_WIDTH: f32 = 54.0;
pub(super) const HOST_SERVICE_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(12);
pub(super) const HOST_SERVICE_SNAPSHOT_MAX_OUTPUT_SIZE: usize = 256 * 1024;
pub(super) const HOST_SERVICE_LOGS_TIMEOUT: Duration = Duration::from_secs(8);
pub(super) const HOST_SERVICE_ACTION_TIMEOUT: Duration = Duration::from_secs(15);
pub(super) const HOST_SERVICE_ACTION_MAX_OUTPUT_SIZE: usize = 4096;
pub(super) const HOST_SERVICE_LOGS_MAX_OUTPUT_SIZE: usize = 128 * 1024;
pub(super) const HOST_SERVICE_LOGS_DIALOG_WIDTH: f32 = 760.0;
// The PING probe self-bounds with `redis-cli -t 3`; the runner timeout only
// covers a hung connection setup on top of it.
pub(super) const HOST_REDIS_PROBE_TIMEOUT: Duration = Duration::from_secs(12);
pub(super) const HOST_REDIS_PROBE_MAX_OUTPUT_SIZE: usize = 64 * 1024;
// Browsing runs one bounded SCAN per database, so it needs room for several
// databases worth of keys and a longer runner timeout than the PING probe.
pub(super) const HOST_REDIS_KEYS_TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const HOST_REDIS_KEYS_MAX_OUTPUT_SIZE: usize = 512 * 1024;
pub(super) const HOST_REDIS_TREE_ROW_HEIGHT: f32 = 24.0;
pub(super) const HOST_REDIS_ENDPOINT_CHIP_HEIGHT: f32 = 26.0;
// A value read runs TYPE plus one bounded range/scan on top of the password
// handshake, so it needs more room than the key listing.
pub(super) const HOST_REDIS_VALUE_TIMEOUT: Duration = Duration::from_secs(20);
pub(super) const HOST_REDIS_VALUE_MAX_OUTPUT_SIZE: usize = 512 * 1024;
pub(super) const HOST_REDIS_ACTION_TIMEOUT: Duration = Duration::from_secs(12);
pub(super) const HOST_REDIS_ACTION_MAX_OUTPUT_SIZE: usize = 4096;
pub(super) const HOST_REDIS_VALUE_DIALOG_WIDTH: f32 = 720.0;
// String/JSON values are longer than a key name; the editor gets room to show
// the document instead of a single 32px line.
pub(super) const HOST_REDIS_VALUE_INPUT_HEIGHT: f32 = 96.0;
pub(super) const HOST_REDIS_VALUE_DIALOG_MAX_HEIGHT: f32 = 560.0;
// The dialog is a viewer over a bounded capture; per-frame line output stops
// here instead of building thousands of GPUI elements.
pub(super) const HOST_REDIS_VALUE_MAX_LINES: usize = 400;
pub(super) const HOST_SERVICE_LOGS_DIALOG_MAX_HEIGHT: f32 = 520.0;
pub(super) const HOST_LOG_LIST_ESTIMATED_ROW_HEIGHT: f32 = 56.0;
pub(super) const HOST_LOG_TABLE_HEADER_HEIGHT: f32 = 28.0;
pub(super) const HOST_LOG_TIME_COLUMN_WIDTH: f32 = 92.0;
pub(super) const HOST_LOG_LEVEL_COLUMN_WIDTH: f32 = 58.0;
pub(super) const HOST_LOG_SOURCE_COLUMN_WIDTH: f32 = 96.0;
pub(super) const HOST_LOG_UNIT_COLUMN_WIDTH: f32 = 96.0;
pub(super) const HOST_LOG_CONTEXT_COLUMNS_MIN_WIDTH: f32 = 680.0;
pub(super) const HOST_LOG_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const HOST_LOG_SNAPSHOT_LIMIT: usize = 300;
pub(super) const HOST_LOG_SNAPSHOT_MAX_OUTPUT_SIZE: usize = 256 * 1024;
pub(super) const HOST_TMUX_LIST_ESTIMATED_ROW_HEIGHT: f32 = 64.0;
pub(super) const HOST_TMUX_TABLE_HEADER_HEIGHT: f32 = 28.0;
pub(super) const HOST_TMUX_TABLE_MAIN_ROW_HEIGHT: f32 = 36.0;
pub(super) const HOST_TMUX_ATTACHED_COLUMN_WIDTH: f32 = 74.0;
pub(super) const HOST_TMUX_WINDOWS_COLUMN_WIDTH: f32 = 58.0;
pub(super) const HOST_TMUX_PANES_COLUMN_WIDTH: f32 = 48.0;
pub(super) const HOST_TMUX_ACTIVITY_COLUMN_WIDTH: f32 = 92.0;
pub(super) const HOST_TMUX_CONTEXT_COLUMNS_MIN_WIDTH: f32 = 620.0;
pub(super) const HOST_TMUX_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(8);
pub(super) const HOST_TMUX_SNAPSHOT_MAX_OUTPUT_SIZE: usize = 128 * 1024;
pub(super) const HOST_TMUX_ACTION_TIMEOUT: Duration = Duration::from_secs(8);
pub(super) const HOST_TMUX_ACTION_MAX_OUTPUT_SIZE: usize = 4096;
pub(super) const HOST_TMUX_INPUT_DIALOG_WIDTH: f32 = 460.0;
pub(super) const HOST_PORT_LIST_ESTIMATED_ROW_HEIGHT: f32 = 64.0;
pub(super) const HOST_PORT_TABLE_HEADER_HEIGHT: f32 = 28.0;
pub(super) const HOST_PORT_TABLE_MAIN_ROW_HEIGHT: f32 = 36.0;
pub(super) const HOST_PORT_PROTOCOL_COLUMN_WIDTH: f32 = 46.0;
pub(super) const HOST_PORT_STATE_COLUMN_WIDTH: f32 = 78.0;
pub(super) const HOST_PORT_PID_COLUMN_WIDTH: f32 = 58.0;
pub(super) const HOST_PORT_PROCESS_COLUMN_WIDTH: f32 = 96.0;
pub(super) const HOST_PORT_REMOTE_COLUMN_WIDTH: f32 = 132.0;
pub(super) const HOST_PORT_CONTEXT_COLUMNS_MIN_WIDTH: f32 = 680.0;
pub(super) const HOST_PORT_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(8);
pub(super) const HOST_PORT_SNAPSHOT_MAX_OUTPUT_SIZE: usize = 256 * 1024;
pub(super) const HOST_SCHEDULE_LIST_ESTIMATED_ROW_HEIGHT: f32 = 64.0;
pub(super) const HOST_SCHEDULE_TABLE_HEADER_HEIGHT: f32 = 28.0;
pub(super) const HOST_SCHEDULE_TABLE_MAIN_ROW_HEIGHT: f32 = 36.0;
pub(super) const HOST_SCHEDULE_SOURCE_COLUMN_WIDTH: f32 = 74.0;
pub(super) const HOST_SCHEDULE_STATE_COLUMN_WIDTH: f32 = 78.0;
pub(super) const HOST_SCHEDULE_ENABLED_COLUMN_WIDTH: f32 = 72.0;
pub(super) const HOST_SCHEDULE_NEXT_COLUMN_WIDTH: f32 = 112.0;
pub(super) const HOST_SCHEDULE_LAST_COLUMN_WIDTH: f32 = 112.0;
pub(super) const HOST_SCHEDULE_CONTEXT_COLUMNS_MIN_WIDTH: f32 = 720.0;
pub(super) const HOST_SCHEDULE_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const HOST_SCHEDULE_SNAPSHOT_MAX_OUTPUT_SIZE: usize = 256 * 1024;
pub(super) const HOST_SCHEDULE_ACTION_TIMEOUT: Duration = Duration::from_secs(12);
pub(super) const HOST_SCHEDULE_ACTION_MAX_OUTPUT_SIZE: usize = 4096;
pub(super) const HOST_SCHEDULE_LOGS_TIMEOUT: Duration = Duration::from_secs(8);
pub(super) const HOST_SCHEDULE_LOGS_MAX_OUTPUT_SIZE: usize = 128 * 1024;
pub(super) const HOST_SCHEDULE_LOGS_DIALOG_WIDTH: f32 = 760.0;
pub(super) const HOST_SCHEDULE_LOGS_DIALOG_MAX_HEIGHT: f32 = 520.0;
pub(super) const HOST_FILESYSTEM_LIST_ESTIMATED_ROW_HEIGHT: f32 = 64.0;
pub(super) const HOST_FILESYSTEM_TABLE_HEADER_HEIGHT: f32 = 28.0;
pub(super) const HOST_FILESYSTEM_TABLE_MAIN_ROW_HEIGHT: f32 = 36.0;
pub(super) const HOST_FILESYSTEM_KIND_COLUMN_WIDTH: f32 = 74.0;
pub(super) const HOST_FILESYSTEM_USAGE_COLUMN_WIDTH: f32 = 70.0;
pub(super) const HOST_FILESYSTEM_INODE_COLUMN_WIDTH: f32 = 64.0;
pub(super) const HOST_FILESYSTEM_FS_COLUMN_WIDTH: f32 = 74.0;
pub(super) const HOST_FILESYSTEM_RO_COLUMN_WIDTH: f32 = 48.0;
pub(super) const HOST_FILESYSTEM_SIZE_COLUMN_WIDTH: f32 = 104.0;
pub(super) const HOST_FILESYSTEM_CONTEXT_COLUMNS_MIN_WIDTH: f32 = 720.0;
pub(super) const HOST_FILESYSTEM_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(15);
pub(super) const HOST_FILESYSTEM_SNAPSHOT_MAX_OUTPUT_SIZE: usize = 512 * 1024;
pub(super) const HOST_PACKAGE_LIST_ESTIMATED_ROW_HEIGHT: f32 = 64.0;
pub(super) const HOST_PACKAGE_TABLE_HEADER_HEIGHT: f32 = 28.0;
pub(super) const HOST_PACKAGE_TABLE_MAIN_ROW_HEIGHT: f32 = 36.0;
pub(super) const HOST_PACKAGE_STATUS_COLUMN_WIDTH: f32 = 84.0;
pub(super) const HOST_PACKAGE_VERSION_COLUMN_WIDTH: f32 = 116.0;
pub(super) const HOST_PACKAGE_MANAGER_COLUMN_WIDTH: f32 = 66.0;
pub(super) const HOST_PACKAGE_SERVICE_COLUMN_WIDTH: f32 = 108.0;
pub(super) const HOST_PACKAGE_CONTEXT_COLUMNS_MIN_WIDTH: f32 = 720.0;
pub(super) const HOST_PACKAGE_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(18);
pub(super) const HOST_PACKAGE_SNAPSHOT_MAX_OUTPUT_SIZE: usize = 512 * 1024;

pub(super) const MONITOR_POOL_REFRESH_INTERVAL: Duration = Duration::from_millis(2000);
// The compact sidebar must stay on GPUI List scrolling; ordinary Div overflow
// repaints too much of the Host Tools panel during trackpad scrolling.
pub(super) const COMPACT_MONITOR_LIST_ESTIMATED_ROW_HEIGHT: f32 = 34.0;
pub(super) const COMPACT_MONITOR_LIST_OVERSCAN: usize = 8;
pub(super) const COMPACT_MONITOR_METRIC_ROW_HEIGHT: f32 = 32.0;
// Network rows use the extra height only when their values must stack.
pub(super) const COMPACT_MONITOR_STACKED_ROW_HEIGHT: f32 = 52.0;
pub(super) const COMPACT_MONITOR_SECTION_ROW_HEIGHT: f32 = 32.0;
pub(super) const COMPACT_MONITOR_DETAIL_ROW_HEIGHT: f32 = 28.0;
pub(super) const COMPACT_MONITOR_RETRY_ROW_HEIGHT: f32 = 44.0;
// Compact monitoring rows use the same space-efficient inset at every width.
pub(super) const COMPACT_MONITOR_ROW_SIDE_PADDING: f32 = 12.0;
pub(super) const COMPACT_MONITOR_STACKED_LAYOUT_MAX_WIDTH: f32 = 360.0;
pub(super) const COMPACT_MONITOR_VALUE_MAX_WIDTH_RATIO: f32 = 0.58;
pub(super) const COMPACT_MONITOR_DETAIL_VALUE_MAX_WIDTH_RATIO: f32 = 0.55;
pub(super) const COMPACT_MONITOR_DETAIL_INDENT: f32 = 22.0;
pub(super) const MONITOR_BORDER_ALPHA: u32 = 0x80;
pub(super) const MONITOR_TINT_ALPHA: u32 = 0x1a;
pub(super) const MONITOR_EMERALD: u32 = 0x34d399;
pub(super) const MONITOR_EMERALD_DARK: u32 = 0x10b981;
pub(super) const MONITOR_AMBER: u32 = 0xf59e0b;
pub(super) const MONITOR_RED: u32 = 0xef4444;
pub(super) const MONITOR_BLUE: u32 = 0x3b82f6;
pub(super) const TOPOLOGY_BG_GRID_STEP: f32 = 40.0;
pub(super) const TOPOLOGY_BG_GRID_ALPHA: u32 = 0x1a;
pub(super) const TOPOLOGY_PANEL_BG_ALPHA_20: u32 = 0x33;
pub(super) const TOPOLOGY_PANEL_BORDER_ALPHA_50: u32 = 0x80;
pub(super) const TOPOLOGY_MUTED_TEXT_ALPHA_70: u32 = 0xb3;
pub(super) const TOPOLOGY_INSTRUCTION_ALPHA_60: u32 = 0x99;
pub(super) const TOPOLOGY_LINE_INACTIVE_ALPHA: u32 = 0x66;
pub(super) const TOPOLOGY_LINE_GLOW_ALPHA: u32 = 0x26;
pub(super) const TOPOLOGY_CONNECTED: u32 = 0x22c55e;
pub(super) const TOPOLOGY_CONNECTING: u32 = 0xeab308;
pub(super) const TOPOLOGY_FAILED: u32 = 0xef4444;
pub(super) const TOPOLOGY_DISCONNECTED: u32 = 0x71717a;
pub(super) const TOPOLOGY_PENDING: u32 = 0xf59e0b;
pub(super) const TOPOLOGY_ZOOM_INITIAL: f32 = 0.9;
pub(super) const TOPOLOGY_ZOOM_MIN: f32 = 0.3;
pub(super) const TOPOLOGY_ZOOM_MAX: f32 = 3.0;
pub(super) const TOPOLOGY_PAN_INITIAL_X: f32 = 0.0;
pub(super) const TOPOLOGY_PAN_INITIAL_Y: f32 = 50.0;
pub(super) const TOPOLOGY_MENU_WIDTH: f32 = 180.0;
pub(super) const TOPOLOGY_MENU_MAX_HEIGHT: f32 = 250.0;

pub(super) fn connection_monitor_surface_bg(theme_bg: u32, has_background: bool) -> Rgba {
    if has_background {
        rgba(0x00000000)
    } else {
        rgb(theme_bg)
    }
}

#[derive(Clone, Copy)]
pub(super) struct TopologyTransform {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) k: f32,
}

impl Default for TopologyTransform {
    fn default() -> Self {
        Self {
            x: TOPOLOGY_PAN_INITIAL_X,
            y: TOPOLOGY_PAN_INITIAL_Y,
            k: TOPOLOGY_ZOOM_INITIAL,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct TopologyDragState {
    pub(super) last_x: f32,
    pub(super) last_y: f32,
}

#[derive(Clone)]
pub(super) struct TopologyNodeMenuState {
    pub(super) node_id: Option<NodeId>,
    pub(super) name: String,
    pub(super) host: String,
    pub(super) view_status: TopologyViewStatus,
    pub(super) x: f32,
    pub(super) y: f32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MonitorConnectionOption {
    // Sidebar monitoring only needs selector/header fields; avoid cloning the
    // full registry connection payload on every scroll-driven render.
    pub(super) connection_id: String,
    pub(super) host: String,
    pub(super) port: u16,
    pub(super) username: String,
}

impl MonitorConnectionOption {
    pub(super) fn from_connection_info(connection: oxideterm_ssh::ConnectionInfo) -> Self {
        Self {
            connection_id: connection.connection_id,
            host: connection.host,
            port: connection.port,
            username: connection.username,
        }
    }

    pub(super) fn from_pool_summary(summary: &ConnectionPoolEntrySummary) -> Self {
        Self {
            connection_id: summary.id.clone(),
            host: summary.host.clone(),
            port: summary.port,
            username: summary.username.clone(),
        }
    }
}

#[derive(Clone)]
pub(super) struct HostProcessActionRequest {
    pub(super) connection_id: String,
    pub(super) pid: String,
    // Process names can contain credential-like command fragments. All UI
    // clones share one buffer that is cleared when the confirmation closes.
    pub(super) display_command: Arc<zeroize::Zeroizing<String>>,
    pub(super) action: ProcessActionKind,
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct HostProcessActionRun {
    pub(super) connection_id: String,
    pub(super) pid: String,
    pub(super) action: ProcessActionKind,
}

pub(super) struct HostProcessActionDelivery {
    pub(super) request: HostProcessActionRun,
    pub(super) result: Result<bool, ()>,
}

pub(super) struct HostProcessActionsState {
    pub(super) pending_confirm: Option<HostToolConfirmState<HostProcessActionRequest>>,
    pub(super) running: Option<HostProcessActionRun>,
}

impl HostProcessActionsState {
    pub(super) fn new() -> Self {
        Self {
            pending_confirm: None,
            running: None,
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct HostDockerActionRequest {
    pub(super) connection_id: String,
    pub(super) container_id: String,
    pub(super) container_name: String,
    pub(super) action: DockerActionKind,
}

pub(super) struct HostDockerActionDelivery {
    pub(super) request: HostDockerActionRequest,
    pub(super) result: Result<bool, ()>,
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct HostDockerLogsRequest {
    pub(super) connection_id: String,
    pub(super) container_id: String,
    pub(super) container_name: String,
    pub(super) failure_fallback: String,
    pub(super) empty_fallback: String,
}

pub(super) struct HostDockerLogsDelivery {
    pub(super) request: HostDockerLogsRequest,
    pub(super) result: Result<SshCommandOutput, ()>,
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct HostDockerLogsDialog {
    pub(super) request: HostDockerLogsRequest,
    // Docker output stays in one shared zeroizing buffer while rendered.
    pub(super) output: Option<Arc<zeroize::Zeroizing<String>>>,
    pub(super) error: Option<String>,
    pub(super) loading: bool,
}

pub(super) struct HostDockerOperationsState {
    pub(super) pending_confirm: Option<HostToolConfirmState<HostDockerActionRequest>>,
    pub(super) action_running: Option<HostDockerActionRequest>,
    pub(super) logs_dialog: Option<HostDockerLogsDialog>,
}

impl HostDockerOperationsState {
    pub(super) fn new() -> Self {
        Self {
            pending_confirm: None,
            action_running: None,
            logs_dialog: None,
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct HostServiceSnapshotRequest {
    pub(super) connection_id: String,
    pub(super) connection_fallback: String,
    pub(super) failure_fallback: String,
}

pub(super) struct HostServiceSnapshotDelivery {
    pub(super) request: HostServiceSnapshotRequest,
    pub(super) result: Result<SshCommandOutput, ()>,
}

pub(super) struct HostServiceSnapshotPending {
    pub(super) request: HostServiceSnapshotRequest,
    pub(super) runtime: tokio::runtime::Handle,
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct HostServiceActionRequest {
    pub(super) connection_id: String,
    pub(super) service_id: String,
    pub(super) description: String,
    pub(super) action: ServiceActionKind,
}

pub(super) struct HostServiceActionDelivery {
    pub(super) request: HostServiceActionRequest,
    pub(super) result: Result<bool, ()>,
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct HostServiceLogsRequest {
    pub(super) connection_id: String,
    pub(super) service_id: String,
    pub(super) description: String,
    pub(super) failure_fallback: String,
    pub(super) empty_fallback: String,
}

pub(super) struct HostServiceLogsDelivery {
    pub(super) request: HostServiceLogsRequest,
    pub(super) result: Result<SshCommandOutput, ()>,
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct HostServiceLogsDialog {
    pub(super) request: HostServiceLogsRequest,
    // Service output stays in one shared zeroizing buffer while rendered.
    pub(super) output: Option<Arc<zeroize::Zeroizing<String>>>,
    pub(super) error: Option<String>,
    pub(super) loading: bool,
}

pub(super) struct HostServicesState {
    pub(super) snapshot_connection_id: Option<String>,
    pub(super) snapshot: Option<oxideterm_connection_monitor::ResourceServiceSnapshot>,
    pub(super) snapshot_running: Option<HostServiceSnapshotRequest>,
    pub(super) snapshot_pending: Option<HostServiceSnapshotPending>,
    pub(super) snapshot_in_flight: bool,
    pub(super) pending_confirm: Option<HostToolConfirmState<HostServiceActionRequest>>,
    pub(super) action_running: Option<HostServiceActionRequest>,
    pub(super) logs_dialog: Option<HostServiceLogsDialog>,
}

impl HostServicesState {
    pub(super) fn new() -> Self {
        Self {
            snapshot_connection_id: None,
            snapshot: None,
            snapshot_running: None,
            snapshot_pending: None,
            snapshot_in_flight: false,
            pending_confirm: None,
            action_running: None,
            logs_dialog: None,
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct HostRedisSnapshotRequest {
    pub(super) connection_id: String,
    pub(super) connection_fallback: String,
    pub(super) failure_fallback: String,
}

pub(super) struct HostRedisSnapshotDelivery {
    pub(super) request: HostRedisSnapshotRequest,
    pub(super) result: Result<SshCommandOutput, ()>,
}

pub(super) struct HostRedisSnapshotPending {
    pub(super) request: HostRedisSnapshotRequest,
    pub(super) runtime: tokio::runtime::Handle,
}

/// One key-browsing request for a single endpoint.
///
/// The password lives only in this request and in the focused input buffer: it
/// is never formatted, audited, or written to a snapshot.
#[derive(Clone)]
pub(super) struct HostRedisKeysRequest {
    pub(super) connection_id: String,
    pub(super) endpoint: oxideterm_connection_monitor::ResourceRedisEndpoint,
    pub(super) password: zeroize::Zeroizing<String>,
    pub(super) failure_fallback: String,
}

impl HostRedisKeysRequest {
    /// Request identity ignores the credential so a re-typed password for the
    /// same endpoint still replaces the in-flight result.
    pub(super) fn same_target(&self, other: &Self) -> bool {
        self.connection_id == other.connection_id && self.endpoint == other.endpoint
    }
}

pub(super) struct HostRedisKeysDelivery {
    pub(super) request: HostRedisKeysRequest,
    pub(super) result: Result<SshSecretCommandOutput, ()>,
}

pub(super) struct HostRedisKeysPending {
    pub(super) request: HostRedisKeysRequest,
    pub(super) runtime: tokio::runtime::Handle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HostRedisValuePurpose {
    /// The dialog is open and renders the value.
    Open,
    /// The row menu copies the value without opening the dialog.
    Copy,
}

#[derive(Clone)]
pub(super) struct HostRedisValueRequest {
    pub(super) connection_id: String,
    pub(super) endpoint: oxideterm_connection_monitor::ResourceRedisEndpoint,
    /// Database the key lives in; a key name alone is ambiguous across dbs.
    pub(super) database: u32,
    pub(super) key: String,
    pub(super) password: zeroize::Zeroizing<String>,
    pub(super) purpose: HostRedisValuePurpose,
    pub(super) failure_fallback: String,
}

impl HostRedisValueRequest {
    pub(super) fn same_target(&self, other: &Self) -> bool {
        self.connection_id == other.connection_id
            && self.endpoint == other.endpoint
            && self.database == other.database
            && self.key == other.key
            && self.purpose == other.purpose
    }
}

pub(super) struct HostRedisValueDelivery {
    pub(super) request: HostRedisValueRequest,
    pub(super) result: Result<SshSecretCommandOutput, ()>,
}

pub(super) struct HostRedisValuePending {
    pub(super) request: HostRedisValueRequest,
    pub(super) runtime: tokio::runtime::Handle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HostRedisMutationKind {
    Delete,
    /// Deletes every key in `keys` in one exec (prefix deletion).
    DeleteMany,
    Rename,
}

#[derive(Clone)]
pub(super) struct HostRedisMutationRequest {
    pub(super) connection_id: String,
    pub(super) endpoint: oxideterm_connection_monitor::ResourceRedisEndpoint,
    pub(super) database: u32,
    pub(super) kind: HostRedisMutationKind,
    pub(super) key: String,
    /// Targets for `DeleteMany`; single mutations leave this empty.
    pub(super) keys: Vec<String>,
    pub(super) new_key: Option<String>,
    pub(super) password: zeroize::Zeroizing<String>,
    pub(super) failure_fallback: String,
}

pub(super) struct HostRedisMutationDelivery {
    pub(super) request: HostRedisMutationRequest,
    pub(super) result: Result<SshSecretCommandOutput, ()>,
}

pub(super) struct HostRedisValueDialog {
    pub(super) connection_id: String,
    pub(super) endpoint: oxideterm_connection_monitor::ResourceRedisEndpoint,
    pub(super) database: u32,
    pub(super) key: String,
    pub(super) value: Option<oxideterm_connection_monitor::ResourceRedisValue>,
    pub(super) loading: bool,
    pub(super) error: Option<String>,
    pub(super) json_view: bool,
}

pub(super) struct HostRedisDeleteConfirm {
    pub(super) connection_id: String,
    pub(super) endpoint: oxideterm_connection_monitor::ResourceRedisEndpoint,
    pub(super) database: u32,
    /// Display name: the key name, or the prefix for a subtree deletion.
    pub(super) key: String,
    /// Every key the confirmation removes; a single key for leaf deletes.
    pub(super) keys: Vec<String>,
    pub(super) presence: ExitPresence,
}

pub(in crate::workspace) struct HostRedisCreateKeyDialog {
    pub(super) connection_id: String,
    pub(super) endpoint: oxideterm_connection_monitor::ResourceRedisEndpoint,
    pub(super) database: u32,
    /// New key name; zeroized when the dialog closes.
    pub(super) name: zeroize::Zeroizing<String>,
    pub(super) kind: oxideterm_connection_monitor::RedisCreateKind,
    /// When true the key is created without an expiry and the TTL field is hidden.
    pub(super) ttl_never: bool,
    pub(super) ttl: zeroize::Zeroizing<String>,
    /// Value for `string`/`json`, or the pending element while adding entries.
    pub(super) value: zeroize::Zeroizing<String>,
    /// Pending hash field while adding a pair.
    pub(super) field: zeroize::Zeroizing<String>,
    /// Committed elements: `field<TAB>value` for hash, `member<TAB>score` for a
    /// sorted set, plain values otherwise.
    pub(super) entries: Vec<zeroize::Zeroizing<String>>,
    pub(super) error: Option<String>,
    pub(super) loading: bool,
}

#[derive(Clone)]
pub(super) struct HostRedisCreateKeyRequest {
    pub(super) connection_id: String,
    pub(super) endpoint: oxideterm_connection_monitor::ResourceRedisEndpoint,
    pub(super) database: u32,
    pub(super) key: String,
    pub(super) password: zeroize::Zeroizing<String>,
    pub(super) value: zeroize::Zeroizing<String>,
    pub(super) failure_fallback: String,
}

pub(super) struct HostRedisCreateKeyDelivery {
    pub(super) request: HostRedisCreateKeyRequest,
    pub(super) result: Result<SshSecretCommandOutput, ()>,
}

pub(in crate::workspace) struct HostRedisSetValueDialog {
    pub(super) connection_id: String,
    pub(super) endpoint: oxideterm_connection_monitor::ResourceRedisEndpoint,
    pub(super) database: u32,
    pub(super) key: String,
    /// Shape being edited, taken from the fetched value.
    pub(super) kind: RedisValueWriteKind,
    /// Replacement value for string/JSON, or the pending element.
    pub(super) value: zeroize::Zeroizing<String>,
    /// Pending hash field while adding a pair.
    pub(super) field: zeroize::Zeroizing<String>,
    /// Committed rows: `field<TAB>value`, `member<TAB>score`, or elements.
    pub(super) entries: Vec<zeroize::Zeroizing<String>>,
    pub(super) error: Option<String>,
    pub(super) loading: bool,
}

#[derive(Clone)]
pub(super) struct HostRedisSetValueRequest {
    pub(super) connection_id: String,
    pub(super) endpoint: oxideterm_connection_monitor::ResourceRedisEndpoint,
    pub(super) database: u32,
    pub(super) key: String,
    pub(super) password: zeroize::Zeroizing<String>,
    pub(super) write_kind: RedisValueWriteKind,
    pub(super) value: zeroize::Zeroizing<String>,
    pub(super) failure_fallback: String,
}

pub(super) struct HostRedisSetValueDelivery {
    pub(super) request: HostRedisSetValueRequest,
    pub(super) result: Result<SshSecretCommandOutput, ()>,
}

pub(in crate::workspace) struct HostRedisRenameDialog {
    pub(super) connection_id: String,
    pub(super) endpoint: oxideterm_connection_monitor::ResourceRedisEndpoint,
    pub(super) database: u32,
    pub(super) key: String,
    /// New key name; zeroized when the dialog closes.
    pub(super) value: zeroize::Zeroizing<String>,
    pub(super) error: Option<String>,
    pub(super) loading: bool,
}

pub(super) struct HostRedisState {
    pub(super) snapshot_connection_id: Option<String>,
    pub(super) snapshot: Option<oxideterm_connection_monitor::ResourceRedisSnapshot>,
    pub(super) snapshot_running: Option<HostRedisSnapshotRequest>,
    pub(super) snapshot_pending: Option<HostRedisSnapshotPending>,
    pub(super) snapshot_in_flight: bool,
    pub(super) keys_connection_id: Option<String>,
    pub(super) keys_endpoint: Option<oxideterm_connection_monitor::ResourceRedisEndpoint>,
    pub(super) keys: Option<oxideterm_connection_monitor::ResourceRedisKeys>,
    pub(super) keys_running: Option<HostRedisKeysRequest>,
    pub(super) keys_pending: Option<HostRedisKeysPending>,
    pub(super) keys_in_flight: bool,
    pub(super) value_dialog: Option<HostRedisValueDialog>,
    pub(super) value_running: Option<HostRedisValueRequest>,
    pub(super) value_pending: Option<HostRedisValuePending>,
    pub(super) value_in_flight: bool,
    pub(super) mutation_running: Option<HostRedisMutationRequest>,
    pub(super) set_value_running: Option<HostRedisSetValueRequest>,
    pub(super) create_key_running: Option<HostRedisCreateKeyRequest>,
    /// Key awaiting a delete confirmation.
    pub(super) delete_confirm: Option<HostRedisDeleteConfirm>,
    /// Databases the user collapsed. Databases start expanded.
    pub(super) collapsed_databases: std::collections::BTreeSet<String>,
    /// Key-tree segments the user expanded. Segments start collapsed, so a
    /// freshly browsed database shows only its first-level prefixes.
    pub(super) expanded_segments: std::collections::BTreeSet<String>,
    pub(super) list_state: ListState,
    pub(super) list_cache: std::cell::RefCell<VirtualListSignatureCache>,
}

impl HostRedisState {
    pub(super) fn new() -> Self {
        Self {
            snapshot_connection_id: None,
            snapshot: None,
            snapshot_running: None,
            snapshot_pending: None,
            snapshot_in_flight: false,
            keys_connection_id: None,
            keys_endpoint: None,
            keys: None,
            keys_running: None,
            keys_pending: None,
            keys_in_flight: false,
            value_dialog: None,
            value_running: None,
            value_pending: None,
            value_in_flight: false,
            mutation_running: None,
            set_value_running: None,
            create_key_running: None,
            delete_confirm: None,
            collapsed_databases: std::collections::BTreeSet::new(),
            expanded_segments: std::collections::BTreeSet::new(),
            list_state: tauri_virtual_list_state(
                0,
                gpui::ListAlignment::Top,
                TauriVirtualListSpec::new(px(HOST_REDIS_TREE_ROW_HEIGHT), 8),
            ),
            list_cache: std::cell::RefCell::new(VirtualListSignatureCache::default()),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct HostPodmanActionRequest {
    pub(super) connection_id: String,
    pub(super) container_id: String,
    pub(super) container_name: String,
    pub(super) action: PodmanActionKind,
}

pub(super) struct HostPodmanActionDelivery {
    pub(super) request: HostPodmanActionRequest,
    pub(super) result: Result<bool, ()>,
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct HostPodmanPodActionRequest {
    pub(super) connection_id: String,
    pub(super) pod_id: String,
    pub(super) pod_name: String,
    pub(super) action: PodmanPodActionKind,
}

pub(super) struct HostPodmanPodActionDelivery {
    pub(super) request: HostPodmanPodActionRequest,
    pub(super) result: Result<bool, ()>,
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct HostPodmanLogsRequest {
    pub(super) connection_id: String,
    pub(super) container_id: String,
    pub(super) container_name: String,
    pub(super) failure_fallback: String,
    pub(super) empty_fallback: String,
}

pub(super) struct HostPodmanLogsDelivery {
    pub(super) request: HostPodmanLogsRequest,
    pub(super) result: Result<SshCommandOutput, ()>,
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct HostPodmanLogsDialog {
    pub(super) request: HostPodmanLogsRequest,
    // Podman output stays in one shared zeroizing buffer while rendered.
    pub(super) output: Option<Arc<zeroize::Zeroizing<String>>>,
    pub(super) error: Option<String>,
    pub(super) loading: bool,
}

pub(super) struct HostPodmanOperationsState {
    pub(super) pending_confirm: Option<HostToolConfirmState<HostPodmanActionRequest>>,
    pub(super) action_running: Option<HostPodmanActionRequest>,
    pub(super) pod_pending_confirm: Option<HostToolConfirmState<HostPodmanPodActionRequest>>,
    pub(super) pod_action_running: Option<HostPodmanPodActionRequest>,
    pub(super) logs_dialog: Option<HostPodmanLogsDialog>,
}

impl HostPodmanOperationsState {
    pub(super) fn new() -> Self {
        Self {
            pending_confirm: None,
            action_running: None,
            pod_pending_confirm: None,
            pod_action_running: None,
            logs_dialog: None,
        }
    }
}

/// Which Podman resource sub-view is active inside the Host Tool panel.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::workspace) enum HostPodmanTab {
    #[default]
    Containers,
    Pods,
    Images,
    Volumes,
    Networks,
}

/// User-composed terminal operations live in the input dialog until submit;
/// the buffers zeroize when the dialog closes or the command is built.
#[derive(Clone, Eq, PartialEq)]
pub(in crate::workspace) enum HostPodmanInputDialogKind {
    RunContainer,
    RenameContainer { container_id: String },
    PullImage,
    PushImage,
    BuildImage,
    TagImage { image_id: String },
    SaveImage { image_id: String },
    LoadImage,
    ImportImage,
    ExportContainer { container_id: String },
    CopyFiles { container_id: String },
    CreateVolume,
    CreateNetwork,
    SearchImages,
    Login,
}

pub(in crate::workspace) struct HostPodmanInputDialog {
    pub(in crate::workspace) kind: HostPodmanInputDialogKind,
    // Composed command inputs zeroize when the dialog closes or the value
    // hands off to command construction.
    pub(in crate::workspace) value: zeroize::Zeroizing<String>,
}

#[derive(Clone)]
pub(in crate::workspace) struct HostPodmanTextDialog {
    pub(in crate::workspace) title: String,
    pub(in crate::workspace) output: Option<Arc<zeroize::Zeroizing<String>>>,
    pub(in crate::workspace) error: Option<String>,
    pub(in crate::workspace) loading: bool,
    /// Shared scroll state so the rendered scrollbar and wheel scrolling
    /// stay in sync across re-renders while content streams in.
    pub(in crate::workspace) scroll_handle: gpui::ScrollHandle,
}

/// Pointer anchor for the capture dialog's corner resize grip.
#[derive(Clone, Copy)]
pub(in crate::workspace) struct HostPodmanDialogResizeDrag {
    pub(super) start_cursor: gpui::Point<gpui::Pixels>,
    pub(super) start_width: f32,
    pub(super) start_height: f32,
}

#[derive(Clone, Eq, PartialEq)]
pub(in crate::workspace) enum HostPodmanCaptureKind {
    Resources,
    Stats,
    Search { term: String },
    Inspect { reference: String },
    PodInspect { pod_id: String },
    History { image_id: String },
    Top { container_id: String },
    Diff { container_id: String },
    VolumeInspect { name: String },
    NetworkInspect { name: String },
    VolumeUsage { name: String },
}

#[derive(Clone, Eq, PartialEq)]
pub(in crate::workspace) struct HostPodmanCaptureRequest {
    pub(in crate::workspace) connection_id: String,
    pub(in crate::workspace) kind: HostPodmanCaptureKind,
    pub(in crate::workspace) failure_fallback: String,
}

pub(in crate::workspace) struct HostPodmanCaptureDelivery {
    pub(in crate::workspace) request: HostPodmanCaptureRequest,
    pub(in crate::workspace) result: Result<SshCommandOutput, ()>,
}

#[derive(Clone, Eq, PartialEq)]
pub(in crate::workspace) struct HostPodmanResourceActionRequest {
    pub(in crate::workspace) connection_id: String,
    pub(in crate::workspace) description: String,
    pub(in crate::workspace) action: PodmanResourceAction,
}

pub(in crate::workspace) struct HostPodmanResourceActionDelivery {
    pub(in crate::workspace) request: HostPodmanResourceActionRequest,
    pub(in crate::workspace) result: Result<bool, ()>,
}

pub(super) struct HostPodmanResourcesState {
    pub(super) snapshot_connection_id: Option<String>,
    pub(super) images: Option<ResourcePodmanImagesSnapshot>,
    pub(super) volumes: Option<ResourcePodmanVolumesSnapshot>,
    pub(super) networks: Option<ResourcePodmanNetworksSnapshot>,
    pub(super) snapshot_running: bool,
    pub(super) snapshot_pending: Option<HostPodmanCaptureRequest>,
    /// Last resources capture failure; cleared on the next successful capture.
    pub(super) resources_error: Option<String>,
    pub(super) stats: Option<ResourcePodmanStatsSnapshot>,
    pub(super) stats_in_flight: bool,
    pub(super) search_in_flight: bool,
    pub(super) text_dialog: Option<HostPodmanTextDialog>,
    pub(super) text_dialog_running: Option<HostPodmanCaptureRequest>,
    /// User-chosen capture dialog size, kept across openings.
    pub(super) text_dialog_size: (f32, f32),
    pub(super) text_dialog_resize: Option<HostPodmanDialogResizeDrag>,
    pub(super) resource_confirm: Option<HostToolConfirmState<HostPodmanResourceActionRequest>>,
    pub(super) resource_action_running: bool,
}

impl HostPodmanResourcesState {
    pub(super) fn new() -> Self {
        Self {
            snapshot_connection_id: None,
            images: None,
            volumes: None,
            networks: None,
            snapshot_running: false,
            snapshot_pending: None,
            resources_error: None,
            stats: None,
            stats_in_flight: false,
            search_in_flight: false,
            text_dialog: None,
            text_dialog_running: None,
            text_dialog_size: (
                HOST_PODMAN_CAPTURE_DIALOG_DEFAULT_WIDTH,
                HOST_PODMAN_CAPTURE_DIALOG_DEFAULT_HEIGHT,
            ),
            text_dialog_resize: None,
            resource_confirm: None,
            resource_action_running: false,
        }
    }
}

/// Container tool inventory tabs. Pods only exist on Podman, so the panel
/// hides that tab instead of rendering an always-empty page.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum HostContainerTab {
    #[default]
    Containers,
    Images,
    Volumes,
    Networks,
    Pods,
    Stats,
    Registries,
}

pub(super) const HOST_CONTAINER_CAPTURE_DIALOG_DEFAULT_WIDTH: f32 = 760.0;
pub(super) const HOST_CONTAINER_CAPTURE_DIALOG_DEFAULT_HEIGHT: f32 = 420.0;
pub(super) const HOST_CONTAINER_CAPTURE_DIALOG_MIN_WIDTH: f32 = 420.0;
pub(super) const HOST_CONTAINER_CAPTURE_DIALOG_MIN_HEIGHT: f32 = 260.0;
pub(super) const HOST_CONTAINER_CAPTURE_DIALOG_MAX_WIDTH: f32 = 1100.0;
pub(super) const HOST_CONTAINER_CAPTURE_DIALOG_MAX_HEIGHT: f32 = 720.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::workspace) enum HostContainerConfirmKind {
    Container(oxideterm_container::ContainerAction),
    Image(oxideterm_container::ImageAction),
    Volume(oxideterm_container::VolumeAction),
    Network(oxideterm_container::NetworkAction),
    Pod(oxideterm_container::PodAction),
    /// A command assembled from an input dialog; never shown as a confirm.
    Raw,
}

/// A confirmed mutation stays here until its command delivery resolves.
#[derive(Clone)]
pub(in crate::workspace) struct HostContainerConfirmRequest {
    pub(in crate::workspace) connection_id: String,
    pub(in crate::workspace) instance: oxideterm_container::ContainerInstance,
    pub(in crate::workspace) target_id: String,
    pub(in crate::workspace) label: String,
    pub(in crate::workspace) kind: HostContainerConfirmKind,
}

pub(in crate::workspace) struct HostContainerConfirmDelivery {
    pub(in crate::workspace) request: HostContainerConfirmRequest,
    pub(in crate::workspace) result: Result<SshCommandOutput, ()>,
}

/// Text-input dialogs for the container tool. Free-form arguments stay raw by
/// design; identifiers are validated before they reach a command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HostContainerInputKind {
    Run,
    Pull,
    Push,
    Build,
    Tag { image_id: String },
    Save { image_id: String },
    Load,
    Import,
    Export { container_id: String },
    Copy { container_id: String },
    CreateVolume,
    CreateNetwork,
    CreatePod,
    Rename { container_id: String },
    Search,
    Login,
}

#[derive(Clone)]
pub(super) struct HostContainerInputDialog {
    pub(super) kind: HostContainerInputKind,
    pub(super) value: String,
}

#[derive(Clone)]
pub(in crate::workspace) enum HostContainerCaptureKind {
    /// Engine detection; also refreshes the instance switcher.
    Discovery,
    Snapshot(oxideterm_container::ContainerInstance),
    /// One-shot container resource usage.
    Stats,
    /// Authenticated registries read from the engine auth file.
    Registries,
    /// Read-only output rendered in the shared text dialog.
    Text {
        capture: oxideterm_container::ContainerCapture,
        title: String,
    },
}

#[derive(Clone)]
pub(in crate::workspace) struct HostContainerCaptureRequest {
    pub(in crate::workspace) connection_id: String,
    pub(in crate::workspace) kind: HostContainerCaptureKind,
    pub(in crate::workspace) failure_fallback: String,
}

pub(in crate::workspace) struct HostContainerCaptureDelivery {
    pub(in crate::workspace) request: HostContainerCaptureRequest,
    pub(in crate::workspace) result: Result<SshCommandOutput, ()>,
}

pub(super) struct HostContainerTextDialog {
    pub(super) title: String,
    pub(super) output: Option<Arc<zeroize::Zeroizing<String>>>,
    pub(super) error: Option<String>,
    pub(super) loading: bool,
    pub(super) scroll_handle: gpui::ScrollHandle,
}

#[derive(Clone, Copy)]
pub(super) struct HostContainerDialogResizeDrag {
    pub(super) start_cursor: gpui::Point<gpui::Pixels>,
    pub(super) start_width: f32,
    pub(super) start_height: f32,
}

pub(super) struct HostContainerState {
    pub(super) discovery: Option<oxideterm_container::ContainerDiscovery>,
    pub(super) discovery_connection_id: Option<String>,
    pub(super) discovery_running: bool,
    /// Engine selection survives refreshes and is dropped only when the engine
    /// disappears from discovery.
    pub(super) selected_instance_id: Option<String>,
    pub(super) snapshot_connection_id: Option<String>,
    pub(super) snapshot: Option<oxideterm_container::ContainerSnapshot>,
    pub(super) snapshot_running: bool,
    pub(super) tab: HostContainerTab,
    pub(super) confirm: Option<HostToolConfirmState<HostContainerConfirmRequest>>,
    pub(super) action_running: bool,
    pub(super) text_dialog: Option<HostContainerTextDialog>,
    pub(super) text_dialog_running: Option<HostContainerCaptureRequest>,
    /// User-chosen capture dialog size, kept across openings.
    pub(super) text_dialog_size: (f32, f32),
    pub(super) text_dialog_resize: Option<HostContainerDialogResizeDrag>,
    pub(super) input_running: bool,
    pub(super) stats: Option<Vec<oxideterm_container::ContainerStat>>,
    pub(super) stats_running: bool,
    pub(super) registries: Option<Vec<oxideterm_container::ContainerRegistry>>,
    pub(super) registries_running: bool,
    /// Row whose action strip is expanded; only one row shows actions at a time.
    pub(super) selected_row: Option<String>,
    /// One virtual list serves every inventory tab.
    pub(super) list_state: ListState,
    pub(super) list_cache: std::cell::RefCell<VirtualListSignatureCache>,
}

impl HostContainerState {
    pub(super) fn new() -> Self {
        Self {
            discovery: None,
            discovery_connection_id: None,
            discovery_running: false,
            selected_instance_id: None,
            snapshot_connection_id: None,
            snapshot: None,
            snapshot_running: false,
            tab: HostContainerTab::Containers,
            confirm: None,
            action_running: false,
            text_dialog: None,
            text_dialog_running: None,
            text_dialog_size: (
                HOST_CONTAINER_CAPTURE_DIALOG_DEFAULT_WIDTH,
                HOST_CONTAINER_CAPTURE_DIALOG_DEFAULT_HEIGHT,
            ),
            text_dialog_resize: None,
            input_running: false,
            stats: None,
            stats_running: false,
            registries: None,
            registries_running: false,
            selected_row: None,
            list_state: tauri_virtual_list_state(
                0,
                ListAlignment::Top,
                TauriVirtualListSpec::new(px(88.0), 6),
            ),
            list_cache: std::cell::RefCell::new(VirtualListSignatureCache::default()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::workspace) enum HostSnapshotFeedback {
    Silent,
    Toast,
}

/// Identifies the top Host Tools portal using the same order as root rendering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::workspace) enum HostToolsWindowModalSnapshot {
    ProcessConfirm(oxideterm_gpui_ui::motion::ExitPhase),
    DockerConfirm(oxideterm_gpui_ui::motion::ExitPhase),
    DockerLogs,
    PodmanConfirm(oxideterm_gpui_ui::motion::ExitPhase),
    PodmanLogs,
    PodmanInput,
    PodmanCaptureText,
    ContainerConfirm(oxideterm_gpui_ui::motion::ExitPhase),
    ContainerText,
    ContainerInput,
    ServiceConfirm(oxideterm_gpui_ui::motion::ExitPhase),
    ServiceLogs,
    TmuxConfirm(oxideterm_gpui_ui::motion::ExitPhase),
    TmuxInput,
    ScheduleConfirm(oxideterm_gpui_ui::motion::ExitPhase),
    ScheduleLogs,
}

impl HostSnapshotFeedback {
    pub(super) fn should_toast(self) -> bool {
        matches!(self, Self::Toast)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HostLogSnapshotRequest {
    pub(super) connection_id: String,
    pub(super) preset: LogPreset,
    pub(super) limit: usize,
    pub(super) feedback: HostSnapshotFeedback,
    pub(super) failure_fallback: String,
}

pub(super) struct HostLogSnapshotDelivery {
    pub(super) request: HostLogSnapshotRequest,
    pub(super) result: Result<SshCommandOutput, ()>,
}

pub(super) fn zeroize_host_snapshot_output(output: &mut SshCommandOutput) {
    // Host inspection commands may return credentials embedded in logs or
    // diagnostics. Clear both raw streams once parsing or classification ends.
    output.stdout.zeroize();
    output.stderr.zeroize();
}

#[cfg(test)]
mod snapshot_output_zeroize_tests {
    use super::*;

    #[test]
    fn host_snapshot_output_zeroizes_both_raw_streams() {
        let mut output = SshCommandOutput {
            stdout: "Authorization: secret-output".to_string(),
            stderr: "Proxy-Authorization: secret-error".to_string(),
            exit_code: Some(1),
            truncated: true,
        };

        zeroize_host_snapshot_output(&mut output);

        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
        assert_eq!(output.exit_code, Some(1));
        assert!(output.truncated);
    }
}

pub(super) struct HostLogsState {
    pub(super) expanded_index: Option<usize>,
    pub(super) preset: LogPreset,
    pub(super) snapshot_connection_id: Option<String>,
    pub(super) snapshot: Option<ResourceLogSnapshot>,
    pub(super) running: Option<HostLogSnapshotRequest>,
    pub(super) snapshot_in_flight: bool,
    pub(super) list_state: ListState,
    pub(super) list_cache: RefCell<VirtualListSignatureCache>,
}

impl HostLogsState {
    pub(super) fn new() -> Self {
        Self {
            expanded_index: None,
            preset: LogPreset::All,
            snapshot_connection_id: None,
            snapshot: None,
            running: None,
            snapshot_in_flight: false,
            list_state: tauri_virtual_list_state(
                0,
                ListAlignment::Top,
                TauriVirtualListSpec::new(px(HOST_LOG_LIST_ESTIMATED_ROW_HEIGHT), 8),
            ),
            list_cache: RefCell::new(VirtualListSignatureCache::default()),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct HostTmuxSnapshotRequest {
    pub(super) connection_id: String,
    pub(super) feedback: HostSnapshotFeedback,
    pub(super) search_query: String,
    pub(super) failure_fallback: String,
    pub(super) unavailable_fallback: String,
}

pub(super) struct HostTmuxSnapshotDelivery {
    pub(super) request: HostTmuxSnapshotRequest,
    pub(super) result: Result<SshCommandOutput, ()>,
}

#[derive(Clone, Eq, PartialEq)]
pub(super) enum HostTmuxDestructiveAction {
    KillSession { target: String },
    KillWindow { target: String },
    KillPane { target: String },
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct HostTmuxActionRequest {
    pub(super) connection_id: String,
    pub(super) session_id: String,
    pub(super) session_name: String,
    pub(super) target_label: String,
    // Confirm state accepts only destructive actions, so secret-bearing rename
    // and send-command values can never enter its cloneable request type.
    pub(super) action: HostTmuxDestructiveAction,
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct HostTmuxActionRun {
    pub(super) connection_id: String,
    pub(super) session_id: String,
    pub(super) session_name: String,
    pub(super) target_label: String,
}

pub(super) struct HostTmuxActionDelivery {
    pub(super) request: HostTmuxActionRun,
    pub(super) result: Result<bool, ()>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HostPortSnapshotRequest {
    pub(super) connection_id: String,
    pub(super) feedback: HostSnapshotFeedback,
    pub(super) failure_fallback: String,
}

pub(super) struct HostPortSnapshotDelivery {
    pub(super) request: HostPortSnapshotRequest,
    pub(super) result: Result<SshCommandOutput, ()>,
}

pub(super) struct HostPortsState {
    pub(super) filter: PortFilter,
    pub(super) expanded_index: Option<usize>,
    pub(super) snapshot_connection_id: Option<String>,
    pub(super) snapshot: Option<ResourcePortSnapshot>,
    pub(super) running: Option<HostPortSnapshotRequest>,
    pub(super) snapshot_in_flight: bool,
    pub(super) list_state: ListState,
    pub(super) list_cache: RefCell<VirtualListSignatureCache>,
}

impl HostPortsState {
    pub(super) fn new() -> Self {
        Self {
            filter: PortFilter::All,
            expanded_index: None,
            snapshot_connection_id: None,
            snapshot: None,
            running: None,
            snapshot_in_flight: false,
            list_state: tauri_virtual_list_state(
                0,
                ListAlignment::Top,
                TauriVirtualListSpec::new(px(HOST_PORT_LIST_ESTIMATED_ROW_HEIGHT), 8),
            ),
            list_cache: RefCell::new(VirtualListSignatureCache::default()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HostScheduleSnapshotRequest {
    pub(super) connection_id: String,
    pub(super) feedback: HostSnapshotFeedback,
    pub(super) failure_fallback: String,
}

pub(super) struct HostScheduleSnapshotDelivery {
    pub(super) request: HostScheduleSnapshotRequest,
    pub(super) result: Result<SshCommandOutput, ()>,
}

pub(super) struct HostSchedulesState {
    pub(super) filter: ScheduledTaskFilter,
    pub(super) expanded_index: Option<usize>,
    pub(super) snapshot_connection_id: Option<String>,
    pub(super) snapshot: Option<ResourceScheduledTaskSnapshot>,
    pub(super) running: Option<HostScheduleSnapshotRequest>,
    pub(super) snapshot_in_flight: bool,
    pub(super) list_state: ListState,
    pub(super) list_cache: RefCell<VirtualListSignatureCache>,
    pub(super) pending_confirm: Option<HostToolConfirmState<HostScheduleActionRequest>>,
    pub(super) action_running: Option<HostScheduleActionRequest>,
    pub(super) logs_dialog: Option<HostScheduleLogsDialog>,
}

impl HostSchedulesState {
    pub(super) fn new() -> Self {
        Self {
            filter: ScheduledTaskFilter::All,
            expanded_index: None,
            snapshot_connection_id: None,
            snapshot: None,
            running: None,
            snapshot_in_flight: false,
            list_state: tauri_virtual_list_state(
                0,
                ListAlignment::Top,
                TauriVirtualListSpec::new(px(HOST_SCHEDULE_LIST_ESTIMATED_ROW_HEIGHT), 8),
            ),
            list_cache: RefCell::new(VirtualListSignatureCache::default()),
            pending_confirm: None,
            action_running: None,
            logs_dialog: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HostFilesystemSnapshotRequest {
    pub(super) connection_id: String,
    pub(super) feedback: HostSnapshotFeedback,
    pub(super) failure_fallback: String,
}

pub(super) struct HostFilesystemSnapshotDelivery {
    pub(super) request: HostFilesystemSnapshotRequest,
    pub(super) result: Result<SshCommandOutput, ()>,
}

pub(super) struct HostFilesystemsState {
    pub(super) filter: FilesystemFilter,
    pub(super) expanded_index: Option<usize>,
    pub(super) snapshot_connection_id: Option<String>,
    pub(super) snapshot: Option<ResourceFilesystemSnapshot>,
    pub(super) running: Option<HostFilesystemSnapshotRequest>,
    pub(super) snapshot_in_flight: bool,
    pub(super) list_state: ListState,
    pub(super) list_cache: RefCell<VirtualListSignatureCache>,
}

impl HostFilesystemsState {
    pub(super) fn new() -> Self {
        Self {
            filter: FilesystemFilter::All,
            expanded_index: None,
            snapshot_connection_id: None,
            snapshot: None,
            running: None,
            snapshot_in_flight: false,
            list_state: tauri_virtual_list_state(
                0,
                ListAlignment::Top,
                TauriVirtualListSpec::new(px(HOST_FILESYSTEM_LIST_ESTIMATED_ROW_HEIGHT), 8),
            ),
            list_cache: RefCell::new(VirtualListSignatureCache::default()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HostPackageSnapshotRequest {
    pub(super) connection_id: String,
    pub(super) feedback: HostSnapshotFeedback,
    pub(super) failure_fallback: String,
}

pub(super) struct HostPackageSnapshotDelivery {
    pub(super) request: HostPackageSnapshotRequest,
    pub(super) result: Result<SshCommandOutput, ()>,
}

pub(super) struct HostPackagesState {
    pub(super) filter: PackageFilter,
    pub(super) expanded_index: Option<usize>,
    pub(super) snapshot_connection_id: Option<String>,
    pub(super) snapshot: Option<ResourcePackageSnapshot>,
    pub(super) running: Option<HostPackageSnapshotRequest>,
    pub(super) snapshot_in_flight: bool,
    pub(super) list_state: ListState,
    pub(super) list_cache: RefCell<VirtualListSignatureCache>,
}

impl HostPackagesState {
    pub(super) fn new() -> Self {
        Self {
            filter: PackageFilter::All,
            expanded_index: None,
            snapshot_connection_id: None,
            snapshot: None,
            running: None,
            snapshot_in_flight: false,
            list_state: tauri_virtual_list_state(
                0,
                ListAlignment::Top,
                TauriVirtualListSpec::new(px(HOST_PACKAGE_LIST_ESTIMATED_ROW_HEIGHT), 8),
            ),
            list_cache: RefCell::new(VirtualListSignatureCache::default()),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct HostScheduleActionRequest {
    pub(super) connection_id: String,
    pub(super) task_id: String,
    pub(super) task_name: String,
    pub(super) unit: String,
    pub(super) action: ScheduledTaskActionKind,
}

pub(super) struct HostScheduleActionDelivery {
    pub(super) request: HostScheduleActionRequest,
    pub(super) result: Result<bool, ()>,
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct HostScheduleLogsRequest {
    pub(super) connection_id: String,
    // Keep sampled commands out of asynchronous deliveries; logs need only
    // the public task identity used by the dialog and follow action.
    pub(super) task_id: String,
    pub(super) task_name: String,
    pub(super) task_source: String,
    pub(super) task_unit: String,
    pub(super) failure_fallback: String,
    pub(super) empty_fallback: String,
}

pub(super) struct HostScheduleLogsDelivery {
    pub(super) request: HostScheduleLogsRequest,
    pub(super) result: Result<SshCommandOutput, ()>,
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct HostScheduleLogsDialog {
    pub(super) request: HostScheduleLogsRequest,
    // The Entity and render tree share one zeroizing capture buffer.
    pub(super) output: Option<Arc<zeroize::Zeroizing<String>>>,
    pub(super) error: Option<String>,
    pub(super) loading: bool,
}

#[derive(Clone, Eq, PartialEq)]
pub(in crate::workspace) enum HostTmuxInputDialogKind {
    RenameSession { target: String },
    RenameWindow { target: String },
    SendPaneCommand { target: String },
}

pub(in crate::workspace) struct HostTmuxInputDialog {
    pub(super) connection_id: String,
    pub(super) session_id: String,
    pub(super) session_name: String,
    pub(super) target_label: String,
    // User commands may contain secrets; the dialog clears the only retained
    // input buffer when it closes or hands the value to command construction.
    pub(in crate::workspace) value: zeroize::Zeroizing<String>,
    pub(super) kind: HostTmuxInputDialogKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::workspace) enum HostToolsTextInput {
    ProcessSearch,
    ProcessRenice,
    DockerSearch,
    PodmanSearch,
    ServiceSearch,
    LogSearch,
    TmuxSearch,
    TmuxDialog,
    PodmanDialog,
    PortSearch,
    ScheduleSearch,
    FilesystemSearch,
    PackageSearch,
    ContainerSearch,
    ContainerDialog,
    RedisPassword,
    RedisRename,
    RedisSetValue,
    RedisSetValueField,
    RedisCreateKeyName,
    RedisCreateKeyValue,
    RedisCreateKeyField,
    RedisCreateKeyTtl,
}

pub(super) struct HostTmuxState {
    pub(super) snapshot_connection_id: Option<String>,
    pub(super) snapshot: Option<ResourceTmuxSnapshot>,
    pub(super) snapshot_running: Option<HostTmuxSnapshotRequest>,
    pub(super) snapshot_in_flight: bool,
    pub(super) last_error: Option<String>,
    pub(super) pending_confirm: Option<HostToolConfirmState<HostTmuxActionRequest>>,
    pub(super) action_running: Option<HostTmuxActionRun>,
}

impl HostTmuxState {
    pub(super) fn new() -> Self {
        Self {
            snapshot_connection_id: None,
            snapshot: None,
            snapshot_running: None,
            snapshot_in_flight: false,
            last_error: None,
            pending_confirm: None,
            action_running: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::workspace) enum ConnectionRuntimeSection {
    Overview,
    Topology,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::workspace) enum HostToolsVisibility {
    Hidden,
    VisibleMainTab,
    VisibleSidebar,
    VisibleDetachedWindow,
    VisibleMultiple {
        main_tab: bool,
        sidebar: bool,
        detached_window: bool,
    },
    Dropped,
}

impl HostToolsVisibility {
    pub(in crate::workspace) fn from_mounts(
        main_tab: bool,
        sidebar: bool,
        detached_window: bool,
    ) -> Self {
        match (
            usize::from(main_tab) + usize::from(sidebar) + usize::from(detached_window),
            main_tab,
            sidebar,
            detached_window,
        ) {
            (0, _, _, _) => Self::Hidden,
            (1, true, _, _) => Self::VisibleMainTab,
            (1, _, true, _) => Self::VisibleSidebar,
            (1, _, _, true) => Self::VisibleDetachedWindow,
            _ => Self::VisibleMultiple {
                main_tab,
                sidebar,
                detached_window,
            },
        }
    }

    pub(in crate::workspace) fn is_visible(self) -> bool {
        !matches!(self, Self::Hidden | Self::Dropped)
    }

    pub(in crate::workspace) fn sidebar_is_visible(self) -> bool {
        matches!(
            self,
            Self::VisibleSidebar | Self::VisibleMultiple { sidebar: true, .. }
        )
    }

    pub(in crate::workspace) fn main_window_is_visible(self) -> bool {
        matches!(
            self,
            Self::VisibleMainTab
                | Self::VisibleSidebar
                | Self::VisibleMultiple { main_tab: true, .. }
                | Self::VisibleMultiple { sidebar: true, .. }
        )
    }
}

#[derive(Clone)]
pub(in crate::workspace) struct HostToolsMessages {
    pub(super) service_connection_missing: String,
    pub(super) service_action_failed: String,
    pub(super) log_unknown_error: String,
    pub(super) port_unknown_error: String,
    pub(super) filesystem_unknown_error: String,
    pub(super) package_unknown_error: String,
    pub(super) schedule_unknown_error: String,
    pub(super) tmux_unknown_error: String,
    pub(super) tmux_unavailable: String,
    pub(super) redis_connection_missing: String,
    pub(super) redis_probe_failed: String,
    pub(super) redis_key_action_failed: String,
    pub(super) redis_rename_invalid: String,
    pub(super) redis_auth_required: String,
    pub(super) redis_key_missing: String,
    pub(super) redis_value_set_invalid_json: String,
    pub(super) redis_create_key_exists: String,
    pub(super) redis_create_key_invalid_name: String,
    pub(super) redis_create_key_invalid_ttl: String,
    pub(super) redis_create_key_empty_entries: String,
    pub(super) redis_create_key_invalid_field: String,
    pub(super) podman_capture_failed: String,
    pub(super) container_terminal_opened: String,
    pub(super) container_terminal_missing: String,
}

impl HostToolsMessages {
    pub(in crate::workspace) fn from_i18n(i18n: &I18n) -> Self {
        Self {
            service_connection_missing: i18n.t("sidebar.host_services.toast.connection_missing"),
            service_action_failed: i18n.t("sidebar.host_services.toast.action_failed"),
            log_unknown_error: i18n.t("sidebar.host_logs.toast.unknown_error"),
            port_unknown_error: i18n.t("sidebar.host_ports.toast.unknown_error"),
            filesystem_unknown_error: i18n.t("sidebar.host_filesystems.toast.unknown_error"),
            package_unknown_error: i18n.t("sidebar.host_packages.toast.unknown_error"),
            schedule_unknown_error: i18n.t("sidebar.host_schedules.toast.unknown_error"),
            tmux_unknown_error: i18n.t("sidebar.host_tmux.toast.unknown_error"),
            tmux_unavailable: i18n.t("sidebar.host_tmux.unavailable"),
            redis_connection_missing: i18n.t("sidebar.host_redis.toast.connection_missing"),
            redis_probe_failed: i18n.t("sidebar.host_redis.toast.probe_failed"),
            redis_key_action_failed: i18n.t("sidebar.host_redis.toast.key_action_failed"),
            redis_rename_invalid: i18n.t("sidebar.host_redis.rename.invalid"),
            redis_auth_required: i18n.t("sidebar.host_redis.auth_required"),
            redis_key_missing: i18n.t("sidebar.host_redis.value.ttl_missing"),
            redis_value_set_invalid_json: i18n.t("sidebar.host_redis.set_value.invalid_json"),
            redis_create_key_exists: i18n.t("sidebar.host_redis.create_key.exists"),
            redis_create_key_invalid_name: i18n.t("sidebar.host_redis.create_key.invalid_name"),
            redis_create_key_invalid_ttl: i18n.t("sidebar.host_redis.create_key.invalid_ttl"),
            redis_create_key_empty_entries: i18n.t("sidebar.host_redis.create_key.empty_entries"),
            redis_create_key_invalid_field: i18n.t("sidebar.host_redis.create_key.invalid_field"),
            podman_capture_failed: i18n.t("sidebar.host_podman.captures.failed"),
            container_terminal_opened: i18n.t("sidebar.host_container.toast.terminal_opened"),
            container_terminal_missing: i18n.t("sidebar.host_container.toast.terminal_missing"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CompactMonitorLayout {
    Inline,
    Stacked,
}

/// Keeps a standard Host Tools confirmation payload alive while its exit motion runs.
pub(super) struct HostToolConfirmState<T> {
    pub(super) request: T,
    pub(super) presence: ExitPresence,
}

pub(super) struct HostGpuViewState {
    pub(super) update_tx: tokio::sync::mpsc::UnboundedSender<GpuUpdate>,
    pub(super) sampling_task: Option<GpuSamplingTask>,
    pub(super) snapshot_connection_id: Option<String>,
    pub(super) snapshot: Option<GpuSnapshot>,
    pub(super) expanded_uuid: Option<String>,
    pub(super) list_state: ListState,
    pub(super) list_cache: RefCell<VirtualListSignatureCache>,
}

impl HostGpuViewState {
    pub(super) fn new(update_tx: tokio::sync::mpsc::UnboundedSender<GpuUpdate>) -> Self {
        Self {
            update_tx,
            sampling_task: None,
            snapshot_connection_id: None,
            snapshot: None,
            expanded_uuid: None,
            list_state: tauri_virtual_list_state(
                0,
                ListAlignment::Top,
                TauriVirtualListSpec::new(px(HOST_GPU_LIST_ESTIMATED_ROW_HEIGHT), 8),
            ),
            list_cache: RefCell::new(VirtualListSignatureCache::default()),
        }
    }
}

impl<T> HostToolConfirmState<T> {
    pub(super) fn new(request: T) -> Self {
        Self {
            request,
            presence: ExitPresence::visible(),
        }
    }

    /// Reuses the generation so a stale timer cannot close a replacement request.
    pub(super) fn open(slot: &mut Option<Self>, request: T) {
        if let Some(state) = slot.as_mut() {
            state.request = request;
            state.presence.reopen();
        } else {
            *slot = Some(Self::new(request));
        }
    }
}

/// Owns Host Tools input, selection, expansion, and virtual-list presentation state.
pub(in crate::workspace) struct HostToolsUiState {
    pub(in crate::workspace) focused_input: Option<HostToolsTextInput>,
    /// Container search text lives with the other Host Tools inputs so the
    /// shared IME plumbing can read and replace it.
    pub(super) host_container_search_query: String,
    pub(super) host_container_input_dialog: Option<HostContainerInputDialog>,
    pub(in crate::workspace) host_process_search_query: String,
    pub(super) host_process_filter: ProcessFilter,
    pub(super) host_process_sort: ProcessSort,
    pub(super) host_process_sort_descending: bool,
    pub(in crate::workspace) host_process_expanded_pid: Option<String>,
    pub(super) host_process_list_state: ListState,
    pub(super) host_process_list_cache: RefCell<VirtualListSignatureCache>,
    pub(in crate::workspace) host_process_renice_value: String,
    pub(in crate::workspace) host_docker_search_query: String,
    pub(in crate::workspace) host_docker_expanded_id: Option<String>,
    pub(super) host_docker_list_state: ListState,
    pub(super) host_docker_list_cache: RefCell<VirtualListSignatureCache>,
    pub(in crate::workspace) host_podman_search_query: String,
    pub(in crate::workspace) host_podman_expanded_id: Option<String>,
    pub(in crate::workspace) host_podman_pod_expanded_id: Option<String>,
    pub(super) host_podman_pod_list_state: ListState,
    pub(super) host_podman_pod_list_cache: RefCell<VirtualListSignatureCache>,
    pub(in crate::workspace) host_podman_tab: HostPodmanTab,
    pub(in crate::workspace) host_podman_stats_visible: bool,
    pub(super) host_podman_list_state: ListState,
    pub(super) host_podman_list_cache: RefCell<VirtualListSignatureCache>,
    pub(in crate::workspace) host_service_search_query: String,
    pub(in crate::workspace) host_service_expanded_id: Option<String>,
    pub(super) host_service_list_state: ListState,
    pub(super) host_service_list_cache: RefCell<VirtualListSignatureCache>,
    pub(in crate::workspace) host_log_search_query: String,
    pub(in crate::workspace) host_tmux_search_query: String,
    pub(in crate::workspace) host_tmux_expanded_session_id: Option<String>,
    pub(in crate::workspace) host_tmux_expanded_window_id: Option<String>,
    pub(in crate::workspace) host_tmux_input_dialog: Option<HostTmuxInputDialog>,
    pub(in crate::workspace) host_podman_input_dialog: Option<HostPodmanInputDialog>,
    pub(in crate::workspace) host_redis_rename_dialog: Option<HostRedisRenameDialog>,
    pub(in crate::workspace) host_redis_set_value_dialog: Option<HostRedisSetValueDialog>,
    pub(in crate::workspace) host_redis_create_key_dialog: Option<HostRedisCreateKeyDialog>,
    pub(super) host_tmux_list_state: ListState,
    pub(super) host_tmux_list_cache: RefCell<VirtualListSignatureCache>,
    pub(in crate::workspace) host_port_search_query: String,
    pub(in crate::workspace) host_schedule_search_query: String,
    pub(in crate::workspace) host_filesystem_search_query: String,
    pub(in crate::workspace) host_package_search_query: String,
    /// Redis password buffer; zeroized when the field or panel is dropped.
    pub(in crate::workspace) host_redis_password: zeroize::Zeroizing<String>,
    /// When false the tree hides databases that hold no keys.
    pub(in crate::workspace) host_redis_show_all_databases: bool,
}

impl HostToolsUiState {
    pub(in crate::workspace) fn new() -> Self {
        Self {
            focused_input: None,
            host_container_search_query: String::new(),
            host_container_input_dialog: None,
            host_process_search_query: String::new(),
            host_process_filter: ProcessFilter::All,
            host_process_sort: ProcessSort::Memory,
            host_process_sort_descending: true,
            host_process_expanded_pid: None,
            host_process_list_state: tauri_virtual_list_state(
                0,
                ListAlignment::Top,
                TauriVirtualListSpec::new(px(HOST_PROCESS_LIST_ESTIMATED_ROW_HEIGHT), 8),
            ),
            host_process_list_cache: RefCell::new(VirtualListSignatureCache::default()),
            host_process_renice_value: "0".to_string(),
            host_docker_search_query: String::new(),
            host_docker_expanded_id: None,
            host_docker_list_state: tauri_virtual_list_state(
                0,
                ListAlignment::Top,
                TauriVirtualListSpec::new(px(HOST_DOCKER_LIST_ESTIMATED_ROW_HEIGHT), 8),
            ),
            host_docker_list_cache: RefCell::new(VirtualListSignatureCache::default()),
            host_podman_search_query: String::new(),
            host_podman_expanded_id: None,
            host_podman_pod_expanded_id: None,
            host_podman_pod_list_state: tauri_virtual_list_state(
                0,
                ListAlignment::Top,
                TauriVirtualListSpec::new(px(HOST_PODMAN_LIST_ESTIMATED_ROW_HEIGHT), 8),
            ),
            host_podman_pod_list_cache: RefCell::new(VirtualListSignatureCache::default()),
            host_podman_tab: HostPodmanTab::Containers,
            host_podman_stats_visible: false,
            host_podman_list_state: tauri_virtual_list_state(
                0,
                ListAlignment::Top,
                TauriVirtualListSpec::new(px(HOST_PODMAN_LIST_ESTIMATED_ROW_HEIGHT), 8),
            ),
            host_podman_list_cache: RefCell::new(VirtualListSignatureCache::default()),
            host_service_search_query: String::new(),
            host_service_expanded_id: None,
            host_service_list_state: tauri_virtual_list_state(
                0,
                ListAlignment::Top,
                TauriVirtualListSpec::new(px(HOST_SERVICE_LIST_ESTIMATED_ROW_HEIGHT), 8),
            ),
            host_service_list_cache: RefCell::new(VirtualListSignatureCache::default()),
            host_log_search_query: String::new(),
            host_tmux_search_query: String::new(),
            host_tmux_expanded_session_id: None,
            host_tmux_expanded_window_id: None,
            host_tmux_input_dialog: None,
            host_podman_input_dialog: None,
            host_redis_rename_dialog: None,
            host_redis_set_value_dialog: None,
            host_redis_create_key_dialog: None,
            host_tmux_list_state: tauri_virtual_list_state(
                0,
                ListAlignment::Top,
                TauriVirtualListSpec::new(px(HOST_TMUX_LIST_ESTIMATED_ROW_HEIGHT), 8),
            ),
            host_tmux_list_cache: RefCell::new(VirtualListSignatureCache::default()),
            host_port_search_query: String::new(),
            host_schedule_search_query: String::new(),
            host_filesystem_search_query: String::new(),
            host_package_search_query: String::new(),
            host_redis_password: zeroize::Zeroizing::new(String::new()),
            host_redis_show_all_databases: false,
        }
    }

    pub(in crate::workspace) fn focus_input(&mut self, input: HostToolsTextInput) {
        self.focused_input = Some(input);
    }

    pub(in crate::workspace) fn clear_input_focus(&mut self) {
        self.focused_input = None;
    }

    pub(in crate::workspace) fn retain_input_focus_for_tool(&mut self, tool: ContextSidebarTool) {
        let belongs_to_tool = matches!(
            (self.focused_input, tool),
            (
                Some(HostToolsTextInput::ProcessSearch | HostToolsTextInput::ProcessRenice),
                ContextSidebarTool::Processes
            ) | (
                Some(HostToolsTextInput::DockerSearch),
                ContextSidebarTool::Docker
            ) | (
                Some(HostToolsTextInput::PodmanSearch | HostToolsTextInput::PodmanDialog),
                ContextSidebarTool::Podman
            ) | (
                Some(HostToolsTextInput::ServiceSearch),
                ContextSidebarTool::Services
            ) | (
                Some(HostToolsTextInput::LogSearch),
                ContextSidebarTool::Logs
            ) | (
                Some(HostToolsTextInput::TmuxSearch | HostToolsTextInput::TmuxDialog),
                ContextSidebarTool::Tmux
            ) | (
                Some(HostToolsTextInput::PortSearch),
                ContextSidebarTool::Ports
            ) | (
                Some(HostToolsTextInput::ScheduleSearch),
                ContextSidebarTool::Schedules
            ) | (
                Some(HostToolsTextInput::FilesystemSearch),
                ContextSidebarTool::Filesystems
            ) | (
                Some(HostToolsTextInput::PackageSearch),
                ContextSidebarTool::Packages
            ) | (
                Some(HostToolsTextInput::ContainerSearch | HostToolsTextInput::ContainerDialog),
                ContextSidebarTool::Container
            ) | (
                Some(
                    HostToolsTextInput::RedisPassword
                        | HostToolsTextInput::RedisRename
                        | HostToolsTextInput::RedisSetValue
                        | HostToolsTextInput::RedisSetValueField
                        | HostToolsTextInput::RedisCreateKeyName
                        | HostToolsTextInput::RedisCreateKeyValue
                        | HostToolsTextInput::RedisCreateKeyField
                        | HostToolsTextInput::RedisCreateKeyTtl
                ),
                ContextSidebarTool::Redis
            )
        );
        if !belongs_to_tool {
            self.clear_input_focus();
        }
    }

    pub(in crate::workspace) fn input_is_focused(&self, input: HostToolsTextInput) -> bool {
        self.focused_input == Some(input)
    }

    pub(in crate::workspace) fn input_value(&self, input: HostToolsTextInput) -> Option<&str> {
        if !self.input_is_focused(input) {
            return None;
        }
        match input {
            HostToolsTextInput::ProcessSearch => Some(&self.host_process_search_query),
            HostToolsTextInput::ProcessRenice => Some(&self.host_process_renice_value),
            HostToolsTextInput::DockerSearch => Some(&self.host_docker_search_query),
            HostToolsTextInput::PodmanSearch => Some(&self.host_podman_search_query),
            HostToolsTextInput::ServiceSearch => Some(&self.host_service_search_query),
            HostToolsTextInput::LogSearch => Some(&self.host_log_search_query),
            HostToolsTextInput::TmuxSearch => Some(&self.host_tmux_search_query),
            HostToolsTextInput::TmuxDialog => self
                .host_tmux_input_dialog
                .as_ref()
                .map(|dialog| dialog.value.as_str()),
            HostToolsTextInput::PodmanDialog => self
                .host_podman_input_dialog
                .as_ref()
                .map(|dialog| dialog.value.as_str()),
            HostToolsTextInput::PortSearch => Some(&self.host_port_search_query),
            HostToolsTextInput::ScheduleSearch => Some(&self.host_schedule_search_query),
            HostToolsTextInput::FilesystemSearch => Some(&self.host_filesystem_search_query),
            HostToolsTextInput::PackageSearch => Some(&self.host_package_search_query),
            HostToolsTextInput::ContainerSearch => Some(&self.host_container_search_query),
            HostToolsTextInput::ContainerDialog => self
                .host_container_input_dialog
                .as_ref()
                .map(|dialog| dialog.value.as_str()),
            HostToolsTextInput::RedisPassword => Some(&self.host_redis_password),
            HostToolsTextInput::RedisRename => self.host_redis_rename_value(),
            HostToolsTextInput::RedisSetValue => self.host_redis_set_value(),
            HostToolsTextInput::RedisSetValueField => self.host_redis_set_value_field(),
            HostToolsTextInput::RedisCreateKeyName => self.host_redis_create_key_name(),
            HostToolsTextInput::RedisCreateKeyValue => self.host_redis_create_key_value(),
            HostToolsTextInput::RedisCreateKeyField => self.host_redis_create_key_field(),
            HostToolsTextInput::RedisCreateKeyTtl => self.host_redis_create_key_ttl(),
        }
    }

    fn host_redis_create_key_name(&self) -> Option<&str> {
        self.host_redis_create_key_dialog
            .as_ref()
            .map(|dialog| dialog.name.as_str())
    }

    fn host_redis_create_key_name_mut(&mut self) -> Option<&mut String> {
        self.host_redis_create_key_dialog
            .as_mut()
            .map(|dialog| &mut *dialog.name)
    }

    fn host_redis_create_key_field(&self) -> Option<&str> {
        self.host_redis_create_key_dialog
            .as_ref()
            .map(|dialog| dialog.field.as_str())
    }

    fn host_redis_create_key_field_mut(&mut self) -> Option<&mut String> {
        self.host_redis_create_key_dialog
            .as_mut()
            .map(|dialog| &mut *dialog.field)
    }

    fn host_redis_create_key_ttl(&self) -> Option<&str> {
        self.host_redis_create_key_dialog
            .as_ref()
            .map(|dialog| dialog.ttl.as_str())
    }

    fn host_redis_create_key_ttl_mut(&mut self) -> Option<&mut String> {
        self.host_redis_create_key_dialog
            .as_mut()
            .map(|dialog| &mut *dialog.ttl)
    }

    fn host_redis_create_key_value(&self) -> Option<&str> {
        self.host_redis_create_key_dialog
            .as_ref()
            .map(|dialog| dialog.value.as_str())
    }

    fn host_redis_create_key_value_mut(&mut self) -> Option<&mut String> {
        self.host_redis_create_key_dialog
            .as_mut()
            .map(|dialog| &mut *dialog.value)
    }

    fn host_redis_set_value_field(&self) -> Option<&str> {
        self.host_redis_set_value_dialog
            .as_ref()
            .map(|dialog| dialog.field.as_str())
    }

    fn host_redis_set_value_field_mut(&mut self) -> Option<&mut String> {
        self.host_redis_set_value_dialog
            .as_mut()
            .map(|dialog| &mut *dialog.field)
    }

    fn host_redis_set_value(&self) -> Option<&str> {
        self.host_redis_set_value_dialog
            .as_ref()
            .map(|dialog| dialog.value.as_str())
    }

    fn host_redis_set_value_mut(&mut self) -> Option<&mut String> {
        self.host_redis_set_value_dialog
            .as_mut()
            .map(|dialog| &mut *dialog.value)
    }

    fn host_redis_rename_value(&self) -> Option<&str> {
        self.host_redis_rename_dialog
            .as_ref()
            .map(|dialog| dialog.value.as_str())
    }

    fn host_redis_rename_value_mut(&mut self) -> Option<&mut String> {
        self.host_redis_rename_dialog
            .as_mut()
            .map(|dialog| &mut *dialog.value)
    }

    pub(in crate::workspace) fn input_value_mut(
        &mut self,
        input: HostToolsTextInput,
    ) -> Option<&mut String> {
        if !self.input_is_focused(input) {
            return None;
        }
        match input {
            HostToolsTextInput::ProcessSearch => Some(&mut self.host_process_search_query),
            HostToolsTextInput::ProcessRenice => Some(&mut self.host_process_renice_value),
            HostToolsTextInput::DockerSearch => Some(&mut self.host_docker_search_query),
            HostToolsTextInput::PodmanSearch => Some(&mut self.host_podman_search_query),
            HostToolsTextInput::ServiceSearch => Some(&mut self.host_service_search_query),
            HostToolsTextInput::LogSearch => Some(&mut self.host_log_search_query),
            HostToolsTextInput::TmuxSearch => Some(&mut self.host_tmux_search_query),
            HostToolsTextInput::TmuxDialog => self
                .host_tmux_input_dialog
                .as_mut()
                .map(|dialog| &mut *dialog.value),
            HostToolsTextInput::PodmanDialog => self
                .host_podman_input_dialog
                .as_mut()
                .map(|dialog| &mut *dialog.value),
            HostToolsTextInput::PortSearch => Some(&mut self.host_port_search_query),
            HostToolsTextInput::ScheduleSearch => Some(&mut self.host_schedule_search_query),
            HostToolsTextInput::FilesystemSearch => Some(&mut self.host_filesystem_search_query),
            HostToolsTextInput::PackageSearch => Some(&mut self.host_package_search_query),
            HostToolsTextInput::ContainerSearch => Some(&mut self.host_container_search_query),
            HostToolsTextInput::ContainerDialog => self
                .host_container_input_dialog
                .as_mut()
                .map(|dialog| &mut dialog.value),
            HostToolsTextInput::RedisPassword => Some(&mut self.host_redis_password),
            HostToolsTextInput::RedisRename => self.host_redis_rename_value_mut(),
            HostToolsTextInput::RedisSetValue => self.host_redis_set_value_mut(),
            HostToolsTextInput::RedisSetValueField => self.host_redis_set_value_field_mut(),
            HostToolsTextInput::RedisCreateKeyName => self.host_redis_create_key_name_mut(),
            HostToolsTextInput::RedisCreateKeyValue => self.host_redis_create_key_value_mut(),
            HostToolsTextInput::RedisCreateKeyField => self.host_redis_create_key_field_mut(),
            HostToolsTextInput::RedisCreateKeyTtl => self.host_redis_create_key_ttl_mut(),
        }
    }
}
