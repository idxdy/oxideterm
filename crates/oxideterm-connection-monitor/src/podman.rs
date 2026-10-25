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
}

const PODMAN_UNAVAILABLE_MARKER: &str = "__OXIDE_PODMAN_UNAVAILABLE__";
const PODMAN_ERROR_MARKER: &str = "__OXIDE_PODMAN_ERROR__";
pub(crate) const PODMAN_ROW_PREFIX: &str = "PS\t";

// Unlike docker, a failing `podman ps` must never be retried with sudo:
// `sudo podman ps` reads root's container store instead of the invoking
// user's rootless store, silently switching the data source.
const PODMAN_SAMPLE_COMMAND_UNIX: &str = concat!(
    "echo '===PODMAN==='; ",
    "if command -v podman >/dev/null 2>&1; then ",
    "oxide_podman_ps=$(podman ps -a --no-trunc --format '",
    "{{json .}}",
    "' 2>&1); ",
    "oxide_podman_status=$?; ",
    "if [ \"$oxide_podman_status\" -eq 0 ]; then ",
    "printf '%s\\n' \"$oxide_podman_ps\" | sed 's/^/PS\\t/'; ",
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
    "{{json .}}",
    "' 2>&1;",
    "if($LASTEXITCODE -eq 0){",
    "$oxidePodmanPs|ForEach-Object{Write-Output ('PS'+[char]9+$_)}",
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
            };
        }
        if let Some(detail) = line.strip_prefix(PODMAN_UNAVAILABLE_MARKER) {
            return ResourcePodmanSnapshot {
                status: ResourcePodmanStatus::Unavailable {
                    detail: clean_marker_detail(detail),
                },
                containers: Vec::new(),
            };
        }
        if let Some(message) = line.strip_prefix(PODMAN_ERROR_MARKER) {
            return ResourcePodmanSnapshot {
                status: ResourcePodmanStatus::Error {
                    message: clean_marker_detail(message)
                        .unwrap_or_else(|| "Podman command failed.".to_string()),
                },
                containers: Vec::new(),
            };
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
    ResourcePodmanSnapshot { status, containers }
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
        "stopped" => "sidebar.host_podman.states.stopped",
        "dead" => "sidebar.host_podman.states.dead",
        _ => "sidebar.host_podman.states.unknown",
    }
}

fn parse_podman_container_line(line: &str) -> Option<ResourcePodmanContainer> {
    let payload = line.strip_prefix(PODMAN_ROW_PREFIX)?;
    let value = serde_json::from_str::<serde_json::Value>(payload).ok()?;
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
        status: clean_json_container_field(&value, "Status").unwrap_or_else(|| "-".to_string()),
    })
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

fn podman_container_matches_query(container: &ResourcePodmanContainer, query: &str) -> bool {
    container.id.to_lowercase().contains(query)
        || container.name.to_lowercase().contains(query)
        || container.image.to_lowercase().contains(query)
        || container.state.to_lowercase().contains(query)
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
    fn podman_sample_command_keeps_machine_readable_format_without_sudo() {
        let expected_format = "{{json .}}";
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
