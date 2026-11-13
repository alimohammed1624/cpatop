//! Application state and message handling. Key and mouse bindings live in
//! [`keys`], overlays (pickers, range editor) in [`overlay`], the request-log
//! state in [`live`].

mod keys;
pub mod live;
pub mod overlay;

pub use live::{EventFilters, LiveState, LiveStatus, ResultFilter};
pub use overlay::{Overlay, PickItem, PickKind, Picker, RangeEditor, Unit};

use crate::api::models::*;
use crate::api::sse::{LiveEvent, LiveParams};
use crate::api::{ApiError, LoginMode, Role};
use crate::auth::{AuthCmd, AuthEvent};
use crate::config::{GraphSymbols, Settings, UiState};
use crate::layout::{self, PaneId, Tab};
use crate::poller::{Command, Group, Msg, Payload, Query};
use crate::range::{RtWindow, TimeRange, limits};
use crate::theme::Theme;
use chrono::{DateTime, FixedOffset, Utc};
use ratatui::layout::Rect;
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
    /// Range-scoped per-key usage for the Keys tab.
    pub key_usage: Loaded<Comparisons>,
    pub event_models: Loaded<EventModelFilters>,
    pub event_sources: Loaded<EventSourceFilters>,
    pub sessions: Loaded<AuthSessions>,
    pub status: Loaded<ServerStatus>,
    pub version: Loaded<VersionInfo>,
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
    /// Plot area and bucket count of each graph pane (for the cursor).
    pub graphs: Vec<(PaneId, Rect, usize)>,
    /// ID of the selected row per table, after sorting/filtering.
    pub selected: HashMap<PaneId, String>,
    /// Picker rows (item index → screen row) for mouse selection.
    pub picker: Vec<(usize, Rect)>,
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

/// How long `X` stays armed for the logout confirmation.
pub const LOGOUT_ARM: Duration = Duration::from_secs(3);

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
    /// Graph cursor (bucket index) per graph pane.
    pub cursor: HashMap<PaneId, usize>,
    pub mode: InputMode,
    pub overlay: Option<Overlay>,
    pub flash: Option<(String, Instant)>,
    pub poll_error: HashMap<Group, String>,
    pub clock: Clock,
    pub hit: Hit,
    pub effects: Vec<Effect>,
    pub should_quit: bool,
    pub expired_sent: Option<u64>,
    pub gate_sent: Option<u64>,
    pub logout_armed: Option<Instant>,
    pub tab_by_role: std::collections::BTreeMap<String, String>,
    pub warnings: Vec<String>,
    /// UTC offset of the server, learned from timestamps in its responses.
    pub server_offset: Option<FixedOffset>,
}

/// Dimensions of multi-dimension tables (`d` cycles them).
pub fn dims_for(id: PaneId, role: Option<Role>) -> &'static [&'static str] {
    let admin = role == Some(Role::Admin);
    match (id, admin) {
        (PaneId::OvDist, true) => &["models", "api keys", "credentials", "providers"],
        (PaneId::OvDist, false) => &["models", "api keys"],
        (PaneId::RtShare, true) => &["models", "api keys", "credentials", "providers"],
        (PaneId::RtShare, false) => &["models"],
        // Models already have their own pane on the Analysis tab, so the
        // composition starts on credentials.
        (PaneId::AnComposition, true) => &["credentials", "api keys", "providers", "models"],
        (PaneId::AnComposition, false) => &["models"],
        _ => &[],
    }
}

