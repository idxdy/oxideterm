// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! Unified inventory shapes rendered by the container panel.
//!
//! Every engine contributes the same rows so the panel never branches on the
//! backend: Docker and `wslc` fill the same fields from `--format json`, Podman
//! adds pods and subnets, and unsupported fields stay empty.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerStatus {
    #[default]
    Unknown,
    Available,
    /// The CLI is missing or cannot reach any daemon.
    Unavailable {
        detail: Option<String>,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerSnapshot {
    pub status: ContainerStatus,
    #[serde(default)]
    pub containers: Vec<Container>,
    #[serde(default)]
    pub images: Vec<Image>,
    #[serde(default)]
    pub volumes: Vec<Volume>,
    #[serde(default)]
    pub networks: Vec<Network>,
    #[serde(default)]
    pub pods: Vec<Pod>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    /// Human status line (`Up 2 minutes`); empty when the engine omits it.
    pub status: String,
    pub ports: Option<String>,
    pub created: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub id: String,
    pub repository: String,
    pub tag: String,
    pub size: String,
    pub created: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    pub name: String,
    pub driver: String,
    pub mount_point: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
    /// Only Podman reports subnets in its list output.
    pub subnets: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pod {
    pub id: String,
    pub name: String,
    pub status: String,
    pub created: Option<String>,
    pub containers: usize,
}

impl Container {
    pub fn matches_query(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }
        [&self.id, &self.name, &self.image, &self.state, &self.status]
            .iter()
            .any(|value| value.to_lowercase().contains(&query))
    }
}

impl Pod {
    pub fn matches_query(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }
        [&self.id, &self.name, &self.status]
            .iter()
            .any(|value| value.to_lowercase().contains(&query))
    }
}

/// Row identity for the virtualized list: sampled status must not reset scroll.
pub fn container_row_signature(container: &Container) -> u64 {
    let mut hasher = DefaultHasher::new();
    container.id.hash(&mut hasher);
    hasher.finish()
}

pub fn pod_row_signature(pod: &Pod) -> u64 {
    let mut hasher = DefaultHasher::new();
    pod.id.hash(&mut hasher);
    hasher.finish()
}

pub fn container_state_label_key(state: &str) -> &'static str {
    match state.trim().to_ascii_lowercase().as_str() {
        "running" => "sidebar.host_container.states.running",
        "exited" => "sidebar.host_container.states.exited",
        "created" => "sidebar.host_container.states.created",
        "paused" => "sidebar.host_container.states.paused",
        "restarting" => "sidebar.host_container.states.restarting",
        "removing" => "sidebar.host_container.states.removing",
        "stopped" => "sidebar.host_container.states.stopped",
        "dead" => "sidebar.host_container.states.dead",
        _ => "sidebar.host_container.states.unknown",
    }
}

pub fn pod_state_label_key(status: &str) -> &'static str {
    match status.trim().to_ascii_lowercase().as_str() {
        "running" => "sidebar.host_container.states.running",
        "exited" => "sidebar.host_container.states.exited",
        "created" => "sidebar.host_container.states.created",
        "paused" => "sidebar.host_container.states.paused",
        "degraded" => "sidebar.host_container.states.degraded",
        "stopped" => "sidebar.host_container.states.stopped",
        _ => "sidebar.host_container.states.unknown",
    }
}
