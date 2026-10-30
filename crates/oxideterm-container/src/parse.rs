// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! Snapshot parsing for every engine's JSON dialect.
//!
//! Docker and `wslc` print one compact JSON object per line with `--format
//! json`; Podman prints `{{json .}}` per line and has no populated `Status` or
//! `Ports` in that projection, so the snapshot command appends them as extra
//! tab-separated columns. Rows are split on the JSON object boundary rather
//! than on `\t`, so a crafted field can never shift the appended columns.

use serde_json::{Map, Value};

use crate::model::{Container, ContainerSnapshot, ContainerStatus, Image, Network, Pod, Volume};

const ROW_PREFIX_CONTAINER: &str = "CTR";
const ROW_PREFIX_IMAGE: &str = "IMG";
const ROW_PREFIX_VOLUME: &str = "VOL";
const ROW_PREFIX_NETWORK: &str = "NET";
const ROW_PREFIX_POD: &str = "POD";
const MARKER_UNAVAILABLE: &str = "UNAVAILABLE";
const MARKER_ERROR: &str = "ERR";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum RowKind {
    Container,
    Image,
    Volume,
    Network,
    Pod,
}

impl RowKind {
    fn from_prefix(prefix: &str) -> Option<Self> {
        match prefix {
            ROW_PREFIX_CONTAINER => Some(Self::Container),
            ROW_PREFIX_IMAGE => Some(Self::Image),
            ROW_PREFIX_VOLUME => Some(Self::Volume),
            ROW_PREFIX_NETWORK => Some(Self::Network),
            ROW_PREFIX_POD => Some(Self::Pod),
            _ => None,
        }
    }
}

pub fn parse_snapshot(output: &str) -> ContainerSnapshot {
    let Some(section) = extract_section(output, "CONTAINER") else {
        return ContainerSnapshot::default();
    };

    let mut snapshot = ContainerSnapshot::default();
    let mut buckets: std::collections::BTreeMap<RowKind, Vec<&str>> =
        std::collections::BTreeMap::new();

    for line in section
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        match line {
            MARKER_UNAVAILABLE => {
                return ContainerSnapshot {
                    status: ContainerStatus::Unavailable { detail: None },
                    ..ContainerSnapshot::default()
                };
            }
            MARKER_ERROR => return error_snapshot("Container command failed."),
            _ => {}
        }
        if let Some(detail) = line.strip_prefix(MARKER_UNAVAILABLE) {
            let detail = clean_detail(detail);
            return ContainerSnapshot {
                status: ContainerStatus::Unavailable { detail },
                ..ContainerSnapshot::default()
            };
        }
        if let Some(message) = line.strip_prefix(MARKER_ERROR) {
            return error_snapshot(
                &clean_detail(message).unwrap_or_else(|| "Container command failed.".to_string()),
            );
        }

        let Some((prefix, payload)) = split_row(line) else {
            continue;
        };
        if let Some(kind) = RowKind::from_prefix(prefix) {
            buckets.entry(kind).or_default().push(payload);
        }
    }

    let mut container_rows = 0usize;
    let mut parsed_containers = 0usize;
    for (kind, payloads) in buckets {
        for (json, extra) in record_payloads(&payloads) {
            match kind {
                RowKind::Container => {
                    container_rows += 1;
                    if let Some(container) = parse_container(&json, &extra) {
                        parsed_containers += 1;
                        snapshot.containers.push(container);
                    }
                }
                RowKind::Image => {
                    if let Some(image) = parse_image(&json) {
                        snapshot.images.push(image);
                    }
                }
                RowKind::Volume => {
                    if let Some(volume) = parse_volume(&json) {
                        snapshot.volumes.push(volume);
                    }
                }
                RowKind::Network => {
                    if let Some(network) = parse_network(&json) {
                        snapshot.networks.push(network);
                    }
                }
                RowKind::Pod => {
                    if let Some(pod) = parse_pod(&json, &extra) {
                        snapshot.pods.push(pod);
                    }
                }
            }
        }
    }

    snapshot.status = if container_rows > 0 && parsed_containers == 0 {
        // Rows arrived but none matched the expected shape: surface it instead
        // of showing a misleading empty list.
        ContainerStatus::Error {
            message: "Container output could not be parsed.".to_string(),
        }
    } else {
        ContainerStatus::Available
    };
    snapshot
}

