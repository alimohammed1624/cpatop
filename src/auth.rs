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
    /// The server answered `503 migration_in_progress`: wait for the
    /// startup gate again, then resume the session.
    Gate,
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
    /// Consecutive expiries shortly after an automatic login.
    quick_expiries: u32,
    /// Set once a configured credential was rejected, to avoid retry loops.
    auto_disabled: bool,
    /// Server role of the current session (`admin`, `api_key_viewer`).
    role: Option<String>,
}

/// Server role string for a login mode.
pub fn role_of(mode: LoginMode) -> &'static str {
    match mode {
        LoginMode::Admin => "admin",
        LoginMode::ApiKey => "api_key_viewer",
    }
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
            quick_expiries: 0,
            auto_disabled: false,
            role: None,
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
        self.establish(true, None).await;
        while let Some(cmd) = self.rx.recv().await {
            match cmd {
                AuthCmd::Login(mode, secret) => self.interactive_login(mode, secret).await,
                AuthCmd::Expired(epoch) => {
                    if epoch != self.epoch {
                        continue;
                    }
                    if let Some(tok) = self.client.token() {
                        let _ = self.store.remove_token(self.client.base_url(), &tok);
                    }
                    self.client.set_token(None);
                    // Re-login automatically, but stop if sessions keep dying
                    // right after login (e.g. a revoked key or clock issue).
                    let quick = self
                        .last_auto_login
                        .is_some_and(|t| t.elapsed() < Duration::from_secs(30));
                    self.quick_expiries = if quick { self.quick_expiries + 1 } else { 0 };
                    if self.quick_expiries >= 2 {
                        self.quick_expiries = 0;
                        self.send(AuthEvent::NeedLogin {
                            error: Some("session keeps expiring right after login".into()),
                            mode: self.creds.preferred_mode(),
                        });
                    } else {
                        let want = self.role.clone();
                        self.establish(true, want).await;
                    }
                }
                AuthCmd::Logout => {
                    let _ = self.client.logout().await;
                    let _ = self
                        .store
                        .remove(self.client.base_url(), self.role.as_deref());
                    self.role = None;
                    self.interactive = None;
                    self.auto_disabled = true;
                    self.epoch += 1;
                    self.send(AuthEvent::LoggedOut);
                    self.send(AuthEvent::NeedLogin {
                        error: None,
                        mode: self.creds.preferred_mode(),
                    });
                }
                AuthCmd::RetryNow => self.establish(true, None).await,
                AuthCmd::Gate => {
                    let want = self.role.clone();
                    self.establish(true, want).await
                }
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
                self.role = s.role.clone();
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

    /// The credential to log in with automatically, if any.
    fn auto_credential(&self) -> Option<Credential> {
        self.interactive.clone().or_else(|| {
            (!self.auto_disabled)
                .then(|| self.creds.pick().cloned())
                .flatten()
        })
    }

    /// The server role the session must have. An explicitly supplied
    /// credential (file, environment or login screen) decides it, so
    /// `--api-key-file` is never shadowed by a stored admin session. After an
    /// expiry the previous role is kept.
    fn wanted_role(&self, want: Option<String>) -> Option<String> {
        let explicit = self
            .interactive
            .as_ref()
            .or_else(|| self.creds.pick_explicit().filter(|_| !self.auto_disabled));
        match explicit {
            Some(c) => Some(role_of(c.mode).to_string()),
            None => want.or_else(|| self.creds.prefer.map(|m| role_of(m).to_string())),
        }
    }

    /// Bring the client to an authenticated state, or ask for a login.
    /// `want` is the role to restore (after an expiry or a startup gate).
    async fn establish(&mut self, allow_auto: bool, want: Option<String>) {
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

            // 2. Reuse a session: the one the client holds, then stored ones
            //    for this server — only those with the wanted role.
            let wanted = self.wanted_role(want.clone());
            let base = self.client.base_url().to_string();
            let mut candidates: Vec<Option<String>> = Vec::new();
            if self.client.has_token() {
                candidates.push(self.client.token());
            }
            let mut stored = self.store.load_all(&base);
            if let Some(w) = &wanted {
                stored.retain(|s| &s.role == w);
            }
            candidates.extend(stored.into_iter().map(|s| Some(s.token)));
            if candidates.is_empty() {
                // Still ask the server, to detect connectivity problems.
                candidates.push(None);
            }
            let mut offline = None;
            for tok in candidates {
                self.client.set_token(tok.clone());
                match self.client.session().await {
                    Ok(s) if s.authenticated => {
                        let role_ok = wanted.is_none() || s.role.as_deref() == wanted.as_deref();
                        if role_ok {
                            self.announce(s);
                            return;
                        }
                        // A valid session of another role: keep it stored
                        // for later, but do not use it now.
                    }
                    Ok(_) => {
                        if let Some(t) = &tok {
                            let _ = self.store.remove_token(&base, t);
                        }
                    }
                    Err(e) => {
                        offline = Some(e);
                        break;
                    }
                }
            }
            self.client.set_token(None);
            if let Some(e) = offline {
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

            // 3. Log in with a configured or previously entered credential.
            let cred = self.auto_credential();
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
