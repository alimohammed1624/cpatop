//! Privacy mode and API-key naming.
//!
//! Privacy mode (`--privacy`, `x`) shows no part of any e-mail address or
//! API key: e-mails become stable `account-N` labels, keys are named by
//! their database id (plus an alias that is itself not an identifier), and
//! IP addresses are hidden — in tables, details, pickers and free text.

use super::*;

impl App {
    //
    // Privacy mode (`--privacy`, `x`) shows no part of any e-mail address or
    // API key: e-mails become stable `account-N` labels, keys are named by
    // their database id (plus an alias that is itself not an identifier),
    // IP addresses are hidden.

    /// Stable `account-N` label for an identifier.
    fn alias_of(&self, s: &str) -> String {
        let mut m = self.privacy_aliases.borrow_mut();
        let next = m.len() + 1;
        let n = *m.entry(s.to_string()).or_insert(next);
        format!("account-{n}")
    }

    /// An identity name (credential, source, account). In privacy mode,
    /// anything that is or contains an e-mail address or a key is replaced.
    pub fn privacy(&self, s: &str) -> String {
        if self.settings.privacy && is_identifier(s) {
            self.alias_of(s)
        } else {
            s.to_string()
        }
    }

    /// Free text (error bodies, user agents, status messages): in privacy
    /// mode every e-mail-like or key-like word is replaced.
    pub fn privacy_text(&self, s: &str) -> String {
        if !self.settings.privacy {
            return s.to_string();
        }
        let mut out = String::with_capacity(s.len());
        let mut word = String::new();
        let flush = |word: &mut String, out: &mut String| {
            if word.is_empty() {
                return;
            }
            let core = word.trim_end_matches(['.', ',', ':', ';']);
            if is_ip(core) {
                out.push_str("‹ip›");
                out.push_str(&word[core.len()..]);
            } else if is_identifier(core) {
                let tail = &word[core.len()..];
                out.push_str(&self.alias_of(core));
                out.push_str(tail);
            } else {
                out.push_str(word);
            }
            word.clear();
        };
        for c in s.chars() {
            if c.is_whitespace() || "\"'()<>[]{}|=,".contains(c) {
                flush(&mut word, &mut out);
                out.push(c);
            } else {
                word.push(c);
            }
        }
        flush(&mut word, &mut out);
        out
    }

    /// An IP address (hidden in privacy mode).
    pub fn ip(&self, s: &str) -> String {
        if self.settings.privacy && !s.is_empty() && s != "-" {
            "hidden".into()
        } else {
            s.to_string()
        }
    }

    // ------------------------------------------------------------ key names

    /// Distinguishable API-key name: `#3 ci-bot xyz…` / `#1 xyz…`; in
    /// privacy mode `key #3 ci-bot` / `key #1` — never any part of the key.
    pub fn key_name(&self, id: &str, label: &str) -> String {
        let item = self
            .data
            .api_keys
            .get()
            .and_then(|k| k.items.iter().find(|i| i.id == id));
        if self.settings.privacy {
            let alias = item.map(|i| i.key_alias.as_str()).unwrap_or("");
            return private_key_name(id, alias);
        }
        match item {
            Some(i) => key_item_name(i),
            None if id.is_empty() => crate::fmt::redact_key(label),
            None => {
                let l = crate::fmt::redact_key(label);
                if l.is_empty() {
                    format!("#{id}")
                } else {
                    format!("#{id} {l}")
                }
            }
        }
    }

    /// Name for a key known only by its server label (request events).
    pub fn key_name_by_label(&self, label: &str) -> String {
        if let Some(keys) = self.data.api_keys.get() {
            let mut m = keys
                .items
                .iter()
                .filter(|i| i.label == label || (!i.key_alias.is_empty() && i.key_alias == label));
            if let (Some(i), None) = (m.next(), m.next()) {
                return self.key_name(&i.id, &i.label);
            }
        }
        if self.settings.privacy {
            "key".into()
        } else {
            crate::fmt::redact_key(label)
        }
    }

    /// Name of the session's own key (API-key role).
    pub fn session_key_name(&self) -> String {
        let Some(k) = self.session.as_ref().and_then(|s| s.api_key.as_ref()) else {
            return String::new();
        };
        let alias = k.alias.as_deref().unwrap_or("");
        if self.settings.privacy {
            return private_key_name(&k.id, alias);
        }
        let prefix = crate::fmt::redact_key(&k.display_key);
        if alias.is_empty() {
            format!("#{} {prefix}", k.id)
        } else {
            format!("#{} {alias} {prefix}", k.id)
        }
    }
}

/// Whether a string is (or contains) an e-mail address or an API key, in
/// full or masked form.
pub fn is_identifier(s: &str) -> bool {
    let s = s.trim();
    if s.contains('@') || s.contains('*') || s.contains('…') {
        return true;
    }
    // Long opaque tokens without separators look like keys.
    s.len() >= 20
        && !s.contains(' ')
        && !s.contains('/')
        && s.chars().any(|c| c.is_ascii_digit())
        && s.chars().any(|c| c.is_ascii_uppercase())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Whether a word is an IP address (optionally `v4:port`).
pub fn is_ip(s: &str) -> bool {
    use std::net::IpAddr;
    if s.parse::<IpAddr>().is_ok() {
        return true;
    }
    match s.rsplit_once(':') {
        Some((host, port)) => {
            port.chars().all(|c| c.is_ascii_digit()) && host.parse::<std::net::Ipv4Addr>().is_ok()
        }
        None => false,
    }
}

/// Privacy-mode key name: the id, plus the alias when it is not itself an
/// identifier.
fn private_key_name(id: &str, alias: &str) -> String {
    let alias = alias.trim();
    let id = if id.is_empty() { "?" } else { id };
    if alias.is_empty() || is_identifier(alias) {
        format!("key #{id}")
    } else {
        format!("key #{id} {alias}")
    }
}

fn key_item_name(i: &ApiKeyItem) -> String {
    if i.source_type == "plugin" {
        let p = i.access_provider.trim_start_matches("plugin:");
        return format!("#{} plugin {p}", i.id).trim_end().to_string();
    }
    let prefix = crate::fmt::redact_key(&i.display_key);
    if i.key_alias.is_empty() {
        format!("#{} {prefix}", i.id)
    } else {
        format!("#{} {} {prefix}", i.id, i.key_alias)
    }
}

#[cfg(test)]
mod privacy_tests {
    use super::*;

    fn app() -> App {
        let settings = Settings {
            privacy: true,
            ..Default::default()
        };
        App::new(settings, vec![], &UiState::default())
    }

    #[test]
    fn free_text_loses_emails_keys_and_ips() {
        let a = app();
        let s = a.privacy_text(
            "read tcp [2406:b400:d13::74d]:37854->[2607:6bc0::10]:443: for bob@x.io, key sk-***abc 10.0.0.7:80 down",
        );
        assert!(!s.contains("2406") && !s.contains("2607"), "{s}");
        assert!(
            !s.contains('@') && !s.contains("***") && !s.contains("10.0.0.7"),
            "{s}"
        );
        assert!(s.contains("‹ip›") && s.contains("account-"), "{s}");
        // Model names and versions are not identifiers.
        assert_eq!(
            a.privacy_text("claude-opus-4-1-20250805 v2.1.0"),
            "claude-opus-4-1-20250805 v2.1.0"
        );
        // Stable aliases.
        assert_eq!(a.privacy("a@b.c"), a.privacy("a@b.c"));
        assert_ne!(a.privacy("a@b.c"), a.privacy("d@e.f"));
        assert_eq!(a.ip("127.0.0.1"), "hidden");
    }
}
