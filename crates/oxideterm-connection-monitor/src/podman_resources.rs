use serde::{Deserialize, Serialize};

use crate::podman::{PodmanActionCommand, shell_quote_single};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePodmanImage {
    pub repository: String,
    pub tag: String,
    pub image_id: String,
    pub size: String,
    pub created: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePodmanImagesSnapshot {
    pub images: Vec<ResourcePodmanImage>,
    pub unavailable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePodmanVolume {
    pub name: String,
    pub driver: String,
    pub mount_point: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePodmanVolumesSnapshot {
    pub volumes: Vec<ResourcePodmanVolume>,
    pub unavailable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePodmanNetwork {
    pub name: String,
    pub driver: String,
    pub interface: String,
    pub subnets: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePodmanNetworksSnapshot {
    pub networks: Vec<ResourcePodmanNetwork>,
    pub unavailable: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePodmanStat {
    pub container_id: String,
    pub name: String,
    pub cpu_percent: String,
    pub mem_usage: String,
    pub mem_percent: String,
    pub net_io: String,
    pub block_io: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePodmanStatsSnapshot {
    pub stats: Vec<ResourcePodmanStat>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePodmanSearchResult {
    pub name: String,
    pub description: String,
    pub stars: String,
}

const PODMAN_RESOURCES_UNAVAILABLE_MARKER: &str = "__OXIDE_PODMAN_UNAVAILABLE__";

// One SSH exec fills all three slow-changing inventories; containers keep
// riding the continuous sampler on their own.
const PODMAN_RESOURCES_SNAPSHOT_COMMAND_UNIX: &str = concat!(
    "echo '===PODMAN_IMAGES==='; ",
    "if command -v podman >/dev/null 2>&1; then ",
    "podman images --format '",
    "{{json .}}",
    "' 2>/dev/null | sed 's/^/IMG\\t/'; ",
    "echo '===PODMAN_IMAGES_END==='; ",
    "echo '===PODMAN_VOLUMES==='; ",
    "podman volume ls --format '",
    "{{json .}}",
    "' 2>/dev/null | sed 's/^/VOL\\t/'; ",
    "echo '===PODMAN_VOLUMES_END==='; ",
    "echo '===PODMAN_NETWORKS==='; ",
    "podman network ls --format '",
    "{{json .}}",
    "' 2>/dev/null | sed 's/^/NET\\t/'; ",
    "echo '===PODMAN_NETWORKS_END==='; ",
    "else echo '===PODMAN_RESOURCES==='; echo '__OXIDE_PODMAN_UNAVAILABLE__'; echo '===PODMAN_RESOURCES_END==='; fi; ",
    "echo '===PODMAN_RESOURCES_END==='"
);

const PODMAN_RESOURCES_SNAPSHOT_COMMAND_WINDOWS: &str = concat!(
    "Write-Output '===PODMAN_IMAGES===';",
    "if(Get-Command podman -ErrorAction SilentlyContinue){",
    "podman images --format '",
    "{{json .}}",
    "' 2>$null|ForEach-Object{Write-Output ('IMG'+[char]9+$_)};",
    "Write-Output '===PODMAN_IMAGES_END===';",
    "Write-Output '===PODMAN_VOLUMES===';",
    "podman volume ls --format '",
    "{{json .}}",
    "' 2>$null|ForEach-Object{Write-Output ('VOL'+[char]9+$_)};",
    "Write-Output '===PODMAN_VOLUMES_END===';",
    "Write-Output '===PODMAN_NETWORKS===';",
    "podman network ls --format '",
    "{{json .}}",
    "' 2>$null|ForEach-Object{Write-Output ('NET'+[char]9+$_)};",
    "Write-Output '===PODMAN_NETWORKS_END===';",
    "}else{Write-Output '===PODMAN_RESOURCES===';Write-Output '__OXIDE_PODMAN_UNAVAILABLE__';Write-Output '===PODMAN_RESOURCES_END==='};",
    "Write-Output '===PODMAN_RESOURCES_END===';"
);

pub fn podman_resources_snapshot_command(os_type: &str) -> &'static str {
    match os_type {
        "Windows" | "windows" => PODMAN_RESOURCES_SNAPSHOT_COMMAND_WINDOWS,
        _ => PODMAN_RESOURCES_SNAPSHOT_COMMAND_UNIX,
    }
}

// `--no-stream` keeps stats a bounded one-shot table instead of a live stream.
const PODMAN_STATS_COMMAND_UNIX: &str = concat!(
    "echo '===PODMAN_STATS==='; ",
    "podman stats --no-stream --format '",
    "{{json .}}",
    "' 2>/dev/null | sed 's/^/STAT\\t/'; ",
    "echo '===PODMAN_STATS_END==='"
);

const PODMAN_STATS_COMMAND_WINDOWS: &str = concat!(
    "Write-Output '===PODMAN_STATS===';",
    "podman stats --no-stream --format '",
    "{{json .}}",
    "' 2>$null|ForEach-Object{Write-Output ('STAT'+[char]9+$_)};",
    "Write-Output '===PODMAN_STATS_END===';"
);

pub fn podman_stats_command(os_type: &str) -> &'static str {
    match os_type {
        "Windows" | "windows" => PODMAN_STATS_COMMAND_WINDOWS,
        _ => PODMAN_STATS_COMMAND_UNIX,
    }
}

/// Search results feed human reading only: the remote output is capped before
/// it reaches the table and descriptions are bounded by the parser.
pub fn build_podman_search_command(term: &str) -> Result<String, String> {
    let term = validated_resource_name(term)?;
    Ok(format!(
        "echo '===PODMAN_SEARCH==='; podman search --limit 20 --format '{{json .}}' {} 2>/dev/null | sed 's/^/SEARCH\\t/'; echo '===PODMAN_SEARCH_END==='",
        shell_quote_single(term)
    ))
}

/// Wraps a remote path so it stays one argv element in the composed command.
fn validated_resource_path(path: &str) -> Result<&str, String> {
    let path = path.trim();
    if path.is_empty() || path.len() > 1024 {
        return Err("Invalid Podman path.".to_string());
    }
    Ok(path)
}

/// One-shot resource mutation; the caller owns confirm dialogs and auditing.
pub fn build_podman_resource_action_command(
    os_type: &str,
    action: PodmanResourceAction,
) -> Result<PodmanActionCommand, String> {
    fn posix(os_type: &str, command: String) -> String {
        match os_type {
            "Windows" | "windows" => {
                format!("powershell -NoProfile -ExecutionPolicy Bypass -Command \"{command}\"")
            }
            _ => format!("{command} 2>&1"),
        }
    }
    let command = match action {
        PodmanResourceAction::RemoveImage { image_id } => {
            let id = crate::docker::validated_container_id(&image_id)?;
            format!("podman rmi {id}")
        }
        PodmanResourceAction::PruneDanglingImages => "podman image prune -f".to_string(),
        PodmanResourceAction::PruneAllUnusedImages => "podman image prune -a -f".to_string(),
        PodmanResourceAction::TagImage {
            image_id,
            reference,
        } => {
            let id = crate::docker::validated_container_id(&image_id)?;
            let reference = validated_resource_name(&reference)?;
            format!("podman tag {id} {reference}")
        }
        PodmanResourceAction::RenameContainer {
            container_id,
            new_name,
        } => {
            let id = crate::docker::validated_container_id(&container_id)?;
            let new_name = validated_resource_name(&new_name)?;
            format!("podman rename {id} {new_name}")
        }
        PodmanResourceAction::RunHealthcheck { container_id } => {
            let id = crate::docker::validated_container_id(&container_id)?;
            format!("podman healthcheck run {id}")
        }
        PodmanResourceAction::CreateVolume { name } => {
            let name = validated_resource_name(&name)?;
            format!("podman volume create {name}")
        }
        PodmanResourceAction::RemoveVolume { name } => {
            let name = validated_resource_name(&name)?;
            format!("podman volume rm {name}")
        }
        PodmanResourceAction::PruneVolumes => "podman volume prune -f".to_string(),
        PodmanResourceAction::CreateNetwork { name, driver } => {
            let name = validated_resource_name(&name)?;
            match driver.trim().is_empty() {
                true => format!("podman network create {name}"),
                false => {
                    let driver = validated_resource_name(driver.trim())?;
                    format!("podman network create --driver {driver} {name}")
                }
            }
        }
        PodmanResourceAction::RemoveNetwork { name } => {
            let name = validated_resource_name(&name)?;
            format!("podman network rm {name}")
        }
        PodmanResourceAction::Logout { registry } => {
            let registry = validated_resource_name(&registry)?;
            format!("podman logout {registry}")
        }
    };
    Ok(PodmanActionCommand {
        command: posix(os_type, command),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PodmanResourceAction {
    RemoveImage {
        image_id: String,
    },
    /// `podman image prune -f`: dangling (untagged) images only.
    PruneDanglingImages,
    /// `podman image prune -a -f`: every image without a running container.
    PruneAllUnusedImages,
    TagImage {
        image_id: String,
        reference: String,
    },
    RenameContainer {
        container_id: String,
        new_name: String,
    },
    RunHealthcheck {
        container_id: String,
    },
    CreateVolume {
        name: String,
    },
    RemoveVolume {
        name: String,
    },
    PruneVolumes,
    CreateNetwork {
        name: String,
        driver: String,
    },
    RemoveNetwork {
        name: String,
    },
    Logout {
        registry: String,
    },
}

/// Composes an interactive terminal command. The user's own terminal session
/// owns the stream; the app never captures or audits these command lines.
pub enum PodmanTerminalOp {
    Pull { reference: String },
    Push { reference: String },
    Build { args: String },
    Run { args: String },
    Load { path: String },
    Save { path: String, reference: String },
    Export { path: String, container_id: String },
    Import { path: String, reference: String },
    Copy { source: String, destination: String },
    Attach { container_id: String },
    Login { registry: String },
}

pub fn build_podman_attach_command(container_id: &str) -> Result<String, String> {
    build_podman_terminal_command(PodmanTerminalOp::Attach {
        container_id: container_id.to_string(),
    })
}

pub fn build_podman_terminal_command(op: PodmanTerminalOp) -> Result<String, String> {
    Ok(match op {
        PodmanTerminalOp::Pull { reference } => {
            let reference = validated_resource_name(&reference)?;
            format!("podman pull {}", shell_quote_single(reference))
        }
        PodmanTerminalOp::Push { reference } => {
            let reference = validated_resource_name(&reference)?;
            format!("podman push {}", shell_quote_single(reference))
        }
        PodmanTerminalOp::Build { args } => {
            let args = args.trim();
            if args.is_empty() {
                return Err("Podman build arguments are required.".to_string());
            }
            format!("podman build {args}")
        }
        PodmanTerminalOp::Run { args } => {
            let args = args.trim();
            if args.is_empty() {
                return Err("Podman run arguments are required.".to_string());
            }
            format!("podman run {args}")
        }
        PodmanTerminalOp::Load { path } => {
            let path = validated_resource_path(&path)?;
            format!("podman load -i {}", shell_quote_single(path))
        }
        PodmanTerminalOp::Save { path, reference } => {
            let path = validated_resource_path(&path)?;
            let reference = validated_resource_name(&reference)?;
            format!(
                "podman save -o {} {}",
                shell_quote_single(path),
                shell_quote_single(reference)
            )
        }
        PodmanTerminalOp::Export { path, container_id } => {
            let path = validated_resource_path(&path)?;
            let id = crate::docker::validated_container_id(&container_id)?;
            format!("podman export -o {} {id}", shell_quote_single(path))
        }
        PodmanTerminalOp::Import { path, reference } => {
            let path = validated_resource_path(&path)?;
            let reference = validated_resource_name(&reference)?;
            format!(
                "podman import {} {}",
                shell_quote_single(path),
                shell_quote_single(reference)
            )
        }
        PodmanTerminalOp::Copy {
            source,
            destination,
        } => {
            let source = validated_resource_path(&source)?;
            let destination = validated_resource_path(&destination)?;
            format!(
                "podman cp {} {}",
                shell_quote_single(source),
                shell_quote_single(destination)
            )
        }
        PodmanTerminalOp::Attach { container_id } => {
            let id = crate::docker::validated_container_id(&container_id)?;
            format!("podman attach {id}")
        }
        PodmanTerminalOp::Login { registry } => {
            let registry = validated_resource_name(&registry)?;
            // Interactive: the password goes straight to the TTY prompt and
            // never passes through the application.
            format!("podman login {}", shell_quote_single(registry))
        }
    })
}

fn bounded_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(180).collect())
}

/// Podman's image JSON drifts across releases: repo/tag can be plain strings,
/// arrays of strings, or missing; size/created can be numbers. Accepts the
/// first present shape and bounds the text before it reaches the UI.
fn image_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        let field = value.get(key)?;
        let text = match field {
            serde_json::Value::String(text) => Some(text.clone()),
            serde_json::Value::Number(number) => Some(number.to_string()),
            serde_json::Value::Array(items) => items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .next()
                .map(str::to_string),
            _ => None,
        };
        if let Some(text) = text {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.chars().take(180).collect());
            }
        }
    }
    None
}

fn parse_prefixed_json<T>(
    section: &str,
    prefix: &str,
    build: impl Fn(&serde_json::Value) -> Option<T>,
) -> Vec<T> {
    section
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            let payload = line.strip_prefix(prefix)?;
            let value = serde_json::from_str::<serde_json::Value>(payload).ok()?;
            build(&value)
        })
        .collect()
}

pub fn parse_podman_resources_snapshot(
    output: &str,
) -> (
    ResourcePodmanImagesSnapshot,
    ResourcePodmanVolumesSnapshot,
    ResourcePodmanNetworksSnapshot,
) {
    let images_section = extract_section(output, "PODMAN_IMAGES");
    let volumes_section = extract_section(output, "PODMAN_VOLUMES");
    let networks_section = extract_section(output, "PODMAN_NETWORKS");
    let unavailable = extract_section(output, "PODMAN_RESOURCES")
        .is_some_and(|section| section.trim() == PODMAN_RESOURCES_UNAVAILABLE_MARKER);

    let images = ResourcePodmanImagesSnapshot {
        images: images_section
            .map(|section| {
                parse_prefixed_json(section, "IMG\t", |value| {
                    let id = bounded_field(value, "Id")?;
                    Some(ResourcePodmanImage {
                        repository: image_field(value, &["Repository", "repository"])
                            .unwrap_or_else(|| "<none>".to_string()),
                        tag: image_field(value, &["Tag", "tag"])
                            .unwrap_or_else(|| "<none>".to_string()),
                        image_id: id.trim_start_matches("sha256:").chars().take(12).collect(),
                        size: image_field(value, &["Size", "size"])
                            .unwrap_or_else(|| "-".to_string()),
                        created: image_field(value, &["CreatedTime", "Created", "created"])
                            .unwrap_or_else(|| "-".to_string()),
                    })
                })
            })
            .unwrap_or_default(),
        unavailable: unavailable,
    };
    let volumes = ResourcePodmanVolumesSnapshot {
        volumes: volumes_section
            .map(|section| {
                parse_prefixed_json(section, "VOL\t", |value| {
                    let name = bounded_field(value, "Name")?;
                    Some(ResourcePodmanVolume {
                        name,
                        driver: bounded_field(value, "Driver").unwrap_or_else(|| "-".to_string()),
                        mount_point: bounded_field(value, "Mountpoint")
                            .unwrap_or_else(|| "-".to_string()),
                    })
                })
            })
            .unwrap_or_default(),
        unavailable: unavailable,
    };
    let networks = ResourcePodmanNetworksSnapshot {
        networks: networks_section
            .map(|section| {
                parse_prefixed_json(section, "NET\t", |value| {
                    let name =
                        bounded_field(value, "name").or_else(|| bounded_field(value, "Name"))?;
                    let subnets = value
                        .get("subnets")
                        .and_then(serde_json::Value::as_array)
                        .map(|subnets| {
                            subnets
                                .iter()
                                .filter_map(|subnet| {
                                    subnet.get("subnet").and_then(serde_json::Value::as_str)
                                })
                                .map(|subnet| subnet.chars().take(64).collect::<String>())
                                .collect()
                        })
                        .unwrap_or_default();
                    Some(ResourcePodmanNetwork {
                        name,
                        driver: bounded_field(value, "driver")
                            .or_else(|| bounded_field(value, "Driver"))
                            .unwrap_or_else(|| "-".to_string()),
                        interface: bounded_field(value, "network_interface")
                            .or_else(|| bounded_field(value, "NetworkInterface"))
                            .unwrap_or_else(|| "-".to_string()),
                        subnets,
                    })
                })
            })
            .unwrap_or_default(),
        unavailable: unavailable,
    };
    (images, volumes, networks)
}

pub fn parse_podman_stats_snapshot(output: &str) -> ResourcePodmanStatsSnapshot {
    let Some(section) = extract_section(output, "PODMAN_STATS") else {
        return ResourcePodmanStatsSnapshot::default();
    };
    ResourcePodmanStatsSnapshot {
        stats: parse_prefixed_json(section, "STAT\t", |value| {
            // Field names drift across podman releases; tolerant access keeps
            // the table renderable with whatever subset is present.
            let id = bounded_field(value, "CID").or_else(|| bounded_field(value, "ID"))?;
            Some(ResourcePodmanStat {
                container_id: id.chars().take(12).collect(),
                name: bounded_field(value, "NAME")
                    .or_else(|| bounded_field(value, "Name"))
                    .unwrap_or_else(|| "-".to_string()),
                cpu_percent: bounded_field(value, "CPU%")
                    .or_else(|| bounded_field(value, "CPUPerc"))
                    .unwrap_or_else(|| "-".to_string()),
                mem_usage: bounded_field(value, "MEM USAGE / LIMIT")
                    .or_else(|| bounded_field(value, "MemUsage"))
                    .unwrap_or_else(|| "-".to_string()),
                mem_percent: bounded_field(value, "MEM%")
                    .or_else(|| bounded_field(value, "MemPerc"))
                    .unwrap_or_else(|| "-".to_string()),
                net_io: bounded_field(value, "NET I/O")
                    .or_else(|| bounded_field(value, "NetIO"))
                    .unwrap_or_else(|| "-".to_string()),
                block_io: bounded_field(value, "BLOCK I/O")
                    .or_else(|| bounded_field(value, "BlockIO"))
                    .unwrap_or_else(|| "-".to_string()),
            })
        }),
    }
}

pub fn parse_podman_search_results(output: &str) -> Vec<ResourcePodmanSearchResult> {
    let Some(section) = extract_section(output, "PODMAN_SEARCH") else {
        return Vec::new();
    };
    parse_prefixed_json(section, "SEARCH\t", |value| {
        let name = bounded_field(value, "Name")?;
        Some(ResourcePodmanSearchResult {
            name,
            description: bounded_field(value, "Description").unwrap_or_default(),
            stars: value
                .get("Stars")
                .and_then(serde_json::Value::as_u64)
                .map(|stars| stars.to_string())
                .unwrap_or_else(|| "-".to_string()),
        })
    })
}

pub fn validated_resource_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty()
        || name.len() > 128
        || !name.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '_' | '.' | '-' | ':' | '/' | '@')
        })
    {
        return Err("Invalid Podman resource reference.".to_string());
    }
    Ok(name)
}

