//! Application state and message handling. Key and mouse bindings live in
//! [`keys`], overlays (pickers, range editor) in [`overlay`], the request-log
//! state in [`live`].

mod keys;
pub mod live;
mod messages;
pub mod overlay;
mod privacy;

pub use live::{EventFilters, LiveState, LiveStatus, ResultFilter};
pub use overlay::{Overlay, PickItem, PickKind, Picker, RangeEditor, Unit};
pub use privacy::{
    HIDDEN, IP_MASK, Sensitive, contains_ip, guard_buffer, ident_kind, is_identifier, is_ip,
    is_sensitive, looks_like_key, mask_ips, raw_ident_name, scrub, sensitive_spans,
};

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
use std::cell::{Cell, RefCell};
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
    /// Last bucket with data per graph pane (where the cursor starts).
    pub graph_last: HashMap<PaneId, usize>,
    /// ID of the selected row per table, after sorting/filtering.
    pub selected: HashMap<PaneId, String>,
    /// Picker rows (item index → screen row) for mouse selection.
    pub picker: Vec<(usize, Rect)>,
    /// Every field (column title, full text) of the selected row per
    /// table, for the row detail popup.
    pub row_fields: HashMap<PaneId, Vec<(String, String)>>,
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

/// How a tab's data is refreshed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cadence {
    /// Polled at this interval.
    Every(Duration),
    /// Pushed by the server (the request log's live stream).
    Live,
    /// Fetched on entry, range change or `R`.
    OnDemand,
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
    /// Help overlay scroll offset; the renderer reports the largest useful
    /// offset and the visible height.
    pub help_scroll: usize,
    pub help_max: Cell<usize>,
    pub help_page: Cell<usize>,
    /// Row popup scroll offset; like the help, the renderer reports the
    /// largest useful offset and the visible height.
    pub row_scroll: usize,
    pub row_max: Cell<usize>,
    pub row_page: Cell<usize>,
    /// Privacy mode: account names → credential id, cached per credential
    /// list (keyed by its load time).
    pub privacy_aliases: RefCell<(Option<Instant>, HashMap<String, u64>)>,
    /// Privacy mode: key for the tags of names that are not a credential
    /// (persisted in the UI state so tags are stable across runs).
    pub privacy_salt: u64,
    /// Privacy mode: spans the render guard masked in the last frame
    /// (should stay 0 — panes replace identifiers themselves).
    pub privacy_guard_hits: Cell<usize>,
    /// Tag of a newer cpatop release, from the background update check.
    pub update_available: Option<String>,
}

/// Dimensions of multi-dimension tables (`d` cycles them).
pub fn dims_for(id: PaneId, role: Option<Role>) -> &'static [&'static str] {
    let admin = role == Some(Role::Admin);
    match (id, admin) {
        (PaneId::OvDist, true) => &["models", "api keys", "credentials", "providers"],
        // A key viewer sees one key: an "api keys" dimension adds nothing.
        (PaneId::OvDist, false) => &["models"],
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
            help_scroll: 0,
            help_max: Cell::new(0),
            help_page: Cell::new(10),
            row_scroll: 0,
            row_max: Cell::new(0),
            row_page: Cell::new(10),
            privacy_aliases: RefCell::new((None, HashMap::new())),
            privacy_salt: ui
                .privacy_salt
                .unwrap_or_else(|| fastrand::u64(1..=i64::MAX as u64)),
            privacy_guard_hits: Cell::new(0),
            update_available: None,
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
        let order = ui.pane_order.unwrap_or(1);
        for (k, nums) in &ui.hidden {
            if let Some(t) = Tab::from_key(k) {
                let ps = layout::panes(t);
                let ids: Vec<PaneId> = nums
                    .iter()
                    .map(|n| layout::migrate_pane_number(t, *n, order))
                    .filter_map(|n| ps.get(n.wrapping_sub(1)).map(|d| d.id))
                    .collect();
                if ids.len() < ps.len() {
                    self.hidden.insert(t, ids);
                }
            }
        }
        for (k, v) in &ui.sort {
            if let Some(id) = pane_from_key(k, order) {
                let mut parts = v.split(':');
                if let Some(Ok(col)) = parts.next().map(str::parse::<usize>) {
                    let desc = parts.next() == Some("desc");
                    self.table_mut(id).sort = Some((col, desc));
                }
            }
        }
        for (k, v) in &ui.dims {
            if let Some(id) = pane_from_key(k, order) {
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
            privacy_salt: Some(self.privacy_salt),
            tab: self.tab_by_role.clone(),
            pane_order: Some(layout::PANE_ORDER),
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

    /// Hidden panes plus panes that make no sense for the role: the key
    /// viewer's composition only repeats the models pane, and its key ×
    /// model heatmap has a single row.
    pub fn effective_hidden(&self, tab: Tab) -> Vec<PaneId> {
        let mut v = self.hidden(tab).to_vec();
        if tab == Tab::Analysis && self.role == Some(Role::KeyViewer) {
            v.extend([PaneId::AnComposition, PaneId::AnHeatmap]);
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

/// The pane a persisted `tab.N` key names, `N` in numbering `order`.
fn pane_from_key(k: &str, order: u32) -> Option<PaneId> {
    let (t, n) = k.split_once('.')?;
    let tab = Tab::from_key(t)?;
    let n = layout::migrate_pane_number(tab, n.parse().ok()?, order);
    layout::panes(tab).get(n.checked_sub(1)?).map(|d| d.id)
}

impl GraphSymbols {
    pub fn braille(self) -> bool {
        self == GraphSymbols::Braille
    }
}
