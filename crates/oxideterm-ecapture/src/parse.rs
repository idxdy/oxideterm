// Copyright (C) 2026 jaime.su
// SPDX-License-Identifier: GPL-3.0-only

//! Parsers for the framed probe, poll, and event text produced remotely.
//!
//! eCapture text output changed between major versions: v2 prints a bracketed
//! timestamp with `PID:... FD:... WRITE (N bytes):`, Go TLS prints
//! `PID:..., Type:WRITE, Len:N` followed by `Data:`, and older releases printed
//! `??? UUID:...` blocks. All three are accepted so a host that still ships an
//! older binary remains readable.

use std::sync::LazyLock;

use regex::Regex;

use crate::command::{
    absent_marker, end_marker, events_marker, log_marker, meta_marker, started_marker,
};
use crate::model::{
    ECAPTURE_MAX_EVENT_PAYLOAD_BYTES, EcaptureCommandCapability, EcaptureDirection, EcaptureEvent,
    EcaptureKind, ResourceEcaptureProbe, ResourceEcaptureStatus,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EcapturePoll {
    /// The run directory does not exist: no capture was started for this host.
    pub absent: bool,
    pub running: bool,
    pub total_lines: u64,
    pub file_bytes: u64,
    pub start_line: u64,
    pub end_line: u64,
    /// Only newline-terminated event lines fetched by this poll.
    pub events_text: String,
    pub log_lines: Vec<String>,
    /// All three end markers arrived; a truncated response replays the cut line.
    pub complete: bool,
    /// The SSH capture budget truncated the command output.
    pub truncated: bool,
}

impl EcapturePoll {
    /// Number of complete event lines this poll actually delivered.
    pub fn complete_line_count(&self) -> usize {
        self.events_text
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
    }
}

pub fn parse_ecapture_probe(stdout: &str) -> ResourceEcaptureProbe {
    let mut probe = ResourceEcaptureProbe::default();
    let mut saw_field = false;
    for line in stdout.lines() {
        let Some((key, value)) = line.trim_end_matches('\r').split_once('\t') else {
            continue;
        };
        saw_field = true;
        let value = value.trim();
        match key {
            "BIN" => probe.binary = value.to_string(),
            "VERSION" => probe.version = value.to_string(),
            "PRIV" => probe.privileged = value == "1",
            "BTF" => probe.btf = value == "1",
            "KERNEL" => probe.kernel = value.to_string(),
            _ => {}
        }
    }
    if saw_field {
        probe.status = if probe.binary.is_empty() {
            ResourceEcaptureStatus::Unavailable
        } else {
            ResourceEcaptureStatus::Available {
                capability: if probe.privileged {
                    EcaptureCommandCapability::Full
                } else {
                    // Unprivileged sessions can inspect the host but cannot
                    // attach eBPF probes; the panel keeps start disabled.
                    EcaptureCommandCapability::Partial
                },
                platform: "linux".to_string(),
            }
        };
    }
    probe
}

pub fn parse_ecapture_start_finished(run_id: &str, stdout: &str) -> bool {
    let expected = started_marker(run_id);
    stdout
        .lines()
        .any(|line| line.trim_end_matches('\r') == expected)
}

pub fn parse_ecapture_poll(run_id: &str, stdout: &str, truncated: bool) -> EcapturePoll {
    let absent = absent_marker(run_id);
    let meta = meta_marker(run_id);
    let events = events_marker(run_id);
    let log = log_marker(run_id);
    let end = end_marker(run_id);

    let mut poll = EcapturePoll {
        truncated,
        ..EcapturePoll::default()
    };
    let mut saw_meta = false;
    let mut saw_end = false;
    let mut section = Section::None;
    let mut events_text = String::new();
    let mut log_lines = Vec::new();

    // A missing trailing newline means the SSH budget cut the last line mid
    // record; that fragment must not enter the event stream.
    let output_complete = stdout.ends_with('\n') || stdout.is_empty();
    for chunk in stdout.split_inclusive('\n') {
        let terminated = chunk.ends_with('\n');
        let line = chunk.trim_end_matches(['\n', '\r']);
        if !terminated && !output_complete {
            break;
        }
        if line == absent {
            poll.absent = true;
            poll.complete = true;
            return poll;
        }
        if let Some(fields) = line.strip_prefix(meta.as_str()).and_then(|rest| {
            rest.strip_prefix('\t')
                .map(|rest| rest.split('\t').collect::<Vec<_>>())
        }) {
            saw_meta = true;
            if fields.len() >= 5 {
                poll.running = fields[0].trim() == "1";
                poll.total_lines = parse_u64(fields[1]);
                poll.file_bytes = parse_u64(fields[2]);
                poll.start_line = parse_u64(fields[3]);
                poll.end_line = parse_u64(fields[4]);
            }
            continue;
        }
        if line == events {
            section = Section::Events;
            continue;
        }
        if line == log {
            section = Section::Log;
            continue;
        }
        if line == end {
            saw_end = true;
            break;
        }
        match section {
            Section::Events => {
                if !terminated {
                    break;
                }
                events_text.push_str(chunk);
            }
            Section::Log => log_lines.push(line.to_string()),
            Section::None => {}
        }
    }

    if !poll.absent {
        poll.events_text = events_text;
        poll.log_lines = log_lines;
        poll.complete = saw_meta && saw_end;
        if !saw_meta {
            poll.start_line = 0;
        }
    }
    poll
}

enum Section {
    None,
    Events,
    Log,
}

fn parse_u64(value: &str) -> u64 {
    value.trim().parse().unwrap_or(0)
}

enum RecordHeader {
    Openssl {
        timestamp: String,
        pid: u32,
        tid: u32,
        comm: String,
        fd: u32,
        tuple: String,
        direction: EcaptureDirection,
        length: usize,
    },
    Gotls {
        pid: u32,
        tid: u32,
        comm: String,
        fd: u32,
        tuple: String,
        direction: EcaptureDirection,
        length: usize,
    },
    Legacy {
        pid: u32,
        tid: u32,
        comm: String,
        fd: u32,
        tuple: String,
        name: String,
        length: usize,
    },
}

static OPENSSL_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\[([^\[\]]{1,64})\] PID:(\d{1,10}) TID:(\d{1,10}) Comm:(.*?) FD:(\d{1,10})(?: Tuple:(.*?))? (READ|WRITE) \((\d{1,10}) bytes\):$",
    )
    .expect("static openssl header regex")
});

