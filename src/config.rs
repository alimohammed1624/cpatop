//! Configuration file, persisted UI state and filesystem locations.

use crate::api::LoginMode;
use crate::range::{RtWindow, TimeRange};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const DEFAULT_BASE_URL: &str = "http://localhost:8318";

/// How graphs are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum GraphSymbols {
    #[default]
    Braille,
    Block,
    Ascii,
}

impl GraphSymbols {
    pub fn next(self) -> Self {
        match self {
            GraphSymbols::Braille => GraphSymbols::Block,
            GraphSymbols::Block => GraphSymbols::Ascii,
            GraphSymbols::Ascii => GraphSymbols::Braille,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            GraphSymbols::Braille => "braille",
            GraphSymbols::Block => "block",
            GraphSymbols::Ascii => "ascii",
        }
    }
}

/// `config.toml`. Every key is optional.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct FileConfig {
    pub base_url: Option<String>,
    /// Which credential to use when both are configured.
    pub auth: Option<LoginMode>,
    pub password_file: Option<PathBuf>,
    pub api_key_file: Option<PathBuf>,
    /// Look up credentials in the OS keyring (default true).
    pub keyring: Option<bool>,
    /// Overview/realtime poll interval in seconds.
    pub refresh_secs: Option<u64>,
    /// Credentials/quota/settings poll interval in seconds.
    pub slow_refresh_secs: Option<u64>,
    pub theme: Option<String>,
    pub range: Option<TimeRange>,
    pub graph_symbols: Option<GraphSymbols>,
    /// Hide e-mail addresses, API keys and IP addresses (handy for
    /// screenshots and screen sharing).
    pub privacy: Option<bool>,
    pub mouse: Option<bool>,
    /// Log the server session out when quitting (default: keep it so the
    /// next start reuses it).
    pub logout_on_exit: Option<bool>,
    /// Check GitHub for a newer cpatop release on start (default true).
    pub update_check: Option<bool>,
}

impl FileConfig {
    pub fn load(path: &Path) -> Result<Option<FileConfig>, String> {
        match std::fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).map(Some).map_err(|e| {
                format!(
                    "{}: {}",
                    crate::app::shown_path(path),
                    crate::app::scrub_output(&e.to_string())
                )
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", crate::app::shown_path(path))),
        }
    }
}

/// UI state persisted between runs (`ui-state.toml` in the config dir).
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct UiState {
    pub theme: Option<String>,
    pub range: Option<TimeRange>,
    pub window: Option<RtWindow>,
    pub refresh_secs: Option<u64>,
    pub graph_symbols: Option<GraphSymbols>,
    pub privacy: Option<bool>,
    /// Key for privacy-mode tags of names that are not a known credential.
    pub privacy_salt: Option<u64>,
    /// Layout preset index per tab.
    pub layout: BTreeMap<String, usize>,
    /// Hidden pane numbers per tab.
    pub hidden: BTreeMap<String, Vec<usize>>,
    /// Pane numbering the numbers here use (`layout::PANE_ORDER`); missing
    /// in files written before panes were renumbered.
    pub pane_order: Option<u32>,
    /// Sort column and direction per table (`"col:desc"`).
    pub sort: BTreeMap<String, String>,
    /// Dimension index for multi-dimension tables.
    pub dims: BTreeMap<String, usize>,
    /// Last active tab per role.
    pub tab: BTreeMap<String, String>,
}

impl UiState {
    pub fn load(path: &Path) -> UiState {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let s = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        let body = format!("# cpatop UI state — written automatically, safe to delete.\n{s}");
        crate::session_store::write_private(path, body.as_bytes())
    }
}

/// Filesystem locations.
#[derive(Debug, Clone, Default)]
pub struct Paths {
    pub config_dir: Option<PathBuf>,
    pub state_dir: Option<PathBuf>,
}

