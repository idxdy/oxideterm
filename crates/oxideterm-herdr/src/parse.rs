// Copyright (C) 2026 jaime.su
// SPDX-License-Identifier: GPL-3.0-only

//! Envelope parsing. Each capture prints section markers around one JSON
//! document per list; the parsers walk those sections and read herdr's error
//! envelopes, which are the only failure signal herdr emits.

use crate::model::{
    HerdrAgent, HerdrPane, HerdrSession, HerdrSessionDetail, HerdrSnapshot, HerdrStatus, HerdrTab,
    HerdrWorkspace,
};

const HERDR_UNAVAILABLE_MARKER: &str = "__OXIDE_HERDR_UNAVAILABLE__";
const HERDR_SECTION_PREFIX: &str = "===HERDR_";
const HERDR_SNAPSHOT_END: &str = "===HERDR_END===";
const HERDR_ERROR_MESSAGE_MAX_CHARS: usize = 180;

/// A failed herdr action still exits zero and reports the failure as a JSON
/// envelope, so the envelope decides; the exit code only covers invocations
/// that produced no envelope at all.
pub fn herdr_action_outcome(stdout: &str, exit_code: Option<i32>) -> bool {
    let envelope_failed = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .any(|value| value.get("error").is_some_and(|error| !error.is_null()));
    !envelope_failed && exit_code.unwrap_or(0) == 0
}

/// Lines between `start_marker` and the next section marker. Markers are
/// matched as whole lines because workspace labels and pane cwds are server
/// controlled and may carry marker text inside a JSON document.
fn section_lines<'a>(output: &'a str, start_marker: &str) -> Vec<&'a str> {
    let mut lines = output
        .lines()
        .skip_while(|line| line.trim() != start_marker);
    lines.next();
    lines
        .map(str::trim)
        .take_while(|line| !line.starts_with(HERDR_SECTION_PREFIX) && *line != HERDR_SNAPSHOT_END)
        .collect()
}

fn bounded_error_message(message: &str) -> String {
    let cleaned: String = message
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(HERDR_ERROR_MESSAGE_MAX_CHARS)
        .collect();
    let cleaned = cleaned.trim();
    (!cleaned.is_empty())
        .then(|| cleaned.to_string())
        .unwrap_or_else(|| "herdr command failed.".to_string())
}

fn parse_status_line(line: &str) -> Option<HerdrStatus> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    // A success document may still carry `"error": null`; only a populated
    // error member makes this line a status.
    if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
        let code = error.get("code").and_then(|c| c.as_str()).unwrap_or("");
        let message = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or_default();
        if code == "server_not_running" {
            return Some(HerdrStatus::ServerNotRunning);
        }
        return Some(HerdrStatus::Error {
            message: bounded_error_message(message),
        });
    }
    None
}

fn parse_workspace(value: &serde_json::Value) -> Option<HerdrWorkspace> {
    let workspace_id = value.get("workspace_id")?.as_str()?.to_string();
    Some(HerdrWorkspace {
        label: value
            .get("label")
            .and_then(|v| v.as_str())
            .map(|label| label.chars().take(128).collect())
            .unwrap_or_else(|| workspace_id.clone()),
        number: value.get("number").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        focused: value
            .get("focused")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        pane_count: value
            .get("pane_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        tab_count: value.get("tab_count").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        agent_status: value
            .get("agent_status")
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(32).collect()),
        workspace_id,
    })
}

fn parse_tab(value: &serde_json::Value) -> Option<HerdrTab> {
    Some(HerdrTab {
        tab_id: value.get("tab_id")?.as_str()?.to_string(),
        workspace_id: value.get("workspace_id")?.as_str()?.to_string(),
        label: value
            .get("label")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .map(|label| label.chars().take(128).collect()),
        number: value.get("number").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        pane_count: value
            .get("pane_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        focused: value
            .get("focused")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        agent_status: value
            .get("agent_status")
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(32).collect()),
    })
}

