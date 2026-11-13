//! Server-Sent Events client for `GET /usage/events/live`.
//!
//! Reconnects with exponential backoff and jitter, honours `Retry-After` on
//! 429, reconnects immediately after a `reset` frame, and aborts the stream
//! when nothing arrives for 45 seconds (heartbeats come every 15 s).

use super::client::{ApiError, Client, Query, error_from};
use super::models::{LiveAppend, LiveBounds, LiveCode, LiveHeartbeat, LiveSnapshot};
use crate::range::TimeRange;
use futures_util::StreamExt;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

const MAX_FRAME: usize = 1 << 20;
const IDLE_TIMEOUT: Duration = Duration::from_secs(45);

/// One parsed SSE frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub event: String,
    pub data: String,
}

/// Incremental SSE parser handling LF, CR and CRLF line endings.
#[derive(Debug, Default)]
pub struct SseParser {
    buf: Vec<u8>,
    event: Option<String>,
    data: Vec<String>,
    data_len: usize,
}

impl SseParser {
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Frame>, String> {
        self.buf.extend_from_slice(chunk);
        let mut frames = Vec::new();
        while let Some(i) = self.buf.iter().position(|&b| b == b'\n' || b == b'\r') {
            let consumed = if self.buf[i] == b'\r' {
                match self.buf.get(i + 1) {
                    Some(b'\n') => i + 2,
                    Some(_) => i + 1,
                    None => break, // wait: a '\n' may follow in the next chunk
                }
            } else {
                i + 1
            };
            let line = String::from_utf8_lossy(&self.buf[..i]).into_owned();
            self.buf.drain(..consumed);
            if line.is_empty() {
                if !self.data.is_empty() || self.event.is_some() {
                    frames.push(Frame {
                        event: self.event.take().unwrap_or_else(|| "message".into()),
                        data: self.data.join("\n"),
                    });
                }
                self.data.clear();
                self.data_len = 0;
                continue;
            }
            if line.starts_with(':') {
                continue;
            }
            let (field, value) = match line.split_once(':') {
                Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
                None => (line.as_str(), ""),
            };
            match field {
                "event" => self.event = Some(value.to_string()),
                "data" => {
                    self.data_len += value.len();
                    self.data.push(value.to_string());
                }
                _ => {}
            }
            if self.data_len > MAX_FRAME {
                return Err("event frame larger than 1 MiB".into());
            }
        }
        if self.buf.len() > MAX_FRAME {
            return Err("event line larger than 1 MiB".into());
        }
        Ok(frames)
    }
}

/// Result filter of the request log (`result=` on the server).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ResultFilter {
    #[default]
    All,
    Success,
    Failed,
}

impl ResultFilter {
    pub fn next(self) -> ResultFilter {
        match self {
            ResultFilter::All => ResultFilter::Success,
            ResultFilter::Success => ResultFilter::Failed,
            ResultFilter::Failed => ResultFilter::All,
        }
    }
    pub fn param(self) -> Option<&'static str> {
        match self {
            ResultFilter::All => None,
            ResultFilter::Success => Some("success"),
            ResultFilter::Failed => Some("failed"),
        }
    }
}

/// Server-side filters of the request log, sent on the live stream and on
/// history pages alike so both always describe the same result set.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct EventFilters {
    pub result: ResultFilter,
    /// Exact model name.
    pub model: Option<String>,
    /// Source identity value (`auth_index`) and its display label.
    pub source: Option<(String, String)>,
}

impl EventFilters {
    pub fn is_empty(&self) -> bool {
        self.result == ResultFilter::All && self.model.is_none() && self.source.is_none()
    }

    /// Stable key for scopes.
    pub fn key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.result.param().unwrap_or("all"),
            self.model.as_deref().unwrap_or(""),
            self.source.as_ref().map(|s| s.0.as_str()).unwrap_or("")
        )
    }
}

/// Query for `/usage/events` and `/usage/events/live` (cursor mode, 50 rows).
pub fn events_query(
    range: TimeRange,
    api_key_id: Option<&str>,
    filters: &EventFilters,
    cursor: Option<&str>,
) -> Query {
    let mut q: Query = range.params();
    q.push(("page_size", "50".into()));
    q.push(("cursor_mode", "true".into()));
    if let Some(c) = cursor {
        q.push(("cursor", c.to_string()));
    }
    if let Some(k) = api_key_id {
        q.push(("api_key_id", k.to_string()));
    }
    if let Some(r) = filters.result.param() {
        q.push(("result", r.into()));
    }
    if let Some(m) = &filters.model {
        q.push(("model", m.clone()));
    }
    if let Some((v, _)) = &filters.source {
        q.push(("source", v.clone()));
    }
    q
}

/// Parameters of the live stream; a change triggers a reconnect.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LiveParams {
    pub enabled: bool,
    pub range: Option<TimeRange>,
    pub api_key_id: Option<String>,
    pub filters: EventFilters,
    pub epoch: u64,
    /// Bumped on manual refresh to force a reconnect.
    pub nonce: u64,
}

