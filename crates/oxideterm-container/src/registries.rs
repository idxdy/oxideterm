// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! Authenticated registries from the engine's auth file.
//!
//! Docker stores credentials in `config.json` (`auths` map) and Podman in
//! `auth.json`, which uses the same shape. Only registry keys leave the host:
//! the values hold base64 `user:password`, identity tokens and helper names, and
//! none of them are ever read into a row, a log or a notice.

use serde::{Deserialize, Serialize};

use crate::engine::ContainerEngine;
use crate::instance::ContainerInstance;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerRegistrySource {
    Docker,
    Podman,
}

impl ContainerRegistrySource {
    pub fn id(self) -> &'static str {
        match self {
            Self::Docker => "docker",
            Self::Podman => "podman",
        }
    }

    pub fn label_key(self) -> &'static str {
        match self {
            Self::Docker => "sidebar.host_container.sources.docker",
            Self::Podman => "sidebar.host_container.sources.podman",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "docker" => Some(Self::Docker),
            "podman" => Some(Self::Podman),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerRegistry {
    pub source: ContainerRegistrySource,
    pub registry: String,
}

/// Extracts the keys of the `auths` object without printing any value.
const AUTH_KEYS_AWK: &str = r#"awk '
{ buf = buf $0 "\n" }
END {
  depth = 0; target = -1; in_str = 0; esc = 0; token = ""; last_key = ""
  n = length(buf); i = 1
  while (i <= n) {
    c = substr(buf, i, 1)
    if (in_str) {
      if (esc) { token = token c; esc = 0 }
      else if (c == "\\") { esc = 1 }
      else if (c == "\"") {
        in_str = 0
        j = i + 1
        while (j <= n && substr(buf, j, 1) ~ /[ \t\r\n]/) j++
        if (substr(buf, j, 1) == ":") {
          last_key = token
          if (target > 0 && depth == target) print token
        }
      } else token = token c
    } else {
      if (c == "\"") { in_str = 1; token = "" }
      else if (c == "{") { depth++; if (last_key == "auths" && target < 0) target = depth }
      else if (c == "}") { if (depth == target) target = -1; depth-- }
    }
    i++
  }
}'"#;

pub fn registries_command(os_type: &str, instance: &ContainerInstance) -> String {
    match os_type {
        "Windows" | "windows" => windows_registries(instance),
        _ => posix_registries(instance),
    }
}

fn posix_files(engine: ContainerEngine) -> &'static str {
    match engine {
        // Podman: explicit override, runtime socket dir, user config, then the
        // Docker file podman falls back to when its own auth file is missing.
        ContainerEngine::Podman => {
            r#""$REGISTRY_AUTH_FILE" "${XDG_RUNTIME_DIR}/containers/auth.json" "$HOME/.config/containers/auth.json" "$HOME/.docker/config.json""#
        }
        // Docker: DOCKER_CONFIG wins over the default home config.
        _ => r#""${DOCKER_CONFIG}/config.json" "$HOME/.docker/config.json""#,
    }
}

fn posix_registries(instance: &ContainerInstance) -> String {
    let source = match instance.engine {
        ContainerEngine::Podman => "podman",
        _ => "docker",
    };
    format!(
        "echo '===CONTAINER_REGISTRIES==='; \
         for oxide_file in {}; do \
         [ -n \"$oxide_file\" ] && [ -f \"$oxide_file\" ] || continue; \
         {AUTH_KEYS_AWK} \"$oxide_file\" 2>/dev/null | head -n 100 | awk -v src='{source}' '{{print \"REG\\t\" src \"\\t\" $0}}'; \
         done; \
         echo '===CONTAINER_REGISTRIES_END==='",
        posix_files(instance.engine)
    )
}

fn windows_registries(instance: &ContainerInstance) -> String {
    let source = match instance.engine {
        ContainerEngine::Podman => "podman",
        _ => "docker",
    };
    let (first, second) = match instance.engine {
        ContainerEngine::Podman => (
            "$env:REGISTRY_AUTH_FILE",
            "$env:USERPROFILE\\.config\\containers\\auth.json",
        ),
        _ => (
            "$env:DOCKER_CONFIG\\config.json",
            "$env:USERPROFILE\\.docker\\config.json",
        ),
    };
    format!(
        "Write-Output '===CONTAINER_REGISTRIES==='; \
         foreach($oxideFile in @('{first}','{second}','$env:APPDATA\\containers\\auth.json')){{ \
         if([string]::IsNullOrWhiteSpace($oxideFile) -or -not (Test-Path $oxideFile)){{continue}}; \
         try {{ $oxideJson = Get-Content -Raw -LiteralPath $oxideFile | ConvertFrom-Json }} catch {{ continue }}; \
         if($oxideJson.auths){{ $oxideJson.auths.PSObject.Properties.Name | Select-Object -First 100 | ForEach-Object {{ Write-Output ('REG'+[char]9+'{source}'+[char]9+$_) }} }} \
         }}; \
         Write-Output '===CONTAINER_REGISTRIES_END===';"
    )
}

