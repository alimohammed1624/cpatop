//! Snapshot tests of the main screens, rendered from recorded fixtures with
//! ratatui's `TestBackend` at 80x24, 160x48 and 220x60 in both roles.

use chrono::{DateTime, Utc};
use cpatop::api::Role;
use cpatop::api::models::*;
use cpatop::api::sse::LiveEvent;
use cpatop::app::{App, Screen};
use cpatop::auth::AuthEvent;
use cpatop::config::{Settings, UiState};
use cpatop::layout::{PaneId, Tab};
use cpatop::poller::{Group, Msg, Payload};
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

fn frozen_now() -> DateTime<Utc> {
    "2026-11-13T11:15:00Z".parse().unwrap()
}

fn new_app() -> App {
    let settings = Settings::default();
    let themes = cpatop::theme::all_themes(None).0;
    let mut app = App::new(settings, themes, &UiState::default());
    app.clock.frozen = Some((Instant::now(), frozen_now()));
    app
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

fn login(app: &mut App, role: Role) {
    let session: SessionResponse = load(match role {
        Role::Admin => "auth-session-admin.json",
        Role::KeyViewer => "auth-session-key.json",
    });
    app.on_msg(Msg::Auth(AuthEvent::LoggedIn {
        role,
        session,
        epoch: 1,
    }));
}

fn populated(role: Role) -> App {
    let mut app = new_app();
    login(&mut app, role);
    let admin = role == Role::Admin;
    let pick = |a: &'static str, k: &'static str| if admin { a } else { k };
    data(
        &mut app,
        Group::Overview,
        Payload::Overview(Ok(load(pick("usage-overview.json", "key-overview.json")))),
    );
    data(
        &mut app,
        Group::Overview,
        Payload::Activity(Ok(load(pick("usage-activity.json", "key-activity.json")))),
    );
    data(
        &mut app,
        Group::Overview,
        Payload::Comparisons(Ok(load(pick(
            "usage-overview-comparisons.json",
            "key-overview-comparisons.json",
        )))),
    );
    data(
        &mut app,
        Group::Realtime,
        Payload::Realtime(Ok(load(pick(
            "usage-overview-realtime.json",
            "key-overview-realtime.json",
        )))),
    );
    data(
        &mut app,
        Group::Analysis,
        Payload::Analysis(Ok(load(pick("usage-analysis.json", "key-analysis.json")))),
    );
    data(
        &mut app,
        Group::Analysis,
        Payload::Latency(Ok(load(pick(
            "usage-analysis-latency.json",
            "key-analysis-latency.json",
        )))),
    );
    data(
        &mut app,
        Group::Meta,
        Payload::Version(Ok(load("version.json"))),
    );
    if admin {
        data(
            &mut app,
            Group::Meta,
            Payload::Status(Ok(load("status.json"))),
        );
        data(
            &mut app,
            Group::KeyOptions,
            Payload::KeyOptions(Ok(load("usage-api-keys-options.json"))),
        );
        data(
            &mut app,
            Group::Credentials,
            Payload::Identities(Ok(load("usage-identities-page.json"))),
        );
        data(
            &mut app,
            Group::Quota,
            Payload::Inspection(Ok(load("quota-inspection.json"))),
        );
        data(
            &mut app,
            Group::Quota,
            Payload::AutoRefresh(Ok(load("quota-auto-refresh-settings.json"))),
        );
        let codex: QuotaRefreshTask = load("quota-refresh-task.json");
        let claude: QuotaRefreshTask = load("quota-refresh-task-claude.json");
        data(
            &mut app,
            Group::Quota,
            Payload::QuotaTasks(vec![
                (codex.auth_index.clone(), Ok(codex)),
                (claude.auth_index.clone(), Ok(claude)),
            ]),
        );
        data(
            &mut app,
            Group::Models,
            Payload::Catalog(Ok(load("models.json"))),
        );
        data(
            &mut app,
            Group::Models,
            Payload::Pricing(Ok(load("pricing-models.json"))),
        );
        data(
            &mut app,
            Group::Models,
            Payload::RecalcCurrent(Ok(load("pricing-recalculations-current.json"))),
        );
        data(
            &mut app,
            Group::Models,
            Payload::RecalcOptions(Ok(load("pricing-recalculations-options.json"))),
        );
        data(
            &mut app,
            Group::KeyOptions,
            Payload::ApiKeys(Ok(load("usage-api-keys.json"))),
        );
        data(
            &mut app,
            Group::KeyUsage,
            Payload::KeyUsage(Ok(load("usage-overview-comparisons.json"))),
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
        data(
            &mut app,
            Group::Settings,
            Payload::Sessions(Ok(load("auth-sessions.json"))),
        );
        let page: EventsPage = load("usage-events.json");
        app.on_msg(Msg::Live(LiveEvent::Snapshot(Box::new(LiveSnapshot {
            events: page.events,
            total_count: page.total_count,
            has_more: false,
            next_cursor: None,
            range_start: "2026-11-13T00:00:00+05:30".into(),
            range_end: "2026-11-13T23:59:59.999999999+05:30".into(),
            end_exclusive: false,
            snapshot_at: None,
        }))));
    }
    app
}

fn render(app: &mut App, w: u16, h: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| cpatop::ui::draw(f, app)).unwrap();
    let buf = term.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..h {
        let mut line = String::new();
        for x in 0..w {
            line.push_str(buf[(x, y)].symbol());
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// Render twice so detail panes that depend on the selection get their data.
fn render_tab(app: &mut App, tab: Tab, w: u16, h: u16) -> String {
    app.tab = tab;
    render(app, w, h);
    if let Some(id) = app.hit.selected.get(&PaneId::CrList).cloned() {
        data(
            app,
            Group::Detail,
            Payload::IdentityErrors(id, Ok(load("usage-identity-errors.json"))),
        );
    }
    if let Some(idx) = app.hit.selected.get(&PaneId::QuList).cloned() {
        data(
            app,
            Group::Detail,
            Payload::QuotaHistory(idx, Ok(load("quota-history-claude.json"))),
        );
    }
    render(app, w, h)
}

const SIZES: [(u16, u16); 3] = [(80, 24), (160, 48), (220, 60)];

#[test]
fn admin_screens() {
    let mut app = populated(Role::Admin);
    for tab in cpatop::layout::ALL_TABS {
        for (w, h) in SIZES {
            let s = render_tab(&mut app, tab, w, h);
            insta::assert_snapshot!(format!("admin_{}_{w}x{h}", tab.key()), s);
        }
    }
}

#[test]
fn key_viewer_screens() {
    let mut app = populated(Role::KeyViewer);
    for tab in [Tab::Overview, Tab::Realtime, Tab::Analysis] {
        for (w, h) in SIZES {
            let s = render_tab(&mut app, tab, w, h);
            insta::assert_snapshot!(format!("key_{}_{w}x{h}", tab.key()), s);
        }
    }
}

#[test]
fn login_screen() {
    let mut app = new_app();
    app.on_msg(Msg::Auth(AuthEvent::NeedLogin {
        error: Some("invalid password (from login screen)".into()),
        mode: cpatop::api::LoginMode::Admin,
    }));
    if let Screen::Login(form) = &mut app.screen {
        form.input = "hunter2".into();
    }
    let s = render(&mut app, 80, 24);
    assert!(!s.contains("hunter2"), "secret must never be rendered");
    insta::assert_snapshot!("login_80x24", s);
}

#[test]
fn offline_screen() {
    let mut app = new_app();
    app.on_msg(Msg::Auth(AuthEvent::Offline {
        message: "cannot reach server: connection failed: Connection refused (os error 111)".into(),
        retry_in: std::time::Duration::ZERO,
    }));
    insta::assert_snapshot!("offline_80x24", render(&mut app, 80, 24));
}

#[test]
fn layouts_and_themes_do_not_panic() {
    let mut app = populated(Role::Admin);
    for tab in cpatop::layout::ALL_TABS {
        app.tab = tab;
        for preset in 0..cpatop::layout::presets(tab).len() {
            app.layout_idx.insert(tab, preset);
            for (w, h) in [(40, 12), (80, 24), (120, 30), (220, 60)] {
                render(&mut app, w, h);
            }
        }
    }
    for i in 0..app.themes.len() {
        app.theme_idx = i;
        render(&mut app, 100, 30);
    }
}

#[test]
fn keys_never_render_in_full() {
    let mut app = populated(Role::Admin);
    for tab in cpatop::layout::ALL_TABS {
        let s = render_tab(&mut app, tab, 200, 60);
        assert!(!s.contains("*****"), "masked key tail visible on {tab:?}");
        assert!(!s.contains("demoKe"), "raw key prefix visible on {tab:?}");
    }
}

/// Fixture strings that identify people or keys: e-mail addresses contain
/// `@`; the fixture API keys are masked as `dem***…` / `tst***…`.
fn assert_private(s: &str, what: &str) {
    assert!(!s.contains('@'), "e-mail visible in privacy mode ({what}):\n{s}");
    for frag in ["dem", "tst", "xyz", "***", "127.0.0.1", "example.com"] {
        assert!(
            !s.contains(frag),
            "{frag:?} visible in privacy mode ({what}):\n{s}"
        );
    }
}

#[test]
fn privacy_mode_hides_every_identifier() {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let key = |c: KeyCode| Event::Key(KeyEvent::new(c, KeyModifiers::NONE));
    for role in [Role::Admin, Role::KeyViewer] {
        let mut app = populated(role);
        // Without privacy the fixtures do show identifiers…
        let last = *app.tabs().last().unwrap();
        let plain = render_tab(&mut app, last, 200, 60);
        if role == Role::Admin {
            assert!(plain.contains("dem…"), "{plain}");
        }
        // …and with it, none.
        app.settings.privacy = true;
        let tabs: Vec<Tab> = app.tabs().to_vec();
        for tab in tabs {
            for (w, h) in [(80, 24), (160, 48), (220, 60)] {
                let s = render_tab(&mut app, tab, w, h);
                assert_private(&s, &format!("{role:?} {} {w}x{h}", tab.key()));
                if matches!(tab, Tab::Credentials | Tab::Quota | Tab::Events) {
                    assert!(s.contains("account-"), "{s}");
                }
            }
            // Every dimension of multi-dimension tables.
            for _ in 0..4 {
                app.on_event(key(KeyCode::Char('d')));
                let s = render_tab(&mut app, tab, 200, 60);
                assert_private(&s, &format!("{role:?} {} dims", tab.key()));
            }
        }
        if role == Role::Admin {
            // Pickers list keys and sources.
            app.tab = Tab::Events;
            for c in ['a', 'o'] {
                render(&mut app, 160, 48);
                app.on_event(key(KeyCode::Char(c)));
                let s = render(&mut app, 160, 48);
                assert_private(&s, &format!("picker {c}"));
                app.on_event(key(KeyCode::Esc));
            }
            // With a key filter the header names the key.
            render(&mut app, 160, 48);
            app.on_event(key(KeyCode::Char('a')));
            app.on_event(key(KeyCode::Down));
            app.on_event(key(KeyCode::Enter));
            let s = render(&mut app, 160, 48);
            assert!(s.contains("key #1"), "{s}");
            assert_private(&s, "key filter header");
        }
    }
}
