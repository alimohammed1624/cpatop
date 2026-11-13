//! Background pollers: one task per data group, each on its own cadence.
//!
//! Pollers watch a shared [`Query`]; a change of the relevant parameters
//! (range, API-key filter, role, manual refresh counter…) triggers an
//! immediate fetch, otherwise they wait for their interval. Results are sent to
//! the UI over a channel; the UI never awaits the network.

use crate::api::client::Query as Params;
use crate::api::models::*;
use crate::api::sse::{EventFilters, LiveEvent, events_query};
use crate::api::{ApiError, Client, Role};
use crate::auth::AuthEvent;
use crate::layout::Tab;
use crate::range::{RtWindow, TimeRange};
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

pub type R<T> = Result<T, ApiError>;

/// Data groups, each polled by its own task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    Overview,
    Realtime,
    Analysis,
    KeyOptions,
    Credentials,
    Quota,
    Models,
    Settings,
    /// Per-key usage for the Keys tab (range-scoped comparisons).
    KeyUsage,
    /// Model / source options of the request-log filters.
    EventFilters,
    Meta,
    Detail,
}

pub const POLLED: [Group; 11] = [
    Group::Overview,
    Group::Realtime,
    Group::Analysis,
    Group::KeyOptions,
    Group::Credentials,
    Group::Quota,
    Group::Models,
    Group::Settings,
    Group::KeyUsage,
    Group::EventFilters,
    Group::Meta,
];

/// Everything the pollers need to know about the UI.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub role: Option<Role>,
    pub epoch: u64,
    pub tab: Tab,
    pub range: TimeRange,
    pub window: RtWindow,
    pub api_key_id: Option<String>,
    pub refresh: Duration,
    pub slow: Duration,
    pub manual: u64,
    /// Bumped when the UI discards range-scoped data, forcing a refetch.
    pub nonce: u64,
}

impl Default for Query {
    fn default() -> Self {
        Query {
            role: None,
            epoch: 0,
            tab: Tab::Overview,
            range: TimeRange::Today,
            window: RtWindow::M15,
            api_key_id: None,
            refresh: Duration::from_secs(10),
            slow: Duration::from_secs(60),
            manual: 0,
            nonce: 0,
        }
    }
}

impl Query {
    fn admin(&self) -> bool {
        self.role == Some(Role::Admin)
    }

    /// Identifies the parameters a group's data depends on.
    pub fn scope(&self, g: Group) -> String {
        let role = self.role.map(|r| r.as_str()).unwrap_or("-");
        let key = if self.admin() {
            self.api_key_id.as_deref().unwrap_or("all")
        } else {
            "self"
        };
        match g {
            Group::Overview | Group::Analysis => {
                format!("{role}|{}|{key}|{}", self.range.key(), self.nonce)
            }
            Group::Realtime => format!("{role}|{}|{key}|{}", self.window.param(), self.nonce),
            Group::KeyUsage => format!("{role}|{}|{}", self.range.key(), self.nonce),
            _ => role.to_string(),
        }
    }

    pub fn relevant(&self, g: Group) -> bool {
        if self.role.is_none() {
            return false;
        }
        let admin = self.admin();
        match g {
            Group::Overview => self.tab == Tab::Overview,
            Group::Realtime => self.tab == Tab::Realtime,
            Group::Analysis => self.tab == Tab::Analysis,
            Group::KeyOptions => admin,
            // Everywhere credentials are named (usage tables, request log),
            // so accounts on several providers can be told apart.
            Group::Credentials => admin && !matches!(self.tab, Tab::Models | Tab::Settings),
            // The Credentials tab fills plans from cached quota.
            Group::Quota => admin && matches!(self.tab, Tab::Quota | Tab::Credentials),
            Group::Models => admin && self.tab == Tab::Models,
            Group::Settings | Group::KeyUsage => admin && self.tab == Tab::Settings,
            Group::EventFilters => admin && self.tab == Tab::Events,
            Group::Meta => true,
            Group::Detail => false,
        }
    }

    pub fn interval(&self, g: Group) -> Option<Duration> {
        match g {
            Group::Overview | Group::Realtime => Some(self.refresh),
            Group::Models | Group::Detail | Group::EventFilters => None,
            // Analysis ranges are long; the slow cadence keeps them current
            // without hammering the heavier endpoints.
            Group::Analysis
            | Group::KeyOptions
            | Group::Credentials
            | Group::Quota
            | Group::Settings
            | Group::KeyUsage => Some(self.slow),
            Group::Meta => Some(self.slow.max(Duration::from_secs(60))),
        }
    }