pub fn parse_registries(output: &str) -> Vec<ContainerRegistry> {
    let Some(section) = extract_section(output, "CONTAINER_REGISTRIES") else {
        return Vec::new();
    };
    let mut registries: Vec<ContainerRegistry> = Vec::new();
    for line in section
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let mut columns = line.split('\t');
        if columns.next() != Some("REG") {
            continue;
        }
        let Some(source) = columns.next().and_then(ContainerRegistrySource::parse) else {
            continue;
        };
        let Some(raw) = columns.next() else {
            continue;
        };
        let Some(registry) = normalize_registry(raw) else {
            continue;
        };
        if registries
            .iter()
            .any(|existing| existing.registry.eq_ignore_ascii_case(&registry))
        {
            continue;
        }
        registries.push(ContainerRegistry { source, registry });
    }
    registries.sort_by(|left, right| left.registry.cmp(&right.registry));
    registries
}

/// Registry keys are URLs or hosts; show the host (and any path) without the
/// scheme or trailing slash, and reject control characters from crafted files.
fn normalize_registry(raw: &str) -> Option<String> {
    let mut value = raw.trim().trim_start_matches("https://");
    if let Some(rest) = value.strip_prefix("http://") {
        value = rest;
    }
    let value = value.trim_end_matches('/');
    if value.is_empty()
        || value.len() > 200
        || value.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return None;
    }
    Some(value.to_string())
}

fn extract_section<'a>(output: &'a str, name: &str) -> Option<&'a str> {
    let start = format!("==={name}===");
    let end = format!("==={name}_END===");
    let after_start = output.split_once(&start)?.1;
    Some(match after_start.split_once(&end) {
        Some((section, _)) => section.trim(),
        None => after_start.trim(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::ContainerTransport;

    fn instance(engine: ContainerEngine) -> ContainerInstance {
        ContainerInstance::new(engine, ContainerTransport::Cli, None)
    }

    #[test]
    fn parses_and_dedupes_registries_across_files() {
        let output = concat!(
            "===CONTAINER_REGISTRIES===\n",
            "REG\tpodman\t423437244826.dkr.ecr.ap-southeast-1.amazonaws.com\n",
            "REG\tpodman\tharbor.emea.ocp.int.kn\n",
            "REG\tpodman\thttps://index.docker.io/v1/\n",
            "REG\tpodman\tharbor.emea.ocp.int.kn\n",
            "REG\tdocker\tghcr.io\n",
            "bogus line\n",
            "===CONTAINER_REGISTRIES_END===",
        );
        let registries = parse_registries(output);
        assert_eq!(registries.len(), 4);
        assert_eq!(
            registries[0].registry,
            "423437244826.dkr.ecr.ap-southeast-1.amazonaws.com"
        );
        assert_eq!(registries[0].source, ContainerRegistrySource::Podman);
        assert!(
            registries
                .iter()
                .any(|registry| registry.registry == "index.docker.io/v1")
        );
        assert_eq!(
            registries
                .iter()
                .find(|registry| registry.registry == "ghcr.io")
                .map(|registry| registry.source),
            Some(ContainerRegistrySource::Docker)
        );
    }

    #[test]
    fn missing_section_and_bad_keys_are_ignored() {
        assert!(parse_registries("").is_empty());
        let output = "===CONTAINER_REGISTRIES===\nREG\tpodman\thas space\nREG\tunknown\tx\n===CONTAINER_REGISTRIES_END===";
        assert!(parse_registries(output).is_empty());
    }

    #[test]
    fn commands_read_only_the_engine_auth_files() {
        let podman = registries_command("Linux", &instance(ContainerEngine::Podman));
        assert!(podman.contains("$REGISTRY_AUTH_FILE"));
        assert!(podman.contains("$HOME/.config/containers/auth.json"));
        assert!(podman.contains("$HOME/.docker/config.json"));
        assert!(!podman.contains("auth\":"));
        let docker = registries_command("Linux", &instance(ContainerEngine::Docker));
        assert!(docker.contains("\"${DOCKER_CONFIG}/config.json\""));
        assert!(!docker.contains("containers/auth.json"));
        let windows = registries_command("Windows", &instance(ContainerEngine::Podman));
        assert!(windows.contains("ConvertFrom-Json"));
        assert!(windows.contains("auths.PSObject.Properties.Name"));
    }
}
