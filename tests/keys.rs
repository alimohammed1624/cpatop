//! Key-event tests: drive `App::on_event` and check state and effects.
//! (The `Esc`-quits bug would have been caught here.)

use chrono::{DateTime, Utc};
use cpatop::api::{LoginMode, Role};
use cpatop::app::{App, Effect, Overlay, PickKind, ResultFilter, Screen};
use cpatop::auth::{AuthCmd, AuthEvent};
use cpatop::config::{Settings, UiState};
use cpatop::layout::{PaneId, Tab};
use cpatop::poller::{Group, Msg, Payload};
use cpatop::range::{Custom, TimeRange};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use serde::de::DeserializeOwned;
use std::path::Path;
use std::time::Instant;

fn load<T: DeserializeOwned>(name: &str) -> T {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn data(app: &mut App, group: Group, payload: Payload) {
    let mut q = app.query();
    q.role = app.role;
    let scope = q.scope(group);
    app.on_msg(Msg::Data {
        group,
        scope,
        payload,
    });
}

fn admin() -> App {
    let mut app = App::new(
        Settings::default(),
        cpatop::theme::all_themes(None).0,
        &UiState::default(),
    );
    let now: DateTime<Utc> = "2026-11-13T11:15:00Z".parse().unwrap();
    app.clock.frozen = Some((Instant::now(), now));
    app.on_msg(Msg::Auth(AuthEvent::LoggedIn {
        role: Role::Admin,
        session: load("auth-session-admin.json"),
        epoch: 1,
    }));
    data(
        &mut app,
        Group::Overview,
        Payload::Overview(Ok(load("usage-overview.json"))),
    );
    data(
        &mut app,
        Group::Overview,
        Payload::Comparisons(Ok(load("usage-overview-comparisons.json"))),
    );
    data(
        &mut app,
        Group::KeyOptions,
        Payload::KeyOptions(Ok(load("usage-api-keys-options.json"))),
    );
    data(
        &mut app,
        Group::KeyOptions,
        Payload::ApiKeys(Ok(load("usage-api-keys.json"))),
    );
    data(
        &mut app,
        Group::EventFilters,
        Payload::EventModels(Ok(load("usage-events-filters-models.json"))),
    );
    data(
        &mut app,
        Group::EventFilters,
        Payload::EventSources(Ok(load("usage-events-filters-sources.json"))),
    );
    draw(&mut app);
    app
}

fn draw(app: &mut App) {
    let mut term = Terminal::new(TestBackend::new(160, 48)).unwrap();
    term.draw(|f| cpatop::ui::draw(f, app)).unwrap();
}

fn press(app: &mut App, code: KeyCode) {
    app.on_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    draw(app);
}

fn ctrl(app: &mut App, c: char) {
    app.on_event(Event::Key(KeyEvent::new(
        KeyCode::Char(c),
        KeyModifiers::CONTROL,
    )));
}

fn typed(app: &mut App, s: &str) {
    for c in s.chars() {
        press(app, KeyCode::Char(c));
    }
}

#[test]
fn esc_never_quits() {
    let mut app = admin();
    for _ in 0..3 {
        press(&mut app, KeyCode::Esc);
        assert!(!app.should_quit, "Esc at the top level must not quit");
    }
    // Not on the login screen either; Ctrl-C does quit there.
    app.on_msg(Msg::Auth(AuthEvent::NeedLogin {
        error: None,
        mode: LoginMode::Admin,
    }));
    typed(&mut app, "secret");
    press(&mut app, KeyCode::Esc);
    assert!(!app.should_quit);
    assert_eq!(app.login_form().unwrap().input, "", "Esc clears the field");
    ctrl(&mut app, 'c');
    assert!(app.should_quit);
}

#[test]
fn q_and_ctrl_c_quit() {
    let mut app = admin();
    press(&mut app, KeyCode::Char('q'));
    assert!(app.should_quit);
    let mut app = admin();
    ctrl(&mut app, 'c');
    assert!(app.should_quit);
}

#[test]
fn esc_closes_the_innermost_thing() {
    let mut app = admin();
    press(&mut app, KeyCode::Char('?'));
    assert_eq!(app.overlay, Some(Overlay::Help));
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.overlay, None);

    // A table filter: `/` types into it, Enter keeps it, Esc clears it.
    press(&mut app, KeyCode::Char('/'));
    typed(&mut app, "opus");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.table(PaneId::OvDist).filter, "opus");
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.table(PaneId::OvDist).filter, "");
    assert!(!app.should_quit);
}