/// Events forwarded to the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum LiveEvent {
    Stopped,
    Connecting,
    Snapshot(Box<LiveSnapshot>),
    Append(Box<LiveAppend>),
    Checkpoint(LiveBounds),
    Heartbeat(LiveHeartbeat),
    Busy,
    Reset(String),
    /// Recoverable failure; reconnecting after `retry_in`.
    Retrying {
        message: String,
        retry_in: Duration,
    },
    /// Non-recoverable for these parameters.
    Failed(String),
    /// `409`: a custom range drifted out of the server's bounds.
    RangeConflict(String),
    Unauthorized,
}

enum Outcome {
    Reset,
    Retry(String, Duration),
    Terminal(String),
    Conflict(String),
    Unauthorized,
}

/// Exponential backoff: `min(30 s, 1 s · 2^min(n,5))` with ±20 % jitter.
pub fn backoff(attempt: u32) -> Duration {
    let base = 1000u64 * (1u64 << attempt.min(5));
    let base = base.min(30_000) as f64;
    let jitter = 0.8 + fastrand::f64() * 0.4;
    Duration::from_millis((base * jitter) as u64)
}

async fn stream_once<F: FnMut(LiveEvent)>(
    client: &Client,
    p: &LiveParams,
    attempt: &mut u32,
    emit: &mut F,
) -> Outcome {
    let range = p.range.unwrap_or(TimeRange::Today);
    let q = events_query(range, p.api_key_id.as_deref(), &p.filters, None);
    let resp = match client
        .get_builder("/usage/events/live", &q)
        .header(reqwest::header::ACCEPT, "text/event-stream")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let d = backoff(*attempt);
            *attempt += 1;
            let msg = if e.is_connect() {
                "connection failed"
            } else {
                "request failed"
            };
            return Outcome::Retry(msg.into(), d);
        }
    };
    let status = resp.status();
    if !status.is_success() {
        let headers = resp.headers().clone();
        let body = resp.bytes().await.unwrap_or_default();
        return match error_from(status, &headers, &body) {
            ApiError::Unauthorized(_) | ApiError::Forbidden(_) => Outcome::Unauthorized,
            ApiError::RateLimited {
                retry_after,
                message,
            } => Outcome::Retry(
                format!("rate limited ({message})"),
                Duration::from_secs(retry_after.unwrap_or(5).max(1)),
            ),
            ApiError::Http {
                status: 409,
                message,
                ..
            } => Outcome::Conflict(message),
            ApiError::Http {
                status: s @ (400 | 404 | 405 | 501),
                message,
                ..
            } => Outcome::Terminal(format!("{s} {message}")),
            other => {
                let d = backoff(*attempt);
                *attempt += 1;
                Outcome::Retry(other.to_string(), d)
            }
        };
    }
    let is_sse = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/event-stream"));
    if !is_sse {
        return Outcome::Terminal("server did not return an event stream".into());
    }
    let mut stream = resp.bytes_stream();
    let mut parser = SseParser::default();
    loop {
        let next = tokio::time::timeout(IDLE_TIMEOUT, stream.next()).await;
        let chunk = match next {
            Err(_) => {
                let d = backoff(*attempt);
                *attempt += 1;
                return Outcome::Retry("stream idle for 45s".into(), d);
            }
            Ok(None) => {
                let d = backoff(*attempt);
                *attempt += 1;
                return Outcome::Retry("stream closed by server".into(), d);
            }
            Ok(Some(Err(_))) => {
                let d = backoff(*attempt);
                *attempt += 1;
                return Outcome::Retry("stream interrupted".into(), d);
            }
            Ok(Some(Ok(b))) => b,
        };
        let frames = match parser.push(&chunk) {
            Ok(f) => f,
            Err(e) => {
                let d = backoff(*attempt);
                *attempt += 1;
                return Outcome::Retry(e, d);
            }
        };
        for f in frames {
            match f.event.as_str() {
                "snapshot" => {
                    if let Ok(s) = serde_json::from_str::<LiveSnapshot>(&f.data) {
                        *attempt = 0;
                        emit(LiveEvent::Snapshot(Box::new(s)));
                    }
                }
                "append" => {
                    if let Ok(a) = serde_json::from_str::<LiveAppend>(&f.data) {
                        *attempt = 0;
                        emit(LiveEvent::Append(Box::new(a)));
                    }
                }
                "checkpoint" => {
                    if let Ok(b) = serde_json::from_str::<LiveBounds>(&f.data) {
                        emit(LiveEvent::Checkpoint(b));
                    }
                }
                "heartbeat" => {
                    if let Ok(h) = serde_json::from_str::<LiveHeartbeat>(&f.data) {
                        emit(LiveEvent::Heartbeat(h));
                    }
                }
                "busy" => emit(LiveEvent::Busy),
                "reset" => {
                    let code = serde_json::from_str::<LiveCode>(&f.data)
                        .map(|c| c.code)
                        .unwrap_or_default();
                    emit(LiveEvent::Reset(code));
                    return Outcome::Reset;
                }
                _ => {}
            }
        }
    }
}

