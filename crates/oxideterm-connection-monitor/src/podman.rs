use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePodmanContainer {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    /// Published ports as reported by `podman ps` (docker-compatible text).
    pub ports: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePodmanPod {
    pub id: String,
    pub name: String,
    pub status: String,
    /// Humanized age (`podman pod ps --format '{{.Created}}'`).
    pub created: String,
    pub containers: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourcePodmanStatus {
    #[default]
    Unknown,
    Available,
    /// Podman is not usable for the invoking user: the binary is missing or
    /// the daemon/socket (or `podman machine` on Windows) is not running.
    Unavailable {
        detail: Option<String>,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePodmanSnapshot {
    pub status: ResourcePodmanStatus,
    pub containers: Vec<ResourcePodmanContainer>,
    #[serde(default)]
    pub pods: Vec<ResourcePodmanPod>,
}

const PODMAN_UNAVAILABLE_MARKER: &str = "__OXIDE_PODMAN_UNAVAILABLE__";
const PODMAN_ERROR_MARKER: &str = "__OXIDE_PODMAN_ERROR__";
pub(crate) const PODMAN_ROW_PREFIX: &str = "PS\t";
pub(crate) const PODMAN_POD_PREFIX: &str = "POD\t";

// Unlike docker, a failing `podman ps` must never be retried with sudo:
// `sudo podman ps` reads root's container store instead of the invoking
// user's rootless store, silently switching the data source.
const PODMAN_SAMPLE_COMMAND_UNIX: &str = concat!(
    "echo '===PODMAN==='; ",
    "if command -v podman >/dev/null 2>&1; then ",
    "oxide_podman_ps=$(podman ps -a --no-trunc --format '",
    "{{json .}}\t{{.Status}}",
    "' 2>&1); ",
    "oxide_podman_status=$?; ",
    "if [ \"$oxide_podman_status\" -eq 0 ]; then ",
    "printf '%s\\n' \"$oxide_podman_ps\" | sed 's/^/PS\\t/'; ",
    "podman pod ps --format '",
    "{{json .}}\t{{.Created}}",
    "' 2>/dev/null | sed 's/^/POD\\t/'; ",
    "elif printf '%s' \"$oxide_podman_ps\" | grep -q 'Cannot connect'; then ",
    "printf '__OXIDE_PODMAN_UNAVAILABLE__\\t%s\\n' \"$(printf '%s' \"$oxide_podman_ps\" | head -n 1 | tr '\\t' ' ')\"; ",
    "else ",
    "printf '__OXIDE_PODMAN_ERROR__\\t%s\\n' \"$(printf '%s' \"$oxide_podman_ps\" | head -n 1 | tr '\\t' ' ')\"; ",
    "fi; ",
    "else ",
    "echo '__OXIDE_PODMAN_UNAVAILABLE__'; ",
    "fi; ",
    "echo '===PODMAN_END==='"
);

const PODMAN_SAMPLE_COMMAND_WINDOWS: &str = concat!(
    "Write-Output '===PODMAN===';",
    "if(Get-Command podman -ErrorAction SilentlyContinue){",
    "$oxidePodmanPs=& podman ps -a --no-trunc --format '",
    "{{json .}}\t{{.Status}}",
    "' 2>&1;",
    "if($LASTEXITCODE -eq 0){",
    "$oxidePodmanPs|ForEach-Object{Write-Output ('PS'+[char]9+$_)};",
    "podman pod ps --format '",
    "{{json .}}\t{{.Created}}",
    "' 2>$null|ForEach-Object{Write-Output ('POD'+[char]9+$_)}",
    "}else{",
    "$oxidePodmanFirst=($oxidePodmanPs|Select-Object -First 1) -replace [char]9,' ';",
    "if(\"$oxidePodmanFirst\" -match 'Cannot connect'){",
    "Write-Output ('__OXIDE_PODMAN_UNAVAILABLE__'+[char]9+$oxidePodmanFirst)",
    "}else{",
    "Write-Output ('__OXIDE_PODMAN_ERROR__'+[char]9+$oxidePodmanFirst)",
    "}",
    "}",
    "}else{Write-Output '__OXIDE_PODMAN_UNAVAILABLE__'};",
    "Write-Output '===PODMAN_END===';"
);

pub fn podman_sample_command(os_type: &str) -> &'static str {
    match os_type {
        "Windows" | "windows" => PODMAN_SAMPLE_COMMAND_WINDOWS,
        _ => PODMAN_SAMPLE_COMMAND_UNIX,
    }
}

pub fn parse_podman_snapshot(output: &str) -> ResourcePodmanSnapshot {
    let Some(section) = extract_section(output, "PODMAN") else {
        return ResourcePodmanSnapshot::default();
    };

    let mut containers = Vec::new();
    let mut pods = Vec::new();
    let mut row_lines = 0usize;
    for line in section
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        // Markers are recognized as standalone lines only so a crafted
        // container field can never forge a status.
        if line == PODMAN_UNAVAILABLE_MARKER {
            return ResourcePodmanSnapshot {
                status: ResourcePodmanStatus::Unavailable { detail: None },
                containers: Vec::new(),
                pods: Vec::new(),
            };
        }
        if let Some(detail) = line.strip_prefix(PODMAN_UNAVAILABLE_MARKER) {
            return ResourcePodmanSnapshot {
                status: ResourcePodmanStatus::Unavailable {
                    detail: clean_marker_detail(detail),
                },
                containers: Vec::new(),
                pods: Vec::new(),
            };
        }
        if let Some(message) = line.strip_prefix(PODMAN_ERROR_MARKER) {
            return ResourcePodmanSnapshot {
                status: ResourcePodmanStatus::Error {
                    message: clean_marker_detail(message)
                        .unwrap_or_else(|| "Podman command failed.".to_string()),
                },
                containers: Vec::new(),
                pods: Vec::new(),
            };
        }
        if let Some(pod) = parse_podman_pod_line(line) {
            pods.push(pod);
            continue;
        }
        if let Some(container) = parse_podman_container_line(line) {
            containers.push(container);
            continue;
        }
        if line.starts_with(PODMAN_ROW_PREFIX) {
            row_lines += 1;
        }
    }

    let status = if row_lines > 0 && containers.is_empty() {
        // The binary reported rows but none matched the expected JSON fields.
        ResourcePodmanStatus::Error {
            message: "Podman container output could not be parsed.".to_string(),
        }
    } else {
        ResourcePodmanStatus::Available
    };
    ResourcePodmanSnapshot {
        status,
        containers,
        pods,
    }
}

pub fn visible_podman_rows(
    containers: &[ResourcePodmanContainer],
    query: &str,
) -> Vec<ResourcePodmanContainer> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return containers.to_vec();
    }
    containers
        .iter()
        .filter(|container| podman_container_matches_query(container, &query))
        .cloned()
        .collect()
}