#[test]
fn graph_cursor_takes_arrows_when_a_graph_is_focused() {
    let mut app = admin();
    // Focus moves from the distribution table to the trend graph.
    press(&mut app, KeyCode::Char('f'));
    assert_eq!(app.focused(), Some(PaneId::OvTrend));
    press(&mut app, KeyCode::Left);
    assert_eq!(app.tab, Tab::Overview, "← moves the cursor, not the tab");
    let last = *app.cursor.get(&PaneId::OvTrend).unwrap();
    press(&mut app, KeyCode::Left);
    assert_eq!(app.cursor[&PaneId::OvTrend], last - 1);
    press(&mut app, KeyCode::Char('g'));
    assert_eq!(app.cursor[&PaneId::OvTrend], 0);
    press(&mut app, KeyCode::Esc);
    assert!(!app.cursor.contains_key(&PaneId::OvTrend));
    // Tab still switches tabs.
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.tab, Tab::Realtime);
}

#[test]
fn request_filters_are_server_side_parameters() {
    let mut app = admin();
    app.tab = Tab::Events;
    draw(&mut app);
    assert!(app.live_params().filters.is_empty());
    press(&mut app, KeyCode::Char('e'));
    press(&mut app, KeyCode::Char('e'));
    assert_eq!(app.live_params().filters.result, ResultFilter::Failed);

    press(&mut app, KeyCode::Char('m'));
    typed(&mut app, "luna");
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.live_params().filters.model.as_deref(),
        Some("gpt-6-luna")
    );

    press(&mut app, KeyCode::Char('o'));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    let src = app.live_params().filters.source.clone().unwrap();
    assert_eq!(src.0, "92799d3c51327a24");

    // Changing filters restarts the log; the scope changes so stale history
    // pages are dropped.
    let scope = app.events_scope();
    press(&mut app, KeyCode::Char('c'));
    assert!(app.live_params().filters.is_empty());
    assert_ne!(app.events_scope(), scope);
}

#[test]
fn key_picker_filters_by_id() {
    let mut app = admin();
    press(&mut app, KeyCode::Char('a'));
    let Some(Overlay::Picker(p)) = &app.overlay else {
        panic!("picker expected");
    };
    assert_eq!(p.kind, PickKind::ApiKey);
    let labels: Vec<&str> = p.items.iter().map(|i| i.label.as_str()).collect();
    // Two keys share the masked prefix "dem…"; ids tell them apart.
    assert_eq!(labels, vec!["all keys", "#1 dem…", "#2 dem…", "#3 tst…"]);
    typed(&mut app, "#2");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.query().api_key_id.as_deref(), Some("2"));
    // Esc cancels without changing the filter.
    press(&mut app, KeyCode::Char('a'));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.query().api_key_id.as_deref(), Some("2"));
}

#[test]
fn range_picker_with_custom_days() {
    let mut app = admin();
    app.server_offset = Some(chrono::FixedOffset::east_opt(5 * 3600 + 1800).unwrap());
    press(&mut app, KeyCode::Char('r'));
    typed(&mut app, "custom d");
    press(&mut app, KeyCode::Enter);
    assert!(matches!(app.overlay, Some(Overlay::Range(_))));
    // A range ending tomorrow is rejected in place.
    ctrl(&mut app, 'u');
    typed(&mut app, "11-01");
    press(&mut app, KeyCode::Tab);
    ctrl(&mut app, 'u');
    typed(&mut app, "2026-11-14");
    press(&mut app, KeyCode::Enter);
    match &app.overlay {
        Some(Overlay::Range(ed)) => assert!(ed.error.as_deref().unwrap().contains("future")),
        o => panic!("editor should stay open: {o:?}"),
    }
    ctrl(&mut app, 'u');
    typed(&mut app, "11-13");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.overlay, None);
    let expect: TimeRange = "custom:2026-11-01..2026-11-13".parse().unwrap();
    assert_eq!(app.range, expect);
    assert!(matches!(app.range, TimeRange::Custom(Custom::Days { .. })));
    // `]` leaves the custom range for the presets.
    press(&mut app, KeyCode::Char(']'));
    assert_eq!(app.range, TimeRange::Today);
}

#[test]
fn logout_needs_a_second_x() {
    let mut app = admin();
    press(&mut app, KeyCode::Char('X'));
    assert!(app.logout_countdown().is_some());
    assert!(
        app.drain_effects()
            .iter()
            .all(|e| !matches!(e, Effect::Auth(_)))
    );
    // Any other key cancels.
    press(&mut app, KeyCode::Char('j'));
    assert!(app.logout_countdown().is_none());
    press(&mut app, KeyCode::Char('X'));
    press(&mut app, KeyCode::Char('X'));
    assert!(
        app.drain_effects()
            .iter()
            .any(|e| matches!(e, Effect::Auth(AuthCmd::Logout)))
    );
}

#[test]
fn non_dashboard_screens_ignore_esc() {
    let mut app = admin();
    app.screen = Screen::Connecting;
    press(&mut app, KeyCode::Esc);
    assert!(!app.should_quit);
    press(&mut app, KeyCode::Char('q'));
    assert!(app.should_quit);
}
