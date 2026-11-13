//! The "change server URL" field of the offline, connecting and login
//! screens: edit, validate, probe, then switch and save.

use super::{App, Data, Effect, Screen};
use crate::server::{self, BaseUrlSource, ProbeError};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

/// The URL being typed. `cursor` counts characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerEdit {
    pub input: String,
    pub cursor: usize,
    pub error: Option<String>,
    /// Id of the probe in flight for the submitted URL.
    pub probing: Option<u64>,
}

impl ServerEdit {
    fn len(&self) -> usize {
        self.input.chars().count()
    }

    fn byte_at(&self, char_idx: usize) -> usize {
        self.input
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(self.input.len())
    }

    fn insert(&mut self, c: char) {
        let at = self.byte_at(self.cursor);
        self.input.insert(at, c);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let at = self.byte_at(self.cursor);
            self.input.remove(at);
        }
    }

    fn delete(&mut self) {
        if self.cursor < self.len() {
            let at = self.byte_at(self.cursor);
            self.input.remove(at);
        }
    }
}

/// The typed URL as shown in privacy mode: the scheme, `:port` and `/` stay,
/// every other character (host, IP, user, path) becomes `•`. One character
/// per character typed, so the cursor stays put.
pub fn privacy_echo(s: &str) -> String {
    let bullets = |t: &str| "•".repeat(t.chars().count());
    let (scheme, rest) = match s.find("://") {
        Some(i) if s[..i].chars().all(|c| c.is_ascii_alphabetic()) => s.split_at(i + 3),
        _ => ("", s),
    };
    let auth_end = rest.find('/').unwrap_or(rest.len());
    let (auth, path) = rest.split_at(auth_end);
    let port_at = auth.rfind(':').filter(|&i| {
        let host = &auth[..i];
        auth[i + 1..].chars().all(|c| c.is_ascii_digit())
            && (!host.contains(':') || host.ends_with(']'))
    });
    let (host, port) = match port_at {
        Some(i) => auth.split_at(i),
        None => (auth, ""),
    };
    let path: String = path
        .chars()
        .map(|c| if c == '/' { '/' } else { '•' })
        .collect();
    format!("{scheme}{}{port}{path}", bullets(host))
}

impl App {
    /// Open the field, prefilled with the current URL.
    pub fn open_server_edit(&mut self) {
        let input = self.settings.base_url.clone();
        self.server_edit = Some(ServerEdit {
            cursor: input.chars().count(),
            input,
            error: None,
            probing: None,
        });
    }

    /// The field's text as displayed (masked in privacy mode).
    pub fn server_edit_echo(&self, e: &ServerEdit) -> String {
        if self.settings.privacy {
            privacy_echo(&e.input)
        } else {
            e.input.clone()
        }
    }

    pub(crate) fn on_server_edit_key(&mut self, k: KeyEvent) {
        let Some(e) = &mut self.server_edit else {
            return;
        };
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let before = e.input.clone();
        match k.code {
            KeyCode::Esc => {
                self.server_edit = None;
                return;
            }
            KeyCode::Enter => {
                if e.probing.is_some() {
                    return;
                }
                match server::normalize_input(&e.input) {
                    Err(m) => e.error = Some(m),
                    Ok(url) => {
                        self.probe_seq += 1;
                        e.probing = Some(self.probe_seq);
                        e.error = None;
                        self.effects.push(Effect::ProbeServer {
                            id: self.probe_seq,
                            url,
                        });
                    }
                }
                return;
            }
            KeyCode::Left => e.cursor = e.cursor.saturating_sub(1),
            KeyCode::Right => e.cursor = (e.cursor + 1).min(e.len()),
            KeyCode::Home => e.cursor = 0,
            KeyCode::End => e.cursor = e.len(),
            KeyCode::Char('a') if ctrl => e.cursor = 0,
            KeyCode::Char('e') if ctrl => e.cursor = e.len(),
            KeyCode::Char('u') if ctrl => {
                e.input.clear();
                e.cursor = 0;
            }
            KeyCode::Backspace => e.backspace(),
            KeyCode::Delete => e.delete(),
            KeyCode::Char(c) if !ctrl && e.input.len() < 2048 => e.insert(c),
            _ => {}
        }
        if e.input != before {
            // Editing abandons a check in flight and clears its verdict.
            e.probing = None;
            e.error = None;
        }
    }

