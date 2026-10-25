use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceRedisStatus {
    #[default]
    Unknown,
    Available,
    /// The `redis-cli` binary is not installed on the host. This says nothing
    /// about a server: containerized or server-only installs have no client.
    Unavailable,
    /// Nothing is listening on the probe target.
    NoServer,
    /// The server answers but requires authentication; v1 probes are
    /// unauthenticated by policy and never send credentials.
    AuthRequired,
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRedisSnapshot {
    pub status: ResourceRedisStatus,
}

const REDIS_UNAVAILABLE_MARKER: &str = "__OXIDE_REDIS_UNAVAILABLE__";
const REDIS_ERROR_MARKER: &str = "__OXIDE_REDIS_ERROR__";
const REDIS_OK_LINE: &str = "REDIS_OK";
pub const REDIS_PROBE_TARGET: &str = "127.0.0.1:6379";

// The probe is deliberately unauthenticated: `-a`, `REDISCLI_AUTH`, and
// `--askpass` would place a credential in the remote command line where `ps`
// exposes it. Non-default ports belong to per-connection configuration.
const REDIS_SAMPLE_COMMAND_UNIX: &str = concat!(
    "echo '===REDIS==='; ",
    "if command -v redis-cli >/dev/null 2>&1; then ",
    "oxide_redis_reply=$(redis-cli -h 127.0.0.1 -p 6379 -t 3 PING 2>&1); ",
    "if [ \"$oxide_redis_reply\" = \"PONG\" ]; then ",
    "echo 'REDIS_OK'; ",
    "else ",
    "printf '__OXIDE_REDIS_ERROR__\\t%s\\n' \"$(printf '%s' \"$oxide_redis_reply\" | head -n 1 | tr '\\t' ' ' | tr -d '\\r')\"; ",
    "fi; ",
    "else ",
    "echo '__OXIDE_REDIS_UNAVAILABLE__'; ",
    "fi; ",
    "echo '===REDIS_END==='"
);

const REDIS_SAMPLE_COMMAND_WINDOWS: &str = concat!(
    "Write-Output '===REDIS===';",
    "if(Get-Command redis-cli -ErrorAction SilentlyContinue){",
    "$oxideRedisReply=& redis-cli -h 127.0.0.1 -p 6379 -t 3 PING 2>&1;",
    "if(\"$oxideRedisReply\" -eq 'PONG'){Write-Output 'REDIS_OK'}",
    "else{",
    "$oxideRedisFirst=($oxideRedisReply|Select-Object -First 1) -replace [char]9,' ' -replace [char]13,'';",
    "Write-Output ('__OXIDE_REDIS_ERROR__'+[char]9+$oxideRedisFirst)",
    "}",
    "}else{Write-Output '__OXIDE_REDIS_UNAVAILABLE__'};",
    "Write-Output '===REDIS_END===';"
);

pub fn redis_sample_command(os_type: &str) -> &'static str {
    match os_type {
        "Windows" | "windows" => REDIS_SAMPLE_COMMAND_WINDOWS,
        _ => REDIS_SAMPLE_COMMAND_UNIX,
    }
}

pub fn parse_redis_snapshot(output: &str) -> ResourceRedisSnapshot {
    let Some(section) = extract_section(output, "REDIS") else {
        return ResourceRedisSnapshot::default();
    };

    for line in section
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        // Markers are recognized as standalone lines only so a crafted server
        // reply cannot forge a status.
        if line == REDIS_UNAVAILABLE_MARKER {
            return ResourceRedisSnapshot {
                status: ResourceRedisStatus::Unavailable,
            };
        }
        if line == REDIS_OK_LINE {
            return ResourceRedisSnapshot {
                status: ResourceRedisStatus::Available,
            };
        }
        if let Some(reply) = line.strip_prefix(REDIS_ERROR_MARKER) {
            return ResourceRedisSnapshot {
                status: classify_redis_failure(clean_server_reply(reply)),
            };
        }
    }

    ResourceRedisSnapshot::default()
}

/// Redis replies are server-controlled strings, unlike docker's local binary
/// output: keep the first line only, replace control characters with spaces,
/// and bound the length before the text reaches UI or diagnostics.
fn clean_server_reply(value: &str) -> Option<String> {
    let line = value.trim_start_matches('\t').lines().next()?;
    let cleaned: String = line
        .trim()
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(180)
        .collect();
    let cleaned = cleaned.trim();
    (!cleaned.is_empty()).then(|| cleaned.to_string())
}