fn parse_pane(value: &serde_json::Value) -> Option<HerdrPane> {
    Some(HerdrPane {
        pane_id: value.get("pane_id")?.as_str()?.to_string(),
        workspace_id: value.get("workspace_id")?.as_str()?.to_string(),
        tab_id: value
            .get("tab_id")
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(64).collect()),
        label: value
            .get("label")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .map(|label| label.chars().take(128).collect()),
        terminal_title: value
            .get("terminal_title")
            .or_else(|| value.get("terminal_title_stripped"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(|title| title.chars().take(160).collect()),
        cwd: value
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(256).collect()),
        focused: value
            .get("focused")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        agent_status: value
            .get("agent_status")
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(32).collect()),
        restore_failed: value
            .get("restore_error")
            .and_then(|v| v.as_str())
            .is_some_and(|error| !error.is_empty()),
    })
}

fn parse_session(value: &serde_json::Value) -> Option<HerdrSession> {
    let name = value.get("name")?.as_str()?;
    // The attach builder trims and length-limits this name, so a row whose
    // name would change there is dropped instead of attaching elsewhere.
    if name.is_empty()
        || name.trim() != name
        || name.chars().count() > 128
        || name.chars().any(char::is_control)
    {
        return None;
    }
    Some(HerdrSession {
        name: name.to_string(),
        running: value
            .get("running")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    })
}

fn parse_agent(value: &serde_json::Value) -> Option<HerdrAgent> {
    Some(HerdrAgent {
        // The pane reference is the only field the row joins on; an agent
        // without one cannot be attached to or rendered against a pane.
        pane_id: value
            .get("pane_id")
            .or_else(|| value.get("target"))
            .and_then(|v| v.as_str())?
            .to_string(),
        workspace_id: value
            .get("workspace_id")
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(64).collect()),
        label: value
            .get("label")
            .or_else(|| value.get("name"))
            // herdr reports the agent kind under `agent` and the pane title
            // separately; either is a better row label than the raw pane id.
            .or_else(|| value.get("agent"))
            .or_else(|| value.get("terminal_title"))
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(128).collect()),
        status: value
            .get("status")
            .or_else(|| value.get("agent_status"))
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(32).collect()),
    })
}

/// Walks one section's lines as candidate JSON documents. Success documents
/// feed the matching list; the first error envelope decides the status.
fn parse_herdr_section<T>(
    lines: &[&str],
    extract: impl Fn(&serde_json::Value) -> Option<T> + Copy,
    carrier: impl Fn(&serde_json::Value) -> Option<&serde_json::Value> + Copy,
) -> (Vec<T>, Option<HerdrStatus>) {
    let mut items = Vec::new();
    let mut status = None;
    for line in lines.iter().filter(|line| !line.is_empty()) {
        if let Some(line_status) = parse_status_line(line) {
            if status.is_none() {
                status = Some(line_status);
            }
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            // Non-JSON noise (shell banners) never reaches the tables.
            continue;
        };
        if let Some(list) = carrier(&value) {
            if let Some(array) = list.as_array() {
                for entry in array {
                    if let Some(item) = extract(entry) {
                        items.push(item);
                    }
                }
            }
        } else if let Some(item) = extract(&value) {
            items.push(item);
        }
    }
    (items, status)
}

fn fold_status(
    section_status: Option<HerdrStatus>,
    server_down: &mut bool,
    error: &mut Option<HerdrStatus>,
) {
    match section_status {
        Some(HerdrStatus::ServerNotRunning) => *server_down = true,
        Some(status) if error.is_none() => *error = Some(status),
        _ => {}
    }
}

fn resolve_status(server_down: bool, error: Option<HerdrStatus>) -> HerdrStatus {
    if server_down {
        HerdrStatus::ServerNotRunning
    } else if let Some(error) = error {
        error
    } else {
        // No error envelope: the lists either produced rows or legitimately
        // produced nothing, both of which still mean a reachable server.
        HerdrStatus::Available
    }
}

pub fn parse_herdr_snapshot(output: &str) -> HerdrSnapshot {
    if output
        .lines()
        .any(|line| line.trim() == HERDR_UNAVAILABLE_MARKER)
    {
        return HerdrSnapshot {
            status: HerdrStatus::Unavailable,
            ..HerdrSnapshot::default()
        };
    }
    let mut error: Option<HerdrStatus> = None;
    let mut server_down = false;

    let (sessions, status) = parse_herdr_section(
        &section_lines(output, "===HERDR_SESSIONS==="),
        parse_session,
        |value| value.get("sessions"),
    );
    fold_status(status, &mut server_down, &mut error);

    HerdrSnapshot {
        status: resolve_status(server_down, error),
        sessions,
    }
}