    /// A probe finished. Only the latest one for the open field counts.
    pub fn on_server_probe(&mut self, id: u64, url: String, result: Result<(), ProbeError>) {
        let Some(e) = &mut self.server_edit else {
            return;
        };
        if e.probing != Some(id) {
            return;
        }
        e.probing = None;
        if let Err(err) = result {
            e.error = Some(err.to_string());
            return;
        }
        // Forget everything about the old server; the runner restarts the
        // background tasks for the new one, which logs in or asks to.
        self.server_edit = None;
        self.settings.base_url = url.clone();
        self.host = crate::api::client::display_host(&url);
        self.role = None;
        self.session = None;
        self.data = Data::default();
        self.live.reset();
        self.api_key = None;
        self.overlay = None;
        self.poll_error.clear();
        self.expired_sent = None;
        self.gate_sent = None;
        self.logout_armed = None;
        self.server_offset = None;
        self.screen = Screen::Connecting;
        self.effects.push(Effect::SwitchServer(url));
    }

    /// The runner switched servers and tried to save the URL.
    pub fn on_server_saved(&mut self, saved: Result<PathBuf, String>) {
        let privacy = self.settings.privacy;
        let msg = match saved {
            Ok(p) => {
                let shown = server::display_path(&p, privacy, server::home_dir().as_deref());
                if self.base_url_source == BaseUrlSource::Default {
                    self.base_url_source = BaseUrlSource::Config;
                }
                match self.base_url_source.overrides() {
                    Some(o) => format!("saved to {shown} · {o} wins next start"),
                    None => format!("server URL saved to {shown}"),
                }
            }
            Err(_) if privacy => "server changed · not saved to the config file".into(),
            Err(e) => format!("server changed · not saved: {e}"),
        };
        self.server_notice = Some(msg.clone());
        self.flash(msg);
    }

    /// The new server's client could not be created (should not happen
    /// after a successful probe).
    pub fn on_server_switch_failed(&mut self, err: String) {
        self.open_server_edit();
        if let Some(e) = &mut self.server_edit {
            e.error = Some(err);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_echo_hides_hosts_and_paths() {
        assert_eq!(
            privacy_echo("http://127.0.0.1:8318"),
            "http://•••••••••:8318"
        );
        assert_eq!(
            privacy_echo("https://keeper.example/sub"),
            "https://••••••••••••••/•••"
        );
        assert_eq!(privacy_echo("10.0.0.1"), "••••••••");
        assert_eq!(privacy_echo("http://[::1]:8318"), "http://•••••:8318");
        assert_eq!(privacy_echo("http://u:pw@h"), "http://••••••");
        assert_eq!(privacy_echo("fe80::1"), "•••••••");
        for s in ["http://127.0.0.1:8318", "x.example/a/b", "http://h:1/"] {
            assert_eq!(privacy_echo(s).chars().count(), s.chars().count(), "{s}");
        }
    }

    #[test]
    fn editing_moves_the_cursor_by_characters() {
        let mut e = ServerEdit {
            input: "hé".into(),
            cursor: 2,
            error: None,
            probing: None,
        };
        e.backspace();
        assert_eq!((e.input.as_str(), e.cursor), ("h", 1));
        e.insert('ü');
        e.cursor = 0;
        e.insert('x');
        assert_eq!((e.input.as_str(), e.cursor), ("xhü", 1));
        e.delete();
        assert_eq!(e.input, "xü");
    }
}
