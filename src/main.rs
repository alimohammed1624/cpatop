use clap::Parser;
use color_eyre::eyre::{Result, WrapErr, eyre};
use cpatop::api::client::normalize_base_url;
use cpatop::api::{Client, LoginMode};
use cpatop::app::App;
use cpatop::cli::{Cli, Command, KeyringAction};
use cpatop::config::{DEFAULT_BASE_URL, FileConfig, Paths, Settings, UiState};
use cpatop::secrets::{self, ResolveOptions, Secret};
use cpatop::session_store::SessionStore;
use cpatop::theme;

fn main() -> Result<()> {
    color_eyre::install()?;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cli = Cli::parse();
    let paths = Paths::discover();

    let config_path = cli.config.clone().or_else(|| paths.config_file());
    let file_cfg = match &config_path {
        Some(p) => FileConfig::load(p)
            .map_err(|e| eyre!(e))
            .wrap_err("invalid config file")?
            .unwrap_or_default(),
        None => FileConfig::default(),
    };
    if cli.config.is_some() && config_path.as_ref().is_some_and(|p| !p.exists()) {
        return Err(eyre!(
            "config file {} not found",
            config_path.unwrap().display()
        ));
    }
    let ui_state = paths
        .ui_state_file()
        .map(|p| UiState::load(&p))
        .unwrap_or_default();

    let raw_base = cli
        .base_url
        .clone()
        .or(file_cfg.base_url.clone())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
    let base_url = normalize_base_url(&raw_base).map_err(|e| eyre!(e))?;

    let (themes, theme_errors) = theme::all_themes(paths.themes_dir().as_deref());

    match &cli.command {
        Some(Command::Themes { dump }) => {
            if let Some(name) = dump {
                let t = themes
                    .iter()
                    .find(|t| &t.name == name)
                    .ok_or_else(|| eyre!("unknown theme {name:?}"))?;
                print!("{}", theme::to_toml(t));
            } else {
                for t in &themes {
                    let kind = if theme::BUILTIN.contains(&t.name.as_str()) {
                        "built-in"
                    } else {
                        "user"
                    };
                    println!("{:<14} {kind}", t.name);
                }
                for e in theme_errors {
                    eprintln!("warning: {e}");
                }
            }
            return Ok(());
        }
        Some(Command::Paths) => {
            let show = |p: Option<std::path::PathBuf>| {
                p.map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(unavailable)".into())
            };
            println!("config file : {}", show(config_path.clone()));
            println!("ui state    : {}", show(paths.ui_state_file()));
            println!("themes dir  : {}", show(paths.themes_dir()));
            println!("session     : {}", show(paths.session_file()));
            return Ok(());
        }
        Some(Command::Keyring { action }) => {
            return match action {
                KeyringAction::Set { mode } => {
                    let label = match mode {
                        LoginMode::Admin => "Admin password",
                        LoginMode::ApiKey => "CPA API key",
                    };
                    let value = rpassword::prompt_password(format!("{label} for {base_url}: "))
                        .wrap_err("could not read from the terminal")?;
                    let secret = Secret::new(value.trim().to_string());
                    drop(value);
                    if secret.is_empty() {
                        return Err(eyre!("nothing entered"));
                    }
                    secrets::keyring_set(*mode, &base_url, &secret).map_err(|e| eyre!(e))?;
                    println!(
                        "stored in the OS keyring as {}",
                        secrets::keyring_user(*mode, &base_url)
                    );
                    Ok(())
                }
                KeyringAction::Delete { mode } => {
                    let existed =
                        secrets::keyring_delete(*mode, &base_url).map_err(|e| eyre!(e))?;
                    println!("{}", if existed { "removed" } else { "nothing stored" });
                    Ok(())
                }
            };
        }
        Some(Command::Logout) => {
            let store = SessionStore::new(paths.session_file());
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let client = Client::new(&base_url).map_err(|e| eyre!(e))?;
            match store.load(&base_url) {
                Some(tok) => {
                    client.set_token(Some(tok));
                    let r = rt.block_on(client.logout());
                    store.remove(&base_url).map_err(|e| eyre!(e))?;
                    match r {
                        Ok(()) => println!("logged out of {base_url}"),
                        Err(e) => println!("forgot the local session ({e})"),
                    }
                }
                None => println!("no stored session for {base_url}"),
            }
            return Ok(());
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
    .map_err(|e| eyre!(e))?;

    let store = SessionStore::new(paths.session_file());
    if cli.fresh_session {
        let _ = store.remove(&base_url);
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
    };

    let mut app = App::new(settings, themes, &ui_state);
    app.warnings = warnings;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(cpatop::tui::run(cpatop::tui::RunArgs {
        app,
        creds,
        store,
        ui_state_path: paths.ui_state_file(),
    }))
}