static GOTLS_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^PID:(\d{1,10}), TID:(\d{1,10}), Comm:(.*?), FD:(\d{1,10})(?:, Tuple:(.*?))?, Type:(READ|WRITE), Len:(\d{1,10})$",
    )
    .expect("static gotls header regex")
});

static LEGACY_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:[0-9T:.+\-Z]{1,40} )?\?\?\? UUID:([^,]{1,256}), Name:([^,]{1,64}), Type:(\d{1,10}), Length:(\d{1,10})$",
    )
    .expect("static legacy header regex")
});

fn parse_direction(value: &str) -> EcaptureDirection {
    match value {
        "READ" => EcaptureDirection::Read,
        "WRITE" => EcaptureDirection::Write,
        _ => EcaptureDirection::Unknown,
    }
}

fn record_header(line: &str) -> Option<RecordHeader> {
    let line = line.trim_end_matches('\r');
    if let Some(captures) = OPENSSL_HEADER.captures(line) {
        return Some(RecordHeader::Openssl {
            timestamp: captures
                .get(1)
                .map_or(String::new(), |m| m.as_str().to_string()),
            pid: captures.get(2)?.as_str().parse().ok()?,
            tid: captures.get(3)?.as_str().parse().ok()?,
            comm: captures
                .get(4)
                .map_or(String::new(), |m| m.as_str().to_string()),
            fd: captures.get(5)?.as_str().parse().ok()?,
            tuple: captures
                .get(6)
                .map_or(String::new(), |m| m.as_str().to_string()),
            direction: parse_direction(captures.get(7)?.as_str()),
            length: captures.get(8)?.as_str().parse().ok()?,
        });
    }
    if let Some(captures) = GOTLS_HEADER.captures(line) {
        return Some(RecordHeader::Gotls {
            pid: captures.get(1)?.as_str().parse().ok()?,
            tid: captures.get(2)?.as_str().parse().ok()?,
            comm: captures
                .get(3)
                .map_or(String::new(), |m| m.as_str().to_string()),
            fd: captures.get(4)?.as_str().parse().ok()?,
            tuple: captures
                .get(5)
                .map_or(String::new(), |m| m.as_str().to_string()),
            direction: parse_direction(captures.get(6)?.as_str()),
            length: captures.get(7)?.as_str().parse().ok()?,
        });
    }
    let captures = LEGACY_HEADER.captures(line)?;
    let identity = parse_legacy_uuid(captures.get(1)?.as_str())?;
    Some(RecordHeader::Legacy {
        pid: identity.pid,
        tid: identity.tid,
        comm: identity.comm,
        fd: identity.fd,
        tuple: identity.tuple,
        name: captures
            .get(2)
            .map_or(String::new(), |m| m.as_str().to_string()),
        length: captures.get(4)?.as_str().parse().ok()?,
    })
}

