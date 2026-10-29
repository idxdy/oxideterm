// Copyright (C) 2026 OxideTerm contributors.
// SPDX-License-Identifier: GPL-3.0-only

//! Snapshot shapes rendered by the monitor UI and the queries it runs over
//! them. Field names mirror herdr's JSON so the serde layer stays trivial.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HerdrStatus {
    #[default]
    Unknown,
    Available,
    Unavailable,
    /// The herdr CLI exists but its server is not running; launching `herdr`
    /// in a terminal starts it.
    ServerNotRunning,
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HerdrSession {
    pub name: String,
    /// A stopped session still accepts `session attach`, which starts it.
    #[serde(default)]
    pub running: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HerdrWorkspace {
    pub workspace_id: String,
    pub label: String,
    pub number: u32,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub pane_count: u32,
    #[serde(default)]
    pub tab_count: u32,
    #[serde(default)]
    pub agent_status: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HerdrTab {
    pub tab_id: String,
    pub workspace_id: String,
    /// The tab name shown in herdr's tab strip.
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub number: u32,
    #[serde(default)]
    pub pane_count: u32,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub agent_status: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HerdrPane {
    pub pane_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub tab_id: Option<String>,
    /// The pane's own name set through `pane rename`; herdr shows it in the
    /// tab strip, so it is the name users recognize.
    #[serde(default)]
    pub label: Option<String>,
    /// Latest OSC 0/2 terminal title. Dynamic and independent of `label`.
    #[serde(default)]
    pub terminal_title: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub agent_status: Option<String>,
    /// The remote keeps its restore diagnostics server-side; only whether the
    /// pane failed to restore is worth a row badge.
    #[serde(default)]
    pub restore_failed: bool,
}

/// Agent rows are joined onto panes by id; every field except the pane
/// reference is optional because agent detection varies by herdr release and
/// agent kind.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HerdrAgent {
    pub pane_id: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HerdrSnapshot {
    pub status: HerdrStatus,
    pub sessions: Vec<HerdrSession>,
}

/// Everything beneath one session. Fetched per session because every herdr
/// workspace command is scoped to a session socket.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HerdrSessionDetail {
    pub status: HerdrStatus,
    pub workspaces: Vec<HerdrWorkspace>,
    pub tabs: Vec<HerdrTab>,
    pub panes: Vec<HerdrPane>,
    pub agents: Vec<HerdrAgent>,
}

impl HerdrSessionDetail {
    pub fn tabs_for_workspace(&self, workspace_id: &str) -> Vec<&HerdrTab> {
        self.tabs
            .iter()
            .filter(|tab| tab.workspace_id == workspace_id)
            .collect()
    }

    pub fn panes_for_tab(&self, tab_id: &str) -> Vec<&HerdrPane> {
        self.panes
            .iter()
            .filter(|pane| pane.tab_id.as_deref() == Some(tab_id))
            .collect()
    }

    /// Agents that reference a pane which the pane list does not carry; they
    /// render in a fallback group instead of disappearing.
    pub fn orphan_agents(&self) -> Vec<&HerdrAgent> {
        self.agents
            .iter()
            .filter(|agent| !self.panes.iter().any(|pane| pane.pane_id == agent.pane_id))
            .collect()
    }

    pub fn agent_for_pane(&self, pane_id: &str) -> Option<&HerdrAgent> {
        self.agents.iter().find(|agent| agent.pane_id == pane_id)
    }
}

/// Maps a herdr agent status onto the sidebar translation key that names it.
pub fn herdr_status_label_key(status: &str) -> &'static str {
    match status {
        "working" => "sidebar.host_herdr.status.working",
        "blocked" => "sidebar.host_herdr.status.blocked",
        "idle" => "sidebar.host_herdr.status.idle",
        "done" => "sidebar.host_herdr.status.done",
        _ => "sidebar.host_herdr.status.unknown",
    }
}

/// Sidebar search runs case-insensitively over the fields the tree shows, so
/// a row is visible exactly when one of its visible values matches.
fn contains_query(query: &str, value: &str) -> bool {
    value.to_lowercase().contains(query)
}

pub fn herdr_pane_matches_query(pane: &HerdrPane, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    contains_query(&query, &pane.pane_id)
        || pane
            .label
            .as_deref()
            .is_some_and(|label| contains_query(&query, label))
        || pane
            .terminal_title
            .as_deref()
            .is_some_and(|title| contains_query(&query, title))
        || pane
            .cwd
            .as_deref()
            .is_some_and(|cwd| contains_query(&query, cwd))
        || pane
            .agent_status
            .as_deref()
            .is_some_and(|status| contains_query(&query, status))
}

pub fn herdr_agent_matches_query(agent: &HerdrAgent, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    contains_query(&query, &agent.pane_id)
        || agent
            .label
            .as_deref()
            .is_some_and(|label| contains_query(&query, label))
        || agent
            .status
            .as_deref()
            .is_some_and(|status| contains_query(&query, status))
}

pub fn herdr_tab_matches_query(detail: &HerdrSessionDetail, tab: &HerdrTab, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    contains_query(&query, &tab.tab_id)
        || tab
            .label
            .as_deref()
            .is_some_and(|label| contains_query(&query, label))
        || detail
            .panes_for_tab(&tab.tab_id)
            .iter()
            .any(|pane| herdr_pane_matches_query(pane, &query))
}

pub fn herdr_workspace_matches_query(
    detail: &HerdrSessionDetail,
    workspace: &HerdrWorkspace,
    query: &str,
) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    contains_query(&query, &workspace.workspace_id)
        || contains_query(&query, &workspace.label)
        || detail
            .tabs_for_workspace(&workspace.workspace_id)
            .iter()
            .any(|tab| herdr_tab_matches_query(detail, tab, &query))
}

