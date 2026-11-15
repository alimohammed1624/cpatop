//! "A newer cpatop release is available": a check against cpatop's own
//! GitHub releases (never Keeper's `/update/check`), and the self-update
//! the dashboard offers.
//!
//! The dashboard checks in the background on every start (a cached newer
//! tag is reported at once) and caches the answer in the state directory;
//! `cpatop check-update` checks now. Only a `cpatop/<version>` User-Agent is
//! sent. Updating downloads this platform's release archive, checks it
//! against the release's `SHA256SUMS` and swaps the binary in place.

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
/// Release downloads: `<base>/<tag>/<asset>`.
pub const DOWNLOAD_BASE: &str = "https://github.com/alimohammed1624/cpatop/releases/download";
/// Test-only override of [`DOWNLOAD_BASE`] (undocumented).
pub const DOWNLOAD_ENV: &str = "CPATOP_UPDATE_DOWNLOAD_URL";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// How long a cached answer counts as fresh (for `is_fresh`; the
/// dashboard asks again on every start regardless).
pub const MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const TIMEOUT: Duration = Duration::from_secs(3);
/// A whole download (archive or checksums).
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);

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
#[serde(default)]
pub struct Cache {
    /// Unix seconds of the last check.
    pub checked_at: u64,
    /// Latest stable release tag seen, if any.
    pub latest: Option<String>,
    /// A release the user chose not to be asked about again.
    pub skipped: Option<String>,
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
/// check keeps the previously seen tag.
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
            skipped: prev.skipped,
        }
        .save(p);
    }
    r
}

/// The dashboard's check on every start: a cached newer tag is reported at
/// once, then GitHub is asked. `newer` gets each newer tag found and
/// whether to offer it (not when the user skipped that release). Errors
/// are swallowed.
pub async fn background(cache_path: Option<PathBuf>, newer: impl Fn(String, bool)) {
    let cached = cache_path.as_deref().and_then(Cache::load);
    let skipped = cached.as_ref().and_then(|c| c.skipped.clone());
    let offer = |tag: &str| skipped.as_deref() != Some(tag);
    let mut told = None;
    if let Some(tag) = cached.and_then(|c| c.newer_than(CURRENT)) {
        newer(tag.clone(), offer(&tag));
        told = Some(tag);
    }
    if let Ok(tag) = check_and_cache(cache_path.as_deref()).await
        && is_newer(&tag, CURRENT)
        && told.as_deref() != Some(tag.as_str())
    {
        let o = offer(&tag);
        newer(tag, o);
    }
}

/// Record that the user does not want to be asked about `tag` again.
pub fn skip(cache_path: &Path, tag: &str) -> Result<(), String> {
    let mut c = Cache::load(cache_path).unwrap_or_default();
    c.skipped = Some(tag.to_string());
    c.save(cache_path)
}

/// The release asset target of this build, `None` where no prebuilt
/// binary exists (the update then has to be done by hand).
pub fn asset_target() -> Option<&'static str> {
    target_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn target_for(os: &str, arch: &str) -> Option<&'static str> {
    Some(match (os, arch) {
        ("linux", "x86_64") => "x86_64-unknown-linux-musl",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        _ => return None,
    })
}