/// Parses one section's payload lines.
///
/// Docker and `wslc` print one JSON object per line (plus optional tab columns
/// for appended fields); Podman prints a single JSON array. Engine warnings
/// share the stream, so anything that is not part of the JSON payload is
/// dropped instead of being counted as a broken row.
fn record_payloads(payloads: &[&str]) -> Vec<(String, String)> {
    // Array mode only when the section itself starts with `[`; a JSON object
    // legitimately contains arrays (Names, Labels, subnets), and treating
    // those as the section payload would silently drop every row.
    let array_start = payloads
        .iter()
        .position(|payload| payload.trim_start().starts_with('['));
    let joined = array_start
        .map(|start| payloads[start..].join("\n"))
        .unwrap_or_default();
    if let (Some(start), Some(end)) = (joined.find('['), joined.rfind(']'))
        && start < end
    {
        if let Ok(serde_json::Value::Array(entries)) =
            serde_json::from_str::<serde_json::Value>(&joined[start..=end])
        {
            return entries
                .into_iter()
                .filter(|entry| entry.is_object())
                .filter_map(|entry| serde_json::to_string(&entry).ok())
                .map(|json| (json, String::new()))
                .collect();
        }
    }
    payloads
        .iter()
        .filter_map(|payload| {
            let (json, rest) = split_json_prefix(payload)?;
            Some((json.to_string(), rest.trim_start_matches('\t').to_string()))
        })
        .collect()
}

fn error_snapshot(message: &str) -> ContainerSnapshot {
    ContainerSnapshot {
        status: ContainerStatus::Error {
            message: message.to_string(),
        },
        ..ContainerSnapshot::default()
    }
}

fn clean_detail(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    Some(raw.chars().take(200).collect())
}

/// Splits `<PREFIX>\t<json…>` without trusting the JSON to avoid tabs.
fn split_row(line: &str) -> Option<(&str, &str)> {
    let (prefix, rest) = line.split_once('\t')?;
    Some((prefix.trim(), rest))
}