fn xdg(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

impl Paths {
    /// Resolve locations, honouring `XDG_CONFIG_HOME` / `XDG_STATE_HOME` on every platform.
    pub fn discover() -> Paths {
        let proj = directories::ProjectDirs::from("", "", "cpatop");
        let config_dir = xdg("XDG_CONFIG_HOME")
            .map(|p| p.join("cpatop"))
            .or_else(|| proj.as_ref().map(|p| p.config_dir().to_path_buf()));
        let state_dir = xdg("XDG_STATE_HOME").map(|p| p.join("cpatop")).or_else(|| {
            proj.as_ref().map(|p| {
                p.state_dir()
                    .unwrap_or_else(|| p.data_local_dir())
                    .to_path_buf()
            })
        });
        Paths {
            config_dir,
            state_dir,
        }
    }

    pub fn config_file(&self) -> Option<PathBuf> {
        self.config_dir.as_ref().map(|d| d.join("config.toml"))
    }
    pub fn ui_state_file(&self) -> Option<PathBuf> {
        self.config_dir.as_ref().map(|d| d.join("ui-state.toml"))
    }
    pub fn themes_dir(&self) -> Option<PathBuf> {
        self.config_dir.as_ref().map(|d| d.join("themes"))
    }
    pub fn session_file(&self) -> Option<PathBuf> {
        self.state_dir.as_ref().map(|d| d.join("session.json"))
    }
    pub fn update_cache_file(&self) -> Option<PathBuf> {
        self.state_dir.as_ref().map(|d| d.join("update-check.json"))
    }
}

/// Effective runtime settings after merging CLI, environment, config and UI state.
#[derive(Debug, Clone)]
pub struct Settings {
    pub base_url: String,
    pub refresh_secs: u64,
    pub slow_refresh_secs: u64,
    pub theme: String,
    pub range: TimeRange,
    pub window: RtWindow,
    pub graph_symbols: GraphSymbols,
    pub privacy: bool,
    pub mouse: bool,
    /// End the server session when quitting.
    pub logout_on_exit: bool,
    /// `NO_COLOR` is set: render without relying on colour.
    pub no_color: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            base_url: DEFAULT_BASE_URL.into(),
            refresh_secs: 10,
            slow_refresh_secs: 60,
            theme: "default".into(),
            range: TimeRange::Today,
            window: RtWindow::M15,
            graph_symbols: GraphSymbols::Braille,
            privacy: false,
            mouse: true,
            logout_on_exit: false,
            no_color: false,
        }
    }
}

/// Allowed refresh intervals for `+` / `-`.
pub const REFRESH_STEPS: [u64; 8] = [2, 3, 5, 10, 15, 30, 60, 120];

pub fn step_refresh(cur: u64, up: bool) -> u64 {
    let idx = REFRESH_STEPS
        .iter()
        .position(|&s| s >= cur)
        .unwrap_or(REFRESH_STEPS.len() - 1);
    if up {
        REFRESH_STEPS[(idx + 1).min(REFRESH_STEPS.len() - 1)]
    } else {
        REFRESH_STEPS[idx.saturating_sub(1)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_parses() {
        let s = include_str!("../config.example.toml");
        let c: FileConfig = toml::from_str(s).expect("config.example.toml parses");
        assert_eq!(c.base_url.as_deref(), Some("http://localhost:8318"));
    }

    #[test]
    fn ui_state_round_trip() {
        let mut st = UiState {
            theme: Some("nord".into()),
            range: Some(TimeRange::Days(7)),
            ..Default::default()
        };
        st.hidden.insert("overview".into(), vec![3]);
        let s = toml::to_string_pretty(&st).unwrap();
        let back: UiState = toml::from_str(&s).unwrap();
        assert_eq!(back, st);
    }

    #[test]
    fn refresh_steps() {
        assert_eq!(step_refresh(10, true), 15);
        assert_eq!(step_refresh(10, false), 5);
        assert_eq!(step_refresh(2, false), 2);
        assert_eq!(step_refresh(120, true), 120);
        assert_eq!(step_refresh(7, true), 15);
    }
}
