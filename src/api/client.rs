//! HTTP client for the Keeper API.
//!
//! The session cookie is handled explicitly instead of through a cookie jar so
//! that a `401` (whose response clears the cookie) never overwrites a session
//! we still want to persist, and so the token can be saved to disk.

use super::models::{ErrorBody, SessionResponse, StartupStatus};
use crate::secrets::Secret;
use reqwest::{StatusCode, header};
use serde::de::DeserializeOwned;
use std::sync::{Arc, RwLock};
use std::time::Duration;

pub const COOKIE_NAME: &str = "cpa_usage_keeper_session";
const INTENT_HEADER: &str = "X-CPA-Usage-Keeper-Request";

/// Errors surfaced to the UI. Messages never contain credentials.
#[derive(Debug, Clone, PartialEq)]
pub enum ApiError {
    /// 401: no session, expired or revoked.
    Unauthorized(String),
    /// 403: wrong role or feature disabled.
    Forbidden(String),
    /// 503 `migration_in_progress`.
    Migrating,
    /// 503 `costs_busy`.
    CostsBusy,
    /// 429 with an optional `Retry-After` in seconds.
    RateLimited {
        retry_after: Option<u64>,
        message: String,
    },
    /// Any other non-2xx response.
    Http {
        status: u16,
        code: Option<String>,
        message: String,
    },
    /// Connection refused, DNS, TLS, timeout…
    Network(String),
    /// The body did not match the expected shape.
    Decode(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Unauthorized(m) => write!(f, "401 {m}"),
            ApiError::Forbidden(m) => write!(f, "403 {m}"),
            ApiError::Migrating => f.write_str("server is starting (migration in progress)"),
            ApiError::CostsBusy => f.write_str("cost statistics are being updated"),
            ApiError::RateLimited { message, .. } => write!(f, "429 {message}"),
            ApiError::Http {
                status, message, ..
            } => write!(f, "{status} {message}"),
            ApiError::Network(m) => write!(f, "network: {m}"),
            ApiError::Decode(m) => write!(f, "unexpected response: {m}"),
        }
    }
}

impl std::error::Error for ApiError {}

/// Strip URLs and query strings from reqwest errors to keep messages short.
fn network_message(e: &reqwest::Error) -> String {
    let mut msg = if e.is_timeout() {
        "timed out".to_string()
    } else if e.is_connect() {
        "connection failed".to_string()
    } else {
        "request failed".to_string()
    };
    let mut src: Option<&dyn std::error::Error> = std::error::Error::source(e);
    let mut last = None;
    while let Some(s) = src {
        last = Some(s.to_string());
        src = s.source();
    }
    if let Some(l) = last {
        msg.push_str(": ");
        msg.push_str(&l);
    }
    msg
}

/// Normalise a user-supplied base URL: trim, drop trailing `/` and `/api/v1`.
pub fn normalize_base_url(raw: &str) -> Result<String, String> {
    let mut s = raw.trim().trim_end_matches('/').to_string();
    if let Some(stripped) = s.strip_suffix("/api/v1") {
        s = stripped.to_string();
    }
    let parsed = url::Url::parse(&s).map_err(|e| format!("invalid base URL {s:?}: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!(
            "base URL must be http or https, got {:?}",
            parsed.scheme()
        ));
    }
    if parsed.host_str().is_none() {
        return Err(format!("base URL {s:?} has no host"));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("base URL must not contain credentials".into());
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("base URL must not contain a query or fragment".into());
    }
    Ok(s.trim_end_matches('/').to_string())
}

/// Host (and port) part of a base URL, for display.
pub fn display_host(base: &str) -> String {
    url::Url::parse(base)
        .ok()
        .map(|u| {
            let mut h = u.host_str().unwrap_or("?").to_string();
            if let Some(p) = u.port() {
                h.push_str(&format!(":{p}"));
            }
            let path = u.path().trim_end_matches('/');
            if !path.is_empty() {
                h.push_str(path);
            }
            h
        })
        .unwrap_or_else(|| base.to_string())
}

#[derive(Debug)]
struct Inner {
    http: reqwest::Client,
    base: String,
    token: RwLock<Option<String>>,
}

/// Cheap-to-clone API client.
#[derive(Debug, Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

