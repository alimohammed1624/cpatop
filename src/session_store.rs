//! Persisted session cookies (never passwords), one per base URL.
//!
//! Stored as JSON in the XDG state directory with mode 0600.

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
    #[serde(default)]
    role: String,
    #[serde(default)]
    saved_at: String,
}

#[derive(Debug, Clone)]
pub struct SessionStore {
    path: Option<PathBuf>,
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

    pub fn load(&self, base_url: &str) -> Option<String> {
        self.read()
            .sessions
            .get(base_url)
            .map(|e| e.token.clone())
            .filter(|t| !t.is_empty())
    }

    pub fn save(&self, base_url: &str, token: &str, role: &str) -> Result<(), String> {
        let mut f = self.read();
        f.sessions.insert(
            base_url.to_string(),
            Entry {
                token: token.to_string(),
                role: role.to_string(),
                saved_at: chrono::Utc::now().to_rfc3339(),
            },
        );
        self.write(&f)
    }

    pub fn remove(&self, base_url: &str) -> Result<(), String> {
        let mut f = self.read();
        if f.sessions.remove(base_url).is_some() {
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
            .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        f.write_all(data).map_err(|e| e.to_string())?;
        f.sync_all().ok();
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("cannot write {}: {e}", path.display()))
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
        .map_err(|e| format!("cannot create {}: {e}", dir.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_private_mode() {
        let dir = std::env::temp_dir().join(format!("cpatop-store-{}", std::process::id()));
        let path = dir.join("nested").join("session.json");
        let store = SessionStore::new(Some(path.clone()));
        assert_eq!(store.load("http://a"), None);
        store.save("http://a", "tok", "admin").unwrap();
        store.save("http://b", "tok2", "api_key_viewer").unwrap();
        assert_eq!(store.load("http://a").as_deref(), Some("tok"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        store.remove("http://a").unwrap();
        assert_eq!(store.load("http://a"), None);
        assert_eq!(store.load("http://b").as_deref(), Some("tok2"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