/// Produces a stable identity signature without treating sampled status as row geometry.
pub fn podman_row_signature(container: &ResourcePodmanContainer) -> u64 {
    let mut hasher = DefaultHasher::new();
    container.id.hash(&mut hasher);
    hasher.finish()
}

pub fn podman_state_label_key(state: &str) -> &'static str {
    match state.trim().to_lowercase().as_str() {
        "running" => "sidebar.host_podman.states.running",
        "exited" => "sidebar.host_podman.states.exited",
        "created" => "sidebar.host_podman.states.created",
        "paused" => "sidebar.host_podman.states.paused",
        "restarting" => "sidebar.host_podman.states.restarting",
        "removing" => "sidebar.host_podman.states.removing",
        "stopped" => "sidebar.host_podman.states.stopped",
        "dead" => "sidebar.host_podman.states.dead",
        _ => "sidebar.host_podman.states.unknown",
    }
}

fn parse_podman_container_line(line: &str) -> Option<ResourcePodmanContainer> {
    let payload = line.strip_prefix(PODMAN_ROW_PREFIX)?;
    // The sample command appends `{{.Status}}` after the JSON payload because
    // podman 5/6 leave the JSON `Status` field empty. JSON never contains a raw
    // tab, so the first tab is a safe separator.
    let (json_text, status_text) = match payload.split_once('\t') {
        Some((json, status)) => (json, Some(status.trim())),
        None => (payload, None),
    };
    let value = serde_json::from_str::<serde_json::Value>(json_text).ok()?;
    let id = clean_json_container_field(&value, "Id")
        .or_else(|| clean_json_container_field(&value, "ID"))?;
    if id.is_empty() {
        return None;
    }

    Some(ResourcePodmanContainer {
        id: id.to_string(),
        name: clean_json_names_field(&value).unwrap_or_else(|| id.to_string()),
        image: clean_json_container_field(&value, "Image").unwrap_or_else(|| "-".to_string()),
        state: clean_json_container_field(&value, "State").unwrap_or_else(|| "unknown".to_string()),
        status: status_text
            .filter(|status| !status.is_empty())
            .map(str::to_string)
            .or_else(|| clean_json_container_field(&value, "Status"))
            .unwrap_or_else(|| "-".to_string()),
        ports: podman_ports_text(&value),
    })
}

/// Parses one `podman pod ps` row: JSON plus the humanized `{{.Created}}`
/// column that podman only renders through a template field.
fn parse_podman_pod_line(line: &str) -> Option<ResourcePodmanPod> {
    let payload = line.strip_prefix(PODMAN_POD_PREFIX)?;
    let (json_text, created_text) = match payload.split_once('\t') {
        Some((json, created)) => (json, Some(created.trim())),
        None => (payload, None),
    };
    let value = serde_json::from_str::<serde_json::Value>(json_text).ok()?;
    let id = clean_json_container_field(&value, "Id")
        .or_else(|| clean_json_container_field(&value, "ID"))?;
    if id.is_empty() {
        return None;
    }
    let containers = value
        .get("Containers")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    Some(ResourcePodmanPod {
        id: id.to_string(),
        name: clean_json_container_field(&value, "Name").unwrap_or_else(|| id.to_string()),
        status: clean_json_container_field(&value, "Status")
            .unwrap_or_else(|| "unknown".to_string()),
        created: created_text
            .filter(|created| !created.is_empty())
            .map(str::to_string)
            .or_else(|| clean_json_container_field(&value, "Created"))
            .unwrap_or_else(|| "-".to_string()),
        containers,
    })
}

pub fn visible_podman_pods(pods: &[ResourcePodmanPod], query: &str) -> Vec<ResourcePodmanPod> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return pods.to_vec();
    }
    pods.iter()
        .filter(|pod| {
            pod.id.to_lowercase().contains(&query)
                || pod.name.to_lowercase().contains(&query)
                || pod.status.to_lowercase().contains(&query)
        })
        .cloned()
        .collect()
}

pub fn podman_pod_row_signature(pod: &ResourcePodmanPod) -> u64 {
    let mut hasher = DefaultHasher::new();
    pod.id.hash(&mut hasher);
    hasher.finish()
}

