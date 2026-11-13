//! Command-line interface.

use crate::api::LoginMode;
use crate::config::GraphSymbols;
use crate::range::TimeRange;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// A btop-style terminal dashboard for CPA Usage Keeper.
///
/// Secrets are never accepted as arguments. Provide them with
/// --password-file / --api-key-file, the CPATOP_PASSWORD / CPATOP_API_KEY
/// environment variables, the OS keyring (`cpatop keyring set`), or type them
/// into the login screen.
///
/// --theme, --range, --refresh, --symbols and --privacy apply to this run
/// only; changes made inside the app (t, r, +/-, b, x) are remembered.
#[derive(Debug, Parser)]
#[command(name = "cpatop", version, about, long_about)]
pub struct Cli {
    /// Keeper base URL, e.g. http://localhost:8318 or https://host/keeper
    #[arg(long, global = true, env = "CPATOP_BASE_URL", value_name = "URL")]
    pub base_url: Option<String>,

    /// Config file (default: $XDG_CONFIG_HOME/cpatop/config.toml)
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// File containing the admin password
    #[arg(long, global = true, value_name = "PATH")]
    pub password_file: Option<PathBuf>,

    /// File containing a CPA API key (per-key dashboard)
    #[arg(long, global = true, value_name = "PATH")]
    pub api_key_file: Option<PathBuf>,

    /// Preferred login when several credentials are available
    #[arg(long, global = true, value_enum, value_name = "MODE")]
    pub auth: Option<LoginMode>,

    /// Do not look up credentials in the OS keyring
    #[arg(long, global = true)]
    pub no_keyring: bool,

    /// Log out the stored session for this server and start a new one
    #[arg(long, global = true)]
    pub fresh_session: bool,

    /// Colour theme name
    #[arg(long, global = true, value_name = "NAME")]
    pub theme: Option<String>,

    /// Time range: today, yesterday, 5h..24h, 1d..30d or custom:START..END (YYYY-MM-DD)
    #[arg(long, global = true, value_name = "RANGE", value_parser = parse_range)]
    pub range: Option<TimeRange>,

    /// Overview/realtime refresh interval in seconds
    #[arg(long, global = true, value_name = "SECS", value_parser = clap::value_parser!(u64).range(1..=3600))]
    pub refresh: Option<u64>,

    /// Graph symbols
    #[arg(long, global = true, value_enum, value_name = "KIND")]
    pub symbols: Option<GraphSymbols>,

    /// Hide e-mail addresses, API keys and IP addresses (screenshots, screen
    /// sharing), for this run only; `x` in the app toggles it and is remembered
    #[arg(long, global = true)]
    pub privacy: bool,

    /// Log out on the server when quitting (by default `q` keeps the session
    /// for the next start; `cpatop logout` ends it later)
    #[arg(long, global = true)]
    pub logout_on_exit: bool,

    /// Disable mouse support
    #[arg(long, global = true)]
    pub no_mouse: bool,

    /// Do not check GitHub for a newer cpatop release on start
    /// (also CPATOP_NO_UPDATE_CHECK=1 or config `update_check = false`)
    #[arg(long, global = true)]
    pub no_update_check: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

fn parse_range(s: &str) -> Result<TimeRange, String> {
    s.parse()
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Store or remove credentials in the OS keyring
    Keyring {
        #[command(subcommand)]
        action: KeyringAction,
    },
    /// Log out on the server and forget the stored session cookie
    Logout,
    /// List available themes, or print one as TOML to start a custom theme
    Themes {
        /// Print this theme in the theme file format
        #[arg(long, value_name = "NAME")]
        dump: Option<String>,
    },
    /// Show the config, theme and state paths in use
    Paths,
    /// Check GitHub for a newer cpatop release now
    CheckUpdate,
}

#[derive(Debug, Subcommand)]
pub enum KeyringAction {
    /// Prompt for a secret (hidden input) and store it in the keyring
    Set {
        #[arg(value_enum)]
        mode: LoginMode,
    },
    /// Remove a stored secret
    Delete {
        #[arg(value_enum)]
        mode: LoginMode,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_flags() {
        let c = Cli::try_parse_from([
            "cpatop",
            "--base-url",
            "https://h/keeper",
            "--password-file",
            "/tmp/x",
            "--range",
            "7d",
            "--auth",
            "api-key",
        ])
        .unwrap();
        assert_eq!(c.range, Some(TimeRange::Days(7)));
        assert_eq!(c.auth, Some(LoginMode::ApiKey));
        assert!(Cli::try_parse_from(["cpatop", "--range", "99d"]).is_err());
        // Connection and auth flags also work after a subcommand.
        let c = Cli::try_parse_from(["cpatop", "logout", "--base-url", "http://h:1", "--privacy"])
            .unwrap();
        assert_eq!(c.base_url.as_deref(), Some("http://h:1"));
        assert!(c.privacy && matches!(c.command, Some(Command::Logout)));
        // There is no flag that takes a secret value.
        assert!(Cli::try_parse_from(["cpatop", "--password", "x"]).is_err());
    }
}
