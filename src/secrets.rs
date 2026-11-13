//! Credential sources: files, environment, OS keyring and the interactive prompt.
//!
//! Secrets are wrapped in [`Secret`], whose `Debug`/`Display` never reveal the
//! value. They are never accepted as plain command-line arguments.

use crate::api::LoginMode;
use std::path::{Path, PathBuf};

/// A secret string that does not print itself.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: impl Into<String>) -> Self {
        Secret(s.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl std::fmt::Display for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("***")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        // Best-effort scrub of the heap buffer before it is freed.
        // SAFETY: we overwrite the existing bytes with zeros (valid UTF-8).
        unsafe {
            for b in self.0.as_bytes_mut() {
                std::ptr::write_volatile(b, 0);
            }
        }
    }
}

/// Where a credential came from (shown in messages, never the value).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    File(PathBuf),
    Env(&'static str),
    Keyring,
    Prompt,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::File(p) => write!(f, "file {}", crate::app::shown_path(p)),
            Source::Env(v) => write!(f, "${v}"),
            Source::Keyring => f.write_str("OS keyring"),
            Source::Prompt => f.write_str("login screen"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Credential {
    pub mode: LoginMode,
    pub secret: Secret,
    pub source: Source,
}

/// Credentials discovered at startup.
#[derive(Debug, Clone, Default)]
pub struct Credentials {
    pub admin: Option<Credential>,
    pub api_key: Option<Credential>,
    pub prefer: Option<LoginMode>,
    /// Non-fatal problems (e.g. permissive file modes, keyring unavailable).
    pub warnings: Vec<String>,
}

impl Credentials {
    /// The credential to use for automatic login. Explicit credentials win
    /// over keyring ones.
    pub fn pick(&self) -> Option<&Credential> {
        if let Some(c) = self.pick_explicit() {
            return Some(c);
        }
        match self.prefer {
            Some(LoginMode::ApiKey) => self.api_key.as_ref().or(self.admin.as_ref()),
            _ => self.admin.as_ref().or(self.api_key.as_ref()),
        }
    }

    /// The credential to use when it was supplied explicitly for this run
    /// (a file flag or an environment variable), as opposed to one found in
    /// the OS keyring. An explicit credential decides the session role.
    pub fn pick_explicit(&self) -> Option<&Credential> {
        let explicit = |c: &&Credential| matches!(c.source, Source::File(_) | Source::Env(_));
        let admin = self.admin.as_ref().filter(explicit);
        let key = self.api_key.as_ref().filter(explicit);
        match self.prefer {
            Some(LoginMode::ApiKey) => key.or(admin),
            _ => admin.or(key),
        }
    }

    /// The mode the login screen should open on.
    pub fn preferred_mode(&self) -> LoginMode {
        self.prefer
            .or_else(|| self.pick().map(|c| c.mode))
            .unwrap_or(LoginMode::Admin)
    }
}

/// Inputs for [`resolve`].
#[derive(Debug, Clone, Default)]
pub struct ResolveOptions {
    pub base_url: String,
    pub password_file: Option<PathBuf>,
    pub api_key_file: Option<PathBuf>,
    pub prefer: Option<LoginMode>,
    pub use_keyring: bool,
}

/// Read a secret from a file, trimming surrounding whitespace / newline.
/// `what` names the file in messages (`password file`); its path is shown
/// as `(‹path›)` in privacy mode.
pub fn read_secret_file(path: &Path, what: &str) -> Result<(Secret, Option<String>), String> {
    let shown = crate::app::shown_path(path);
    let raw = std::fs::read_to_string(path).map_err(|e| {
        // The OS message, without anything that could repeat the path.
        let reason = match e.kind() {
            std::io::ErrorKind::NotFound => "no such file".to_string(),
            std::io::ErrorKind::PermissionDenied => "permission denied".to_string(),
            std::io::ErrorKind::IsADirectory => "is a directory".to_string(),
            std::io::ErrorKind::InvalidData => "not UTF-8 text".to_string(),
            _ => crate::app::scrub_output(&e.to_string()),
        };
        format!("cannot read {what} {shown}: {reason}")
    })?;
    let s = raw.trim();
    if s.is_empty() {
        return Err(format!("{what} {shown} is empty"));
    }
    if s.len() > 4000 {
        return Err(format!("{what} {shown} is too large"));
    }
    let mut warning = None;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path)
            && meta.permissions().mode() & 0o077 != 0
        {
            warning = Some(format!(
                "{what} {shown} is readable by other users; consider chmod 600"
            ));
        }
    }
    Ok((Secret::new(s), warning))
}

fn from_env(var: &'static str) -> Option<Secret> {
    std::env::var(var)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .map(Secret::new)
}

/// Resolve credentials in precedence order: file → environment → keyring.
pub fn resolve(opts: &ResolveOptions) -> Result<Credentials, String> {
    let mut out = Credentials {
        prefer: opts.prefer,
        ..Default::default()
    };
    for mode in [LoginMode::Admin, LoginMode::ApiKey] {
        let (file, env, what) = match mode {
            LoginMode::Admin => (
                opts.password_file.as_ref(),
                "CPATOP_PASSWORD",
                "password file",
            ),
            LoginMode::ApiKey => (opts.api_key_file.as_ref(), "CPATOP_API_KEY", "API key file"),
        };
        let mut found = None;
        if let Some(path) = file {
            let (secret, warning) = read_secret_file(path, what)?;
            out.warnings.extend(warning);
            found = Some(Credential {
                mode,
                secret,
                source: Source::File(path.clone()),
            });
        } else if let Some(secret) = from_env(env) {
            found = Some(Credential {
                mode,
                secret,
                source: Source::Env(env),
            });
        } else if opts.use_keyring {
            match keyring_get(mode, &opts.base_url) {
                Ok(Some(secret)) => {
                    found = Some(Credential {
                        mode,
                        secret,
                        source: Source::Keyring,
                    })
                }
                Ok(None) => {}
                Err(e) => out.warnings.push(format!("keyring unavailable: {e}")),
            }
        }
        match mode {
            LoginMode::Admin => out.admin = found,
            LoginMode::ApiKey => out.api_key = found,
        }
    }
    out.warnings.dedup();
    Ok(out)
}

