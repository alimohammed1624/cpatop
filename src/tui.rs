//! Terminal setup and the main event loop.

use crate::api::Client;
use crate::api::sse::{self, LiveParams};
use crate::app::{App, Effect};
use crate::auth::{AuthCmd, AuthEvent, AuthManager};
use crate::config::UiState;
use crate::poller::{self, Command, Msg, Query};
use crate::secrets::Credentials;
use crate::server::{self, BaseUrlSource, ProbeError};
use crate::session_store::SessionStore;
use color_eyre::eyre::{Result, eyre};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture, EventStream};
use futures_util::{FutureExt, StreamExt};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

pub struct RunArgs {
    pub app: App,
    pub creds: Credentials,
    pub store: SessionStore,
    pub ui_state_path: Option<PathBuf>,
    /// Run the background release check (see `crate::update`).
    pub update_check: bool,
    pub update_cache_path: Option<PathBuf>,
    /// Config file a changed server URL is saved to (`--config` or the XDG
    /// default).
    pub config_path: Option<PathBuf>,
    pub base_url_source: BaseUrlSource,
    /// Look up keyring credentials for a server switched to at runtime.
    pub use_keyring: bool,
}

/// Everything that talks to one server: the client, pollers, live stream,
/// command runner and auth manager, and the channels to them. Switching
/// servers retires the whole set and spawns a new one, so no request goes to
/// the old server afterwards and no stale message reaches the app.
pub struct Backend {
    pub client: Client,
    query_tx: watch::Sender<Query>,
    live_tx: watch::Sender<LiveParams>,
    pub auth_tx: mpsc::UnboundedSender<AuthCmd>,
    cmd_tx: mpsc::UnboundedSender<Command>,
    pub rx: mpsc::UnboundedReceiver<Msg>,
    tasks: Vec<JoinHandle<()>>,
    auth: JoinHandle<()>,
}

impl Backend {
    /// Connect to `base_url`. `creds` are looked up for that server (keyring
    /// entries are per server) unless `exact` is set.
    pub fn spawn(
        base_url: &str,
        creds: &Credentials,
        exact: bool,
        use_keyring: bool,
        store: SessionStore,
    ) -> Result<Backend, String> {
        let client = Client::new(base_url)?;
        let (tx, rx) = mpsc::unbounded_channel();
        let (query_tx, query_rx) = watch::channel(Query::default());
        let (live_tx, live_rx) = watch::channel(LiveParams::default());
        let (auth_tx, auth_rx) = mpsc::unbounded_channel();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let mut tasks = poller::spawn_pollers(&client, &query_rx, &tx);
        tasks.push(tokio::spawn(sse::run_live(
            client.clone(),
            live_rx,
            tx.clone(),
        )));
        tasks.push(tokio::spawn(poller::run_commands(
            client.clone(),
            cmd_rx,
            tx.clone(),
        )));
        let auth = if exact {
            tokio::spawn(AuthManager::new(client.clone(), store, creds.clone(), tx, auth_rx).run())
        } else {
            let (c, creds, url) = (client.clone(), creds.clone(), base_url.to_string());
            tokio::spawn(async move {
                // The keyring may block briefly (D-Bus).
                let creds = tokio::task::spawn_blocking(move || {
                    server::credentials_for(&creds, &url, use_keyring)
                })
                .await
                .unwrap_or_default();
                AuthManager::new(c, store, creds, tx, auth_rx).run().await
            })
        };
        Ok(Backend {
            client,
            query_tx,
            live_tx,
            auth_tx,
            cmd_tx,
            rx,
            tasks,
            auth,
        })
    }

    /// Forward the app's requests and its current query / live parameters.
    pub fn send(&self, app: &App, effects: Vec<Effect>) {
        for e in effects {
            match e {
                Effect::Auth(c) => {
                    let _ = self.auth_tx.send(c);
                }
                Effect::Fetch(c) => {
                    let _ = self.cmd_tx.send(c);
                }
                _ => {}
            }
        }
        let q = app.query();
        self.query_tx.send_if_modified(|cur| {
            if *cur != q {
                *cur = q;
                true
            } else {
                false
            }
        });
        let lp = app.live_params();
        self.live_tx.send_if_modified(|cur| {
            if *cur != lp {
                *cur = lp;
                true
            } else {
                false
            }
        });
    }