/// The SHA-256 (lowercase hex) `SHA256SUMS` lists for `name`.
pub fn sum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (hash, file) = l.trim().split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        (file == name && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

fn sha256_hex(data: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, data)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The `cpatop` binary inside a release archive (`cpatop-<target>/cpatop`).
pub fn extract_binary(archive: &[u8], target: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let want = format!("cpatop-{target}/cpatop");
    let gz = flate2::read::GzDecoder::new(archive);
    let mut tar = tar::Archive::new(gz);
    let entries = tar.entries().map_err(|e| format!("bad archive: {e}"))?;
    for entry in entries {
        let mut e = entry.map_err(|e| format!("bad archive: {e}"))?;
        let path = e.path().map_err(|e| format!("bad archive: {e}"))?;
        if path.to_string_lossy().trim_start_matches("./") == want {
            let mut out = Vec::new();
            e.read_to_end(&mut out)
                .map_err(|e| format!("bad archive: {e}"))?;
            return Ok(out);
        }
    }
    Err(format!("{want} is not in the archive"))
}

/// The running binary's real path (symlinks resolved): what an update
/// replaces and a restart runs.
pub fn current_exe() -> Result<PathBuf, String> {
    let p = std::env::current_exe().map_err(|e| format!("cannot find the cpatop binary: {e}"))?;
    std::fs::canonicalize(&p).map_err(|e| format!("cannot find the cpatop binary: {e}"))
}

/// Replace `exe` with `bin`: written to a temporary file beside it, made
/// executable, then renamed over it, so `exe` is never half-written.
pub fn install_binary(bin: &[u8], exe: &Path) -> Result<(), String> {
    let dir = exe
        .parent()
        .ok_or_else(|| "cannot find the cpatop binary's folder".to_string())?;
    let tmp = dir.join(format!(".cpatop-update-{}", fastrand::u64(..)));
    let write = || -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bin)?;
        f.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        }
        std::fs::rename(&tmp, exe)
    };
    // No path in the message: it may hold the user name.
    write().map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("cannot replace the cpatop binary: {e}")
    })
}

/// A self-update step, for the progress line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Downloading,
    Verifying,
    Installing,
}

impl Step {
    pub fn label(self) -> &'static str {
        match self {
            Step::Downloading => "downloading…",
            Step::Verifying => "verifying…",
            Step::Installing => "installing…",
        }
    }
}

async fn download(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let resp = client.get(url).send().await.map_err(describe)?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("download failed: HTTP {}", status.as_u16()));
    }
    resp.bytes().await.map(|b| b.to_vec()).map_err(describe)
}