impl App {
    pub fn new(settings: Settings, themes: Vec<Theme>, ui: &UiState) -> App {
        let host = crate::api::client::display_host(&settings.base_url);
        let mut themes = if themes.is_empty() {
            vec![Theme::default()]
        } else {
            themes
        };
        for t in &mut themes {
            t.no_color = settings.no_color;
        }
        let theme_idx = themes
            .iter()
            .position(|t| t.name == settings.theme)
            .unwrap_or(0);
        let mut app = App {
            range: settings.range,
            window: settings.window,
            settings,
            host,
            themes,
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
            cursor: HashMap::new(),
            mode: InputMode::Normal,
            overlay: None,
            flash: None,
            poll_error: HashMap::new(),
            clock: Clock::default(),
            hit: Hit::default(),
            effects: Vec::new(),
            should_quit: false,
            expired_sent: None,
            gate_sent: None,
            logout_armed: None,
            tab_by_role: ui.tab.clone(),
            warnings: Vec::new(),
            server_offset: None,
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

    /// Current time in the server's offset (falls back to the local offset).
    pub fn server_now(&self) -> DateTime<FixedOffset> {
        let off = self
            .server_offset
            .unwrap_or_else(|| *chrono::Local::now().offset());
        self.clock.utc().with_timezone(&off)
    }

    /// Longest custom day range for the current tab.
    pub fn max_custom_days(&self) -> i64 {
        if self.tab == Tab::Events {
            limits::MAX_DAYS_EVENTS
        } else {
            limits::MAX_DAYS
        }
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

    /// Hidden panes plus panes that make no sense for the role (the key
    /// viewer's composition only repeats the models pane).
    pub fn effective_hidden(&self, tab: Tab) -> Vec<PaneId> {
        let mut v = self.hidden(tab).to_vec();
        if tab == Tab::Analysis && self.role == Some(Role::KeyViewer) {
            v.push(PaneId::AnComposition);
        }
        v
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

    // ------------------------------------------------------------ key names

    /// Distinguishable API-key name: `#3 ci-bot xyz…` / `#1 xyz…`.
    /// Never shows more of a key than its 3–6 character prefix.
    pub fn key_name(&self, id: &str, label: &str) -> String {
        let item = self
            .data
            .api_keys
            .get()
            .and_then(|k| k.items.iter().find(|i| i.id == id));
        match item {
            Some(i) => key_item_name(i),
            None if id.is_empty() => crate::fmt::redact_key(label),
            None => {
                let l = crate::fmt::redact_key(label);
                if l.is_empty() {
                    format!("#{id}")
                } else {
                    format!("#{id} {l}")
                }
            }
        }
    }

    /// Name for a key known only by its server label (request events).
    pub fn key_name_by_label(&self, label: &str) -> String {
        if let Some(keys) = self.data.api_keys.get() {
            let mut m = keys
                .items
                .iter()
                .filter(|i| i.label == label || (!i.key_alias.is_empty() && i.key_alias == label));
            if let (Some(i), None) = (m.next(), m.next()) {
                return key_item_name(i);
            }
        }
        crate::fmt::redact_key(label)
    }

    /// Name of the session's own key (API-key role).
    pub fn session_key_name(&self) -> String {
        let Some(k) = self.session.as_ref().and_then(|s| s.api_key.as_ref()) else {
            return String::new();
        };
        let prefix = crate::fmt::redact_key(&k.display_key);
        match k.alias.as_deref().filter(|a| !a.is_empty()) {
            Some(a) => format!("#{} {a} {prefix}", k.id),
            None => format!("#{} {prefix}", k.id),
        }
    }

    // ------------------------------------------------------------ queries

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
            filters: self.live.filters.clone(),
            epoch: self.epoch,
            nonce: self.manual,
        }
    }

    /// Identifies the request-log result set (range, key, filters); history
    /// pages for another scope are dropped.
    pub fn events_scope(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.range.key(),
            self.api_key
                .as_ref()
                .map(|k| k.id.as_str())
                .unwrap_or("all"),
            self.live.filters.key(),
            self.nonce
        )
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
                self.overlay = None;
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
                self.gate_sent = None;
                self.screen = Screen::Dashboard;
                if let Some(w) = self.warnings.first().cloned() {
                    self.flash(format!("⚠ {w}"));
                }
                if role_changed {
                    self.data = Data::default();
                    self.live.reset();
                    self.api_key = None;
                    self.overlay = None;
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
                self.live.reset();
                self.overlay = None;
            }
        }
    }