    fn range_params(&self) -> Params {
        let mut q: Params = self.range.params();
        if self.admin()
            && let Some(k) = &self.api_key_id
        {
            q.push(("api_key_id", k.clone()));
        }
        q
    }

    fn activity_params(&self) -> Params {
        let mut q: Params = match self.range {
            TimeRange::Today | TimeRange::Days(1) => vec![("window", "today".into())],
            TimeRange::Yesterday => vec![("window", "yesterday".into())],
            r => r.params(),
        };
        if self.admin()
            && let Some(k) = &self.api_key_id
        {
            q.push(("api_key_id", k.clone()));
        }
        q
    }
}

/// Payloads delivered to the UI.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Payload {
    Overview(R<Overview>),
    Activity(R<Activity>),
    Comparisons(R<Comparisons>),
    Realtime(R<Realtime>),
    Analysis(R<Analysis>),
    Latency(R<LatencyDiagnostics>),
    KeyOptions(R<ApiKeyOptions>),
    KeyUsage(R<Comparisons>),
    EventModels(R<EventModelFilters>),
    EventSources(R<EventSourceFilters>),
    Identities(R<IdentitiesPage>),
    Inspection(R<QuotaInspection>),
    QuotaTasks(Vec<(String, R<QuotaRefreshTask>)>),
    AutoRefresh(R<AutoRefreshSettings>),
    QuotaHistory(String, R<QuotaHistory>),
    IdentityErrors(String, R<IdentityErrors>),
    Catalog(R<ModelCatalog>),
    Pricing(R<PricingModels>),
    RecalcCurrent(R<Option<RecalcTask>>),
    RecalcOptions(R<RecalcOptions>),
    ApiKeys(R<ApiKeys>),
    Sessions(R<AuthSessions>),
    Status(R<ServerStatus>),
    Version(R<VersionInfo>),
    MoreEvents(R<EventsPage>),
}

/// Messages into the UI loop.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Msg {
    Data {
        group: Group,
        scope: String,
        payload: Payload,
    },
    Live(LiveEvent),
    Auth(AuthEvent),
    /// A newer cpatop release (its tag) was found.
    UpdateAvailable(String),
}

impl From<LiveEvent> for Msg {
    fn from(e: LiveEvent) -> Self {
        Msg::Live(e)
    }
}

/// On-demand fetches requested by the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    LoadMoreEvents {
        scope: String,
        range: TimeRange,
        api_key_id: Option<String>,
        filters: EventFilters,
        cursor: String,
    },
    IdentityErrors {
        id: String,
    },
    QuotaHistory {
        auth_index: String,
    },
}

/// Percent-encode a single path segment.
pub fn path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

