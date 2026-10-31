// Copyright (C) 2026 jaime.su
// SPDX-License-Identifier: GPL-3.0-only

//! UI-shaped eCapture probe, event, and label types.
//!
//! Captured payloads are TLS plaintext: they stay in memory only, are bounded
//! by the ring limits below, and never reach logs, audit records, or the typed
//! workspace event boundary.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::template::EcaptureValidationError;

/// Maximum retained events per capture session.
pub const ECAPTURE_MAX_EVENTS: usize = 500;
/// Maximum retained payload bytes per capture session.
pub const ECAPTURE_MAX_EVENT_BYTES: usize = 2 * 1024 * 1024;
/// Maximum retained payload bytes for one event.
pub const ECAPTURE_MAX_EVENT_PAYLOAD_BYTES: usize = 64 * 1024;
/// Maximum unfinished record text kept between polls.
pub const ECAPTURE_MAX_PENDING_BYTES: usize = 256 * 1024;
/// Maximum retained runtime log lines.
pub const ECAPTURE_MAX_LOG_LINES: usize = 40;
/// Stop polling once one session file reaches this size; the panel offers a
/// fresh start instead of letting a root process fill the host's `/tmp`.
pub const ECAPTURE_MAX_STREAM_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EcaptureCommandCapability {
    #[default]
    Unknown,
    Full,
    Partial,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceEcaptureStatus {
    #[default]
    Unknown,
    Available {
        capability: EcaptureCommandCapability,
        platform: String,
    },
    Unavailable,
    Error {
        message: String,
    },
}

/// Result of the availability probe on one host.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceEcaptureProbe {
    pub status: ResourceEcaptureStatus,
    pub binary: String,
    pub version: String,
    pub kernel: String,
    /// Root or passwordless `sudo`; eCapture cannot attach probes otherwise.
    pub privileged: bool,
    /// Kernel BTF is present, so CO-RE bytecode can load.
    pub btf: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EcaptureDirection {
    #[default]
    Unknown,
    /// Bytes read from the socket: received data.
    Read,
    /// Bytes written to the socket: sent data.
    Write,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EcaptureKind {
    #[default]
    Data,
    Request,
    Response,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EcaptureFilter {
    #[default]
    All,
    Requests,
    Responses,
}

/// One decrypted payload record emitted by eCapture.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EcaptureEvent {
    /// Monotonic session identity; the list uses it for row identity.
    pub sequence: u64,
    pub timestamp: String,
    pub pid: u32,
    pub tid: u32,
    pub comm: String,
    pub fd: u32,
    /// `source->destination` when eCapture resolved the connection tuple.
    pub tuple: String,
    pub direction: EcaptureDirection,
    pub kind: EcaptureKind,
    /// Declared payload length reported by eCapture.
    pub length: usize,
    pub payload: String,
    /// First meaningful line, used as the list summary.
    pub summary: String,
    pub payload_truncated: bool,
}

pub fn ecapture_capability_label_key(capability: EcaptureCommandCapability) -> &'static str {
    match capability {
        EcaptureCommandCapability::Unknown => "sidebar.host_ecapture.capability.unknown",
        EcaptureCommandCapability::Full => "sidebar.host_ecapture.capability.full",
        EcaptureCommandCapability::Partial => "sidebar.host_ecapture.capability.partial",
    }
}

pub fn ecapture_direction_label_key(direction: EcaptureDirection) -> &'static str {
    match direction {
        EcaptureDirection::Unknown => "sidebar.host_ecapture.direction.unknown",
        EcaptureDirection::Read => "sidebar.host_ecapture.direction.read",
        EcaptureDirection::Write => "sidebar.host_ecapture.direction.write",
    }
}

pub fn ecapture_kind_label_key(kind: EcaptureKind) -> &'static str {
    match kind {
        EcaptureKind::Data => "sidebar.host_ecapture.kinds.data",
        EcaptureKind::Request => "sidebar.host_ecapture.kinds.request",
        EcaptureKind::Response => "sidebar.host_ecapture.kinds.response",
    }
}

pub fn ecapture_filter_label_key(filter: EcaptureFilter) -> &'static str {
    match filter {
        EcaptureFilter::All => "sidebar.host_ecapture.filters.all",
        EcaptureFilter::Requests => "sidebar.host_ecapture.filters.requests",
        EcaptureFilter::Responses => "sidebar.host_ecapture.filters.responses",
    }
}

pub fn ecapture_validation_label_key(error: EcaptureValidationError) -> &'static str {
    match error {
        EcaptureValidationError::TargetRequired => {
            "sidebar.host_ecapture.validation.target_required"
        }
        EcaptureValidationError::TargetTooLong => {
            "sidebar.host_ecapture.validation.target_too_long"
        }
        EcaptureValidationError::TargetInvalid => "sidebar.host_ecapture.validation.target_invalid",
        EcaptureValidationError::PidInvalid => "sidebar.host_ecapture.validation.pid_invalid",
    }
}

/// Row identity excludes payload bytes so a live stream does not resync the
/// virtual list on every poll; expansion participates because it changes geometry.
pub fn ecapture_event_row_signature(event: &EcaptureEvent, expanded: bool) -> u64 {
    let mut hasher = DefaultHasher::new();
    event.sequence.hash(&mut hasher);
    expanded.hash(&mut hasher);
    hasher.finish()
}
