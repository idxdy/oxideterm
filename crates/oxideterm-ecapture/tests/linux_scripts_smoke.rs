// Copyright (C) 2026 jaime.su
// SPDX-License-Identifier: GPL-3.0-only

//! End-to-end smoke of the generated scripts on a Linux host.
//!
//! The test replaces `ecapture` with a fake binary that emits the real v2 text
//! format, so it exercises command generation, privilege wrapping, POSIX
//! quoting, the detached start, incremental polls, stop, and clear without
//! needing eBPF or a kernel hook.
//!
//! Run from WSL (root or passwordless sudo):
//!
//! ```text
//! cargo test -p oxideterm-ecapture --test linux_scripts_smoke -- --ignored --test-threads=1
//! ```

#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use oxideterm_ecapture::{
    EcaptureLaunch, EcaptureSession, EcaptureTemplate, build_ecapture_clear_command,
    build_ecapture_poll_command, build_ecapture_probe_command, build_ecapture_start_command,
    build_ecapture_stop_command, ecapture_run_id, parse_ecapture_poll,
    parse_ecapture_start_finished, validate_ecapture_launch,
};

fn run(command: &str) -> (String, String, Option<i32>) {
    let output = Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .output()
        .expect("run generated command");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code(),
    )
}

fn marker(text: &str, marker: &str) -> bool {
    text.lines().any(|line| line.trim() == marker)
}

struct FakeBinary {
    root: PathBuf,
    binary: PathBuf,
    argv: PathBuf,
}

impl FakeBinary {
    fn install() -> Self {
        let root = std::env::temp_dir().join(format!("oxide-ecapture-fake-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create fake root");
        let argv = root.join("argv.txt");
        let binary = root.join("ecapture");
        let script = format!(
            concat!(
                "#!/bin/sh\n",
                "umask 022\n",
                "printf '%s\\n' \"$@\" > {argv}\n",
                "trap 'exit 0' INT TERM\n",
                "printf '[2026-01-01 00:00:00.000] PID:1 TID:1 Comm:fake FD:3 ",
                "Tuple:127.0.0.1:443->127.0.0.1:40000 WRITE (18 bytes):\\n",
                "GET /smoke HTTP/1.1\\n'\n",
                "printf '[2026-01-01 00:00:00.100] PID:1 TID:1 Comm:fake FD:3 ",
                "READ (17 bytes):\\nHTTP/1.1 200 OK\\n'\n",
                "while true; do sleep 1; done\n"
            ),
            argv = shell_path(&argv)
        );
        fs::write(&binary, script).expect("write fake binary");
        let mut permissions = fs::metadata(&binary).expect("fake metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&binary, permissions).expect("chmod fake binary");
        Self { root, binary, argv }
    }

    fn argv(&self) -> String {
        fs::read_to_string(&self.argv).unwrap_or_default()
    }
}

impl Drop for FakeBinary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Quotes one path for the fake script; the test owns these paths.
fn shell_path(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\"'\"'"))
}

fn poll_once(run_id: &str, session: &mut EcaptureSession) -> (bool, String) {
    let command = build_ecapture_poll_command("Linux", run_id, session.next_line())
        .expect("poll command")
        .command;
    let (stdout, stderr, code) = run(&command);
    assert_eq!(code, Some(0), "poll failed: {stderr}");
    let poll = parse_ecapture_poll(run_id, &stdout, false);
    session.apply_poll(poll);
    (session.running(), stderr)
}

fn wait_for_events(session: &mut EcaptureSession, run_id: &str, minimum: usize) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let _ = poll_once(run_id, session);
        if session.event_count() >= minimum {
            return;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    panic!(
        "no events after 10s; log: {:?}",
        session.runtime_log().iter().collect::<Vec<_>>()
    );
}