/// Pods report capitalized states (`Running`, `Exited`, `Degraded`).
pub fn podman_pod_state_label_key(state: &str) -> &'static str {
    match state.trim().to_lowercase().as_str() {
        "running" => "sidebar.host_podman.states.running",
        "exited" => "sidebar.host_podman.states.exited",
        "created" => "sidebar.host_podman.states.created",
        "paused" => "sidebar.host_podman.states.paused",
        "stopped" => "sidebar.host_podman.states.stopped",
        "degraded" => "sidebar.host_podman.states.degraded",
        "dead" => "sidebar.host_podman.states.dead",
        _ => "sidebar.host_podman.states.unknown",
    }
}

/// `podman ps --format '{{json .}}'` exposes `Names` as an array on podman 4/5
/// while scripts built on docker templates may surface a plain string.
fn clean_json_names_field(value: &serde_json::Value) -> Option<String> {
    match value.get("Names") {
        Some(serde_json::Value::Array(names)) => names
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(|name| name.trim().trim_start_matches('/').to_string())
            .find(|name| !name.is_empty()),
        Some(serde_json::Value::String(name)) => {
            let name = name.trim().trim_start_matches('/').to_string();
            (!name.is_empty()).then_some(name)
        }
        _ => None,
    }
}

/// Podman renders ports either as docker-style text (`0.0.0.0:8080->80/tcp`)
/// or, on podman 5/6, as an array of `{host_ip, host_port, container_port,
/// range, protocol}` objects. Both shapes collapse into one display string.
fn podman_ports_text(value: &serde_json::Value) -> Option<String> {
    match value.get("Ports") {
        Some(serde_json::Value::String(text)) => {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        }
        Some(serde_json::Value::Array(items)) => {
            let mut rendered = Vec::new();
            for item in items {
                let host_ip = item
                    .get("host_ip")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|ip| !ip.is_empty())
                    .unwrap_or("0.0.0.0");
                let host_port = item.get("host_port").and_then(serde_json::Value::as_u64);
                let container_port = item
                    .get("container_port")
                    .and_then(serde_json::Value::as_u64);
                let range = item
                    .get("range")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(1)
                    .max(1);
                let protocol = item
                    .get("protocol")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("tcp");
                match (host_port, container_port) {
                    (Some(host), Some(container)) if host > 0 && range == 1 => {
                        rendered.push(format!("{host_ip}:{host}->{container}/{protocol}"));
                    }
                    (Some(host), Some(container)) if host > 0 => {
                        rendered.push(format!(
                            "{host_ip}:{host}-{}->{container}-{}/{protocol}",
                            host.saturating_add(range - 1),
                            container.saturating_add(range - 1)
                        ));
                    }
                    (_, Some(container)) => {
                        rendered.push(format!("{container}/{protocol}"));
                    }
                    _ => {}
                }
            }
            (!rendered.is_empty()).then(|| rendered.join(", "))
        }
        _ => None,
    }
}

fn podman_container_matches_query(container: &ResourcePodmanContainer, query: &str) -> bool {
    container.id.to_lowercase().contains(query)
        || container.name.to_lowercase().contains(query)
        || container.image.to_lowercase().contains(query)
        || container.state.to_lowercase().contains(query)
        || container
            .ports
            .as_deref()
            .is_some_and(|ports| ports.to_lowercase().contains(query))
}

/// Remote command output is untrusted: keep the first line only, replace
/// control characters with spaces, and bound the length before it reaches UI
/// or diagnostics.
fn clean_marker_detail(value: &str) -> Option<String> {
    let line = value.trim_start_matches('\t').lines().next()?.trim();
    let cleaned: String = line
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(180)
        .collect();
    let cleaned = cleaned.trim();
    (!cleaned.is_empty()).then(|| cleaned.to_string())
}

