use clap::Parser;
use cpatop::api::client::normalize_base_url;
use cpatop::api::{ApiError, Client, LoginMode};
use cpatop::app::{
    App, output_privacy, private_path, scrub_output, set_output_privacy, shown_path,
};
use cpatop::cli::{Cli, Command, KeyringAction};
use cpatop::config::{DEFAULT_BASE_URL, FileConfig, Paths, Settings, UiState};
use cpatop::secrets::{self, ResolveOptions, Secret};
use cpatop::session_store::SessionStore;
use cpatop::theme;
use cpatop::update;
use std::io::IsTerminal;
use std::process::ExitCode;

/// A fatal error, printed as `cpatop: <msg>` (plus an optional `hint:` line).
#[derive(Debug)]
struct CliError {
    msg: String,
    hint: Option<String>,
    /// 2: bad flag value, base URL, config or credential file; 1: runtime.
    code: u8,
    /// Full report for runtime errors, shown when RUST_BACKTRACE is set.
    report: Option<color_eyre::Report>,
}

impl CliError {
    fn usage(msg: impl Into<String>) -> Self {
        CliError {
            msg: msg.into(),
            hint: None,
            code: 2,
            report: None,
        }
    }

    fn runtime(msg: impl Into<String>) -> Self {
        CliError {
            code: 1,
            ..CliError::usage(msg)
        }
    }

    fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Print to stderr; in privacy mode every line goes through the same
    /// scrubber as the screen.
    fn print(&self) {
        let verbose = std::env::var_os("RUST_BACKTRACE").is_some_and(|v| !v.is_empty() && v != "0");
        if verbose && let Some(report) = &self.report {
            eprintln!("{}", scrub_output(&format!("{report:?}")));
            return;
        }
        eprintln!("cpatop: {}", scrub_output(&self.msg));
        if let Some(h) = &self.hint {
            eprintln!("hint: {}", scrub_output(h));
        }
    }
}

impl From<color_eyre::Report> for CliError {
    fn from(e: color_eyre::Report) -> Self {
        let msg = e
            .chain()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(": ");
        CliError {
            report: Some(e),
            ..CliError::runtime(msg)
        }
    }
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        CliError::from(color_eyre::Report::new(e))
    }
}

fn main() -> ExitCode {
    // Privacy mode must be known before anything is printed, including
    // argument errors: from the raw arguments, the config file and the
    // saved UI state (refined once they are parsed).
    set_output_privacy(early_privacy());
    install_hooks();
    let _ = rustls::crypto::ring::default_provider().install_default();
    match run() {
        Ok(code) => code,
        Err(e) => {
            e.print();
            ExitCode::from(e.code)
        }
    }
}

/// Error-report and panic hooks. Panic messages are scrubbed (and
/// uncoloured) in privacy mode; reports are printed by [`CliError::print`].
fn install_hooks() {
    // Uncoloured when NO_COLOR is set or stderr is not a terminal.
    let plain = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
        || !std::io::stderr().is_terminal();
    let theme = if plain {
        color_eyre::config::Theme::new()
    } else {
        color_eyre::config::Theme::dark()
    };
    let (panic_hook, eyre_hook) = color_eyre::config::HookBuilder::default()
        .theme(theme)
        .into_hooks();
    let (plain_panic, _) = color_eyre::config::HookBuilder::default()
        .theme(color_eyre::config::Theme::new())
        .into_hooks();
    let _ = eyre_hook.install();
    std::panic::set_hook(Box::new(move |info| {
        if output_privacy() {
            eprintln!(
                "{}",
                scrub_output(&plain_panic.panic_report(info).to_string())
            );
        } else {
            eprintln!("{}", panic_hook.panic_report(info));
        }
    }));
}

