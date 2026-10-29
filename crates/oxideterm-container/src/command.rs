// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! Mutating actions and read-only captures.
//!
//! Commands are composed from the instance's CLI prefix, so every backend and
//! transport shares one code path. `wslc` differences are limited to its
//! `remove` verb and its missing pause/image-history support; both are handled
//! by capability checks instead of dialect branches in the panel.

use crate::instance::ContainerInstance;
use crate::quote::{shell_quote_single, validated_container_id, validated_resource_name};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContainerAction {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
    Remove,
}

impl ContainerAction {
    pub fn id(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Pause => "pause",
            Self::Unpause => "unpause",
            Self::Remove => "remove",
        }
    }

    pub fn label_key(self) -> &'static str {
        match self {
            Self::Start => "sidebar.host_container.actions.start",
            Self::Stop => "sidebar.host_container.actions.stop",
            Self::Restart => "sidebar.host_container.actions.restart",
            Self::Pause => "sidebar.host_container.actions.pause",
            Self::Unpause => "sidebar.host_container.actions.unpause",
            Self::Remove => "sidebar.host_container.actions.remove",
        }
    }

    pub fn is_destructive(self) -> bool {
        matches!(self, Self::Remove)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ContainerActionAvailability {
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

/// Unknown states expose no mutating affordances.
pub fn container_action_availability(state: &str) -> ContainerActionAvailability {
    let state = state.trim().to_ascii_lowercase();
    let running = matches!(state.as_str(), "running" | "restarting");
    ContainerActionAvailability {
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PodActionAvailability {
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
pub fn pod_action_availability(status: &str) -> PodActionAvailability {
    let status = status.trim().to_ascii_lowercase();
    let running = matches!(status.as_str(), "running" | "degraded");
    PodActionAvailability {
        can_start: matches!(
            status.as_str(),
            "exited" | "stopped" | "created" | "degraded"
        ),
        can_stop: running,
        can_restart: running,
        can_pause: status == "running",
        can_unpause: status == "paused",
        can_remove: matches!(status.as_str(), "exited" | "stopped" | "created"),
    }
}

pub fn container_action_command(
    os_type: &str,
    instance: &ContainerInstance,
    action: ContainerAction,
    container_id: &str,
) -> Result<String, String> {
    let container_id = validated_container_id(container_id)?;
    if !matches!(
        action,
        ContainerAction::Start
            | ContainerAction::Stop
            | ContainerAction::Restart
            | ContainerAction::Remove
            | ContainerAction::Pause
            | ContainerAction::Unpause
    ) {
        return Err("Unsupported container action.".to_string());
    }
    if matches!(action, ContainerAction::Pause | ContainerAction::Unpause)
        && !instance.engine.supports_pause()
    {
        return Err("This engine does not support pausing containers.".to_string());
    }
    let operation = match action {
        ContainerAction::Remove => instance.engine.container_remove_command(),
        other => other.id(),
    };
    Ok(wrapped_command(
        os_type,
        format!("{} {operation} {container_id}", instance.cli_prefix()),
    ))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageAction {
    Remove,
    /// Dangling (untagged) images only.
    Prune,
}

impl ImageAction {
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Remove => "sidebar.host_container.actions.remove",
            Self::Prune => "sidebar.host_container.actions.prune",
        }
    }

    pub fn is_destructive(self) -> bool {
        true
    }
}

pub fn image_action_command(
    os_type: &str,
    instance: &ContainerInstance,
    action: ImageAction,
    image_id: Option<&str>,
) -> Result<String, String> {
    let prefix = instance.cli_prefix();
    let command = match action {
        ImageAction::Remove => {
            let image_id = validated_container_id(image_id.unwrap_or_default())?;
            format!("{prefix} rmi {image_id}")
        }
        ImageAction::Prune => format!("{prefix} image prune -f"),
    };
    Ok(wrapped_command(os_type, command))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VolumeAction {
    Remove,
    Prune,
}

impl VolumeAction {
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Remove => "sidebar.host_container.actions.remove",
            Self::Prune => "sidebar.host_container.actions.prune",
        }
    }

    pub fn is_destructive(self) -> bool {
        true
    }
}

pub fn volume_action_command(
    os_type: &str,
    instance: &ContainerInstance,
    action: VolumeAction,
    name: Option<&str>,
) -> Result<String, String> {
    let prefix = instance.cli_prefix();
    let command = match action {
        VolumeAction::Remove => {
            let name = validated_resource_name(name.unwrap_or_default())?;
            format!("{prefix} volume rm {}", shell_quote_single(name))
        }
        VolumeAction::Prune => format!("{prefix} volume prune -f"),
    };
    Ok(wrapped_command(os_type, command))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkAction {
    Remove,
    Prune,
}

impl NetworkAction {
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Remove => "sidebar.host_container.actions.remove",
            Self::Prune => "sidebar.host_container.actions.prune",
        }
    }

    pub fn is_destructive(self) -> bool {
        true
    }
}

pub fn network_action_command(
    os_type: &str,
    instance: &ContainerInstance,
    action: NetworkAction,
    name: Option<&str>,
) -> Result<String, String> {
    let prefix = instance.cli_prefix();
    let command = match action {
        NetworkAction::Remove => {
            let name = validated_resource_name(name.unwrap_or_default())?;
            format!("{prefix} network rm {}", shell_quote_single(name))
        }
        NetworkAction::Prune => format!("{prefix} network prune -f"),
    };
    Ok(wrapped_command(os_type, command))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PodAction {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
    Remove,
}

impl PodAction {
    pub fn id(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Pause => "pause",
            Self::Unpause => "unpause",
            Self::Remove => "remove",
        }
    }

    pub fn label_key(self) -> &'static str {
        match self {
            Self::Start => "sidebar.host_container.actions.start",
            Self::Stop => "sidebar.host_container.actions.stop",
            Self::Restart => "sidebar.host_container.actions.restart",
            Self::Pause => "sidebar.host_container.actions.pause",
            Self::Unpause => "sidebar.host_container.actions.unpause",
            Self::Remove => "sidebar.host_container.actions.remove",
        }
    }

    pub fn is_destructive(self) -> bool {
        matches!(self, Self::Remove)
    }
}

pub fn pod_action_command(
    os_type: &str,
    instance: &ContainerInstance,
    action: PodAction,
    pod_id: &str,
) -> Result<String, String> {
    if !instance.engine.supports_pods() {
        return Err("This engine does not support pods.".to_string());
    }
    let pod_id = validated_resource_name(pod_id)?;
    let operation = match action {
        PodAction::Remove => "rm",
        other => other.id(),
    };
    Ok(wrapped_command(
        os_type,
        format!("{} pod {operation} {pod_id}", instance.cli_prefix()),
    ))
}

/// Read-only outputs rendered in the shared text dialog.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContainerCapture {
    ContainerInspect { container_id: String },
    ContainerLogs { container_id: String },
    ImageInspect { image_id: String },
    ImageHistory { image_id: String },
    VolumeInspect { name: String },
    VolumeUsage { name: String },
    NetworkInspect { name: String },
    PodInspect { pod_id: String },
    Search { term: String },
    Top { container_id: String },
    Diff { container_id: String },
}

