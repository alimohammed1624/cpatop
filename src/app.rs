//! Application state, message handling and key/mouse bindings.

use crate::api::models::*;
use crate::api::sse::{LiveEvent, LiveParams};
use crate::api::{ApiError, LoginMode, Role};
use crate::auth::{AuthCmd, AuthEvent};
use crate::config::{GraphSymbols, Settings, UiState, step_refresh};
use crate::layout::{self, PaneId, Tab};
use crate::poller::{Command, Group, Msg, Payload, Query};
use crate::range::{RtWindow, TimeRange};
use crate::secrets::Secret;
use crate::theme::Theme;
use chrono::{DateTime, Utc};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use std::cell::Cell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A piece of data plus its freshness.
#[derive(Debug)]
pub struct Loaded<T> {
    pub data: Option<T>,
    pub error: Option<String>,
    pub at: Option<Instant>,
}

impl<T> Default for Loaded<T> {
    fn default() -> Self {
        Loaded {
            data: None,
            error: None,
            at: None,
        }
    }
}

impl<T> Loaded<T> {
    fn apply(&mut self, r: Result<T, ApiError>, now: Instant) -> Option<ApiError> {
        match r {
            Ok(v) => {
                self.data = Some(v);
                self.error = None;
                self.at = Some(now);
                None
            }
            Err(ApiError::CostsBusy) => {
                self.error = Some("cost statistics are being updated".into());
                None
            }
            Err(e) => {
                self.error = Some(e.to_string());
                Some(e)
            }
        }
    }
    pub fn get(&self) -> Option<&T> {
        self.data.as_ref()
    }
}

/// All dashboard data.
#[derive(Debug, Default)]
pub struct Data {
    pub overview: Loaded<Overview>,
    pub activity: Loaded<Activity>,
    pub comparisons: Loaded<Comparisons>,
    pub realtime: Loaded<Realtime>,
    pub analysis: Loaded<Analysis>,
    pub latency: Loaded<LatencyDiagnostics>,
    pub key_options: Loaded<ApiKeyOptions>,
    pub identities: Loaded<IdentitiesPage>,
    pub inspection: Loaded<QuotaInspection>,
    pub auto_refresh: Loaded<AutoRefreshSettings>,
    pub quota: Loaded<HashMap<String, Result<QuotaRefreshTask, String>>>,
    pub quota_history: Option<(String, Loaded<QuotaHistory>)>,
    pub identity_errors: Option<(String, Loaded<IdentityErrors>)>,
    pub catalog: Loaded<ModelCatalog>,
    pub pricing: Loaded<PricingModels>,
    pub recalc: Loaded<Option<RecalcTask>>,
    pub recalc_options: Loaded<RecalcOptions>,
    pub api_keys: Loaded<ApiKeys>,
    pub sessions: Loaded<AuthSessions>,
    pub status: Loaded<ServerStatus>,
    pub version: Loaded<VersionInfo>,
}

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
    pub total: i64,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub loading_more: bool,
    pub range_start: String,
    pub appended: u64,
    pub last_frame: Option<Instant>,
    pub result_filter: ResultFilter,
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
            result_filter: ResultFilter::All,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultFilter {
    All,
    Success,
    Failed,
}

const MAX_EVENTS: usize = 2000;

fn cmp_id(a: &str, b: &str) -> std::cmp::Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

impl LiveState {
    fn sort(&mut self) {
        self.events.sort_by(|a, b| {
            b.timestamp
                .cmp(&a.timestamp)
                .then_with(|| cmp_id(&b.id, &a.id))
        });
    }

    fn merge(&mut self, new: Vec<UsageEvent>) -> usize {
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

    fn prune(&mut self) {
        if !self.range_start.is_empty() {
            let start = self.range_start.clone();
            self.events
                .retain(|e| e.timestamp.as_str() >= start.as_str());
        }
    }
}

/// Per-table UI state.
#[derive(Debug, Default)]
pub struct TableState {
    pub selected: usize,
    pub offset: Cell<usize>,
    /// Sort column index and descending flag.
    pub sort: Option<(usize, bool)>,
    pub filter: String,
    /// Rows after filtering, set during render.
    pub len: Cell<usize>,
    /// Visible body rows, set during render.
    pub page: Cell<usize>,
    /// Number of sortable columns, set during render.
    pub cols: Cell<usize>,
}

/// The login form. The typed secret is never rendered or logged.
pub struct LoginForm {
    pub mode: LoginMode,
    pub input: String,
    pub error: Option<String>,
}

impl std::fmt::Debug for LoginForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginForm")
            .field("mode", &self.mode)
            .field("input", &"***")
            .field("error", &self.error)
            .finish()
    }
}

#[derive(Debug)]
pub enum Screen {
    Connecting,
    Startup(StartupStatus),
    Offline { message: String, retry_at: Instant },
    Login(LoginForm),
    LoggingIn { mode: LoginMode, source: String },
    Dashboard,
}

/// Where things were drawn, for mouse hit-testing and selection tracking.
#[derive(Debug, Default)]
pub struct Hit {
    pub tabs: Vec<(Tab, Rect)>,
    pub panes: Vec<(PaneId, Rect)>,
    pub bodies: Vec<(PaneId, Rect)>,
    /// ID of the selected row per table, after sorting/filtering.
    pub selected: HashMap<PaneId, String>,
}

/// Side effects requested by the UI.
#[derive(Debug)]
pub enum Effect {
    Auth(AuthCmd),
    Fetch(Command),
    SaveState,
}

#[derive(Debug)]
pub enum InputMode {
    Normal,
    Filter(PaneId),
}

