//! HTTP-backed integration tests: the real client, pollers, live stream and
//! auth manager against a local mock Keeper (wiremock).

use cpatop::api::sse::{self, EventFilters, LiveEvent, LiveParams, ResultFilter};
use cpatop::api::{Client, LoginMode, Role};
use cpatop::app::{App, Effect};
use cpatop::auth::{AuthCmd, AuthEvent, AuthManager};
use cpatop::config::{Settings, UiState};
use cpatop::layout::Tab;
use cpatop::poller::{self, Msg};
use cpatop::range::TimeRange;
use cpatop::secrets::{Credential, Credentials, Secret, Source};
use cpatop::session_store::SessionStore;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use serde_json::json;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const COOKIE: &str = "cpa_usage_keeper_session";

fn admin_app(base: &str) -> App {
    let settings = Settings {
        base_url: base.into(),
        ..Default::default()
    };
    let mut app = App::new(settings, vec![], &UiState::default());
    app.on_msg(Msg::Auth(AuthEvent::LoggedIn {
        role: Role::Admin,
        session: Default::default(),
        epoch: 1,
    }));
    app
}

fn draw(app: &mut App) {
    let mut term = Terminal::new(TestBackend::new(160, 48)).unwrap();
    term.draw(|f| cpatop::ui::draw(f, app)).unwrap();
}

fn event(id: u32, ts: &str) -> serde_json::Value {
    json!({
        "id": id.to_string(), "timestamp": ts, "model": "gpt-6-luna", "source": "s",
        "failed": true, "status_code": 503, "latency_ms": 10,
        "tokens": {"input_tokens":0,"output_tokens":0,"reasoning_tokens":0,
                   "cache_read_tokens":0,"cache_creation_tokens":0,"total_tokens":0},
        "cost_usd": 0, "cost_available": false
    })
}

async fn recv_until<T>(
    rx: &mut mpsc::UnboundedReceiver<Msg>,
    mut f: impl FnMut(&Msg) -> Option<T>,
    app: Option<&mut App>,
) -> T {
    let mut app = app;
    loop {
        let m = timeout(Duration::from_secs(10), rx.recv())
            .await
            .expect("timed out waiting for a message")
            .expect("channel closed");
        let hit = f(&m);
        if let Some(a) = app.as_deref_mut() {
            a.on_msg(m);
        }
        if let Some(v) = hit {
            return v;
        }
    }
}

