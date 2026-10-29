// Copyright (C) 2026 OxideTerm contributors.
// SPDX-License-Identifier: GPL-3.0-only

//! Command builders and envelope parsers for the herdr CLI.
//!
//! The protocol layer owns no process, terminal, or network transport: the
//! caller runs these commands on the SSH channel that already owns the
//! connection. Herdr speaks newline-delimited JSON envelopes natively, so
//! parsing keys on the envelope shape rather than screen-scraped tables.
//! Exit codes carry no signal: error envelopes return exit code zero
//! (measured against herdr 0.9.4), so action success is read from the
//! envelope by [`herdr_action_outcome`].

use std::time::Duration;

mod command;
mod ids;
mod model;
mod parse;
mod quote;

pub use command::{
    HerdrActionCommand, HerdrActionKind, HerdrAttachTarget, HerdrCaptureCommand,
    build_herdr_action_command, build_herdr_attach_command, build_herdr_read_command,
    build_herdr_session_detail_command, build_herdr_snapshot_command,
};
pub use model::{
    HerdrAgent, HerdrPane, HerdrSession, HerdrSessionDetail, HerdrSnapshot, HerdrStatus, HerdrTab,
    HerdrWorkspace, herdr_agent_matches_query, herdr_pane_matches_query,
    herdr_session_matches_query, herdr_status_label_key, herdr_tab_matches_query,
    herdr_workspace_matches_query,
};
pub use parse::{herdr_action_outcome, parse_herdr_session_detail, parse_herdr_snapshot};

pub const HERDR_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(12);
pub const HERDR_SNAPSHOT_MAX_OUTPUT_SIZE: usize = 512 * 1024;
pub const HERDR_READ_LINES: usize = 500;