fn clean_json_container_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn extract_section<'a>(output: &'a str, name: &str) -> Option<&'a str> {
    let start = format!("==={name}===");
    let end = format!("==={name}_END===");
    let after_start = output.split_once(&start)?.1;
    Some(after_start.split_once(&end)?.0.trim())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PodmanActionKind {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
    Remove,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PodmanActionAvailability {
    pub can_start: bool,
    pub can_stop: bool,
    pub can_restart: bool,
    pub can_pause: bool,
    pub can_unpause: bool,
    /// Removal is limited to containers that never ran or fully exited, so a
    /// destructive remove never sits behind a running workload.
    pub can_remove: bool,
    /// Live log reads tolerate a paused freezer; exec does not.
    pub can_use_live_tools: bool,
    pub can_exec: bool,
}

/// Podman reports never-started containers as `created` (or `configured` on
/// older releases) and stopped ones as `exited` or `stopped` depending on the
/// version. Unknown states expose no mutating affordances.
pub fn podman_action_availability(state: &str) -> PodmanActionAvailability {
    let state = state.trim().to_ascii_lowercase();
    let running = matches!(state.as_str(), "running" | "restarting");
    PodmanActionAvailability {
        can_start: matches!(
            state.as_str(),
            "created" | "configured" | "stopped" | "exited"
        ),
        can_stop: running,
        can_restart: running,
        can_pause: running,
        can_unpause: state == "paused",
        can_remove: matches!(
            state.as_str(),
            "exited" | "stopped" | "created" | "configured" | "dead"
        ),
        can_use_live_tools: running || state == "paused",
        can_exec: running,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PodmanActionCommand {
    pub command: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PodmanCaptureCommand {
    pub command: String,
}

pub fn build_podman_action_command(
    os_type: &str,
    container_id: &str,
    action: PodmanActionKind,
) -> Result<PodmanActionCommand, String> {
    let container_id = crate::docker::validated_container_id(container_id)?;
    let operation = match action {
        PodmanActionKind::Start => "start",
        PodmanActionKind::Stop => "stop",
        PodmanActionKind::Restart => "restart",
        PodmanActionKind::Pause => "pause",
        PodmanActionKind::Unpause => "unpause",
        PodmanActionKind::Remove => "rm",
    };
    // No sudo fallback: root's container store is a different dataset.
    let command = match os_type {
        "Windows" | "windows" => format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -Command \"podman {operation} {container_id}\""
        ),
        _ => format!("podman {operation} {container_id} 2>&1"),
    };
    Ok(PodmanActionCommand { command })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PodmanPodActionKind {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
    Remove,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PodmanPodActionAvailability {
    pub can_start: bool,
    pub can_stop: bool,
    pub can_restart: bool,
    pub can_pause: bool,
    pub can_unpause: bool,
    /// `podman pod rm` needs a stopped pod.
    pub can_remove: bool,
}

/// Degraded pods keep their workloads running but lose the infra container, so
/// only start/stop-style actions are offered and removal waits for a stop.
pub fn podman_pod_action_availability(status: &str) -> PodmanPodActionAvailability {
    let status = status.trim().to_ascii_lowercase();
    let running = matches!(status.as_str(), "running" | "degraded");
    PodmanPodActionAvailability {
        can_start: matches!(
            status.as_str(),
            "exited" | "stopped" | "created" | "degraded"
        ),
        can_stop: running,
        can_restart: running,
        can_pause: running && status == "running",
        can_unpause: status == "paused",
        can_remove: matches!(status.as_str(), "exited" | "stopped" | "created"),
    }
}

pub fn build_podman_pod_action_command(
    os_type: &str,
    pod_id: &str,
    action: PodmanPodActionKind,
) -> Result<PodmanActionCommand, String> {
    let pod_id = crate::docker::validated_container_id(pod_id)?;
    let operation = match action {
        PodmanPodActionKind::Start => "start",
        PodmanPodActionKind::Stop => "stop",
        PodmanPodActionKind::Restart => "restart",
        PodmanPodActionKind::Pause => "pause",
        PodmanPodActionKind::Unpause => "unpause",
        PodmanPodActionKind::Remove => "rm",
    };
    // No sudo fallback: root's container store is a different dataset.
    let command = match os_type {
        "Windows" | "windows" => format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -Command \"podman pod {operation} {pod_id}\""
        ),
        _ => format!("podman pod {operation} {pod_id} 2>&1"),
    };
    Ok(PodmanActionCommand { command })
}

pub fn build_podman_logs_command(
    os_type: &str,
    container_id: &str,
) -> Result<PodmanCaptureCommand, String> {
    let container_id = crate::docker::validated_container_id(container_id)?;
    let command = match os_type {
        "Windows" | "windows" => format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -Command \"podman logs --tail 200 --timestamps {container_id}\""
        ),
        _ => format!("podman logs --tail 200 --timestamps {container_id} 2>&1"),
    };
    Ok(PodmanCaptureCommand { command })
}

pub fn build_podman_exec_shell_command(container_id: &str) -> Result<String, String> {
    let container_id = crate::docker::validated_container_id(container_id)?;
    Ok(format!(
        "podman exec -it {container_id} sh -lc 'if command -v bash >/dev/null 2>&1; then exec bash; else exec sh; fi'"
    ))
}

pub fn build_podman_follow_logs_command(container_id: &str) -> Result<String, String> {
    let container_id = crate::docker::validated_container_id(container_id)?;
    Ok(format!(
        "podman logs -f --tail 200 --timestamps {container_id}"
    ))
}

/// Wraps a user-authored argument so it stays one argv element in a POSIX shell.
pub fn shell_quote_single(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn build_podman_rename_command(
    os_type: &str,
    container_id: &str,
    new_name: &str,
) -> Result<PodmanActionCommand, String> {
    let container_id = crate::docker::validated_container_id(container_id)?;
    let new_name = crate::podman_resources::validated_resource_name(new_name)?;
    let command = match os_type {
        "Windows" | "windows" => format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -Command \"podman rename {container_id} {new_name}\""
        ),
        _ => format!("podman rename {container_id} {new_name} 2>&1"),
    };
    Ok(PodmanActionCommand { command })
}

pub fn build_podman_healthcheck_command(
    os_type: &str,
    container_id: &str,
) -> Result<PodmanActionCommand, String> {
    let container_id = crate::docker::validated_container_id(container_id)?;
    let command = match os_type {
        "Windows" | "windows" => format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -Command \"podman healthcheck run {container_id}\""
        ),
        _ => format!("podman healthcheck run {container_id} 2>&1"),
    };
    Ok(PodmanActionCommand { command })
}

pub fn build_podman_inspect_command(
    os_type: &str,
    reference: &str,
) -> Result<PodmanCaptureCommand, String> {
    let reference = crate::podman_resources::validated_resource_name(reference)?;
    let command = match os_type {
        "Windows" | "windows" => format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -Command \"podman inspect {reference}\""
        ),
        _ => format!("podman inspect {reference} 2>&1"),
    };
    Ok(PodmanCaptureCommand { command })
}

pub fn build_podman_pod_inspect_command(
    os_type: &str,
    pod_id: &str,
) -> Result<PodmanCaptureCommand, String> {
    let pod_id = crate::docker::validated_container_id(pod_id)?;
    let command = match os_type {
        "Windows" | "windows" => format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -Command \"podman pod inspect {pod_id}\""
        ),
        _ => format!("podman pod inspect {pod_id} 2>&1"),
    };
    Ok(PodmanCaptureCommand { command })
}

