// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! Interactive terminal operations.
//!
//! These run inside the user's own terminal tab, so the app neither captures
//! output nor audits the composed arguments. Free-form `run`/`build` argv text
//! is the user's typing; identifiers and paths are validated bounds only, and
//! every interpolated value is single-quote escaped before it reaches a shell.

use crate::engine::ContainerEngine;
use crate::instance::ContainerInstance;
use crate::quote::{shell_quote_single, validated_container_id, validated_resource_name};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContainerTerminalOp {
    /// Raw `run` arguments authored by the user in the input dialog.
    Run {
        args: String,
    },
    Attach {
        container_id: String,
    },
    Pull {
        reference: String,
    },
    Push {
        reference: String,
    },
    /// Raw `build` arguments authored by the user in the input dialog.
    Build {
        args: String,
    },
    Save {
        reference: String,
        path: String,
    },
    Load {
        path: String,
    },
    Import {
        path: String,
        reference: String,
    },
    Export {
        container_id: String,
        path: String,
    },
    Copy {
        source: String,
        destination: String,
    },
    Login {
        registry: String,
    },
}

pub fn terminal_command(
    instance: &ContainerInstance,
    op: ContainerTerminalOp,
) -> Result<String, String> {
    let prefix = instance.cli_prefix();
    Ok(match op {
        ContainerTerminalOp::Run { args } => {
            let args = raw_arguments(&args, "run")?;
            format!("{prefix} run {args}")
        }
        ContainerTerminalOp::Build { args } => {
            let args = raw_arguments(&args, "build")?;
            format!("{prefix} build {args}")
        }
        ContainerTerminalOp::Attach { container_id } => {
            let container_id = validated_container_id(&container_id)?;
            format!("{prefix} attach {container_id}")
        }
        ContainerTerminalOp::Pull { reference } => {
            let reference = validated_resource_name(&reference)?;
            format!("{prefix} pull {}", shell_quote_single(reference))
        }
        ContainerTerminalOp::Push { reference } => {
            let reference = validated_resource_name(&reference)?;
            format!("{prefix} push {}", shell_quote_single(reference))
        }
        ContainerTerminalOp::Save { reference, path } => {
            let reference = validated_resource_name(&reference)?;
            let path = validated_path(&path)?;
            format!(
                "{prefix} save -o {} {}",
                shell_quote_single(path),
                shell_quote_single(reference)
            )
        }
        ContainerTerminalOp::Load { path } => {
            let path = validated_path(&path)?;
            format!("{prefix} load -i {}", shell_quote_single(path))
        }
        ContainerTerminalOp::Import { path, reference } => {
            let path = validated_path(&path)?;
            let reference = validated_resource_name(&reference)?;
            format!(
                "{prefix} import {} {}",
                shell_quote_single(path),
                shell_quote_single(reference)
            )
        }
        ContainerTerminalOp::Export { container_id, path } => {
            let container_id = validated_container_id(&container_id)?;
            let path = validated_path(&path)?;
            format!(
                "{prefix} export -o {} {container_id}",
                shell_quote_single(path)
            )
        }
        ContainerTerminalOp::Copy {
            source,
            destination,
        } => {
            let source = validated_path(&source)?;
            let destination = validated_path(&destination)?;
            format!(
                "{prefix} cp {} {}",
                shell_quote_single(source),
                shell_quote_single(destination)
            )
        }
        ContainerTerminalOp::Login { registry } => {
            let registry = validated_resource_name(&registry)?;
            format!("{prefix} login {}", shell_quote_single(registry))
        }
    })
}

fn raw_arguments<'a>(args: &'a str, verb: &str) -> Result<&'a str, String> {
    let args = args.trim();
    if args.is_empty() {
        return Err(format!("{verb} arguments are required."));
    }
    if args.chars().any(|character| character.is_control()) {
        return Err("Control characters are not allowed.".to_string());
    }
    Ok(args)
}

/// Paths may contain separators, spaces and drive letters but never control
/// characters; the value is still quoted for the shell.
fn validated_path(path: &str) -> Result<&str, String> {
    let path = path.trim();
    if path.is_empty() || path.len() > 512 {
        return Err("Invalid path.".to_string());
    }
    if path.chars().any(|character| character.is_control()) {
        return Err("Control characters are not allowed in paths.".to_string());
    }
    Ok(path)
}

#[allow(dead_code)]
fn engine_supports_attach(engine: ContainerEngine) -> bool {
    // Attach is available on every engine, including `wslc`.
    matches!(
        engine,
        ContainerEngine::Docker | ContainerEngine::Podman | ContainerEngine::Wslc
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::ContainerTransport;

    fn podman() -> ContainerInstance {
        ContainerInstance::new(
            ContainerEngine::Podman,
            ContainerTransport::Socket {
                endpoint: "unix:///run/user/1000/podman/podman.sock".into(),
            },
            None,
        )
    }

    #[test]
    fn run_and_build_keep_user_arguments_and_prefix() {
        assert_eq!(
            terminal_command(
                &podman(),
                ContainerTerminalOp::Run {
                    args: "-d --name web nginx:alpine".into()
                }
            )
            .unwrap(),
            "podman --url 'unix:///run/user/1000/podman/podman.sock' run -d --name web nginx:alpine"
        );
        assert!(
            terminal_command(&podman(), ContainerTerminalOp::Run { args: "  ".into() }).is_err()
        );
        assert!(
            terminal_command(
                &podman(),
                ContainerTerminalOp::Build {
                    args: "x\nrm".into()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn file_operations_quote_paths_and_references() {
        assert_eq!(
            terminal_command(
                &podman(),
                ContainerTerminalOp::Save {
                    reference: "docker.io/library/nginx:alpine".into(),
                    path: "/tmp/my image.tar".into()
                }
            )
            .unwrap(),
            "podman --url 'unix:///run/user/1000/podman/podman.sock' save -o '/tmp/my image.tar' 'docker.io/library/nginx:alpine'"
        );
        assert!(
            terminal_command(
                &podman(),
                ContainerTerminalOp::Export {
                    container_id: "a1b2c3d4e5f6".into(),
                    path: "/tmp/app.tar".into()
                }
            )
            .unwrap()
            .ends_with("export -o '/tmp/app.tar' a1b2c3d4e5f6")
        );
        assert!(
            terminal_command(
                &podman(),
                ContainerTerminalOp::Copy {
                    source: "web:/srv".into(),
                    destination: "/tmp/srv".into()
                }
            )
            .is_ok()
        );
        assert!(
            terminal_command(
                &podman(),
                ContainerTerminalOp::Load {
                    path: String::new()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn attach_pull_push_login_use_the_instance_prefix() {
        assert_eq!(
            terminal_command(
                &podman(),
                ContainerTerminalOp::Attach {
                    container_id: "a1b2c3d4e5f6".into()
                }
            )
            .unwrap(),
            "podman --url 'unix:///run/user/1000/podman/podman.sock' attach a1b2c3d4e5f6"
        );
        assert!(
            terminal_command(
                &podman(),
                ContainerTerminalOp::Pull {
                    reference: "nginx:alpine".into()
                }
            )
            .unwrap()
            .contains("pull 'nginx:alpine'")
        );
        assert!(
            terminal_command(
                &podman(),
                ContainerTerminalOp::Login {
                    registry: "ghcr.io".into()
                }
            )
            .unwrap()
            .ends_with("login 'ghcr.io'")
        );
    }
}
