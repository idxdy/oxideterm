// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! How the engine CLI reaches its daemon.
//!
//! Preference order is socket → TCP → plain CLI: a local socket is the least
//! ambiguous endpoint, TCP is only meaningful when the daemon was published,
//! and the bare CLI inherits whatever context the host already configured.
//! Endpoint text always originates from the discovery parser, never from raw
//! user input, but it is quoted anyway because it lands in a shell command.

use serde::{Deserialize, Serialize};

use crate::engine::ContainerEngine;
use crate::quote::shell_quote_single;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerTransportKind {
    Socket,
    Tcp,
    Cli,
}

impl ContainerTransportKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Socket => "socket",
            Self::Tcp => "tcp",
            Self::Cli => "cli",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "socket" => Some(Self::Socket),
            "tcp" => Some(Self::Tcp),
            "cli" => Some(Self::Cli),
            _ => None,
        }
    }

    /// Lower is preferred when one engine answers on several endpoints.
    fn rank(self) -> u8 {
        match self {
            Self::Socket => 0,
            Self::Tcp => 1,
            Self::Cli => 2,
        }
    }

    pub fn label_key(self) -> &'static str {
        match self {
            Self::Socket => "sidebar.host_container.transports.socket",
            Self::Tcp => "sidebar.host_container.transports.tcp",
            Self::Cli => "sidebar.host_container.transports.cli",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ContainerTransport {
    /// `unix:///…` on POSIX hosts and `npipe:////./pipe/…` on Windows.
    Socket { endpoint: String },
    /// `tcp://host:port`; `tls` adds `--tlsverify` for daemons on 2376.
    Tcp { endpoint: String, tls: bool },
    /// Whatever daemon the CLI resolves on its own (context, machine, env).
    Cli,
}

impl ContainerTransport {
    pub fn kind(&self) -> ContainerTransportKind {
        match self {
            Self::Socket { .. } => ContainerTransportKind::Socket,
            Self::Tcp { .. } => ContainerTransportKind::Tcp,
            Self::Cli => ContainerTransportKind::Cli,
        }
    }

    pub fn endpoint(&self) -> Option<&str> {
        match self {
            Self::Socket { endpoint } => Some(endpoint),
            Self::Tcp { endpoint, .. } => Some(endpoint),
            Self::Cli => None,
        }
    }

    pub fn tls(&self) -> bool {
        matches!(self, Self::Tcp { tls: true, .. })
    }

    /// Stable identity used for selection and dedupe.
    pub fn id(&self) -> String {
        match self {
            Self::Socket { endpoint } => format!("socket:{endpoint}"),
            Self::Tcp { endpoint, .. } => format!("tcp:{endpoint}"),
            Self::Cli => "cli".to_string(),
        }
    }

    pub fn preference(&self) -> u8 {
        self.kind().rank()
    }

    /// Builds a transport from discovery output, rejecting endpoints that do
    /// not carry the scheme the transport implies.
    pub fn parse(kind: &str, endpoint: &str, tls: bool) -> Option<Self> {
        let endpoint = endpoint.trim();
        match ContainerTransportKind::parse(kind)? {
            ContainerTransportKind::Socket => {
                if !(endpoint.starts_with("unix://") || endpoint.starts_with("npipe://")) {
                    return None;
                }
                Some(Self::Socket {
                    endpoint: endpoint.to_string(),
                })
            }
            ContainerTransportKind::Tcp => {
                if !endpoint.starts_with("tcp://") {
                    return None;
                }
                Some(Self::Tcp {
                    endpoint: endpoint.to_string(),
                    tls,
                })
            }
            ContainerTransportKind::Cli => Some(Self::Cli),
        }
    }

    /// The CLI arguments that point the engine at this transport.
    ///
    /// Docker and `wslc`-style CLIs take `-H`; Podman takes `--url`. `wslc`
    /// has no remoting at all, so a non-CLI transport is rejected upstream by
    /// the discovery parser.
    pub fn connection_flag(&self, engine: ContainerEngine) -> Option<String> {
        let Some(endpoint) = self.endpoint() else {
            return None;
        };
        let flag = match engine {
            ContainerEngine::Podman => "--url",
            _ => "-H",
        };
        let mut text = format!("{flag} {}", shell_quote_single(endpoint));
        if self.tls() {
            text.push_str(" --tlsverify");
        }
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_ids_and_flags_are_engine_specific() {
        let socket = ContainerTransport::Socket {
            endpoint: "unix:///var/run/docker.sock".into(),
        };
        assert_eq!(socket.id(), "socket:unix:///var/run/docker.sock");
        assert_eq!(
            socket.connection_flag(ContainerEngine::Docker),
            Some("-H 'unix:///var/run/docker.sock'".to_string())
        );
        assert_eq!(
            socket.connection_flag(ContainerEngine::Podman),
            Some("--url 'unix:///var/run/docker.sock'".to_string())
        );
        assert_eq!(
            ContainerTransport::Cli.connection_flag(ContainerEngine::Docker),
            None
        );

        let tcp = ContainerTransport::Tcp {
            endpoint: "tcp://10.0.0.2:2376".into(),
            tls: true,
        };
        assert_eq!(
            tcp.connection_flag(ContainerEngine::Docker),
            Some("-H 'tcp://10.0.0.2:2376' --tlsverify".to_string())
        );
        assert_eq!(tcp.preference(), 1);
        assert_eq!(ContainerTransport::Cli.preference(), 2);
    }

    #[test]
    fn transport_parse_rejects_mismatched_schemes() {
        assert_eq!(
            ContainerTransport::parse("socket", "unix:///run/podman/podman.sock", false),
            Some(ContainerTransport::Socket {
                endpoint: "unix:///run/podman/podman.sock".into()
            })
        );
        assert_eq!(
            ContainerTransport::parse("socket", "npipe:////./pipe/docker_engine", false),
            Some(ContainerTransport::Socket {
                endpoint: "npipe:////./pipe/docker_engine".into()
            })
        );
        assert!(ContainerTransport::parse("socket", "tcp://127.0.0.1:2375", false).is_none());
        assert!(ContainerTransport::parse("tcp", "unix:///var/run/docker.sock", false).is_none());
        assert_eq!(
            ContainerTransport::parse("cli", "", false),
            Some(ContainerTransport::Cli)
        );
    }

    #[test]
    fn socket_endpoints_are_shell_quoted() {
        let transport = ContainerTransport::Socket {
            endpoint: "unix:///tmp/it's.sock".into(),
        };
        assert_eq!(
            transport.connection_flag(ContainerEngine::Docker),
            Some("-H 'unix:///tmp/it'\\''s.sock'".to_string())
        );
    }
}
