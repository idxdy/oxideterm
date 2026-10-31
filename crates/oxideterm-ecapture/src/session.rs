// Copyright (C) 2026 jaime.su
// SPDX-License-Identifier: GPL-3.0-only

//! Per-connection capture cursor and bounded event ring.
//!
//! Polls advance the remote line cursor by exactly the newline-terminated lines
//! they delivered, while an unfinished record stays buffered locally. That
//! keeps a poll from splitting a record at a chunk boundary and keeps the
//! remote read window free of duplicates.

use std::collections::VecDeque;

use crate::model::{
    ECAPTURE_MAX_EVENT_BYTES, ECAPTURE_MAX_EVENTS, ECAPTURE_MAX_LOG_LINES,
    ECAPTURE_MAX_PENDING_BYTES, EcaptureEvent, EcaptureFilter, EcaptureKind,
};
use crate::parse::{EcapturePoll, is_record_header, parse_event};

pub struct EcaptureSession {
    run_id: String,
    /// Next 1-based line of `events.log` the remote poll must return.
    next_line: u64,
    total_lines: u64,
    file_bytes: u64,
    running: bool,
    absent: bool,
    pending: VecDeque<String>,
    pending_bytes: usize,
    events: VecDeque<EcaptureEvent>,
    event_bytes: usize,
    runtime_log: VecDeque<String>,
    dropped_events: u64,
    skipped_lines: u64,
    sequence: u64,
}

impl EcaptureSession {
    pub fn new(run_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            next_line: 1,
            total_lines: 0,
            file_bytes: 0,
            running: false,
            absent: false,
            pending: VecDeque::new(),
            pending_bytes: 0,
            events: VecDeque::new(),
            event_bytes: 0,
            runtime_log: VecDeque::new(),
            dropped_events: 0,
            skipped_lines: 0,
            sequence: 0,
        }
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn next_line(&self) -> u64 {
        self.next_line
    }

    pub fn file_bytes(&self) -> u64 {
        self.file_bytes
    }

    pub fn running(&self) -> bool {
        self.running
    }

    /// No run directory exists for this host, so no capture was started here.
    pub fn absent(&self) -> bool {
        self.absent
    }

    pub fn dropped_events(&self) -> u64 {
        self.dropped_events
    }

    /// Lines the SSH capture budget forced the session to skip.
    pub fn skipped_lines(&self) -> u64 {
        self.skipped_lines
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// More complete lines are waiting than the last poll fetched.
    pub fn has_backlog(&self) -> bool {
        !self.absent && self.next_line <= self.total_lines
    }

    pub fn events(&self) -> &VecDeque<EcaptureEvent> {
        &self.events
    }

    pub fn event(&self, sequence: u64) -> Option<&EcaptureEvent> {
        let front = self.events.front()?.sequence;
        let index = usize::try_from(sequence.checked_sub(front)?).ok()?;
        self.events
            .get(index)
            .filter(|event| event.sequence == sequence)
    }

    pub fn runtime_log(&self) -> &VecDeque<String> {
        &self.runtime_log
    }

    pub fn apply_poll(&mut self, poll: EcapturePoll) {
        if poll.absent {
            // The run directory is the source of truth: a missing directory
            // means the capture was cleared from another client.
            self.running = false;
            self.absent = true;
            self.total_lines = 0;
            self.file_bytes = 0;
            self.pending.clear();
            self.pending_bytes = 0;
            return;
        }

        self.absent = false;
        self.running = poll.running;
        self.total_lines = poll.total_lines;
        self.file_bytes = poll.file_bytes;

        if poll.start_line != self.next_line {
            // A new start wiped the file or another client moved the cursor.
            // Mixing the two streams would splice unrelated records.
            self.pending.clear();
            self.pending_bytes = 0;
            self.next_line = poll.start_line.max(1);
        }

        for line in &poll.log_lines {
            if line.is_empty() {
                continue;
            }
            self.runtime_log.push_back(line.clone());
            while self.runtime_log.len() > ECAPTURE_MAX_LOG_LINES {
                self.runtime_log.pop_front();
            }
        }

        let accepted = poll.complete_line_count();
        if accepted == 0 {
            if poll.truncated && !poll.events_text.is_empty() {
                // One record line alone exceeded the capture budget. Skipping
                // it is the only way to make progress; the UI reports the loss.
                self.skipped_lines += 1;
                self.next_line = poll.start_line.max(1) + 1;
            }
        } else {
            for line in poll.events_text.split_terminator('\n') {
                let line = line.trim_end_matches('\r').to_string();
                self.pending_bytes += line.len() + 1;
                self.pending.push_back(line);
            }
            self.next_line = poll.start_line.max(1) + accepted as u64;
        }

        self.drain_records();
        self.trim_pending();
        self.trim_events();

        if !self.running && !self.has_backlog() {
            // The process is gone and every complete line was fetched, so the
            // buffered record has no continuation to wait for.
            self.flush_pending();
        }
    }

    pub fn visible_sequences(&self, query: &str, filter: EcaptureFilter) -> Vec<u64> {
        let query = query.trim().to_lowercase();
        self.events
            .iter()
            .rev()
            .filter(|event| event_matches_filter(event, filter))
            .filter(|event| query.is_empty() || event_matches_query(event, &query))
            .map(|event| event.sequence)
            .collect()
    }

    fn drain_records(&mut self) {
        loop {
            while self
                .pending
                .front()
                .is_some_and(|line| !is_record_header(line))
            {
                self.pop_front_line();
            }
            if self.pending.front().is_none() {
                break;
            }
            let boundary = self
                .pending
                .iter()
                .enumerate()
                .skip(1)
                .find(|(_, line)| is_record_header(line))
                .map(|(index, _)| index);
            let Some(boundary) = boundary else {
                break;
            };
            let record: Vec<String> = self.pending.drain(..boundary).collect();
            self.pending_bytes = self
                .pending_bytes
                .saturating_sub(record.iter().map(|line| line.len() + 1).sum::<usize>());
            self.push_record(&record);
        }
    }

    fn flush_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        if self
            .pending
            .front()
            .is_some_and(|line| is_record_header(line))
        {
            let record: Vec<String> = self.pending.drain(..).collect();
            self.pending_bytes = 0;
            self.push_record(&record);
        } else {
            self.pending.clear();
            self.pending_bytes = 0;
        }
    }