pub fn build_podman_history_command(
    os_type: &str,
    image_id: &str,
) -> Result<PodmanCaptureCommand, String> {
    let image_id = crate::docker::validated_container_id(image_id)?;
    let command = match os_type {
        "Windows" | "windows" => format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -Command \"podman history {image_id}\""
        ),
        _ => format!("podman history {image_id} 2>&1"),
    };
    Ok(PodmanCaptureCommand { command })
}

pub fn build_podman_top_command(
    os_type: &str,
    container_id: &str,
) -> Result<PodmanCaptureCommand, String> {
    let container_id = crate::docker::validated_container_id(container_id)?;
    let command = match os_type {
        "Windows" | "windows" => format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -Command \"podman top {container_id}\""
        ),
        _ => format!("podman top {container_id} 2>&1"),
    };
    Ok(PodmanCaptureCommand { command })
}

pub fn build_podman_diff_command(
    os_type: &str,
    container_id: &str,
) -> Result<PodmanCaptureCommand, String> {
    let container_id = crate::docker::validated_container_id(container_id)?;
    let command = match os_type {
        "Windows" | "windows" => format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -Command \"podman diff {container_id}\""
        ),
        _ => format!("podman diff {container_id} 2>&1"),
    };
    Ok(PodmanCaptureCommand { command })
}

pub fn build_podman_volume_usage_command(
    os_type: &str,
    volume_name: &str,
) -> Result<PodmanCaptureCommand, String> {
    let volume_name = crate::podman_resources::validated_resource_name(volume_name)?;
    let command = match os_type {
        "Windows" | "windows" => format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -Command \"podman ps -a --filter volume={volume_name}\""
        ),
        _ => format!("podman ps -a --filter volume={volume_name} 2>&1"),
    };
    Ok(PodmanCaptureCommand { command })
}

/// Volume/network inspect need their dedicated subcommands: the bare
/// `podman inspect` lookup path behaves differently across podman releases
/// for non-container references.
pub fn build_podman_volume_inspect_command(
    os_type: &str,
    volume_name: &str,
) -> Result<PodmanCaptureCommand, String> {
    let volume_name = crate::podman_resources::validated_resource_name(volume_name)?;
    let command = match os_type {
        "Windows" | "windows" => format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -Command \"podman volume inspect {volume_name}\""
        ),
        _ => format!("podman volume inspect {volume_name} 2>&1"),
    };
    Ok(PodmanCaptureCommand { command })
}

pub fn build_podman_network_inspect_command(
    os_type: &str,
    network_name: &str,
) -> Result<PodmanCaptureCommand, String> {
    let network_name = crate::podman_resources::validated_resource_name(network_name)?;
    let command = match os_type {
        "Windows" | "windows" => format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -Command \"podman network inspect {network_name}\""
        ),
        _ => format!("podman network inspect {network_name} 2>&1"),
    };
    Ok(PodmanCaptureCommand { command })
}

pub fn podman_action_succeeded(exit_code: Option<i32>) -> bool {
    exit_code.unwrap_or(0) == 0
}

pub fn podman_action_success_message(stdout: &str, stderr: &str) -> String {
    crate::docker::compact_docker_command_message(stdout)
        .or_else(|| crate::docker::compact_docker_command_message(stderr))
        .unwrap_or_else(|| "Podman action completed.".to_string())
}

