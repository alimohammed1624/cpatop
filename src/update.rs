//! "A newer cpatop release is available": a check against cpatop's own
//! GitHub releases (never Keeper's `/update/check`).
//!
//! The dashboard checks in the background at most once per 24 hours and
//! caches the answer in the state directory; `cpatop check-update` checks
//! now. Only a `cpatop/<version>` User-Agent is sent.

use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Redirects to `…/releases/tag/<latest tag>`.
pub const LATEST_URL: &str = "https://github.com/alimohammed1624/cpatop/releases/latest";
/// Fallback: JSON with `tag_name`.
pub const API_URL: &str = "https://api.github.com/repos/alimohammed1624/cpatop/releases/latest";
pub const UPGRADE_CMD: &str =
    "curl -fsSL https://raw.githubusercontent.com/alimohammed1624/cpatop/main/install.sh | sh";
pub const CARGO_CMD: &str = "cargo install --git https://github.com/alimohammed1624/cpatop";
/// Test-only override of the release URL (undocumented).
pub const URL_ENV: &str = "CPATOP_UPDATE_URL";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// How long a cached answer is reused.
pub const MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const TIMEOUT: Duration = Duration::from_secs(3);

/// `X.Y.Z` with an optional `v` prefix; pre-releases (`-…`) and anything
/// else are `None`. Build metadata (`+…`) is ignored.
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim();
    let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
    let s = s.split('+').next()?;
    if s.contains('-') {
        return None;
    }
    let mut it = s.split('.').map(|p| p.parse::<u64>().ok());
    let v = (it.next()??, it.next()??, it.next()??);
    it.next().is_none().then_some(v)
}

/// `latest` is a stable release newer than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// `v1.2.3` → `1.2.3`.
pub fn display_version(tag: &str) -> &str {
    tag.trim().strip_prefix(['v', 'V']).unwrap_or(tag.trim())
}

/// The tag in a `Location` header such as
/// `https://github.com/o/r/releases/tag/v0.2.0`.
pub fn tag_from_location(loc: &str) -> Option<String> {
    let (_, rest) = loc.split_once("/releases/tag/")?;
    let tag = rest.split(['?', '#', '/']).next()?.trim();
    (!tag.is_empty()).then(|| tag.to_string())
}

/// Whether the background check runs: `--no-update-check`, config
/// `update_check = false`, `CPATOP_NO_UPDATE_CHECK` and `CI` each turn it
/// off; nothing turns it back on.
pub fn enabled(
    no_flag: bool,
    config: Option<bool>,
    no_env: Option<&OsStr>,
    ci_env: Option<&OsStr>,
) -> bool {
    let set = |v: Option<&OsStr>| {
        v.is_some_and(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"))
    };
    !no_flag && config != Some(false) && !set(no_env) && !set(ci_env)
}

/// `update-check.json` in the state directory.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cache {
    /// Unix seconds of the last check.
    pub checked_at: u64,
    /// Latest stable release tag seen, if any.
    pub latest: Option<String>,
}

impl Cache {
    pub fn load(path: &Path) -> Option<Cache> {
        let s = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&s).ok()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let s = serde_json::to_string(self).map_err(|e| e.to_string())?;
        crate::session_store::write_private(path, s.as_bytes())
    }

    /// Checked less than 24 h before `now` (a time in the future is stale).
    pub fn is_fresh(&self, now: u64) -> bool {
        now >= self.checked_at && now - self.checked_at < MAX_AGE.as_secs()
    }

    /// The cached tag, when it is newer than `current`.
    pub fn newer_than(&self, current: &str) -> Option<String> {
        self.latest.clone().filter(|l| is_newer(l, current))
    }
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Where to ask: the override (alone) or GitHub with its API as fallback.
pub fn sources() -> Vec<String> {
    match std::env::var(URL_ENV) {
        Ok(u) if !u.trim().is_empty() => vec![u.trim().to_string()],
        _ => vec![LATEST_URL.to_string(), API_URL.to_string()],
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(TIMEOUT)
        .connect_timeout(TIMEOUT)
        .user_agent(format!("cpatop/{CURRENT}"))
        .build()
        .map_err(|e| e.without_url().to_string())
}

fn describe(e: reqwest::Error) -> String {
    if e.is_timeout() {
        format!("no answer within {} s", TIMEOUT.as_secs())
    } else if e.is_connect() {
        "cannot connect to GitHub".into()
    } else {
        e.without_url().to_string()
    }
}

/// Ask one source: a redirect's `Location` tag, else JSON `tag_name`.
async fn ask(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let resp = client.get(url).send().await.map_err(describe)?;
    let status = resp.status();
    if status.is_redirection() {
        let loc = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        return tag_from_location(loc).ok_or_else(|| "no release found".to_string());
    }
    if !status.is_success() {
        return Err(format!("HTTP {}", status.as_u16()));
    }
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
    }
    resp.json::<Release>()
        .await
        .map(|r| r.tag_name)
        .map_err(|_| "unexpected response".to_string())
}

/// The latest release tag, trying each source in turn.
pub async fn fetch_latest() -> Result<String, String> {
    let client = client()?;
    let mut err = String::from("no source");
    for url in sources() {
        match ask(&client, &url).await {
            Ok(tag) => return Ok(tag),
            Err(e) => err = e,
        }
    }
    Err(err)
}

/// Check now and record the answer (and the time) in the cache. A failed
/// check keeps the previously seen tag but still counts as a check, so an
/// offline machine is not asked again for 24 h.
pub async fn check_and_cache(cache_path: Option<&Path>) -> Result<String, String> {
    let r = fetch_latest().await;
    if let Some(p) = cache_path {
        let prev = Cache::load(p).unwrap_or_default();
        let latest = match &r {
            // A pre-release tag is never recorded as "latest".
            Ok(tag) if parse_version(tag).is_some() => Some(tag.clone()),
            _ => prev.latest,
        };
        let _ = Cache {
            checked_at: now_secs(),
            latest,
        }
        .save(p);
    }
    r
}

