// Copyright (C) 2026 jaime.su
// SPDX-License-Identifier: GPL-3.0-only

//! POSIX quoting shared by every generated eCapture script.

/// Quotes one value for a POSIX shell argument.
pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Runs a generated script under `sh` because the account's login shell may be
/// Fish, Csh, Tcsh, or Nushell.
pub(crate) fn posix_shell_command(script: &str) -> String {
    format!("/bin/sh -c {}", shell_quote(script))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_preserves_spaces_and_apostrophes() {
        assert_eq!(shell_quote("Oxide Term"), "'Oxide Term'");
        assert_eq!(shell_quote("it's-ready"), "'it'\"'\"'s-ready'");
        assert_eq!(posix_shell_command("echo 1"), "/bin/sh -c 'echo 1'");
    }
}