/// Whether privacy mode is on before the arguments are parsed: `--privacy`
/// among the raw arguments, or `privacy` in the config file (default or
/// `--config`) or the saved UI state.
fn early_privacy() -> bool {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if args
        .iter()
        .any(|a| a == "--privacy" || a.starts_with("--privacy="))
    {
        return true;
    }
    let paths = Paths::discover();
    let config = args
        .iter()
        .position(|a| a == "--config")
        .and_then(|i| args.get(i + 1).map(std::path::PathBuf::from))
        .or_else(|| {
            args.iter()
                .find_map(|a| a.strip_prefix("--config=").map(std::path::PathBuf::from))
        })
        .or_else(|| paths.config_file());
    let file = config
        .and_then(|p| FileConfig::load(&p).ok().flatten())
        .and_then(|c| c.privacy);
    let state = paths
        .ui_state_file()
        .map(|p| UiState::load(&p))
        .and_then(|s| s.privacy);
    state.or(file).unwrap_or(false)
}

/// Parse the arguments. Errors (and help, which can show environment
/// values) are scrubbed in privacy mode.
fn parse_cli() -> Cli {
    match Cli::try_parse() {
        Ok(c) => c,
        Err(e) if output_privacy() => {
            let text = scrub_output(&e.render().to_string());
            if e.use_stderr() {
                eprint!("{text}");
            } else {
                print!("{text}");
            }
            std::process::exit(e.exit_code());
        }
        Err(e) => e.exit(),
    }
}

/// Print a subcommand's output line, scrubbed in privacy mode.
fn out(s: impl AsRef<str>) {
    println!("{}", scrub_output(s.as_ref()));
}

fn current_thread_runtime() -> Result<tokio::runtime::Runtime, CliError> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?)
}

/// A stored session's role and the server's answer to its logout.
type RoleLogout = (String, Result<(), ApiError>);

/// Log every stored session for `base_url` out on the server, then forget it
/// locally. Returns each session's role and the server's answer.
fn logout_stored(store: &SessionStore, base_url: &str) -> Result<Vec<RoleLogout>, CliError> {
    let stored = store.load_all(base_url);
    if stored.is_empty() {
        return Ok(Vec::new());
    }
    let rt = current_thread_runtime()?;
    let client = Client::new(base_url).map_err(CliError::runtime)?;
    let mut out = Vec::new();
    for st in stored {
        client.set_token(Some(st.token.clone()));
        let r = rt.block_on(client.logout());
        store
            .remove_token(base_url, &st.token)
            .map_err(CliError::runtime)?;
        out.push((st.role, r));
    }
    Ok(out)
}

