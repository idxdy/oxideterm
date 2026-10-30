// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! Container backend abstraction shared by the Host Tools container panel.
//!
//! The crate is engine-agnostic: [`ContainerEngine`] selects the CLI family
//! (Docker, Podman, the WSL container CLI), [`ContainerTransport`] selects how
//! that CLI reaches its daemon (unix/npipe socket, TCP endpoint, plain CLI),
//! and every command builder produces shell text that runs on the monitored
//! host. Discovery, snapshots and command builders never execute anything
//! locally, so nothing here depends on the workstation having a container CLI.

mod command;
mod discovery;
mod engine;
mod instance;
mod model;
mod parse;
mod quote;
mod registries;
mod snapshot;
mod stats;
mod terminal;
mod transport;

pub use command::{
    ContainerAction, ContainerActionAvailability, ContainerCapture, ImageAction, NetworkAction,
    PodAction, PodActionAvailability, VolumeAction, capture_command, container_action_availability,
    container_action_command, create_network_command, create_pod_command, create_volume_command,
    exec_shell_command, follow_logs_command, healthcheck_command, image_action_command,
    logout_command, network_action_command, pod_action_availability, pod_action_command,
    prune_images_command, rename_container_command, tag_image_command, volume_action_command,
};
pub use discovery::{ContainerDiscovery, ContainerUnavailable, discovery_command, parse_discovery};
pub use engine::ContainerEngine;
pub use instance::{ContainerInstance, dedupe_instances, preferred_instance};
pub use model::{
    Container, ContainerSnapshot, ContainerStatus, Image, Network, Pod, Volume,
    container_row_signature, container_state_label_key, pod_row_signature, pod_state_label_key,
};
pub use parse::{parse_snapshot, split_json_prefix};
pub use registries::{
    ContainerRegistry, ContainerRegistrySource, parse_registries, registries_command,
};
pub use snapshot::snapshot_command;
pub use stats::{ContainerStat, parse_stats, stats_command};
pub use terminal::{ContainerTerminalOp, terminal_command};
pub use transport::{ContainerTransport, ContainerTransportKind};