/// The dashboard's check: a cached newer tag is reported at once; a fresh
/// cache ends there, otherwise GitHub is asked. `newer` gets each newer tag
/// found. Errors are swallowed.
pub async fn background(cache_path: Option<PathBuf>, newer: impl Fn(String)) {
    if let Some(c) = cache_path.as_deref().and_then(Cache::load) {
        if let Some(tag) = c.newer_than(CURRENT) {
            newer(tag);
        }
        if c.is_fresh(now_secs()) {
            return;
        }
    }
    if let Ok(tag) = check_and_cache(cache_path.as_deref()).await
        && is_newer(&tag, CURRENT)
    {
        newer(tag);
    }
}

/// The one line `cpatop check-update` prints.
pub fn report(result: &Result<String, String>, current: &str) -> String {
    match result {
        Ok(tag) if is_newer(tag, current) => format!(
            "cpatop {} is available (you have {current}) — upgrade: {UPGRADE_CMD}",
            display_version(tag)
        ),
        Ok(_) => format!("cpatop {current} is up to date"),
        Err(e) => format!("could not check for updates: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_comparison() {
        assert_eq!(parse_version("v0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_version("0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_version("1.2.3+build.5"), Some((1, 2, 3)));
        assert_eq!(parse_version("v1.2"), None);
        assert_eq!(parse_version("v1.2.3.4"), None);
        assert_eq!(parse_version("latest"), None);
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.1.1", "0.1.0"));
        assert!(is_newer("v0.10.0", "0.9.9"), "numeric, not lexical");
        assert!(is_newer("v1.0.0", "0.99.99"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
        // Pre-releases are never offered.
        assert!(parse_version("v0.2.0-rc.1").is_none());
        assert!(!is_newer("v0.2.0-rc.1", "0.1.0"));
        assert!(!is_newer("v9.0.0-beta", "0.1.0"));
        assert!(!is_newer("garbage", "0.1.0"));
        assert_eq!(display_version("v0.2.0"), "0.2.0");
    }

    #[test]
    fn location_parsing() {
        let t = |s| tag_from_location(s);
        assert_eq!(
            t("https://github.com/alimohammed1624/cpatop/releases/tag/v0.2.0").as_deref(),
            Some("v0.2.0")
        );
        assert_eq!(
            t("/alimohammed1624/cpatop/releases/tag/v1.0.0?x=1").as_deref(),
            Some("v1.0.0")
        );
        assert_eq!(
            t("https://github.com/o/r/releases/tag/v1.0.0/").as_deref(),
            Some("v1.0.0")
        );
        // No releases yet: GitHub redirects to the release list.
        assert_eq!(
            t("https://github.com/alimohammed1624/cpatop/releases"),
            None
        );
        assert_eq!(t("https://github.com/o/r/releases/tag/"), None);
        assert_eq!(t(""), None);
    }

    #[test]
    fn cache_expiry() {
        let now = 1_800_000_000;
        let c = |ago: u64| Cache {
            checked_at: now - ago,
            latest: Some("v0.2.0".into()),
        };
        assert!(c(0).is_fresh(now));
        assert!(c(23 * 3600).is_fresh(now));
        assert!(!c(24 * 3600).is_fresh(now));
        assert!(!c(25 * 3600).is_fresh(now));
        // Clock went backwards: treat as stale and check again.
        let future = Cache {
            checked_at: now + 60,
            latest: None,
        };
        assert!(!future.is_fresh(now));
        assert_eq!(c(0).newer_than("0.1.0").as_deref(), Some("v0.2.0"));
        assert_eq!(c(0).newer_than("0.2.0"), None);
    }

    #[test]
    fn cache_round_trip_is_private() {
        let dir = std::env::temp_dir().join(format!("cpatop-upd-{}", fastrand::u64(..)));
        let p = dir.join("update-check.json");
        let c = Cache {
            checked_at: 42,
            latest: Some("v0.2.0".into()),
        };
        c.save(&p).unwrap();
        assert_eq!(Cache::load(&p), Some(c));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn opt_out_precedence() {
        let os = |s: &'static str| Some(OsStr::new(s));
        assert!(enabled(false, None, None, None));
        assert!(enabled(false, Some(true), None, None));
        assert!(!enabled(true, None, None, None));
        assert!(!enabled(true, Some(true), None, None), "flag beats config");
        assert!(!enabled(false, Some(false), None, None));
        assert!(
            !enabled(false, Some(true), os("1"), None),
            "env beats config"
        );
        assert!(
            !enabled(false, Some(true), None, os("true")),
            "CI beats config"
        );
        assert!(!enabled(false, None, None, os("1")));
        // Empty / "0" / "false" do not opt out.
        assert!(enabled(false, None, os(""), os("")));
        assert!(enabled(false, None, os("0"), os("false")));
    }

    #[test]
    fn report_lines() {
        assert_eq!(
            report(&Ok("v0.1.0".into()), "0.1.0"),
            "cpatop 0.1.0 is up to date"
        );
        assert_eq!(
            report(&Ok("v0.2.0".into()), "0.1.0"),
            format!("cpatop 0.2.0 is available (you have 0.1.0) — upgrade: {UPGRADE_CMD}")
        );
        assert_eq!(
            report(&Ok("v0.2.0-rc.1".into()), "0.1.0"),
            "cpatop 0.1.0 is up to date"
        );
        assert_eq!(
            report(&Err("timed out".into()), "0.1.0"),
            "could not check for updates: timed out"
        );
    }
}