    fn learn_offset(&mut self, ts: &str) {
        if let Some(t) = crate::fmt::parse_ts(ts)
            && !ts.ends_with('Z')
        {
            self.server_offset = Some(*t.offset());
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
                self.learn_offset(&s.range_start);
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
            LiveEvent::RangeConflict(m) => {
                if !self.clamp_custom_range() {
                    self.live.status = LiveStatus::Failed(format!("409 {m}"));
                }
            }
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

    /// After a `409`, move a custom range back inside the server's bounds.
    /// Returns false when the range is not custom.
    fn clamp_custom_range(&mut self) -> bool {
        let TimeRange::Custom(c) = self.range else {
            return false;
        };
        let clamped = c.clamp(self.server_now(), self.max_custom_days());
        if clamped == c {
            return false;
        }
        let r = TimeRange::Custom(clamped);
        self.set_range(r);
        self.flash(format!("range moved out of bounds — now {}", r.label()));
        true
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
            Payload::Activity(r) => {
                let e = d.activity.apply(r, now);
                if let Some(ts) = self
                    .data
                    .activity
                    .get()
                    .filter(|a| a.grain == "short" || a.grain == "daily")
                    .map(|a| a.window_start.clone())
                {
                    self.learn_offset(&ts);
                }
                e
            }
            Payload::Comparisons(r) => {
                let e = d.comparisons.apply(r, now);
                if let Some(b) = self.data.comparisons.get().and_then(|c| c.buckets.first()) {
                    let b = b.clone();
                    self.learn_offset(&b);
                }
                e
            }
            Payload::Realtime(r) => {
                let e = d.realtime.apply(r, now);
                if let Some(w) = self.data.realtime.get().and_then(|r| r.window_end.clone()) {
                    self.learn_offset(&w);
                }
                e
            }
            Payload::Analysis(r) => {
                let e = d.analysis.apply(r, now);
                if let Some(s) = self.data.analysis.get().and_then(|a| a.range_start.clone()) {
                    self.learn_offset(&s);
                }
                e
            }
            Payload::Latency(r) => d.latency.apply(r, now),
            Payload::KeyOptions(r) => {
                let e = d.key_options.apply(r, now);
                if let (Some(sel), Some(opts)) = (&self.api_key, d.key_options.get())
                    && !opts.options.iter().any(|o| o.id == sel.id)
                {
                    self.api_key = None;
                    self.discard_scoped();
                    self.flash("the filtered API key no longer exists — showing all keys");
                }
                e
            }
            Payload::KeyUsage(r) => d.key_usage.apply(r, now),
            Payload::EventModels(r) => {
                let e = d.event_models.apply(r, now);
                let gone = match (&self.live.filters.model, self.data.event_models.get()) {
                    (Some(m), Some(list)) => !list.models.contains(m),
                    _ => false,
                };
                if gone {
                    let mut f = self.live.filters.clone();
                    f.model = None;
                    self.set_event_filters(f);
                    self.flash("model filter no longer offered — cleared");
                }
                e
            }
            Payload::EventSources(r) => {
                let e = d.event_sources.apply(r, now);
                let gone = match (&self.live.filters.source, self.data.event_sources.get()) {
                    (Some((v, _)), Some(list)) => !list.sources.iter().any(|s| &s.value == v),
                    _ => false,
                };
                if gone {
                    let mut f = self.live.filters.clone();
                    f.source = None;
                    self.set_event_filters(f);
                    self.flash("source filter no longer offered — cleared");
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
                                if matches!(e, ApiError::Unauthorized(_) | ApiError::Migrating) {
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
                if scope != self.events_scope() {
                    return;
                }
                self.live.loading_more = false;
                match r {
                    Ok(page) => {
                        self.live.has_more = page.has_more;
                        self.live.next_cursor = page.next_cursor.filter(|_| page.has_more);
                        self.live.merge(page.events);
                        None
                    }
                    Err(ApiError::Http { status: 400, .. }) => {
                        // Invalid cursor: restart from the first page.
                        self.live.reset();
                        self.manual += 1;
                        self.flash("history cursor rejected — reloading the request log");
                        None
                    }
                    Err(e) => Some(e),
                }
            }
        };
        match err {
            Some(ApiError::Unauthorized(_)) => self.session_expired(),
            Some(ApiError::Migrating) => {
                if self.gate_sent != Some(self.epoch) {
                    self.gate_sent = Some(self.epoch);
                    self.effects.push(Effect::Auth(AuthCmd::Gate));
                }
            }
            Some(ApiError::Http { status: 409, .. }) if self.clamp_custom_range() => {}
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
            Group::Settings => d.sessions.error.is_some(),
            Group::KeyOptions => d.api_keys.error.is_some() || d.key_options.error.is_some(),
            Group::EventFilters => {
                d.event_models.error.is_some() || d.event_sources.error.is_some()
            }
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
            Tab::Events => &[Group::Detail, Group::EventFilters],
            Tab::Credentials => &[Group::Credentials, Group::Detail],
            Tab::Quota => &[Group::Quota, Group::Credentials],
            Tab::Models => &[Group::Models],
            Tab::Settings => &[
                Group::Settings,
                Group::KeyUsage,
                Group::KeyOptions,
                Group::Meta,
            ],
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
            Tab::Settings => d.key_usage.at.or(d.sessions.at),
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
            && now.duration_since(t) >= LOGOUT_ARM
        {
            self.logout_armed = None;
        }
    }

    /// Seconds left before an armed logout disarms.
    pub fn logout_countdown(&self) -> Option<u64> {
        let t = self.logout_armed?;
        let left = LOGOUT_ARM.saturating_sub(self.now().saturating_duration_since(t));
        Some(left.as_secs_f64().ceil().max(1.0) as u64)
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
        // Infinite scroll of the request log: near the end of what is loaded,
        // fetch the next server page (with the same server-side filters).
        if self.tab == Tab::Events && !self.live.loading_more && self.live.has_more {
            let st = self.table(PaneId::EvLog);
            let len = st.len.get();
            let near_end = st.selected + 2 >= len;
            if near_end && let Some(cursor) = self.live.next_cursor.clone() {
                self.live.loading_more = true;
                let q = self.query();
                self.effects.push(Effect::Fetch(Command::LoadMoreEvents {
                    scope: self.events_scope(),
                    range: q.range,
                    api_key_id: q.api_key_id,
                    filters: self.live.filters.clone(),
                    cursor,
                }));
            }
        }
    }

    // ------------------------------------------------------------ actions

    pub(crate) fn set_tab(&mut self, t: Tab) {
        if self.tab != t {
            self.tab = t;
            self.overlay = None;
            if let Some(r) = self.role {
                self.tab_by_role.insert(r.as_str().into(), t.key().into());
            }
            self.effects.push(Effect::SaveState);
        }
    }

    /// Forget range- and key-scoped data so stale numbers never show under
    /// new parameters, and force a refetch.
    fn discard_scoped(&mut self) {
        self.data.overview = Loaded::default();
        self.data.activity = Loaded::default();
        self.data.comparisons = Loaded::default();
        self.data.realtime = Loaded::default();
        self.data.analysis = Loaded::default();
        self.data.latency = Loaded::default();
        self.data.key_usage = Loaded::default();
        self.live.reset();
        self.table_mut(PaneId::EvLog).selected = 0;
        self.nonce += 1;
    }

    pub fn set_range(&mut self, r: TimeRange) {
        if r != self.range {
            self.range = r;
            self.discard_scoped();
            self.effects.push(Effect::SaveState);
        }
    }

    pub fn set_api_key(&mut self, k: Option<ApiKeyOption>) {
        if k.as_ref().map(|k| &k.id) == self.api_key.as_ref().map(|k| &k.id) {
            return;
        }
        let label = match &k {
            Some(k) => self.key_name(&k.id, &k.label),
            None => "all keys".into(),
        };
        self.api_key = k;
        self.discard_scoped();
        self.flash(format!("API key: {label}"));
    }

    /// Change the request-log filters; the live stream reconnects and history
    /// paging restarts with the new server-side filters.
    pub fn set_event_filters(&mut self, f: EventFilters) {
        if f == self.live.filters {
            return;
        }
        self.live.filters = f;
        self.live.reset();
        self.table_mut(PaneId::EvLog).selected = 0;
        self.table_mut(PaneId::EvLog).offset.set(0);
    }
}

fn key_item_name(i: &ApiKeyItem) -> String {
    if i.source_type == "plugin" {
        let p = i.access_provider.trim_start_matches("plugin:");
        return format!("#{} plugin {p}", i.id).trim_end().to_string();
    }
    let prefix = crate::fmt::redact_key(&i.display_key);
    if i.key_alias.is_empty() {
        format!("#{} {prefix}", i.id)
    } else {
        format!("#{} {} {prefix}", i.id, i.key_alias)
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