pub type Query = Vec<(&'static str, String)>;

impl Client {
    pub fn new(base_url: &str) -> Result<Self, String> {
        let base = normalize_base_url(base_url)?;
        // reqwest is built without a default crypto provider; install ring once.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .user_agent(concat!("cpatop/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(5))
            .read_timeout(Duration::from_secs(50))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| format!("http client: {e}"))?;
        Ok(Client {
            inner: Arc::new(Inner {
                http,
                base,
                token: RwLock::new(None),
            }),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.inner.base
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}/api/v1{}", self.inner.base, path)
    }

    pub fn set_token(&self, token: Option<String>) {
        *self.inner.token.write().expect("token lock") = token;
    }

    pub fn token(&self) -> Option<String> {
        self.inner.token.read().expect("token lock").clone()
    }

    pub fn has_token(&self) -> bool {
        self.inner.token.read().expect("token lock").is_some()
    }

    fn request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        let mut rb = self
            .inner
            .http
            .request(method, url)
            .header(INTENT_HEADER, "fetch");
        if let Some(t) = self.token() {
            rb = rb.header(header::COOKIE, format!("{COOKIE_NAME}={t}"));
        }
        rb
    }

    /// Build a GET request for an API path (used by the SSE client).
    pub fn get_builder(&self, path: &str, query: &Query) -> reqwest::RequestBuilder {
        self.request(reqwest::Method::GET, &self.url(path))
            .query(query)
    }

    /// GET a JSON document.
    pub async fn get<T: DeserializeOwned>(&self, path: &str, query: &Query) -> Result<T, ApiError> {
        let resp = self
            .get_builder(path, query)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| ApiError::Network(network_message(&e)))?;
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| ApiError::Network(network_message(&e)))?;
        if !status.is_success() {
            return Err(error_from(status, &headers, &bytes));
        }
        serde_json::from_slice(&bytes).map_err(|e| ApiError::Decode(format!("{path}: {e}")))
    }

    pub async fn startup_status(&self) -> Result<StartupStatus, ApiError> {
        let s: StartupStatus = match self.get("/startup/status", &vec![]).await {
            Err(ApiError::Decode(_)) => {
                return Err(ApiError::Decode(format!(
                    "{} did not answer like a CPA Usage Keeper API (non-JSON response) — check the base URL and sub-path",
                    self.inner.base
                )));
            }
            other => other?,
        };
        if s.phase.is_empty() {
            return Err(ApiError::Decode(
                "startup status has no phase — is this a CPA Usage Keeper URL?".into(),
            ));
        }
        Ok(s)
    }

    pub async fn session(&self) -> Result<SessionResponse, ApiError> {
        self.get("/auth/session", &vec![]).await
    }

    /// `POST /auth/login` or `/auth/api-key-login`. On success the session token
    /// is stored in the client and returned so it can be persisted.
    pub async fn login(&self, mode: LoginMode, secret: &Secret) -> Result<String, ApiError> {
        let (path, body) = match mode {
            LoginMode::Admin => (
                "/auth/login",
                serde_json::json!({ "password": secret.expose() }),
            ),
            LoginMode::ApiKey => (
                "/auth/api-key-login",
                serde_json::json!({ "apiKey": secret.expose() }),
            ),
        };
        let resp = self
            .inner
            .http
            .post(self.url(path))
            .header(INTENT_HEADER, "fetch")
            .json(&body)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|e| ApiError::Network(network_message(&e)))?;
        drop(body);
        let status = resp.status();
        let headers = resp.headers().clone();
        if status.is_success() {
            let token = headers
                .get_all(header::SET_COOKIE)
                .iter()
                .filter_map(|v| v.to_str().ok())
                .find_map(parse_session_cookie);
            return match token {
                Some(t) => {
                    self.set_token(Some(t.clone()));
                    Ok(t)
                }
                None => Err(ApiError::Decode(
                    "login succeeded but no session cookie was set".into(),
                )),
            };
        }
        let bytes = resp.bytes().await.unwrap_or_default();
        Err(error_from(status, &headers, &bytes))
    }

    /// `POST /auth/logout`; always clears the local token.
    pub async fn logout(&self) -> Result<(), ApiError> {
        let result = self
            .request(reqwest::Method::POST, &self.url("/auth/logout"))
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| ApiError::Network(network_message(&e)));
        self.set_token(None);
        result.map(|_| ())
    }
}

/// Which login endpoint to use.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
#[value(rename_all = "kebab-case")]
pub enum LoginMode {
    Admin,
    ApiKey,
}