pub fn podman_action_failure_message(stdout: &str, stderr: &str, exit_code: Option<i32>) -> String {
    crate::docker::compact_docker_command_message(stderr)
        .or_else(|| crate::docker::compact_docker_command_message(stdout))
        .unwrap_or_else(|| {
            exit_code
                .map(|code| format!("Podman action failed with exit code {code}."))
                .unwrap_or_else(|| "Podman action failed.".to_string())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn podman_parser_preserves_container_fields() {
        for (case, output, expected) in [
            (
                "podman_json_rows",
                concat!(
                    "===PODMAN===\n",
                    "PS\t{\"Id\":\"a1b2c3d4e5f6\",\"Names\":[\"web\"],\"Image\":\"docker.io/library/nginx:alpine\",\"State\":\"running\",\"Status\":\"Up 2 minutes\"}\n",
                    "PS\t{\"Id\":\"f6e5d4c3b2a1\",\"Names\":[\"db\"],\"Image\":\"docker.io/library/postgres:16\",\"State\":\"exited\",\"Status\":\"Exited (0) 1 hour ago\"}\n",
                    "===PODMAN_END===",
                ),
                vec![
                    (
                        "a1b2c3d4e5f6",
                        "web",
                        "docker.io/library/nginx:alpine",
                        "running",
                        "Up 2 minutes",
                    ),
                    (
                        "f6e5d4c3b2a1",
                        "db",
                        "docker.io/library/postgres:16",
                        "exited",
                        "Exited (0) 1 hour ago",
                    ),
                ],
            ),
            (
                "docker_style_string_names_field",
                concat!(
                    "===PODMAN===\n",
                    "PS\t{\"Id\":\"a1b2c3d4e5f6\",\"Names\":\"web\",\"Image\":\"nginx:alpine\",\"State\":\"running\",\"Status\":\"Up 2 minutes\"}\n",
                    "===PODMAN_END===",
                ),
                vec![(
                    "a1b2c3d4e5f6",
                    "web",
                    "nginx:alpine",
                    "running",
                    "Up 2 minutes",
                )],
            ),
            (
                "empty_podman_host_is_available",
                "===PODMAN===\n===PODMAN_END===",
                vec![],
            ),
        ] {
            let snapshot = parse_podman_snapshot(output);
            assert_eq!(snapshot.status, ResourcePodmanStatus::Available, "{case}");
            assert_eq!(
                snapshot
                    .containers
                    .iter()
                    .map(|container| (
                        container.id.as_str(),
                        container.name.as_str(),
                        container.image.as_str(),
                        container.state.as_str(),
                        container.status.as_str(),
                    ))
                    .collect::<Vec<_>>(),
                expected,
                "{case}"
            );
        }
    }

    #[test]
    fn podman_parser_keeps_published_ports_and_search_matches_them() {
        let output = concat!(
            "===PODMAN===\n",
            "PS\t{\"Id\":\"a1b2c3d4e5f6\",\"Names\":[\"web\"],\"Image\":\"nginx:alpine\",",
            "\"State\":\"running\",\"Status\":\"Up 2 minutes\",",
            "\"Ports\":\"0.0.0.0:8080->80/tcp, 0.0.0.0:8443->443/tcp\"}\n",
            "PS\t{\"Id\":\"f6e5d4c3b2a1\",\"Names\":[\"db\"],\"Image\":\"postgres:16\",",
            "\"State\":\"exited\",\"Status\":\"Exited (0) 1 hour ago\"}\n",
            "===PODMAN_END===",
        );
        let snapshot = parse_podman_snapshot(output);
        assert_eq!(snapshot.status, ResourcePodmanStatus::Available);
        assert_eq!(
            snapshot.containers[0].ports.as_deref(),
            Some("0.0.0.0:8080->80/tcp, 0.0.0.0:8443->443/tcp")
        );
        assert_eq!(snapshot.containers[1].ports, None);

        // The search box looks at the same operational fields as docker.
        let matched = visible_podman_rows(&snapshot.containers, "8443");
        assert_eq!(
            matched
                .iter()
                .map(|row| row.name.as_str())
                .collect::<Vec<_>>(),
            vec!["web"]
        );
        assert!(visible_podman_rows(&snapshot.containers, "nope").is_empty());
    }

    #[test]
    fn podman_parser_reads_pods_with_created_and_container_count() {
        // Captured shape from `podman 6.1.4 pod ps --format '{{json .}}\t{{.Created}}'`.
        let output = concat!(
            "===PODMAN===\n",
            "POD\t{\"Id\":\"3cd7df7af038e334ea5eb404719eea184303544618b797d57887c41f6ddc9ed0\",",
            "\"Name\":\"pod_ldap\",\"Status\":\"Exited\",\"Created\":\"2026-09-01T20:04:03+08:00\",",
            "\"Containers\":[{\"Id\":\"552d85b062e9\",\"Names\":\"openldap\",\"Status\":\"exited\"}]}\t8 weeks ago\n",
            "===PODMAN_END===",
        );
        let snapshot = parse_podman_snapshot(output);
        assert_eq!(snapshot.status, ResourcePodmanStatus::Available);
        assert!(snapshot.containers.is_empty());
        let pod = &snapshot.pods[0];
        assert_eq!(pod.name, "pod_ldap");
        assert_eq!(pod.status, "Exited");
        // The template column carries the humanized age; JSON only has RFC3339.
        assert_eq!(pod.created, "8 weeks ago");
        assert_eq!(pod.containers, 1);

        // Pods are searchable by name/id/state like containers.
        assert_eq!(
            visible_podman_pods(&snapshot.pods, "ldap")
                .iter()
                .map(|pod| pod.name.as_str())
                .collect::<Vec<_>>(),
            vec!["pod_ldap"]
        );
        assert!(visible_podman_pods(&snapshot.pods, "missing").is_empty());
    }

    #[test]
    fn podman_pod_inspect_command_validates_the_id() {
        let id = "3cd7df7af038e334ea5eb404719eea184303544618b797d57887c41f6ddc9ed0";
        assert_eq!(
            build_podman_pod_inspect_command("Linux", id)
                .unwrap()
                .command,
            format!("podman pod inspect {id} 2>&1")
        );
        assert!(build_podman_pod_inspect_command("Linux", "abc").is_err());
    }

    #[test]
    fn podman_pod_actions_follow_the_pod_state() {
        for (status, expected) in [
            ("Running", (false, true, true, true, false, false)),
            ("Degraded", (true, true, true, false, false, false)),
            ("Exited", (true, false, false, false, false, true)),
            ("Paused", (false, false, false, false, true, false)),
            ("Created", (true, false, false, false, false, true)),
        ] {
            let availability = podman_pod_action_availability(status);
            assert_eq!(
                (
                    availability.can_start,
                    availability.can_stop,
                    availability.can_restart,
                    availability.can_pause,
                    availability.can_unpause,
                    availability.can_remove,
                ),
                expected,
                "{status}"
            );
        }
        // Same validation as containers: only full hex ids reach a command.
        let id = "3cd7df7af038e334ea5eb404719eea184303544618b797d57887c41f6ddc9ed0";
        assert_eq!(
            build_podman_pod_action_command("Linux", id, PodmanPodActionKind::Restart)
                .unwrap()
                .command,
            format!("podman pod restart {id} 2>&1")
        );
        assert_eq!(
            build_podman_pod_action_command("Windows", id, PodmanPodActionKind::Remove)
                .unwrap()
                .command,
            format!(
                "powershell -NoProfile -ExecutionPolicy Bypass -Command \"podman pod rm {id}\""
            )
        );
        assert!(
            build_podman_pod_action_command("Linux", "abc", PodmanPodActionKind::Stop).is_err()
        );
    }

    #[test]
    fn podman_parser_reports_unavailable_with_detail() {
        for (case, output, expected) in [
            (
                "missing_binary",
                "===PODMAN===\n__OXIDE_PODMAN_UNAVAILABLE__\n===PODMAN_END===",
                ResourcePodmanStatus::Unavailable { detail: None },
            ),
            (
                "stopped_rootless_socket",
                concat!(
                    "===PODMAN===\n",
                    "__OXIDE_PODMAN_UNAVAILABLE__\tCannot connect to Podman. Is the podman socket service running?\n",
                    "===PODMAN_END===",
                ),
                ResourcePodmanStatus::Unavailable {
                    detail: Some(
                        "Cannot connect to Podman. Is the podman socket service running?"
                            .to_string(),
                    ),
                },
            ),
        ] {
            assert_eq!(parse_podman_snapshot(output).status, expected, "{case}");
        }
    }

    #[test]
    fn podman_parser_sanitizes_error_messages() {
        let output = concat!(
            "===PODMAN===\n",
            "__OXIDE_PODMAN_ERROR__\tunexpected\u{7}output\twith tab\u{1b}escape\nsecond line\n",
            "===PODMAN_END===",
        );
        assert_eq!(
            parse_podman_snapshot(output).status,
            ResourcePodmanStatus::Error {
                message: "unexpected output with tab escape".to_string(),
            }
        );
    }

    #[test]
    fn podman_parser_rejects_forged_status_markers() {
        // A crafted container name must not flip the snapshot into a state.
        let output = concat!(
            "===PODMAN===\n",
            "PS\t{\"Id\":\"a1b2c3d4e5f6\",\"Names\":\"__OXIDE_PODMAN_UNAVAILABLE__\",\"Image\":\"nginx\",\"State\":\"running\",\"Status\":\"Up\"}\n",
            "===PODMAN_END===",
        );
        let snapshot = parse_podman_snapshot(output);
        assert_eq!(snapshot.status, ResourcePodmanStatus::Available);
        assert_eq!(snapshot.containers.len(), 1);
        assert_eq!(snapshot.containers[0].name, "__OXIDE_PODMAN_UNAVAILABLE__");
    }

    #[test]
    fn podman_parser_reports_unparsable_rows() {
        // Field renames across podman versions must not silently hide containers.
        let output = concat!(
            "===PODMAN===\n",
            "PS\t{\"ContainerID\":\"a1b2c3d4e5f6\"}\n",
            "===PODMAN_END===",
        );
        assert_eq!(
            parse_podman_snapshot(output).status,
            ResourcePodmanStatus::Error {
                message: "Podman container output could not be parsed.".to_string(),
            }
        );
    }

    #[test]
    fn podman_state_label_keys_cover_podman_lifecycle_states() {
        assert_eq!(
            podman_state_label_key("running"),
            "sidebar.host_podman.states.running"
        );
        assert_eq!(
            podman_state_label_key("Stopped"),
            "sidebar.host_podman.states.stopped"
        );
        // Docker surfaces restart/remove as their own states; podman reports them
        // through `podman ps` too, so the labels must exist here as well.
        assert_eq!(
            podman_state_label_key("restarting"),
            "sidebar.host_podman.states.restarting"
        );
        assert_eq!(
            podman_state_label_key("removing"),
            "sidebar.host_podman.states.removing"
        );
        assert_eq!(
            podman_state_label_key("mystery"),
            "sidebar.host_podman.states.unknown"
        );
    }

    #[test]
    fn podman_search_checks_operational_fields() {
        let snapshot = parse_podman_snapshot(concat!(
            "===PODMAN===\n",
            "PS\t{\"Id\":\"a1b2c3d4e5f6\",\"Names\":[\"web\"],\"Image\":\"nginx:alpine\",\"State\":\"running\",\"Status\":\"Up\"}\n",
            "PS\t{\"Id\":\"f6e5d4c3b2a1\",\"Names\":[\"db\"],\"Image\":\"postgres:16\",\"State\":\"exited\",\"Status\":\"Exited\"}\n",
            "===PODMAN_END===",
        ));

        for (query, expected) in [
            ("nginx", vec!["web"]),
            ("exited", vec!["db"]),
            ("missing", vec![]),
            ("", vec!["web", "db"]),
        ] {
            let rows = visible_podman_rows(&snapshot.containers, query);
            assert_eq!(
                rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
                expected,
                "{query}"
            );
        }
    }

    #[test]
    fn podman_action_availability_maps_podman_states() {
        let running = podman_action_availability("Running");
        assert!(running.can_stop && running.can_restart && running.can_pause);
        assert!(running.can_exec && running.can_use_live_tools);
        assert!(!running.can_start && !running.can_unpause && !running.can_remove);

        let stopped = podman_action_availability("stopped");
        assert!(stopped.can_start && stopped.can_remove);
        assert!(!stopped.can_stop && !stopped.can_exec && !stopped.can_use_live_tools);

        let paused = podman_action_availability("paused");
        assert!(paused.can_unpause && paused.can_use_live_tools);
        assert!(!paused.can_exec && !paused.can_pause && !paused.can_remove);

        let configured = podman_action_availability("configured");
        assert!(configured.can_start && configured.can_remove);

        let unknown = podman_action_availability("mystery");
        assert!(
            !unknown.can_start && !unknown.can_stop && !unknown.can_remove && !unknown.can_exec
        );
    }

    #[test]
    fn podman_action_commands_validate_ids_without_sudo() {
        let pause =
            build_podman_action_command("Linux", "abc123def456", PodmanActionKind::Pause).unwrap();
        assert!(pause.command.contains("podman pause abc123def456"));
        let remove =
            build_podman_action_command("Linux", "abc123def456", PodmanActionKind::Remove).unwrap();
        assert!(remove.command.contains("podman rm abc123def456"));
        assert!(
            build_podman_action_command("Linux", "abc123;rm -rf /", PodmanActionKind::Stop)
                .is_err()
        );
        // Root's container store is a different dataset; actions never escalate.
        assert!(!pause.command.contains("sudo"));

        let logs = build_podman_logs_command("Linux", "abc123def456").unwrap();
        assert!(
            logs.command
                .contains("podman logs --tail 200 --timestamps abc123def456")
        );
        let exec = build_podman_exec_shell_command("abc123def456").unwrap();
        assert!(exec.contains("podman exec -it abc123def456"));
        assert!(build_podman_exec_shell_command("abc123;bad").is_err());
        let follow = build_podman_follow_logs_command("abc123def456").unwrap();
        assert_eq!(
            follow,
            "podman logs -f --tail 200 --timestamps abc123def456"
        );
    }

    #[test]
    fn podman_parser_reads_a_real_podman_6_row() {
        // Captured from `podman 6.1.4 ps --format '{{json .}}\t{{.Status}}'`
        // for a redis container published on 16379.
        let row = concat!(
            "{\"Id\":\"c6d920d1533d\",\"Names\":[\"oxide-redis-probe\"],",
            "\"Image\":\"docker.io/library/redis:7-alpine\",\"State\":\"running\",",
            "\"Status\":\"\",\"Ports\":[{\"host_ip\":\"\",\"container_port\":6379,",
            "\"host_port\":16379,\"range\":1,\"protocol\":\"tcp\"}]}"
        );
        let snapshot = parse_podman_snapshot(&format!(
            "===PODMAN===\nPS\t{row}\tUp 4 minutes\n===PODMAN_END==="
        ));
        let container = &snapshot.containers[0];
        assert_eq!(container.name, "oxide-redis-probe");
        assert_eq!(container.state, "running");
        assert_eq!(container.status, "Up 4 minutes");
        assert_eq!(container.ports.as_deref(), Some("0.0.0.0:16379->6379/tcp"));
    }

    #[test]
    fn podman_ports_render_from_array_and_string_shapes() {
        // podman 5/6: `Ports` is an array of mapping objects.
        let array_row = concat!(
            "{\"Id\":\"a1b2c3d4e5f6\",\"Names\":[\"web\"],\"Image\":\"nginx:alpine\",",
            "\"State\":\"running\",\"Status\":\"\",",
            "\"Ports\":[{\"host_ip\":\"0.0.0.0\",\"container_port\":80,\"host_port\":18080,\"range\":1,\"protocol\":\"tcp\"},",
            "{\"host_ip\":\"\",\"container_port\":443,\"host_port\":0,\"range\":1,\"protocol\":\"tcp\"}]}"
        );
        let snapshot = parse_podman_snapshot(&format!(
            "===PODMAN===\nPS\t{array_row}\tUp 8 seconds\n===PODMAN_END==="
        ));
        let container = &snapshot.containers[0];
        // Docker-style text matches what the Docker tool prints for the column.
        assert_eq!(
            container.ports.as_deref(),
            Some("0.0.0.0:18080->80/tcp, 443/tcp")
        );
        // The appended template column supplies the status podman omits in JSON.
        assert_eq!(container.status, "Up 8 seconds");

        // Older podman/docker-style string ports keep their text verbatim.
        let string_row = concat!(
            "{\"Id\":\"f6e5d4c3b2a1\",\"Names\":\"db\",\"Image\":\"postgres:16\",",
            "\"State\":\"exited\",\"Status\":\"Exited (0) 1 hour ago\",",
            "\"Ports\":\"0.0.0.0:5432->5432/tcp\"}"
        );
        let snapshot =
            parse_podman_snapshot(&format!("===PODMAN===\nPS\t{string_row}\n===PODMAN_END==="));
        let container = &snapshot.containers[0];
        assert_eq!(container.ports.as_deref(), Some("0.0.0.0:5432->5432/tcp"));
        assert_eq!(container.status, "Exited (0) 1 hour ago");
    }

    #[test]
    fn podman_sample_command_keeps_machine_readable_format_without_sudo() {
        let expected_format = "{{json .}}\t{{.Status}}";
        assert!(podman_sample_command("Linux").contains(expected_format));
        assert!(podman_sample_command("Windows").contains(expected_format));
        assert!(podman_sample_command("Linux").contains("--no-trunc"));
        assert!(podman_sample_command("Windows").contains("--no-trunc"));
        assert!(podman_sample_command("Linux").contains(PODMAN_UNAVAILABLE_MARKER));
        assert!(podman_sample_command("Windows").contains(PODMAN_UNAVAILABLE_MARKER));
        // Root's container store is a different dataset; never switch to it.
        assert!(!podman_sample_command("Linux").contains("sudo"));
        assert!(!podman_sample_command("Windows").contains("sudo"));
    }
}
