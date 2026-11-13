//! State of the live request log (SSE first page plus cursor history).

use crate::api::models::UsageEvent;
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
        }
    }
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