impl LoginMode {
    pub fn label(self) -> &'static str {
        match self {
            LoginMode::Admin => "Admin",
            LoginMode::ApiKey => "API key",
        }
    }
}

/// Extract the session token from a `Set-Cookie` header value.
pub fn parse_session_cookie(v: &str) -> Option<String> {
    let first = v.split(';').next()?.trim();
    let (name, value) = first.split_once('=')?;
    if name.trim() != COOKIE_NAME {
        return None;
    }
    let value = value.trim().trim_matches('"');
    (!value.is_empty()).then(|| value.to_string())
}

/// Map a non-2xx response to an [`ApiError`].
pub fn error_from(status: StatusCode, headers: &header::HeaderMap, body: &[u8]) -> ApiError {
    let parsed: ErrorBody = serde_json::from_slice(body).unwrap_or_default();
    let text = || {
        let s = String::from_utf8_lossy(body);
        let s = s.trim();
        if s.is_empty() || s.starts_with('<') {
            status.canonical_reason().unwrap_or("error").to_string()
        } else {
            crate::fmt::trunc(s, 120)
        }
    };
    let message = parsed
        .error
        .clone()
        .or_else(|| parsed.message.clone())
        .or_else(|| parsed.code.clone())
        .unwrap_or_else(text);
    let code = parsed.code.clone();
    match status.as_u16() {
        401 => ApiError::Unauthorized(message),
        403 => ApiError::Forbidden(message),
        429 => ApiError::RateLimited {
            retry_after: headers
                .get(header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse().ok()),
            message,
        },
        503 if code.as_deref() == Some("migration_in_progress") => ApiError::Migrating,
        503 if code.as_deref() == Some("costs_busy") => ApiError::CostsBusy,
        s => ApiError::Http {
            status: s,
            code,
            message,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_urls() {
        assert_eq!(
            normalize_base_url("http://localhost:8318/").unwrap(),
            "http://localhost:8318"
        );
        assert_eq!(
            normalize_base_url("https://host/keeper/api/v1").unwrap(),
            "https://host/keeper"
        );
        assert!(normalize_base_url("ftp://x").is_err());
        assert!(normalize_base_url("http://u:p@host").is_err());
        assert_eq!(
            display_host("https://host.example/keeper"),
            "host.example/keeper"
        );
        assert_eq!(display_host("http://localhost:8318"), "localhost:8318");
    }

    #[test]
    fn urls_include_sub_path() {
        let c = Client::new("https://host/keeper").unwrap();
        assert_eq!(
            c.url("/usage/overview"),
            "https://host/keeper/api/v1/usage/overview"
        );
    }

    #[test]
    fn cookie_parsing() {
        assert_eq!(
            parse_session_cookie("cpa_usage_keeper_session=abc123; Path=/; HttpOnly"),
            Some("abc123".into())
        );
        assert_eq!(
            parse_session_cookie("cpa_usage_keeper_session=; Max-Age=0"),
            None
        );
        assert_eq!(parse_session_cookie("other=1"), None);
    }

    #[test]
    fn error_mapping() {
        let h = header::HeaderMap::new();
        assert_eq!(
            error_from(
                StatusCode::UNAUTHORIZED,
                &h,
                br#"{"error":"authentication required"}"#
            ),
            ApiError::Unauthorized("authentication required".into())
        );
        assert_eq!(
            error_from(
                StatusCode::SERVICE_UNAVAILABLE,
                &h,
                br#"{"code":"costs_busy","message":"x"}"#
            ),
            ApiError::CostsBusy
        );
        assert_eq!(
            error_from(
                StatusCode::SERVICE_UNAVAILABLE,
                &h,
                br#"{"code":"migration_in_progress","message":"x"}"#
            ),
            ApiError::Migrating
        );
        let mut h2 = header::HeaderMap::new();
        h2.insert(header::RETRY_AFTER, "5".parse().unwrap());
        assert_eq!(
            error_from(
                StatusCode::TOO_MANY_REQUESTS,
                &h2,
                br#"{"code":"live_capacity"}"#
            ),
            ApiError::RateLimited {
                retry_after: Some(5),
                message: "live_capacity".into()
            }
        );
        assert!(matches!(
            error_from(StatusCode::NOT_FOUND, &h, b"404 page not found"),
            ApiError::Http { status: 404, .. }
        ));
    }
}