/// A session row stays visible when the session name matches or when a loaded
/// descendant does; an unexpanded session has no descendants to search.
pub fn herdr_session_matches_query(
    session: &HerdrSession,
    detail: Option<&HerdrSessionDetail>,
    query: &str,
) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    contains_query(&query, &session.name)
        || detail.is_some_and(|detail| {
            detail
                .workspaces
                .iter()
                .any(|workspace| herdr_workspace_matches_query(detail, workspace, &query))
                || detail
                    .agents
                    .iter()
                    .any(|agent| herdr_agent_matches_query(agent, &query))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{parse_herdr_session_detail, parse_herdr_snapshot};

    const DETAIL: &str = r#"===HERDR===
===HERDR_WORKSPACES===
{"result":{"workspaces":[{"workspace_id":"wA","label":"helphub","number":1,"focused":true,"pane_count":7,"tab_count":2,"agent_status":"working"},{"workspace_id":"wD","label":"dbx","number":2}]}}
===HERDR_TABS===
{"result":{"tabs":[{"tab_id":"wA:tB","workspace_id":"wA","label":"backend","number":11,"pane_count":1,"agent_status":"idle"},{"tab_id":"wA:tA","workspace_id":"wA","label":"terminal","number":12,"pane_count":1}]}}
===HERDR_PANES===
{"result":{"panes":[{"pane_id":"wA:pC","workspace_id":"wA","tab_id":"wA:tB","terminal_title":"omp","label":"1324","cwd":"/srv/api","agent_status":"working"},{"pane_id":"wA:pB","workspace_id":"wA","tab_id":"wA:tA","cwd":"/srv/web"}]}}
===HERDR_AGENTS===
{"result":{"agents":[{"label":"claude","pane_id":"wA:pC","status":"working","workspace_id":"wA"}]}}
===HERDR_END==="#;

    const SESSIONS: &str = r#"===HERDR===
===HERDR_SESSIONS===
{"sessions":[{"name":"default","running":true},{"name":"scratch","running":false}]}
===HERDR_END==="#;

    fn detail() -> HerdrSessionDetail {
        parse_herdr_session_detail(DETAIL)
    }

    fn visible_names(query: &str) -> Vec<String> {
        let snapshot = parse_herdr_snapshot(SESSIONS);
        let detail = detail();
        // Only the session whose subtree was loaded can match on descendants.
        snapshot
            .sessions
            .iter()
            .filter(|session| {
                let loaded = (session.name == "default").then_some(&detail);
                herdr_session_matches_query(session, loaded, query)
            })
            .map(|session| session.name.clone())
            .collect()
    }

    #[test]
    fn herdr_search_matches_names_labels_paths_and_agents() {
        // Session names match directly, without a loaded subtree.
        assert_eq!(visible_names("scratch"), vec!["scratch"]);
        // Every query below matches no session name, only loaded descendants.
        for query in ["wA", "helphub", "backend", "1324", "omp", "/srv/web"] {
            assert_eq!(
                visible_names(query),
                vec!["default"],
                "{query} must reach the session through its descendants"
            );
        }
        assert!(visible_names("no-such-row").is_empty());
        assert_eq!(visible_names(""), vec!["default", "scratch"]);
    }

    #[test]
    fn herdr_search_filters_each_level_independently() {
        let detail = detail();
        let workspace = &detail.workspaces[0];
        assert!(herdr_workspace_matches_query(&detail, workspace, "helphub"));
        assert!(herdr_workspace_matches_query(&detail, workspace, "1324"));
        assert!(!herdr_workspace_matches_query(
            &detail,
            &detail.workspaces[1],
            "helphub"
        ));

        let tab = &detail.tabs[0];
        assert!(herdr_tab_matches_query(&detail, tab, "backend"));
        assert!(herdr_tab_matches_query(&detail, tab, "1324"));
        // `/srv/web` lives in the sibling tab, not this one.
        assert!(!herdr_tab_matches_query(&detail, tab, "/srv/web"));
        assert!(herdr_tab_matches_query(
            &detail,
            &detail.tabs[1],
            "/srv/web"
        ));

        assert!(herdr_pane_matches_query(&detail.panes[0], "omp"));
        assert!(herdr_pane_matches_query(&detail.panes[0], "WORKING"));
        assert!(!herdr_pane_matches_query(&detail.panes[0], "/srv/web"));
        assert!(herdr_agent_matches_query(&detail.agents[0], "claude"));
        assert!(!herdr_agent_matches_query(&detail.agents[0], "codex"));
    }

    #[test]
    fn herdr_search_without_loaded_descendants_matches_only_the_session() {
        let snapshot = parse_herdr_snapshot(SESSIONS);
        assert_eq!(
            snapshot
                .sessions
                .iter()
                .filter(|session| herdr_session_matches_query(session, None, "scratch"))
                .count(),
            1
        );
        assert_eq!(
            snapshot
                .sessions
                .iter()
                .filter(|session| herdr_session_matches_query(session, None, "helphub"))
                .count(),
            0
        );
    }
}