pub fn capture_command(
    os_type: &str,
    instance: &ContainerInstance,
    capture: &ContainerCapture,
) -> Result<String, String> {
    let prefix = instance.cli_prefix();
    let command = match capture {
        ContainerCapture::ContainerInspect { container_id } => {
            let id = validated_container_id(container_id)?;
            format!("{prefix} inspect {id}")
        }
        ContainerCapture::ContainerLogs { container_id } => {
            let id = validated_container_id(container_id)?;
            format!("{prefix} logs --tail 200 --timestamps {id}")
        }
        ContainerCapture::ImageInspect { image_id } => {
            let id = validated_container_id(image_id)?;
            format!("{prefix} image inspect {id}")
        }
        ContainerCapture::ImageHistory { image_id } => {
            if !instance.engine.supports_image_history() {
                return Err("This engine does not support image history.".to_string());
            }
            let id = validated_container_id(image_id)?;
            format!("{prefix} history {id}")
        }
        ContainerCapture::VolumeInspect { name } => {
            let name = validated_resource_name(name)?;
            format!("{prefix} volume inspect {}", shell_quote_single(name))
        }
        ContainerCapture::NetworkInspect { name } => {
            let name = validated_resource_name(name)?;
            format!("{prefix} network inspect {}", shell_quote_single(name))
        }
        ContainerCapture::VolumeUsage { name } => {
            let name = validated_resource_name(name)?;
            format!(
                "{prefix} ps -a --filter volume={} --format '{{{{.Names}}}}'",
                shell_quote_single(name)
            )
        }
        ContainerCapture::PodInspect { pod_id } => {
            if !instance.engine.supports_pods() {
                return Err("This engine does not support pods.".to_string());
            }
            let id = validated_resource_name(pod_id)?;
            format!("{prefix} pod inspect {id}")
        }
        ContainerCapture::Search { term } => {
            if !instance.engine.supports_search() {
                return Err("This engine does not support image search.".to_string());
            }
            let term = validated_resource_name(term)?;
            format!("{prefix} search --limit 20 {}", shell_quote_single(term))
        }
        ContainerCapture::Top { container_id } => {
            if !instance.engine.supports_container_inspection() {
                return Err("This engine does not support container top.".to_string());
            }
            let id = validated_container_id(container_id)?;
            format!("{prefix} top {id}")
        }
        ContainerCapture::Diff { container_id } => {
            if !instance.engine.supports_container_inspection() {
                return Err("This engine does not support container diff.".to_string());
            }
            let id = validated_container_id(container_id)?;
            format!("{prefix} diff {id}")
        }
    };
    Ok(match os_type {
        "Windows" | "windows" => {
            format!("powershell -NoProfile -ExecutionPolicy Bypass -Command \"{command} 2>&1\"")
        }
        _ => format!("{command} 2>&1"),
    })
}

