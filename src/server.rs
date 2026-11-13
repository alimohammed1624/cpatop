//! Changing the server at runtime: validating a typed base URL, probing it
//! for a Keeper API and saving it to the config file.

use crate::api::client::normalize_base_url;
use crate::api::{ApiError, Client, LoginMode};
use crate::secrets::{self, Credential, Credentials, Source};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long the probe of a new server may take in total.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Where the base URL of this run came from. A flag or environment variable
/// overrides the config file, so saving there only takes effect once it is
/// gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BaseUrlSource {
    #[default]
    Default,
    Config,
    Flag,
    Env,
}

impl BaseUrlSource {
    /// `cli` is the value clap produced for `--base-url` (which also reads
    /// `CPATOP_BASE_URL`).
    pub fn detect(cli: Option<&str>, in_config: bool) -> BaseUrlSource {
        match cli {
            Some(v) if std::env::var("CPATOP_BASE_URL").is_ok_and(|e| e == v) => BaseUrlSource::Env,
            Some(_) => BaseUrlSource::Flag,
            None if in_config => BaseUrlSource::Config,
            None => BaseUrlSource::Default,
        }
    }

    /// What overrides the config file next time, if anything.
    pub fn overrides(self) -> Option<&'static str> {
        match self {
            BaseUrlSource::Flag => Some("--base-url"),
            BaseUrlSource::Env => Some("CPATOP_BASE_URL"),
            _ => None,
        }
    }
}

/// Validate and normalise a URL typed by the user: `http://` is added when
/// there is no scheme, a trailing `/` and `/api/v1` are dropped, a sub-path is
/// kept. The rules are those of `--base-url`; messages never repeat the input.
pub fn normalize_input(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("enter the server URL, e.g. http://localhost:8318".into());
    }
    if s.chars().any(char::is_whitespace) {
        return Err("the URL must not contain spaces".into());
    }
    let s = if s.contains("://") {
        s.to_string()
    } else {
        format!("http://{s}")
    };
    let u = url::Url::parse(&s).map_err(|e| format!("not a valid URL ({e})"))?;
    if !matches!(u.scheme(), "http" | "https") {
        return Err("only http:// and https:// URLs are supported".into());
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err("the URL must not contain a user name or password".into());
    }
    if u.query().is_some() || u.fragment().is_some() {
        return Err("the URL must not contain ? or #".into());
    }
    if u.host_str().is_none_or(str::is_empty) {
        return Err("the URL has no host".into());
    }
    normalize_base_url(&s).map_err(|_| "not a valid URL".to_string())
}

/// Why a probed URL was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeError {
    Invalid(String),
    Unreachable(String),
    NotKeeper(String),
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProbeError::Invalid(m) => write!(f, "invalid URL: {m}"),
            ProbeError::Unreachable(m) => write!(f, "unreachable: {m}"),
            ProbeError::NotKeeper(m) => write!(f, "not a CPA Usage Keeper server: {m}"),
        }
    }
}

fn not_keeper(e: ApiError) -> ProbeError {
    match e {
        ApiError::Network(m) => ProbeError::Unreachable(m),
        ApiError::Http { status: 404, .. } => {
            ProbeError::NotKeeper("404 not found — check the sub-path".into())
        }
        ApiError::Http { status, .. } if (300..400).contains(&status) => ProbeError::NotKeeper(
            format!("{status} redirect — try https:// or the exact sub-path"),
        ),
        ApiError::Decode(_) => {
            ProbeError::NotKeeper("no Keeper API answers here — check the sub-path".into())
        }
        other => ProbeError::NotKeeper(crate::fmt::trunc(&other.to_string(), 60)),
    }
}

/// Check that `url` (already normalised) answers like a Keeper API: the
/// public `/startup/status` must name a phase and `/auth/session` must be
/// JSON. Gives up after `timeout`.
pub async fn probe(url: &str, timeout: Duration) -> Result<(), ProbeError> {
    let c = Client::new(url).map_err(ProbeError::Invalid)?;
    let check = async {
        c.startup_status().await.map_err(not_keeper)?;
        match c.session().await {
            // Mid-migration a Keeper may refuse everything but the gate.
            Ok(_) | Err(ApiError::Migrating) => Ok(()),
            Err(e) => Err(not_keeper(e)),
        }
    };
    match tokio::time::timeout(timeout, check).await {
        Ok(r) => r,
        Err(_) => Err(ProbeError::Unreachable("timed out".into())),
    }
}