    fn push_record(&mut self, record: &[String]) {
        let sequence = self.sequence + 1;
        let Some(event) = parse_event(sequence, record) else {
            return;
        };
        self.sequence = sequence;
        self.event_bytes += event.payload.len();
        self.events.push_back(event);
    }

    fn pop_front_line(&mut self) {
        if let Some(line) = self.pending.pop_front() {
            self.pending_bytes = self.pending_bytes.saturating_sub(line.len() + 1);
        }
    }

    fn trim_pending(&mut self) {
        // A well-formed stream keeps at most one unfinished record here. More
        // than the cap means the tail is too large to render; dropping it is
        // bounded and visible through `skipped_lines`.
        if self.pending_bytes <= ECAPTURE_MAX_PENDING_BYTES {
            return;
        }
        self.pending.clear();
        self.pending_bytes = 0;
        self.skipped_lines += 1;
    }

    fn trim_events(&mut self) {
        while self.events.len() > ECAPTURE_MAX_EVENTS || self.event_bytes > ECAPTURE_MAX_EVENT_BYTES
        {
            let Some(event) = self.events.pop_front() else {
                break;
            };
            self.event_bytes = self.event_bytes.saturating_sub(event.payload.len());
            self.dropped_events += 1;
        }
    }
}

fn event_matches_filter(event: &EcaptureEvent, filter: EcaptureFilter) -> bool {
    match filter {
        EcaptureFilter::All => true,
        EcaptureFilter::Requests => event.kind == EcaptureKind::Request,
        EcaptureFilter::Responses => event.kind == EcaptureKind::Response,
    }
}

