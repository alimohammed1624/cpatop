//! Persisted session cookies (never passwords), one per base URL and role.
//!
//! Stored as JSON in the XDG state directory with mode 0600. Entries are keyed
//! by `"<base URL> <role>"` so an admin session and an API-key session for the
//! same server, or sessions for two servers, never shadow each other.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoreFile {
    #[serde(default)]
    sessions: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    token: String,
    /// Server role string (`admin`, `api_key_viewer`).
    #[serde(default)]
    role: String,
    #[serde(default)]
    saved_at: String,
}

/// A stored session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub token: String,
    /// Server role string (`admin`, `api_key_viewer`).
    pub role: String,
}

#[derive(Debug, Clone)]
pub struct SessionStore {
    path: Option<PathBuf>,
}

fn key(base_url: &str, role: &str) -> String {
    format!("{base_url} {role}")
}

/// Split a store key into base URL and role. Entries written by cpatop 0.1
/// were keyed by the base URL alone; their role comes from the entry.
fn split_key<'a>(k: &'a str, e: &'a Entry) -> (&'a str, &'a str) {
    match k.rsplit_once(' ') {
        Some((base, role)) => (base, role),
        None => (k, e.role.as_str()),
    }
}

impl SessionStore {
    pub fn new(path: Option<PathBuf>) -> Self {
        SessionStore { path }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn read(&self) -> StoreFile {
        self.path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn write(&self, f: &StoreFile) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let json = serde_json::to_string_pretty(f).map_err(|e| e.to_string())?;
        write_private(path, json.as_bytes())
    }

    /// Every stored session for `base_url`, most recently saved first.
    pub fn load_all(&self, base_url: &str) -> Vec<Stored> {
        let f = self.read();
        let mut v: Vec<(String, Stored)> = f
            .sessions
            .iter()
            .filter_map(|(k, e)| {
                let (base, role) = split_key(k, e);
                (base == base_url && !e.token.is_empty()).then(|| {
                    (
                        e.saved_at.clone(),
                        Stored {
                            token: e.token.clone(),
                            role: role.to_string(),
                        },
                    )
                })
            })
            .collect();
        v.sort_by(|a, b| b.0.cmp(&a.0));
        v.into_iter().map(|(_, s)| s).collect()
    }

    /// The stored session for `base_url` with server role `role`.
    pub fn load(&self, base_url: &str, role: &str) -> Option<String> {
        self.load_all(base_url)
            .into_iter()
            .find(|s| s.role == role)
            .map(|s| s.token)
    }

    pub fn save(&self, base_url: &str, token: &str, role: &str) -> Result<(), String> {
        let mut f = self.read();
        // Drop a legacy un-keyed entry for this server; it is superseded.
        f.sessions.remove(base_url);
        f.sessions.insert(
            key(base_url, role),
            Entry {
                token: token.to_string(),
                role: role.to_string(),
                saved_at: chrono::Utc::now().to_rfc3339(),
            },
        );
        self.write(&f)
    }

    /// Forget the session for one role (`Some`) or every role (`None`).
    pub fn remove(&self, base_url: &str, role: Option<&str>) -> Result<(), String> {
        let mut f = self.read();
        let before = f.sessions.len();
        f.sessions.retain(|k, e| {
            let (base, r) = split_key(k, e);
            !(base == base_url && role.is_none_or(|want| want == r))
        });
        if f.sessions.len() != before {
            self.write(&f)?;
        }
        Ok(())
    }

    /// Forget the session whose token is `token` (used after a 401).
    pub fn remove_token(&self, base_url: &str, token: &str) -> Result<(), String> {
        let mut f = self.read();
        let before = f.sessions.len();
        f.sessions.retain(|k, e| {
            let (base, _) = split_key(k, e);
            !(base == base_url && e.token == token)
        });
        if f.sessions.len() != before {
            self.write(&f)?;
        }
        Ok(())
    }
}

/// Write a file readable only by the current user (atomic rename).
pub fn write_private(path: &Path, data: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("invalid path")?;
    create_private_dir(dir)?;
    let tmp = path.with_extension("tmp");
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts
            .open(&tmp)
            .map_err(|e| format!("cannot write {}: {e}", crate::app::shown_path(&tmp)))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        f.write_all(data).map_err(|e| e.to_string())?;
        f.sync_all().ok();
    }
    std::fs::rename(&tmp, path)
        .map_err(|e| format!("cannot write {}: {e}", crate::app::shown_path(path)))
}

/// Create a directory (and parents) with mode 0700 on Unix.
pub fn create_private_dir(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        return Ok(());
    }
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o700);
    }
    b.create(dir)
        .map_err(|e| format!("cannot create {}: {e}", crate::app::shown_path(dir)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_private_mode() {
        let dir = std::env::temp_dir().join(format!("cpatop-store-{}", std::process::id()));
        let path = dir.join("nested").join("session.json");
        let store = SessionStore::new(Some(path.clone()));
        assert_eq!(store.load("http://a", "admin"), None);
        store.save("http://a", "tok", "admin").unwrap();
        store.save("http://a", "ktok", "api_key_viewer").unwrap();
        store.save("http://b", "tok2", "api_key_viewer").unwrap();
        assert_eq!(store.load("http://a", "admin").as_deref(), Some("tok"));
        assert_eq!(
            store.load("http://a", "api_key_viewer").as_deref(),
            Some("ktok")
        );
        assert_eq!(store.load("http://b", "admin"), None);
        assert_eq!(store.load_all("http://a").len(), 2);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        store.remove("http://a", Some("admin")).unwrap();
        assert_eq!(store.load("http://a", "admin"), None);
        assert_eq!(
            store.load("http://a", "api_key_viewer").as_deref(),
            Some("ktok")
        );
        store.remove_token("http://a", "ktok").unwrap();
        assert!(store.load_all("http://a").is_empty());
        assert_eq!(
            store.load("http://b", "api_key_viewer").as_deref(),
            Some("tok2")
        );
        store.remove("http://b", None).unwrap();
        assert!(store.load_all("http://b").is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_legacy_entries_keyed_by_base_url() {
        let dir = std::env::temp_dir().join(format!("cpatop-legacy-{}", std::process::id()));
        let path = dir.join("session.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            &path,
            r#"{"sessions":{"http://a":{"token":"old","role":"admin","saved_at":"x"}}}"#,
        )
        .unwrap();
        let store = SessionStore::new(Some(path));
        assert_eq!(store.load("http://a", "admin").as_deref(), Some("old"));
        store.save("http://a", "new", "admin").unwrap();
        assert_eq!(store.load_all("http://a").len(), 1);
        assert_eq!(store.load("http://a", "admin").as_deref(), Some("new"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