fn classify_redis_failure(reply: Option<String>) -> ResourceRedisStatus {
    let Some(reply) = reply else {
        return ResourceRedisStatus::Error {
            message: "Redis probe failed.".to_string(),
        };
    };
    // Match the reply-code prefix, not the sentence: Valkey/KeyDB and redis
    // releases reword the text but keep the error code.
    let lowered = reply.to_lowercase();
    if lowered.starts_with("noauth") {
        ResourceRedisStatus::AuthRequired
    } else if lowered.contains("connection refused") {
        ResourceRedisStatus::NoServer
    } else {
        ResourceRedisStatus::Error { message: reply }
    }
}

fn extract_section<'a>(output: &'a str, name: &str) -> Option<&'a str> {
    let start = format!("==={name}===");
    let end = format!("==={name}_END===");
    let after_start = output.split_once(&start)?.1;
    Some(after_start.split_once(&end)?.0.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redis_parser_maps_ping_replies_to_states() {
        for (case, output, expected) in [
            (
                "pong",
                "===REDIS===\nREDIS_OK\n===REDIS_END===",
                ResourceRedisStatus::Available,
            ),
            (
                "missing_client_binary",
                "===REDIS===\n__OXIDE_REDIS_UNAVAILABLE__\n===REDIS_END===",
                ResourceRedisStatus::Unavailable,
            ),
            (
                "auth_required",
                concat!(
                    "===REDIS===\n",
                    "__OXIDE_REDIS_ERROR__\tNOAUTH Authentication required.\n",
                    "===REDIS_END===",
                ),
                ResourceRedisStatus::AuthRequired,
            ),
            (
                "valkey_reworded_auth_reply_keeps_error_code",
                concat!(
                    "===REDIS===\n",
                    "__OXIDE_REDIS_ERROR__\tNOAUTH HELLO must be called with AUTH\n",
                    "===REDIS_END===",
                ),
                ResourceRedisStatus::AuthRequired,
            ),
            (
                "nothing_listening",
                concat!(
                    "===REDIS===\n",
                    "__OXIDE_REDIS_ERROR__\tCould not connect to Redis at 127.0.0.1:6379: Connection refused\n",
                    "===REDIS_END===",
                ),
                ResourceRedisStatus::NoServer,
            ),
            (
                "unexpected_reply",
                concat!(
                    "===REDIS===\n",
                    "__OXIDE_REDIS_ERROR__\tERR unknown command 'PONG'\n",
                    "===REDIS_END===",
                ),
                ResourceRedisStatus::Error {
                    message: "ERR unknown command 'PONG'".to_string(),
                },
            ),
        ] {
            assert_eq!(parse_redis_snapshot(output).status, expected, "{case}");
        }
    }

    #[test]
    fn redis_parser_sanitizes_server_controlled_text() {
        let output = concat!(
            "===REDIS===\n",
            "__OXIDE_REDIS_ERROR__\tERR injected\u{7}bell\u{1b}[31mcolor\nsecond line\n",
            "===REDIS_END===",
        );
        assert_eq!(
            parse_redis_snapshot(output).status,
            ResourceRedisStatus::Error {
                message: "ERR injected bell [31mcolor".to_string(),
            }
        );
    }

    #[test]
    fn redis_parser_rejects_forged_markers_inside_server_replies() {
        // A hostile server reply must not flip the probe into a status marker;
        // only standalone marker lines are recognized.
        let output = concat!(
            "===REDIS===\n",
            "__OXIDE_REDIS_ERROR__\tERR crafted __OXIDE_REDIS_UNAVAILABLE__ payload\n",
            "===REDIS_END===",
        );
        assert_eq!(
            parse_redis_snapshot(output).status,
            ResourceRedisStatus::Error {
                message: "ERR crafted __OXIDE_REDIS_UNAVAILABLE__ payload".to_string(),
            }
        );
    }

    #[test]
    fn redis_parser_empty_capture_stays_unknown() {
        assert_eq!(
            parse_redis_snapshot("===REDIS===\n===REDIS_END===").status,
            ResourceRedisStatus::Unknown
        );
        assert_eq!(
            parse_redis_snapshot("no section at all").status,
            ResourceRedisStatus::Unknown
        );
    }

    #[test]
    fn redis_sample_command_probes_without_credentials() {
        for os_type in ["Linux", "Windows"] {
            let command = redis_sample_command(os_type);
            assert!(command.contains("-h 127.0.0.1"));
            assert!(command.contains("-p 6379"));
            assert!(command.contains("PING"));
            // Credentials on the command line are visible to `ps` and would
            // leak into remote shell history; the probe stays anonymous.
            assert!(!command.contains("-a "));
            assert!(!command.contains("REDISCLI_AUTH"));
            assert!(!command.contains("askpass"));
            assert!(command.contains(REDIS_UNAVAILABLE_MARKER));
            assert!(command.contains(REDIS_ERROR_MARKER));
        }
    }
}