fn event_matches_query(event: &EcaptureEvent, query: &str) -> bool {
    event.summary.to_lowercase().contains(query)
        || event.comm.to_lowercase().contains(query)
        || event.tuple.to_lowercase().contains(query)
        || event.pid.to_string().contains(query)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{ecapture_run_id, end_marker, events_marker, log_marker, meta_marker};
    use crate::parse::parse_ecapture_poll;

    fn poll(run_id: &str, running: bool, start: u64, total: u64, events: &str) -> EcapturePoll {
        let source = format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n{}\n{}{}\n{}\n",
            meta_marker(run_id),
            u8::from(running),
            total,
            0,
            start,
            start,
            events_marker(run_id),
            events,
            log_marker(run_id),
            end_marker(run_id),
        );
        parse_ecapture_poll(run_id, &source, false)
    }

    fn openssl_header(pid: u32, direction: &str, length: usize) -> String {
        format!(
            "[2026-10-29 10:11:12.345] PID:{pid} TID:43 Comm:curl FD:5 Tuple:10.0.0.2:443->10.0.0.1:51000 {direction} ({length} bytes):"
        )
    }

    #[test]
    fn session_keeps_unfinished_records_until_the_next_header() {
        let run_id = ecapture_run_id("connection-a");
        let mut session = EcaptureSession::new(&run_id);

        let first = format!("{}\nGET /one HTTP/1.1\n", openssl_header(1, "WRITE", 18));
        session.apply_poll(poll(&run_id, true, 1, 2, &first));
        // The record has no following header yet, so it stays buffered.
        assert_eq!(session.event_count(), 0);
        assert_eq!(session.next_line(), 3);

        let second = format!("{}\nHTTP/1.1 200 OK\n", openssl_header(1, "READ", 17));
        session.apply_poll(poll(&run_id, true, 3, 4, &second));
        assert_eq!(session.event_count(), 1);
        let event = session.event(1).expect("first event");
        assert_eq!(event.summary, "GET /one HTTP/1.1");
        assert_eq!(event.payload, "GET /one HTTP/1.1");
        assert_eq!(session.next_line(), 5);
    }

    #[test]
    fn session_flushes_the_last_record_when_the_capture_stops() {
        let run_id = ecapture_run_id("connection-a");
        let mut session = EcaptureSession::new(&run_id);
        let text = format!("{}\nGET /final HTTP/1.1\n", openssl_header(7, "WRITE", 18));
        session.apply_poll(poll(&run_id, true, 1, 2, &text));
        assert_eq!(session.event_count(), 0);

        session.apply_poll(poll(&run_id, false, 3, 2, ""));
        assert!(!session.running());
        assert_eq!(session.event_count(), 1);
        assert_eq!(
            session.event(1).map(|event| event.summary.as_str()),
            Some("GET /final HTTP/1.1")
        );
    }

    #[test]
    fn session_skips_an_unreadable_line_and_reports_it() {
        let run_id = ecapture_run_id("connection-a");
        let mut session = EcaptureSession::new(&run_id);
        let mut truncated = poll(&run_id, true, 5, 40, "");
        truncated.truncated = true;
        truncated.events_text = "x".repeat(4096);
        session.apply_poll(truncated);
        assert_eq!(session.skipped_lines(), 1);
        assert_eq!(session.next_line(), 6);
    }

    #[test]
    fn session_resets_when_the_remote_cursor_restarts() {
        let run_id = ecapture_run_id("connection-a");
        let mut session = EcaptureSession::new(&run_id);
        let old = format!("{}\nGET /old HTTP/1.1\n", openssl_header(1, "WRITE", 18));
        let restart = format!("{}\nGET /new HTTP/1.1\n", openssl_header(2, "WRITE", 18));
        session.apply_poll(poll(&run_id, true, 1, 2, &old));
        session.apply_poll(poll(&run_id, true, 1, 2, &restart));
        assert_eq!(session.event_count(), 0);
        session.apply_poll(poll(&run_id, false, 3, 2, ""));
        assert_eq!(
            session.event(1).map(|event| event.summary.as_str()),
            Some("GET /new HTTP/1.1")
        );
    }

    #[test]
    fn session_filters_requests_responses_and_queries() {
        let run_id = ecapture_run_id("connection-a");
        let mut session = EcaptureSession::new(&run_id);
        let mut text = String::new();
        for (pid, direction, line) in [
            (1, "WRITE", "GET /item HTTP/1.1"),
            (1, "READ", "HTTP/1.1 200 OK"),
            (2, "WRITE", "POST /upload HTTP/1.1"),
        ] {
            text.push_str(&openssl_header(pid, direction, 20));
            text.push('\n');
            text.push_str(line);
            text.push('\n');
        }
        // A trailing header makes the last payload record complete.
        text.push_str(&openssl_header(3, "READ", 0));
        text.push('\n');
        session.apply_poll(poll(&run_id, true, 1, 8, &text));

        assert_eq!(session.event_count(), 3);
        assert_eq!(
            session
                .visible_sequences("", EcaptureFilter::Requests)
                .len(),
            2
        );
        assert_eq!(
            session
                .visible_sequences("", EcaptureFilter::Responses)
                .len(),
            1
        );
        let upload = session.visible_sequences("upload", EcaptureFilter::All);
        assert_eq!(upload.len(), 1);
        assert_eq!(session.event(upload[0]).map(|event| event.pid), Some(2));
        assert_eq!(
            session
                .visible_sequences("10.0.0.2", EcaptureFilter::All)
                .len(),
            3
        );
    }

    #[test]
    fn session_bounds_the_event_ring() {
        let run_id = ecapture_run_id("connection-a");
        let mut session = EcaptureSession::new(&run_id);
        let total = ECAPTURE_MAX_EVENTS + 5;
        let mut text = String::new();
        for index in 0..total {
            text.push_str(&openssl_header(index as u32 + 1, "WRITE", 20));
            text.push('\n');
            text.push_str("GET /item HTTP/1.1\n");
        }
        // Leave the final record open so exactly `total` records are complete.
        text.push_str(&openssl_header(u32::MAX, "READ", 0));
        text.push('\n');
        session.apply_poll(poll(&run_id, true, 1, total as u64 * 2 + 1, &text));

        assert_eq!(session.event_count(), ECAPTURE_MAX_EVENTS);
        assert_eq!(session.dropped_events(), 5);
        assert!(session.event(1).is_none());
        assert!(session.event(6).is_some());
    }

    #[test]
    fn session_reports_an_absent_directory() {
        let run_id = ecapture_run_id("connection-a");
        let mut session = EcaptureSession::new(&run_id);
        let absent = format!(
            "__OXIDE_{}_ABSENT__\n",
            run_id.to_ascii_uppercase().replace('-', "_")
        );
        session.apply_poll(parse_ecapture_poll(&run_id, &absent, false));
        assert!(session.absent());
        assert!(!session.running());
        assert!(!session.has_backlog());
    }
}