async fn fetch_group(g: Group, c: &Client, q: &Query, tx: &mpsc::UnboundedSender<Msg>) {
    let scope = q.scope(g);
    let send = |payload: Payload| {
        let _ = tx.send(Msg::Data {
            group: g,
            scope: scope.clone(),
            payload,
        });
    };
    let admin = q.admin();
    let none: Params = vec![];
    match g {
        Group::Overview => {
            let rq = q.range_params();
            let aq = q.activity_params();
            let (ov, ac, cmp) = if admin {
                tokio::join!(
                    c.get::<Overview>("/usage/overview", &rq),
                    c.get::<Activity>("/usage/activity", &aq),
                    c.get::<Comparisons>("/usage/overview/comparisons", &rq),
                )
            } else {
                tokio::join!(
                    c.get::<Overview>("/key-overview", &rq),
                    c.get::<Activity>("/key-activity", &aq),
                    c.get::<Comparisons>("/key-overview/comparisons", &rq),
                )
            };
            send(Payload::Overview(ov));
            send(Payload::Activity(ac));
            send(Payload::Comparisons(cmp));
        }
        Group::Realtime => {
            let mut wq: Params = vec![("window", q.window.param().into())];
            if admin && let Some(k) = &q.api_key_id {
                wq.push(("api_key_id", k.clone()));
            }
            let path = if admin {
                "/usage/overview/realtime"
            } else {
                "/key-overview/realtime"
            };
            send(Payload::Realtime(c.get(path, &wq).await));
        }
        Group::Analysis => {
            let rq = q.range_params();
            let (core, lat) = if admin {
                ("/usage/analysis", "/usage/analysis/latency")
            } else {
                ("/key-analysis", "/key-analysis/latency")
            };
            let (a, l) = tokio::join!(
                c.get::<Analysis>(core, &rq),
                c.get::<LatencyDiagnostics>(lat, &rq)
            );
            send(Payload::Analysis(a));
            send(Payload::Latency(l));
        }
        Group::KeyOptions => {
            // The options carry only `{id,label}`; the key list adds alias and
            // the masked key so keys can be told apart everywhere.
            let (opts, keys) = tokio::join!(
                c.get::<ApiKeyOptions>("/usage/api-keys/options", &none),
                c.get::<ApiKeys>("/usage/api-keys", &none),
            );
            send(Payload::ApiKeys(keys));
            send(Payload::KeyOptions(opts));
        }
        Group::KeyUsage => {
            send(Payload::KeyUsage(
                c.get("/usage/overview/comparisons", &q.range.params())
                    .await,
            ));
        }
        Group::EventFilters => {
            let (m, s) = tokio::join!(
                c.get::<EventModelFilters>("/usage/events/filters/models", &none),
                c.get::<EventSourceFilters>("/usage/events/filters/sources", &none),
            );
            send(Payload::EventModels(m));
            send(Payload::EventSources(s));
        }
        Group::Credentials => {
            let pq: Params = vec![
                ("sort", "priority".into()),
                ("page", "1".into()),
                ("page_size", "500".into()),
            ];
            send(Payload::Identities(
                c.get("/usage/identities/page", &pq).await,
            ));
        }
        Group::Quota => {
            let (insp, auto, ids) = tokio::join!(
                c.get::<QuotaInspection>("/quota/inspection", &none),
                c.get::<AutoRefreshSettings>("/quota/auto-refresh/settings", &none),
                c.get::<Identities>("/usage/identities", &none),
            );
            send(Payload::Inspection(insp));
            send(Payload::AutoRefresh(auto));
            match ids {
                Ok(ids) => {
                    let idx: Vec<String> = ids
                        .identities
                        .iter()
                        .filter(|i| i.auth_type == 1 && !i.is_deleted && !i.identity.is_empty())
                        .map(|i| i.identity.clone())
                        .collect();
                    let futs = idx.iter().map(|a| {
                        let path = format!("/quota/refresh/{}", path_segment(a));
                        async move { c.get::<QuotaRefreshTask>(&path, &vec![]).await }
                    });
                    let results = futures_util::future::join_all(futs).await;
                    send(Payload::QuotaTasks(idx.into_iter().zip(results).collect()));
                }
                Err(e) => send(Payload::QuotaTasks(vec![(String::new(), Err(e))])),
            }
        }
        Group::Models => {
            let (cat, pr, cur, opt) = tokio::join!(
                c.get::<ModelCatalog>("/models", &none),
                c.get::<PricingModels>("/pricing/models", &none),
                c.get::<Option<RecalcTask>>("/pricing/recalculations/current", &none),
                c.get::<RecalcOptions>("/pricing/recalculations/options", &none),
            );
            send(Payload::Catalog(cat));
            send(Payload::Pricing(pr));
            send(Payload::RecalcCurrent(cur));
            send(Payload::RecalcOptions(opt));
        }
        Group::Settings => {
            send(Payload::Sessions(c.get("/auth/sessions", &none).await));
        }
        Group::Meta => {
            send(Payload::Version(c.get("/version", &none).await));
            if admin {
                send(Payload::Status(c.get("/status", &none).await));
            }
        }
        Group::Detail => {}
    }
}