pub(crate) fn is_record_header(line: &str) -> bool {
    record_header(line).is_some()
}

struct LegacyIdentity {
    pid: u32,
    tid: u32,
    comm: String,
    fd: u32,
    tuple: String,
}

/// Older releases append the connection tuple to `pid_tid_comm_fd_datatype`.
/// The comm field may itself contain underscores, so the numeric fields are
/// read from both ends instead of splitting positionally.
fn parse_legacy_uuid(uuid: &str) -> Option<LegacyIdentity> {
    let mut parts: Vec<&str> = uuid.split('_').collect();
    let mut tuple = String::new();
    if parts.len() > 5 && parts.last().is_some_and(|part| part.contains(':')) {
        tuple = parts.pop().unwrap_or_default().to_string();
    }
    if parts.len() < 4 {
        return None;
    }
    let pid = parts[0].parse().ok()?;
    let tid = parts[1].parse().ok()?;
    let fd = parts[parts.len() - 2].parse().ok()?;
    let comm = parts[2..parts.len() - 2].join("_");
    Some(LegacyIdentity {
        pid,
        tid,
        comm,
        fd,
        tuple,
    })
}

pub(crate) fn parse_event(sequence: u64, record: &[String]) -> Option<EcaptureEvent> {
    let header_line = record.first()?;
    let header = record_header(header_line)?;
    let mut payload_lines = &record[1..];
    let (timestamp, pid, tid, comm, fd, tuple, direction, length, kind_hint) = match header {
        RecordHeader::Openssl {
            timestamp,
            pid,
            tid,
            comm,
            fd,
            tuple,
            direction,
            length,
        } => (
            timestamp, pid, tid, comm, fd, tuple, direction, length, None,
        ),
        RecordHeader::Gotls {
            pid,
            tid,
            comm,
            fd,
            tuple,
            direction,
            length,
        } => {
            if payload_lines
                .first()
                .is_some_and(|line| matches!(line.trim_end_matches('\r'), "Data:" | "Data(hex):"))
            {
                payload_lines = &payload_lines[1..];
            }
            (
                String::new(),
                pid,
                tid,
                comm,
                fd,
                tuple,
                direction,
                length,
                None,
            )
        }
        RecordHeader::Legacy {
            pid,
            tid,
            comm,
            fd,
            tuple,
            name,
            length,
        } => {
            let kind = if name.contains("Request") {
                EcaptureKind::Request
            } else if name.contains("Response") {
                EcaptureKind::Response
            } else {
                EcaptureKind::Data
            };
            (
                String::new(),
                pid,
                tid,
                comm,
                fd,
                tuple,
                EcaptureDirection::Unknown,
                length,
                Some((kind, name)),
            )
        }
    };

    let mut payload = payload_lines.join("\n");
    let mut payload_truncated = false;
    if payload.len() > ECAPTURE_MAX_EVENT_PAYLOAD_BYTES {
        payload = truncate_at_char_boundary(&payload, ECAPTURE_MAX_EVENT_PAYLOAD_BYTES);
        payload_truncated = true;
    }
    let payload = sanitize_payload(&payload);

    let (kind, summary) = match kind_hint {
        Some((kind, name)) => (kind, legacy_summary(&name, &payload)),
        None => classify_payload(&payload),
    };

    Some(EcaptureEvent {
        sequence,
        timestamp,
        pid,
        tid,
        comm,
        fd,
        tuple,
        direction,
        kind,
        length,
        summary,
        payload,
        payload_truncated,
    })
}

const HTTP_METHODS: [&str; 9] = [
    "GET", "POST", "PUT", "DELETE", "HEAD", "OPTIONS", "PATCH", "CONNECT", "TRACE",
];

fn classify_payload(payload: &str) -> (EcaptureKind, String) {
    let first = payload
        .lines()
        .next()
        .unwrap_or_default()
        .trim_end_matches('\r');
    let first = first.trim();
    if let Some((method, _)) = first.split_once(' ')
        && HTTP_METHODS.contains(&method)
    {
        return (EcaptureKind::Request, summarize(first));
    }
    if first.starts_with("HTTP/") {
        return (EcaptureKind::Response, summarize(first));
    }
    if first.starts_with("PRI * HTTP/2.0") {
        return (EcaptureKind::Data, "HTTP/2".to_string());
    }
    if first.is_empty() || first.chars().any(char::is_control) {
        return (EcaptureKind::Data, format!("{} bytes", payload.len()));
    }
    (EcaptureKind::Data, summarize(first))
}