    /// Stop everything at once; the auth manager first logs its session out
    /// when `logout` is set. Returns the auth task, which may still be doing
    /// that.
    pub fn retire(self, logout: bool) -> JoinHandle<()> {
        for t in &self.tasks {
            t.abort();
        }
        let _ = self.auth_tx.send(AuthCmd::Shutdown { logout });
        self.auth
    }
}

/// Results from tasks that outlive a backend.
enum Aux {
    Update(String),
    Probe {
        id: u64,
        url: String,
        result: Result<(), ProbeError>,
    },
}

pub async fn run(args: RunArgs) -> Result<()> {
    let RunArgs {
        mut app,
        creds,
        store,
        ui_state_path,
        update_check,
        update_cache_path,
        config_path,
        base_url_source,
        use_keyring,
    } = args;
    app.config_path = config_path.clone();
    app.base_url_source = base_url_source;
    let mut backend = Backend::spawn(
        &app.settings.base_url,
        &creds,
        true,
        use_keyring,
        store.clone(),
    )
    .map_err(|e| eyre!(e))?;
    // Auth tasks of servers switched away from, finishing their logout.
    let mut retired: Vec<JoinHandle<()>> = Vec::new();
    let (aux_tx, mut aux_rx) = mpsc::unbounded_channel();

    let mouse = app.settings.mouse;
    let mut terminal = ratatui::init();
    // `ratatui::init` restores raw mode and the alternate screen on a panic;
    // mouse capture and the cursor are ours to undo, before its hook runs.
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::execute!(
            std::io::stdout(),
            DisableMouseCapture,
            crossterm::cursor::Show
        );
        prev(info);
    }));
    // TERM / HUP / INT end the run like `q`: the terminal is restored and
    // `--logout-on-exit` honoured below.
    let mut signals = Signals::new();
    let mouse_on = if mouse {
        crossterm::execute!(std::io::stdout(), EnableMouseCapture).map_err(color_eyre::Report::from)
    } else {
        Ok(())
    };
    // After the UI is up and never awaited: a slow or failing check only
    // means no badge.
    if update_check {
        let tx = aux_tx.clone();
        tokio::spawn(crate::update::background(update_cache_path, move |tag| {
            let _ = tx.send(Aux::Update(tag));
        }));
    }

    let save = |app: &App| {
        if let Some(p) = &ui_state_path {
            let st: UiState = app.ui_state();
            let _ = st.save(p);
        }
    };

    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // Every way out of the loop — `q`, a signal, an error, even a panic —
    // reaches the teardown after it.
    let ui = async {
        mouse_on?;
        loop {
            if let Err(e) = terminal.draw(|f| crate::ui::draw(f, &mut app)) {
                break Err(e.into());
            }
            app.after_draw();
            let mut dirty = false;
            let mut forward = Vec::new();
            for e in app.drain_effects() {
                match e {
                    Effect::SaveState => dirty = true,
                    Effect::ProbeServer { id, url } => {
                        let tx = aux_tx.clone();
                        tokio::spawn(async move {
                            let result = server::probe(&url, server::PROBE_TIMEOUT).await;
                            let _ = tx.send(Aux::Probe { id, url, result });
                        });
                    }
                    Effect::SwitchServer(url) => {
                        match Backend::spawn(&url, &creds, false, use_keyring, store.clone()) {
                            Ok(new) => {
                                let old = std::mem::replace(&mut backend, new);
                                retired.push(old.retire(app.settings.logout_on_exit));
                                // Requests queued for the old server are dropped.
                                forward.clear();
                                let saved = match &config_path {
                                    Some(p) => server::save_base_url(p, &url).map(|()| p.clone()),
                                    None => Err("no config location".into()),
                                };
                                app.on_server_saved(saved);
                            }
                            Err(e) => app.on_server_switch_failed(e),
                        }
                    }
                    other => forward.push(other),
                }
            }
            if dirty {
                save(&app);
            }
            backend.send(&app, forward);
            if app.should_quit {
                break Ok(());
            }
            tokio::select! {
                ev = events.next() => match ev {
                    Some(Ok(e)) => app.on_event(e),
                    Some(Err(e)) => break Err(e.into()),
                    None => break Ok(()),
                },
                Some(msg) = backend.rx.recv() => {
                    app.on_msg(msg);
                    while let Ok(m) = backend.rx.try_recv() {
                        app.on_msg(m);
                    }
                }
                Some(aux) = aux_rx.recv() => match aux {
                    Aux::Update(tag) => app.on_msg(Msg::UpdateAvailable(tag)),
                    Aux::Probe { id, url, result } => app.on_server_probe(id, url, result),
                },
                _ = tick.tick() => app.on_tick(),
                _ = signals.recv() => break Ok(()),
            }
        }
    };
    let result: Result<()> = match std::panic::AssertUnwindSafe(ui).catch_unwind().await {
        Ok(r) => r,
        Err(_) => Err(eyre!("internal error (panic); see the message above")),
    };

    // Never panics: with SIGHUP the terminal may already be gone.
    let _ = crossterm::execute!(
        std::io::stdout(),
        DisableMouseCapture,
        crossterm::cursor::Show
    );
    let _ = ratatui::try_restore();
    save(&app);
    // `--logout-on-exit`: end the server session too (q alone keeps it for
    // the next start). Wait briefly for the auth task to confirm; another
    // signal (Ctrl-C now that the terminal is restored) stops waiting.
    let logout = app.settings.logout_on_exit;
    let role = app.role.is_some();
    let ending = async move {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        if logout && role && backend.auth_tx.send(AuthCmd::Logout).is_ok() {
            let wait = async {
                while let Some(m) = backend.rx.recv().await {
                    if matches!(m, Msg::Auth(AuthEvent::LoggedOut)) {
                        break;
                    }
                }
            };
            let _ = tokio::time::timeout_at(deadline, wait).await;
        }
        // A session still being set up (or not yet announced to the app) is
        // ended by the auth task itself as it stops.
        let current = backend.retire(logout);
        if logout {
            let _ = tokio::time::timeout_at(deadline, current).await;
        }
        // Servers switched away from may still be ending their sessions.
        for h in retired {
            let _ = tokio::time::timeout_at(deadline, h).await;
        }
    };
    tokio::select! {
        _ = ending => {}
        _ = signals.recv() => {}
    }
    result
}

/// Termination signals: SIGTERM, SIGHUP and SIGINT on Unix (Ctrl-C in the
/// app arrives as a key, but `kill -INT` does not), Ctrl-C elsewhere.
struct Signals {
    #[cfg(unix)]
    streams: Vec<(&'static str, tokio::signal::unix::Signal)>,
}

impl Signals {
    fn new() -> Signals {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let streams = [
                ("SIGTERM", SignalKind::terminate()),
                ("SIGHUP", SignalKind::hangup()),
                ("SIGINT", SignalKind::interrupt()),
            ]
            .into_iter()
            .filter_map(|(n, k)| signal(k).ok().map(|s| (n, s)))
            .collect();
            Signals { streams }
        }
        #[cfg(not(unix))]
        Signals {}
    }

    /// Wait for the next signal; returns its name.
    async fn recv(&mut self) -> &'static str {
        #[cfg(unix)]
        {
            if self.streams.is_empty() {
                return std::future::pending().await;
            }
            let futs = self.streams.iter_mut().map(|(n, s)| {
                let n = *n;
                Box::pin(async move {
                    s.recv().await;
                    n
                })
            });
            futures_util::future::select_all(futs).await.0
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
            "Ctrl-C"
        }
    }
}
