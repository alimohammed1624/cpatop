//! State of the live request log (SSE first page plus cursor history).

use crate::api::models::{ErrorEvent, UsageEvent};
pub use crate::api::sse::{EventFilters, ResultFilter};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq)]
pub enum LiveStatus {
    Idle,
    Connecting,
    Live,
    Busy,
    Retrying { message: String, until: Instant },
    Failed(String),
}

/// State of the live request log.
#[derive(Debug)]
pub struct LiveState {
    pub status: LiveStatus,
    pub events: Vec<UsageEvent>,
    /// Server-side total for the current range and filters.
    pub total: i64,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub loading_more: bool,
    pub range_start: String,
    pub appended: u64,
    pub last_frame: Option<Instant>,
    /// Server-side filters (sent on the stream and on history pages).
    pub filters: EventFilters,
    /// The request selected in the last frame (its credential's error log
    /// is fetched when it failed; panes are sized before the selection of
    /// the frame being drawn is known).
    pub selected: Option<String>,
}

impl Default for LiveState {
    fn default() -> Self {
        LiveState {
            status: LiveStatus::Idle,
            events: Vec::new(),
            total: 0,
            next_cursor: None,
            has_more: false,
            loading_more: false,
            range_start: String::new(),
            appended: 0,
            last_frame: None,
            filters: EventFilters::default(),
            selected: None,
        }
    }
}

/// How far an error-log entry may be from the end of a failed request
/// (start plus latency) to be taken as its error. The error is logged when
/// the request ends; both times are cut to the second in places.
const ERROR_MATCH_SECS: f64 = 3.0;

/// The credential error-log entry behind a failed request. Usage events
/// carry no error text (the server keeps only the status), so the entry is
/// found in the credential's error log: the same model, logged when the
/// request ended. The status may differ (a client that hung up is `499` in
/// the request log and `500 context canceled` in the error log), so an
/// equal status only breaks ties.
pub fn match_error<'a>(e: &UsageEvent, errors: &'a [ErrorEvent]) -> Option<&'a ErrorEvent> {
    if !e.failed {
        return None;
    }
    let start = crate::fmt::parse_ts(&e.timestamp)?;
    let end = start.timestamp_millis() as f64 / 1000.0 + e.latency_ms.max(0.0) / 1000.0;
    let begin = start.timestamp_millis() as f64 / 1000.0;
    errors
        .iter()
        .filter(|x| {
            let m = x.model.as_deref().unwrap_or("");
            m.is_empty() || e.model.is_empty() || m == e.model
        })
        .filter_map(|x| {
            let at = crate::fmt::parse_ts(&x.timestamp)?.timestamp_millis() as f64 / 1000.0;
            if at < begin - 1.0 || at > end + ERROR_MATCH_SECS {
                return None;
            }
            let off = (at - end).abs();
            (off <= ERROR_MATCH_SECS || at <= end).then_some((off, x))
        })
        .min_by(|a, b| {
            let same = |x: &ErrorEvent| Some(x.status_code) != e.status_code;
            a.0.partial_cmp(&b.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| same(a.1).cmp(&same(b.1)))
        })
        .map(|(_, x)| x)
}

/// Upper bound of events kept in memory.
pub const MAX_EVENTS: usize = 5000;

fn cmp_id(a: &str, b: &str) -> std::cmp::Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

impl LiveState {
    /// A fresh log that keeps the user's filters.
    pub fn reset(&mut self) {
        let filters = std::mem::take(&mut self.filters);
        *self = LiveState {
            filters,
            ..Default::default()
        };
    }

    pub fn sort(&mut self) {
        self.events.sort_by(|a, b| {
            b.timestamp
                .cmp(&a.timestamp)
                .then_with(|| cmp_id(&b.id, &a.id))
        });
    }

    /// Merge events by id; returns how many were new.
    pub fn merge(&mut self, new: Vec<UsageEvent>) -> usize {
        let mut added = 0;
        for e in new {
            if let Some(slot) = self
                .events
                .iter_mut()
                .find(|x| x.id == e.id && !e.id.is_empty())
            {
                *slot = e;
            } else {
                self.events.push(e);
                added += 1;
            }
        }
        self.sort();
        self.events.truncate(MAX_EVENTS);
        added
    }

    /// Drop events that fell out of a rolling window.
    pub fn prune(&mut self) {
        if !self.range_start.is_empty() {
            let start = self.range_start.clone();
            self.events
                .retain(|e| e.timestamp.as_str() >= start.as_str());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(ts: &str, model: &str, status: i64, body: &str) -> ErrorEvent {
        ErrorEvent {
            timestamp: ts.into(),
            model: Some(model.into()),
            status_code: status,
            body_summary: body.into(),
            ..Default::default()
        }
    }

    #[test]
    fn failed_requests_find_their_logged_error() {
        let errors = vec![
            err(
                "2026-11-13T20:24:32.1+05:30",
                "claude-fable-5-2",
                500,
                "fable",
            ),
            err(
                "2026-11-13T20:24:31.7+05:30",
                "claude-opus-5-5",
                500,
                "opus late",
            ),
            err(
                "2026-11-13T20:24:31.2+05:30",
                "claude-opus-5-5",
                500,
                "opus",
            ),
            err(
                "2026-11-13T18:56:32.5+05:30",
                "claude-opus-5-5",
                503,
                "validation",
            ),
        ];
        let ev = |ts: &str, model: &str, status: i64, ms: f64| UsageEvent {
            timestamp: ts.into(),
            model: model.into(),
            status_code: Some(status),
            latency_ms: ms,
            failed: true,
            ..Default::default()
        };
        // Ends 20:24:31.65: the opus entry logged then (499 vs 500).
        let e = ev("2026-11-13T20:24:29+05:30", "claude-opus-5-5", 499, 2650.0);
        assert_eq!(match_error(&e, &errors).unwrap().body_summary, "opus late");
        let e = ev("2026-11-13T20:24:24+05:30", "claude-fable-5-2", 499, 7220.0);
        assert_eq!(match_error(&e, &errors).unwrap().body_summary, "fable");
        let e = ev("2026-11-13T18:56:31+05:30", "claude-opus-5-5", 503, 1560.0);
        assert_eq!(match_error(&e, &errors).unwrap().body_summary, "validation");
        // Nothing logged near it, or a different model: no match.
        let e = ev("2026-11-13T17:00:00+05:30", "claude-opus-5-5", 503, 500.0);
        assert!(match_error(&e, &errors).is_none());
        let e = ev("2026-11-13T18:56:31+05:30", "gpt-6.1-sol", 503, 1560.0);
        assert!(match_error(&e, &errors).is_none());
        // Successful requests have none.
        let mut e = ev("2026-11-13T18:56:31+05:30", "claude-opus-5-5", 503, 1560.0);
        e.failed = false;
        assert!(match_error(&e, &errors).is_none());
    }
}
