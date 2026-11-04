// Copyright (C) 2026 jaime.su
// SPDX-License-Identifier: GPL-3.0-only

//! Remote eCapture session protocol for OxideTerm Host Tools.
//!
//! eCapture is a Linux eBPF collector that reads TLS plaintext from hooked
//! processes and must run as root on the traffic owner. The Host Tools panel
//! never opens a listener, forwards a port, or assumes anything beyond the SSH
//! exec channel: eCapture is started detached with its event stream redirected
//! to a per-connection file, polled incrementally over the same channel, and
//! stopped by signal. That keeps the tool usable on hosts where only TCP/22 is
//! reachable and port forwarding is disabled.
//!
//! This crate owns no transport and no GPUI state. It builds the POSIX scripts,
//! parses their framed output, and keeps the per-connection capture cursor and
//! event ring so the UI layer only schedules commands and renders rows.

mod command;
mod model;
mod parse;
mod session;
mod shell;
mod template;

pub use command::{
    ECAPTURE_POLL_LINES, ECAPTURE_POLL_LOG_LINES, EcaptureCommand, build_ecapture_clear_command,
    build_ecapture_poll_command, build_ecapture_probe_command, build_ecapture_start_command,
    build_ecapture_stop_command, ecapture_run_id,
};
pub use model::{
    ECAPTURE_MAX_EVENT_BYTES, ECAPTURE_MAX_EVENT_PAYLOAD_BYTES, ECAPTURE_MAX_EVENTS,
    ECAPTURE_MAX_LOG_LINES, ECAPTURE_MAX_PENDING_BYTES, ECAPTURE_MAX_STREAM_BYTES,
    EcaptureCommandCapability, EcaptureDirection, EcaptureEvent, EcaptureFilter, EcaptureKind,
    ResourceEcaptureProbe, ResourceEcaptureStatus, ecapture_capability_label_key,
    ecapture_direction_label_key, ecapture_event_row_signature, ecapture_filter_label_key,
    ecapture_kind_label_key, ecapture_validation_label_key,
};
pub use parse::{
    EcapturePoll, parse_ecapture_poll, parse_ecapture_probe, parse_ecapture_start_finished,
};
pub use session::EcaptureSession;
pub use template::{
    ECAPTURE_MAX_TARGET_LEN, EcaptureLaunch, EcaptureSpec, EcaptureTemplate,
    EcaptureValidationError, ecapture_template_label_key, validate_ecapture_launch,
};