fn extract_section<'a>(output: &'a str, name: &str) -> Option<&'a str> {
    let start = format!("==={name}===");
    let end = format!("==={name}_END===");
    let after_start = output.split_once(&start)?.1;
    Some(after_start.split_once(&end)?.0.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resources_snapshot_parser_splits_inventories() {
        let output = concat!(
            "===PODMAN_IMAGES===\n",
            "IMG\t{\"Id\":\"a1b2c3d4e5f6\",\"Repository\":\"docker.io/library/nginx\",\"Tag\":\"alpine\",\"Size\":\"19.4 MB\",\"CreatedTime\":\"2 weeks ago\"}\n",
            "===PODMAN_IMAGES_END===\n",
            "===PODMAN_VOLUMES===\n",
            "VOL\t{\"Name\":\"app-data\",\"Driver\":\"local\",\"Mountpoint\":\"/var/lib/containers/storage/volumes/app-data/_data\"}\n",
            "===PODMAN_VOLUMES_END===\n",
            "===PODMAN_NETWORKS===\n",
            "NET\t{\"name\":\"podman\",\"driver\":\"bridge\",\"network_interface\":\"podman0\",\"subnets\":[{\"subnet\":\"10.88.0.0/16\",\"gateway\":\"10.88.0.1\"}]}\n",
            "===PODMAN_NETWORKS_END===",
        );
        let (images, volumes, networks) = parse_podman_resources_snapshot(output);
        assert_eq!(images.images.len(), 1);
        assert_eq!(images.images[0].repository, "docker.io/library/nginx");
        assert_eq!(images.images[0].image_id, "a1b2c3d4e5f6");
        assert_eq!(volumes.volumes[0].name, "app-data");
        assert!(volumes.volumes[0].mount_point.contains("/_data"));
        assert_eq!(networks.networks[0].name, "podman");
        assert_eq!(networks.networks[0].subnets, vec!["10.88.0.0/16"]);
    }

    #[test]
    fn resources_snapshot_parser_reports_missing_binary() {
        let output = concat!(
            "===PODMAN_IMAGES===\n",
            "===PODMAN_IMAGES_END===\n",
            "===PODMAN_VOLUMES===\n",
            "===PODMAN_VOLUMES_END===\n",
            "===PODMAN_NETWORKS===\n",
            "===PODMAN_NETWORKS_END===\n",
            "===PODMAN_RESOURCES===\n",
            "__OXIDE_PODMAN_UNAVAILABLE__\n",
            "===PODMAN_RESOURCES_END===",
        );
        let (images, volumes, networks) = parse_podman_resources_snapshot(output);
        assert!(images.unavailable);
        assert!(images.images.is_empty());
        assert!(volumes.unavailable);
        assert!(networks.unavailable);
    }

    #[test]
    fn stats_and_search_parsers_bound_remote_output() {
        let stats_output = concat!(
            "===PODMAN_STATS===\n",
            "STAT\t{\"CID\":\"a1b2c3d4e5f67890\",\"NAME\":\"web\",\"CPU%\":\"0.15%\",\"MEM USAGE / LIMIT\":\"12MiB / 2GiB\",\"MEM%\":\"0.59%\",\"NET I/O\":\"1kB / 2kB\",\"BLOCK I/O\":\"3kB / 0B\"}\n",
            "===PODMAN_STATS_END===",
        );
        let stats = parse_podman_stats_snapshot(stats_output);
        assert_eq!(stats.stats.len(), 1);
        assert_eq!(stats.stats[0].name, "web");
        assert_eq!(stats.stats[0].mem_usage, "12MiB / 2GiB");

        let long_description = "x".repeat(400);
        let search_output = format!(
            "===PODMAN_SEARCH===\nSEARCH\t{{\"Name\":\"nginx\",\"Description\":\"{long_description}\",\"Stars\":9876}}\n===PODMAN_SEARCH_END===",
        );
        let results = parse_podman_search_results(&search_output);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "nginx");
        assert_eq!(results[0].description.chars().count(), 180);
        assert_eq!(results[0].stars, "9876");
    }

    #[test]
    fn resource_names_and_paths_are_validated() {
        assert!(validated_resource_name("app-data").is_ok());
        assert!(validated_resource_name("docker.io/library/nginx:alpine").is_ok());
        assert!(validated_resource_name("bad name; rm -rf /").is_err());
        assert!(validated_resource_name("").is_err());
        assert!(
            build_podman_terminal_command(PodmanTerminalOp::Copy {
                source: "/tmp/a".to_string(),
                destination: "web:/srv".to_string(),
            })
            .is_ok()
        );
        assert!(
            build_podman_terminal_command(PodmanTerminalOp::Copy {
                source: String::new(),
                destination: "web:/srv".to_string(),
            })
            .is_err()
        );
        assert!(
            build_podman_terminal_command(PodmanTerminalOp::Run {
                args: "-d --name web nginx".to_string(),
            })
            .is_ok()
        );
        assert!(
            build_podman_terminal_command(PodmanTerminalOp::Run {
                args: "   ".to_string(),
            })
            .is_err()
        );
    }

    #[test]
    fn composed_commands_quote_user_input() {
        let command = build_podman_terminal_command(PodmanTerminalOp::Copy {
            source: "/tmp/it's".to_string(),
            destination: "web:/srv data".to_string(),
        })
        .unwrap();
        // Single-argv quoting keeps metacharacters inside one argument.
        assert!(command.contains("'\\''"));
        assert!(command.contains("podman cp"));
        let stats = podman_stats_command("Linux");
        assert!(stats.contains("===PODMAN_STATS==="));
        assert!(podman_resources_snapshot_command("Linux").contains("podman images"));
        assert!(podman_resources_snapshot_command("Windows").contains("Get-Command podman"));
    }
}