/// Download release `tag` for this platform, verify it and replace the
/// running binary. `step` is told each step as it starts.
pub async fn self_update(tag: &str, step: impl Fn(Step)) -> Result<(), String> {
    let target = asset_target().ok_or_else(|| {
        format!(
            "no prebuilt cpatop for {}/{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;
    let exe = current_exe()?;
    let base = std::env::var(DOWNLOAD_ENV)
        .ok()
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| DOWNLOAD_BASE.to_string());
    let base = base.trim().trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .connect_timeout(Duration::from_secs(10))
        .user_agent(format!("cpatop/{CURRENT}"))
        .build()
        .map_err(|e| e.without_url().to_string())?;
    step(Step::Downloading);
    let asset = format!("cpatop-{target}.tar.gz");
    let sums = download(&client, &format!("{base}/{tag}/SHA256SUMS")).await?;
    let archive = download(&client, &format!("{base}/{tag}/{asset}")).await?;
    step(Step::Verifying);
    let want = sum_for(&String::from_utf8_lossy(&sums), &asset)
        .ok_or_else(|| format!("SHA256SUMS has no entry for {asset}"))?;
    if sha256_hex(&archive) != want {
        return Err(format!("{asset} does not match its SHA-256 in SHA256SUMS"));
    }
    let bin = extract_binary(&archive, target)?;
    if bin.is_empty() {
        return Err(format!("{asset} holds an empty cpatop binary"));
    }
    step(Step::Installing);
    tokio::task::spawn_blocking(move || install_binary(&bin, &exe))
        .await
        .map_err(|e| e.to_string())?
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
            skipped: None,
        };
        assert!(c(0).is_fresh(now));
        assert!(c(23 * 3600).is_fresh(now));
        assert!(!c(24 * 3600).is_fresh(now));
        assert!(!c(25 * 3600).is_fresh(now));
        // Clock went backwards: treat as stale and check again.
        let future = Cache {
            checked_at: now + 60,
            latest: None,
            skipped: None,
        };
        assert!(!future.is_fresh(now));
        assert_eq!(c(0).newer_than("0.1.0").as_deref(), Some("v0.2.0"));
        assert_eq!(c(0).newer_than("0.2.0"), None);
    }

    #[test]
    fn old_cache_files_still_load() {
        let dir = std::env::temp_dir().join(format!("cpatop-upd-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("update-check.json");
        std::fs::write(&p, r#"{"checked_at":1794748031,"latest":"v0.2.1"}"#).unwrap();
        let c = Cache::load(&p).unwrap();
        assert_eq!(c.latest.as_deref(), Some("v0.2.1"));
        assert_eq!(c.skipped, None);
        skip(&p, "v0.2.3").unwrap();
        let c = Cache::load(&p).unwrap();
        assert_eq!(c.skipped.as_deref(), Some("v0.2.3"));
        assert_eq!(c.latest.as_deref(), Some("v0.2.1"), "skip keeps the rest");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn release_targets() {
        assert_eq!(
            target_for("linux", "x86_64"),
            Some("x86_64-unknown-linux-musl")
        );
        assert_eq!(
            target_for("linux", "aarch64"),
            Some("aarch64-unknown-linux-musl")
        );
        assert_eq!(target_for("macos", "aarch64"), Some("aarch64-apple-darwin"));
        assert_eq!(target_for("macos", "x86_64"), Some("x86_64-apple-darwin"));
        assert_eq!(target_for("windows", "x86_64"), None);
        assert_eq!(target_for("linux", "riscv64"), None);
    }

    #[test]
    fn checksum_lines() {
        let h = "3264b4640f7bb0d03996b976acfbb33735db0e4af62fd76ddfa8fb4e96249d6a";
        let sums = format!(
            "{}  cpatop-aarch64-apple-darwin.tar.gz\n{}  cpatop-x86_64-unknown-linux-musl.tar.gz\n",
            "d".repeat(64),
            h.to_uppercase()
        );
        assert_eq!(
            sum_for(&sums, "cpatop-x86_64-unknown-linux-musl.tar.gz").as_deref(),
            Some(h)
        );
        // `sha256sum -b` marks binary mode with `*`.
        assert_eq!(
            sum_for(&format!("{h} *a.tar.gz"), "a.tar.gz").as_deref(),
            Some(h)
        );
        assert_eq!(sum_for(&sums, "cpatop-x86_64-apple-darwin.tar.gz"), None);
        assert_eq!(sum_for("nothex  a.tar.gz", "a.tar.gz"), None);
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn archive_binary_and_install() {
        let target = "x86_64-unknown-linux-musl";
        let mut b = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        for (path, body) in [
            (format!("cpatop-{target}/README.md"), &b"readme"[..]),
            (format!("cpatop-{target}/cpatop"), &b"new binary"[..]),
        ] {
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(0o755);
            h.set_cksum();
            b.append_data(&mut h, path, body).unwrap();
        }
        let gz = b.into_inner().unwrap().finish().unwrap();
        assert_eq!(extract_binary(&gz, target).unwrap(), b"new binary");
        assert!(extract_binary(&gz, "aarch64-apple-darwin").is_err());
        assert!(extract_binary(b"not gzip", target).is_err());

        let dir = std::env::temp_dir().join(format!("cpatop-upd-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("cpatop");
        std::fs::write(&exe, b"old").unwrap();
        install_binary(b"new binary", &exe).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new binary");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&exe).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
        }
        // Only the binary is left: no temporary file.
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        // An unwritable target leaves nothing behind either.
        let missing = dir.join("no-such-dir").join("cpatop");
        assert!(install_binary(b"x", &missing).is_err());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cache_round_trip_is_private() {
        let dir = std::env::temp_dir().join(format!("cpatop-upd-{}", fastrand::u64(..)));
        let p = dir.join("update-check.json");
        let c = Cache {
            checked_at: 42,
            latest: Some("v0.2.0".into()),
            skipped: None,
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
