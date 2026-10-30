// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! Snapshot command generation.
//!
//! One remote exec fills the whole inventory: containers first (the fast,
//! frequently changing part) followed by images, volumes, networks and — on
//! Podman — pods. Each section is capped so a host with thousands of images
//! cannot flood the delivery channel. Output never leaves the host's shell: it
//! is parsed by [`crate::parse_snapshot`] on the app side.

use crate::engine::ContainerEngine;
use crate::instance::ContainerInstance;

pub fn snapshot_command(os_type: &str, instance: &ContainerInstance) -> String {
    match os_type {
        "Windows" | "windows" => windows_snapshot(instance),
        _ => posix_snapshot(instance),
    }
}

/// Records stay one-per-line on every engine: Docker/`wslc` use `--format
/// json`, Podman uses a compact Go template. Arrays are also accepted by the
/// reader, but they are never the projection here because their pretty-printed
/// lines cannot be capped without cutting a record.

fn posix_snapshot(instance: &ContainerInstance) -> String {
    let program = instance.engine.program();
    let prefix = instance.cli_prefix();
    let pods = instance.engine.supports_pods();
    let mut script = String::new();
    script.push_str("echo '===CONTAINER==='; ");
    // Podman's template form prints one compact object per line (status, ports
    // and age projected alongside), so line caps keep whole records. Docker and
    // `wslc` do the same through `--format json`.
    script.push_str(&format!(
        "if command -v {program} >/dev/null 2>&1; then \
         oxide_ctr=$({prefix} ps -a --no-trunc {} 2>/dev/null); oxide_ctr_rc=$?; \
         if [ \"$oxide_ctr_rc\" -eq 0 ]; then \
         printf '%s\\n' \"$oxide_ctr\" | head -n 500 | awk '{{print \"CTR\\t\" $0}}'; \
         {prefix} images {} 2>/dev/null | head -n 300 | awk '{{print \"IMG\\t\" $0}}'; \
         {prefix} volume ls {} 2>/dev/null | head -n 300 | awk '{{print \"VOL\\t\" $0}}'; \
         {prefix} network ls {} 2>/dev/null | head -n 300 | awk '{{print \"NET\\t\" $0}}'; ",
        row_format(instance),
        image_format(instance),
        image_format(instance),
        image_format(instance),
    ));
    if pods {
        script.push_str(&format!(
            "{prefix} pod ps {} 2>/dev/null | head -n 200 | awk '{{print \"POD\\t\" $0}}'; ",
            pod_format()
        ));
    }
    script.push_str(
        "else printf 'ERR\\t%s\\n' \"$(printf '%s' \"$oxide_ctr\" | head -n 1 | tr '\\t' ' ')\"; fi; ",
    );
    script.push_str(&format!(
        "else printf 'UNAVAILABLE\\t%s\\n' '{program} CLI not found'; fi; "
    ));
    script.push_str("echo '===CONTAINER_END==='");
    script
}

fn windows_snapshot(instance: &ContainerInstance) -> String {
    let program = instance.engine.program();
    let prefix = instance.cli_prefix();
    let pods = instance.engine.supports_pods();
    let mut script = String::new();
    script.push_str("Write-Output '===CONTAINER==='; ");
    script.push_str(&format!(
        "if(Get-Command {program} -ErrorAction SilentlyContinue){{ \
         $oxideCtr = @(& {prefix} ps -a --no-trunc {} 2>&1); \
         if($LASTEXITCODE -eq 0){{ \
         $oxideCtr | Select-Object -First 500 | ForEach-Object {{ Write-Output ('CTR'+[char]9+$_) }}; \
         @(& {prefix} images {} 2>$null) | Select-Object -First 300 | ForEach-Object {{ Write-Output ('IMG'+[char]9+$_) }}; \
         @(& {prefix} volume ls {} 2>$null) | Select-Object -First 300 | ForEach-Object {{ Write-Output ('VOL'+[char]9+$_) }}; \
         @(& {prefix} network ls {} 2>$null) | Select-Object -First 300 | ForEach-Object {{ Write-Output ('NET'+[char]9+$_) }}; ",
        row_format(instance),
        image_format(instance),
        image_format(instance),
        image_format(instance),
    ));
    if pods {
        script.push_str(&format!(
            "@(& {prefix} pod ps {} 2>$null) | Select-Object -First 200 | ForEach-Object {{ Write-Output ('POD'+[char]9+$_) }}; ",
            pod_format()
        ));
    }
    script.push_str(
        "} else { Write-Output ('ERR'+[char]9+((($oxideCtr | Select-Object -First 1) -replace [char]9,' '))) } \
         } else { Write-Output ('UNAVAILABLE'+[char]9+'",
    );
    script.push_str(program);
    script.push_str(" CLI not found') }; ");
    script.push_str("Write-Output '===CONTAINER_END===';");
    script
}

