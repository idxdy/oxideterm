//! Host Tools execution targets. A capture runs either on an SSH node or on
//! the machine running the application; both return the same output shape so
//! the per-tool parsers and delivery paths stay unchanged.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use oxideterm_connection_monitor::{ResourceSampleShell, ResourceSampler, ResourceSamplerFuture};
use oxideterm_ssh::{SshCommandOutput, SshConnectionHandle};
use tokio::io::AsyncReadExt;

/// Synthetic Host Tools connection id for the local machine.
pub(in crate::workspace) const LOCAL_HOST_CONNECTION_ID: &str = "oxide-local-host";

/// Local OS in the same vocabulary as remote environment detection, so the
/// existing per-OS command builders select the local dialect unchanged.
pub(in crate::workspace) fn local_host_os_type() -> &'static str {
    match std::env::consts::OS {
        "windows" => "Windows",
        "macos" => "macOS",
        "freebsd" => "FreeBSD",
        "openbsd" => "OpenBSD",
        "netbsd" => "NetBSD",
        "solaris" => "SunOS",
        _ => "Linux",
    }
}

/// Where one Host Tools capture or action runs.
pub(in crate::workspace) enum HostCommandTarget {
    Ssh(SshConnectionHandle),
    Local,
}

impl HostCommandTarget {
    pub(super) async fn run_command_capture(
        &self,
        command: &str,
        timeout: Duration,
        max_output_size: usize,
    ) -> Result<SshCommandOutput, ()> {
        match self {
            Self::Ssh(handle) => handle
                .run_command_capture(command, timeout, max_output_size)
                .await
                .map_err(|_| ()),
            Self::Local => run_local_command_capture(command, timeout, max_output_size).await,
        }
    }
}

/// Executes a generated command line through the local shell the way an SSH
/// exec would on that platform: `cmd.exe` on Windows (the line already carries
/// the PowerShell invocation) and `sh` elsewhere.
async fn run_local_command_capture(
    command: &str,
    timeout: Duration,
    max_output_size: usize,
) -> Result<SshCommandOutput, ()> {
    let line = command.trim_end_matches(['\r', '\n']);
    let mut process = local_shell_command(line);
    process
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = process.spawn().map_err(|_| ())?;
    let stdout = child.stdout.take().ok_or(())?;
    let stderr = child.stderr.take().ok_or(())?;
    // Both streams share one budget, matching the SSH capture limit.
    let budget = Arc::new(AtomicUsize::new(max_output_size));
    let (stdout, stderr, status) = tokio::time::timeout(timeout, async {
        let (stdout, stderr, status) = tokio::join!(
            read_capped(stdout, budget.clone()),
            read_capped(stderr, budget.clone()),
            child.wait(),
        );
        (stdout, stderr, status)
    })
    .await
    .map_err(|_| ())?;
    let status = status.map_err(|_| ())?;
    let (stdout, stdout_truncated) = stdout;
    let (stderr, stderr_truncated) = stderr;
    Ok(SshCommandOutput {
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        exit_code: status.code(),
        truncated: stdout_truncated || stderr_truncated,
    })
}

/// Runs profiler samples as one-shot local captures. SSH profilers keep a
/// dedicated shell per connection; the local machine instead pays one process
/// spawn per sample, which the 10 s sampling interval makes acceptable.
pub(in crate::workspace) struct LocalResourceSampler;

impl ResourceSampler for LocalResourceSampler {
    fn open_shell<'a>(
        &'a self,
        _init_command: &'a str,
        _timeout: Duration,
    ) -> ResourceSamplerFuture<'a, Result<Box<dyn ResourceSampleShell>, String>> {
        Box::pin(
            async move { Ok(Box::new(LocalResourceSampleShell) as Box<dyn ResourceSampleShell>) },
        )
    }
}

struct LocalResourceSampleShell;

impl ResourceSampleShell for LocalResourceSampleShell {
    fn sample_until<'a>(
        &'a mut self,
        command: &'a str,
        _end_marker: &'a str,
        timeout: Duration,
        max_output_size: usize,
    ) -> ResourceSamplerFuture<'a, Result<String, String>> {
        Box::pin(async move {
            run_local_command_capture(command, timeout, max_output_size)
                .await
                .map(|output| output.stdout)
                .map_err(|_| "Local sample capture failed.".to_string())
        })
    }

    fn close<'a>(&'a mut self) -> ResourceSamplerFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
}

#[cfg(windows)]
const LOCAL_HOST_COMMAND_CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn local_shell_command(line: &str) -> tokio::process::Command {
    #[cfg(windows)]
    {
        // Captures run from the GUI process, which has no console. Without
        // this flag every command flashes a console window for cmd.exe,
        // powershell.exe, and their children.
        use std::os::windows::process::CommandExt;
        let mut process = tokio::process::Command::new("cmd");
        // The generated line already carries the quoting its own shell expects
        // (`-Command "..."`). `arg` would escape those quotes for the C
        // runtime and cmd would hand PowerShell a literal quoted string, so the
        // line is appended verbatim instead.
        process.arg("/C");
        process.raw_arg(line);
        process.creation_flags(LOCAL_HOST_COMMAND_CREATE_NO_WINDOW);
        process
    }
    #[cfg(not(windows))]
    {
        let mut process = tokio::process::Command::new("sh");
        process.arg("-c").arg(line);
        process
    }
}

/// Drains a stream to EOF while retaining at most the shared budget, so a
/// large capture cannot block the child on a full pipe.
async fn read_capped<R>(mut reader: R, budget: Arc<AtomicUsize>) -> (Vec<u8>, bool)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut output = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut truncated = false;
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let keep = match budget.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                    Some(left.saturating_sub(read))
                }) {
                    Ok(previous) => previous.min(read),
                    Err(_) => 0,
                };
                if keep > 0 {
                    output.extend_from_slice(&chunk[..keep]);
                }
                if keep < read {
                    truncated = true;
                }
            }
        }
    }
    (output, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_capture_reports_output_and_exit_code() {
        let command = "echo oxide-local";
        let output = run_local_command_capture(command, Duration::from_secs(10), 4096)
            .await
            .expect("local capture");
        assert_eq!(output.stdout.trim(), "oxide-local");
        assert_eq!(output.exit_code, Some(0));
        assert!(!output.truncated);
    }

    #[tokio::test]
    async fn local_capture_truncates_at_the_shared_budget() {
        let command = if cfg!(windows) {
            "for /L %i in (1,1,2000) do @echo 0123456789"
        } else {
            "i=0; while [ $i -lt 2000 ]; do echo 0123456789; i=$((i+1)); done"
        };
        let output = run_local_command_capture(command, Duration::from_secs(30), 64)
            .await
            .expect("local capture");
        assert!(output.truncated);
        assert!(output.stdout.len() + output.stderr.len() <= 64);
    }

    #[tokio::test]
    #[cfg(windows)]
    async fn local_capture_preserves_embedded_double_quotes() {
        // Generated Windows commands carry `-Command "..."` scripts. Escaping
        // the embedded quotes a second time made cmd hand PowerShell a literal
        // string, which PowerShell printed instead of running.
        let output = run_local_command_capture(
            "powershell -NoProfile -NonInteractive -Command \"Write-Output ('oxide'+'-quoted')\"",
            Duration::from_secs(30),
            4096,
        )
        .await
        .expect("local capture");
        assert_eq!(output.stdout.trim(), "oxide-quoted");
    }
}
