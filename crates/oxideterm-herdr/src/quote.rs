// Copyright (C) 2026 jaime.su
// SPDX-License-Identifier: GPL-3.0-only

/// Quote one value for use as a single POSIX shell argument.
pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Run a generated Unix probe with a stable POSIX parser instead of the
/// account's interactive login shell, which may be Fish, Csh, Tcsh, or Nushell.
pub(crate) fn posix_shell_command(script: &str) -> String {
    format!("/bin/sh -c {}", shell_quote(script))
}

/// Quote one value for use as a single PowerShell string argument.
pub(crate) fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Run a generated script on a Windows target.
///
/// Windows sshd parses the exec line with `cmd.exe` or with `powershell.exe`,
/// depending on `DefaultShell`, and both rewrite `$` and the surrounding
/// quotes before PowerShell sees the script: `2>$null` arrives as `2>` under a
/// PowerShell default shell, and an unescaped `"..."` payload is echoed
/// instead of executed under `cmd.exe`. The script therefore travels as
/// base64 UTF-16LE, which neither shell rewrites.
pub(crate) fn windows_command(script: &str) -> String {
    use base64::Engine as _;
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("powershell -NoProfile -NonInteractive -EncodedCommand {encoded}\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn herdr_quoting_preserves_spaces_and_apostrophes() {
        assert_eq!(shell_quote("Oxide Term"), "'Oxide Term'");
        assert_eq!(shell_quote("it's-ready"), "'it'\"'\"'s-ready'");
        assert_eq!(powershell_quote("it's-ready"), "'it''s-ready'");
    }
}