/// Legacy HTTP/2 blocks print `header field ":path" = "..."`; those values are
/// the most useful summary for a multiplexed stream.
fn legacy_summary(name: &str, payload: &str) -> String {
    for pseudo in [":path", ":status"] {
        let needle = format!("header field \"{pseudo}\" = \"");
        if let Some(rest) = payload.split(&needle).nth(1)
            && let Some(value) = rest.split('"').next()
        {
            return summarize(&format!("{pseudo}={value}"));
        }
    }
    if name.is_empty() {
        return summarize(payload.lines().next().unwrap_or_default());
    }
    summarize(name)
}

fn summarize(text: &str) -> String {
    let text = text.trim_end_matches('\r');
    let mut summary: String = text.chars().take(140).collect();
    if text.chars().count() > 140 {
        summary.push('…');
    }
    summary
}

fn sanitize_payload(payload: &str) -> String {
    // Terminal text rendering cannot place raw control bytes; the declared
    // length still reports the original size.
    payload
        .chars()
        .map(|character| match character {
            '\n' | '\r' | '\t' => character,
            '\0' => '␀',
            control if control.is_control() => '·',
            other => other,
        })
        .collect()
}

fn truncate_at_char_boundary(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::ecapture_run_id;

    #[test]
    fn probe_reports_available_and_unavailable_hosts() {
        let available = parse_ecapture_probe(
            "UID\t0\nBIN\t/usr/local/bin/ecapture\nVERSION\teCapture version:\tlinux_amd64:v2.9.0\nPRIV\t1\nBTF\t1\nKERNEL\t5.15.0\n",
        );
        assert_eq!(
            available.status,
            ResourceEcaptureStatus::Available {
                capability: EcaptureCommandCapability::Full,
                platform: "linux".to_string()
            }
        );
        assert_eq!(available.binary, "/usr/local/bin/ecapture");
        assert_eq!(available.version, "eCapture version:\tlinux_amd64:v2.9.0");
        assert!(available.privileged && available.btf);

        let unavailable = parse_ecapture_probe("UID\t1000\nBIN\t\nPRIV\t0\nBTF\t1\n");
        assert_eq!(unavailable.status, ResourceEcaptureStatus::Unavailable);
        assert!(!unavailable.privileged);

        assert_eq!(
            parse_ecapture_probe("garbage").status,
            ResourceEcaptureStatus::Unknown
        );
    }

    #[test]
    fn poll_parser_separates_events_and_logs() {
        let run_id = ecapture_run_id("connection-a");
        let meta = meta_marker(&run_id);
        let events = events_marker(&run_id);
        let log = log_marker(&run_id);
        let end = end_marker(&run_id);
        let stdout = format!(
            "{meta}\t1\t42\t2048\t7\t9\n{events}\nfirst\nsecond\n{log}\nINF started\nWRN detail\n{end}\n"
        );
        let poll = parse_ecapture_poll(&run_id, &stdout, false);
        assert!(poll.complete);
        assert!(poll.running);
        assert_eq!(poll.total_lines, 42);
        assert_eq!(poll.file_bytes, 2048);
        assert_eq!((poll.start_line, poll.end_line), (7, 9));
        assert_eq!(poll.events_text, "first\nsecond\n");
        assert_eq!(poll.complete_line_count(), 2);
        assert_eq!(poll.log_lines, ["INF started", "WRN detail"]);
    }

    #[test]
    fn poll_parser_keeps_cut_line_out_of_the_stream() {
        let run_id = ecapture_run_id("connection-a");
        let events = events_marker(&run_id);
        let truncated = format!("{events}\ncomplete\npartial");
        let poll = parse_ecapture_poll(&run_id, &truncated, true);
        assert!(!poll.complete);
        assert!(poll.truncated);
        assert_eq!(poll.events_text, "complete\n");
        assert!(poll.log_lines.is_empty());
    }

    #[test]
    fn poll_parser_recognises_the_absent_marker() {
        let run_id = ecapture_run_id("connection-a");
        let stdout = format!("{}\n", absent_marker(&run_id));
        let poll = parse_ecapture_poll(&run_id, &stdout, false);
        assert!(poll.absent);
        assert!(poll.complete);
    }

    fn record(header: &str, payload: &[&str]) -> Vec<String> {
        let mut lines = vec![header.to_string()];
        lines.extend(payload.iter().map(|line| line.to_string()));
        lines
    }

    #[test]
    fn parses_openssl_records_and_classifies_http() {
        let event = parse_event(
            3,
            &record(
                "[2026-10-29 10:11:12.345] PID:42 TID:43 Comm:curl FD:5 Tuple:10.0.0.2:443->10.0.0.1:51000 WRITE (31 bytes):",
                &[
                    "GET /api/v1 HTTP/1.1",
                    "Host: example.test",
                ],
            ),
        )
        .expect("openssl record");
        assert_eq!(event.sequence, 3);
        assert_eq!(event.timestamp, "2026-10-29 10:11:12.345");
        assert_eq!((event.pid, event.tid, event.fd), (42, 43, 5));
        assert_eq!(event.comm, "curl");
        assert_eq!(event.tuple, "10.0.0.2:443->10.0.0.1:51000");
        assert_eq!(event.direction, EcaptureDirection::Write);
        assert_eq!(event.kind, EcaptureKind::Request);
        assert_eq!(event.summary, "GET /api/v1 HTTP/1.1");
        assert_eq!(event.payload, "GET /api/v1 HTTP/1.1\nHost: example.test");

        let event = parse_event(
            4,
            &record(
                "[2026-10-29 10:11:12.400] PID:42 TID:43 Comm:curl FD:5 READ (17 bytes):",
                &["HTTP/1.1 200 OK"],
            ),
        )
        .expect("response record");
        assert_eq!(event.direction, EcaptureDirection::Read);
        assert_eq!(event.kind, EcaptureKind::Response);
        assert_eq!(event.summary, "HTTP/1.1 200 OK");
    }

    #[test]
    fn parses_gotls_records_and_skips_the_data_marker() {
        let event = parse_event(
            1,
            &record(
                "PID:9, TID:10, Comm:go-app, FD:7, Tuple:127.0.0.1:8443->127.0.0.1:52000, Type:WRITE, Len:9",
                &["Data:", "PING body"],
            ),
        )
        .expect("gotls record");
        assert_eq!(event.pid, 9);
        assert_eq!(event.comm, "go-app");
        assert_eq!(event.tuple, "127.0.0.1:8443->127.0.0.1:52000");
        assert_eq!(event.direction, EcaptureDirection::Write);
        assert_eq!(event.payload, "PING body");
        assert_eq!(event.kind, EcaptureKind::Data);
        assert_eq!(event.summary, "PING body");
    }

    #[test]
    fn parses_legacy_uuid_records_with_tuple_and_http2_headers() {
        let event = parse_event(
            2,
            &record(
                "2024-09-15T11:51:53Z ??? UUID:233851_233852_my_comm_5_1_172.16.71.1:51837, Name:HTTP2Request, Type:2, Length:304",
                &[
                    "Frame Type\t=>\tHEADERS",
                    "header field \":method\" = \"GET\"",
                    "header field \":path\" = \"/index.html\"",
                ],
            ),
        )
        .expect("legacy record");
        assert_eq!(event.pid, 233851);
        assert_eq!(event.tid, 233852);
        assert_eq!(event.comm, "my_comm");
        assert_eq!(event.fd, 5);
        assert_eq!(event.tuple, "172.16.71.1:51837");
        assert_eq!(event.kind, EcaptureKind::Request);
        assert_eq!(event.summary, ":path=/index.html");
    }

    #[test]
    fn payload_sanitizes_control_characters_and_truncates_long_lines() {
        let event = parse_event(
            1,
            &record(
                "PID:1, TID:1, Comm:x, FD:2, Type:READ, Len:10",
                &["Data:", "a\u{0000}b\u{0007}c"],
            ),
        )
        .expect("gotls record");
        assert_eq!(event.payload, "a␀b·c");

        let long = "x".repeat(ECAPTURE_MAX_EVENT_PAYLOAD_BYTES + 4);
        let event = parse_event(
            1,
            &record(
                "PID:1, TID:1, Comm:x, FD:2, Type:READ, Len:1",
                &["Data:", &long],
            ),
        )
        .expect("gotls record");
        assert!(event.payload_truncated);
        assert_eq!(event.payload.len(), ECAPTURE_MAX_EVENT_PAYLOAD_BYTES);
    }

    #[test]
    fn start_marker_matches_only_the_started_run() {
        let run_id = ecapture_run_id("connection-a");
        let other = ecapture_run_id("connection-b");
        let stdout = format!("{}\n", started_marker(&run_id));
        assert!(parse_ecapture_start_finished(&run_id, &stdout));
        assert!(!parse_ecapture_start_finished(&other, &stdout));
    }
}
