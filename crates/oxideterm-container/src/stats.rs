// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! One-shot container resource usage.
//!
//! Docker prints JSON per line, Podman prints one JSON array, and `wslc stats`
//! is already a snapshot and rejects `--no-stream`. All three are parsed by the
//! same tolerant record reader.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::instance::ContainerInstance;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerStat {
    pub id: String,
    pub name: String,
    pub cpu_percent: String,
    pub mem_usage: String,
    pub mem_percent: String,
    pub net_io: String,
    pub block_io: String,
    pub pids: String,
}

pub fn stats_command(os_type: &str, instance: &ContainerInstance) -> String {
    let program = instance.engine.program();
    let prefix = instance.cli_prefix();
    let no_stream = if instance.engine.supports_stats_no_stream() {
        "--no-stream "
    } else {
        ""
    };
    match os_type {
        "Windows" | "windows" => format!(
            "Write-Output '===CONTAINER_STATS==='; \
             if(Get-Command {program} -ErrorAction SilentlyContinue){{ \
             @(& {prefix} stats {no_stream}--format json 2>&1) | ForEach-Object {{ Write-Output ('STAT'+[char]9+$_) }} \
             }} else {{ Write-Output ('ERR'+[char]9+'{program} CLI not found') }}; \
             Write-Output '===CONTAINER_STATS_END===';"
        ),
        _ => format!(
            "echo '===CONTAINER_STATS==='; \
             if command -v {program} >/dev/null 2>&1; then \
             {prefix} stats {no_stream}--format json 2>&1 | awk '{{print \"STAT\\t\" $0}}'; \
             else printf 'ERR\\t%s\\n' '{program} CLI not found'; fi; \
             echo '===CONTAINER_STATS_END==='"
        ),
    }
}

pub fn parse_stats(output: &str) -> Vec<ContainerStat> {
    let Some(section) = extract_section(output, "CONTAINER_STATS") else {
        return Vec::new();
    };
    let mut records = Vec::new();
    let payloads = section
        .lines()
        .filter_map(|line| line.strip_prefix("STAT\t"))
        .collect::<Vec<_>>();
    // Podman prints one array; Docker prints one object per line. Engine
    // warnings share the stream and must not break either shape.
    let body = match payloads
        .iter()
        .position(|payload| payload.trim_start().starts_with('['))
    {
        Some(start) => payloads[start..].join("\n"),
        None => format!(
            "[{}]",
            payloads
                .iter()
                .filter(|payload| payload.trim_start().starts_with('{'))
                .copied()
                .collect::<Vec<_>>()
                .join(",")
        ),
    };
    let Ok(Value::Array(entries)) = serde_json::from_str::<Value>(&body) else {
        return Vec::new();
    };
    for entry in entries {
        if let Value::Object(object) = entry {
            records.push(parse_stat(&object));
        }
    }
    records
}

fn parse_stat(object: &Map<String, Value>) -> ContainerStat {
    let field = |names: &[&str]| -> String {
        names
            .iter()
            .find_map(|name| {
                object
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(name))
                    .and_then(|(_, value)| match value {
                        Value::String(text) => Some(text.clone()),
                        Value::Number(number) => Some(number.to_string()),
                        _ => None,
                    })
            })
            .unwrap_or_default()
    };
    ContainerStat {
        id: field(&["ID", "Id", "id", "Container"]),
        name: field(&["Name", "name"]),
        cpu_percent: field(&["CPUPerc", "CPUPerc", "cpu_percent", "CPU"]),
        mem_usage: field(&["MemUsage", "MemUsage", "mem_usage", "MEM USAGE / LIMIT"]),
        mem_percent: field(&["MemPerc", "mem_percent", "MEM %"]),
        net_io: field(&["NetIO", "net_io", "NET I/O"]),
        block_io: field(&["BlockIO", "block_io", "BLOCK I/O"]),
        pids: field(&["PIDs", "PIDs", "pids"]),
    }
}

fn extract_section<'a>(output: &'a str, name: &str) -> Option<&'a str> {
    let start = format!("==={name}===");
    let end = format!("==={name}_END===");
    let after_start = output.split_once(&start)?.1;
    // Truncated captures keep whatever rows arrived; only the tail is lost.
    Some(match after_start.split_once(&end) {
        Some((section, _)) => section.trim(),
        None => after_start.trim(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::ContainerEngine;
    use crate::transport::ContainerTransport;

    #[test]
    fn parses_podman_array_stats() {
        let output = concat!(
            "===CONTAINER_STATS===\n",
            "STAT\t[\n",
            "STAT\t {\n",
            "STAT\t  \"id\": \"e04c3e01c195\",\n",
            "STAT\t  \"name\": \"slapd-proxy\",\n",
            "STAT\t  \"cpu_percent\": \"0.00%\",\n",
            "STAT\t  \"mem_usage\": \"47.57MB / 16.48GB\",\n",
            "STAT\t  \"mem_percent\": \"0.29%\",\n",
            "STAT\t  \"net_io\": \"21.2kB / 7.907kB\",\n",
            "STAT\t  \"block_io\": \"0B / 0B\",\n",
            "STAT\t  \"pids\": \"2\"\n",
            "STAT\t }\n",
            "STAT\t]\n",
            "===CONTAINER_STATS_END===",
        );
        let stats = parse_stats(output);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].name, "slapd-proxy");
        assert_eq!(stats[0].mem_usage, "47.57MB / 16.48GB");
        assert_eq!(stats[0].pids, "2");
    }

    #[test]
    fn parses_docker_line_stats() {
        let output = concat!(
            "===CONTAINER_STATS===\n",
            "STAT\t{\"BlockIO\":\"0B / 0B\",\"CPUPerc\":\"1.25%\",\"Container\":\"a1b2\",\"ID\":\"a1b2\",\"MemPerc\":\"0.5%\",\"MemUsage\":\"10MiB / 1GiB\",\"Name\":\"web\",\"NetIO\":\"1kB / 2kB\",\"PIDs\":\"3\"}\n",
            "===CONTAINER_STATS_END===",
        );
        let stats = parse_stats(output);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].name, "web");
        assert_eq!(stats[0].cpu_percent, "1.25%");
        assert_eq!(stats[0].pids, "3");
    }

    #[test]
    fn stats_command_omits_no_stream_for_wslc() {
        let wslc = ContainerInstance::new(ContainerEngine::Wslc, ContainerTransport::Cli, None);
        assert!(!stats_command("Windows", &wslc).contains("--no-stream"));
        let podman = ContainerInstance::new(ContainerEngine::Podman, ContainerTransport::Cli, None);
        assert!(stats_command("Linux", &podman).contains("stats --no-stream --format json"));
    }
}