/// Frozen time for deterministic rendering in tests.
#[derive(Debug, Clone, Default)]
pub struct Clock {
    pub frozen: Option<(Instant, DateTime<Utc>)>,
}

impl Clock {
    pub fn now(&self) -> Instant {
        self.frozen.map(|f| f.0).unwrap_or_else(Instant::now)
    }
    pub fn utc(&self) -> DateTime<Utc> {
        self.frozen.map(|f| f.1).unwrap_or_else(Utc::now)
    }
}

pub struct App {
    pub settings: Settings,
    pub host: String,
    pub themes: Vec<Theme>,
    pub theme_idx: usize,
    pub screen: Screen,
    pub role: Option<Role>,
    pub session: Option<SessionResponse>,
    pub epoch: u64,
    pub tab: Tab,
    pub range: TimeRange,
    pub window: RtWindow,
    pub api_key: Option<ApiKeyOption>,
    pub manual: u64,
    /// See [`Query::nonce`].
    pub nonce: u64,
    pub data: Data,
    pub live: LiveState,
    pub tables: HashMap<PaneId, TableState>,
    pub layout_idx: HashMap<Tab, usize>,
    pub hidden: HashMap<Tab, Vec<PaneId>>,
    pub focus: HashMap<Tab, PaneId>,
    pub dims: HashMap<PaneId, usize>,
    pub mode: InputMode,
    pub help: bool,
    pub flash: Option<(String, Instant)>,
    pub poll_error: HashMap<Group, String>,
    pub clock: Clock,
    pub hit: Hit,
    pub effects: Vec<Effect>,
    pub should_quit: bool,
    pub expired_sent: Option<u64>,
    pub logout_armed: Option<Instant>,
    pub tab_by_role: std::collections::BTreeMap<String, String>,
    pub warnings: Vec<String>,
}

/// Sortable columns per table pane (index → header), used for `s`.
pub fn dims_for(id: PaneId, role: Option<Role>) -> &'static [&'static str] {
    let admin = role == Some(Role::Admin);
    match (id, admin) {
        (PaneId::OvDist, true) => &["models", "api keys", "credentials", "providers"],
        (PaneId::OvDist, false) => &["models", "api keys"],
        (PaneId::RtShare, true) => &["models", "api keys", "credentials", "providers"],
        (PaneId::RtShare, false) => &["models"],
        (PaneId::AnComposition, true) => &["models", "api keys", "credentials", "providers"],
        (PaneId::AnComposition, false) => &["models"],
        _ => &[],
    }
}

impl App {
    pub fn new(settings: Settings, themes: Vec<Theme>, ui: &UiState) -> App {
        let host = crate::api::client::display_host(&settings.base_url);
        let mut theme_idx = themes
            .iter()
            .position(|t| t.name == settings.theme)
            .unwrap_or(0);
        if themes.is_empty() {
            theme_idx = 0;
        }
        let mut app = App {
            range: settings.range,
            window: settings.window,
            settings,
            host,
            themes: if themes.is_empty() {
                vec![Theme::default()]
            } else {
                themes
            },
            theme_idx,
            screen: Screen::Connecting,
            role: None,
            session: None,
            epoch: 0,
            tab: Tab::Overview,
            api_key: None,
            manual: 0,
            nonce: 0,
            data: Data::default(),
            live: LiveState::default(),
            tables: HashMap::new(),
            layout_idx: HashMap::new(),
            hidden: HashMap::new(),
            focus: HashMap::new(),
            dims: HashMap::new(),
            mode: InputMode::Normal,
            help: false,
            flash: None,
            poll_error: HashMap::new(),
            clock: Clock::default(),
            hit: Hit::default(),
            effects: Vec::new(),
            should_quit: false,
            expired_sent: None,
            logout_armed: None,
            tab_by_role: ui.tab.clone(),
            warnings: Vec::new(),
        };
        app.restore_ui(ui);
        app
    }

    fn restore_ui(&mut self, ui: &UiState) {
        for (k, v) in &ui.layout {
            if let Some(t) = Tab::from_key(k) {
                self.layout_idx.insert(t, *v % layout::presets(t).len());
            }
        }
        for (k, nums) in &ui.hidden {
            if let Some(t) = Tab::from_key(k) {
                let ps = layout::panes(t);
                let ids: Vec<PaneId> = nums
                    .iter()
                    .filter_map(|n| ps.get(n.wrapping_sub(1)).map(|d| d.id))
                    .collect();
                if ids.len() < ps.len() {
                    self.hidden.insert(t, ids);
                }
            }
        }
        for (k, v) in &ui.sort {
            if let Some(id) = pane_from_key(k) {
                let mut parts = v.split(':');
                if let Some(Ok(col)) = parts.next().map(str::parse::<usize>) {
                    let desc = parts.next() == Some("desc");
                    self.table_mut(id).sort = Some((col, desc));
                }
            }
        }
        for (k, v) in &ui.dims {
            if let Some(id) = pane_from_key(k) {
                self.dims.insert(id, *v);
            }
        }
    }

