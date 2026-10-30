// Copyright (C) 2026 jaime.su
// SPDX-License-Identifier: GPL-3.0-only

//! Launch templates and form validation for one eCapture capture.
//!
//! A template selects the eCapture subcommand and the remote value the user
//! must supply. Command construction stays in [`crate::command`]; these types
//! are serializable form state so the panel can keep a draft per window.

use serde::{Deserialize, Serialize};

/// Upper bound for a user-supplied remote path or container reference.
pub const ECAPTURE_MAX_TARGET_LEN: usize = 1024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EcaptureTemplate {
    /// `ecapture tls` with eCapture's own libssl discovery.
    #[default]
    Libssl,
    /// `ecapture tls --libssl <path>` for a Node.js runtime or its libssl.
    Node,
    /// `ecapture gotls --elfpath <path>` for a Go binary.
    Gotls,
    /// `ecapture tls --cgroup_path ...` scoped to one Docker container.
    Docker,
}

impl EcaptureTemplate {
    pub const ALL: [Self; 4] = [Self::Libssl, Self::Node, Self::Gotls, Self::Docker];

    pub fn label_key(self) -> &'static str {
        ecapture_template_label_key(self)
    }

    /// The libssl template defers to eCapture's own discovery, so its target
    /// stays optional; every other template needs an explicit remote value.
    pub fn target_required(self) -> bool {
        !matches!(self, Self::Libssl)
    }

    /// Whether the target is a remote path rather than a container reference.
    pub fn target_is_path(self) -> bool {
        matches!(self, Self::Libssl | Self::Node | Self::Gotls)
    }

    pub fn target_label_key(self) -> Option<&'static str> {
        match self {
            Self::Libssl => Some("sidebar.host_ecapture.fields.libssl_path"),
            Self::Node => Some("sidebar.host_ecapture.fields.node_path"),
            Self::Gotls => Some("sidebar.host_ecapture.fields.go_binary"),
            Self::Docker => Some("sidebar.host_ecapture.fields.container"),
        }
    }

    pub fn target_placeholder_key(self) -> Option<&'static str> {
        match self {
            Self::Libssl => Some("sidebar.host_ecapture.fields.libssl_placeholder"),
            Self::Node => Some("sidebar.host_ecapture.fields.node_placeholder"),
            Self::Gotls => Some("sidebar.host_ecapture.fields.go_placeholder"),
            Self::Docker => Some("sidebar.host_ecapture.fields.container_placeholder"),
        }
    }
}

pub fn ecapture_template_label_key(template: EcaptureTemplate) -> &'static str {
    match template {
        EcaptureTemplate::Libssl => "sidebar.host_ecapture.templates.libssl",
        EcaptureTemplate::Node => "sidebar.host_ecapture.templates.node",
        EcaptureTemplate::Gotls => "sidebar.host_ecapture.templates.gotls",
        EcaptureTemplate::Docker => "sidebar.host_ecapture.templates.docker",
    }
}

/// Draft form state owned by the panel.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EcaptureLaunch {
    pub template: EcaptureTemplate,
    pub target: String,
    /// Raw PID field text; parsed and validated on start.
    pub pid: String,
}

/// Validated launch values consumed by the command builders.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EcaptureSpec {
    pub template: EcaptureTemplate,
    pub target: String,
    pub pid: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EcaptureValidationError {
    TargetRequired,
    TargetTooLong,
    TargetInvalid,
    PidInvalid,
}

impl EcaptureValidationError {
    pub fn label_key(self) -> &'static str {
        crate::model::ecapture_validation_label_key(self)
    }
}

/// Validates form text before any remote command interpolates it.
///
/// Empty optional targets stay empty so eCapture keeps its own discovery. PID
/// `0` means "every process" for eCapture, which is the same as omitting the
/// filter here.
pub fn validate_ecapture_launch(
    launch: &EcaptureLaunch,
) -> Result<EcaptureSpec, EcaptureValidationError> {
    let target = launch.target.trim();
    if target.is_empty() {
        if launch.template.target_required() {
            return Err(EcaptureValidationError::TargetRequired);
        }
    } else if target.len() > ECAPTURE_MAX_TARGET_LEN {
        return Err(EcaptureValidationError::TargetTooLong);
    } else if target.chars().any(char::is_control) {
        return Err(EcaptureValidationError::TargetInvalid);
    } else if launch.template == EcaptureTemplate::Docker && !valid_container_reference(target) {
        return Err(EcaptureValidationError::TargetInvalid);
    }

    let pid = match launch.pid.trim() {
        "" => None,
        value => match value.parse::<u32>() {
            Ok(0) => None,
            Ok(pid) => Some(pid),
            Err(_) => return Err(EcaptureValidationError::PidInvalid),
        },
    };

    Ok(EcaptureSpec {
        template: launch.template,
        target: target.to_string(),
        pid,
    })
}

/// Docker accepts names and ID prefixes; keeping the reference inside this
/// charset stops a typo from reaching `docker inspect` as an option.
fn valid_container_reference(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && chars.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_requires_targets_except_libssl() {
        for (case, template, expected) in [
            ("libssl optional", EcaptureTemplate::Libssl, None),
            (
                "node required",
                EcaptureTemplate::Node,
                Some(EcaptureValidationError::TargetRequired),
            ),
            (
                "gotls required",
                EcaptureTemplate::Gotls,
                Some(EcaptureValidationError::TargetRequired),
            ),
        ] {
            let launch = EcaptureLaunch {
                template,
                target: String::new(),
                pid: String::new(),
            };
            let result = validate_ecapture_launch(&launch);
            match expected {
                None => assert!(result.is_ok(), "{case}"),
                Some(error) => assert_eq!(result, Err(error), "{case}"),
            }
        }
    }

    #[test]
    fn launch_rejects_control_characters_and_bad_pids() {
        let launch = EcaptureLaunch {
            template: EcaptureTemplate::Gotls,
            target: "/opt/app\nrm -rf /".to_string(),
            pid: String::new(),
        };
        assert_eq!(
            validate_ecapture_launch(&launch),
            Err(EcaptureValidationError::TargetInvalid)
        );

        let launch = EcaptureLaunch {
            template: EcaptureTemplate::Libssl,
            target: String::new(),
            pid: "12x".to_string(),
        };
        assert_eq!(
            validate_ecapture_launch(&launch),
            Err(EcaptureValidationError::PidInvalid)
        );
    }

    #[test]
    fn launch_keeps_pid_filters_and_container_references() {
        let launch = EcaptureLaunch {
            template: EcaptureTemplate::Docker,
            target: " api-gateway_1 ".to_string(),
            pid: "0".to_string(),
        };
        let spec = validate_ecapture_launch(&launch).expect("valid docker launch");
        assert_eq!(spec.target, "api-gateway_1");
        assert_eq!(spec.pid, None);

        for invalid in ["api/gateway", "-api", "api gateway"] {
            let launch = EcaptureLaunch {
                template: EcaptureTemplate::Docker,
                target: invalid.to_string(),
                pid: String::new(),
            };
            assert_eq!(
                validate_ecapture_launch(&launch),
                Err(EcaptureValidationError::TargetInvalid),
                "{invalid}"
            );
        }

        let launch = EcaptureLaunch {
            template: EcaptureTemplate::Gotls,
            target: "/srv/go/service".to_string(),
            pid: " 4242 ".to_string(),
        };
        assert_eq!(validate_ecapture_launch(&launch).unwrap().pid, Some(4242));
    }
}