/// Finds the end of the first JSON object in `line`, honoring strings and
/// escapes, and returns the object plus whatever follows it.
pub fn split_json_prefix(line: &str) -> Option<(&str, &str)> {
    let line = line.trim_start();
    if !line.starts_with('{') {
        return None;
    }
    let bytes = line.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some((&line[..=index], &line[index + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_container(json: &str, extra: &str) -> Option<Container> {
    let value = parse_object(json)?;
    let extra: Vec<&str> = extra.split('\t').collect();
    let id = string_field(&value, &["ID", "Id"])?;
    let name = first_string_field(&value, &["Names", "Name"])?;
    let image = string_field(&value, &["Image"]).unwrap_or_default();
    let state = string_field(&value, &["State"]).unwrap_or_default();
    let appended_status = extra.first().map(|value| value.trim()).unwrap_or("");
    let status = first_non_empty_string(&value, &["Status"]).unwrap_or_else(|| {
        if appended_status.is_empty() {
            state.clone()
        } else {
            appended_status.to_string()
        }
    });
    let ports = first_non_empty_string(&value, &["Ports"]).or_else(|| {
        extra
            .get(1)
            .map(|ports| ports.trim().to_string())
            .filter(|ports| !ports.is_empty())
    });
    // Podman projects an empty `CreatedAt` in template mode and only the ISO
    // `Created`; the appended running-for column keeps the human age.
    let created = extra
        .get(2)
        .map(|created| created.trim().to_string())
        .filter(|created| !created.is_empty())
        .or_else(|| first_non_empty_string(&value, &["RunningFor", "CreatedAt", "Created"]));
    Some(Container {
        id,
        name,
        image,
        state,
        status,
        ports,
        created,
    })
}

fn parse_image(json: &str) -> Option<Image> {
    let value = parse_object(json)?;
    let id = string_field(&value, &["ID", "Id"])?;
    let mut repository = string_field(&value, &["Repository", "repository"]).unwrap_or_default();
    let mut tag = string_field(&value, &["Tag", "tag"]).unwrap_or_default();
    if repository.is_empty() || repository == "<none>" {
        if let Some(reference) = first_string_field(&value, &["RepoTags", "Names"]) {
            if let Some((repo, parsed_tag)) = split_reference(&reference) {
                repository = repo;
                if tag.is_empty() {
                    tag = parsed_tag;
                }
            }
        }
    }
    let size = first_non_empty_string(&value, &["Size", "size"]).unwrap_or_default();
    let created = first_non_empty_string(
        &value,
        &["CreatedSince", "CreatedAt", "CreatedTime", "Created"],
    )
    .unwrap_or_default();
    Some(Image {
        id,
        repository,
        tag,
        size,
        created,
    })
}

fn parse_volume(json: &str) -> Option<Volume> {
    let value = parse_object(json)?;
    let name = string_field(&value, &["Name", "name"])?;
    let driver = string_field(&value, &["Driver", "driver"]).unwrap_or_default();
    let mount_point =
        string_field(&value, &["Mountpoint", "MountPoint", "mountpoint"]).unwrap_or_default();
    Some(Volume {
        name,
        driver,
        mount_point,
    })
}

fn parse_network(json: &str) -> Option<Network> {
    let value = parse_object(json)?;
    let name = string_field(&value, &["Name", "name"])?;
    let id = string_field(&value, &["ID", "Id", "id"]).unwrap_or_default();
    let driver = string_field(&value, &["Driver", "driver"]).unwrap_or_default();
    let scope = string_field(&value, &["Scope", "scope"]).unwrap_or_default();
    let subnets = value
        .get("subnets")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| match entry {
                    Value::String(subnet) => Some(subnet.clone()),
                    Value::Object(object) => object
                        .get("subnet")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Network {
        id,
        name,
        driver,
        scope,
        subnets,
    })
}

fn parse_pod(json: &str, extra: &str) -> Option<Pod> {
    let value = parse_object(json)?;
    let id = string_field(&value, &["Id", "ID"])?;
    let name = string_field(&value, &["Name"])?;
    let status = string_field(&value, &["Status"]).unwrap_or_default();
    let containers = value
        .get("Containers")
        .map(|containers| match containers {
            Value::Array(entries) => entries.len(),
            Value::Number(number) => number.as_u64().unwrap_or_default() as usize,
            _ => 0,
        })
        .unwrap_or_default();
    let created = extra
        .split('\t')
        .next()
        .map(str::trim)
        .filter(|created| !created.is_empty())
        .map(str::to_string);
    Some(Pod {
        id,
        name,
        status,
        created,
        containers,
    })
}

fn parse_object(json: &str) -> Option<Map<String, Value>> {
    match serde_json::from_str::<Value>(json).ok()? {
        Value::Object(object) => Some(object),
        _ => None,
    }
}

fn field<'a>(object: &'a Map<String, Value>, names: &[&str]) -> Option<&'a Value> {
    // Names are ordered by preference: `RunningFor` must win over `CreatedAt`
    // no matter how the engine sorts its JSON keys.
    names.iter().find_map(|name| {
        object
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value)
    })
}

fn first_non_empty_string(object: &Map<String, Value>, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        let value = object
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value)?;
        let text = match value {
            Value::String(text) => text.clone(),
            Value::Number(number) => number.to_string(),
            _ => return None,
        };
        (!text.trim().is_empty()).then_some(text)
    })
}

fn string_field(object: &Map<String, Value>, names: &[&str]) -> Option<String> {
    field(object, names).and_then(|value| match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    })
}

fn first_string_field(object: &Map<String, Value>, names: &[&str]) -> Option<String> {
    let value = field(object, names)?;
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(entries) => entries
            .iter()
            .find_map(|entry| entry.as_str().map(str::to_string)),
        _ => None,
    }
}

