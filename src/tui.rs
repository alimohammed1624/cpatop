//! Terminal setup and the main event loop.

use crate::api::Client;
use crate::api::sse::{self, LiveParams};
use crate::app::{App, Effect};
use crate::auth::{AuthCmd, AuthEvent, AuthManager};
use crate::config::UiState;
use crate::poller::{self, Msg, Query};
use crate::secrets::Credentials;
use crate::session_store::SessionStore;
use color_eyre::eyre::{Result, eyre};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture, EventStream};
use futures_util::StreamExt;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

pub struct RunArgs {
    pub app: App,
    pub creds: Credentials,
    pub store: SessionStore,
    pub ui_state_path: Option<PathBuf>,
    /// Run the background release check (see `crate::update`).
    pub update_check: bool,
    pub update_cache_path: Option<PathBuf>,
}

pub async fn run(args: RunArgs) -> Result<()> {
    let RunArgs {
        mut app,
        creds,
        store,
        ui_state_path,
        update_check,
        update_cache_path,
    } = args;
    let client = Client::new(&app.settings.base_url).map_err(|e| eyre!(e))?;

    let (tx, mut rx) = mpsc::unbounded_channel();
    let (query_tx, query_rx) = watch::channel(Query::default());
    let (live_tx, live_rx) = watch::channel(LiveParams::default());
    let (auth_tx, auth_rx) = mpsc::unbounded_channel();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

    poller::spawn_pollers(&client, &query_rx, &tx);
    tokio::spawn(sse::run_live(client.clone(), live_rx, tx.clone()));
    tokio::spawn(poller::run_commands(client.clone(), cmd_rx, tx.clone()));
    tokio::spawn(AuthManager::new(client.clone(), store, creds, tx.clone(), auth_rx).run());
    let update_tx = update_check.then(|| tx.clone());
    drop(tx);

    let mouse = app.settings.mouse;
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::execute!(std::io::stdout(), DisableMouseCapture);
        prev(info);
    }));
    let mut terminal = ratatui::init();
    if mouse {
        crossterm::execute!(std::io::stdout(), EnableMouseCapture)?;
    }
    // After the UI is up and never awaited: a slow or failing check only
    // means no badge.
    if let Some(tx) = update_tx {
        tokio::spawn(crate::update::background(update_cache_path, move |tag| {
            let _ = tx.send(Msg::UpdateAvailable(tag));
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
    let result: Result<()> = loop {
        if let Err(e) = terminal.draw(|f| crate::ui::draw(f, &mut app)) {
            break Err(e.into());
        }
        app.after_draw();
        let mut dirty = false;
        for e in app.drain_effects() {
            match e {
                Effect::Auth(c) => {
                    let _ = auth_tx.send(c);
                }
                Effect::Fetch(c) => {
                    let _ = cmd_tx.send(c);
                }
                Effect::SaveState => dirty = true,
            }
        }
        if dirty {
            save(&app);
        }
        let q = app.query();
        query_tx.send_if_modified(|cur| {
            if *cur != q {
                *cur = q;
                true
            } else {
                false
            }
        });
        let lp = app.live_params();
        live_tx.send_if_modified(|cur| {
            if *cur != lp {
                *cur = lp;
                true
            } else {
                false
            }
        });
        if app.should_quit {
            break Ok(());
        }
        tokio::select! {
            ev = events.next() => match ev {
                Some(Ok(e)) => app.on_event(e),
                Some(Err(e)) => break Err(e.into()),
                None => break Ok(()),
            },
            Some(msg) = rx.recv() => {
                app.on_msg(msg);
                while let Ok(m) = rx.try_recv() {
                    app.on_msg(m);
                }
            }
            _ = tick.tick() => app.on_tick(),
        }
    };

    if mouse {
        let _ = crossterm::execute!(std::io::stdout(), DisableMouseCapture);
    }
    ratatui::restore();
    save(&app);
    // `--logout-on-exit`: end the server session too (q alone keeps it for
    // the next start). Wait briefly for the auth task to confirm.
    if app.settings.logout_on_exit && app.role.is_some() && auth_tx.send(AuthCmd::Logout).is_ok() {
        let wait = async {
            while let Some(m) = rx.recv().await {
                if matches!(m, Msg::Auth(AuthEvent::LoggedOut)) {
                    break;
                }
            }
        };
        let _ = tokio::time::timeout(Duration::from_secs(5), wait).await;
    }
    result
}