/// Run the live stream until the receiver side of `tx` is dropped.
pub async fn run_live<T: From<LiveEvent> + Send + 'static>(
    client: Client,
    mut params: watch::Receiver<LiveParams>,
    tx: mpsc::UnboundedSender<T>,
) {
    let mut attempt = 0u32;
    let send = |e: LiveEvent| tx.send(T::from(e)).is_ok();
    loop {
        let p = params.borrow_and_update().clone();
        if !p.enabled {
            if !send(LiveEvent::Stopped) || params.changed().await.is_err() {
                return;
            }
            continue;
        }
        if !send(LiveEvent::Connecting) {
            return;
        }
        let mut emit = |e: LiveEvent| {
            let _ = tx.send(T::from(e));
        };
        let outcome = tokio::select! {
            o = stream_once(&client, &p, &mut attempt, &mut emit) => o,
            r = params.changed() => {
                if r.is_err() { return; }
                attempt = 0;
                continue;
            }
        };
        match outcome {
            Outcome::Reset => continue,
            Outcome::Retry(message, retry_in) => {
                if !send(LiveEvent::Retrying { message, retry_in }) {
                    return;
                }
                tokio::select! {
                    _ = tokio::time::sleep(retry_in) => {}
                    r = params.changed() => { if r.is_err() { return; } attempt = 0; }
                }
            }
            Outcome::Terminal(m) => {
                if !send(LiveEvent::Failed(m)) || params.changed().await.is_err() {
                    return;
                }
                attempt = 0;
            }
            Outcome::Conflict(m) => {
                if !send(LiveEvent::RangeConflict(m)) || params.changed().await.is_err() {
                    return;
                }
                attempt = 0;
            }
            Outcome::Unauthorized => {
                if !send(LiveEvent::Unauthorized) || params.changed().await.is_err() {
                    return;
                }
                attempt = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frames_across_chunks_and_line_endings() {
        let mut p = SseParser::default();
        let mut out = p.push(b"event: snapshot\ndata: {\"a\":").unwrap();
        assert!(out.is_empty());
        out = p
            .push(b"1}\n\n: comment\r\nevent: heartbeat\r\ndata: {}\r\n\r")
            .unwrap();
        assert_eq!(
            out,
            vec![Frame {
                event: "snapshot".into(),
                data: "{\"a\":1}".into()
            }]
        );
        out = p.push(b"\ndata: x\rdata: y\r\r").unwrap();
        // The final CR may be the first half of a CRLF, so it waits for more input.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].event, "heartbeat");
        out = p.push(b"\n").unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].event, "message");
        assert_eq!(out[0].data, "x\ny");
    }

    #[test]
    fn events_query_carries_every_filter() {
        let f = EventFilters {
            result: ResultFilter::Failed,
            model: Some("gpt-6-luna".into()),
            source: Some(("6d78555c26de98aa".into(), "a@b".into())),
        };
        let q = events_query(TimeRange::Hours(8), Some("3"), &f, Some("abc"));
        let has = |k: &str, v: &str| q.iter().any(|(a, b)| *a == k && b == v);
        assert!(has("range", "8h"));
        assert!(has("page_size", "50"));
        assert!(has("cursor_mode", "true"));
        assert!(has("cursor", "abc"));
        assert!(has("api_key_id", "3"));
        assert!(has("result", "failed"));
        assert!(has("model", "gpt-6-luna"));
        assert!(has("source", "6d78555c26de98aa"));
        let q = events_query(TimeRange::Today, None, &EventFilters::default(), None);
        assert_eq!(q.len(), 3);
    }

    #[test]
    fn rejects_huge_frames() {
        let mut p = SseParser::default();
        let big = vec![b'a'; MAX_FRAME + 10];
        let mut chunk = b"data: ".to_vec();
        chunk.extend_from_slice(&big);
        chunk.push(b'\n');
        assert!(p.push(&chunk).is_err());
    }

    #[test]
    fn backoff_is_capped() {
        for n in 0..20 {
            let d = backoff(n);
            assert!(d <= Duration::from_millis(36_000), "{d:?}");
            assert!(d >= Duration::from_millis(800));
        }
    }

    #[test]
    fn parses_live_fixture() {
        let text = include_str!("../../tests/fixtures/usage-events-live.txt");
        let mut p = SseParser::default();
        let frames = p.push(text.as_bytes()).unwrap();
        let snap = frames.iter().find(|f| f.event == "snapshot").unwrap();
        let s: LiveSnapshot = serde_json::from_str(&snap.data).unwrap();
        assert!(!s.events.is_empty());
        assert!(frames.iter().any(|f| f.event == "heartbeat"));
    }
}