/// Interactive terminal command: an interactive shell inside a running
/// container. The terminal owns the stream; nothing is captured or audited.
pub fn exec_shell_command(
    instance: &ContainerInstance,
    container_id: &str,
) -> Result<String, String> {
    let container_id = validated_container_id(container_id)?;
    Ok(format!(
        "{} exec -it {container_id} sh -lc 'if command -v bash >/dev/null 2>&1; then exec bash; else exec sh; fi'",
        instance.cli_prefix()
    ))
}

/// Interactive terminal command: follow a container's log stream.
pub fn follow_logs_command(
    instance: &ContainerInstance,
    container_id: &str,
) -> Result<String, String> {
    let container_id = validated_container_id(container_id)?;
    Ok(format!(
        "{} logs -f --tail 200 --timestamps {container_id}",
        instance.cli_prefix()
    ))
}

fn wrapped_command(os_type: &str, command: String) -> String {
    match os_type {
        "Windows" | "windows" => {
            format!("powershell -NoProfile -ExecutionPolicy Bypass -Command \"{command} 2>&1\"")
        }
        _ => format!("{command} 2>&1"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::ContainerEngine;
    use crate::transport::ContainerTransport;

    fn instance(engine: ContainerEngine) -> ContainerInstance {
        ContainerInstance::new(
            engine,
            ContainerTransport::Socket {
                endpoint: "unix:///var/run/docker.sock".into(),
            },
            None,
        )
    }

    const ID: &str = "a1b2c3d4e5f6";

    #[test]
    fn container_actions_carry_the_transport_and_engine_remove_verb() {
        let docker = instance(ContainerEngine::Docker);
        assert_eq!(
            container_action_command("Linux", &docker, ContainerAction::Start, ID).unwrap(),
            "docker -H 'unix:///var/run/docker.sock' start a1b2c3d4e5f6 2>&1"
        );
        assert_eq!(
            container_action_command("Linux", &docker, ContainerAction::Remove, ID).unwrap(),
            "docker -H 'unix:///var/run/docker.sock' rm a1b2c3d4e5f6 2>&1"
        );
        let wslc = ContainerInstance::new(ContainerEngine::Wslc, ContainerTransport::Cli, None);
        assert_eq!(
            container_action_command("Windows", &wslc, ContainerAction::Remove, ID).unwrap(),
            "powershell -NoProfile -ExecutionPolicy Bypass -Command \"wslc remove a1b2c3d4e5f6 2>&1\""
        );
        assert!(container_action_command("Windows", &wslc, ContainerAction::Pause, ID).is_err());
        assert!(container_action_command("Linux", &docker, ContainerAction::Stop, "abc").is_err());
    }

    #[test]
    fn resource_actions_validate_names_and_prune_flags() {
        let podman = instance(ContainerEngine::Podman);
        assert_eq!(
            volume_action_command("Linux", &podman, VolumeAction::Remove, Some("app-data"))
                .unwrap(),
            "podman --url 'unix:///var/run/docker.sock' volume rm 'app-data' 2>&1"
        );
        assert_eq!(
            network_action_command("Linux", &podman, NetworkAction::Prune, None).unwrap(),
            "podman --url 'unix:///var/run/docker.sock' network prune -f 2>&1"
        );
        assert!(volume_action_command("Linux", &podman, VolumeAction::Remove, None).is_err());
        assert!(
            image_action_command("Linux", &podman, ImageAction::Remove, Some("a1b2c3d4e5f6"))
                .is_ok()
        );
        assert!(
            image_action_command("Linux", &podman, ImageAction::Remove, Some("nginx:latest"))
                .is_err()
        );
        assert_eq!(
            image_action_command("Linux", &podman, ImageAction::Prune, None).unwrap(),
            "podman --url 'unix:///var/run/docker.sock' image prune -f 2>&1"
        );
    }

    #[test]
    fn pod_actions_require_podman() {
        let podman = instance(ContainerEngine::Podman);
        assert_eq!(
            pod_action_command("Linux", &podman, PodAction::Stop, "pod_ldap").unwrap(),
            "podman --url 'unix:///var/run/docker.sock' pod stop pod_ldap 2>&1"
        );
        assert_eq!(
            pod_action_command("Linux", &podman, PodAction::Remove, "pod_ldap").unwrap(),
            "podman --url 'unix:///var/run/docker.sock' pod rm pod_ldap 2>&1"
        );
        let docker = instance(ContainerEngine::Docker);
        assert!(pod_action_command("Linux", &docker, PodAction::Stop, "pod_ldap").is_err());
    }

    #[test]
    fn captures_cover_every_inventory_and_reject_unsupported_ones() {
        let docker = instance(ContainerEngine::Docker);
        assert_eq!(
            capture_command(
                "Linux",
                &docker,
                &ContainerCapture::ContainerInspect {
                    container_id: ID.into()
                }
            )
            .unwrap(),
            "docker -H 'unix:///var/run/docker.sock' inspect a1b2c3d4e5f6 2>&1"
        );
        assert_eq!(
            capture_command(
                "Linux",
                &docker,
                &ContainerCapture::ContainerLogs {
                    container_id: ID.into()
                }
            )
            .unwrap(),
            "docker -H 'unix:///var/run/docker.sock' logs --tail 200 --timestamps a1b2c3d4e5f6 2>&1"
        );
        let wslc = ContainerInstance::new(ContainerEngine::Wslc, ContainerTransport::Cli, None);
        assert!(
            capture_command(
                "Windows",
                &wslc,
                &ContainerCapture::ImageHistory {
                    image_id: ID.into()
                }
            )
            .is_err()
        );
        assert!(
            capture_command(
                "Windows",
                &wslc,
                &ContainerCapture::VolumeInspect {
                    name: "app-data".into()
                }
            )
            .unwrap()
            .contains("wslc volume inspect 'app-data'")
        );
        assert!(
            capture_command(
                "Linux",
                &docker,
                &ContainerCapture::PodInspect {
                    pod_id: "pod_ldap".into()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn availability_matrices_match_container_states() {
        assert!(container_action_availability("running").can_stop);
        assert!(!container_action_availability("running").can_remove);
        assert!(container_action_availability("exited").can_start);
        assert!(container_action_availability("exited").can_remove);
        assert!(container_action_availability("paused").can_unpause);
        assert!(!container_action_availability("paused").can_exec);
        assert!(container_action_availability("paused").can_use_live_tools);
        assert!(!container_action_availability("dead").can_start);
        assert!(pod_action_availability("degraded").can_stop);
        assert!(!pod_action_availability("degraded").can_remove);
        assert!(pod_action_availability("Exited").can_remove);
    }

    #[test]
    fn terminal_commands_use_the_instance_prefix() {
        let podman = instance(ContainerEngine::Podman);
        assert_eq!(
            exec_shell_command(&podman, ID).unwrap(),
            "podman --url 'unix:///var/run/docker.sock' exec -it a1b2c3d4e5f6 sh -lc 'if command -v bash >/dev/null 2>&1; then exec bash; else exec sh; fi'"
        );
        assert_eq!(
            follow_logs_command(&podman, ID).unwrap(),
            "podman --url 'unix:///var/run/docker.sock' logs -f --tail 200 --timestamps a1b2c3d4e5f6"
        );
    }
}

// ---- lifecycle extras ----

pub fn create_volume_command(
    os_type: &str,
    instance: &ContainerInstance,
    name: &str,
) -> Result<String, String> {
    let name = validated_resource_name(name)?;
    Ok(wrapped_command(
        os_type,
        format!(
            "{} volume create {}",
            instance.cli_prefix(),
            shell_quote_single(name)
        ),
    ))
}

pub fn create_network_command(
    os_type: &str,
    instance: &ContainerInstance,
    name: &str,
    driver: Option<&str>,
) -> Result<String, String> {
    let name = validated_resource_name(name)?;
    let driver = match driver.map(str::trim).filter(|driver| !driver.is_empty()) {
        Some(driver) => {
            let driver = validated_resource_name(driver)?;
            format!(" --driver {}", shell_quote_single(driver))
        }
        None => String::new(),
    };
    Ok(wrapped_command(
        os_type,
        format!(
            "{} network create{driver} {}",
            instance.cli_prefix(),
            shell_quote_single(name)
        ),
    ))
}

pub fn create_pod_command(
    os_type: &str,
    instance: &ContainerInstance,
    name: &str,
) -> Result<String, String> {
    if !instance.engine.supports_pods() {
        return Err("This engine does not support pods.".to_string());
    }
    let name = validated_resource_name(name)?;
    Ok(wrapped_command(
        os_type,
        format!(
            "{} pod create --name {}",
            instance.cli_prefix(),
            shell_quote_single(name)
        ),
    ))
}

pub fn rename_container_command(
    os_type: &str,
    instance: &ContainerInstance,
    container_id: &str,
    new_name: &str,
) -> Result<String, String> {
    if !instance.engine.supports_container_maintenance() {
        return Err("This engine does not support renaming containers.".to_string());
    }
    let container_id = validated_container_id(container_id)?;
    let new_name = validated_resource_name(new_name)?;
    Ok(wrapped_command(
        os_type,
        format!(
            "{} rename {container_id} {}",
            instance.cli_prefix(),
            shell_quote_single(new_name)
        ),
    ))
}

pub fn tag_image_command(
    os_type: &str,
    instance: &ContainerInstance,
    image_id: &str,
    reference: &str,
) -> Result<String, String> {
    let image_id = validated_container_id(image_id)?;
    let reference = validated_resource_name(reference)?;
    Ok(wrapped_command(
        os_type,
        format!(
            "{} tag {image_id} {}",
            instance.cli_prefix(),
            shell_quote_single(reference)
        ),
    ))
}

pub fn healthcheck_command(
    os_type: &str,
    instance: &ContainerInstance,
    container_id: &str,
) -> Result<String, String> {
    if !instance.engine.supports_container_maintenance() {
        return Err("This engine does not support healthchecks.".to_string());
    }
    let container_id = validated_container_id(container_id)?;
    Ok(wrapped_command(
        os_type,
        format!("{} healthcheck run {container_id}", instance.cli_prefix()),
    ))
}

/// Dangling images only unless `all` is set, which also drops unused tagged images.
pub fn prune_images_command(
    os_type: &str,
    instance: &ContainerInstance,
    all: bool,
) -> Result<String, String> {
    let suffix = if all { " -a" } else { "" };
    Ok(wrapped_command(
        os_type,
        format!("{} image prune{suffix} -f", instance.cli_prefix()),
    ))
}

pub fn logout_command(
    os_type: &str,
    instance: &ContainerInstance,
    registry: &str,
) -> Result<String, String> {
    let registry = validated_resource_name(registry)?;
    Ok(wrapped_command(
        os_type,
        format!(
            "{} logout {}",
            instance.cli_prefix(),
            shell_quote_single(registry)
        ),
    ))
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::engine::ContainerEngine;
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

    fn wslc() -> ContainerInstance {
        ContainerInstance::new(ContainerEngine::Wslc, ContainerTransport::Cli, None)
    }

    #[test]
    fn creation_commands_quote_names_and_drivers() {
        assert_eq!(
            create_volume_command("Linux", &podman(), "app data").is_err(),
            true
        );
        assert_eq!(
            create_volume_command("Linux", &podman(), "app-data").unwrap(),
            "podman --url 'unix:///run/user/1000/podman/podman.sock' volume create 'app-data' 2>&1"
        );
        assert_eq!(
            create_network_command("Linux", &podman(), "front", Some("bridge")).unwrap(),
            "podman --url 'unix:///run/user/1000/podman/podman.sock' network create --driver 'bridge' 'front' 2>&1"
        );
        assert_eq!(
            create_pod_command("Linux", &podman(), "web").unwrap(),
            "podman --url 'unix:///run/user/1000/podman/podman.sock' pod create --name 'web' 2>&1"
        );
        assert!(create_pod_command("Linux", &wslc(), "web").is_err());
    }

    #[test]
    fn maintenance_commands_validate_identifiers_and_capabilities() {
        assert_eq!(
            rename_container_command("Linux", &podman(), "a1b2c3d4e5f6", "web").unwrap(),
            "podman --url 'unix:///run/user/1000/podman/podman.sock' rename a1b2c3d4e5f6 'web' 2>&1"
        );
        assert!(rename_container_command("Linux", &wslc(), "a1b2c3d4e5f6", "web").is_err());
        assert_eq!(
            tag_image_command("Linux", &podman(), "a1b2c3d4e5f6", "registry/web:1").unwrap(),
            "podman --url 'unix:///run/user/1000/podman/podman.sock' tag a1b2c3d4e5f6 'registry/web:1' 2>&1"
        );
        assert!(tag_image_command("Linux", &podman(), "a1b2c3d4e5f6", "bad name").is_err());
        assert!(healthcheck_command("Linux", &wslc(), "a1b2c3d4e5f6").is_err());
        assert_eq!(
            prune_images_command("Linux", &podman(), true).unwrap(),
            "podman --url 'unix:///run/user/1000/podman/podman.sock' image prune -a -f 2>&1"
        );
        assert_eq!(
            logout_command("Linux", &podman(), "ghcr.io").unwrap(),
            "podman --url 'unix:///run/user/1000/podman/podman.sock' logout 'ghcr.io' 2>&1"
        );
    }

    #[test]
    fn search_top_diff_and_usage_captures_respect_capabilities() {
        assert_eq!(
            capture_command(
                "Linux",
                &podman(),
                &ContainerCapture::Search {
                    term: "nginx".into()
                }
            )
            .unwrap(),
            "podman --url 'unix:///run/user/1000/podman/podman.sock' search --limit 20 'nginx' 2>&1"
        );
        assert!(
            capture_command(
                "Windows",
                &wslc(),
                &ContainerCapture::Search {
                    term: "nginx".into()
                }
            )
            .is_err()
        );
        assert_eq!(
            capture_command(
                "Linux",
                &podman(),
                &ContainerCapture::Top {
                    container_id: "a1b2c3d4e5f6".into()
                }
            )
            .unwrap(),
            "podman --url 'unix:///run/user/1000/podman/podman.sock' top a1b2c3d4e5f6 2>&1"
        );
        assert!(
            capture_command(
                "Linux",
                &podman(),
                &ContainerCapture::VolumeUsage {
                    name: "app-data".into()
                }
            )
            .unwrap()
            .contains("ps -a --filter volume='app-data'")
        );
    }
}