/// `e`/`m` filters are query parameters of the live stream *and* of the
/// history pages; paging continues with the cursor and the same filters.
#[tokio::test]
async fn request_log_filters_and_paging_are_server_side() {
    let server = MockServer::start().await;
    let snapshot = json!({
        "events": [event(10, "2026-11-13T10:00:00+05:30"), event(9, "2026-11-13T09:00:00+05:30")],
        "total_count": 3, "page": 1, "page_size": 50, "total_pages": 1,
        "next_cursor": "CURSOR1", "has_more": true,
        "range_start": "2026-11-13T00:00:00+05:30", "range_end": "2026-11-14T00:00:00+05:30",
        "end_exclusive": true
    });
    Mock::given(method("GET"))
        .and(path("/api/v1/usage/events/live"))
        .and(query_param("range", "today"))
        .and(query_param("result", "failed"))
        .and(query_param("model", "gpt-6-luna"))
        .and(query_param("cursor_mode", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            format!("event: snapshot\ndata: {snapshot}\n\n"),
            "text/event-stream",
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/usage/events"))
        .and(query_param("cursor", "CURSOR1"))
        .and(query_param("result", "failed"))
        .and(query_param("model", "gpt-6-luna"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "events": [event(8, "2026-11-13T08:00:00+05:30")],
            "total_count": -1, "page": 1, "page_size": 50, "total_pages": 0, "has_more": false
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let mut app = admin_app(&server.uri());
    app.tab = Tab::Events;
    app.live.filters = EventFilters {
        result: ResultFilter::Failed,
        model: Some("gpt-6-luna".into()),
        source: None,
    };
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
    let (live_tx, live_rx) = watch::channel(app.live_params());
    tokio::spawn(sse::run_live(client.clone(), live_rx, tx.clone()));
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    tokio::spawn(poller::run_commands(client, cmd_rx, tx.clone()));

    recv_until(
        &mut rx,
        |m| matches!(m, Msg::Live(LiveEvent::Snapshot(_))).then_some(()),
        Some(&mut app),
    )
    .await;
    assert_eq!(app.live.events.len(), 2);
    assert_eq!(app.live.total, 3);

    // Jump to the end of the table: the app asks for the next page.
    draw(&mut app);
    app.table_mut(cpatop::layout::PaneId::EvLog).selected = 1;
    app.after_draw();
    for e in app.drain_effects() {
        if let Effect::Fetch(c) = e {
            cmd_tx.send(c).unwrap();
        }
    }
    recv_until(
        &mut rx,
        |m| {
            matches!(
                m,
                Msg::Data {
                    payload: cpatop::poller::Payload::MoreEvents(_),
                    ..
                }
            )
            .then_some(())
        },
        Some(&mut app),
    )
    .await;
    assert_eq!(app.live.events.len(), 3, "the history page was merged");
    assert!(!app.live.has_more);
    drop(live_tx);
}

/// A history page that arrives after the filters changed is dropped.
#[test]
fn history_pages_for_old_filters_are_dropped() {
    let mut app = admin_app("http://127.0.0.1:9");
    let old_scope = app.events_scope();
    app.set_event_filters(EventFilters {
        result: ResultFilter::Success,
        ..Default::default()
    });
    let page: cpatop::api::models::EventsPage = serde_json::from_value(json!({
        "events": [event(1, "2026-11-13T08:00:00+05:30")], "has_more": false
    }))
    .unwrap();
    app.on_msg(Msg::Data {
        group: poller::Group::Detail,
        scope: old_scope,
        payload: cpatop::poller::Payload::MoreEvents(Ok(page)),
    });
    assert!(app.live.events.is_empty());
}

fn store_with(dir: &std::path::Path, entries: &[(&str, &str, &str)]) -> SessionStore {
    let store = SessionStore::new(Some(dir.join("session.json")));
    for (base, tok, role) in entries {
        store.save(base, tok, role).unwrap();
    }
    store
}

fn tempdir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("cpatop-http-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

async fn mount_session_endpoints(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/v1/startup/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"phase":"ready"})))
        .mount(server)
        .await;
    // /auth/session answers by cookie.
    Mock::given(method("GET"))
        .and(path("/api/v1/auth/session"))
        .respond_with(|req: &Request| {
            let cookie = req
                .headers
                .get("cookie")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            let body = if cookie.contains("admintok") {
                json!({"authenticated": true, "role": "admin"})
            } else if cookie.contains("keytok") {
                json!({"authenticated": true, "role": "api_key_viewer",
                       "api_key": {"id": "3", "display_key": "abc***…"}})
            } else {
                json!({"authenticated": false})
            };
            ResponseTemplate::new(200).set_body_json(body)
        })
        .mount(server)
        .await;
}

async fn next_auth(rx: &mut mpsc::UnboundedReceiver<Msg>) -> AuthEvent {
    loop {
        let m = timeout(Duration::from_secs(10), rx.recv())
            .await
            .expect("timed out")
            .expect("closed");
        if let Msg::Auth(e) = m
            && !matches!(e, AuthEvent::Connecting | AuthEvent::LoggingIn { .. })
        {
            return e;
        }
    }
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

/// An explicit `--api-key-file` wins over a stored admin session; after a
/// 401 the app logs in again with the same role.
#[tokio::test]
async fn explicit_credential_overrides_stored_session_and_relogs_after_401() {
    let server = MockServer::start().await;
    mount_session_endpoints(&server).await;
    Mock::given(method("POST"))
        .and(path("/api/v1/auth/api-key-login"))
        .and(header("X-CPA-Usage-Keeper-Request", "fetch"))
        .and(body_json(json!({"apiKey": "key-secret"})))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("set-cookie", format!("{COOKIE}=keytok; Path=/; HttpOnly")),
        )
        .expect(2)
        .mount(&server)
        .await;

    let dir = tempdir("override");
    let base = server.uri();
    let store = store_with(&dir, &[(&base, "admintok", "admin")]);
    let client = Client::new(&base).unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    tokio::spawn(AuthManager::new(client, store.clone(), key_creds(), tx, cmd_rx).run());

    match next_auth(&mut rx).await {
        AuthEvent::LoggedIn { role, epoch, .. } => {
            assert_eq!(role, Role::KeyViewer, "the explicit key decides the role");
            // A request in this session gets a 401.
            cmd_tx.send(AuthCmd::Expired(epoch)).unwrap();
        }
        e => panic!("expected login, got {e:?}"),
    }
    match next_auth(&mut rx).await {
        AuthEvent::LoggedIn { role, epoch, .. } => {
            assert_eq!(role, Role::KeyViewer);
            assert_eq!(epoch, 2);
        }
        e => panic!("expected re-login, got {e:?}"),
    }
    // The admin session is still stored for later; the key session too.
    assert_eq!(store.load(&base, "admin").as_deref(), Some("admintok"));
    assert_eq!(
        store.load(&base, "api_key_viewer").as_deref(),
        Some("keytok")
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Without an explicit credential a stored session is reused (no login).
#[tokio::test]
async fn stored_session_is_reused_without_explicit_credentials() {
    let server = MockServer::start().await;
    mount_session_endpoints(&server).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let dir = tempdir("reuse");
    let base = server.uri();
    // A session stored for another server is never sent here.
    let store = store_with(
        &dir,
        &[
            ("http://other.example", "keytok", "api_key_viewer"),
            (&base, "admintok", "admin"),
        ],
    );
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (_cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    tokio::spawn(
        AuthManager::new(
            Client::new(&base).unwrap(),
            store,
            Credentials::default(),
            tx,
            cmd_rx,
        )
        .run(),
    );
    match next_auth(&mut rx).await {
        AuthEvent::LoggedIn { role, .. } => assert_eq!(role, Role::Admin),
        e => panic!("expected reuse, got {e:?}"),
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `503 migration_in_progress` sends the app back to the startup gate, which
/// waits for `phase: ready` and resumes the session.
#[tokio::test]
async fn migration_gate_waits_for_ready() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/startup/status"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"phase":"migrating","message":"upgrading database"})),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    mount_session_endpoints(&server).await;
    Mock::given(method("GET"))
        .and(path("/api/v1/usage/overview"))
        .respond_with(ResponseTemplate::new(503).set_body_json(
            json!({"code":"migration_in_progress","message":"Service is starting."}),
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .with_priority(10)
        .mount(&server)
        .await;

    // The poller sees the 503 and the app asks for the gate.
    let base = server.uri();
    let mut app = admin_app(&base);
    let client = Client::new(&base).unwrap();
    client.set_token(Some("admintok".into()));
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (_qtx, qrx) = watch::channel(app.query());
    poller::spawn_pollers(&client, &qrx, &tx);
    recv_until(
        &mut rx,
        |m| {
            matches!(
                m,
                Msg::Data {
                    payload: cpatop::poller::Payload::Overview(Err(_)),
                    ..
                }
            )
            .then_some(())
        },
        Some(&mut app),
    )
    .await;
    let effects = app.drain_effects();
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::Auth(AuthCmd::Gate))),
        "{effects:?}"
    );

    // The auth manager shows the startup screen, then resumes.
    let dir = tempdir("gate");
    let store = store_with(&dir, &[(&base, "admintok", "admin")]);
    let (atx, mut arx) = mpsc::unbounded_channel();
    let (_cmd, cmd_rx) = mpsc::unbounded_channel();
    tokio::spawn(AuthManager::new(client, store, Credentials::default(), atx, cmd_rx).run());
    match next_auth(&mut arx).await {
        AuthEvent::Startup(s) => assert_eq!(s.phase, "migrating"),
        e => panic!("expected the startup gate, got {e:?}"),
    }
    match next_auth(&mut arx).await {
        AuthEvent::LoggedIn { role, .. } => assert_eq!(role, Role::Admin),
        e => panic!("expected resume, got {e:?}"),
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A slow response for the old range must not overwrite data for the new one.
#[tokio::test]
async fn stale_responses_after_a_range_change_are_rejected() {
    let server = MockServer::start().await;
    let overview = |n: u32| json!({"usage": {"total_requests": n}});
    Mock::given(method("GET"))
        .and(path("/api/v1/usage/overview"))
        .and(query_param("range", "today"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(overview(1))
                .set_delay(Duration::from_millis(600)),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/usage/overview"))
        .and(query_param("range", "7d"))
        .respond_with(ResponseTemplate::new(200).set_body_json(overview(7)))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .with_priority(10)
        .mount(&server)
        .await;

    let base = server.uri();
    let mut app = admin_app(&base);
    let client = Client::new(&base).unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (qtx, qrx) = watch::channel(app.query());
    poller::spawn_pollers(&client, &qrx, &tx);
    // Change the range while the `today` request is in flight.
    tokio::time::sleep(Duration::from_millis(100)).await;
    app.set_range(TimeRange::Days(7));
    qtx.send(app.query()).unwrap();

    let mut saw_old = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        let Ok(Some(m)) = timeout(Duration::from_millis(300), rx.recv()).await else {
            if saw_old && app.data.overview.get().is_some() {
                break;
            }
            continue;
        };
        if let Msg::Data { scope, .. } = &m
            && scope.contains("|today|")
        {
            saw_old = true;
        }
        app.on_msg(m);
        if let Some(o) = app.data.overview.get() {
            assert_eq!(o.usage.total_requests, 7.0, "stale data was applied");
        }
    }
    assert!(saw_old, "the slow response did arrive (and was dropped)");
    assert_eq!(app.data.overview.get().unwrap().usage.total_requests, 7.0);
}

/// Changing a filter reconnects the live stream with the new parameters.
#[tokio::test]
async fn live_stream_params_change_reconnects() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/usage/events/live"))
        .respond_with(|req: &Request| {
            let failed = req.url.query().unwrap_or("").contains("result=failed");
            let snap = json!({"events": [], "total_count": if failed { 41 } else { 2000 },
                              "has_more": false, "range_start": "", "range_end": ""});
            ResponseTemplate::new(200).set_body_raw(
                format!("event: snapshot\ndata: {snap}\n\n"),
                "text/event-stream",
            )
        })
        .mount(&server)
        .await;
    let client = Client::new(&server.uri()).unwrap();
    let mut params = LiveParams {
        enabled: true,
        range: Some(TimeRange::Today),
        ..Default::default()
    };
    let (ptx, prx) = watch::channel(params.clone());
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
    tokio::spawn(sse::run_live(client, prx, tx));
    let total = |m: &Msg| match m {
        Msg::Live(LiveEvent::Snapshot(s)) => Some(s.total_count),
        _ => None,
    };
    assert_eq!(recv_until(&mut rx, total, None).await, 2000);
    params.filters.result = ResultFilter::Failed;
    ptx.send(params).unwrap();
    // The next snapshot (after the reconnect) is the filtered one.
    loop {
        let t = recv_until(&mut rx, total, None).await;
        if t == 41 {
            break;
        }
    }
}

/// An explicit `--api-key-file` with a valid stored session of the same
/// role reuses it: no new server session per launch.
#[tokio::test]
async fn explicit_credential_reuses_a_valid_stored_session_of_its_role() {
    let server = MockServer::start().await;
    mount_session_endpoints(&server).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let dir = tempdir("reuse-explicit");
    let base = server.uri();
    let store = store_with(
        &dir,
        &[
            (&base, "admintok", "admin"),
            (&base, "keytok", "api_key_viewer"),
        ],
    );
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (_cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    tokio::spawn(
        AuthManager::new(Client::new(&base).unwrap(), store, key_creds(), tx, cmd_rx).run(),
    );
    match next_auth(&mut rx).await {
        AuthEvent::LoggedIn { role, .. } => assert_eq!(role, Role::KeyViewer),
        e => panic!("expected reuse, got {e:?}"),
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