/// Podman projects empty `Status`/`Ports` through `{{json .}}`, so the extra
/// columns supply them; Docker and `wslc` already carry them in JSON.
fn row_format(instance: &ContainerInstance) -> &'static str {
    match instance.engine {
        ContainerEngine::Podman => {
            "--format '{{json .}}\\t{{.Status}}\\t{{.Ports}}\\t{{.RunningFor}}'"
        }
        _ => "--format json",
    }
}

fn image_format(instance: &ContainerInstance) -> &'static str {
    match instance.engine {
        ContainerEngine::Podman => "--format '{{json .}}'",
        _ => "--format json",
    }
}

fn pod_format() -> &'static str {
    "--format '{{json .}}\\t{{.Created}}'"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::ContainerEngine;
    use crate::transport::ContainerTransport;

    fn instance(engine: ContainerEngine, transport: ContainerTransport) -> ContainerInstance {
        ContainerInstance::new(engine, transport, Some("1.0.0"))
    }

    #[test]
    fn docker_snapshot_uses_the_json_projection_and_socket_endpoint() {
        let script = snapshot_command(
            "Linux",
            &instance(
                ContainerEngine::Docker,
                ContainerTransport::Socket {
                    endpoint: "unix:///var/run/docker.sock".into(),
                },
            ),
        );
        assert!(
            script
                .contains("docker -H 'unix:///var/run/docker.sock' ps -a --no-trunc --format json")
        );
        assert!(script.contains("images --format json"));
        assert!(script.contains("volume ls --format json"));
        assert!(script.contains("network ls --format json"));
        assert!(!script.contains("pod ps"));
        assert!(script.contains("UNAVAILABLE"));
        assert!(script.contains("===CONTAINER_END==="));
    }

    #[test]
    fn podman_snapshot_uses_compact_rows_and_pods() {
        let script = snapshot_command(
            "Linux",
            &instance(
                ContainerEngine::Podman,
                ContainerTransport::Socket {
                    endpoint: "unix:///run/user/1000/podman/podman.sock".into(),
                },
            ),
        );
        assert!(script.contains(
            "podman --url 'unix:///run/user/1000/podman/podman.sock' ps -a --no-trunc --format '{{json .}}\\t{{.Status}}\\t{{.Ports}}\\t{{.RunningFor}}'"
        ));
        assert!(script.contains("pod ps --format '{{json .}}\\t{{.Created}}'"));
    }

    #[test]
    fn cli_transport_has_no_connection_flag() {
        let script = snapshot_command(
            "Linux",
            &instance(ContainerEngine::Podman, ContainerTransport::Cli),
        );
        assert!(script.contains("oxide_ctr=$(podman ps -a"));
        assert!(!script.contains("--url"));
    }

    #[test]
    fn wslc_snapshot_is_powershell_without_remoting_or_pods() {
        let script = snapshot_command(
            "Windows",
            &instance(ContainerEngine::Wslc, ContainerTransport::Cli),
        );
        assert!(script.contains("Get-Command wslc"));
        assert!(script.contains("& wslc ps -a --no-trunc --format json"));
        assert!(!script.contains("pod ps"));
        assert!(!script.contains("-H"));
    }

    #[test]
    fn docker_windows_snapshot_keeps_the_npipe_endpoint() {
        let script = snapshot_command(
            "Windows",
            &instance(
                ContainerEngine::Docker,
                ContainerTransport::Socket {
                    endpoint: "npipe:////./pipe/docker_engine".into(),
                },
            ),
        );
        assert!(script.contains("& docker -H 'npipe:////./pipe/docker_engine' ps -a"));
        assert!(script.contains("Write-Output '===CONTAINER_END===';"));
    }
}
