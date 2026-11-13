//! Handling of messages from the auth manager, the pollers and the live
//! stream: applying data, dropping stale responses, reacting to errors
//! (401 → re-login, 503 migration → startup gate, 409 → clamp range).

use super::*;

impl App {
    /// Local midnight the selected range starts at, for calendar-day
    /// ranges with hourly buckets (`today`, `yesterday`).
    fn day_range_start(&self) -> Option<DateTime<FixedOffset>> {
        let now = self.server_now();
        let today = now
            .date_naive()
            .and_hms_opt(0, 0, 0)?
            .and_local_timezone(*now.offset())
            .single()?;
        match self.range {
            TimeRange::Today => Some(today),
            TimeRange::Yesterday => Some(today - chrono::Duration::days(1)),
            _ => None,
        }
    }

    pub fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Auth(e) => self.on_auth(e),
            Msg::Live(e) => self.on_live(e),
            Msg::UpdateAvailable(tag) => self.update_available = Some(tag),
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
                self.server_edit = None;
                if let Some(n) = self.server_notice.take() {
                    self.flash(n);
                } else if let Some(w) = self.warnings.first().cloned() {
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
        // Not a choice of the user's: a command-line range stays unsaved.
        self.apply_range(r);
        self.flash(format!("range moved out of bounds — now {}", r.label()));
        true
    }

    fn on_data(&mut self, group: Group, scope: String, payload: Payload) {
        // Drop results for parameters that are no longer current.
        if group != Group::Detail && scope != self.query_scope(group) {
            return;
        }
        let now = self.now();
        let start = self.day_range_start();
        let d = &mut self.data;
        let err: Option<ApiError> = match payload {
            Payload::Overview(r) => d.overview.apply(
                r.map(|mut o| {
                    if let Some(s) = start {
                        crate::series::clip_first(&mut o.series.buckets, s);
                    }
                    o
                }),
                now,
            ),
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
                let r = r.map(|mut c| {
                    if let Some(s) = start {
                        crate::series::clip_first(&mut c.buckets, s);
                    }
                    c
                });
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
                let utc = self.clock.utc();
                let r = r.map(|mut a| {
                    crate::series::densify_analysis(&mut a, utc);
                    a
                });
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
            Payload::KeyUsage(r) => d.key_usage.apply(
                r.map(|mut c| {
                    if let Some(s) = start {
                        crate::series::clip_first(&mut c.buckets, s);
                    }
                    c
                }),
                now,
            ),
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
            // A late answer for a credential no longer selected is dropped.
            Payload::IdentityErrors(id, r)
                if d.identity_errors.as_ref().is_none_or(|(cur, _)| *cur == id) =>
            {
                let mut l = Loaded::default();
                let e = l.apply(r, now);
                d.identity_errors = Some((id, l));
                e
            }
            Payload::IdentityErrors(..) => None,
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
        // Before the next frame, so a restored empty dimension never shows.
        if !self.dims_unchecked.is_empty() {
            self.settle_dims();
        }
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
            Tab::Credentials => &[Group::Credentials, Group::Detail, Group::Quota],
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

    /// How the current tab's data is refreshed (shown in the header).
    pub fn cadence(&self) -> Cadence {
        if self.tab == Tab::Events {
            return Cadence::Live;
        }
        let q = self.query();
        match self.tab_groups().first().and_then(|g| q.interval(*g)) {
            Some(d) => Cadence::Every(d),
            None => Cadence::OnDemand,
        }
    }

    /// Whether data fetched at `at` is older than its cadence allows (a few
    /// refreshes were missed).
    pub fn is_stale(&self, at: Option<Instant>) -> Option<u64> {
        let Cadence::Every(d) = self.cadence() else {
            return None;
        };
        let age = self.now().saturating_duration_since(at?);
        let limit = (d * 5 / 2).max(Duration::from_secs(30));
        (age > limit).then_some(age.as_secs())
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
}
