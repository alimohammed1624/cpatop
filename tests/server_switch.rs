//! Changing the server URL at runtime: the offline/login screens' field,
//! the probe, switching the background tasks and saving to the config file.

use cpatop::api::{LoginMode, Role};
use cpatop::app::{App, Effect, Screen};
use cpatop::auth::AuthEvent;
use cpatop::config::{Settings, UiState};
use cpatop::poller::Msg;
use cpatop::secrets::{Credential, Credentials, Secret, Source};
use cpatop::server::{self, BaseUrlSource, PROBE_TIMEOUT};
use cpatop::session_store::SessionStore;
use cpatop::tui::Backend;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::timeout;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const COOKIE: &str = "cpa_usage_keeper_session";

fn tempdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cpatop-switch-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn key_creds() -> Credentials {
    Credentials {
        api_key: Some(Credential {
            mode: LoginMode::ApiKey,
            secret: Secret::new("key-secret"),
            source: Source::File("/dev/null/key".into()),
        }),
        ..Default::default()
    }
}

/// A Keeper that accepts the key and answers `/auth/session` by cookie.
async fn keeper() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/startup/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"phase":"ready"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/auth/session"))
        .respond_with(|req: &Request| {
            let cookie = req
                .headers
                .get("cookie")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            let body = if cookie.contains("keytok") {
                json!({"authenticated": true, "role": "api_key_viewer",
                       "api_key": {"id": "3", "display_key": "abc"}})
            } else {
                json!({"authenticated": false})
            };
            ResponseTemplate::new(200).set_body_json(body)
        })
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/auth/api-key-login"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("set-cookie", format!("{COOKIE}=keytok; Path=/; HttpOnly")),
        )
        .mount(&server)
        .await;
    server
}

fn key(app: &mut App, code: KeyCode) {
    app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn ctrl(app: &mut App, c: char) {
    app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
}

fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        key(app, KeyCode::Char(c));
    }
}

/// Submit the field and run the probe the app asks for, like the runner.
async fn submit(app: &mut App) -> Option<String> {
    key(app, KeyCode::Enter);
    let mut probe = None;
    let mut rest = Vec::new();
    for e in app.drain_effects() {
        match e {
            Effect::ProbeServer { id, url } => probe = Some((id, url)),
            other => rest.push(other),
        }
    }
    app.effects = rest;
    let (id, url) = probe?;
    let r = server::probe(&url, PROBE_TIMEOUT).await;
    app.on_server_probe(id, url.clone(), r);
    Some(url)
}

fn edit_error(app: &App) -> String {
    app.server_edit
        .as_ref()
        .and_then(|e| e.error.clone())
        .unwrap_or_default()
}

/// Offline → edit → invalid → unreachable → not a Keeper → a Keeper: the
/// app switches, logs in on the new server and the URL is saved to the
/// config file without disturbing the rest of it.
#[tokio::test]
async fn offline_screen_switches_to_a_new_server_and_saves_it() {
    let good = keeper().await;
    let not_keeper = MockServer::start().await; // 404 for everything
    let dir = tempdir("flow");
    let cfg = dir.join("config.toml");
    std::fs::write(
        &cfg,
        "# mine\ntheme = \"nord\"\nbase_url = \"http://127.0.0.1:1\"\n",
    )
    .unwrap();

    let mut app = App::new(
        Settings {
            base_url: "http://127.0.0.1:1".into(),
            ..Default::default()
        },
        vec![],
        &UiState::default(),
    );
    app.config_path = Some(cfg.clone());
    app.base_url_source = BaseUrlSource::Flag;
    app.on_msg(Msg::Auth(AuthEvent::Offline {
        message: "cannot reach server: connection failed".into(),
        retry_in: Duration::from_secs(5),
    }));

    // `b` opens the field with the current URL and the cursor at its end.
    key(&mut app, KeyCode::Char('b'));
    let e = app.server_edit.clone().expect("field open");
    assert_eq!(e.input, "http://127.0.0.1:1");
    assert_eq!(e.cursor, e.input.len());
    // `q` is text here, not quit.
    key(&mut app, KeyCode::Char('q'));
    assert!(!app.should_quit);
    key(&mut app, KeyCode::Backspace);

    // Invalid: refused before any probe.
    ctrl(&mut app, 'u');
    typ(&mut app, "ftp://example");
    assert_eq!(submit(&mut app).await, None);
    assert!(
        edit_error(&app).contains("only http"),
        "{}",
        edit_error(&app)
    );

    // Unreachable.
    ctrl(&mut app, 'u');
    typ(&mut app, "127.0.0.1:1");
    assert_eq!(
        submit(&mut app).await.as_deref(),
        Some("http://127.0.0.1:1")
    );
    assert!(
        edit_error(&app).starts_with("unreachable"),
        "{}",
        edit_error(&app)
    );
    assert!(matches!(app.screen, Screen::Offline { .. }));

    // Answers, but not like a Keeper. The scheme is added.
    ctrl(&mut app, 'u');
    typ(&mut app, not_keeper.uri().trim_start_matches("http://"));
    submit(&mut app).await.unwrap();
    assert!(
        edit_error(&app).starts_with("not a CPA Usage Keeper server"),
        "{}",
        edit_error(&app)
    );

    // A Keeper: the app switches.
    ctrl(&mut app, 'u');
    typ(&mut app, &format!("{}/", good.uri()));
    let url = submit(&mut app).await.unwrap();
    assert_eq!(url, good.uri(), "trailing slash dropped");
    assert!(app.server_edit.is_none());
    assert!(matches!(app.screen, Screen::Connecting));
    assert_eq!(app.settings.base_url, url);
    let switch: Vec<_> = app
        .drain_effects()
        .into_iter()
        .filter_map(|e| match e {
            Effect::SwitchServer(u) => Some(u),
            _ => None,
        })
        .collect();
    assert_eq!(switch, vec![url.clone()]);

    // What the runner does: new background tasks, save, report.
    let store = SessionStore::new(Some(dir.join("session.json")));
    let mut backend = Backend::spawn(&url, &key_creds(), false, false, store.clone()).unwrap();
    app.on_server_saved(server::save_base_url(&cfg, &url).map(|()| cfg.clone()));
    let notice = app.server_notice.clone().unwrap();
    assert!(notice.contains("saved to"), "{notice}");
    assert!(notice.contains("--base-url wins next start"), "{notice}");
    loop {
        let m = timeout(Duration::from_secs(10), backend.rx.recv())
            .await
            .expect("timed out")
            .expect("closed");
        app.on_msg(m);
        if matches!(app.screen, Screen::Dashboard) {
            break;
        }
    }
    assert_eq!(app.role, Some(Role::KeyViewer));
    assert_eq!(
        store.load(&url, "api_key_viewer").as_deref(),
        Some("keytok")
    );
    assert!(
        app.server_notice.is_none(),
        "shown as a flash on the dashboard"
    );
    assert_eq!(
        std::fs::read_to_string(&cfg).unwrap(),
        format!("# mine\ntheme = \"nord\"\nbase_url = \"{url}\"\n")
    );
    let _ = backend.retire(false).await;
    std::fs::remove_dir_all(dir).unwrap();
}