fn split_reference(reference: &str) -> Option<(String, String)> {
    let reference = reference.trim();
    if reference.is_empty() {
        return None;
    }
    match reference.rsplit_once(':') {
        Some((repository, tag)) if !tag.contains('/') => {
            Some((repository.to_string(), tag.to_string()))
        }
        _ => Some((reference.to_string(), String::new())),
    }
}

fn extract_section<'a>(output: &'a str, name: &str) -> Option<&'a str> {
    let start = format!("==={name}===");
    let end = format!("==={name}_END===");
    let after_start = output.split_once(&start)?.1;
    // A capture capped mid-stream loses the end marker; the rows that did
    // arrive are still valid, so parse up to the truncation instead of
    // discarding the whole inventory.
    Some(match after_start.split_once(&end) {
        Some((section, _)) => section.trim(),
        None => after_start.trim(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docker_lines_keep_status_and_ports_from_json() {
        let output = concat!(
            "===CONTAINER===\n",
            "CTR\t{\"Command\":\"\\\"sleep 300\\\"\",\"CreatedAt\":\"2026-10-09 10:37:29 +0800 GMT+8\",",
            "\"ID\":\"19ff1a78d092\",\"Image\":\"alpine:latest\",\"Names\":\"uc-cli-pub\",",
            "\"Ports\":\"0.0.0.0:8080->80/tcp\",\"RunningFor\":\"2 weeks ago\",",
            "\"State\":\"created\",\"Status\":\"Created\"}\n",
            "IMG\t{\"ID\":\"320994c3b997\",\"Repository\":\"alpine\",\"Tag\":\"latest\",",
            "\"Size\":\"8.42MB\",\"CreatedSince\":\"5 weeks ago\"}\n",
            "VOL\t{\"Name\":\"app-data\",\"Driver\":\"local\",\"Mountpoint\":\"/var/lib/docker/volumes/app-data/_data\"}\n",
            "NET\t{\"ID\":\"73bb526fbe81\",\"Name\":\"bridge\",\"Driver\":\"bridge\",\"Scope\":\"local\"}\n",
            "===CONTAINER_END===",
        );
        let snapshot = parse_snapshot(output);
        assert_eq!(snapshot.status, ContainerStatus::Available);
        assert_eq!(snapshot.containers.len(), 1);
        let container = &snapshot.containers[0];
        assert_eq!(container.id, "19ff1a78d092");
        assert_eq!(container.name, "uc-cli-pub");
        assert_eq!(container.ports.as_deref(), Some("0.0.0.0:8080->80/tcp"));
        assert_eq!(container.status, "Created");
        assert_eq!(container.created.as_deref(), Some("2 weeks ago"));
        assert_eq!(snapshot.images[0].repository, "alpine");
        assert_eq!(snapshot.images[0].tag, "latest");
        assert_eq!(snapshot.volumes[0].name, "app-data");
        assert_eq!(snapshot.networks[0].name, "bridge");
        assert!(snapshot.networks[0].subnets.is_empty());
    }

    #[test]
    fn podman_lines_take_appended_status_ports_and_pod_created() {
        let output = concat!(
            "===CONTAINER===\n",
            "CTR\t{\"Id\":\"42bac06d921bbf44e8c9d3a0d3b9a7dee566423ecce71688aa24bd0cb1deb356\",",
            "\"Names\":[\"vigilant_meninsky\"],\"Image\":\"docker.io/mlan/openldap:latest\",",
            "\"State\":\"exited\",\"Status\":\"\",\"Ports\":null,\"CreatedAt\":\"2 months ago\"}",
            "\tExited (0) 2 months ago\t0.0.0.0:389->389/tcp\t2 months ago\n",
            "IMG\t{\"repository\":\"docker.io/library/redis\",\"tag\":\"7-alpine\",\"Id\":\"98c6ca8f397f\",",
            "\"Size\":\"41.2 MB\",\"CreatedAt\":\"3 weeks ago\"}\n",
            "NET\t{\"name\":\"podman\",\"id\":\"e4881cd70467\",\"driver\":\"bridge\",\"scope\":\"local\",",
            "\"subnets\":[{\"subnet\":\"10.88.0.0/16\"},{\"subnet\":\"fd00::/64\"}]}\n",
            "POD\t{\"Id\":\"3cd7df7af038e334ea5eb404719eea184303544618b797d57887c41f6ddc9ed0\",",
            "\"Name\":\"pod_ldap\",\"Status\":\"Exited\",\"Containers\":[{\"Id\":\"552d85b062e9\"}]}",
            "\t8 weeks ago\n",
            "===CONTAINER_END===",
        );
        let snapshot = parse_snapshot(output);
        assert_eq!(snapshot.status, ContainerStatus::Available);
        let container = &snapshot.containers[0];
        assert_eq!(container.name, "vigilant_meninsky");
        assert_eq!(container.status, "Exited (0) 2 months ago");
        assert_eq!(container.ports.as_deref(), Some("0.0.0.0:389->389/tcp"));
        assert_eq!(container.created.as_deref(), Some("2 months ago"));
        assert_eq!(snapshot.images[0].tag, "7-alpine");
        assert_eq!(
            snapshot.networks[0].subnets,
            vec!["10.88.0.0/16", "fd00::/64"]
        );
        assert_eq!(snapshot.pods[0].name, "pod_ldap");
        assert_eq!(snapshot.pods[0].containers, 1);
        assert_eq!(snapshot.pods[0].created.as_deref(), Some("8 weeks ago"));
    }

    #[test]
    fn missing_cli_and_failures_are_distinct() {
        let missing = parse_snapshot(
            "===CONTAINER===\nUNAVAILABLE\tdocker CLI not found\n===CONTAINER_END===",
        );
        match missing.status {
            ContainerStatus::Unavailable { detail } => {
                assert_eq!(detail.as_deref(), Some("docker CLI not found"));
            }
            other => panic!("expected unavailable, got {other:?}"),
        }
        let failed = parse_snapshot(
            "===CONTAINER===\nERR\tCannot connect to the Docker daemon\n===CONTAINER_END===",
        );
        assert_eq!(
            failed.status,
            ContainerStatus::Error {
                message: "Cannot connect to the Docker daemon".to_string()
            }
        );
        // Engine warnings and other noise are ignored; a JSON-shaped row that
        // still cannot be parsed is reported instead of silently dropped.
        let noise = parse_snapshot(
            "===CONTAINER===\nCTR\ttime=\"...\" level=warning msg=\"rootless\"\n===CONTAINER_END===",
        );
        assert_eq!(noise.status, ContainerStatus::Available);
        let unparseable = parse_snapshot("===CONTAINER===\nCTR\t{\"State\":}\n===CONTAINER_END===");
        assert!(matches!(unparseable.status, ContainerStatus::Error { .. }));
    }

    #[test]
    fn split_json_prefix_ignores_tabs_and_braces_inside_strings() {
        let line = "{\"Ports\":\"a\tb\",\"Nested\":{\"x\":1}}\tUp 2 minutes\t80/tcp";
        let (json, rest) = split_json_prefix(line).expect("json prefix");
        assert_eq!(json, "{\"Ports\":\"a\tb\",\"Nested\":{\"x\":1}}");
        assert_eq!(rest, "\tUp 2 minutes\t80/tcp");
        assert!(split_json_prefix("not json").is_none());
        assert!(split_json_prefix("{\"unterminated\":true").is_none());
    }

    #[test]
    fn images_without_repository_fall_back_to_their_tags() {
        let output = concat!(
            "===CONTAINER===\n",
            "IMG\t{\"Id\":\"98c6ca8f397f\",\"RepoTags\":[\"docker.io/library/redis:7-alpine\"],\"Size\":\"41 MB\"}\n",
            "===CONTAINER_END===",
        );
        let snapshot = parse_snapshot(output);
        assert_eq!(snapshot.status, ContainerStatus::Available);
        assert_eq!(snapshot.images.len(), 1);
        assert_eq!(snapshot.images[0].repository, "docker.io/library/redis");
        assert_eq!(snapshot.images[0].tag, "7-alpine");
    }
}
