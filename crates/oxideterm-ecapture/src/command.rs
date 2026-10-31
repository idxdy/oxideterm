// Copyright (C) 2026 jaime.su
// SPDX-License-Identifier: GPL-3.0-only

//! Remote command construction for one eCapture session.
//!
//! Every script is framed with markers derived from the validated run id so the
//! parsers can separate metadata, event text, and runtime logs without guessing.
//! eCapture itself is started detached and its streams are redirected to files
//! inside the run directory; polls read only new line ranges from that file.

use sha2::{Digest, Sha256};

use crate::shell::{posix_shell_command, shell_quote};
use crate::template::EcaptureSpec;

/// Lines of captured event text fetched per poll. Kept low so one poll stays
/// comfortably below the SSH capture budget even with long TLS payloads.
pub const ECAPTURE_POLL_LINES: u64 = 128;
/// Runtime log lines fetched per poll.
pub const ECAPTURE_POLL_LOG_LINES: usize = 30;

const RUN_ID_PREFIX: &str = "ecapture-";
const RUN_ID_HEX_LEN: usize = 24;
const RUN_DIRECTORY: &str = "/tmp";
const RUN_ENV: &str = "OXIDE_ECAPTURE_RUN";
const STARTED_MARKER_SUFFIX: &str = "STARTED";
const STOPPED_MARKER_SUFFIX: &str = "STOPPED";
const CLEARED_MARKER_SUFFIX: &str = "CLEARED";
const ABSENT_MARKER_SUFFIX: &str = "ABSENT";
const META_MARKER_SUFFIX: &str = "META";
const EVENTS_MARKER_SUFFIX: &str = "EVENTS";
const LOG_MARKER_SUFFIX: &str = "LOG";
const END_MARKER_SUFFIX: &str = "END";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EcaptureCommand {
    pub command: String,
}

/// Stable run id for one connection.
///
/// The id survives an app restart, so the panel can reattach to a capture it
/// started earlier and can always find (or clean) the same `/tmp` directory
/// instead of leaking one directory per launch.
pub fn ecapture_run_id(connection_id: &str) -> String {
    let digest = Sha256::digest(connection_id.as_bytes());
    let mut run_id = String::with_capacity(RUN_ID_PREFIX.len() + RUN_ID_HEX_LEN);
    run_id.push_str(RUN_ID_PREFIX);
    for byte in &digest[..RUN_ID_HEX_LEN / 2] {
        run_id.push_str(&format!("{byte:02x}"));
    }
    run_id
}

pub(crate) fn marker(run_id: &str, suffix: &str) -> String {
    format!(
        "__OXIDE_{}_{}__",
        run_id.to_ascii_uppercase().replace('-', "_"),
        suffix
    )
}

pub(crate) fn started_marker(run_id: &str) -> String {
    marker(run_id, STARTED_MARKER_SUFFIX)
}

pub(crate) fn stopped_marker(run_id: &str) -> String {
    marker(run_id, STOPPED_MARKER_SUFFIX)
}

pub(crate) fn cleared_marker(run_id: &str) -> String {
    marker(run_id, CLEARED_MARKER_SUFFIX)
}

pub(crate) fn absent_marker(run_id: &str) -> String {
    marker(run_id, ABSENT_MARKER_SUFFIX)
}

pub(crate) fn meta_marker(run_id: &str) -> String {
    marker(run_id, META_MARKER_SUFFIX)
}

pub(crate) fn events_marker(run_id: &str) -> String {
    marker(run_id, EVENTS_MARKER_SUFFIX)
}

pub(crate) fn log_marker(run_id: &str) -> String {
    marker(run_id, LOG_MARKER_SUFFIX)
}

pub(crate) fn end_marker(run_id: &str) -> String {
    marker(run_id, END_MARKER_SUFFIX)
}

/// eCapture runs only on Linux; the probe and both actions reject other OS
/// families before any script reaches the host.
fn require_linux(os_type: &str) -> Result<(), String> {
    if os_type.trim().eq_ignore_ascii_case("linux") {
        Ok(())
    } else {
        Err(format!(
            "eCapture requires a Linux host (remote OS: {os_type})."
        ))
    }
}

fn run_directory(run_id: &str) -> Result<String, String> {
    if !valid_run_id(run_id) {
        return Err(format!("invalid eCapture run id: {run_id}"));
    }
    Ok(format!("{RUN_DIRECTORY}/{run_id}"))
}