pub fn parse_herdr_session_detail(output: &str) -> HerdrSessionDetail {
    if output
        .lines()
        .any(|line| line.trim() == HERDR_UNAVAILABLE_MARKER)
    {
        return HerdrSessionDetail {
            status: HerdrStatus::Unavailable,
            ..HerdrSessionDetail::default()
        };
    }
    let mut detail = HerdrSessionDetail::default();
    let mut error: Option<HerdrStatus> = None;
    let mut server_down = false;

    let (workspaces, status) = parse_herdr_section(
        &section_lines(output, "===HERDR_WORKSPACES==="),
        parse_workspace,
        |value| value.get("result").and_then(|r| r.get("workspaces")),
    );
    detail.workspaces = workspaces;
    fold_status(status, &mut server_down, &mut error);

    let (tabs, status) = parse_herdr_section(
        &section_lines(output, "===HERDR_TABS==="),
        parse_tab,
        |value| value.get("result").and_then(|r| r.get("tabs")),
    );
    detail.tabs = tabs;
    fold_status(status, &mut server_down, &mut error);

    let (panes, status) = parse_herdr_section(
        &section_lines(output, "===HERDR_PANES==="),
        parse_pane,
        |value| value.get("result").and_then(|r| r.get("panes")),
    );
    detail.panes = panes;
    fold_status(status, &mut server_down, &mut error);

    let (agents, status) = parse_herdr_section(
        &section_lines(output, "===HERDR_AGENTS==="),
        parse_agent,
        |value| value.get("result").and_then(|r| r.get("agents")),
    );
    detail.agents = agents;
    fold_status(status, &mut server_down, &mut error);

    detail.status = resolve_status(server_down, error);
    detail
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixtures below are captured verbatim from herdr 0.9.4-preview.
    const WORKSPACE_ENVELOPE: &str = r#"{"id":"cli:workspace:list","result":{"type":"workspace_list","workspaces":[{"active_tab_id":"wA:tB","agent_status":"unknown","focused":true,"label":"helphub","number":1,"pane_count":7,"tab_count":7,"workspace_id":"wA"},{"active_tab_id":"wD:t3","agent_status":"unknown","focused":false,"label":"dbx","number":2,"pane_count":2,"tab_count":2,"workspace_id":"wD"}]}}"#;
    const PANE_ENVELOPE: &str = r#"{"id":"cli:pane:list","result":{"panes":[{"agent":"omp","agent_status":"working","cwd":"C:\\Users\\me\\project\\helphub","focused":true,"pane_id":"wA:pC","revision":0,"tab_id":"wA:tB","terminal_id":"term_1","terminal_title":"omp","terminal_title_stripped":"omp","workspace_id":"wA"},{"agent_status":"unknown","cwd":"/gone/dir","label":"1324","focused":false,"pane_id":"wA:pB","restore_error":"Saved directory is unavailable. Restore the directory and restart this session.","revision":0,"tab_id":"wA:tA","terminal_id":"term_2","workspace_id":"wA"}],"type":"pane_list"}}"#;
    const AGENT_ENVELOPE: &str = r#"{"id":"cli:agent:list","result":{"agents":[{"label":"claude","pane_id":"wA:pC","status":"working","workspace_id":"wA"}],"type":"agent_list"}}"#;
    const TAB_ENVELOPE: &str = r#"{"id":"cli:tab:list","result":{"tabs":[{"agent_status":"idle","focused":true,"label":"backend","number":11,"pane_count":1,"tab_id":"wA:tB","workspace_id":"wA"},{"agent_status":"unknown","focused":false,"label":"terminal","number":12,"pane_count":1,"tab_id":"wA:tA","workspace_id":"wA"},{"agent_status":"unknown","focused":false,"label":"other","number":3,"pane_count":2,"tab_id":"wD:t3","workspace_id":"wD"}]}}"#;
    const SESSION_BARE: &str = r#"{"sessions":[{"default":true,"name":"default","running":true,"session_dir":"C:\\herdr","socket_path":"C:\\herdr\\herdr.sock"},{"default":false,"name":"work space","running":false}]}"#;
    const SERVER_DOWN_ENVELOPE: &str = r#"{"id":"cli:workspace:list","error":{"code":"server_not_running","message":"no herdr server is running at C:\\herdr\\herdr.sock; run `herdr` to start or attach it"}}"#;

    fn full_snapshot_output() -> String {
        format!("===HERDR===\n===HERDR_SESSIONS===\n{SESSION_BARE}\n===HERDR_END===")
    }

    fn full_detail_output() -> String {
        format!(
            "===HERDR===\n===HERDR_WORKSPACES===\n{WORKSPACE_ENVELOPE}\n===HERDR_TABS===\n{TAB_ENVELOPE}\n===HERDR_PANES===\n{PANE_ENVELOPE}\n===HERDR_AGENTS===\n{AGENT_ENVELOPE}\n===HERDR_END==="
        )
    }

    #[test]
    fn herdr_parser_builds_the_tree_from_real_envelopes() {
        let snapshot = parse_herdr_snapshot(&full_snapshot_output());
        assert_eq!(snapshot.status, HerdrStatus::Available);
        assert_eq!(snapshot.sessions.len(), 2);
        assert_eq!(snapshot.sessions[0].name, "default");
        assert!(snapshot.sessions[0].running);
        assert!(!snapshot.sessions[1].running);

        let detail = parse_herdr_session_detail(&full_detail_output());
        assert_eq!(detail.status, HerdrStatus::Available);
        assert_eq!(detail.workspaces.len(), 2);
        assert_eq!(detail.workspaces[0].label, "helphub");
        assert_eq!(detail.workspaces[0].tab_count, 7);
        assert_eq!(detail.workspaces[0].pane_count, 7);
        assert!(detail.workspaces[0].focused);
        assert_eq!(detail.tabs.len(), 3);
        assert_eq!(detail.tabs_for_workspace("wA").len(), 2);
        assert_eq!(detail.tabs_for_workspace("wD").len(), 1);
        assert_eq!(detail.tabs[0].label.as_deref(), Some("backend"));
        assert_eq!(detail.panes.len(), 2);
        assert_eq!(detail.panes_for_tab("wA:tB").len(), 1);
        assert_eq!(detail.panes_for_tab("wA:tA").len(), 1);
        assert_eq!(detail.panes[0].label, None);
        assert_eq!(detail.panes[0].terminal_title.as_deref(), Some("omp"));
        // `pane rename` writes `label`, which is the name herdr shows.
        assert_eq!(detail.panes[1].label.as_deref(), Some("1324"));
        assert_eq!(detail.panes[1].terminal_title, None);
        assert!(!detail.panes[0].restore_failed);
        assert!(detail.panes[1].restore_failed);
        assert_eq!(detail.agents.len(), 1);
        assert_eq!(
            detail
                .agent_for_pane("wA:pC")
                .and_then(|agent| agent.label.clone())
                .as_deref(),
            Some("claude")
        );
        assert!(detail.orphan_agents().is_empty());
    }

    #[test]
    fn herdr_parser_drops_session_names_attach_cannot_target() {
        let bare = r#"{"sessions":[{"name":" ok ","running":true},{"name":"","running":true},{"name":"line\nbreak","running":true},{"name":"work space","running":true}]}"#;
        let output = format!("===HERDR===\n===HERDR_SESSIONS===\n{bare}\n===HERDR_END===");
        let snapshot = parse_herdr_snapshot(&output);
        let names: Vec<&str> = snapshot
            .sessions
            .iter()
            .map(|session| session.name.as_str())
            .collect();
        assert_eq!(names, vec!["work space"]);
    }

    #[test]
    fn herdr_parser_reports_server_not_running_from_any_section() {
        let output = format!(
            "===HERDR===\n===HERDR_WORKSPACES===\n{SERVER_DOWN_ENVELOPE}\n===HERDR_PANES===\n{SERVER_DOWN_ENVELOPE}\n===HERDR_AGENTS===\n{SERVER_DOWN_ENVELOPE}\n===HERDR_END==="
        );
        let detail = parse_herdr_session_detail(&output);
        assert_eq!(detail.status, HerdrStatus::ServerNotRunning);
        assert!(detail.workspaces.is_empty());
    }

    #[test]
    fn herdr_parser_reports_missing_binary_and_noise_tolerance() {
        let unavailable = "===HERDR===\n__OXIDE_HERDR_UNAVAILABLE__\n===HERDR_END===";
        assert_eq!(
            parse_herdr_snapshot(unavailable).status,
            HerdrStatus::Unavailable
        );
        assert_eq!(
            parse_herdr_session_detail(unavailable).status,
            HerdrStatus::Unavailable
        );

        let noisy = format!(
            "===HERDR===\nPowerShell banner noise\n===HERDR_WORKSPACES===\n{WORKSPACE_ENVELOPE}\n===HERDR_AGENTS===\n===HERDR_END==="
        );
        let detail = parse_herdr_session_detail(&noisy);
        assert_eq!(detail.status, HerdrStatus::Available);
        assert_eq!(detail.workspaces.len(), 2);
        assert!(detail.agents.is_empty());
    }

    #[test]
    fn herdr_parser_ignores_marker_text_inside_server_strings() {
        let envelope = r#"{"result":{"workspaces":[{"workspace_id":"wA","label":"===HERDR_PANES=== __OXIDE_HERDR_UNAVAILABLE__","number":1}]}}"#;
        let output = format!("===HERDR===\n===HERDR_WORKSPACES===\n{envelope}\n===HERDR_END===");
        let detail = parse_herdr_session_detail(&output);
        assert_eq!(detail.status, HerdrStatus::Available);
        assert_eq!(detail.workspaces.len(), 1);
        assert_eq!(
            detail.workspaces[0].label,
            "===HERDR_PANES=== __OXIDE_HERDR_UNAVAILABLE__"
        );
    }

    #[test]
    fn herdr_parser_surfaces_other_error_envelopes_bounded() {
        let envelope = r#"{"id":"cli:workspace:list","error":{"code":"invalid_params","message":"boom\nsecond line\u001besc"}}"#;
        let output = format!("===HERDR===\n===HERDR_WORKSPACES===\n{envelope}\n===HERDR_END===");
        match parse_herdr_session_detail(&output).status {
            // Only the first line of a server-authored message survives.
            HerdrStatus::Error { message } => assert_eq!(message, "boom"),
            other => panic!("expected error status, got {other:?}"),
        }
    }

    #[test]
    fn herdr_parser_treats_null_error_member_as_success() {
        let envelope = r#"{"id":"cli:workspace:list","error":null,"result":{"workspaces":[{"workspace_id":"wA","label":"one","number":1}]}}"#;
        let output = format!("===HERDR===\n===HERDR_WORKSPACES===\n{envelope}\n===HERDR_END===");
        let detail = parse_herdr_session_detail(&output);
        assert_eq!(detail.status, HerdrStatus::Available);
        assert_eq!(detail.workspaces.len(), 1);
    }

    #[test]
    fn herdr_orphan_agents_land_in_a_fallback_group() {
        let output = format!("===HERDR===\n===HERDR_AGENTS===\n{AGENT_ENVELOPE}\n===HERDR_END===");
        let detail = parse_herdr_session_detail(&output);
        assert_eq!(detail.orphan_agents().len(), 1);
    }

    #[test]
    fn herdr_action_outcome_reads_envelopes_not_exit_codes() {
        // Server-side failures still exit zero; the envelope decides.
        assert!(!herdr_action_outcome(SERVER_DOWN_ENVELOPE, Some(0)));
        assert!(!herdr_action_outcome(
            r#"{"id":"cli:pane:close","error":{"code":"invalid_params","message":"nope"}}"#,
            Some(0)
        ));
        assert!(herdr_action_outcome(
            r#"{"id":"cli:pane:close","result":{"type":"ok"}}"#,
            Some(0)
        ));
        assert!(herdr_action_outcome(
            r#"{"id":"cli:pane:close","error":null}"#,
            Some(0)
        ));
        assert!(!herdr_action_outcome("", Some(1)));
        // Stderr noise merged into stdout must not fail a clean exit.
        assert!(herdr_action_outcome("warning: detached terminal", Some(0)));
    }
}
