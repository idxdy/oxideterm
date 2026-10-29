// Copyright (C) 2026 jaime.su
// SPDX-License-Identifier: GPL-3.0-only

//! Validators for values that come back from the remote herdr server and
//! therefore cannot be trusted when they are interpolated into a command line.

const HERDR_ID_MAX_LEN: usize = 32;
const HERDR_NAME_MAX_LEN: usize = 128;

/// Herdr ids look like `wA` / `wA:pC` / `wA:tB`; the grammar is stricter than
/// a free label so a hostile id cannot smuggle shell metacharacters.
pub(crate) fn validated_id(id: &str) -> Result<String, String> {
    let id = id.trim();
    if id.is_empty()
        || id.len() > HERDR_ID_MAX_LEN
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-' | '.'))
    {
        return Err("Invalid herdr id.".to_string());
    }
    Ok(id.to_string())
}

/// Session names, workspace labels and pane labels are server-authored free
/// text; control characters are rejected because they would break the
/// single-line command the name is quoted into.
pub(crate) fn validated_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > HERDR_NAME_MAX_LEN
        || name.chars().any(char::is_control)
    {
        return Err("Invalid herdr name.".to_string());
    }
    Ok(name.to_string())
}
