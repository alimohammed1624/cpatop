//! Session lifecycle: startup gate, cookie reuse, automatic and interactive login.

use crate::api::models::{SessionResponse, StartupStatus};
use crate::api::{ApiError, Client, LoginMode, Role};
use crate::poller::Msg;
use crate::secrets::{Credential, Credentials, Secret, Source};
use crate::session_store::SessionStore;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;

/// Events sent to the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum AuthEvent {
    Connecting,
    Startup(StartupStatus),
    Offline {
        message: String,
        retry_in: Duration,
    },
    LoggingIn {
        mode: LoginMode,
        source: String,
    },
    NeedLogin {
        error: Option<String>,
        mode: LoginMode,
    },
    LoggedIn {
        role: Role,
        session: SessionResponse,
        epoch: u64,
    },
    LoggedOut,
}

/// Commands from the UI.
#[derive(Debug, Clone)]
pub enum AuthCmd {
    Login(LoginMode, Secret),
    /// A request in session `epoch` returned 401.
    Expired(u64),
    Logout,
    RetryNow,
}

pub struct AuthManager {
    client: Client,
    store: SessionStore,
    creds: Credentials,
    tx: mpsc::UnboundedSender<Msg>,
    rx: mpsc::UnboundedReceiver<AuthCmd>,
    epoch: u64,
    /// Secret entered on the login screen, kept in memory for re-login.
    interactive: Option<Credential>,
    last_auto_login: Option<Instant>,
    /// Set once a configured credential was rejected, to avoid retry loops.
    auto_disabled: bool,
}

/// User-facing description of an error during connection.
pub fn describe(e: &ApiError) -> String {
    match e {
        ApiError::Network(m) => format!("cannot reach server: {m}"),
        ApiError::Http { status: 404, .. } => {
            "404 not found — is the base URL a CPA Usage Keeper (check any sub-path)?".into()
        }
        ApiError::Decode(m) => m.clone(),
        other => other.to_string(),
    }
}

enum Wait {
    Elapsed,
    Retry,
    Login(LoginMode, Secret),
    Closed,
}

impl AuthManager {
    pub fn new(
        client: Client,
        store: SessionStore,
        creds: Credentials,
        tx: mpsc::UnboundedSender<Msg>,
        rx: mpsc::UnboundedReceiver<AuthCmd>,
    ) -> Self {
        AuthManager {
            client,
            store,
            creds,
            tx,
            rx,
            epoch: 0,
            interactive: None,
            last_auto_login: None,
            auto_disabled: false,
        }
    }

    fn send(&self, e: AuthEvent) {
        let _ = self.tx.send(Msg::Auth(e));
    }

