// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! Shell quoting and identifier validation for generated commands.

/// Wraps a value so a POSIX shell keeps it as one literal argument. The same
/// form is safe in PowerShell, where single quotes are literal too.
pub fn shell_quote_single(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for character in value.chars() {
        if character == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(character);
        }
    }
    quoted.push('\'');
    quoted
}

/// Container and image identifiers are hex digests; 12 characters is the
/// shortest abbreviation every engine accepts, 64 the full digest.
pub fn validated_container_id(container_id: &str) -> Result<&str, String> {
    let container_id = container_id.trim();
    if container_id.len() < 12
        || container_id.len() > 64
        || !container_id
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err("Invalid container identifier.".to_string());
    }
    Ok(container_id)
}

/// Volume, network and pod names allow a wider character set but never shell
/// metacharacters or whitespace.
pub fn validated_resource_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty()
        || name.len() > 128
        || !name.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '_' | '.' | '-' | ':' | '/' | '@')
        })
    {
        return Err("Invalid container resource reference.".to_string());
    }
    Ok(name)
}

/// Versions come from remote output; keep them printable and bounded before
/// they reach a list row.
pub fn sanitized_version(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let sanitized: String = raw
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '+' | '-' | '_')
        })
        .take(32)
        .collect();
    (!sanitized.is_empty()).then_some(sanitized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_and_validation_reject_shell_metacharacters() {
        assert_eq!(shell_quote_single("a'b"), "'a'\\''b'");
        assert!(validated_container_id("a1b2c3d4e5f6").is_ok());
        assert!(validated_container_id("abc123").is_err());
        assert!(validated_container_id("a1b2c3d4e5f6; rm -rf /").is_err());
        assert!(validated_resource_name("app-data").is_ok());
        assert!(validated_resource_name("bad name").is_err());
        assert_eq!(sanitized_version(" 5.8.2\n"), Some("5.8.2".to_string()));
        assert_eq!(
            sanitized_version("3.0.3.0\r\n"),
            Some("3.0.3.0".to_string())
        );
        assert_eq!(
            sanitized_version("5.8.2\u{0}\u{1b}"),
            Some("5.8.2".to_string())
        );
    }
}