/// Set `base_url` in the config file at `path`, keeping every other key and
/// comment. A missing file is created (mode 0600, directory 0700); an
/// existing one keeps its mode, and a symlink is followed.
pub fn save_base_url(path: &Path, url: &str) -> Result<(), String> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let (text, mode) = match std::fs::read_to_string(&target) {
        Ok(s) => (s, file_mode(&target)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (
            "# cpatop configuration — see config.example.toml for every key.\n".to_string(),
            None,
        ),
        Err(e) => return Err(format!("cannot read the config file: {e}")),
    };
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|_| "the config file is not valid TOML; left unchanged".to_string())?;
    match doc.get_mut("base_url").and_then(|i| i.as_value_mut()) {
        Some(v) => {
            // Keep a trailing comment on the line.
            let decor = v.decor().clone();
            *v = url.into();
            *v.decor_mut() = decor;
        }
        None => {
            doc.remove("base_url");
            // Comments after the last key are the document's trailer; keep
            // them above the new key, where they were in the text (unless
            // tables follow the top-level keys).
            let flat = doc.iter().all(|(_, i)| i.is_value());
            let trailer = doc.trailing().as_str().unwrap_or_default().to_string();
            doc.insert("base_url", toml_edit::value(url));
            if flat && !trailer.is_empty() {
                doc.set_trailing("");
                if let Some(mut k) = doc.key_mut("base_url") {
                    k.leaf_decor_mut().set_prefix(trailer);
                }
            }
        }
    }
    crate::session_store::write_private(&target, doc.to_string().as_bytes())?;
    #[cfg(unix)]
    if let Some(m) = mode {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(m));
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(())
}

#[cfg(unix)]
fn file_mode(p: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .ok()
        .map(|m| m.permissions().mode() & 0o7777)
}

#[cfg(not(unix))]
fn file_mode(_: &Path) -> Option<u32> {
    None
}

