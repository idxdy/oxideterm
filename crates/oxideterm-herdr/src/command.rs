// Copyright (C) 2026 jaime.su
// SPDX-License-Identifier: GPL-3.0-only

//! Command builders. Every value that reaches the command line is either a
//! literal or validated and quoted for the shell that will parse it:
//! PowerShell on Windows, POSIX sh elsewhere.

use crate::HERDR_READ_LINES;
use crate::ids::{validated_id, validated_name};
use crate::quote::{posix_shell_command, powershell_quote, shell_quote, windows_command};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HerdrCaptureCommand {
    pub command: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HerdrActionKind {
    NewWorkspace { label: String, cwd: String },
    CloseWorkspace { workspace_id: String },
    RenameWorkspace { workspace_id: String, label: String },
    StopSession { name: String },
    DeleteSession { name: String },
    ClosePane { pane_id: String },
    RenamePane { pane_id: String, label: String },
    RenameTab { tab_id: String, label: String },
    CloseTab { tab_id: String },
    RunPane { pane_id: String, command: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HerdrActionCommand {
    pub command: String,
}

/// Where an interactive herdr attach lands; all of these go to a terminal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HerdrAttachTarget {
    /// Launch or attach the default persistent session.
    Default,
    Session {
        name: String,
    },
    /// `--session <name>` uses the named session or creates it.
    NewSession {
        name: String,
    },
    Agent {
        session: String,
        pane_id: String,
    },
}

fn is_windows_os(os_type: &str) -> bool {
    matches!(os_type, "Windows" | "windows")
}

/// The JSON envelope is the only success/failure signal, so stderr is
/// discarded and each list prints its own markers around one JSON document.
const HERDR_SNAPSHOT_UNIX: &str = concat!(
    "echo '===HERDR==='; ",
    "if command -v herdr >/dev/null 2>&1; then ",
    "echo '===HERDR_SESSIONS==='; herdr session list --json 2>/dev/null; ",
    "else echo '__OXIDE_HERDR_UNAVAILABLE__'; fi; ",
    "echo '===HERDR_END==='"
);

fn build_windows_herdr_snapshot_command() -> String {
    let script = concat!(
        "Write-Output '===HERDR===';",
        "if(Get-Command herdr -ErrorAction SilentlyContinue){",
        "Write-Output '===HERDR_SESSIONS===';",
        "herdr session list --json 2>$null;",
        "}else{Write-Output '__OXIDE_HERDR_UNAVAILABLE__'};",
        "Write-Output '===HERDR_END===';"
    );
    windows_command(&script)
}

pub fn build_herdr_snapshot_command(os_type: &str) -> HerdrCaptureCommand {
    if is_windows_os(os_type) {
        HerdrCaptureCommand {
            command: build_windows_herdr_snapshot_command(),
        }
    } else {
        HerdrCaptureCommand {
            command: posix_shell_command(HERDR_SNAPSHOT_UNIX),
        }
    }
}

/// Workspaces, panes and agents of one session. Every herdr workspace command
/// talks to that session's own socket, so the session is part of the command.
pub fn build_herdr_session_detail_command(
    os_type: &str,
    session: &str,
) -> Result<HerdrCaptureCommand, String> {
    let session = validated_name(session)?;
    if is_windows_os(os_type) {
        let session = powershell_quote(&session);
        let script = format!(
            concat!(
                "Write-Output '===HERDR===';",
                "if(Get-Command herdr -ErrorAction SilentlyContinue){{",
                "Write-Output '===HERDR_WORKSPACES===';",
                "herdr --session {session} workspace list 2>$null;",
                "Write-Output '===HERDR_TABS===';",
                "herdr --session {session} tab list 2>$null;",
                "Write-Output '===HERDR_PANES===';",
                "herdr --session {session} pane list 2>$null;",
                "Write-Output '===HERDR_AGENTS===';",
                "herdr --session {session} agent list 2>$null;",
                "}}else{{Write-Output '__OXIDE_HERDR_UNAVAILABLE__'}};",
                "Write-Output '===HERDR_END===';"
            ),
            session = session
        );
        Ok(HerdrCaptureCommand {
            command: windows_command(&script),
        })
    } else {
        let session = shell_quote(&session);
        let script = format!(
            concat!(
                "echo '===HERDR==='; ",
                "if command -v herdr >/dev/null 2>&1; then ",
                "echo '===HERDR_WORKSPACES==='; herdr --session {session} workspace list 2>/dev/null; ",
                "echo '===HERDR_TABS==='; herdr --session {session} tab list 2>/dev/null; ",
                "echo '===HERDR_PANES==='; herdr --session {session} pane list 2>/dev/null; ",
                "echo '===HERDR_AGENTS==='; herdr --session {session} agent list 2>/dev/null; ",
                "else echo '__OXIDE_HERDR_UNAVAILABLE__'; fi; ",
                "echo '===HERDR_END==='"
            ),
            session = session
        );
        Ok(HerdrCaptureCommand {
            command: posix_shell_command(&script),
        })
    }
}

pub fn build_herdr_read_command(
    os_type: &str,
    session: &str,
    pane_id: &str,
) -> Result<HerdrCaptureCommand, String> {
    let windows = is_windows_os(os_type);
    let quote = if windows {
        powershell_quote
    } else {
        shell_quote
    };
    let pane_id = validated_id(pane_id)?;
    let session = validated_name(session)?;
    // Pane ids are per session, so the read must address that session's socket.
    let read = format!(
        "herdr --session {} pane read {pane_id} --lines {HERDR_READ_LINES} --source recent --format text 2>&1",
        quote(&session)
    );
    let command = if windows {
        windows_command(&read)
    } else {
        read
    };
    Ok(HerdrCaptureCommand { command })
}

pub fn build_herdr_attach_command(
    os_type: &str,
    target: &HerdrAttachTarget,
) -> Result<String, String> {
    // The attach line is typed into the target terminal, so the session name
    // is remote-authored and must be quoted for that terminal's shell.
    let windows = is_windows_os(os_type);
    Ok(match target {
        HerdrAttachTarget::Default => "herdr".to_string(),
        HerdrAttachTarget::Session { name } => {
            let name = validated_name(name)?;
            let name = if windows {
                powershell_quote(&name)
            } else {
                shell_quote(&name)
            };
            format!("herdr session attach {name}")
        }
        HerdrAttachTarget::NewSession { name } => {
            let name = validated_name(name)?;
            let name = if windows {
                powershell_quote(&name)
            } else {
                shell_quote(&name)
            };
            format!("herdr --session {name}")
        }
        HerdrAttachTarget::Agent { session, pane_id } => {
            let pane_id = validated_id(pane_id)?;
            let session = validated_name(session)?;
            let session = if windows {
                powershell_quote(&session)
            } else {
                shell_quote(&session)
            };
            format!("herdr --session {session} agent attach {pane_id}")
        }
    })
}

/// Workspace, tab and pane mutations are addressed over one session's own
/// socket, so `session` is quoted into the global `--session` option. Session
/// level actions (`stop`, `delete`) name their session directly instead.
pub fn build_herdr_action_command(
    os_type: &str,
    session: &str,
    action: &HerdrActionKind,
) -> Result<HerdrActionCommand, String> {
    // Herdr takes one argv per value, so each value is quoted for the shell
    // that parses the command line: PowerShell on Windows, POSIX sh elsewhere.
    let windows = is_windows_os(os_type);
    let quote = if windows {
        powershell_quote
    } else {
        shell_quote
    };
    let session_prefix = || -> Result<String, String> {
        let session = validated_name(session)?;
        Ok(format!("herdr --session {}", quote(&session)))
    };
    let payload = match action {
        HerdrActionKind::NewWorkspace { label, cwd } => {
            let prefix = session_prefix()?;
            let label = validated_name(label)?;
            let cwd = cwd.trim();
            if cwd.is_empty() {
                format!("{prefix} workspace create --label {}", quote(&label))
            } else {
                format!(
                    "{prefix} workspace create --label {} --cwd {}",
                    quote(&label),
                    quote(cwd)
                )
            }
        }
        HerdrActionKind::CloseWorkspace { workspace_id } => {
            let prefix = session_prefix()?;
            let id = validated_id(workspace_id)?;
            format!("{prefix} workspace close {id}")
        }
        HerdrActionKind::RenameWorkspace {
            workspace_id,
            label,
        } => {
            let prefix = session_prefix()?;
            let id = validated_id(workspace_id)?;
            let label = validated_name(label)?;
            format!("{prefix} workspace rename {id} {}", quote(&label))
        }
        HerdrActionKind::StopSession { name } => {
            // The session name is remote-authored; quote it for the shell that
            // parses this command line.
            let name = validated_name(name)?;
            let name = if windows {
                powershell_quote(&name)
            } else {
                shell_quote(&name)
            };
            format!("herdr session stop {name}")
        }
        HerdrActionKind::DeleteSession { name } => {
            let name = validated_name(name)?;
            let name = if windows {
                powershell_quote(&name)
            } else {
                shell_quote(&name)
            };
            format!("herdr session delete {name}")
        }
        HerdrActionKind::ClosePane { pane_id } => {
            let prefix = session_prefix()?;
            let id = validated_id(pane_id)?;
            format!("{prefix} pane close {id}")
        }
        HerdrActionKind::RenamePane { pane_id, label } => {
            let prefix = session_prefix()?;
            let id = validated_id(pane_id)?;
            let label = validated_name(label)?;
            format!("{prefix} pane rename {id} {}", quote(&label))
        }
        HerdrActionKind::RenameTab { tab_id, label } => {
            let prefix = session_prefix()?;
            let id = validated_id(tab_id)?;
            let label = validated_name(label)?;
            format!("{prefix} tab rename {id} {}", quote(&label))
        }
        HerdrActionKind::CloseTab { tab_id } => {
            let prefix = session_prefix()?;
            let id = validated_id(tab_id)?;
            format!("{prefix} tab close {id}")
        }
        HerdrActionKind::RunPane { pane_id, command } => {
            let prefix = session_prefix()?;
            let id = validated_id(pane_id)?;
            let command = command.trim();
            if command.is_empty() {
                return Err("A command is required.".to_string());
            }
            // herdr joins the remaining argv into the command it types into
            // the pane, and it does not consume a `--` separator: passing one
            // makes the pane run `-- <command>`. The quoted command is the
            // only argument after the pane id.
            format!("{prefix} pane run {id} {}", quote(command))
        }
    };
    let command = if windows {
        windows_command(&payload)
    } else {
        format!("{payload} 2>&1")
    };
    Ok(HerdrActionCommand { command })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(session: &str, kind: HerdrActionKind) -> String {
        build_herdr_action_command("Linux", session, &kind)
            .unwrap()
            .command
    }

    /// Windows lines carry the script as base64 UTF-16LE, so assertions read
    /// the decoded script instead of the transport encoding.
    fn windows_script(command: &str) -> String {
        use base64::Engine as _;
        let encoded = command
            .trim_end()
            .rsplit(' ')
            .next()
            .expect("encoded payload");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("base64 payload");
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16(&units).expect("utf-16 payload")
    }

    #[test]
    fn herdr_run_pane_passes_the_command_without_a_separator() {
        // herdr treats `--` as part of the typed command, so a separator
        // would make the pane run `-- ls`.
        let run = action(
            "default",
            HerdrActionKind::RunPane {
                pane_id: "wA:pC".to_string(),
                command: "ls".to_string(),
            },
        );
        assert_eq!(run, "herdr --session 'default' pane run wA:pC 'ls' 2>&1");

        let shell_metacharacters = action(
            "default",
            HerdrActionKind::RunPane {
                pane_id: "wA:pC".to_string(),
                command: "echo hi; rm -rf /".to_string(),
            },
        );
        assert_eq!(
            shell_metacharacters,
            "herdr --session 'default' pane run wA:pC 'echo hi; rm -rf /' 2>&1"
        );
    }

    #[test]
    fn herdr_command_builders_validate_ids_and_quote_inputs() {
        let rename = action(
            "default",
            HerdrActionKind::RenamePane {
                pane_id: "wA:pC".to_string(),
                label: "it's mine".to_string(),
            },
        );
        assert_eq!(
            rename,
            "herdr --session 'default' pane rename wA:pC 'it'\"'\"'s mine' 2>&1"
        );

        assert!(
            build_herdr_action_command(
                "Linux",
                "default",
                &HerdrActionKind::ClosePane {
                    pane_id: "wA; rm".to_string()
                }
            )
            .is_err()
        );
        assert!(
            build_herdr_action_command(
                "Linux",
                "default",
                &HerdrActionKind::RunPane {
                    pane_id: "wA:pC".to_string(),
                    command: "   ".to_string()
                }
            )
            .is_err()
        );
        assert!(
            build_herdr_action_command(
                "Linux",
                "default",
                &HerdrActionKind::RenamePane {
                    pane_id: "wA:pC".to_string(),
                    label: "line\nbreak".to_string()
                }
            )
            .is_err()
        );

        let attach = build_herdr_attach_command(
            "Linux",
            &HerdrAttachTarget::Agent {
                session: "default".to_string(),
                pane_id: "wA:pC".to_string(),
            },
        )
        .unwrap();
        assert_eq!(attach, "herdr --session 'default' agent attach wA:pC");
        assert!(
            build_herdr_attach_command(
                "Linux",
                &HerdrAttachTarget::Agent {
                    session: "default".to_string(),
                    pane_id: "wA; rm".to_string()
                }
            )
            .is_err()
        );

        // Session names are remote-authored and typed into a terminal.
        let session = build_herdr_attach_command(
            "Linux",
            &HerdrAttachTarget::Session {
                name: "work space".to_string(),
            },
        )
        .unwrap();
        assert_eq!(session, "herdr session attach 'work space'");
        let windows_session = build_herdr_attach_command(
            "Windows",
            &HerdrAttachTarget::Session {
                name: "it's work".to_string(),
            },
        )
        .unwrap();
        assert_eq!(windows_session, "herdr session attach 'it''s work'");
        let created = build_herdr_attach_command(
            "Linux",
            &HerdrAttachTarget::NewSession {
                name: "my work".to_string(),
            },
        )
        .unwrap();
        assert_eq!(created, "herdr --session 'my work'");
        assert!(
            build_herdr_attach_command(
                "Linux",
                &HerdrAttachTarget::Session {
                    name: "line\nbreak".to_string()
                }
            )
            .is_err()
        );

        let stop = action(
            "default",
            HerdrActionKind::StopSession {
                name: "work space".to_string(),
            },
        );
        assert_eq!(stop, "herdr session stop 'work space' 2>&1");
        let delete = action(
            "default",
            HerdrActionKind::DeleteSession {
                name: "work space".to_string(),
            },
        );
        assert_eq!(delete, "herdr session delete 'work space' 2>&1");

        let tab_rename = action(
            "default",
            HerdrActionKind::RenameTab {
                tab_id: "wA:tB".to_string(),
                label: "it's mine".to_string(),
            },
        );
        assert_eq!(
            tab_rename,
            "herdr --session 'default' tab rename wA:tB 'it'\"'\"'s mine' 2>&1"
        );
        let tab_close = action(
            "default",
            HerdrActionKind::CloseTab {
                tab_id: "wA:tB".to_string(),
            },
        );
        assert_eq!(tab_close, "herdr --session 'default' tab close wA:tB 2>&1");
        assert!(
            build_herdr_action_command(
                "Linux",
                "default",
                &HerdrActionKind::CloseTab {
                    tab_id: "wA; rm".to_string()
                }
            )
            .is_err()
        );
        assert!(
            build_herdr_action_command(
                "Linux",
                "default",
                &HerdrActionKind::StopSession {
                    name: "line\nbreak".to_string()
                }
            )
            .is_err()
        );

        let session_scoped = action(
            "work space",
            HerdrActionKind::CloseWorkspace {
                workspace_id: "wA".to_string(),
            },
        );
        assert_eq!(
            session_scoped,
            "herdr --session 'work space' workspace close wA 2>&1"
        );

        let read = build_herdr_read_command("Linux", "default", "wA:pC").unwrap();
        assert!(read.command.contains("--session 'default'"));
        assert!(
            read.command
                .contains(&format!("--lines {HERDR_READ_LINES}"))
        );
        assert!(build_herdr_read_command("Linux", "default", "bad id").is_err());
    }

    #[test]
    fn herdr_windows_actions_quote_with_powershell_rules() {
        let rename = build_herdr_action_command(
            "Windows",
            "default",
            &HerdrActionKind::RenameWorkspace {
                workspace_id: "wA".to_string(),
                label: "it's mine".to_string(),
            },
        )
        .unwrap();
        assert!(rename.command.starts_with("powershell -NoProfile"));
        // `cmd.exe` expands `$` and `powershell.exe` expands the surrounding
        // quotes before PowerShell parses the script, so a Windows line may
        // not carry shell-visible text at all.
        assert!(rename.command.contains("-EncodedCommand "));
        assert!(!rename.command.contains('$'));
        assert!(!rename.command.contains('"'));
        let script = windows_script(&rename.command);
        assert!(script.contains("--session 'default'"));
        assert!(script.contains("'it''s mine'"));
        // POSIX escaping must never reach the PowerShell script.
        assert!(!script.contains("'\"'\"'"));

        let stop = build_herdr_action_command(
            "Windows",
            "default",
            &HerdrActionKind::StopSession {
                name: "it's work".to_string(),
            },
        )
        .unwrap();
        assert!(windows_script(&stop.command).contains("herdr session stop 'it''s work'"));
        assert!(!stop.command.contains("'\"'\"'"));
    }

    /// The script a Windows line actually runs, or the line itself on Unix.
    fn script_for(os_type: &str, command: &str) -> String {
        if os_type == "Windows" {
            windows_script(command)
        } else {
            command.to_string()
        }
    }

    #[test]
    fn herdr_snapshot_command_lists_sessions_only() {
        for os_type in ["Linux", "Windows"] {
            let line = build_herdr_snapshot_command(os_type).command;
            let command = script_for(os_type, &line);
            assert!(command.contains("===HERDR_SESSIONS==="), "{os_type}");
            assert!(command.contains("session list --json"), "{os_type}");
            // Workspaces and panes need a session socket, so they are fetched
            // per session instead of in the snapshot.
            assert!(!command.contains("workspace list"), "{os_type}");
            assert!(!command.contains("pane list"), "{os_type}");
        }
    }

    #[test]
    fn herdr_session_detail_command_targets_one_session() {
        for os_type in ["Linux", "Windows"] {
            let line = build_herdr_session_detail_command(os_type, "work")
                .unwrap()
                .command;
            let command = script_for(os_type, &line);
            for marker in [
                "===HERDR_WORKSPACES===",
                "===HERDR_TABS===",
                "===HERDR_PANES===",
                "===HERDR_AGENTS===",
            ] {
                assert!(command.contains(marker), "{os_type} missing {marker}");
            }
            // Every list must address the same session socket.
            assert_eq!(command.matches("--session").count(), 4, "{os_type}");
            for list in ["workspace list", "tab list", "pane list", "agent list"] {
                assert!(command.contains(list), "{os_type} missing {list}");
            }
        }
        assert!(build_herdr_session_detail_command("Linux", "line\nbreak").is_err());
    }
}