    /// Wait for `d`, returning early on a command that matters.
    async fn wait(&mut self, d: Duration) -> Wait {
        let sleep = tokio::time::sleep(d);
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                _ = &mut sleep => return Wait::Elapsed,
                cmd = self.rx.recv() => match cmd {
                    None => return Wait::Closed,
                    Some(AuthCmd::RetryNow) => return Wait::Retry,
                    Some(AuthCmd::Login(m, s)) => return Wait::Login(m, s),
                    Some(_) => {}
                }
            }
        }
    }

    pub async fn run(mut self) {
        self.establish(true).await;
        while let Some(cmd) = self.rx.recv().await {
            match cmd {
                AuthCmd::Login(mode, secret) => self.interactive_login(mode, secret).await,
                AuthCmd::Expired(epoch) => {
                    if epoch != self.epoch {
                        continue;
                    }
                    self.client.set_token(None);
                    let _ = self.store.remove(self.client.base_url());
                    // Avoid hammering the login endpoint if a fresh login expires at once.
                    let recent = self
                        .last_auto_login
                        .is_some_and(|t| t.elapsed() < Duration::from_secs(30));
                    if recent {
                        self.send(AuthEvent::NeedLogin {
                            error: Some("session expired immediately after login".into()),
                            mode: self.creds.preferred_mode(),
                        });
                    } else {
                        self.establish(true).await;
                    }
                }
                AuthCmd::Logout => {
                    let _ = self.client.logout().await;
                    let _ = self.store.remove(self.client.base_url());
                    self.interactive = None;
                    self.auto_disabled = true;
                    self.epoch += 1;
                    self.send(AuthEvent::LoggedOut);
                    self.send(AuthEvent::NeedLogin {
                        error: None,
                        mode: self.creds.preferred_mode(),
                    });
                }
                AuthCmd::RetryNow => self.establish(true).await,
            }
        }
    }

    async fn interactive_login(&mut self, mode: LoginMode, secret: Secret) {
        let cred = Credential {
            mode,
            secret,
            source: Source::Prompt,
        };
        match self.login_with(&cred).await {
            Ok(()) => self.interactive = Some(cred),
            Err((msg, _)) => self.send(AuthEvent::NeedLogin {
                error: Some(msg),
                mode,
            }),
        }
    }

    /// Log in with a credential and announce the session. Errors are
    /// user-facing; the flag is true when the server rejected the credential.
    async fn login_with(&mut self, cred: &Credential) -> Result<(), (String, bool)> {
        self.send(AuthEvent::LoggingIn {
            mode: cred.mode,
            source: cred.source.to_string(),
        });
        match self.client.login(cred.mode, &cred.secret).await {
            Ok(token) => match self.client.session().await {
                Ok(s) if s.authenticated => {
                    let role = s.role.clone().unwrap_or_default();
                    if let Err(e) = self.store.save(self.client.base_url(), &token, &role) {
                        // Not fatal; the session simply won't be reused next time.
                        let _ = e;
                    }
                    self.announce(s);
                    Ok(())
                }
                Ok(_) => Err((
                    "login accepted but the session is not authenticated".into(),
                    true,
                )),
                Err(e) => Err((describe(&e), false)),
            },
            Err(ApiError::Unauthorized(m)) => Err((format!("{m} (from {})", cred.source), true)),
            Err(ApiError::RateLimited { retry_after, .. }) => Err((
                format!(
                    "too many login attempts — try again in {}s",
                    retry_after.unwrap_or(60)
                ),
                false,
            )),
            Err(e) => Err((describe(&e), false)),
        }
    }

    fn announce(&mut self, s: SessionResponse) {
        match Role::from_session(s.role.as_deref()) {
            Some(role) => {
                self.epoch += 1;
                self.send(AuthEvent::LoggedIn {
                    role,
                    session: s,
                    epoch: self.epoch,
                });
            }
            None => self.send(AuthEvent::NeedLogin {
                error: Some(format!("unsupported session role {:?}", s.role)),
                mode: self.creds.preferred_mode(),
            }),
        }
    }

    /// Bring the client to an authenticated state, or ask for a login.
    async fn establish(&mut self, allow_auto: bool) {
        let mut attempt = 0u32;
        self.send(AuthEvent::Connecting);
        loop {
            // 1. Startup gate.
            let gate = match self.client.startup_status().await {
                Ok(s) if s.phase == "ready" => Ok(()),
                Ok(s) => {
                    self.send(AuthEvent::Startup(s));
                    Err(Duration::from_millis(2500))
                }
                Err(e) => {
                    let d = crate::api::sse::backoff(attempt.min(4));
                    attempt += 1;
                    self.send(AuthEvent::Offline {
                        message: describe(&e),
                        retry_in: d,
                    });
                    Err(d)
                }
            };
            if let Err(d) = gate {
                match self.wait(d).await {
                    Wait::Closed => return,
                    Wait::Login(m, s) => {
                        self.interactive_login(m, s).await;
                        return;
                    }
                    _ => {
                        self.send(AuthEvent::Connecting);
                        continue;
                    }
                }
            }

            // 2. Reuse a stored session cookie.
            if let Some(tok) = self.store.load(self.client.base_url()) {
                self.client.set_token(Some(tok));
            }
            match self.client.session().await {
                Ok(s) if s.authenticated => {
                    self.announce(s);
                    return;
                }
                Ok(_) => {
                    if self.client.has_token() {
                        let _ = self.store.remove(self.client.base_url());
                    }
                    self.client.set_token(None);
                }
                Err(e) => {
                    let d = crate::api::sse::backoff(attempt.min(4));
                    attempt += 1;
                    self.send(AuthEvent::Offline {
                        message: describe(&e),
                        retry_in: d,
                    });
                    match self.wait(d).await {
                        Wait::Closed => return,
                        Wait::Login(m, s) => {
                            self.interactive_login(m, s).await;
                            return;
                        }
                        _ => continue,
                    }
                }
            }

            // 3. Log in with a configured or previously entered credential.
            let cred = self.interactive.clone().or_else(|| {
                (!self.auto_disabled)
                    .then(|| self.creds.pick().cloned())
                    .flatten()
            });
            if allow_auto && let Some(cred) = cred {
                self.last_auto_login = Some(Instant::now());
                match self.login_with(&cred).await {
                    Ok(()) => return,
                    Err((msg, rejected)) => {
                        if rejected {
                            if cred.source == Source::Prompt {
                                self.interactive = None;
                            } else {
                                self.auto_disabled = true;
                            }
                        }
                        self.send(AuthEvent::NeedLogin {
                            error: Some(msg),
                            mode: cred.mode,
                        });
                        return;
                    }
                }
            }

            // 4. Ask the user.
            self.send(AuthEvent::NeedLogin {
                error: None,
                mode: self.creds.preferred_mode(),
            });
            return;
        }
    }
}