/// Poll one group forever.
async fn poll_loop(
    g: Group,
    c: Client,
    mut rx: watch::Receiver<Query>,
    tx: mpsc::UnboundedSender<Msg>,
) {
    // (scope, fetched at, manual counter, epoch)
    let mut last: Option<(String, Instant, u64, u64)> = None;
    loop {
        let q = rx.borrow_and_update().clone();
        let now = Instant::now();
        let due = if q.relevant(g) {
            let scope = q.scope(g);
            match &last {
                Some((s, at, m, e)) if *s == scope && *m == q.manual && *e == q.epoch => {
                    q.interval(g).map(|iv| *at + iv)
                }
                _ => Some(now),
            }
        } else {
            None
        };
        match due {
            Some(d) if d <= now => {
                fetch_group(g, &c, &q, &tx).await;
                if tx.is_closed() {
                    return;
                }
                last = Some((q.scope(g), Instant::now(), q.manual, q.epoch));
            }
            Some(d) => {
                tokio::select! {
                    r = rx.changed() => if r.is_err() { return },
                    _ = tokio::time::sleep_until(d) => {}
                }
            }
            None => {
                if rx.changed().await.is_err() {
                    return;
                }
            }
        }
    }
}

/// Spawn every poller. They end when `rx`'s sender or `tx`'s receiver is
/// dropped; the handles allow stopping them at once (switching servers).
pub fn spawn_pollers(
    c: &Client,
    rx: &watch::Receiver<Query>,
    tx: &mpsc::UnboundedSender<Msg>,
) -> Vec<tokio::task::JoinHandle<()>> {
    POLLED
        .into_iter()
        .map(|g| tokio::spawn(poll_loop(g, c.clone(), rx.clone(), tx.clone())))
        .collect()
}

/// Serve on-demand fetches.
pub async fn run_commands(
    c: Client,
    mut rx: mpsc::UnboundedReceiver<Command>,
    tx: mpsc::UnboundedSender<Msg>,
) {
    while let Some(cmd) = rx.recv().await {
        let c = c.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let (scope, payload) = match cmd {
                Command::LoadMoreEvents {
                    scope,
                    range,
                    api_key_id,
                    filters,
                    cursor,
                } => {
                    let q = events_query(range, api_key_id.as_deref(), &filters, Some(&cursor));
                    (scope, Payload::MoreEvents(c.get("/usage/events", &q).await))
                }
                Command::IdentityErrors { id } => {
                    let path = format!("/usage/identities/{}/errors", path_segment(&id));
                    let q: Params = vec![("page_size", "50".into())];
                    let r = c.get(&path, &q).await;
                    (String::new(), Payload::IdentityErrors(id, r))
                }
                Command::QuotaHistory { auth_index } => {
                    let path = format!("/quota/history/{}", path_segment(&auth_index));
                    let r = c.get(&path, &vec![]).await;
                    (String::new(), Payload::QuotaHistory(auth_index, r))
                }
            };
            let _ = tx.send(Msg::Data {
                group: Group::Detail,
                scope,
                payload,
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_and_relevance() {
        let mut q = Query {
            role: Some(Role::Admin),
            ..Default::default()
        };
        assert_eq!(q.scope(Group::Overview), "admin|today|all|0");
        q.api_key_id = Some("3".into());
        assert_eq!(q.scope(Group::Analysis), "admin|today|3|0");
        assert!(q.relevant(Group::Overview));
        assert!(!q.relevant(Group::Analysis));
        q.role = Some(Role::KeyViewer);
        assert_eq!(q.scope(Group::Realtime), "api-key|15m|self|0");
        assert!(!q.relevant(Group::KeyOptions));
        q.role = None;
        assert!(!q.relevant(Group::Meta));
    }

    #[test]
    fn activity_uses_window_for_calendar_ranges() {
        let mut q = Query {
            role: Some(Role::KeyViewer),
            api_key_id: Some("9".into()),
            ..Default::default()
        };
        assert_eq!(q.activity_params(), vec![("window", "today".to_string())]);
        q.range = TimeRange::Days(7);
        assert_eq!(q.activity_params(), vec![("range", "7d".to_string())]);
        assert_eq!(q.range_params(), vec![("range", "7d".to_string())]);
    }

    #[test]
    fn analysis_refreshes_on_the_slow_cadence() {
        let q = Query::default();
        assert_eq!(q.interval(Group::Analysis), Some(q.slow));
        assert_eq!(q.interval(Group::Overview), Some(q.refresh));
        assert_eq!(q.interval(Group::Models), None);
    }

    #[test]
    fn encodes_segments() {
        assert_eq!(path_segment("6d78555c26de98aa"), "6d78555c26de98aa");
        assert_eq!(path_segment("a/b c"), "a%2Fb%20c");
    }
}