/// A late probe answer for a field that was edited since is ignored.
#[tokio::test]
async fn stale_probe_results_are_ignored() {
    let mut app = App::new(Settings::default(), vec![], &UiState::default());
    app.on_msg(Msg::Auth(AuthEvent::Offline {
        message: "x".into(),
        retry_in: Duration::from_secs(5),
    }));
    key(&mut app, KeyCode::Char('b'));
    key(&mut app, KeyCode::Enter);
    let id = app.server_edit.as_ref().unwrap().probing.expect("probing");
    typ(&mut app, "9"); // editing abandons the check
    app.on_server_probe(id, "http://localhost:8318".into(), Ok(()));
    assert!(app.server_edit.is_some(), "not switched");
    assert_eq!(app.settings.base_url, "http://localhost:8318");
    assert!(
        !app.drain_effects()
            .iter()
            .any(|e| matches!(e, Effect::SwitchServer(_)))
    );
    // Esc cancels and goes back to the offline screen.
    key(&mut app, KeyCode::Esc);
    assert!(app.server_edit.is_none());
    assert!(matches!(app.screen, Screen::Offline { .. }));
}

/// The login screen is not a dead end: Ctrl-S opens the field, Esc returns
/// to the form with its mode kept.
#[test]
fn login_screen_opens_the_server_field() {
    let mut app = App::new(Settings::default(), vec![], &UiState::default());
    app.on_msg(Msg::Auth(AuthEvent::NeedLogin {
        error: None,
        mode: LoginMode::ApiKey,
    }));
    typ(&mut app, "ab");
    ctrl(&mut app, 's');
    assert!(app.server_edit.is_some());
    typ(&mut app, "x");
    assert_eq!(
        app.login_form().unwrap().input,
        "ab",
        "typing goes to the URL"
    );
    key(&mut app, KeyCode::Esc);
    assert!(app.server_edit.is_none());
    assert_eq!(app.login_form().unwrap().mode, LoginMode::ApiKey);
}

/// Retiring a backend with `--logout-on-exit` ends its session on the old
/// server; without it the session stays stored for the next start.
#[tokio::test]
async fn retired_backend_logs_out_only_when_asked() {
    for logout in [false, true] {
        let server = keeper().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/auth/logout"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .expect(u64::from(logout))
            .mount(&server)
            .await;
        let dir = tempdir(&format!("retire-{logout}"));
        let store = SessionStore::new(Some(dir.join("session.json")));
        let base = server.uri();
        let mut b = Backend::spawn(&base, &key_creds(), true, false, store.clone()).unwrap();
        loop {
            let m = timeout(Duration::from_secs(10), b.rx.recv())
                .await
                .unwrap()
                .unwrap();
            if matches!(m, Msg::Auth(AuthEvent::LoggedIn { .. })) {
                break;
            }
        }
        let c = b.client.clone();
        timeout(Duration::from_secs(10), b.retire(logout))
            .await
            .unwrap()
            .unwrap();
        let stored = store.load(&base, "api_key_viewer");
        if logout {
            assert_eq!(stored, None);
            assert_eq!(c.token(), None);
        } else {
            assert_eq!(stored.as_deref(), Some("keytok"));
        }
        server.verify().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
}