/// Keyring user name for a mode and server.
pub fn keyring_user(mode: LoginMode, base_url: &str) -> String {
    let m = match mode {
        LoginMode::Admin => "admin",
        LoginMode::ApiKey => "api-key",
    };
    format!("{m}@{base_url}")
}

pub const KEYRING_SERVICE: &str = "cpatop";

#[cfg(feature = "keyring")]
mod os {
    use std::sync::OnceLock;

    fn init() -> Result<(), String> {
        static INIT: OnceLock<Result<(), String>> = OnceLock::new();
        INIT.get_or_init(|| {
            #[cfg(target_os = "macos")]
            {
                let store = apple_native_keyring_store::keychain::Store::new()
                    .map_err(|e| e.to_string())?;
                keyring_core::set_default_store(store);
                Ok(())
            }
            #[cfg(target_os = "linux")]
            {
                let store =
                    zbus_secret_service_keyring_store::Store::new().map_err(|e| e.to_string())?;
                keyring_core::set_default_store(store);
                Ok(())
            }
            #[cfg(not(any(target_os = "macos", target_os = "linux")))]
            {
                Err("no keyring backend for this platform".to_string())
            }
        })
        .clone()
    }

    pub fn get(service: &str, user: &str) -> Result<Option<String>, String> {
        init()?;
        let entry = keyring_core::Entry::new(service, user).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn set(service: &str, user: &str, secret: &str) -> Result<(), String> {
        init()?;
        let entry = keyring_core::Entry::new(service, user).map_err(|e| e.to_string())?;
        entry.set_password(secret).map_err(|e| e.to_string())
    }

    pub fn delete(service: &str, user: &str) -> Result<bool, String> {
        init()?;
        let entry = keyring_core::Entry::new(service, user).map_err(|e| e.to_string())?;
        match entry.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring_core::Error::NoEntry) => Ok(false),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Run a keyring operation on a plain thread with a timeout, so a missing or
/// hung Secret Service never blocks startup.
#[cfg(feature = "keyring")]
fn with_timeout<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| "keyring did not respond".to_string())?
}

/// Look up a stored secret. `Ok(None)` when absent.
pub fn keyring_get(mode: LoginMode, base_url: &str) -> Result<Option<Secret>, String> {
    #[cfg(feature = "keyring")]
    {
        let user = keyring_user(mode, base_url);
        with_timeout(move || os::get(KEYRING_SERVICE, &user)).map(|o| o.map(Secret::new))
    }
    #[cfg(not(feature = "keyring"))]
    {
        let _ = (mode, base_url);
        Ok(None)
    }
}

/// Store a secret in the OS keyring.
pub fn keyring_set(mode: LoginMode, base_url: &str, secret: &Secret) -> Result<(), String> {
    #[cfg(feature = "keyring")]
    {
        let user = keyring_user(mode, base_url);
        let value = secret.expose().to_string();
        with_timeout(move || os::set(KEYRING_SERVICE, &user, &value))
    }
    #[cfg(not(feature = "keyring"))]
    {
        let _ = (mode, base_url, secret);
        Err("cpatop was built without keyring support".into())
    }
}

/// Remove a secret from the OS keyring. Returns whether one existed.
pub fn keyring_delete(mode: LoginMode, base_url: &str) -> Result<bool, String> {
    #[cfg(feature = "keyring")]
    {
        let user = keyring_user(mode, base_url);
        with_timeout(move || os::delete(KEYRING_SERVICE, &user))
    }
    #[cfg(not(feature = "keyring"))]
    {
        let _ = (mode, base_url);
        Err("cpatop was built without keyring support".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_never_prints() {
        let s = Secret::new("hunter2");
        assert_eq!(format!("{s:?}"), "Secret(***)");
        assert_eq!(format!("{s}"), "***");
        assert_eq!(s.expose(), "hunter2");
    }

    #[test]
    fn file_is_trimmed() {
        let dir = std::env::temp_dir().join(format!("cpatop-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("pw");
        std::fs::write(&p, "  s3cret\n").unwrap();
        let (s, _) = read_secret_file(&p, "password file").unwrap();
        assert_eq!(s.expose(), "s3cret");
        std::fs::write(&p, "\n").unwrap();
        assert!(read_secret_file(&p, "password file").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn pick_prefers_admin_unless_asked() {
        let c = |mode| Credential {
            mode,
            secret: Secret::new("x"),
            source: Source::Prompt,
        };
        let mut creds = Credentials {
            admin: Some(c(LoginMode::Admin)),
            api_key: Some(c(LoginMode::ApiKey)),
            ..Default::default()
        };
        assert_eq!(creds.pick().unwrap().mode, LoginMode::Admin);
        creds.prefer = Some(LoginMode::ApiKey);
        assert_eq!(creds.pick().unwrap().mode, LoginMode::ApiKey);
    }
}
