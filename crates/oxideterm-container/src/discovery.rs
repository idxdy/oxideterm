// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! Engine detection.
//!
//! One probe script reports every reachable engine with the most direct
//! transport that answers. Docker and Podman are probed socket → TCP → CLI, so
//! a host where Podman listens on its socket *and* has a working CLI yields a
//! single socket-backed Podman instance. `wslc` has no remoting and is always a
//! CLI instance.

use crate::engine::ContainerEngine;
use crate::instance::{ContainerInstance, dedupe_instances};
use crate::transport::{ContainerTransport, ContainerTransportKind};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContainerDiscovery {
    pub instances: Vec<ContainerInstance>,
    /// Engines whose CLI exists but that could not be reached, plus engines
    /// whose CLI is missing. Only reported when no instance was found.
    pub unavailable: Vec<ContainerUnavailable>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerUnavailable {
    pub engine: ContainerEngine,
    pub detail: Option<String>,
}

const SECTION_START: &str = "===CONTAINER_DISCOVER===";
const SECTION_END: &str = "===CONTAINER_DISCOVER_END===";
const ROW_INSTANCE: &str = "INST";
const ROW_UNAVAILABLE: &str = "UNAVAILABLE";

/// Version probe expression per engine. Docker and Podman print only the
/// daemon version; `wslc` reports its session manager version as JSON.
pub fn discovery_command(os_type: &str) -> String {
    match os_type {
        "Windows" | "windows" => windows_discovery(),
        _ => posix_discovery(),
    }
}

fn posix_discovery() -> String {
    let mut script = String::new();
    script.push_str(&format!("echo '{SECTION_START}'; "));
    // Windows' `timeout.exe` answers `command -v` in Git Bash but waits for a
    // key instead of running a command, so only GNU timeout is usable.
    script.push_str(
        "if timeout --version >/dev/null 2>&1; then oxide_to='timeout 4'; else oxide_to=''; fi; ",
    );

    // Docker: unix socket → DOCKER_HOST → desktop socket → rootless socket → TCP.
    script.push_str(
        "if command -v docker >/dev/null 2>&1; then \
         oxide_ep=''; oxide_kind=''; oxide_v=''; \
         for oxide_candidate in \"unix:///var/run/docker.sock\" \"${DOCKER_HOST}\" \"unix://${HOME}/.docker/run/docker.sock\" \"unix:///run/user/$(id -u)/docker.sock\" \"tcp://127.0.0.1:2375\"; do \
         case \"$oxide_candidate\" in unix://*|npipe://*) oxide_kind=SOCKET ;; tcp://*) oxide_kind=TCP ;; *) continue ;; esac; \
         oxide_v=$($oxide_to docker -H \"$oxide_candidate\" version --format '{{.Server.Version}}' 2>/dev/null); oxide_rc=$?; \
         if [ \"$oxide_rc\" -eq 0 ] && [ -n \"$oxide_v\" ]; then oxide_ep=\"$oxide_candidate\"; break; fi; \
         done; \
         if [ -z \"$oxide_ep\" ]; then \
         oxide_v=$($oxide_to docker -H 'tcp://127.0.0.1:2376' --tlsverify version --format '{{.Server.Version}}' 2>/dev/null); oxide_rc=$?; \
         if [ \"$oxide_rc\" -eq 0 ] && [ -n \"$oxide_v\" ]; then oxide_ep='tcp://127.0.0.1:2376'; oxide_kind=TCP; fi; \
         fi; \
         if [ -n \"$oxide_ep\" ]; then \
         printf 'INST\\tDOCKER\\t%s\\t%s\\t%s\\n' \"$oxide_kind\" \"$oxide_ep\" \"$oxide_v\"; \
         else \
         oxide_v=$($oxide_to docker version --format '{{.Server.Version}}' 2>/dev/null); oxide_rc=$?; \
         if [ \"$oxide_rc\" -eq 0 ] && [ -n \"$oxide_v\" ]; then \
         printf 'INST\\tDOCKER\\tCLI\\t\\t%s\\n' \"$oxide_v\"; \
         else \
         printf 'UNAVAILABLE\\tDOCKER\\t%s\\n' \"$(docker version 2>&1 >/dev/null | head -n 1 | tr '\\t' ' ')\"; \
         fi; \
         fi; \
         else printf 'UNAVAILABLE\\tDOCKER\\t%s\\n' 'docker CLI not found'; fi; ",
    );

    // Podman: rootful socket → user socket → XDG socket → TCP → CLI.
    script.push_str(
        "if command -v podman >/dev/null 2>&1; then \
         oxide_ep=''; oxide_kind=''; oxide_v=''; \
         for oxide_candidate in \"unix:///run/podman/podman.sock\" \"unix:///run/user/$(id -u)/podman/podman.sock\" \"unix://${XDG_RUNTIME_DIR}/podman/podman.sock\" \"tcp://127.0.0.1:2375\"; do \
         case \"$oxide_candidate\" in unix://*) oxide_kind=SOCKET ;; tcp://*) oxide_kind=TCP ;; *) continue ;; esac; \
         oxide_v=$($oxide_to podman --url \"$oxide_candidate\" info --format '{{.Version.Version}}' 2>/dev/null); oxide_rc=$?; \
         if [ \"$oxide_rc\" -eq 0 ] && [ -n \"$oxide_v\" ]; then oxide_ep=\"$oxide_candidate\"; break; fi; \
         done; \
         if [ -n \"$oxide_ep\" ]; then \
         printf 'INST\\tPODMAN\\t%s\\t%s\\t%s\\n' \"$oxide_kind\" \"$oxide_ep\" \"$oxide_v\"; \
         else \
         oxide_v=$($oxide_to podman version --format '{{.Server.Version}}' 2>/dev/null); oxide_rc=$?; \
         if [ \"$oxide_rc\" -eq 0 ] && [ -n \"$oxide_v\" ]; then \
         printf 'INST\\tPODMAN\\tCLI\\t\\t%s\\n' \"$oxide_v\"; \
         else \
         printf 'UNAVAILABLE\\tPODMAN\\t%s\\n' \"$(podman version 2>&1 >/dev/null | head -n 1 | tr '\\t' ' ')\"; \
         fi; \
         fi; \
         else printf 'UNAVAILABLE\\tPODMAN\\t%s\\n' 'podman CLI not found'; fi; ",
    );

    script.push_str(
        "if command -v wslc >/dev/null 2>&1; then \
         oxide_v=$(wslc version --format json 2>/dev/null | sed -n 's/.*\"Version\":\"\\([^\"]*\\)\".*/\\1/p' | head -n 1); \
         if [ -n \"$oxide_v\" ]; then printf 'INST\\tWSLC\\tCLI\\t\\t%s\\n' \"$oxide_v\"; \
         else printf 'UNAVAILABLE\\tWSLC\\t%s\\n' \"$(wslc info 2>&1 >/dev/null | head -n 1 | tr '\\t' ' ')\"; fi; \
         else printf 'UNAVAILABLE\\tWSLC\\t%s\\n' 'wslc CLI not found'; fi; ",
    );

    script.push_str(&format!("echo '{SECTION_END}'"));
    script
}

fn windows_discovery() -> String {
    let mut script = String::new();
    script.push_str(&format!("Write-Output '{SECTION_START}'; "));
    script.push_str(
        "if(Get-Command docker -ErrorAction SilentlyContinue){ \
         $oxideEp=$null; $oxideKind=''; $oxideVersion=''; \
         foreach($oxideCandidate in @('npipe:////./pipe/docker_engine',$env:DOCKER_HOST,'tcp://127.0.0.1:2375')){ \
         if([string]::IsNullOrWhiteSpace($oxideCandidate)){continue}; \
         if($oxideCandidate -like 'npipe://*' -or $oxideCandidate -like 'unix://*'){$oxideKind='SOCKET'}elseif($oxideCandidate -like 'tcp://*'){$oxideKind='TCP'}else{continue}; \
         $oxideOut = & docker -H $oxideCandidate version --format '{{.Server.Version}}' 2>$null; \
         if($LASTEXITCODE -eq 0 -and $oxideOut){ $oxideVersion=($oxideOut|Select-Object -First 1); $oxideEp=$oxideCandidate; break } \
         }; \
         if(-not $oxideEp){ \
         $oxideOut = & docker -H 'tcp://127.0.0.1:2376' --tlsverify version --format '{{.Server.Version}}' 2>$null; \
         if($LASTEXITCODE -eq 0 -and $oxideOut){ $oxideVersion=($oxideOut|Select-Object -First 1); $oxideEp='tcp://127.0.0.1:2376'; $oxideKind='TCP' } \
         }; \
         if($oxideEp){ Write-Output ('INST'+[char]9+'DOCKER'+[char]9+$oxideKind+[char]9+$oxideEp+[char]9+$oxideVersion) } \
         else { \
         $oxideOut = & docker version --format '{{.Server.Version}}' 2>$null; \
         if($LASTEXITCODE -eq 0 -and $oxideOut){ Write-Output ('INST'+[char]9+'DOCKER'+[char]9+'CLI'+[char]9+[char]9+($oxideOut|Select-Object -First 1)) } \
         else { Write-Output ('UNAVAILABLE'+[char]9+'DOCKER'+[char]9+((& docker version 2>&1|Select-Object -First 1) -replace [char]9,' ')) } \
         } \
         }else{ Write-Output ('UNAVAILABLE'+[char]9+'DOCKER'+[char]9+'docker CLI not found') }; ",
    );
    // Podman on Windows reaches its machine/connection through the configured
    // CLI context; there is no fixed socket path to probe.
    script.push_str(
        "if(Get-Command podman -ErrorAction SilentlyContinue){ \
         $oxideOut = & podman version --format '{{.Server.Version}}' 2>$null; \
         if($LASTEXITCODE -eq 0 -and $oxideOut){ Write-Output ('INST'+[char]9+'PODMAN'+[char]9+'CLI'+[char]9+[char]9+($oxideOut|Select-Object -First 1)) } \
         else { Write-Output ('UNAVAILABLE'+[char]9+'PODMAN'+[char]9+((& podman version 2>&1|Select-Object -First 1) -replace [char]9,' ')) } \
         }else{ Write-Output ('UNAVAILABLE'+[char]9+'PODMAN'+[char]9+'podman CLI not found') }; ",
    );
    script.push_str(
        "if(Get-Command wslc -ErrorAction SilentlyContinue){ \
         $oxideInfo = $null; \
         try { $oxideInfo = (& wslc info --format json 2>$null | ConvertFrom-Json) } catch { $oxideInfo = $null }; \
         $oxideVersion = $null; \
         if($oxideInfo){ if($oxideInfo.Server.SessionManagerVersion){ $oxideVersion=$oxideInfo.Server.SessionManagerVersion } elseif($oxideInfo.Client.Version){ $oxideVersion=$oxideInfo.Client.Version } }; \
         if($oxideVersion){ Write-Output ('INST'+[char]9+'WSLC'+[char]9+'CLI'+[char]9+[char]9+$oxideVersion) } \
         else { Write-Output ('UNAVAILABLE'+[char]9+'WSLC'+[char]9+'wslc is not running') } \
         }else{ Write-Output ('UNAVAILABLE'+[char]9+'WSLC'+[char]9+'wslc CLI not found') }; ",
    );
    script.push_str(&format!("Write-Output '{SECTION_END}';"));
    script
}

pub fn parse_discovery(output: &str) -> ContainerDiscovery {
    let Some(section) = extract_section(output, "CONTAINER_DISCOVER") else {
        return ContainerDiscovery::default();
    };
    let mut instances = Vec::new();
    let mut unavailable = Vec::new();

    for line in section
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let mut columns = line.split('\t');
        match columns.next() {
            Some(ROW_INSTANCE) => {
                let Some(engine) = columns.next().and_then(ContainerEngine::parse) else {
                    continue;
                };
                let Some(kind) = columns.next().and_then(ContainerTransportKind::parse) else {
                    continue;
                };
                let endpoint = columns.next().unwrap_or("").trim();
                let version = columns.next().unwrap_or("");
                if kind != ContainerTransportKind::Cli && !engine.supports_remoting() {
                    continue;
                }
                let tls = kind == ContainerTransportKind::Tcp && endpoint.ends_with(":2376");
                let Some(transport) = ContainerTransport::parse(kind.id(), endpoint, tls) else {
                    continue;
                };
                instances.push(ContainerInstance::new(engine, transport, Some(version)));
            }
            Some(ROW_UNAVAILABLE) => {
                let Some(engine) = columns.next().and_then(ContainerEngine::parse) else {
                    continue;
                };
                let detail = columns
                    .next()
                    .map(str::trim)
                    .filter(|detail| !detail.is_empty())
                    .map(|detail| detail.chars().take(200).collect::<String>());
                unavailable.push(ContainerUnavailable { engine, detail });
            }
            _ => {}
        }
    }

    let instances = dedupe_instances(instances);
    unavailable.retain(|entry| {
        !instances
            .iter()
            .any(|instance| instance.engine == entry.engine)
    });
    unavailable.sort_by_key(|entry| entry.engine);
    ContainerDiscovery {
        instances,
        unavailable,
    }
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
    use crate::transport::ContainerTransport;

    #[test]
    fn discovery_keeps_one_instance_per_engine_with_socket_precedence() {
        let output = concat!(
            "===CONTAINER_DISCOVER===\n",
            "INST\tPODMAN\tCLI\t\t5.8.2\n",
            "INST\tPODMAN\tSOCKET\tunix:///run/user/1000/podman/podman.sock\t5.8.2\n",
            "INST\tDOCKER\tTCP\ttcp://127.0.0.1:2376\t28.0.1\n",
            "UNAVAILABLE\tWSLC\twslc CLI not found\n",
            "===CONTAINER_DISCOVER_END===",
        );
        let discovery = parse_discovery(output);
        assert_eq!(discovery.instances.len(), 2);
        assert_eq!(discovery.instances[0].engine, ContainerEngine::Docker);
        assert_eq!(
            discovery.instances[0].transport,
            ContainerTransport::Tcp {
                endpoint: "tcp://127.0.0.1:2376".into(),
                tls: true
            }
        );
        assert_eq!(discovery.instances[1].engine, ContainerEngine::Podman);
        assert_eq!(
            discovery.instances[1].transport,
            ContainerTransport::Socket {
                endpoint: "unix:///run/user/1000/podman/podman.sock".into()
            }
        );
        assert_eq!(discovery.unavailable.len(), 1);
        assert_eq!(discovery.unavailable[0].engine, ContainerEngine::Wslc);
        assert_eq!(
            discovery.unavailable[0].detail.as_deref(),
            Some("wslc CLI not found")
        );
    }

    #[test]
    fn discovery_rejects_remoting_for_engines_without_it() {
        let output = concat!(
            "===CONTAINER_DISCOVER===\n",
            "INST\tWSLC\tSOCKET\tunix:///run/user/1000/podman/podman.sock\t3.0.3\n",
            "INST\tWSLC\tCLI\t\t3.0.3\n",
            "INST\tDOCKER\tSOCKET\ttcp://127.0.0.1:2375\t28.0.1\n",
            "===CONTAINER_DISCOVER_END===",
        );
        let discovery = parse_discovery(output);
        assert_eq!(discovery.instances.len(), 1);
        assert_eq!(discovery.instances[0].engine, ContainerEngine::Wslc);
        assert_eq!(discovery.instances[0].transport, ContainerTransport::Cli);
    }

    #[test]
    fn discovery_without_section_is_empty() {
        let discovery = parse_discovery("docker: command not found\n");
        assert!(discovery.instances.is_empty());
        assert!(discovery.unavailable.is_empty());
    }

    #[test]
    fn discovery_commands_cover_every_transport_per_engine() {
        let posix = discovery_command("Linux");
        assert!(posix.contains("unix:///var/run/docker.sock"));
        assert!(posix.contains("tcp://127.0.0.1:2375"));
        assert!(posix.contains("tcp://127.0.0.1:2376"));
        assert!(posix.contains("podman --url"));
        assert!(posix.contains("wslc version --format json"));
        assert!(posix.starts_with("echo '===CONTAINER_DISCOVER==='; "));
        assert!(posix.ends_with("echo '===CONTAINER_DISCOVER_END==='"));
        assert!(posix.contains("timeout --version"));

        let windows = discovery_command("Windows");
        assert!(windows.contains("npipe:////./pipe/docker_engine"));
        assert!(windows.contains("$env:DOCKER_HOST"));
        assert!(windows.contains("wslc info --format json"));
        assert!(windows.contains("podman version"));
        assert!(windows.starts_with("Write-Output '===CONTAINER_DISCOVER==='; "));
        assert!(windows.ends_with("Write-Output '===CONTAINER_DISCOVER_END===';"));
    }
}
