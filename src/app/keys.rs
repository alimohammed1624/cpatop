//! Key and mouse bindings.
//!
//! Quitting is always explicit: `q` (outside text fields) or Ctrl-C. `Esc`
//! closes the innermost thing — an overlay, a filter prompt, a graph cursor,
//! a table filter — and does nothing at the top level.

use super::overlay::{Overlay, PickItem, PickKind, Picker, RangeEditor, Unit};
use super::{App, Effect, InputMode, LoginForm, Screen};
use crate::api::models::ApiKeyOption;
use crate::api::{LoginMode, Role};
use crate::auth::AuthCmd;
use crate::config::step_refresh;
use crate::layout::{self, PaneId, Tab};
use crate::range::{PRESETS, TimeRange};
use crate::secrets::Secret;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Position;

impl App {
    pub fn on_event(&mut self, ev: Event) {
        match ev {
            Event::Key(k) if k.kind != KeyEventKind::Release => self.on_key(k),
            Event::Mouse(m) if self.settings.mouse => self.on_mouse(m),
            _ => {}
        }
    }

    pub fn on_key(&mut self, k: KeyEvent) {
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
                KeyCode::Char('q') => self.should_quit = true,
                KeyCode::Char('r') | KeyCode::Enter => {
                    self.effects.push(Effect::Auth(AuthCmd::RetryNow))
                }
                KeyCode::Char('t') => self.cycle_theme(true),
                _ => {}
            }
            return;
        }
        // Terminals send `Esc` followed quickly by a key as one sequence that
        // arrives as Alt+key, which would be typed into a picker query (the
        // picker stays open and the key is lost). No binding uses Alt, so
        // handle it as the two keys it most likely was.
        if k.modifiers.contains(KeyModifiers::ALT)
            && !k.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(k.code, KeyCode::Char(_))
        {
            self.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            let mut k = k;
            k.modifiers.remove(KeyModifiers::ALT);
            self.on_key(k);
            return;
        }
        if let InputMode::Filter(id) = self.mode {
            self.on_filter_key(id, k);
            return;
        }
        if self.overlay.is_some() {
            self.on_overlay_key(k);
            return;
        }
        self.on_dashboard_key(k);
    }

    fn on_login_key(&mut self, k: KeyEvent) {
        let Screen::Login(form) = &mut self.screen else {
            return;
        };
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            // Esc clears the field; it never quits (Ctrl-C does).
            KeyCode::Esc => {
                form.input.clear();
                form.error = None;
            }
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
            KeyCode::Char('u') if ctrl => form.input.clear(),
            KeyCode::Char(c) if !ctrl && form.input.len() < 1024 => form.input.push(c),
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

    // ------------------------------------------------------------ overlays

    fn on_overlay_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let now = self.server_now();
        match self.overlay.as_mut() {
            // Row details scroll like the help; `[` `]` (or Shift+↑↓) step
            // through the table's rows underneath.
            Some(Overlay::Row(id)) => {
                let id = *id;
                let page = self.row_page.get().max(1) as isize;
                let shift = k.modifiers.contains(KeyModifiers::SHIFT);
                match k.code {
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => self.overlay = None,
                    KeyCode::Up if shift => self.step_row(id, -1),
                    KeyCode::Down if shift => self.step_row(id, 1),
                    KeyCode::Char('[') | KeyCode::Char('K') => self.step_row(id, -1),
                    KeyCode::Char(']') | KeyCode::Char('J') => self.step_row(id, 1),
                    KeyCode::Down | KeyCode::Char('j') => self.scroll_row(1),
                    KeyCode::Up | KeyCode::Char('k') => self.scroll_row(-1),
                    KeyCode::PageDown | KeyCode::Char(' ') => self.scroll_row(page),
                    KeyCode::PageUp => self.scroll_row(-page),
                    KeyCode::Home | KeyCode::Char('g') => self.row_scroll = 0,
                    KeyCode::End | KeyCode::Char('G') => self.scroll_row(isize::MAX / 2),
                    _ => {}
                }
            }
            // Help scrolls; only Esc, ? and q close it (q never quits here).
            Some(Overlay::Help) => {
                let page = self.help_page.get().max(1) as isize;
                match k.code {
                    KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') | KeyCode::F(1) => {
                        self.overlay = None
                    }
                    KeyCode::Down | KeyCode::Char('j') => self.scroll_help(1),
                    KeyCode::Up | KeyCode::Char('k') => self.scroll_help(-1),
                    KeyCode::PageDown | KeyCode::Char(' ') => self.scroll_help(page),
                    KeyCode::PageUp => self.scroll_help(-page),
                    KeyCode::Home | KeyCode::Char('g') => self.help_scroll = 0,
                    KeyCode::End | KeyCode::Char('G') => self.scroll_help(isize::MAX / 2),
                    _ => {}
                }
            }
            Some(Overlay::Picker(p)) => match k.code {
                KeyCode::Esc => self.overlay = None,
                KeyCode::Enter => {
                    let kind = p.kind;
                    let chosen = p.chosen().cloned();
                    self.overlay = None;
                    if let Some(item) = chosen {
                        self.apply_pick(kind, item);
                    }
                }
                KeyCode::Up => p.move_by(-1),
                KeyCode::Down => p.move_by(1),
                KeyCode::Char('p') if ctrl => p.move_by(-1),
                KeyCode::Char('n') if ctrl => p.move_by(1),
                KeyCode::PageUp => p.move_by(-10),
                KeyCode::PageDown => p.move_by(10),
                KeyCode::Home => p.move_by(-10_000),
                KeyCode::End => p.move_by(10_000),
                KeyCode::Char('u') if ctrl => {
                    p.query.clear();
                    p.selected = 0;
                }
                KeyCode::Backspace => p.backspace(),
                KeyCode::Char(c) if !ctrl => p.type_char(c),
                _ => {}
            },
            Some(Overlay::Range(ed)) => match k.code {
                KeyCode::Esc => self.open_range_picker(),
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => {
                    ed.field = 1 - ed.field;
                    ed.error = None;
                }
                KeyCode::Enter => match ed.build(now) {
                    Ok(c) => {
                        self.overlay = None;
                        self.set_range(TimeRange::Custom(c));
                        self.flash(format!("range {}", TimeRange::Custom(c).label()));
                    }
                    Err(e) => ed.error = Some(e),
                },
                KeyCode::Backspace => {
                    ed.field_mut().pop();
                    ed.error = None;
                }
                KeyCode::Char('u') if ctrl => {
                    ed.field_mut().clear();
                    ed.error = None;
                }
                KeyCode::Char(c) if !ctrl && ed.field_mut().len() < 32 => {
                    ed.field_mut().push(c);
                    ed.error = None;
                }
                _ => {}
            },
            None => {}
        }
    }

    fn apply_pick(&mut self, kind: PickKind, item: PickItem) {
        match kind {
            PickKind::ApiKey => {
                let k = item.value.map(|id| ApiKeyOption {
                    label: self
                        .data
                        .key_options
                        .get()
                        .and_then(|o| o.options.iter().find(|o| o.id == id))
                        .map(|o| o.label.clone())
                        .unwrap_or_default(),
                    id,
                });
                self.set_api_key(k);
            }
            PickKind::Model => {
                let mut f = self.live.filters.clone();
                f.model = item.value;
                self.set_event_filters(f);
            }
            PickKind::Source => {
                let mut f = self.live.filters.clone();
                f.source = item.value.map(|v| (v, item.label.clone()));
                self.set_event_filters(f);
            }
            PickKind::Range => match item.value.as_deref() {
                Some("custom-days") => self.open_range_editor(Unit::Day),
                Some("custom-hours") => self.open_range_editor(Unit::Hour),
                Some(v) => {
                    if let Ok(r) = v.parse::<TimeRange>() {
                        self.set_range(r);
                    }
                }
                None => {}
            },
        }
    }

    pub(crate) fn open_key_picker(&mut self) {
        if self.role != Some(Role::Admin) {
            self.flash("the API-key filter is for admin sessions");
            return;
        }
        let Some(opts) = self.data.key_options.get() else {
            self.flash("API keys are still loading");
            return;
        };
        let usage = self.data.comparisons.get().map(|c| &c.api_keys);
        let mut items = vec![PickItem::new(None, "all keys", "no API-key filter")];
        for o in &opts.options {
            let detail = match usage.and_then(|u| u.iter().find(|c| c.key == o.id)) {
                Some(c) => format!(
                    "{} tok · {} req ({})",
                    crate::fmt::compact(c.total_tokens),
                    crate::fmt::count(c.requests),
                    self.range.label()
                ),
                None => String::new(),
            };
            items.push(PickItem::new(
                Some(o.id.clone()),
                self.key_name(&o.id, &o.label),
                detail,
            ));
        }
        let cur = self.api_key.as_ref().map(|k| k.id.clone());
        for it in &mut items {
            it.current = it.value == cur;
        }
        self.overlay = Some(Overlay::Picker(Picker::new(
            PickKind::ApiKey,
            "API key filter",
            items,
        )));
    }

    fn open_model_picker(&mut self) {
        let Some(list) = self.data.event_models.get() else {
            self.flash("model list is still loading");
            return;
        };
        let mut items = vec![PickItem::new(None, "all models", "")];
        items.extend(
            list.models
                .iter()
                .map(|m| PickItem::new(Some(m.clone()), m.clone(), "")),
        );
        let cur = self.live.filters.model.clone();
        for it in &mut items {
            it.current = it.value == cur;
        }
        self.overlay = Some(Overlay::Picker(Picker::new(
            PickKind::Model,
            "model filter",
            items,
        )));
    }

    fn open_source_picker(&mut self) {
        let Some(list) = self.data.event_sources.get() else {
            self.flash("source list is still loading");
            return;
        };
        let mut items = vec![PickItem::new(None, "all sources", "")];
        for s in &list.sources {
            let label = s
                .display_name
                .clone()
                .filter(|d| !d.is_empty())
                .unwrap_or_else(|| s.label.clone());
            let label = if label.is_empty() {
                s.value.clone()
            } else {
                self.credential_name(&s.value, &label)
            };
            // Several sources can share a label; the identity tells them apart
            // (privacy mode: the label already does, without the server id).
            let short: String = if self.settings.privacy {
                String::new()
            } else {
                s.value.chars().take(8).collect()
            };
            items.push(PickItem::new(
                Some(s.value.clone()),
                label,
                if short.is_empty() {
                    String::new()
                } else {
                    format!("#{short}")
                },
            ));
        }
        let cur = self.live.filters.source.as_ref().map(|s| s.0.clone());
        for it in &mut items {
            it.current = it.value == cur;
        }
        self.overlay = Some(Overlay::Picker(Picker::new(
            PickKind::Source,
            "source filter",
            items,
        )));
    }

    pub(crate) fn open_range_picker(&mut self) {
        let mut items: Vec<PickItem> = PRESETS
            .iter()
            .map(|r| {
                let detail = match r {
                    TimeRange::Today => "since local midnight",
                    TimeRange::Yesterday => "the previous calendar day",
                    TimeRange::Hours(_) => "rolling hours",
                    TimeRange::Days(_) => "rolling days",
                    TimeRange::Custom(_) => "",
                };
                let mut it = PickItem::new(Some(r.to_string()), r.label(), detail);
                it.current = *r == self.range;
                it
            })
            .collect();
        let max = self.max_custom_days();
        let mut days = PickItem::new(
            Some("custom-days".into()),
            "custom days…",
            format!("any 1–{max} days"),
        );
        let mut hours = PickItem::new(
            Some("custom-hours".into()),
            "custom hours…",
            "5–24 hours within the last day",
        );
        if let TimeRange::Custom(c) = self.range {
            match c {
                crate::range::Custom::Days { .. } => days.current = true,
                crate::range::Custom::Hours { .. } => hours.current = true,
            }
            let label = format!("{} (current)", self.range.label());
            match c {
                crate::range::Custom::Days { .. } => days.detail = label,
                crate::range::Custom::Hours { .. } => hours.detail = label,
            }
        }
        items.push(days);
        items.push(hours);
        self.overlay = Some(Overlay::Picker(Picker::new(
            PickKind::Range,
            "time range",
            items,
        )));
    }

    fn open_range_editor(&mut self, unit: Unit) {
        let ed = RangeEditor::new(unit, self.range, self.server_now(), self.max_custom_days());
        self.overlay = Some(Overlay::Range(ed));
    }

    // ------------------------------------------------------------ dashboard

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

    /// Scroll the help overlay, clamped to its content (known after a draw).
    fn scroll_help(&mut self, delta: isize) {
        let max = self.help_max.get() as isize;
        self.help_scroll = (self.help_scroll as isize)
            .saturating_add(delta)
            .clamp(0, max) as usize;
    }

    /// Scroll the row popup, clamped to its content (known after a draw).
    fn scroll_row(&mut self, delta: isize) {
        let max = self.row_max.get() as isize;
        self.row_scroll = (self.row_scroll as isize)
            .saturating_add(delta)
            .clamp(0, max) as usize;
    }

    /// Show the previous / next row in the popup, from the top.
    fn step_row(&mut self, id: PaneId, delta: isize) {
        self.move_sel(id, delta);
        self.row_scroll = 0;
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

    /// Number of buckets of a drawn graph pane.
    fn graph_len(&self, id: PaneId) -> Option<usize> {
        self.hit
            .graphs
            .iter()
            .find(|(g, _, _)| *g == id)
            .map(|(_, _, n)| *n)
    }

    /// Move the graph cursor; the first press lands on the latest bucket
    /// with data (`g`/`G` still reach the ends).
    fn move_cursor(&mut self, id: PaneId, delta: isize) {
        let Some(n) = self.graph_len(id).filter(|n| *n > 0) else {
            return;
        };
        let next = match self.cursor.get(&id) {
            None if delta.abs() > 1 => {
                if delta < 0 {
                    0
                } else {
                    n - 1
                }
            }
            None => self
                .hit
                .graph_last
                .get(&id)
                .copied()
                .unwrap_or(n - 1)
                .min(n - 1),
            Some(c) => ((*c).min(n - 1) as isize + delta).clamp(0, n as isize - 1) as usize,
        };
        self.cursor.insert(id, next);
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

    /// Cycle focus through the interactive panes (tables and graphs).
    fn cycle_focus(&mut self, forward: bool) {
        let visible: Vec<PaneId> = self
            .hit
            .panes
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| {
                let d = layout::pane_def(*id).2;
                d.table || d.graph
            })
            .collect();
        if visible.is_empty() {
            return;
        }
        let n = visible.len();
        let cur = self.focused();
        let i = cur
            .and_then(|c| visible.iter().position(|v| *v == c))
            .map(|i| {
                if forward {
                    (i + 1) % n
                } else {
                    (i + n - 1) % n
                }
            })
            .unwrap_or(0);
        self.focus.insert(self.tab, visible[i]);
    }

    /// Esc at the dashboard level: undo the innermost state, never quit.
    fn escape(&mut self, focused: Option<PaneId>) {
        if let Some(f) = focused {
            if self.cursor.remove(&f).is_some() {
                return;
            }
            if !self.table(f).filter.is_empty() {
                let st = self.table_mut(f);
                st.filter.clear();
                st.selected = 0;
                return;
            }
        }
        if self.tab == Tab::Events && !self.live.filters.is_empty() {
            self.set_event_filters(Default::default());
            self.flash("request filters cleared");
            return;
        }
        self.flash("q quits · ? help");
    }

    fn on_dashboard_key(&mut self, k: KeyEvent) {
        // Any key other than a second X disarms the logout confirmation.
        if k.code != KeyCode::Char('X') && self.logout_armed.take().is_some() {
            self.flash("logout cancelled");
            if k.code == KeyCode::Esc {
                return;
            }
        }
        let focused = self.focused();
        let graph_focus = focused.filter(|f| layout::pane_def(*f).2.graph);
        let page = focused
            .map(|f| self.table(f).page.get().max(1) as isize)
            .unwrap_or(10);
        let admin = self.role == Some(Role::Admin);
        match k.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc => self.escape(focused),
            KeyCode::Char('?') | KeyCode::F(1) => {
                self.help_scroll = 0;
                self.overlay = Some(Overlay::Help)
            }
            KeyCode::Enter if focused.is_some_and(|f| self.hit.row_fields.contains_key(&f)) => {
                self.row_scroll = 0;
                self.overlay = focused.map(Overlay::Row);
            }
            KeyCode::Left if graph_focus.is_some() => {
                self.move_cursor(graph_focus.unwrap_or(PaneId::OvTrend), -1)
            }
            KeyCode::Right if graph_focus.is_some() => {
                self.move_cursor(graph_focus.unwrap_or(PaneId::OvTrend), 1)
            }
            KeyCode::Tab | KeyCode::Right => self.step_tab(true),
            KeyCode::BackTab | KeyCode::Left => self.step_tab(false),
            KeyCode::F(n) if (n as usize) >= 2 && (n as usize) <= self.tabs().len() + 1 => {
                self.set_tab(self.tabs()[n as usize - 2])
            }
            KeyCode::Char(c @ '1'..='9') => self.toggle_pane(c as usize - '0' as usize),
            KeyCode::Char('f') => self.cycle_focus(true),
            KeyCode::Char('F') => self.cycle_focus(false),
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
            KeyCode::Home | KeyCode::Char('g') => match graph_focus {
                Some(g) => self.move_cursor(g, -100_000),
                None => {
                    if let Some(f) = focused {
                        self.table_mut(f).selected = 0
                    }
                }
            },
            KeyCode::End | KeyCode::Char('G') => match graph_focus {
                Some(g) => self.move_cursor(g, 100_000),
                None => {
                    if let Some(f) = focused {
                        let len = self.table(f).len.get();
                        self.table_mut(f).selected = len.saturating_sub(1);
                    }
                }
            },
            KeyCode::Char('s') => {
                if let Some(f) = focused.filter(|f| layout::pane_def(*f).2.table) {
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
                if let Some(f) = focused.filter(|f| layout::pane_def(*f).2.table) {
                    let st = self.table_mut(f);
                    st.sort = Some(match st.sort {
                        None => (0, false),
                        Some((c, d)) => (c, !d),
                    });
                    self.effects.push(Effect::SaveState);
                }
            }
            KeyCode::Char('/') => {
                if let Some(f) = focused.filter(|f| layout::pane_def(*f).2.table) {
                    self.mode = InputMode::Filter(f);
                }
            }
            KeyCode::Char('r') => self.open_range_picker(),
            KeyCode::Char(']') => self.set_range(self.range.step(true)),
            KeyCode::Char('[') => self.set_range(self.range.step(false)),
            KeyCode::Char('w') => {
                self.window = self.window.next();
                self.data.realtime = Default::default();
                self.nonce += 1;
                self.flash(format!("realtime window {}", self.window.param()));
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
                    .filter(|f| !super::dims_for(*f, self.role).is_empty())
                    .or_else(|| {
                        self.hit
                            .panes
                            .iter()
                            .map(|(id, _)| *id)
                            .find(|id| !super::dims_for(*id, self.role).is_empty())
                    });
                if let Some(id) = target {
                    let n = super::dims_for(id, self.role).len();
                    let next = (self.dim(id) + 1) % n;
                    self.dims.insert(id, next);
                    let st = self.table_mut(id);
                    st.selected = 0;
                    st.filter.clear();
                    self.flash(format!("showing {}", super::dims_for(id, self.role)[next]));
                    self.effects.push(Effect::SaveState);
                }
            }
            KeyCode::Char('a') => self.open_key_picker(),
            KeyCode::Char('e') if self.tab == Tab::Events => {
                let mut f = self.live.filters.clone();
                f.result = f.result.next();
                self.set_event_filters(f);
            }
            KeyCode::Char('m') if self.tab == Tab::Events && admin => self.open_model_picker(),
            KeyCode::Char('o') if self.tab == Tab::Events && admin => self.open_source_picker(),
            KeyCode::Char('c') if self.tab == Tab::Events => {
                if !self.live.filters.is_empty() {
                    self.set_event_filters(Default::default());
                    self.flash("request filters cleared");
                }
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
            KeyCode::Char('R') => {
                self.manual += 1;
                if self.tab == Tab::Events {
                    // The live stream reconnects (fresh snapshot) because its
                    // parameters include the manual-refresh counter.
                    self.live.reset();
                }
                self.flash("refreshing");
            }
            KeyCode::Char('X') => {
                if self.logout_armed.take().is_some() {
                    self.effects.push(Effect::Auth(AuthCmd::Logout));
                } else {
                    self.logout_armed = Some(self.now());
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
                if let Some(Overlay::Picker(p)) = &mut self.overlay {
                    let hit = self.hit.picker.iter().find(|(_, r)| r.contains(pos));
                    match hit {
                        Some((idx, _)) => {
                            let idx = *idx;
                            let vis = p.visible();
                            if let Some(sel) = vis.iter().position(|i| *i == idx) {
                                p.selected = sel;
                                let kind = p.kind;
                                let item = p.items[idx].clone();
                                self.overlay = None;
                                self.apply_pick(kind, item);
                            }
                        }
                        None => self.overlay = None,
                    }
                    return;
                }
                if self.overlay.is_some() {
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
                    let def = layout::pane_def(id).2;
                    if def.table || def.graph {
                        self.focus.insert(self.tab, id);
                    }
                    if let Some((_, r, n)) = self
                        .hit
                        .graphs
                        .iter()
                        .find(|(g, r, _)| *g == id && r.contains(pos))
                        .copied()
                        && n > 0
                        && r.width > 0
                    {
                        let i = ((m.column - r.x) as usize * n / r.width as usize).min(n - 1);
                        self.cursor.insert(id, i);
                        return;
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
                let down = m.kind == MouseEventKind::ScrollDown;
                if let Some(Overlay::Picker(p)) = &mut self.overlay {
                    p.move_by(if down { 1 } else { -1 });
                    return;
                }
                if matches!(self.overlay, Some(Overlay::Help)) {
                    self.scroll_help(if down { 3 } else { -3 });
                    return;
                }
                if matches!(self.overlay, Some(Overlay::Row(_))) {
                    self.scroll_row(if down { 3 } else { -3 });
                    return;
                }
                if self.overlay.is_some() {
                    return;
                }
                let delta = if down { 3 } else { -3 };
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

/// Convenience for tests: the login form, if shown.
impl App {
    pub fn login_form(&self) -> Option<&LoginForm> {
        match &self.screen {
            Screen::Login(f) => Some(f),
            _ => None,
        }
    }
}