/// The config file path for messages: `~` for the home directory. In
/// privacy mode no directory names are shown (`~/…/config.toml`).
pub fn display_path(path: &Path, privacy: bool, home: Option<&Path>) -> String {
    let under_home = home.and_then(|h| path.strip_prefix(h).ok());
    if privacy {
        let name = path.file_name().and_then(|n| n.to_str());
        return match (name, under_home.is_some()) {
            (Some("config.toml"), true) => "~/…/config.toml".into(),
            (Some("config.toml"), false) => "…/config.toml".into(),
            _ => "the config file".into(),
        };
    }
    match under_home {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// The user's home directory, for [`display_path`].
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// The credentials for another server: file and environment credentials
/// apply to any server; keyring entries are per server, so they are looked
/// up again for `base_url`.
pub fn credentials_for(base: &Credentials, base_url: &str, use_keyring: bool) -> Credentials {
    let explicit = |c: &Option<Credential>| {
        c.clone()
            .filter(|c| matches!(c.source, Source::File(_) | Source::Env(_)))
    };
    let mut out = Credentials {
        admin: explicit(&base.admin),
        api_key: explicit(&base.api_key),
        prefer: base.prefer,
        warnings: Vec::new(),
    };
    if use_keyring {
        for mode in [LoginMode::Admin, LoginMode::ApiKey] {
            let slot = match mode {
                LoginMode::Admin => &mut out.admin,
                LoginMode::ApiKey => &mut out.api_key,
            };
            if slot.is_none()
                && let Ok(Some(secret)) = secrets::keyring_get(mode, base_url)
            {
                *slot = Some(Credential {
                    mode,
                    secret,
                    source: Source::Keyring,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalisation_table() {
        let ok = [
            ("localhost:8318", "http://localhost:8318"),
            ("  http://localhost:8318/  ", "http://localhost:8318"),
            ("https://keeper.example/sub/", "https://keeper.example/sub"),
            ("keeper.example/sub/api/v1", "http://keeper.example/sub"),
            ("127.0.0.1:8318", "http://127.0.0.1:8318"),
            ("http://[::1]:8318", "http://[::1]:8318"),
            ("HTTPS://host", "HTTPS://host"),
        ];
        for (input, want) in ok {
            assert_eq!(normalize_input(input).as_deref(), Ok(want), "{input}");
        }
        let bad = [
            ("", "enter the server URL"),
            ("   ", "enter the server URL"),
            ("ftp://host", "only http"),
            ("file:///etc/passwd", "only http"),
            ("http://user:pw@host", "user name or password"),
            ("user@host", "user name or password"),
            ("http://host?x=1", "? or #"),
            ("http://host/#a", "? or #"),
            ("http://host:99999", "not a valid URL"),
            ("http://ho st", "spaces"),
            ("http://", "not a valid URL"),
        ];
        for (input, want) in bad {
            let e = normalize_input(input).expect_err(input);
            assert!(e.contains(want), "{input}: {e}");
        }
        // Messages never repeat what was typed.
        let e = normalize_input("ftp://10.1.2.3").unwrap_err();
        assert!(!e.contains("10.1.2.3"), "{e}");
    }

    #[test]
    fn source_and_override() {
        assert_eq!(BaseUrlSource::detect(None, true), BaseUrlSource::Config);
        assert_eq!(BaseUrlSource::detect(None, false), BaseUrlSource::Default);
        assert_eq!(BaseUrlSource::Config.overrides(), None);
        assert_eq!(BaseUrlSource::Flag.overrides(), Some("--base-url"));
        assert_eq!(BaseUrlSource::Env.overrides(), Some("CPATOP_BASE_URL"));
    }

    #[test]
    fn paths_for_messages() {
        let home = Path::new("/home/someone");
        let p = Path::new("/home/someone/.config/cpatop/config.toml");
        assert_eq!(
            display_path(p, false, Some(home)),
            "~/.config/cpatop/config.toml"
        );
        assert_eq!(display_path(p, true, Some(home)), "~/…/config.toml");
        let other = Path::new("/srv/cfg/config.toml");
        assert_eq!(
            display_path(other, false, Some(home)),
            "/srv/cfg/config.toml"
        );
        assert_eq!(display_path(other, true, Some(home)), "…/config.toml");
        let custom = Path::new("/home/someone/work-acme.toml");
        assert_eq!(display_path(custom, true, Some(home)), "the config file");
    }

    #[cfg(unix)]
    fn mode_of(p: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("cpatop-server-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn save_creates_a_private_file() {
        let d = tmp();
        let p = d.join("sub").join("config.toml");
        save_base_url(&p, "http://localhost:8318").unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.starts_with("# cpatop configuration"), "{s}");
        let c: crate::config::FileConfig = toml::from_str(&s).unwrap();
        assert_eq!(c.base_url.as_deref(), Some("http://localhost:8318"));
        #[cfg(unix)]
        assert_eq!(mode_of(&p), 0o600);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn save_keeps_other_keys_comments_and_mode() {
        let d = tmp();
        let p = d.join("config.toml");
        let before = "# my settings\ntheme = \"nord\" # favourite\n\n# where Keeper runs\nbase_url = \"http://old:1\" # old box\nrefresh_secs = 5\n";
        std::fs::write(&p, before).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o640)).unwrap();
        }
        save_base_url(&p, "https://new.example/keeper").unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert_eq!(
            s,
            "# my settings\ntheme = \"nord\" # favourite\n\n# where Keeper runs\nbase_url = \"https://new.example/keeper\" # old box\nrefresh_secs = 5\n"
        );
        #[cfg(unix)]
        assert_eq!(mode_of(&p), 0o640);

        // A file without the key gets it added; the rest is untouched.
        std::fs::write(&p, "# hello\nmouse = false\n").unwrap();
        save_base_url(&p, "http://localhost:8318").unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.starts_with("# hello\nmouse = false\n"), "{s}");
        let c: crate::config::FileConfig = toml::from_str(&s).unwrap();
        assert_eq!(c.base_url.as_deref(), Some("http://localhost:8318"));
        assert_eq!(c.mouse, Some(false));

        // Invalid TOML is left alone.
        std::fs::write(&p, "base_url = \n").unwrap();
        assert!(save_base_url(&p, "http://x").is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "base_url = \n");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn save_follows_a_symlink() {
        let d = tmp();
        let real = d.join("dotfiles.toml");
        std::fs::write(&real, "theme = \"nord\"\n").unwrap();
        let link = d.join("config.toml");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        save_base_url(&link, "http://localhost:8318").unwrap();
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let s = std::fs::read_to_string(&real).unwrap();
        assert!(s.contains("base_url = \"http://localhost:8318\""), "{s}");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn keyring_credentials_are_not_carried_over() {
        let base = Credentials {
            admin: Some(Credential {
                mode: LoginMode::Admin,
                secret: crate::secrets::Secret::new("x"),
                source: Source::Keyring,
            }),
            api_key: Some(Credential {
                mode: LoginMode::ApiKey,
                secret: crate::secrets::Secret::new("y"),
                source: Source::Env("CPATOP_API_KEY"),
            }),
            prefer: Some(LoginMode::ApiKey),
            warnings: vec!["w".into()],
        };
        let c = credentials_for(&base, "http://other:1", false);
        assert!(c.admin.is_none());
        assert_eq!(c.api_key.unwrap().source, Source::Env("CPATOP_API_KEY"));
        assert_eq!(c.prefer, Some(LoginMode::ApiKey));
    }
}