#[test]
#[ignore = "runs generated sudo scripts on a Linux host; use WSL"]
fn generated_scripts_start_poll_stop_and_clear_a_capture() {
    let fake = FakeBinary::install();
    let connection_id = format!("smoke-{}", std::process::id());
    let run_id = ecapture_run_id(&connection_id);
    let spec = validate_ecapture_launch(&EcaptureLaunch {
        template: EcaptureTemplate::Gotls,
        target: "/opt/smoke/app".to_string(),
        pid: String::new(),
    })
    .expect("spec");

    let probe = build_ecapture_probe_command("Linux")
        .expect("probe")
        .command;
    let (probe_stdout, probe_stderr, probe_code) = run(&probe);
    assert_eq!(probe_code, Some(0), "probe failed: {probe_stderr}");
    assert!(probe_stdout.contains("BIN\t"), "{probe_stdout}");

    let start =
        build_ecapture_start_command("Linux", &run_id, &spec, Some(fake.binary.to_str().unwrap()))
            .expect("start")
            .command;
    let (stdout, stderr, code) = run(&start);
    assert!(code == Some(0), "start failed: {code:?} {stderr}");
    assert!(
        parse_ecapture_start_finished(&run_id, &stdout),
        "{stdout}\n{stderr}"
    );

    let mut session = EcaptureSession::new(&run_id);
    wait_for_events(&mut session, &run_id, 1);
    assert_eq!(session.event(1).map(|event| event.pid), Some(1));
    assert_eq!(
        session.event(1).map(|event| event.summary.as_str()),
        Some("GET /smoke HTTP/1.1")
    );
    assert!(session.running());

    let argv = fake.argv();
    assert!(argv.contains("gotls\n"), "{argv}");
    assert!(argv.contains("-m\ntext\n"), "{argv}");
    assert!(argv.contains("--elfpath=/opt/smoke/app"), "{argv}");

    let stop = build_ecapture_stop_command("Linux", &run_id)
        .expect("stop")
        .command;
    let (stdout, stderr, code) = run(&stop);
    assert_eq!(code, Some(0), "stop failed: {stderr}");
    assert!(
        marker(
            &stdout,
            &format!(
                "__OXIDE_{}_STOPPED__",
                run_id.to_ascii_uppercase().replace('-', "_")
            )
        ),
        "{stdout}"
    );

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let (running, _) = poll_once(&run_id, &mut session);
        if !running && !session.has_backlog() {
            break;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    assert!(!session.running(), "capture did not stop");
    assert!(
        session
            .events()
            .iter()
            .any(|event| event.summary == "HTTP/1.1 200 OK"),
        "trailing record was not flushed: {:?}",
        session
            .events()
            .iter()
            .map(|event| &event.summary)
            .collect::<Vec<_>>()
    );

    let clear = build_ecapture_clear_command("Linux", &run_id)
        .expect("clear")
        .command;
    let (stdout, stderr, code) = run(&clear);
    assert_eq!(code, Some(0), "clear failed: {stderr}");
    assert!(
        marker(
            &stdout,
            &format!(
                "__OXIDE_{}_CLEARED__",
                run_id.to_ascii_uppercase().replace('-', "_")
            )
        ),
        "{stdout}"
    );

    // A cleared session reports the absent marker instead of a stale state.
    let command = build_ecapture_poll_command("Linux", &run_id, 1)
        .expect("poll")
        .command;
    let (stdout, _, code) = run(&command);
    assert_eq!(code, Some(0));
    assert!(parse_ecapture_poll(&run_id, &stdout, false).absent);
}

#[test]
#[ignore = "runs generated sudo scripts on a Linux host; use WSL"]
fn hostile_targets_reach_the_capture_as_literal_arguments() {
    let fake = FakeBinary::install();
    let pwned = std::env::temp_dir().join("oxide-ecapture-pwned");
    let _ = fs::remove_file(&pwned);
    let target = format!("/opt/$(touch {})/app", pwned.display());
    let connection_id = format!("smoke-hostile-{}", std::process::id());
    let run_id = ecapture_run_id(&connection_id);
    let spec = validate_ecapture_launch(&EcaptureLaunch {
        template: EcaptureTemplate::Gotls,
        target: target.clone(),
        pid: String::new(),
    })
    .expect("spec");

    let start =
        build_ecapture_start_command("Linux", &run_id, &spec, Some(fake.binary.to_str().unwrap()))
            .expect("start")
            .command;
    let (stdout, stderr, code) = run(&start);
    assert_eq!(code, Some(0), "start failed: {stderr}");
    assert!(
        parse_ecapture_start_finished(&run_id, &stdout),
        "{stdout}\n{stderr}"
    );

    let mut session = EcaptureSession::new(&run_id);
    wait_for_events(&mut session, &run_id, 1);
    std::thread::sleep(Duration::from_millis(200));
    assert!(!pwned.exists(), "command substitution escaped its quoting");
    assert!(
        fake.argv().contains(&format!("--elfpath={target}")),
        "target was not passed literally: {}",
        fake.argv()
    );

    let _ = run(&build_ecapture_stop_command("Linux", &run_id)
        .expect("stop")
        .command);
    let _ = run(&build_ecapture_clear_command("Linux", &run_id)
        .expect("clear")
        .command);
}

#[test]
#[ignore = "requires OXIDETERM_ECAPTURE_BINARY pointing at a real ecapture binary"]
fn real_ecapture_capture_produces_parseable_events() {
    let binary = std::env::var("OXIDETERM_ECAPTURE_BINARY")
        .expect("set OXIDETERM_ECAPTURE_BINARY to a real ecapture binary path");
    let connection_id = format!("smoke-real-{}", std::process::id());
    let run_id = ecapture_run_id(&connection_id);
    let spec = validate_ecapture_launch(&EcaptureLaunch {
        template: EcaptureTemplate::Libssl,
        target: String::new(),
        pid: String::new(),
    })
    .expect("spec");

    let start = build_ecapture_start_command("Linux", &run_id, &spec, Some(&binary))
        .expect("start")
        .command;
    let (stdout, stderr, code) = run(&start);
    assert_eq!(code, Some(0), "start failed: {stderr}");
    assert!(
        parse_ecapture_start_finished(&run_id, &stdout),
        "{stdout}\n{stderr}"
    );

    // eCapture needs a moment to attach its uprobes before traffic is hooked.
    std::thread::sleep(Duration::from_secs(2));
    let _ = Command::new("curl")
        .args(["-sS", "-o", "/dev/null", "https://example.com"])
        .status();

    let mut session = EcaptureSession::new(&run_id);
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline && session.event_count() == 0 {
        let _ = poll_once(&run_id, &mut session);
        std::thread::sleep(Duration::from_millis(250));
    }
    assert!(
        session.event_count() > 0,
        "no captured events; log: {:?}",
        session.runtime_log().iter().collect::<Vec<_>>()
    );
    assert!(
        session
            .events()
            .iter()
            .all(|event| event.pid > 0 && !event.comm.is_empty()),
        "captured events lost their process identity"
    );

    let _ = run(&build_ecapture_stop_command("Linux", &run_id)
        .expect("stop")
        .command);
    let _ = run(&build_ecapture_clear_command("Linux", &run_id)
        .expect("clear")
        .command);
}