fn valid_run_id(run_id: &str) -> bool {
    run_id.strip_prefix(RUN_ID_PREFIX).is_some_and(|suffix| {
        suffix.len() == RUN_ID_HEX_LEN
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

/// Runs the inner script as root without letting a privilege prompt stall the
/// SSH exec channel. The login shell only sees `/bin/sh -c`, so a Fish or
/// Nushell account still executes the POSIX script.
fn privileged_script(script: &str) -> String {
    let quoted = shell_quote(script);
    let wrapper = format!(
        "if [ \"$(id -u)\" -eq 0 ]; then /bin/sh -c {quoted}; else sudo -n /bin/sh -c {quoted}; fi"
    );
    posix_shell_command(&wrapper)
}

pub fn build_ecapture_probe_command(os_type: &str) -> Result<EcaptureCommand, String> {
    require_linux(os_type)?;
    let script = concat!(
        "uid=$(id -u 2>/dev/null || printf 'unknown')\n",
        "bin=$(command -v ecapture 2>/dev/null || true)\n",
        "version=''\n",
        "if [ -n \"$bin\" ]; then version=$(\"$bin\" --version 2>/dev/null | head -n 1); fi\n",
        "priv=0\n",
        "if [ \"$uid\" = '0' ]; then priv=1; ",
        "elif command -v sudo >/dev/null 2>&1 && sudo -n true >/dev/null 2>&1; then priv=1; fi\n",
        "btf=0\n",
        "if [ -e /sys/kernel/btf/vmlinux ]; then btf=1; fi\n",
        "kernel=$(uname -r 2>/dev/null || true)\n",
        "printf 'UID\\t%s\\nBIN\\t%s\\nVERSION\\t%s\\nPRIV\\t%s\\nBTF\\t%s\\nKERNEL\\t%s\\n' ",
        "\"$uid\" \"$bin\" \"$version\" \"$priv\" \"$btf\" \"$kernel\"\n"
    );
    Ok(EcaptureCommand {
        command: posix_shell_command(script),
    })
}

pub fn build_ecapture_start_command(
    os_type: &str,
    run_id: &str,
    spec: &EcaptureSpec,
    binary: Option<&str>,
) -> Result<EcaptureCommand, String> {
    require_linux(os_type)?;
    let directory = run_directory(run_id)?;
    let binary = binary
        .map(str::trim)
        .filter(|value| {
            !value.is_empty() && value.len() <= 1024 && !value.chars().any(char::is_control)
        })
        .unwrap_or("");
    let script = start_script(run_id, &directory, spec, binary);
    Ok(EcaptureCommand {
        command: privileged_script(&script),
    })
}

fn start_script(run_id: &str, directory: &str, spec: &EcaptureSpec, probed_binary: &str) -> String {
    let (prelude, argv) = ecapture_argv(spec);
    let probed_binary = if probed_binary.is_empty() {
        String::new()
    } else {
        shell_quote(probed_binary)
    };
    format!(
        concat!(
            "bin={probed_binary}\n",
            "if [ -z \"$bin\" ] || [ ! -x \"$bin\" ]; then bin=$(command -v ecapture 2>/dev/null || true); fi\n",
            "if [ -z \"$bin\" ]; then for candidate in /usr/local/bin/ecapture /usr/bin/ecapture ",
            "/opt/ecapture/ecapture; do if [ -x \"$candidate\" ]; then bin=$candidate; break; fi; done; fi\n",
            "if [ -z \"$bin\" ]; then printf 'eCapture binary not found in PATH.\\n' >&2; exit 127; fi\n",
            "if [ ! -x \"$bin\" ]; then printf 'eCapture binary is not executable: %s\\n' \"$bin\" >&2; exit 127; fi\n",
            "dir={directory}\n",
            "umask 077\n",
            // A capture can outlive the app that started it; replace any live
            // process for this run id before wiping its stream files.
            "old=$(cat \"$dir/pid\" 2>/dev/null || true)\n",
            "if [ -n \"$old\" ] && [ -d \"/proc/$old\" ] && ",
            "grep -qa '{run_env}={run_id}' \"/proc/$old/environ\" 2>/dev/null; then\n",
            "kill -INT \"$old\" 2>/dev/null || true\n",
            "i=0; while [ \"$i\" -lt 3 ] && [ -d \"/proc/$old\" ]; do sleep 1; i=$((i+1)); done\n",
            "if [ -d \"/proc/$old\" ]; then kill -TERM \"$old\" 2>/dev/null || true; fi\n",
            "fi\n",
            "rm -rf \"$dir\"\n",
            "mkdir -p \"$dir\" || exit 4\n",
            "cd \"$dir\" || exit 4\n",
            "{prelude}",
            "if command -v setsid >/dev/null 2>&1; then launcher=setsid; else launcher=nohup; fi\n",
            "{run_env}={run_id} $launcher sh -c 'echo $$ > \"$1\"; shift; exec \"$@\"' ",
            "sh \"$dir/pid\" \"$bin\" {argv} >events.log 2>runtime.log < /dev/null &\n",
            "printf '{started}\\n'\n"
        ),
        probed_binary = probed_binary,
        directory = shell_quote(directory),
        prelude = prelude,
        run_env = RUN_ENV,
        run_id = run_id,
        argv = argv,
        started = started_marker(run_id),
    )
}

/// Returns the root-side prelude (container/cgroup resolution) and the eCapture
/// argument list. Values are already shell-quoted or reference shell variables.
fn ecapture_argv(spec: &EcaptureSpec) -> (String, String) {
    let pid = spec.pid.map(|pid| format!(" -p {pid}")).unwrap_or_default();
    match spec.template {
        crate::template::EcaptureTemplate::Libssl => {
            let libssl = if spec.target.is_empty() {
                String::new()
            } else {
                format!(" --libssl={}", shell_quote(&spec.target))
            };
            (String::new(), format!("tls -m text{libssl}{pid}"))
        }
        crate::template::EcaptureTemplate::Node => {
            let prelude = String::new();
            let libssl = format!(" --libssl={}", shell_quote(&spec.target));
            (prelude, format!("tls -m text{libssl}{pid}"))
        }
        crate::template::EcaptureTemplate::Gotls => {
            let elf = format!(" --elfpath={}", shell_quote(&spec.target));
            (String::new(), format!("gotls -m text{elf}{pid}"))
        }
        crate::template::EcaptureTemplate::Docker => {
            let container = shell_quote(&spec.target);
            let prelude = format!(
                concat!(
                    "cpid=$(docker inspect --format '{{{{.State.Pid}}}}' {container} 2>/dev/null || true)\n",
                    "if [ -z \"$cpid\" ] || [ \"$cpid\" = '0' ]; then ",
                    "printf 'eCapture: container %s is not running.\\n' {container} >&2; exit 5; fi\n",
                    "cgroup=$(awk -F: '$1 == \"0\" {{ print $3; exit }}' \"/proc/$cpid/cgroup\" 2>/dev/null || true)\n",
                    "if [ -z \"$cgroup\" ]; then ",
                    "printf 'eCapture: no cgroup v2 path for container %s.\\n' {container} >&2; exit 5; fi\n",
                    "cgroup_path=\"/sys/fs/cgroup$cgroup\"\n",
                    "if [ ! -d \"$cgroup_path\" ]; then ",
                    "printf 'eCapture: cgroup path %s does not exist.\\n' \"$cgroup_path\" >&2; exit 5; fi\n"
                ),
                container = container
            );
            (
                prelude,
                format!("tls -m text --cgroup_path=\"$cgroup_path\"{pid}"),
            )
        }
    }
}

/// Reads only newline-terminated lines. `wc -l` counts the complete lines in
/// the file, so the range never includes a record that eCapture is still
/// writing; the app advances its cursor by the number of lines it received.
pub fn build_ecapture_poll_command(
    os_type: &str,
    run_id: &str,
    start_line: u64,
) -> Result<EcaptureCommand, String> {
    require_linux(os_type)?;
    let directory = run_directory(run_id)?;
    let start_line = start_line.max(1);
    let script = format!(
        concat!(
            "dir={directory}\n",
            "if [ ! -d \"$dir\" ]; then printf '{absent}\\n'; exit 0; fi\n",
            "pid=$(cat \"$dir/pid\" 2>/dev/null || true)\n",
            "running=0\n",
            "if [ -n \"$pid\" ] && [ -d \"/proc/$pid\" ] && ",
            "grep -qa '{run_env}={run_id}' \"/proc/$pid/environ\" 2>/dev/null; then running=1; fi\n",
            "total=$(wc -l < \"$dir/events.log\" 2>/dev/null || printf '0')\n",
            "bytes=$(wc -c < \"$dir/events.log\" 2>/dev/null || printf '0')\n",
            "total=${{total:-0}}\n",
            "bytes=${{bytes:-0}}\n",
            "start={start}\n",
            "end=$((start + {lines} - 1))\n",
            "if [ \"$end\" -gt \"$total\" ]; then end=$total; fi\n",
            "printf '{meta}\\t%s\\t%s\\t%s\\t%s\\t%s\\n' \"$running\" \"$total\" \"$bytes\" \"$start\" \"$end\"\n",
            "printf '{events}\\n'\n",
            "if [ \"$start\" -le \"$end\" ]; then sed -n \"${{start}},${{end}}p\" \"$dir/events.log\" 2>/dev/null; fi\n",
            "printf '{log}\\n'\n",
            "tail -n {log_lines} \"$dir/runtime.log\" 2>/dev/null\n",
            "printf '{end_marker}\\n'\n"
        ),
        directory = shell_quote(&directory),
        absent = absent_marker(run_id),
        run_env = RUN_ENV,
        run_id = run_id,
        start = start_line,
        lines = ECAPTURE_POLL_LINES,
        meta = meta_marker(run_id),
        events = events_marker(run_id),
        log = log_marker(run_id),
        log_lines = ECAPTURE_POLL_LOG_LINES,
        end_marker = end_marker(run_id),
    );
    Ok(EcaptureCommand {
        command: privileged_script(&script),
    })
}

pub fn build_ecapture_stop_command(os_type: &str, run_id: &str) -> Result<EcaptureCommand, String> {
    require_linux(os_type)?;
    let directory = run_directory(run_id)?;
    let script = format!(
        concat!(
            "dir={directory}\n",
            "pid=$(cat \"$dir/pid\" 2>/dev/null || true)\n",
            "if [ -z \"$pid\" ] || [ ! -d \"/proc/$pid\" ]; then printf '{stopped}\\n'; exit 0; fi\n",
            "if ! grep -qa '{run_env}={run_id}' \"/proc/$pid/environ\" 2>/dev/null; then ",
            "printf '{stopped}\\n'; exit 0; fi\n",
            "kill -INT \"$pid\" 2>/dev/null || true\n",
            "i=0\n",
            "while [ \"$i\" -lt 5 ] && [ -d \"/proc/$pid\" ]; do sleep 1; i=$((i + 1)); done\n",
            "if [ -d \"/proc/$pid\" ]; then kill -TERM \"$pid\" 2>/dev/null || true; fi\n",
            "printf '{stopped}\\n'\n"
        ),
        directory = shell_quote(&directory),
        stopped = stopped_marker(run_id),
        run_env = RUN_ENV,
        run_id = run_id,
    );
    Ok(EcaptureCommand {
        command: privileged_script(&script),
    })
}

pub fn build_ecapture_clear_command(
    os_type: &str,
    run_id: &str,
) -> Result<EcaptureCommand, String> {
    require_linux(os_type)?;
    let directory = run_directory(run_id)?;
    let script = format!(
        concat!(
            "dir={directory}\n",
            "pid=$(cat \"$dir/pid\" 2>/dev/null || true)\n",
            "if [ -n \"$pid\" ] && [ -d \"/proc/$pid\" ] && ",
            "grep -qa '{run_env}={run_id}' \"/proc/$pid/environ\" 2>/dev/null; then\n",
            "printf 'eCapture is still running.\n' >&2; exit 6\n",
            "fi\n",
            "if [ -d \"$dir\" ]; then rm -rf \"$dir\" || exit 4; fi\n",
            "printf '{cleared}\\n'\n"
        ),
        directory = shell_quote(&directory),
        run_env = RUN_ENV,
        run_id = run_id,
        cleared = cleared_marker(run_id),
    );
    Ok(EcaptureCommand {
        command: privileged_script(&script),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::{EcaptureLaunch, EcaptureTemplate, validate_ecapture_launch};

    fn spec(template: EcaptureTemplate, target: &str, pid: &str) -> EcaptureSpec {
        validate_ecapture_launch(&EcaptureLaunch {
            template,
            target: target.to_string(),
            pid: pid.to_string(),
        })
        .expect("valid launch")
    }

    #[test]
    fn run_id_is_stable_and_safe_for_paths() {
        let first = ecapture_run_id("host-1");
        assert_eq!(first, ecapture_run_id("host-1"));
        assert_ne!(first, ecapture_run_id("host-2"));
        assert!(valid_run_id(&first));
        assert!(build_ecapture_poll_command("Linux", &first, 1).is_ok());
        assert!(build_ecapture_poll_command("Linux", "../escape", 1).is_err());
    }

    #[test]
    fn commands_reject_non_linux_hosts() {
        let spec = spec(EcaptureTemplate::Libssl, "", "");
        for os_type in ["Windows", "macOS", ""] {
            assert!(build_ecapture_probe_command(os_type).is_err(), "{os_type}");
            assert!(
                build_ecapture_start_command(os_type, &ecapture_run_id("c"), &spec, None).is_err(),
                "{os_type}"
            );
            assert!(
                build_ecapture_stop_command(os_type, &ecapture_run_id("c")).is_err(),
                "{os_type}"
            );
            assert!(
                build_ecapture_clear_command(os_type, &ecapture_run_id("c")).is_err(),
                "{os_type}"
            );
        }
    }

    #[test]
    fn start_command_builds_each_template_and_quotes_targets() {
        let run_id = ecapture_run_id("connection-a");

        let libssl = spec(EcaptureTemplate::Libssl, "", "");
        let command = build_ecapture_start_command("Linux", &run_id, &libssl, None)
            .unwrap()
            .command;
        assert!(command.starts_with("/bin/sh -c "), "{command}");
        assert!(command.contains("tls -m text"), "{command}");
        assert!(command.contains("setsid"), "{command}");
        assert!(command.contains("OXIDE_ECAPTURE_RUN="), "{command}");
        assert!(!command.contains("--libssl"), "{command}");

        let node = spec(EcaptureTemplate::Node, "/opt/node's/bin/node", "321");
        let command =
            build_ecapture_start_command("Linux", &run_id, &node, Some("/usr/local/bin/ecapture"))
                .unwrap()
                .command;
        // The target crosses two `/bin/sh -c` layers (login shell and sudo),
        // so quoting is asserted by the Linux smoke test that executes it.
        assert!(command.contains("--libssl="), "{command}");
        assert!(command.contains("/usr/local/bin/ecapture"), "{command}");
        assert!(command.contains("-p 321"), "{command}");

        let gotls = spec(EcaptureTemplate::Gotls, "/srv/$(reboot)/svc", "");
        let command = build_ecapture_start_command("Linux", &run_id, &gotls, None)
            .unwrap()
            .command;
        assert!(command.contains("gotls -m text"), "{command}");
        assert!(command.contains("--elfpath="), "{command}");

        let docker = spec(EcaptureTemplate::Docker, "api-gateway", "");
        let command = build_ecapture_start_command("Linux", &run_id, &docker, None)
            .unwrap()
            .command;
        assert!(command.contains("docker inspect"), "{command}");
        assert!(
            command.contains("--cgroup_path=\"$cgroup_path\""),
            "{command}"
        );
    }

    #[test]
    fn poll_command_reads_only_complete_line_ranges() {
        let run_id = ecapture_run_id("connection-a");
        let command = build_ecapture_poll_command("Linux", &run_id, 41)
            .unwrap()
            .command;
        assert!(command.contains("start=41"), "{command}");
        assert!(
            command.contains(&format!("end=$((start + {ECAPTURE_POLL_LINES} - 1))")),
            "{command}"
        );
        assert!(command.contains("sed -n \"${start},${end}p\""), "{command}");
        assert!(
            command.contains(&format!("tail -n {ECAPTURE_POLL_LOG_LINES}")),
            "{command}"
        );
        assert!(command.contains("sudo -n /bin/sh -c"), "{command}");
    }

    #[test]
    fn stop_and_clear_verify_process_identity_before_acting() {
        let run_id = ecapture_run_id("connection-a");
        let stop = build_ecapture_stop_command("Linux", &run_id)
            .unwrap()
            .command;
        assert!(stop.contains("kill -INT \"$pid\""), "{stop}");
        assert!(stop.contains("kill -TERM \"$pid\""), "{stop}");
        assert!(stop.contains("OXIDE_ECAPTURE_RUN="), "{stop}");

        let clear = build_ecapture_clear_command("Linux", &run_id)
            .unwrap()
            .command;
        assert!(clear.contains("rm -rf \"$dir\""), "{clear}");
    }
}
