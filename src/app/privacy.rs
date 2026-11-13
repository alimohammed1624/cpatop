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

    /// Free text (error bodies, user agents, URLs, status messages): in
    /// privacy mode every IP address becomes `‹ip›` and every e-mail-like or
    /// key-like word an `account-N` label.
    pub fn privacy_text(&self, s: &str) -> String {
        if !self.settings.privacy {
            return s.to_string();
        }
        let s = mask_ips(s);
        let mut out = String::with_capacity(s.len());
        let mut word = String::new();
        let flush = |word: &mut String, out: &mut String| {
            if word.is_empty() {
                return;
            }
            let core = word.trim_end_matches(['.', ',', ':', ';', '!']);
            // IPs were masked above, in context; words are checked for
            // e-mails and keys only.
            let c = core.trim();
            if c.contains('@') || c.contains('*') || c.contains('…') || looks_like_key(c) {
                let tail = &word[core.len()..];
                out.push_str(&self.alias_of(core));
                out.push_str(tail);
            } else {
                out.push_str(word);
            }
            word.clear();
        };
        for c in s.chars() {
            if c.is_whitespace() || TEXT_SEPARATORS.contains(c) {
                flush(&mut word, &mut out);
                out.push(c);
            } else {
                word.push(c);
            }
        }
        flush(&mut word, &mut out);
        out
    }

    /// An IP address field (`‹ip›` in privacy mode).
    pub fn ip(&self, s: &str) -> String {
        if self.settings.privacy && !s.is_empty() && s != "-" {
            IP_MASK.into()
        } else {
            s.to_string()
        }
    }

    /// The server's host (and sub-path) for display; IP literals are masked
    /// in privacy mode, host names such as `localhost` are kept.
    pub fn host_label(&self) -> String {
        if self.settings.privacy {
            mask_ips(&self.host)
        } else {
            self.host.clone()
        }
    }

    /// The base URL for display (login and offline screens).
    pub fn base_url_label(&self) -> String {
        self.privacy_text(&self.settings.base_url)
    }

    // ------------------------------------------------------------ credentials

    /// The credential behind a key from a usage endpoint: the identity
    /// (`auth_index`), its masked form (`6d7*********de98aa`) or the id.
    pub fn identity_for(&self, key: &str) -> Option<&Identity> {
        let ids = &self.data.identities.get()?.identities;
        if let Some(i) = ids.iter().find(|i| i.identity == key || i.id == key) {
            return Some(i);
        }
        let (first, last) = (key.find('*')?, key.rfind('*')?);
        let (pre, suf) = (&key[..first], &key[last + 1..]);
        if pre.is_empty() || suf.is_empty() {
            return None;
        }
        let mut m = ids
            .iter()
            .filter(|i| i.identity.starts_with(pre) && i.identity.ends_with(suf));
        match (m.next(), m.next()) {
            (Some(i), None) => Some(i),
            _ => None,
        }
    }

    /// A credential's display name, told apart from credentials that share
    /// it: the same account on several providers gets ` · claude`,
    /// ` · codex`…, and `#id` when even that is not unique.
    pub fn ident_label(&self, i: &Identity) -> String {
        let base = raw_ident_name(i);
        let name = self.privacy(base);
        let Some(page) = self.data.identities.get() else {
            return name;
        };
        let same: Vec<&Identity> = page
            .identities
            .iter()
            .filter(|o| raw_ident_name(o) == base)
            .collect();
        if same.len() <= 1 {
            return name;
        }
        let kind = ident_kind(i);
        if same.iter().filter(|o| ident_kind(o) == kind).count() > 1 {
            format!("{name} · {kind} #{}", i.id)
        } else {
            format!("{name} · {kind}")
        }
    }

    /// Name for a credential known by a usage key and server label.
    pub fn credential_name(&self, key: &str, label: &str) -> String {
        match self.identity_for(key) {
            Some(i) => self.ident_label(i),
            None => self.privacy(label),
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

/// The name a credential is shown under (before privacy).
pub fn raw_ident_name(i: &Identity) -> &str {
    if !i.display_name.is_empty() {
        &i.display_name
    } else if !i.name.is_empty() {
        &i.name
    } else {
        &i.identity
    }
}

/// Provider type of a credential, marking API-key credentials.
pub fn ident_kind(i: &Identity) -> String {
    if i.auth_type == 2 {
        format!("{} (api)", i.kind)
    } else {
        i.kind.clone()
    }
}

/// What an IP address is replaced with in privacy mode.
pub const IP_MASK: &str = "‹ip›";

/// Characters that end a word in free text. `/`, `?`, `&`, `#` and `:` split
/// URLs and `key=value` pairs so every part is checked on its own.
const TEXT_SEPARATORS: &str = "\"'`()<>[]{}|=,;/?&#:";

/// Whether a string is (or contains) an e-mail address, an API key (in full
/// or masked form, including opaque tokens such as `sk-…`) or an IP address.
pub fn is_identifier(s: &str) -> bool {
    let s = s.trim();
    if s.contains('@') || s.contains('*') || s.contains('…') {
        return true;
    }
    contains_ip(s) || looks_like_key(s)
}

/// Prefixes of well-known secret formats (compared case-insensitively).
const KEY_PREFIXES: [&str; 14] = [
    "sk-",
    "sk_",
    "pk-",
    "pk_",
    "rk_",
    "ak-",
    "cpa-",
    "key-",
    "ghp_",
    "gho_",
    "github_pat_",
    "xoxb-",
    "xoxp-",
    "aiza",
];

/// Whether a word looks like a secret token: a known key prefix followed by
/// at least eight characters, or a long opaque segment (16+ characters mixing
/// letters and digits, or 20+ mixing upper and lower case). Model names and
/// versions are split into short segments by `-`, `.` and `_`, so they pass.
pub fn looks_like_key(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    let token_char = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    if KEY_PREFIXES
        .iter()
        .any(|p| lower.starts_with(p) && s.len() >= p.len() + 8 && s.chars().all(token_char))
    {
        return true;
    }
    let opaque = s.split(|c: char| !c.is_ascii_alphanumeric()).any(|seg| {
        let digits = seg.chars().any(|c| c.is_ascii_digit());
        let letters = seg.chars().any(|c| c.is_ascii_alphabetic());
        let upper = seg.chars().any(|c| c.is_ascii_uppercase());
        let lower = seg.chars().any(|c| c.is_ascii_lowercase());
        (seg.len() >= 16 && digits && letters) || (seg.len() >= 20 && upper && lower)
    });
    if opaque {
        return true;
    }
    // Long tokens without separators other than `-`/`_`, mixing case and digits.
    s.len() >= 20
        && s.chars().all(token_char)
        && s.chars().any(|c| c.is_ascii_digit())
        && s.chars().any(|c| c.is_ascii_uppercase())
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

/// Whether `s` contains an IP address anywhere (URLs, `host:port`, `[v6]`).
pub fn contains_ip(s: &str) -> bool {
    !ip_spans(s).is_empty()
}

/// Replace every IP address inside `s` with [`IP_MASK`].
pub fn mask_ips(s: &str) -> String {
    let spans = ip_spans(s);
    if spans.is_empty() {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut at = 0;
    for (a, b) in spans {
        out.push_str(&s[at..a]);
        out.push_str(IP_MASK);
        at = b;
    }
    out.push_str(&s[at..]);
    out
}

/// Byte ranges of the IP addresses in `s`, in order and non-overlapping.
fn ip_spans(s: &str) -> Vec<(usize, usize)> {
    use std::net::{Ipv4Addr, Ipv6Addr};
    let b = s.as_bytes();
    let hexish = |c: u8| c.is_ascii_hexdigit() || c == b':' || c == b'.';
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if !hexish(b[i]) {
            i += 1;
            continue;
        }
        // A run of hex digits, `:` and `.`: try IPv6 first (longest match
        // starting at a group boundary), then dotted IPv4 inside the run.
        let mut j = i;
        while j < b.len() && hexish(b[j]) {
            j += 1;
        }
        let run = &s[i..j];
        let mut found: Option<(usize, usize)> = None;
        if run.matches(':').count() >= 2 {
            'outer: for a in 0..run.len() {
                if a > 0 && run.as_bytes()[a - 1] != b':' && run.as_bytes()[a] != b':' {
                    continue;
                }
                for e in (a + 2..=run.len()).rev() {
                    let cand = &run[a..e];
                    let groups = cand.split(':').filter(|g| !g.is_empty()).count();
                    if (groups >= 2 || cand == "::1")
                        && cand.contains(':')
                        && cand.parse::<Ipv6Addr>().is_ok()
                    {
                        found = Some((i + a, i + e));
                        break 'outer;
                    }
                }
            }
        }
        if let Some(f) = found {
            out.push(f);
            i = f.1;
            continue;
        }
        // IPv4: four dotted decimal groups not glued to other digits, and
        // not a product version (`Chrome/142.0.0.0`: `name/` before it).
        // Product tokens are capitalised words (`Chrome/`, `Safari/`), so
        // `proxy/10.0.0.5` is still masked.
        let version = |k: usize| {
            if k < 2 || b[k - 1] != b'/' {
                return false;
            }
            let mut w = k - 1;
            while w > 0 && b[w - 1].is_ascii_alphabetic() {
                w -= 1;
            }
            w < k - 1 && b[w].is_ascii_uppercase()
        };
        let mut k = i;
        let mut v4 = None;
        while k < j {
            if b[k].is_ascii_digit()
                && (k == 0 || !(b[k - 1].is_ascii_digit() || b[k - 1] == b'.'))
                && !version(k)
            {
                let mut e = k;
                while e < b.len() && (b[e].is_ascii_digit() || b[e] == b'.') {
                    e += 1;
                }
                let mut cand = &s[k..e];
                cand = cand.trim_end_matches('.');
                if cand.parse::<Ipv4Addr>().is_ok() {
                    v4 = Some((k, k + cand.len()));
                    break;
                }
            }
            k += 1;
        }
        match v4 {
            Some(f) => {
                out.push(f);
                i = f.1;
            }
            None => i = j.max(i + 1),
        }
    }
    out
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
        assert_eq!(a.ip("127.0.0.1"), "‹ip›");
    }

    /// Adversarial inputs: (input, fragments that must not survive).
    const LEAKS: &[(&str, &[&str])] = &[
        ("http://10.0.0.5:8080/x", &["10.0.0.5"]),
        ("https://192.168.1.20/keeper", &["192.168"]),
        ("via proxy/10.9.8.7 failed", &["10.9.8.7"]),
        ("http://[::1]:8318/api", &["::1"]),
        ("[fe80::1%eth0]:443", &["fe80"]),
        (
            "dial tcp 127.0.0.1:8318: connection refused",
            &["127.0.0.1"],
        ),
        ("upstream=172.16.4.2,via=10.1.1.1", &["172.16", "10.1.1.1"]),
        (
            "xff: 203.0.113.9, 198.51.100.7",
            &["203.0.113.9", "198.51.100.7"],
        ),
        (
            "read tcp [2406:b400:d13::74d]:37854->[2607:6bc0::10]:443",
            &["2406", "2607"],
        ),
        ("ip 2001:db8:0:0:0:0:0:1 down", &["2001:db8"]),
        (
            "key sk-abcdefghijklmnop1234 rejected",
            &["abcdefghijklmnop"],
        ),
        (
            "Authorization: Bearer sk-proj-abcdef0123456789",
            &["abcdef0123456789"],
        ),
        ("token=ghp_0123456789abcdefABCDEF", &["0123456789abcdef"]),
        (
            "api_key=cpa-k3y-v4lue-9f8e7d6c5b4a3f2e",
            &["9f8e7d6c5b4a3f2e"],
        ),
        ("opaque 9f8e7d6c5b4a3f2e1d0c tok", &["9f8e7d6c5b4a3f2e1d0c"]),
        ("masked abc*********defghi key", &["abc*", "defghi"]),
        ("mailto:alice@example.org", &["alice", "example.org"]),
        ("<bob.smith@corp.example>", &["bob.smith", "corp.example"]),
        ("\"carol@x.io\"", &["carol"]),
        ("account dave@x.io: limit", &["dave"]),
    ];

    #[test]
    fn adversarial_free_text_is_scrubbed() {
        let a = app();
        for (input, bad) in LEAKS {
            let out = a.privacy_text(input);
            for frag in *bad {
                assert!(!out.contains(frag), "{input:?} -> {out:?} leaks {frag:?}");
            }
        }
    }

    #[test]
    fn harmless_text_is_kept() {
        let a = app();
        for s in [
            "claude-opus-4-1-20250805",
            "gpt-6.1-sol",
            "gemini-2.5-pro-preview-06-05",
            "context canceled",
            "HTTP 503 credential validation failed",
            "Post \"https://api.anthropic.com/v1/messages?beta=true\": EOF",
            "localhost:8318",
            "v2.1.0 at 18:00:25",
            "2026-11-13T18:00:25+05:30",
            "claude-cli/2.1.280 (external, cli)",
            "Mozilla/5.0 AppleWebKit/537.36 Chrome/142.0.0.0 Safari/537.36",
        ] {
            assert_eq!(a.privacy_text(s), s, "{s:?} changed");
        }
    }

    #[test]
    fn hosts_and_names_with_ips_are_hidden() {
        let mut a = app();
        a.host = "10.0.0.5:8318/keeper".into();
        assert_eq!(a.host_label(), "‹ip›:8318/keeper");
        a.host = "[::1]:8318".into();
        assert_eq!(a.host_label(), "[‹ip›]:8318");
        a.host = "localhost:8318".into();
        assert_eq!(a.host_label(), "localhost:8318");
        a.settings.base_url = "http://192.168.0.4:8318".into();
        assert!(
            !a.base_url_label().contains("192"),
            "{}",
            a.base_url_label()
        );
        // An alias that is an IP or a key is not shown as a name.
        assert!(a.privacy("10.2.3.4").starts_with("account-"));
        assert!(a.privacy("sk-abcdefghij0123456789").starts_with("account-"));
        assert_eq!(private_key_name("4", "192.168.1.1"), "key #4");
        assert_eq!(private_key_name("4", "ci-bot"), "key #4 ci-bot");
        // Without privacy nothing changes.
        a.settings.privacy = false;
        assert_eq!(a.host_label(), "localhost:8318");
    }
}