/// The process exit status on success: 0, or 128 + the signal that ended
/// the dashboard.
fn run() -> Result<ExitCode, CliError> {
    let cli = parse_cli();
    let paths = Paths::discover();

    let config_path = cli.config.clone().or_else(|| paths.config_file());
    if let Some(p) = &cli.config
        && !p.exists()
    {
        return Err(
            CliError::usage(format!("config file {} not found", shown_path(p))).hint(
                "create it (see config.example.toml) or drop --config; \
                 `cpatop paths` shows the default location",
            ),
        );
    }
    let file_cfg = match &config_path {
        Some(p) => FileConfig::load(p)
            .map_err(|e| CliError::usage(format!("invalid config file {e}")))?
            .unwrap_or_default(),
        None => FileConfig::default(),
    };
    let ui_state = paths
        .ui_state_file()
        .map(|p| UiState::load(&p))
        .unwrap_or_default();
    set_output_privacy(cli.privacy || ui_state.privacy.or(file_cfg.privacy).unwrap_or(false));

    let raw_base = cli
        .base_url
        .clone()
        .or(file_cfg.base_url.clone())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
    let base_url = normalize_base_url(&raw_base).map_err(|e| {
        CliError::usage(e).hint("use an http:// or https:// URL, e.g. http://localhost:8318")
    })?;

    let (themes, theme_errors) = theme::all_themes(paths.themes_dir().as_deref());

    match &cli.command {
        Some(Command::Themes { dump }) => {
            if let Some(name) = dump {
                let t = themes.iter().find(|t| &t.name == name).ok_or_else(|| {
                    CliError::usage(format!("unknown theme {name:?}"))
                        .hint("`cpatop themes` lists the available themes")
                })?;
                print!("{}", scrub_output(&theme::to_toml(t)));
            } else {
                for t in &themes {
                    let kind = if theme::BUILTIN.contains(&t.name.as_str()) {
                        "built-in"
                    } else {
                        "user"
                    };
                    out(format!("{:<14} {kind}", t.name));
                }
                for e in theme_errors {
                    eprintln!("warning: {}", scrub_output(&e));
                }
            }
            return Ok(ExitCode::SUCCESS);
        }
        Some(Command::Paths) => {
            // In privacy mode each path is masked segment by segment
            // (`~/.config/cpatop/…`), not scrubbed whole as free text.
            let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
            let show = |p: Option<std::path::PathBuf>| match p {
                Some(p) if output_privacy() => private_path(&p, home.as_deref()),
                Some(p) => p.display().to_string(),
                None => "(unavailable)".into(),
            };
            println!("config file : {}", show(config_path.clone()));
            println!("ui state    : {}", show(paths.ui_state_file()));
            println!("themes dir  : {}", show(paths.themes_dir()));
            println!("session     : {}", show(paths.session_file()));
            println!("update cache: {}", show(paths.update_cache_file()));
            return Ok(ExitCode::SUCCESS);
        }
        Some(Command::CheckUpdate) => {
            // Explicit: ignores the opt-outs and the cache (and refreshes it).
            let rt = current_thread_runtime()?;
            let cache = paths.update_cache_file();
            let r = rt.block_on(update::check_and_cache(cache.as_deref()));
            out(update::report(&r, update::CURRENT));
            return Ok(ExitCode::SUCCESS);
        }
        Some(Command::Keyring { action }) => {
            let done: Result<(), CliError> = match action {
                KeyringAction::Set { mode } => {
                    let label = match mode {
                        LoginMode::Admin => "Admin password",
                        LoginMode::ApiKey => "CPA API key",
                    };
                    let value = rpassword::prompt_password(scrub_output(&format!(
                        "{label} for {base_url}: "
                    )))
                    .map_err(|e| {
                        CliError::runtime(format!("could not read from the terminal: {e}"))
                    })?;
                    let secret = Secret::new(value.trim().to_string());
                    drop(value);
                    if secret.is_empty() {
                        return Err(CliError::runtime("nothing entered"));
                    }
                    secrets::keyring_set(*mode, &base_url, &secret).map_err(CliError::runtime)?;
                    out(format!(
                        "stored in the OS keyring as {}",
                        secrets::keyring_user(*mode, &base_url)
                    ));
                    Ok(())
                }
                KeyringAction::Delete { mode } => {
                    let existed =
                        secrets::keyring_delete(*mode, &base_url).map_err(CliError::runtime)?;
                    out(if existed { "removed" } else { "nothing stored" });
                    Ok(())
                }
            };
            done?;
            return Ok(ExitCode::SUCCESS);
        }
        Some(Command::Logout) => {
            let store = SessionStore::new(paths.session_file());
            let results = logout_stored(&store, &base_url)?;
            if results.is_empty() {
                out(format!("no stored session for {base_url}"));
            }
            for (role, r) in results {
                match r {
                    Ok(()) => out(format!("logged out of {base_url} ({role})")),
                    Err(e) => out(format!("forgot the local {role} session ({e})")),
                }
            }
            return Ok(ExitCode::SUCCESS);
        }
        None => {}
    }

    // Credentials.
    let use_keyring = !cli.no_keyring && file_cfg.keyring.unwrap_or(true);
    let creds = secrets::resolve(&ResolveOptions {
        base_url: base_url.clone(),
        password_file: cli.password_file.clone().or(file_cfg.password_file.clone()),
        api_key_file: cli.api_key_file.clone().or(file_cfg.api_key_file.clone()),
        prefer: cli.auth.or(file_cfg.auth),
        use_keyring,
    })
    .map_err(|e| {
        let hint = if e.starts_with("cannot read") {
            "create it with the secret on a single line, then chmod 600 it"
        } else {
            "the file must contain the secret on a single line"
        };
        CliError::usage(e).hint(hint)
    })?;

    let store = SessionStore::new(paths.session_file());
    if cli.fresh_session {
        // End the old sessions on the server too; failures (server down,
        // already expired) only mean there is nothing left to end.
        let _ = logout_stored(&store, &base_url);
        let _ = store.remove(&base_url, None);
    }

    // Effective settings: CLI > persisted UI state > config file > defaults.
    let d = Settings::default();
    let mut warnings = creds.warnings.clone();
    warnings.extend(theme_errors);
    let theme_name = cli
        .theme
        .clone()
        .or(ui_state.theme.clone())
        .or(file_cfg.theme.clone())
        .unwrap_or(d.theme.clone());
    let theme_name = if themes.iter().any(|t| t.name == theme_name) {
        theme_name
    } else {
        warnings.push(format!("unknown theme {theme_name:?}, using default"));
        d.theme.clone()
    };
    let settings = Settings {
        base_url,
        refresh_secs: cli
            .refresh
            .or(ui_state.refresh_secs)
            .or(file_cfg.refresh_secs)
            .unwrap_or(d.refresh_secs)
            .clamp(1, 3600),
        slow_refresh_secs: file_cfg
            .slow_refresh_secs
            .unwrap_or(d.slow_refresh_secs)
            .clamp(10, 3600),
        theme: theme_name,
        range: cli
            .range
            .or(ui_state.range)
            .or(file_cfg.range)
            .unwrap_or(d.range),
        window: ui_state.window.unwrap_or(d.window),
        graph_symbols: cli
            .symbols
            .or(ui_state.graph_symbols)
            .or(file_cfg.graph_symbols)
            .unwrap_or(d.graph_symbols),
        privacy: cli.privacy || ui_state.privacy.or(file_cfg.privacy).unwrap_or(false),
        mouse: !cli.no_mouse && file_cfg.mouse.unwrap_or(true),
        logout_on_exit: cli.logout_on_exit || file_cfg.logout_on_exit.unwrap_or(false),
        no_color: std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()),
    };

    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        return Err(
            CliError::runtime("the dashboard needs an interactive terminal").hint(
                "run cpatop in a terminal; subcommands such as `cpatop paths` work without one",
            ),
        );
    }

    let update_check = update::enabled(
        cli.no_update_check,
        file_cfg.update_check,
        std::env::var_os("CPATOP_NO_UPDATE_CHECK").as_deref(),
        std::env::var_os("CI").as_deref(),
    );

    // Command-line values apply to this run only; they are not saved.
    let cli_overrides = cpatop::config::UiState {
        privacy: cli.privacy.then_some(true),
        range: cli.range.map(|_| settings.range),
        theme: cli.theme.as_ref().map(|_| settings.theme.clone()),
        graph_symbols: cli.symbols.map(|_| settings.graph_symbols),
        refresh_secs: cli.refresh.map(|_| settings.refresh_secs),
        ..Default::default()
    };
    let mut app = App::new(settings, themes, &ui_state);
    app.cli_overrides = cli_overrides;
    app.warnings = warnings;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let exit = rt.block_on(cpatop::tui::run(cpatop::tui::RunArgs {
        app,
        creds,
        store,
        ui_state_path: paths.ui_state_file(),
        update_check,
        update_cache_path: paths.update_cache_file(),
        config_path,
        base_url_source: cpatop::server::BaseUrlSource::detect(
            cli.base_url.as_deref(),
            file_cfg.base_url.is_some(),
        ),
        use_keyring,
    }))?;
    Ok(ExitCode::from(exit.code()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes() {
        let e = CliError::usage("bad").hint("fix it");
        assert_eq!((e.code, e.hint.as_deref()), (2, Some("fix it")));
        assert_eq!(CliError::runtime("x").code, 1);
        let io = std::io::Error::other("tty gone");
        let e = CliError::from(io);
        assert_eq!(e.code, 1);
        assert_eq!(e.msg, "tty gone");
    }

    #[test]
    fn report_chain_is_one_line() {
        use color_eyre::eyre::{WrapErr, eyre};
        let r: color_eyre::Result<()> = Err(eyre!("inner")).wrap_err("outer");
        let e = CliError::from(r.unwrap_err());
        assert_eq!(e.msg, "outer: inner");
        assert!(!e.msg.contains('\n'));
    }
}
