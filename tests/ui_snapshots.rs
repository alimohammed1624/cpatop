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

/// The "change server URL" field over the offline screen, with an error
/// and the cursor inside the text.
#[test]
fn server_url_field() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = new_app();
    app.on_msg(Msg::Auth(AuthEvent::Offline {
        message: "cannot reach server: connection failed".into(),
        retry_in: std::time::Duration::ZERO,
    }));
    app.on_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    if let Some(e) = &mut app.server_edit {
        e.error = Some("not a CPA Usage Keeper server: 404 not found — check the sub-path".into());
        e.cursor = 7;
    }
    insta::assert_snapshot!("server_url_80x24", render(&mut app, 80, 24));
    let narrow = render(&mut app, 50, 12);
    assert!(narrow.contains("Change server URL"), "{narrow}");
}

/// In privacy mode the typed URL is echoed masked: hosts, IPs and paths
/// never show, the scheme and port do.
#[test]
fn privacy_masks_the_server_url_field() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = new_app();
    app.settings.privacy = true;
    app.on_msg(Msg::Auth(AuthEvent::NeedLogin {
        error: None,
        mode: cpatop::api::LoginMode::Admin,
    }));
    app.on_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    for c in "http://127.0.0.1:8318/keeper-tst".chars() {
        app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    if let Some(e) = &mut app.server_edit {
        e.error = Some("unreachable: tcp connect error 10.0.0.5:8318".into());
    }
    let s = render(&mut app, 80, 24);
    assert_private(&s, "server field");
    assert!(s.contains("http://•••••••••:8318/••••••••••"), "{s}");
    assert_eq!(app.privacy_guard_hits.get(), 0, "masked before the guard");
    // A host name and port stay readable, as in the header (P3); the
    // path and user info do not.
    app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    for c in "http://localhost:1/keeper-tst".chars() {
        app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    let s = render(&mut app, 80, 24);
    assert_private(&s, "server field host");
    assert!(s.contains("http://localhost:1/••••••••••"), "{s}");
    app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    for c in "http://dem@localhost:1".chars() {
        app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    let s = render(&mut app, 80, 24);
    assert_private(&s, "server field user info");
    assert!(s.contains("http://•••••••••••••:1"), "{s}");
    // The saved-confirmation names no directories.
    app.server_edit = None;
    app.on_server_saved(Ok("/home/dem/.config/cpatop/config.toml".into()));
    let s = render(&mut app, 80, 24);
    assert_private(&s, "saved notice");
    assert!(s.contains("…/config.toml"), "{s}");
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
    assert!(
        !s.contains('@'),
        "e-mail visible in privacy mode ({what}):\n{s}"
    );
    for frag in [
        "dem",
        "tst",
        "xyz",
        "***",
        "127.0.0.1",
        "example.com",
        "::1",
        "10.0.0.5",
        "fe80",
        "abcdefghijklmnop",
        "9f8e7d6c5b4a3f2e",
    ] {
        assert!(
            !s.contains(frag),
            "{frag:?} visible in privacy mode ({what}):\n{s}"
        );
    }
}

/// Server text that carries IPs and keys in places a sanitizer could miss.
const POISON: &str = "dial tcp 10.0.0.5:8080 via http://[fe80::1]:9/x for sk-abcdefghijklmnop1234 \
                      token 9f8e7d6c5b4a3f2e1d0c";

fn poison(app: &mut App) {
    if app.role == Some(Role::Admin) {
        let mut st: serde_json::Value = load("status.json");
        st["last_error"] = POISON.into();
        st["last_warning"] = POISON.into();
        st["last_status"] = POISON.into();
        let st: ServerStatus = serde_json::from_value(st).unwrap();
        data(app, Group::Meta, Payload::Status(Ok(st)));
    }
    app.poll_error.insert(Group::Overview, POISON.into());
    app.flash(POISON);
}

/// The offline and login screens name the server and show raw errors.
#[test]
fn privacy_covers_login_and_offline_screens() {
    let mut app = new_app();
    app.settings.privacy = true;
    app.settings.base_url = "http://127.0.0.1:8318".into();
    app.host = cpatop::api::client::display_host(&app.settings.base_url);
    app.on_msg(Msg::Auth(AuthEvent::Offline {
        message: format!("cannot reach server: {POISON}"),
        retry_in: std::time::Duration::from_secs(5),
    }));
    let s = render(&mut app, 80, 24);
    assert_private(&s, "offline");
    assert!(s.contains("‹ip›"), "{s}");
    app.on_msg(Msg::Auth(AuthEvent::NeedLogin {
        error: Some(POISON.into()),
        mode: cpatop::api::LoginMode::Admin,
    }));
    assert_private(&render(&mut app, 80, 24), "login");
    app.on_msg(Msg::Auth(AuthEvent::Connecting));
    assert_private(&render(&mut app, 80, 24), "connecting");
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
        // …and with it, none — also with an IP-literal base URL and
        // adversarial server text (URLs with IPs, opaque keys).
        app.settings.privacy = true;
        app.settings.base_url = "http://127.0.0.1:8318".into();
        app.host = cpatop::api::client::display_host(&app.settings.base_url);
        poison(&mut app);
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

/// Without colour, failures in the realtime outcome bars use their own
/// pattern, and the activity legend shows the glyphs the maps draw.
#[test]
fn no_color_keeps_outcomes_and_legends_distinct() {
    let mut app = populated(Role::Admin);
    for t in &mut app.themes {
        t.no_color = true;
    }
    let mut rt: serde_json::Value = load("usage-overview-realtime.json");
    for (i, o) in rt["insights"]["outcomes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        if i % 3 == 0 {
            o["requests"] = 10.into();
            o["failures"] = 6.into();
        }
    }
    data(
        &mut app,
        Group::Realtime,
        Payload::Realtime(Ok(serde_json::from_value(rt).unwrap())),
    );
    for (w, h) in SIZES {
        let s = render_tab(&mut app, Tab::Realtime, w, h);
        assert!(
            s.contains('░') && s.contains("█ ok ░ fail"),
            "{w}x{h} legend cut:\n{s}"
        );
    }
    let s = render_tab(&mut app, Tab::Overview, 160, 48);
    // Legend glyphs: ■ ▲ ✗ for health, · for idle, shades for tokens.
    for g in ["■≥95%", "▲≥50%", "✗<50%", "·idle", "░▒▓█"] {
        assert!(s.contains(g), "legend lacks {g:?}:\n{s}");
    }
}

/// Identifiers planted in every structured field a server can send:
/// mixed-case e-mails, IPs behind capitalised path segments, keys, URLs
/// with e-mails in the path, IPv6 and user agents carrying both.
const PLANTED: [&str; 7] = [
    "Zed.Quill@Mail.Example.ORG",
    "Proxy/10.20.30.40",
    "sk-live-AbCdEfGh0123456789XyZ",
    "http://localhost:1/alice.w@example.org",
    "2001:DB8:AB::77",
    "Mozilla/5.0 (bob.k@x.io; 192.168.7.7) ua",
    "POST /u/carol.p@x.io/v1/messages",
];
/// Fragments of [`PLANTED`] that must never be visible (lower case).
const PLANTED_FRAGS: [&str; 14] = [
    "zed.",
    "quill",
    "mail.example",
    "10.20.30",
    "abcdefgh",
    "0123456789x",
    "alice",
    "example.org",
    "2001:",
    "db8:",
    "bob.k",
    "192.168",
    "carol",
    "x.io",
];

/// Replace every string field that can carry a name, key, address or free
/// text with one of [`PLANTED`].
fn plant(v: &mut serde_json::Value, n: &mut usize) {
    const FIELDS: [&str; 26] = [
        "name",
        "displayName",
        "display_name",
        "alias",
        "key_alias",
        "file_name",
        "note",
        "source",
        "label",
        "display_key",
        "endpoint",
        "user_agent",
        "client_ip",
        "x_forwarded_for",
        "login_ip",
        "last_seen_ip",
        "body_summary",
        "message",
        "error",
        "request_id",
        "api_key",
        "cpa_public_url",
        "last_error",
        "last_warning",
        "last_status",
        "groupLabel",
    ];
    match v {
        serde_json::Value::Object(m) => {
            for (k, x) in m.iter_mut() {
                if FIELDS.contains(&k.as_str()) && x.is_string() {
                    *x = PLANTED[*n % PLANTED.len()].into();
                    *n += 1;
                } else {
                    plant(x, n);
                }
            }
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(|x| plant(x, n)),
        _ => {}
    }
}

fn planted<T: DeserializeOwned>(name: &str, n: &mut usize) -> T {
    let mut v: serde_json::Value = load(name);
    plant(&mut v, n);
    serde_json::from_value(v).unwrap()
}

fn assert_nothing_planted(s: &str, what: &str) {
    let low = s.to_lowercase();
    for f in PLANTED_FRAGS {
        assert!(
            !low.contains(f),
            "{f:?} visible in privacy mode ({what}):\n{s}"
        );
    }
    assert_private(s, what);
}

/// Every tab, both roles, three sizes, with identifiers planted in every
/// field and the two base-URL repros: nothing survives, and the panes
/// replace identifiers themselves (the render guard has nothing to do).
#[test]
fn privacy_survives_planted_identifiers() {
    for role in [Role::Admin, Role::KeyViewer] {
        let mut app = populated(role);
        let mut n = 0;
        let admin = role == Role::Admin;
        let pick = |a: &'static str, k: &'static str| if admin { a } else { k };
        data(
            &mut app,
            Group::Overview,
            Payload::Comparisons(Ok(planted(
                pick(
                    "usage-overview-comparisons.json",
                    "key-overview-comparisons.json",
                ),
                &mut n,
            ))),
        );
        data(
            &mut app,
            Group::Realtime,
            Payload::Realtime(Ok(planted(
                pick("usage-overview-realtime.json", "key-overview-realtime.json"),
                &mut n,
            ))),
        );
        data(
            &mut app,
            Group::Analysis,
            Payload::Analysis(Ok(planted(
                pick("usage-analysis.json", "key-analysis.json"),
                &mut n,
            ))),
        );
        if admin {
            data(
                &mut app,
                Group::Meta,
                Payload::Status(Ok(planted("status.json", &mut n))),
            );
            data(
                &mut app,
                Group::Credentials,
                Payload::Identities(Ok(planted("usage-identities-page.json", &mut n))),
            );
            data(
                &mut app,
                Group::Quota,
                Payload::Inspection(Ok(planted("quota-inspection.json", &mut n))),
            );
            data(
                &mut app,
                Group::KeyOptions,
                Payload::ApiKeys(Ok(planted("usage-api-keys.json", &mut n))),
            );
            data(
                &mut app,
                Group::KeyOptions,
                Payload::KeyOptions(Ok(planted("usage-api-keys-options.json", &mut n))),
            );
            data(
                &mut app,
                Group::KeyUsage,
                Payload::KeyUsage(Ok(planted("usage-overview-comparisons.json", &mut n))),
            );
            data(
                &mut app,
                Group::EventFilters,
                Payload::EventSources(Ok(planted("usage-events-filters-sources.json", &mut n))),
            );
            data(
                &mut app,
                Group::Settings,
                Payload::Sessions(Ok(planted("auth-sessions.json", &mut n))),
            );
            let page: EventsPage = planted("usage-events.json", &mut n);
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
        app.settings.privacy = true;
        for base in [
            "http://localhost:1/alice.w@example.org",
            "http://localhost:1/Proxy/10.20.30.40",
            "http://127.0.0.1:8318",
        ] {
            app.settings.base_url = base.into();
            app.host = cpatop::api::client::display_host(base);
            app.flash(PLANTED[5]);
            let tabs: Vec<Tab> = app.tabs().to_vec();
            for tab in tabs {
                for (w, h) in SIZES {
                    let s = render_tab(&mut app, tab, w, h);
                    let what = format!("{role:?} {} {w}x{h} {base}", tab.key());
                    assert_nothing_planted(&s, &what);
                    assert_eq!(
                        app.privacy_guard_hits.get(),
                        0,
                        "guard fired ({what}):\n{s}"
                    );
                }
            }
        }
        // Pickers list keys, sources and models.
        if admin {
            use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
            let key = |c: KeyCode| Event::Key(KeyEvent::new(c, KeyModifiers::NONE));
            app.tab = Tab::Events;
            for c in ['a', 'o', 'm'] {
                render(&mut app, 160, 48);
                app.on_event(key(KeyCode::Char(c)));
                let s = render(&mut app, 160, 48);
                assert_nothing_planted(&s, &format!("picker {c}"));
                assert_eq!(
                    app.privacy_guard_hits.get(),
                    0,
                    "guard fired (picker {c}):\n{s}"
                );
                app.on_event(key(KeyCode::Esc));
            }
        }
        // The offline screen names the server.
        app.on_msg(Msg::Auth(AuthEvent::Offline {
            message: format!("cannot reach {}", PLANTED[3]),
            retry_in: std::time::Duration::from_secs(5),
        }));
        let s = render(&mut app, 80, 24);
        assert_nothing_planted(&s, "offline");
        assert!(s.contains("http://‹ip›:8318"), "{s}");
    }
}

/// Enter opens every field of the selected row, untruncated, at 80x24;
/// ↑↓ follow the table and Esc closes it.
#[test]
fn row_popup_shows_full_fields() {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let key = |c: KeyCode| Event::Key(KeyEvent::new(c, KeyModifiers::NONE));
    let mut app = populated(Role::Admin);
    for tab in [
        Tab::Events,
        Tab::Credentials,
        Tab::Analysis,
        Tab::Quota,
        Tab::Settings,
    ] {
        render_tab(&mut app, tab, 80, 24);
        app.on_event(key(KeyCode::Enter));
        let s = render(&mut app, 80, 24);
        assert!(s.contains("· row"), "{tab:?} popup missing:\n{s}");
        app.on_event(key(KeyCode::Down));
        let s2 = render(&mut app, 80, 24);
        assert!(s2.contains("· row"), "{s2}");
        app.on_event(key(KeyCode::Esc));
        let s3 = render(&mut app, 80, 24);
        assert!(!s3.contains("· row"), "{s3}");
    }
    // A name cut in the table is whole in the popup.
    render_tab(&mut app, Tab::Events, 80, 24);
    app.on_event(key(KeyCode::Enter));
    let s = render(&mut app, 80, 24);
    assert!(
        s.contains("user1@example.com") && s.contains("claude-opus-5-5"),
        "{s}"
    );
}

fn press(app: &mut App, code: crossterm::event::KeyCode) {
    use crossterm::event::{Event, KeyEvent, KeyModifiers};
    app.on_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

/// Everything the open row popup shows, scrolled from top to bottom a page
/// at a time.
fn popup_pages(app: &mut App, w: u16, h: u16) -> String {
    use crossterm::event::KeyCode;
    let mut out = render(app, w, h);
    for _ in 0..50 {
        let before = app.row_scroll;
        press(app, KeyCode::PageDown);
        if app.row_scroll == before {
            break;
        }
        out.push_str(&render(app, w, h));
    }
    out
}

/// Table panes of a tab, in order.
fn table_panes(tab: Tab) -> Vec<PaneId> {
    cpatop::layout::panes(tab)
        .iter()
        .filter(|d| d.table)
        .map(|d| d.id)
        .collect()
}

/// The row popup scrolls when its fields do not fit (keys, wheel, ends),
/// shows where it is, and `[` `]` step through the table's rows.
#[test]
fn row_popup_scrolls_and_steps_rows() {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
    let mut app = populated(Role::Admin);
    render_tab(&mut app, Tab::Credentials, 80, 24);
    press(&mut app, KeyCode::Enter);
    let s = render(&mut app, 80, 24);
    assert!(s.contains("credentials · row 1/"), "{s}");
    assert!(s.contains("▼") && s.contains("more"), "{s}");
    assert!(s.contains("[ ] row"), "{s}");
    let max = app.row_max.get();
    assert!(max > 0);
    // ↓ scrolls the popup, not the table underneath.
    press(&mut app, KeyCode::Down);
    assert_eq!(app.row_scroll, 1);
    assert_eq!(app.table(PaneId::CrList).selected, 0);
    let s = render(&mut app, 80, 24);
    assert!(s.contains("▲1 ▼"), "{s}");
    // End / Home / PgDn stay in range.
    press(&mut app, KeyCode::End);
    assert_eq!(app.row_scroll, max);
    let s = render(&mut app, 80, 24);
    assert!(s.contains("above"), "{s}");
    press(&mut app, KeyCode::PageDown);
    assert_eq!(app.row_scroll, max);
    press(&mut app, KeyCode::Home);
    assert_eq!(app.row_scroll, 0);
    // The mouse wheel scrolls the popup too.
    let wheel = |kind| {
        Event::Mouse(MouseEvent {
            kind,
            column: 40,
            row: 12,
            modifiers: KeyModifiers::NONE,
        })
    };
    app.on_event(wheel(MouseEventKind::ScrollDown));
    assert_eq!(app.row_scroll, 3.min(max));
    assert_eq!(app.table(PaneId::CrList).selected, 0);
    app.on_event(wheel(MouseEventKind::ScrollUp));
    assert_eq!(app.row_scroll, 0);
    // `]` and Shift+↓ step to the next row, from the top; `[` goes back.
    press(&mut app, KeyCode::End);
    press(&mut app, KeyCode::Char(']'));
    assert_eq!(app.table(PaneId::CrList).selected, 1);
    assert_eq!(app.row_scroll, 0);
    let s = render(&mut app, 80, 24);
    assert!(s.contains("credentials · row 2/"), "{s}");
    app.on_event(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT)));
    assert_eq!(app.table(PaneId::CrList).selected, 0);
    press(&mut app, KeyCode::Char(']'));
    press(&mut app, KeyCode::Char('['));
    assert_eq!(app.table(PaneId::CrList).selected, 0);
    press(&mut app, KeyCode::Esc);
    assert!(app.overlay.is_none());
}

/// The popup lists the whole record behind a row, not just the columns
/// on screen, with spelled-out names.
#[test]
fn row_popup_covers_the_whole_record() {
    use crossterm::event::KeyCode;
    let mut app = populated(Role::Admin);
    let cases: [(Tab, PaneId, &[&str]); 9] = [
        (
            Tab::Credentials,
            PaneId::CrList,
            &[
                "auth type",
                "provider",
                "priority",
                "state",
                "lifetime requests",
                "health, last 5h",
                "5h failures",
                "auth index",
                "last used",
                "last error",
            ],
        ),
        (
            Tab::Events,
            PaneId::EvLog,
            &[
                "API key",
                "source",
                "endpoint",
                "client IP",
                "user agent",
                "request id",
                "cache read tokens",
                "time to first token",
            ],
        ),
        (
            Tab::Quota,
            PaneId::QuList,
            &[
                "credential",
                "quota left",
                "resets in",
                "quota check",
                "checked",
                "auth type",
            ],
        ),
        (
            Tab::Settings,
            PaneId::StKeys,
            &["id", "last synced", "requests", "input tokens", "cost"],
        ),
        (
            Tab::Settings,
            PaneId::StSessions,
            &[
                "logged in via",
                "last seen",
                "expires",
                "user agent",
                "current",
            ],
        ),
        (
            Tab::Models,
            PaneId::MoPricing,
            &["cache read", "multiplier", "price rules"],
        ),
        (
            Tab::Overview,
            PaneId::OvDist,
            &[
                "requests",
                "failures",
                "success rate",
                "output tokens",
                "cache read rate",
            ],
        ),
        (
            Tab::Analysis,
            PaneId::AnEfficiency,
            &[
                "cost per 1M tokens",
                "cost per request",
                "output tokens per request",
            ],
        ),
        (
            Tab::Realtime,
            PaneId::RtSpeed,
            &["speed, first half", "speed change"],
        ),
    ];
    for (tab, pane, want) in cases {
        render_tab(&mut app, tab, 120, 40);
        app.focus.insert(tab, pane);
        render(&mut app, 120, 40);
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            app.overlay,
            Some(cpatop::app::Overlay::Row(pane)),
            "{tab:?}"
        );
        if pane == PaneId::QuList {
            // The first credential has no quota windows cached.
            press(&mut app, KeyCode::Char(']'));
        }
        let s = popup_pages(&mut app, 120, 40);
        for w in want {
            assert!(s.contains(w), "{tab:?} {pane:?}: {w:?} missing:\n{s}");
        }
        press(&mut app, KeyCode::Esc);
    }
}

/// With privacy on, the popup of every table on every tab, in every
/// dimension, scrolled to the end, shows nothing planted (also at 50x12,
/// where it scrolls most).
#[test]
fn row_popup_respects_privacy() {
    use crossterm::event::KeyCode;
    for role in [Role::Admin, Role::KeyViewer] {
        let mut app = populated(role);
        let mut n = 0;
        let admin = role == Role::Admin;
        let pick = |a: &'static str, k: &'static str| if admin { a } else { k };
        data(
            &mut app,
            Group::Overview,
            Payload::Comparisons(Ok(planted(
                pick(
                    "usage-overview-comparisons.json",
                    "key-overview-comparisons.json",
                ),
                &mut n,
            ))),
        );
        data(
            &mut app,
            Group::Realtime,
            Payload::Realtime(Ok(planted(
                pick("usage-overview-realtime.json", "key-overview-realtime.json"),
                &mut n,
            ))),
        );
        data(
            &mut app,
            Group::Analysis,
            Payload::Analysis(Ok(planted(
                pick("usage-analysis.json", "key-analysis.json"),
                &mut n,
            ))),
        );
        if admin {
            data(
                &mut app,
                Group::Credentials,
                Payload::Identities(Ok(planted("usage-identities-page.json", &mut n))),
            );
            data(
                &mut app,
                Group::Quota,
                Payload::Inspection(Ok(planted("quota-inspection.json", &mut n))),
            );
            data(
                &mut app,
                Group::KeyOptions,
                Payload::ApiKeys(Ok(planted("usage-api-keys.json", &mut n))),
            );
            data(
                &mut app,
                Group::KeyUsage,
                Payload::KeyUsage(Ok(planted("usage-overview-comparisons.json", &mut n))),
            );
            data(
                &mut app,
                Group::Settings,
                Payload::Sessions(Ok(planted("auth-sessions.json", &mut n))),
            );
            let page: EventsPage = planted("usage-events.json", &mut n);
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
        app.settings.privacy = true;
        let tabs: Vec<Tab> = app.tabs().to_vec();
        let mut opened = 0;
        for tab in tabs {
            for pane in table_panes(tab) {
                for (w, h) in [(50, 12), (160, 48)] {
                    for dim in 0..4 {
                        app.dims.insert(pane, dim);
                        render_tab(&mut app, tab, w, h);
                        app.focus.insert(tab, pane);
                        render(&mut app, w, h);
                        press(&mut app, KeyCode::Enter);
                        if app.overlay.is_none() {
                            continue;
                        }
                        opened += 1;
                        let s = popup_pages(&mut app, w, h);
                        let what = format!("{role:?} {tab:?} {pane:?} dim {dim} {w}x{h}");
                        assert_nothing_planted(&s, &what);
                        assert_eq!(
                            app.privacy_guard_hits.get(),
                            0,
                            "guard fired ({what}):\n{s}"
                        );
                        press(&mut app, KeyCode::Esc);
                    }
                }
            }
        }
        assert!(opened > 20, "{role:?}: only {opened} popups");
    }
}

/// The update badge survives every width: in full when there is room,
/// as `↑` when there is not; lower-value header fields go first.
#[test]
fn update_badge_never_disappears() {
    for role in [Role::Admin, Role::KeyViewer] {
        let mut app = populated(role);
        app.update_available = Some("v9.9.9".into());
        for (w, h, full) in [(50, 12, false), (80, 24, true), (160, 48, true)] {
            let s = render_tab(&mut app, Tab::Overview, w, h);
            let head = s.lines().next().unwrap();
            assert!(head.contains('↑'), "{role:?} {w}: {head}");
            if full {
                assert!(head.contains("↑ v9.9.9"), "{role:?} {w}: {head}");
            }
            assert!(head.contains("● ok"), "{head}");
            // A toast does not push it off either.
            app.flash("a fairly long message about something that just happened");
            let s = render(&mut app, w, h);
            let head = s.lines().next().unwrap();
            assert!(head.contains('↑'), "{role:?} {w} with toast: {head}");
            app.flash = None;
        }
        // At 160 columns everything fits, the cadence included.
        let s = render(&mut app, 160, 48);
        let head = s.lines().next().unwrap();
        assert!(head.contains('⟳') && head.contains("ago"), "{head}");
    }
}

/// A key viewer pressing an admin tab's key is told why nothing happens,
/// and the header keeps its refresh age.
#[test]
fn admin_tabs_say_admin_only() {
    use crossterm::event::KeyCode;
    let mut app = populated(Role::KeyViewer);
    render_tab(&mut app, Tab::Overview, 80, 24);
    for n in 5..=9 {
        press(&mut app, KeyCode::F(n));
        assert_eq!(app.tab, Tab::Overview);
        let msg = app.flash.as_ref().map(|f| f.0.clone()).unwrap_or_default();
        assert!(msg.starts_with("admin only"), "F{n}: {msg}");
        for (w, h) in [(80, 24), (50, 12)] {
            let s = render(&mut app, w, h);
            let head = s.lines().next().unwrap();
            assert!(head.contains("admin only"), "{head}");
            // The toast never pushes out the key or the refresh age.
            assert!(
                head.contains('↻') && head.contains("key"),
                "F{n} {w}: {head}"
            );
        }
    }
    // Nor does the `Esc` hint.
    press(&mut app, KeyCode::Esc);
    let s = render(&mut app, 50, 12);
    let head = s.lines().next().unwrap();
    assert!(head.contains('↻') && head.contains("key"), "{head}");
    press(&mut app, KeyCode::F(4));
    assert_eq!(app.tab, Tab::Analysis);
}

/// Without colour, `t` says themes are off instead of silently switching.
#[test]
fn no_color_theme_key_explains() {
    use crossterm::event::KeyCode;
    let mut app = populated(Role::Admin);
    app.settings.no_color = true;
    let before = app.theme_idx;
    for c in ['t', 'T'] {
        press(&mut app, KeyCode::Char(c));
        assert_eq!(app.theme_idx, before);
        let msg = app.flash.as_ref().map(|f| f.0.clone()).unwrap_or_default();
        assert_eq!(msg, "themes disabled (NO_COLOR)");
    }
    app.settings.no_color = false;
    press(&mut app, KeyCode::Char('t'));
    assert_ne!(app.theme_idx, before);
}

/// A narrow tab bar scrolls to keep the active tab, with `‹` `›` marking
/// the tabs off either end.
#[test]
fn narrow_tab_bar_keeps_the_active_tab() {
    let mut app = populated(Role::Admin);
    let bar = |app: &mut App, tab| {
        render_tab(app, tab, 50, 12)
            .lines()
            .nth(1)
            .unwrap()
            .to_string()
    };
    let first = bar(&mut app, Tab::Overview);
    assert!(
        first.contains("Ovw") && first.contains('›') && !first.contains('‹'),
        "{first}"
    );
    let last = bar(&mut app, Tab::Settings);
    assert!(
        last.contains("Keys") && last.contains('‹') && !last.contains('›'),
        "{last}"
    );
    for t in cpatop::layout::ALL_TABS {
        let s = bar(&mut app, t);
        assert!(s.contains(t.short_title()), "{t:?}: {s}");
    }
    // Wide enough: no markers.
    let s = render_tab(&mut app, Tab::Settings, 160, 48);
    let wide = s.lines().nth(1).unwrap();
    assert!(!wide.contains('‹') && !wide.contains('›'), "{wide}");
}

/// Boxes drawn in `buf` (top-left `╭` to bottom-left `╰`) that have no
/// inner row or only blank inner rows, as `(x, y, title)`.
fn empty_boxes(buf: &ratatui::buffer::Buffer) -> Vec<(u16, u16, String)> {
    let a = buf.area;
    let mut out = vec![];
    for y in 0..a.height {
        for x in 0..a.width {
            if buf[(x, y)].symbol() != "╭" {
                continue;
            }
            let Some(right) = (x + 1..a.width).find(|c| buf[(*c, y)].symbol() == "╮") else {
                continue;
            };
            let Some(bottom) = (y + 1..a.height).find(|r| buf[(x, *r)].symbol() != "│") else {
                continue;
            };
            if buf[(x, bottom)].symbol() != "╰" {
                continue;
            }
            let blank = (y + 1..bottom)
                .all(|r| (x + 1..right).all(|c| buf[(c, r)].symbol().trim().is_empty()));
            if bottom == y + 1 || blank {
                let title: String = (x..right).map(|c| buf[(c, y)].symbol()).collect();
                out.push((x, y, title));
            }
        }
    }
    out
}

/// A pane too short for its content shows its one-line summary, never an
/// empty box (e.g. the `graphs` layout's totals at 120x30), in every layout
/// preset of every tab at small and medium sizes, in both roles.
#[test]
fn short_panes_collapse_instead_of_drawing_empty_boxes() {
    // The detector itself: a box with a blank inner row, and one with none.
    let mut b = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 10, 5));
    b.set_string(0, 0, "╭─ a ─╮", ratatui::style::Style::default());
    b.set_string(0, 1, "│     │", ratatui::style::Style::default());
    b.set_string(0, 2, "╰─────╯", ratatui::style::Style::default());
    b.set_string(0, 3, "╭─ b ─╮", ratatui::style::Style::default());
    b.set_string(0, 4, "╰─────╯", ratatui::style::Style::default());
    assert_eq!(empty_boxes(&b).len(), 2);
    for role in [Role::Admin, Role::KeyViewer] {
        let mut app = populated(role);
        let tabs: Vec<Tab> = match role {
            Role::Admin => cpatop::layout::ALL_TABS.to_vec(),
            Role::KeyViewer => vec![Tab::Overview, Tab::Realtime, Tab::Analysis],
        };
        for tab in tabs {
            for preset in 0..cpatop::layout::presets(tab).len() {
                app.layout_idx.insert(tab, preset);
                for (w, h) in [(50, 12), (80, 24), (120, 30)] {
                    render_tab(&mut app, tab, w, h);
                    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
                    term.draw(|f| cpatop::ui::draw(f, &mut app)).unwrap();
                    let empty = empty_boxes(term.backend().buffer());
                    let name = cpatop::layout::presets(tab)[preset].name;
                    assert!(
                        empty.is_empty(),
                        "{role:?} {} layout {name} at {w}x{h}: empty boxes {empty:?}\n{}",
                        tab.key(),
                        render(&mut app, w, h)
                    );
                }
            }
        }
    }
}

/// What P2 types character by character; no prefix of it may show.
const TYPED: &[&str] = &[
    r#""alice \"bob"@example.org"#,
    r#""alice \"bob\" smith"@example.org"#,
    "alice/bob",
    "abcdefghijklmnopqrst",
    "10.0.0.5",
];

/// Whether a rendered screen shows any part of a typed prefix that the
/// screen before typing (`base`) did not: a run of three or more letters,
/// or the start of an IP address.
fn shows_typed(screen: &str, base: &str, prefix: &str) -> Option<String> {
    for run in prefix.split(|c: char| !c.is_ascii_alphabetic()) {
        for w in 3..=run.len() {
            for i in 0..=run.len() - w {
                let frag = &run[i..i + w];
                // Short runs only count once they are distinctive.
                if (w >= 4 || ["ali", "bob", "smi", "exa"].contains(&frag))
                    && screen.contains(frag)
                    && !base.contains(frag)
                {
                    return Some(frag.to_string());
                }
            }
        }
    }
    if prefix.starts_with("10.0") && screen.contains("10.0") && !base.contains("10.0") {
        return Some(prefix.into());
    }
    None
}

/// In privacy mode, `/` filters and picker searches are hidden from the
/// first character typed (P2): render after every keystroke.
#[test]
fn privacy_hides_typed_input_at_every_keystroke() {
    use crossterm::event::KeyCode;
    for text in TYPED {
        // `/` in the focused table of each tab.
        for tab in [Tab::Overview, Tab::Credentials, Tab::Models] {
            let mut app = populated(Role::Admin);
            app.settings.privacy = true;
            render_tab(&mut app, tab, 120, 30);
            for _ in 0..8 {
                press(&mut app, KeyCode::Char('/'));
                if matches!(app.mode, cpatop::app::InputMode::Filter(_)) {
                    break;
                }
                press(&mut app, KeyCode::Char('f'));
            }
            assert!(
                matches!(app.mode, cpatop::app::InputMode::Filter(_)),
                "no table to filter on {tab:?}"
            );
            let base = render(&mut app, 120, 30);
            let mut typed = String::new();
            for c in text.chars() {
                press(&mut app, KeyCode::Char(c));
                typed.push(c);
                let s = render(&mut app, 120, 30);
                assert!(
                    s.contains("input hidden in privacy mode"),
                    "{tab:?} prompt:\n{s}"
                );
                if let Some(f) = shows_typed(&s, &base, &typed) {
                    panic!("{tab:?} filter {typed:?} shows {f:?}:\n{s}");
                }
            }
            // Kept filter: the pane title shows bullets and a count.
            press(&mut app, KeyCode::Enter);
            let s = render(&mut app, 120, 30);
            assert!(shows_typed(&s, &base, text).is_none(), "{tab:?} kept:\n{s}");
            assert!(
                s.contains(&format!("/•••••••• {}", text.chars().count())),
                "{tab:?} title:\n{s}"
            );
        }
        // The API-key picker's search.
        let mut app = populated(Role::Admin);
        app.settings.privacy = true;
        render_tab(&mut app, Tab::Overview, 120, 30);
        press(&mut app, KeyCode::Char('a'));
        assert!(app.overlay.is_some(), "key picker did not open");
        let bases = [render(&mut app, 120, 30), render(&mut app, 50, 12)];
        let mut typed = String::new();
        for c in text.chars() {
            press(&mut app, KeyCode::Char(c));
            typed.push(c);
            for ((w, h), base) in [(120, 30), (50, 12)].into_iter().zip(&bases) {
                let s = render(&mut app, w, h);
                if let Some(f) = shows_typed(&s, base, &typed) {
                    panic!("picker {typed:?} at {w}x{h} shows {f:?}:\n{s}");
                }
            }
        }
    }
}

/// `--privacy` from the command line is not saved; `x` in the app is (P4).
#[test]
fn command_line_privacy_is_not_persisted() {
    use crossterm::event::KeyCode;
    let mut app = new_app();
    app.settings.privacy = true;
    app.cli_overrides.privacy = Some(true);
    assert_eq!(app.ui_state().privacy, None, "nothing saved before");
    app.saved_ui.privacy = Some(false);
    assert_eq!(app.ui_state().privacy, Some(false), "saved value kept");
    // Toggling in the app is a choice to remember.
    let mut app = populated(Role::Admin);
    app.settings.privacy = true;
    app.cli_overrides.privacy = Some(true);
    render(&mut app, 80, 24);
    press(&mut app, KeyCode::Char('x'));
    assert!(!app.settings.privacy);
    assert_eq!(app.ui_state().privacy, Some(false));
}

/// A setting changed in the app is saved even when the change ends on the
/// command-line value (`--privacy`, then `x` twice), and survives a restart
/// without the flag; untouched overrides are not saved (M1).
#[test]
fn explicit_changes_back_to_the_command_line_value_persist_across_restart() {
    use cpatop::range::TimeRange;
    use crossterm::event::KeyCode;
    let dir = std::env::temp_dir().join(format!("cpatop-m1-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ui.toml");
    // Saved before: privacy off, range 7d, symbols braille, refresh 5s.
    let before = UiState {
        privacy: Some(false),
        range: Some(TimeRange::Days(7)),
        refresh_secs: Some(5),
        ..Default::default()
    };
    before.save(&path).unwrap();

    // Run 1: `--privacy --range today --refresh 9 --symbols …`.
    let mut app = populated(Role::Admin);
    app.saved_ui = UiState::load(&path);
    app.settings.privacy = true;
    app.range = TimeRange::Today;
    app.settings.refresh_secs = 9;
    let sym = app.settings.graph_symbols;
    app.cli_overrides = UiState {
        privacy: Some(true),
        range: Some(TimeRange::Today),
        refresh_secs: Some(9),
        graph_symbols: Some(sym),
        ..Default::default()
    };
    render(&mut app, 80, 24);
    // Privacy off and on again; range there and back; refresh untouched.
    press(&mut app, KeyCode::Char('x'));
    press(&mut app, KeyCode::Char('x'));
    assert!(app.settings.privacy);
    press(&mut app, KeyCode::Char(']'));
    press(&mut app, KeyCode::Char('['));
    assert_eq!(app.range, TimeRange::Today);
    // Symbols cycled all the way round.
    let mut n = 0;
    loop {
        press(&mut app, KeyCode::Char('b'));
        n += 1;
        if app.settings.graph_symbols == sym || n > 10 {
            break;
        }
    }
    app.ui_state().save(&path).unwrap();

    // Run 2 without flags reads what run 1 saved.
    let st = UiState::load(&path);
    assert_eq!(st.privacy, Some(true), "privacy toggled on in the app");
    assert_eq!(st.range, Some(TimeRange::Today), "range picked in the app");
    assert_eq!(st.graph_symbols, Some(sym), "symbols cycled in the app");
    assert_eq!(st.refresh_secs, Some(5), "untouched --refresh not saved");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The graph cursor stops at the bucket in progress: `G`, `→` and a click
/// past now do not reach the rest of `today` (C4).
#[test]
fn graph_cursor_stops_at_now() {
    use crossterm::event::KeyCode;
    let mut app = populated(Role::Admin);
    app.focus.insert(Tab::Overview, PaneId::OvTrend);
    render(&mut app, 120, 30);
    // Fixture: `today` in +05:30 at 16:45, buckets 00:00, 00:30 … 23:30.
    let now_idx = 17; // 16:30–17:30
    press(&mut app, KeyCode::Char('G'));
    assert_eq!(app.cursor.get(&PaneId::OvTrend), Some(&now_idx));
    render(&mut app, 120, 30);
    press(&mut app, KeyCode::Right);
    assert_eq!(app.cursor.get(&PaneId::OvTrend), Some(&now_idx));
    let s = render(&mut app, 120, 30);
    assert!(s.contains("▸ 11-13 16:30"), "{s}");
    // A click on the dotted rest of the day lands on the same bucket.
    let (_, r, _) = *app
        .hit
        .graphs
        .iter()
        .find(|(id, _, _)| *id == PaneId::OvTrend)
        .unwrap();
    use crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    app.on_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: r.right() - 2,
        row: r.y + 1,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.cursor.get(&PaneId::OvTrend), Some(&now_idx));
}