    /// Snapshot of persisted UI state.
    pub fn ui_state(&self) -> UiState {
        let mut st = UiState {
            theme: Some(self.theme().name.clone()),
            range: Some(self.range),
            window: Some(self.window),
            refresh_secs: Some(self.settings.refresh_secs),
            graph_symbols: Some(self.settings.graph_symbols),
            privacy: Some(self.settings.privacy),
            tab: self.tab_by_role.clone(),
            ..Default::default()
        };
        for (t, i) in &self.layout_idx {
            st.layout.insert(t.key().into(), *i);
        }
        for (t, ids) in &self.hidden {
            let nums: Vec<usize> = ids.iter().map(|id| layout::pane_def(*id).1).collect();
            if !nums.is_empty() {
                st.hidden.insert(t.key().into(), nums);
            }
        }
        for (id, ts) in &self.tables {
            if let Some((c, d)) = ts.sort {
                st.sort.insert(
                    pane_key(*id),
                    format!("{c}:{}", if d { "desc" } else { "asc" }),
                );
            }
        }
        for (id, d) in &self.dims {
            st.dims.insert(pane_key(*id), *d);
        }
        st
    }

    pub fn theme(&self) -> &Theme {
        &self.themes[self.theme_idx.min(self.themes.len() - 1)]
    }

    pub fn now(&self) -> Instant {
        self.clock.now()
    }

    pub fn table(&self, id: PaneId) -> &TableState {
        self.tables.get(&id).unwrap_or_else(|| empty_table())
    }

    pub fn table_mut(&mut self, id: PaneId) -> &mut TableState {
        self.tables.entry(id).or_default()
    }

    pub fn dim(&self, id: PaneId) -> usize {
        let n = dims_for(id, self.role).len().max(1);
        self.dims.get(&id).copied().unwrap_or(0) % n
    }

    pub fn tabs(&self) -> &'static [Tab] {
        layout::tabs_for(self.role)
    }

    pub fn preset(&self, tab: Tab) -> usize {
        self.layout_idx.get(&tab).copied().unwrap_or(0) % layout::presets(tab).len()
    }

    pub fn hidden(&self, tab: Tab) -> &[PaneId] {
        self.hidden.get(&tab).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Focused pane of the current tab (first visible table by default).
    pub fn focused(&self) -> Option<PaneId> {
        let visible: Vec<PaneId> = self.hit.panes.iter().map(|(id, _)| *id).collect();
        self.focused_in(&visible)
    }

    pub fn focused_in(&self, visible: &[PaneId]) -> Option<PaneId> {
        if let Some(f) = self.focus.get(&self.tab)
            && (visible.is_empty() || visible.contains(f))
        {
            return Some(*f);
        }
        layout::panes(self.tab)
            .iter()
            .filter(|d| d.table && (visible.is_empty() || visible.contains(&d.id)))
            .map(|d| d.id)
            .next()
    }

    pub fn privacy(&self, s: &str) -> String {
        if self.settings.privacy && s.contains('@') {
            crate::fmt::mask_email(s)
        } else {
            s.to_string()
        }
    }

    /// Query for the pollers.
    pub fn query(&self) -> Query {
        let dashboard = matches!(self.screen, Screen::Dashboard);
        Query {
            role: if dashboard && self.expired_sent.is_none() {
                self.role
            } else {
                None
            },
            epoch: self.epoch,
            tab: self.tab,
            range: self.range,
            window: self.window,
            api_key_id: self.api_key.as_ref().map(|k| k.id.clone()),
            refresh: Duration::from_secs(self.settings.refresh_secs),
            slow: Duration::from_secs(self.settings.slow_refresh_secs),
            manual: self.manual,
            nonce: self.nonce,
        }
    }

    pub fn live_params(&self) -> LiveParams {
        let q = self.query();
        LiveParams {
            enabled: q.role == Some(Role::Admin) && self.tab == Tab::Events,
            range: Some(self.range),
            api_key_id: q.api_key_id,
            epoch: self.epoch,
            nonce: self.manual,
        }
    }

    pub fn drain_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    pub fn flash(&mut self, msg: impl Into<String>) {
        self.flash = Some((msg.into(), self.now()));
    }

    // ------------------------------------------------------------ messages

    pub fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Auth(e) => self.on_auth(e),
            Msg::Live(e) => self.on_live(e),
            Msg::Data {
                group,
                scope,
                payload,
            } => self.on_data(group, scope, payload),
        }
    }

    fn on_auth(&mut self, e: AuthEvent) {
        match e {
            AuthEvent::Connecting => {
                if !matches!(self.screen, Screen::Dashboard) {
                    self.screen = Screen::Connecting;
                }
            }
            AuthEvent::Startup(s) => self.screen = Screen::Startup(s),
            AuthEvent::Offline { message, retry_in } => {
                self.screen = Screen::Offline {
                    message,
                    retry_at: self.now() + retry_in,
                }
            }
            AuthEvent::LoggingIn { mode, source } => {
                self.screen = Screen::LoggingIn { mode, source }
            }
            AuthEvent::NeedLogin { error, mode } => {
                self.screen = Screen::Login(LoginForm {
                    mode,
                    input: String::new(),
                    error,
                });
            }
            AuthEvent::LoggedIn {
                role,
                session,
                epoch,
            } => {
                let role_changed = self.role != Some(role);
                self.role = Some(role);
                self.session = Some(session);
                self.epoch = epoch;
                self.expired_sent = None;
                self.screen = Screen::Dashboard;
                if let Some(w) = self.warnings.first().cloned() {
                    self.flash(format!("⚠ {w}"));
                }
                if role_changed {
                    self.data = Data::default();
                    self.live = LiveState::default();
                    self.api_key = None;
                    let saved = self
                        .tab_by_role
                        .get(role.as_str())
                        .and_then(|k| Tab::from_key(k));
                    self.tab = saved
                        .filter(|t| self.tabs().contains(t))
                        .unwrap_or(Tab::Overview);
                }
            }
            AuthEvent::LoggedOut => {
                self.role = None;
                self.session = None;
                self.data = Data::default();
                self.live = LiveState::default();
            }
        }
    }

    fn on_live(&mut self, e: LiveEvent) {
        let now = self.now();
        if matches!(
            e,
            LiveEvent::Snapshot(_)
                | LiveEvent::Append(_)
                | LiveEvent::Checkpoint(_)
                | LiveEvent::Heartbeat(_)
        ) {
            self.live.last_frame = Some(now);
        }
        match e {
            LiveEvent::Stopped => self.live.status = LiveStatus::Idle,
            LiveEvent::Connecting => self.live.status = LiveStatus::Connecting,
            LiveEvent::Snapshot(s) => {
                self.live.status = LiveStatus::Live;
                self.live.events = s.events;
                self.live.sort();
                self.live.total = s.total_count;
                self.live.has_more = s.has_more;
                self.live.next_cursor = s.next_cursor.filter(|c| !c.is_empty() && s.has_more);
                self.live.range_start = s.range_start;
                self.live.loading_more = false;
                self.poll_error.remove(&Group::Detail);
            }
            LiveEvent::Append(a) => {
                self.live.status = LiveStatus::Live;
                self.live.range_start = a.range_start;
                let added = self.live.merge(a.events);
                self.live.appended += added as u64;
                self.live.total += added as i64;
                self.live.prune();
            }
            LiveEvent::Checkpoint(b) => {
                self.live.range_start = b.range_start;
                self.live.prune();
            }
            LiveEvent::Heartbeat(h) => {
                if let Some(t) = h.total_count {
                    self.live.total = t;
                }
                if self.live.status == LiveStatus::Busy && !h.busy {
                    self.live.status = LiveStatus::Live;
                }
            }
            LiveEvent::Busy => self.live.status = LiveStatus::Busy,
            LiveEvent::Reset(_) => self.live.status = LiveStatus::Connecting,
            LiveEvent::Retrying { message, retry_in } => {
                self.live.status = LiveStatus::Retrying {
                    message,
                    until: now + retry_in,
                }
            }
            LiveEvent::Failed(m) => self.live.status = LiveStatus::Failed(m),
            LiveEvent::Unauthorized => self.session_expired(),
        }
    }

    fn session_expired(&mut self) {
        if self.expired_sent != Some(self.epoch) && matches!(self.screen, Screen::Dashboard) {
            self.expired_sent = Some(self.epoch);
            self.effects
                .push(Effect::Auth(AuthCmd::Expired(self.epoch)));
            self.flash("session expired — signing in again");
        }
    }

    fn on_data(&mut self, group: Group, scope: String, payload: Payload) {
        // Drop results for parameters that are no longer current.
        if group != Group::Detail && scope != self.query_scope(group) {
            return;
        }
        let now = self.now();
        let d = &mut self.data;
        let err: Option<ApiError> = match payload {
            Payload::Overview(r) => d.overview.apply(r, now),
            Payload::Activity(r) => d.activity.apply(r, now),
            Payload::Comparisons(r) => d.comparisons.apply(r, now),
            Payload::Realtime(r) => d.realtime.apply(r, now),
            Payload::Analysis(r) => d.analysis.apply(r, now),
            Payload::Latency(r) => d.latency.apply(r, now),
            Payload::KeyOptions(r) => {
                let e = d.key_options.apply(r, now);
                if let (Some(sel), Some(opts)) = (&self.api_key, d.key_options.get())
                    && !opts.options.iter().any(|o| o.id == sel.id)
                {
                    self.api_key = None;
                }
                e
            }
            Payload::Identities(r) => d.identities.apply(r, now),
            Payload::Inspection(r) => d.inspection.apply(r, now),
            Payload::AutoRefresh(r) => d.auto_refresh.apply(r, now),
            Payload::QuotaTasks(list) => {
                if let Some((_, Err(e))) = list.iter().find(|(k, _)| k.is_empty()) {
                    let e = e.clone();
                    d.quota.apply(Err(e), now)
                } else {
                    let mut map = HashMap::new();
                    let mut first_err = None;
                    for (k, r) in list {
                        match r {
                            Ok(t) => {
                                map.insert(k, Ok(t));
                            }
                            Err(ApiError::Http { status: 404, .. }) => {
                                map.insert(k, Err("no cached quota yet".into()));
                            }
                            Err(e) => {
                                if matches!(e, ApiError::Unauthorized(_)) {
                                    first_err = Some(e.clone());
                                }
                                map.insert(k, Err(e.to_string()));
                            }
                        }
                    }
                    match first_err {
                        Some(e) => d.quota.apply(Err(e), now),
                        None => d.quota.apply(Ok(map), now),
                    }
                }
            }
            Payload::QuotaHistory(idx, r) => {
                let mut l = Loaded::default();
                let e = l.apply(r, now);
                d.quota_history = Some((idx, l));
                e
            }
            Payload::IdentityErrors(id, r) => {
                let mut l = Loaded::default();
                let e = l.apply(r, now);
                d.identity_errors = Some((id, l));
                e
            }
            Payload::Catalog(r) => d.catalog.apply(r, now),
            Payload::Pricing(r) => d.pricing.apply(r, now),
            Payload::RecalcCurrent(r) => d.recalc.apply(r, now),
            Payload::RecalcOptions(r) => d.recalc_options.apply(r, now),
            Payload::ApiKeys(r) => d.api_keys.apply(r, now),
            Payload::Sessions(r) => d.sessions.apply(r, now),
            Payload::Status(r) => d.status.apply(r, now),
            Payload::Version(r) => d.version.apply(r, now),
            Payload::MoreEvents(r) => {
                self.live.loading_more = false;
                if scope != self.query_scope(Group::Analysis) {
                    return;
                }
                match r {
                    Ok(page) => {
                        self.live.has_more = page.has_more;
                        self.live.next_cursor = page.next_cursor.filter(|_| page.has_more);
                        self.live.merge(page.events);
                        None
                    }
                    Err(e) => Some(e),
                }
            }
        };
        match err {
            Some(ApiError::Unauthorized(_)) => self.session_expired(),
            Some(e) => {
                self.poll_error.insert(group, e.to_string());
            }
            None => {
                // A fully successful group clears its error marker.
                if !self.group_has_error(group) {
                    self.poll_error.remove(&group);
                }
            }
        }
    }

    fn group_has_error(&self, g: Group) -> bool {
        let d = &self.data;
        match g {
            Group::Overview => {
                d.overview.error.is_some()
                    || d.activity.error.is_some()
                    || d.comparisons.error.is_some()
            }
            Group::Analysis => d.analysis.error.is_some() || d.latency.error.is_some(),
            Group::Realtime => d.realtime.error.is_some(),
            Group::Quota => d.inspection.error.is_some() || d.quota.error.is_some(),
            Group::Models => d.catalog.error.is_some() || d.pricing.error.is_some(),
            Group::Settings => d.api_keys.error.is_some() || d.sessions.error.is_some(),
            _ => false,
        }
    }

    fn query_scope(&self, g: Group) -> String {
        let mut q = self.query();
        if q.role.is_none() {
            q.role = self.role;
        }
        q.scope(g)
    }

    /// Groups whose data the current tab shows.
    pub fn tab_groups(&self) -> &'static [Group] {
        match self.tab {
            Tab::Overview => &[Group::Overview],
            Tab::Realtime => &[Group::Realtime],
            Tab::Analysis => &[Group::Analysis],
            Tab::Events => &[Group::Detail],
            Tab::Credentials => &[Group::Credentials, Group::Detail],
            Tab::Quota => &[Group::Quota, Group::Credentials],
            Tab::Models => &[Group::Models],
            Tab::Settings => &[Group::Settings, Group::Meta],
        }
    }

    /// Error to show in the header for the current tab, if any.
    pub fn current_error(&self) -> Option<&str> {
        if self.tab == Tab::Events {
            match &self.live.status {
                LiveStatus::Retrying { message, .. } | LiveStatus::Failed(message) => {
                    return Some(message.as_str());
                }
                _ => {}
            }
        }
        self.tab_groups()
            .iter()
            .chain([Group::Meta].iter())
            .find_map(|g| self.poll_error.get(g))
            .map(|s| s.as_str())
    }

    /// When the current tab's main data was last refreshed.
    pub fn last_update(&self) -> Option<Instant> {
        let d = &self.data;
        match self.tab {
            Tab::Overview => d.overview.at,
            Tab::Realtime => d.realtime.at,
            Tab::Analysis => d.analysis.at,
            Tab::Events => self.live.last_frame,
            Tab::Credentials => d.identities.at,
            Tab::Quota => d.inspection.at,
            Tab::Models => d.pricing.at,
            Tab::Settings => d.sessions.at,
        }
    }

    // ------------------------------------------------------------ tick

    pub fn on_tick(&mut self) {
        let now = self.now();
        if let Some((_, at)) = &self.flash
            && now.duration_since(*at) > Duration::from_secs(4)
        {
            self.flash = None;
        }
        if let Some(t) = self.logout_armed
            && now.duration_since(t) > Duration::from_secs(3)
        {
            self.logout_armed = None;
        }
    }

    /// Called after each draw: react to selection changes.
    pub fn after_draw(&mut self) {
        if self.role != Some(Role::Admin) || !matches!(self.screen, Screen::Dashboard) {
            return;
        }
        if self.tab == Tab::Credentials
            && let Some(id) = self.hit.selected.get(&PaneId::CrList).cloned()
        {
            let current = self.data.identity_errors.as_ref().map(|(i, _)| i.clone());
            if current.as_deref() != Some(id.as_str()) {
                self.data.identity_errors = Some((id.clone(), Loaded::default()));
                self.effects
                    .push(Effect::Fetch(Command::IdentityErrors { id }));
            }
        }
        if self.tab == Tab::Quota
            && let Some(idx) = self.hit.selected.get(&PaneId::QuList).cloned()
        {
            let current = self.data.quota_history.as_ref().map(|(i, _)| i.clone());
            if current.as_deref() != Some(idx.as_str()) {
                self.data.quota_history = Some((idx.clone(), Loaded::default()));
                let kind = self
                    .data
                    .identities
                    .get()
                    .and_then(|p| p.identities.iter().find(|i| i.identity == idx))
                    .map(|i| i.kind.clone())
                    .unwrap_or_default();
                if kind == "codex" || kind == "claude" {
                    self.effects
                        .push(Effect::Fetch(Command::QuotaHistory { auth_index: idx }));
                }
            }
        }
        // Infinite scroll of the request log.
        if self.tab == Tab::Events && !self.live.loading_more && self.live.has_more {
            let st = self.table(PaneId::EvLog);
            let len = st.len.get();
            if len > 0
                && st.selected + 2 >= len
                && st.filter.is_empty()
                && let Some(cursor) = self.live.next_cursor.clone()
            {
                self.live.loading_more = true;
                let q = self.query();
                self.effects.push(Effect::Fetch(Command::LoadMoreEvents {
                    scope: self.query_scope(Group::Analysis),
                    range: q.range,
                    api_key_id: q.api_key_id,
                    cursor,
                }));
            }
        }
    }

    // ------------------------------------------------------------ input

    pub fn on_event(&mut self, ev: Event) {
        match ev {
            Event::Key(k) if k.kind != KeyEventKind::Release => self.on_key(k),
            Event::Mouse(m) if self.settings.mouse => self.on_mouse(m),
            _ => {}
        }
    }

    fn on_key(&mut self, k: KeyEvent) {
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('c')) {
            self.should_quit = true;
            return;
        }
        if let Screen::Login(_) = self.screen {
            self.on_login_key(k);
            return;
        }
        if !matches!(self.screen, Screen::Dashboard) {
            match k.code {
                KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
                KeyCode::Char('r') | KeyCode::Enter => {
                    self.effects.push(Effect::Auth(AuthCmd::RetryNow))
                }
                KeyCode::Char('t') => self.cycle_theme(true),
                _ => {}
            }
            return;
        }
        if let InputMode::Filter(id) = self.mode {
            self.on_filter_key(id, k);
            return;
        }
        if self.help {
            match k.code {
                KeyCode::Char('q') => self.should_quit = true,
                _ => self.help = false,
            }
            return;
        }
        self.on_dashboard_key(k);
    }

    fn on_login_key(&mut self, k: KeyEvent) {
        let Screen::Login(form) = &mut self.screen else {
            return;
        };
        match k.code {
            KeyCode::Esc => self.should_quit = true,
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right => {
                form.mode = match form.mode {
                    LoginMode::Admin => LoginMode::ApiKey,
                    LoginMode::ApiKey => LoginMode::Admin,
                };
                form.input.clear();
                form.error = None;
            }
            KeyCode::Enter => {
                if form.input.is_empty() {
                    form.error = Some(match form.mode {
                        LoginMode::Admin => "enter the admin password".into(),
                        LoginMode::ApiKey => "enter a CPA API key".into(),
                    });
                    return;
                }
                let secret = Secret::new(std::mem::take(&mut form.input));
                let mode = form.mode;
                self.effects
                    .push(Effect::Auth(AuthCmd::Login(mode, secret)));
                self.screen = Screen::LoggingIn {
                    mode,
                    source: "login screen".into(),
                };
            }
            KeyCode::Backspace => {
                form.input.pop();
            }
            KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                form.input.clear();
            }
            KeyCode::Char(c)
                if !k.modifiers.contains(KeyModifiers::CONTROL) && form.input.len() < 1024 =>
            {
                form.input.push(c);
            }
            _ => {}
        }
    }

    fn on_filter_key(&mut self, id: PaneId, k: KeyEvent) {
        let st = self.table_mut(id);
        match k.code {
            KeyCode::Esc => {
                st.filter.clear();
                st.selected = 0;
                self.mode = InputMode::Normal;
            }
            KeyCode::Enter => self.mode = InputMode::Normal,
            KeyCode::Backspace => {
                st.filter.pop();
                st.selected = 0;
            }
            KeyCode::Char(c) => {
                st.filter.push(c);
                st.selected = 0;
            }
            _ => {}
        }
    }

    fn set_tab(&mut self, t: Tab) {
        if self.tab != t {
            self.tab = t;
            self.help = false;
            if let Some(r) = self.role {
                self.tab_by_role.insert(r.as_str().into(), t.key().into());
            }
            self.effects.push(Effect::SaveState);
        }
    }

    fn step_tab(&mut self, forward: bool) {
        let tabs = self.tabs();
        let i = tabs.iter().position(|t| *t == self.tab).unwrap_or(0);
        let n = tabs.len();
        let next = if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        };
        self.set_tab(tabs[next]);
    }

    fn cycle_theme(&mut self, forward: bool) {
        let n = self.themes.len();
        self.theme_idx = if forward {
            (self.theme_idx + 1) % n
        } else {
            (self.theme_idx + n - 1) % n
        };
        let name = self.theme().name.clone();
        self.flash(format!("theme: {name}"));
        self.effects.push(Effect::SaveState);
    }

    fn set_range(&mut self, r: TimeRange) {
        if r != self.range {
            self.range = r;
            // Range-scoped data is stale now; hide it until the new data arrives.
            self.data.overview = Loaded::default();
            self.data.activity = Loaded::default();
            self.data.comparisons = Loaded::default();
            self.data.analysis = Loaded::default();
            self.data.latency = Loaded::default();
            self.live = LiveState {
                result_filter: self.live.result_filter,
                ..Default::default()
            };
            self.table_mut(PaneId::EvLog).selected = 0;
            self.nonce += 1;
            self.effects.push(Effect::SaveState);
        }
    }

    fn move_sel(&mut self, id: PaneId, delta: isize) {
        let st = self.table_mut(id);
        let len = st.len.get();
        if len == 0 {
            st.selected = 0;
            return;
        }
        let cur = st.selected.min(len - 1) as isize;
        st.selected = (cur + delta).clamp(0, len as isize - 1) as usize;
    }

    fn toggle_pane(&mut self, n: usize) {
        let ps = layout::panes(self.tab);
        let Some(def) = ps.get(n.wrapping_sub(1)) else {
            return;
        };
        let preset = layout::presets(self.tab)[self.preset(self.tab)];
        let in_preset = preset
            .rows
            .iter()
            .any(|r| r.panes.iter().any(|(id, _)| *id == def.id));
        let hidden = self.hidden.entry(self.tab).or_default();
        if let Some(pos) = hidden.iter().position(|h| *h == def.id) {
            hidden.remove(pos);
        } else {
            let visible_in_preset = preset
                .rows
                .iter()
                .flat_map(|r| r.panes.iter())
                .filter(|(id, _)| !hidden.contains(id))
                .count();
            if in_preset && visible_in_preset <= 1 {
                self.flash("at least one pane must stay visible");
                return;
            }
            hidden.push(def.id);
        }
        if !in_preset {
            self.flash(format!("{} is not part of this layout — press p", def.name));
        }
        self.effects.push(Effect::SaveState);
    }

    fn cycle_focus(&mut self) {
        let visible: Vec<PaneId> = self
            .hit
            .panes
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| layout::pane_def(*id).2.table)
            .collect();
        if visible.is_empty() {
            return;
        }
        let cur = self.focused();
        let i = cur
            .and_then(|c| visible.iter().position(|v| *v == c))
            .map(|i| (i + 1) % visible.len())
            .unwrap_or(0);
        self.focus.insert(self.tab, visible[i]);
    }

    fn cycle_api_key(&mut self) {
        if self.role != Some(Role::Admin) {
            return;
        }
        let opts: Vec<ApiKeyOption> = self
            .data
            .key_options
            .get()
            .map(|o| o.options.clone())
            .unwrap_or_default();
        if opts.is_empty() {
            self.flash("no API keys to filter by");
            return;
        }
        let next = match &self.api_key {
            None => Some(opts[0].clone()),
            Some(cur) => {
                let i = opts.iter().position(|o| o.id == cur.id);
                match i {
                    Some(i) if i + 1 < opts.len() => Some(opts[i + 1].clone()),
                    _ => None,
                }
            }
        };
        let label = next
            .as_ref()
            .map(|k| crate::fmt::redact_key(&k.label))
            .unwrap_or_else(|| "all".into());
        self.api_key = next;
        self.data.overview = Loaded::default();
        self.data.activity = Loaded::default();
        self.data.comparisons = Loaded::default();
        self.data.realtime = Loaded::default();
        self.data.analysis = Loaded::default();
        self.data.latency = Loaded::default();
        self.live = LiveState::default();
        self.nonce += 1;
        self.flash(format!("api key filter: {label}"));
    }

    fn on_dashboard_key(&mut self, k: KeyEvent) {
        let focused = self.focused();
        let page = focused
            .map(|f| self.table(f).page.get().max(1) as isize)
            .unwrap_or(10);
        match k.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc => {
                let f = focused.map(|f| !self.table(f).filter.is_empty());
                if let (Some(true), Some(id)) = (f, focused) {
                    let st = self.table_mut(id);
                    st.filter.clear();
                    st.selected = 0;
                } else {
                    self.should_quit = true;
                }
            }
            KeyCode::Char('?') | KeyCode::F(1) => self.help = true,
            KeyCode::Tab | KeyCode::Right => self.step_tab(true),
            KeyCode::BackTab | KeyCode::Left => self.step_tab(false),
            KeyCode::F(n) if (n as usize) >= 2 && (n as usize) <= self.tabs().len() + 1 => {
                self.set_tab(self.tabs()[n as usize - 2])
            }
            KeyCode::Char(c @ '1'..='9') => self.toggle_pane(c as usize - '0' as usize),
            KeyCode::Char('f') => self.cycle_focus(),
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(f) = focused {
                    self.move_sel(f, -1)
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(f) = focused {
                    self.move_sel(f, 1)
                }
            }
            KeyCode::PageUp => {
                if let Some(f) = focused {
                    self.move_sel(f, -page)
                }
            }
            KeyCode::PageDown => {
                if let Some(f) = focused {
                    self.move_sel(f, page)
                }
            }
            KeyCode::Home | KeyCode::Char('g') => {
                if let Some(f) = focused {
                    self.table_mut(f).selected = 0
                }
            }
            KeyCode::End | KeyCode::Char('G') => {
                if let Some(f) = focused {
                    let len = self.table(f).len.get();
                    self.table_mut(f).selected = len.saturating_sub(1);
                }
            }
            KeyCode::Char('s') => {
                if let Some(f) = focused {
                    let st = self.table_mut(f);
                    let n = st.cols.get().max(1);
                    st.sort = match st.sort {
                        None => Some((0, true)),
                        Some((c, d)) if c + 1 < n => Some((c + 1, d)),
                        Some(_) => None,
                    };
                    self.effects.push(Effect::SaveState);
                }
            }
            KeyCode::Char('S') => {
                if let Some(f) = focused {
                    let st = self.table_mut(f);
                    st.sort = Some(match st.sort {
                        None => (0, false),
                        Some((c, d)) => (c, !d),
                    });
                    self.effects.push(Effect::SaveState);
                }
            }
            KeyCode::Char('/') => {
                if let Some(f) = focused {
                    self.mode = InputMode::Filter(f);
                }
            }
            KeyCode::Char(']') | KeyCode::Char('r') => self.set_range(self.range.step(true)),
            KeyCode::Char('[') => self.set_range(self.range.step(false)),
            KeyCode::Char('w') => {
                self.window = self.window.next();
                self.data.realtime = Loaded::default();
                self.nonce += 1;
                self.effects.push(Effect::SaveState);
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.settings.refresh_secs = step_refresh(self.settings.refresh_secs, true);
                self.flash(format!("refresh every {}s", self.settings.refresh_secs));
                self.effects.push(Effect::SaveState);
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.settings.refresh_secs = step_refresh(self.settings.refresh_secs, false);
                self.flash(format!("refresh every {}s", self.settings.refresh_secs));
                self.effects.push(Effect::SaveState);
            }
            KeyCode::Char('t') => self.cycle_theme(true),
            KeyCode::Char('T') => self.cycle_theme(false),
            KeyCode::Char('p') | KeyCode::Char('l') => {
                let n = layout::presets(self.tab).len();
                let i = (self.preset(self.tab) + 1) % n;
                self.layout_idx.insert(self.tab, i);
                self.flash(format!("layout: {}", layout::presets(self.tab)[i].name));
                self.effects.push(Effect::SaveState);
            }
            KeyCode::Char('P') => {
                let n = layout::presets(self.tab).len();
                let i = (self.preset(self.tab) + n - 1) % n;
                self.layout_idx.insert(self.tab, i);
                self.flash(format!("layout: {}", layout::presets(self.tab)[i].name));
                self.effects.push(Effect::SaveState);
            }
            KeyCode::Char('d') => {
                let target = focused
                    .filter(|f| !dims_for(*f, self.role).is_empty())
                    .or_else(|| {
                        self.hit
                            .panes
                            .iter()
                            .map(|(id, _)| *id)
                            .find(|id| !dims_for(*id, self.role).is_empty())
                    });
                if let Some(id) = target {
                    let n = dims_for(id, self.role).len();
                    let next = (self.dim(id) + 1) % n;
                    self.dims.insert(id, next);
                    let st = self.table_mut(id);
                    st.selected = 0;
                    st.filter.clear();
                    self.flash(format!("showing {}", dims_for(id, self.role)[next]));
                    self.effects.push(Effect::SaveState);
                }
            }
            KeyCode::Char('a') => self.cycle_api_key(),
            KeyCode::Char('e') if self.tab == Tab::Events => {
                self.live.result_filter = match self.live.result_filter {
                    ResultFilter::All => ResultFilter::Success,
                    ResultFilter::Success => ResultFilter::Failed,
                    ResultFilter::Failed => ResultFilter::All,
                };
                self.table_mut(PaneId::EvLog).selected = 0;
            }
            KeyCode::Char('b') => {
                self.settings.graph_symbols = self.settings.graph_symbols.next();
                self.flash(format!("graphs: {}", self.settings.graph_symbols.name()));
                self.effects.push(Effect::SaveState);
            }
            KeyCode::Char('x') => {
                self.settings.privacy = !self.settings.privacy;
                self.flash(if self.settings.privacy {
                    "privacy mode on"
                } else {
                    "privacy mode off"
                });
                self.effects.push(Effect::SaveState);
            }
            KeyCode::Char('R') | KeyCode::F(5) => {
                self.manual += 1;
                if self.tab == Tab::Events {
                    // The live stream reconnects (fresh snapshot) because its
                    // parameters include the manual-refresh counter.
                    self.live = LiveState {
                        result_filter: self.live.result_filter,
                        ..Default::default()
                    };
                }
                self.flash("refreshing");
            }
            KeyCode::Char('X') => {
                let armed = self.logout_armed.is_some();
                if armed {
                    self.logout_armed = None;
                    self.effects.push(Effect::Auth(AuthCmd::Logout));
                } else {
                    self.logout_armed = Some(self.now());
                    self.flash("press X again to log out");
                }
            }
            _ => {}
        }
    }

    fn on_mouse(&mut self, m: MouseEvent) {
        let pos = Position::new(m.column, m.row);
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if !matches!(self.screen, Screen::Dashboard) {
                    return;
                }
                if self.help {
                    self.help = false;
                    return;
                }
                if let Some((t, _)) = self.hit.tabs.iter().find(|(_, r)| r.contains(pos)) {
                    let t = *t;
                    self.set_tab(t);
                    return;
                }
                let pane = self
                    .hit
                    .panes
                    .iter()
                    .find(|(_, r)| r.contains(pos))
                    .map(|(id, _)| *id);
                if let Some(id) = pane {
                    if layout::pane_def(id).2.table {
                        self.focus.insert(self.tab, id);
                    }
                    let body = self
                        .hit
                        .bodies
                        .iter()
                        .find(|(b, _)| *b == id)
                        .map(|(_, r)| *r);
                    if let Some(body) = body
                        && body.contains(pos)
                    {
                        let st = self.table_mut(id);
                        let row = st.offset.get() + (m.row - body.y) as usize;
                        if row < st.len.get() {
                            st.selected = row;
                        }
                    }
                }
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let delta = if m.kind == MouseEventKind::ScrollDown {
                    3
                } else {
                    -3
                };
                let pane = self
                    .hit
                    .panes
                    .iter()
                    .find(|(_, r)| r.contains(pos))
                    .map(|(id, _)| *id)
                    .filter(|id| layout::pane_def(*id).2.table);
                if let Some(id) = pane {
                    self.focus.insert(self.tab, id);
                    self.move_sel(id, delta);
                }
            }
            _ => {}
        }
    }
}

fn empty_table() -> &'static TableState {
    // TableState contains Cells, so it is not Sync; build one per thread.
    thread_local! {
        static EMPTY: &'static TableState = Box::leak(Box::default());
    }
    EMPTY.with(|e| *e)
}

pub fn pane_key(id: PaneId) -> String {
    let (tab, n, _) = layout::pane_def(id);
    format!("{}.{n}", tab.key())
}

fn pane_from_key(k: &str) -> Option<PaneId> {
    let (t, n) = k.split_once('.')?;
    let tab = Tab::from_key(t)?;
    let n: usize = n.parse().ok()?;
    layout::panes(tab).get(n.checked_sub(1)?).map(|d| d.id)
}

impl GraphSymbols {
    pub fn braille(self) -> bool {
        self == GraphSymbols::Braille
    }
}
