//! Privacy mode and API-key naming.
//!
//! Privacy mode (`--privacy`, `x`) shows no part of any e-mail address, API
//! key or IP address. It works in two layers:
//!
//! 1. **Structured fields are replaced, not scrubbed.** Credential and
//!    account names, sources, key aliases and masked keys, endpoints, user
//!    agents and request ids never reach a pane as data: panes go through
//!    [`App::account`], [`App::credential_name`], [`App::key_name`],
//!    [`App::ip`], [`App::user_agent`], [`App::endpoint`] and
//!    [`App::opaque_id`], which return stable labels (`account-17`,
//!    `key #3`, `‹ip›`) instead. Free text (server errors, messages) goes
//!    through [`App::privacy_text`], which masks every e-mail, IP address
//!    and key-like token found by [`sensitive_spans`].
//! 2. **The rendering boundary is guarded.** After every frame,
//!    [`guard_buffer`] runs the same detector over every row of the
//!    terminal buffer and overwrites whatever it finds. Nothing a pane
//!    draws — including text a future pane forgets to sanitize — reaches
//!    the screen without passing through it.
//!
//! Account labels are derived from the server's credential ids, so the
//! same account has the same label in every run; names that are not a
//! known credential get a short keyed hash (`account-qzv`) whose key is
//! kept in the UI state file.

use super::*;
use ratatui::buffer::Buffer;

impl App {
    // ------------------------------------------------------------ accounts

    /// Stable label for an account name (e-mail, file name, auth index…):
    /// `account-<smallest credential id with that name>`, or a keyed hash
    /// for names the credential list does not know.
    fn alias_of(&self, s: &str) -> String {
        let key = s.trim().to_lowercase();
        if let Some(n) = self.account_ids().get(&key) {
            return format!("account-{n}");
        }
        format!("account-{}", keyed_tag(self.privacy_salt, &key))
    }

    /// Lower-cased account names → smallest credential id, cached per
    /// credential list.
    fn account_ids(&self) -> std::cell::Ref<'_, HashMap<String, u64>> {
        let at = self.data.identities.at;
        let fresh = self.privacy_aliases.borrow().0 == at;
        if !fresh {
            let mut m: HashMap<String, u64> = HashMap::new();
            if let Some(page) = self.data.identities.get() {
                // Every name a credential is known under maps to the same
                // number: the one of its display name.
                let mut by_name: HashMap<String, u64> = HashMap::new();
                for i in &page.identities {
                    let Ok(id) = i.id.parse::<u64>() else {
                        continue;
                    };
                    let e = by_name
                        .entry(raw_ident_name(i).to_lowercase())
                        .or_insert(id);
                    *e = (*e).min(id);
                }
                for i in &page.identities {
                    let Some(&n) = by_name.get(&raw_ident_name(i).to_lowercase()) else {
                        continue;
                    };
                    let names = [
                        Some(i.display_name.as_str()),
                        Some(i.name.as_str()),
                        Some(i.identity.as_str()),
                        i.alias.as_deref(),
                        i.file_name.as_deref(),
                    ];
                    for nm in names.into_iter().flatten() {
                        let nm = nm.trim().to_lowercase();
                        if !nm.is_empty() {
                            let e = m.entry(nm).or_insert(n);
                            *e = (*e).min(n);
                        }
                    }
                }
            }
            *self.privacy_aliases.borrow_mut() = (at, m);
        }
        std::cell::Ref::map(self.privacy_aliases.borrow(), |c| &c.1)
    }

    /// An account or credential name (e-mail, file name, source, alias).
    /// In privacy mode it is always replaced by its stable label.
    pub fn account(&self, s: &str) -> String {
        if self.settings.privacy {
            self.alias_of(s)
        } else {
            s.to_string()
        }
    }

    /// A label that is normally not an identifier (model, provider). In
    /// privacy mode it is scrubbed like free text.
    pub fn privacy(&self, s: &str) -> String {
        self.privacy_text(s)
    }

    /// Free text (error bodies, messages): in privacy mode every IP address
    /// becomes `‹ip›`, every e-mail address the account label (or
    /// `‹email›`) and every key-like token `‹key›`.
    pub fn privacy_text(&self, s: &str) -> String {
        if !self.settings.privacy {
            return s.to_string();
        }
        replace_spans(s, |kind, text| match kind {
            Sensitive::Email if self.account_ids().contains_key(&text.to_lowercase()) => {
                self.alias_of(text)
            }
            k => k.placeholder().to_string(),
        })
    }

    /// An IP address field (`‹ip›` in privacy mode).
    pub fn ip(&self, s: &str) -> String {
        if self.settings.privacy && !s.is_empty() && s != "-" {
            IP_MASK.into()
        } else {
            s.to_string()
        }
    }

    /// A user agent. In privacy mode only the leading product token is kept
    /// (`claude-cli/2.1.3`): comments and further tokens can carry host
    /// names, accounts or ids.
    pub fn user_agent(&self, s: &str) -> String {
        if !self.settings.privacy || s.is_empty() || s == "-" {
            return s.to_string();
        }
        let first = s.split_whitespace().next().unwrap_or("");
        let (name, ver) = first.split_once('/').unwrap_or((first, ""));
        let name_ok = !name.is_empty()
            && name.len() <= 24
            && name
                .chars()
                .all(|c| c.is_ascii_alphabetic() || c == '-' || c == '_' || c == '.')
            && !is_sensitive(name);
        let ver_ok = ver.len() <= 16
            && ver.chars().all(|c| c.is_ascii_digit() || c == '.')
            && ver.matches('.').count() <= 2;
        let rest = if s.trim() != first { " …" } else { "" };
        match (name_ok, ver_ok && !ver.is_empty()) {
            (true, true) => format!("{name}/{ver}{rest}"),
            (true, false) => format!("{name}{rest}"),
            _ => HIDDEN.into(),
        }
    }

    /// A request endpoint (`POST /v1/messages`). In privacy mode only
    /// well-known API routes are shown; anything else is hidden.
    pub fn endpoint(&self, s: &str) -> String {
        if !self.settings.privacy || s.is_empty() || s == "-" {
            return s.to_string();
        }
        let (method, path) = match s.split_once(' ') {
            Some((m, p)) if m.chars().all(|c| c.is_ascii_uppercase()) => (Some(m), p.trim()),
            _ => (None, s.trim()),
        };
        if !known_route(path) {
            return HIDDEN.into();
        }
        match method {
            Some(m) => format!("{m} {path}"),
            None => path.to_string(),
        }
    }

    /// An opaque id (request id): hidden in privacy mode.
    pub fn opaque_id(&self, s: &str) -> String {
        if self.settings.privacy && !s.is_empty() && s != "-" {
            HIDDEN.into()
        } else {
            s.to_string()
        }
    }

    /// The server's host and port for display. In privacy mode the path is
    /// dropped and IP literals become `‹ip›`.
    pub fn host_label(&self) -> String {
        if self.settings.privacy {
            private_origin(&self.settings.base_url, false)
        } else {
            self.host.clone()
        }
    }

    /// The base URL for display (login and offline screens). In privacy
    /// mode only scheme, host (IP literals masked) and port.
    pub fn base_url_label(&self) -> String {
        if self.settings.privacy {
            private_origin(&self.settings.base_url, true)
        } else {
            self.settings.base_url.clone()
        }
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
        let name = self.account(base);
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
            None if label.is_empty() || label == "-" => label.to_string(),
            None => self.account(label),
        }
    }

    // ------------------------------------------------------------ key names

    /// Distinguishable API-key name: `#3 ci-bot xyz…` / `#1 xyz…`; in
    /// privacy mode `key #3` — no alias, never any part of the key.
    pub fn key_name(&self, id: &str, label: &str) -> String {
        if self.settings.privacy {
            return private_key_name(id);
        }
        let item = self
            .data
            .api_keys
            .get()
            .and_then(|k| k.items.iter().find(|i| i.id == id));
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
        if self.settings.privacy {
            return private_key_name(&k.id);
        }
        let alias = k.alias.as_deref().unwrap_or("");
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
/// What a hidden structured field is replaced with.
pub const HIDDEN: &str = "‹hidden›";

/// A kind of identifier found in text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sensitive {
    Ip,
    Email,
    Key,
}

impl Sensitive {
    pub fn placeholder(self) -> &'static str {
        match self {
            Sensitive::Ip => IP_MASK,
            Sensitive::Email => "‹email›",
            Sensitive::Key => "‹key›",
        }
    }
}

/// Short keyed tag (three letters) for a name the credential list does not
/// know. The key is per installation, so the tag is stable across runs but
/// cannot be checked against a guessed e-mail address.
fn keyed_tag(salt: u64, s: &str) -> String {
    // FNV-1a over the salt and the name, then a final avalanche.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in salt.to_le_bytes().iter().chain(s.as_bytes()) {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    (0..3)
        .map(|i| (b'a' + ((h >> (i * 8)) % 26) as u8) as char)
        .collect()
}

/// `scheme://host:port` (or `host:port`) with IP literals masked and no
/// user info or path.
fn private_origin(base: &str, scheme: bool) -> String {
    let Ok(u) = url::Url::parse(base) else {
        return HIDDEN.into();
    };
    let host = match u.host() {
        Some(url::Host::Domain(d)) if !is_sensitive(d) => d.to_string(),
        Some(url::Host::Domain(_)) => HIDDEN.into(),
        Some(_) => IP_MASK.into(),
        None => HIDDEN.into(),
    };
    let port = u.port().map(|p| format!(":{p}")).unwrap_or_default();
    if scheme {
        format!("{}://{host}{port}", u.scheme())
    } else {
        format!("{host}{port}")
    }
}

/// Well-known model API routes (shown in privacy mode).
fn known_route(path: &str) -> bool {
    const ROUTES: [&str; 12] = [
        "/v1/messages",
        "/v1/messages/count_tokens",
        "/v1/chat/completions",
        "/v1/completions",
        "/v1/responses",
        "/v1/responses/compact",
        "/v1/embeddings",
        "/v1/models",
        "/backend-api/codex/responses",
        "/api/provider/openai/v1/chat/completions",
        "/api/provider/anthropic/v1/messages",
        "/v1/images/generations",
    ];
    let path = path.split('?').next().unwrap_or("");
    if ROUTES.contains(&path) {
        return true;
    }
    // Gemini: /v1beta/models/<model>:<method>
    for pre in ["/v1beta/models/", "/v1/models/"] {
        if let Some(rest) = path.strip_prefix(pre) {
            let (model, method) = rest.split_once(':').unwrap_or((rest, ""));
            return !model.is_empty()
                && model.len() <= 48
                && model.chars().all(|c| {
                    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '.')
                })
                && model_shaped(model)
                && method.len() <= 32
                && method.chars().all(|c| c.is_ascii_alphabetic());
        }
    }
    false
}

/// Privacy-mode key name: the database id only (aliases are names people
/// choose and can identify them).
fn private_key_name(id: &str) -> String {
    let id = if id.is_empty() { "?" } else { id };
    format!("key #{id}")
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

// ---------------------------------------------------------------- detection

/// Whether a string is (or contains) an e-mail address, an API key (in full
/// or masked form, including opaque tokens) or an IP address.
pub fn is_identifier(s: &str) -> bool {
    is_sensitive(s)
}

/// Whether [`sensitive_spans`] finds anything in `s`.
pub fn is_sensitive(s: &str) -> bool {
    !sensitive_spans(s).is_empty()
}

/// Whether a word looks like a secret token (see [`sensitive_spans`]).
pub fn looks_like_key(s: &str) -> bool {
    sensitive_spans(s).iter().any(|x| x.2 == Sensitive::Key)
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
    let mut spans: Vec<(usize, usize, Sensitive)> = ip_spans(s)
        .into_iter()
        .map(|(a, b)| (a, b, Sensitive::Ip))
        .collect();
    spans.sort();
    splice(s, &spans, |_, _| IP_MASK.to_string())
}

/// Replace every sensitive span with its placeholder.
pub fn scrub(s: &str) -> String {
    replace_spans(s, |k, _| k.placeholder().to_string())
}

fn replace_spans(s: &str, f: impl Fn(Sensitive, &str) -> String) -> String {
    let spans = sensitive_spans(s);
    splice(s, &spans, f)
}

fn splice(
    s: &str,
    spans: &[(usize, usize, Sensitive)],
    f: impl Fn(Sensitive, &str) -> String,
) -> String {
    if spans.is_empty() {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut at = 0;
    for &(a, b, k) in spans {
        out.push_str(&s[at..a]);
        out.push_str(&f(k, &s[a..b]));
        at = b;
    }
    out.push_str(&s[at..]);
    out
}

/// Prefixes of well-known secret formats (compared case-insensitively).
const KEY_PREFIXES: [&str; 16] = [
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
    "ghs_",
    "github_pat_",
    "xoxb-",
    "xoxp-",
    "aiza",
    "ya29.",
];

/// Characters of an opaque token.
fn token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')
}

/// Characters that can be part of an e-mail address (or its truncation).
fn email_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-' | '\'' | '…' | '*')
}

/// Whether a long token reads like a model name or version rather than a
/// secret: every `-`/`.`/`_` segment is a number (≤ 8 digits), a short mix
/// of letters and digits (≤ 8: `4o`, `480B`, `A35B`) or a word (≤ 20
/// letters; from 12 letters on it must read as words — `generateContent`,
/// `DeepSeekCoder` — not random case like `XpEldHTvjaRE`).
fn model_shaped(tok: &str) -> bool {
    tok.split(['-', '.', '_']).all(|seg| {
        let n = seg.len();
        let digits = seg.chars().filter(|c| c.is_ascii_digit()).count();
        if digits > 0 {
            return n <= 8;
        }
        n <= 20 && (n < 12 || word_shaped(seg))
    })
}

/// Letters that split into words `[A-Z]*[a-z]*` each with at least two
/// lower-case letters or an acronym of at most five capitals.
fn word_shaped(seg: &str) -> bool {
    let b = seg.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let mut up = 0;
        while i < b.len() && b[i].is_ascii_uppercase() {
            up += 1;
            i += 1;
        }
        let mut low = 0;
        while i < b.len() && b[i].is_ascii_lowercase() {
            low += 1;
            i += 1;
        }
        let ok = if low == 0 {
            up <= 5
        } else {
            low >= 2 && up <= 5
        };
        if !ok || (up == 0 && low == 0) {
            return false;
        }
    }
    true
}

/// Byte ranges of every identifier in `s`, sorted and non-overlapping:
///
/// - IPv4 and IPv6 addresses anywhere, in any case and context (URLs,
///   `host:port`, `[v6]`, `proxy/10.0.0.1`, versions that are valid
///   addresses), and dotted/coloned fragments cut by an ellipsis;
/// - e-mail addresses (anything around an `@`), including in URL paths;
/// - masked keys (`abc***def`), keys with a known prefix (`sk-…`), and
///   any token of 20+ `[A-Za-z0-9_.-]` characters that is not shaped like
///   a model name or version — over-masking is preferred to leaking.
pub fn sensitive_spans(s: &str) -> Vec<(usize, usize, Sensitive)> {
    let mut spans: Vec<(usize, usize, Sensitive)> = ip_spans(s)
        .into_iter()
        .map(|(a, b)| (a, b, Sensitive::Ip))
        .collect();
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let end_of = |i: usize| chars.get(i).map(|c| c.0).unwrap_or(s.len());
    // E-mail addresses (and `@handles`): expand around each `@`.
    for (i, &(_, c)) in chars.iter().enumerate() {
        if c != '@' {
            continue;
        }
        let mut a = i;
        while a > 0 && email_char(chars[a - 1].1) {
            a -= 1;
        }
        let mut b = i + 1;
        while b < chars.len() && email_char(chars[b].1) {
            b += 1;
        }
        if b - a > 1 {
            spans.push((end_of(a), end_of(b), Sensitive::Email));
        }
    }
    // Masked keys: runs of `*` inside a token.
    for (i, w) in chars.windows(2).enumerate() {
        if w[0].1 == '*' && w[1].1 == '*' {
            let mut a = i;
            while a > 0 && (token_char(chars[a - 1].1) || matches!(chars[a - 1].1, '*' | '…')) {
                a -= 1;
            }
            let mut b = i + 2;
            while b < chars.len() && (token_char(chars[b].1) || matches!(chars[b].1, '*' | '…')) {
                b += 1;
            }
            spans.push((end_of(a), end_of(b), Sensitive::Key));
        }
    }
    // Tokens: known key prefixes and long opaque strings.
    let mut i = 0;
    while i < chars.len() {
        if !token_char(chars[i].1) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() && token_char(chars[j].1) {
            j += 1;
        }
        let (a, b) = (end_of(i), end_of(j));
        let tok = s[a..b].trim_end_matches('.');
        let lower = tok.to_ascii_lowercase();
        let cut = chars.get(j).is_some_and(|c| matches!(c.1, '…' | '*'));
        let prefixed = KEY_PREFIXES.iter().any(|p| {
            let starts =
                lower.starts_with(p) && (i == 0 || !chars[i - 1].1.is_ascii_alphanumeric());
            let rest = &tok[p.len().min(tok.len())..];
            starts
                && ((rest.len() >= 8
                    && (rest
                        .chars()
                        .any(|c| c.is_ascii_digit() || c.is_ascii_uppercase())
                        || rest.len() >= 16))
                    || (cut && !rest.is_empty()))
        });
        if prefixed || (tok.len() >= 20 && !model_shaped(tok)) {
            spans.push((a, a + tok.len(), Sensitive::Key));
        }
        i = j;
    }
    normalize(spans)
}

/// Sort and merge overlapping spans (the earlier kind wins).
fn normalize(mut spans: Vec<(usize, usize, Sensitive)>) -> Vec<(usize, usize, Sensitive)> {
    spans.sort_by_key(|x| (x.0, std::cmp::Reverse(x.1)));
    let mut out: Vec<(usize, usize, Sensitive)> = Vec::new();
    for s in spans {
        match out.last_mut() {
            Some(l) if s.0 < l.1 => l.1 = l.1.max(s.1),
            _ => out.push(s),
        }
    }
    out
}

/// Byte ranges of the IP addresses in `s`, in order and non-overlapping.
fn ip_spans(s: &str) -> Vec<(usize, usize)> {
    use std::net::Ipv6Addr;
    let b = s.as_bytes();
    let hexish = |c: u8| c.is_ascii_hexdigit() || c == b':' || c == b'.';
    // Whether an ellipsis (U+2026, `e2 80 a6`) ends right before `k` or
    // starts at `k`: a value cut by truncation.
    let ell_before = |k: usize| k >= 3 && &b[k - 3..k] == "…".as_bytes();
    let ell_at = |k: usize| b.len() >= k + 3 && &b[k..k + 3] == "…".as_bytes();
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if !hexish(b[i]) {
            i += 1;
            continue;
        }
        // A run of hex digits, `:` and `.`.
        let mut j = i;
        while j < b.len() && hexish(b[j]) {
            j += 1;
        }
        let run = &s[i..j];
        // IPv6: the longest candidate starting at a group boundary.
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
            // A cut-off address: `2406:b400:d1…` — hex groups with a letter
            // or `::`, next to an ellipsis.
            if found.is_none()
                && (ell_at(j) || ell_before(i))
                && (run.contains("::") || run.bytes().any(|c| c.is_ascii_alphabetic()))
            {
                found = Some((i, j));
            }
        }
        if let Some(f) = found {
            // Mask the whole run: an address glued to other hex digits
            // (`ab2001:db8::1`) must not leave a fragment behind.
            out.push((i, j));
            i = f.1.max(j);
            continue;
        }
        // IPv4: dotted decimal runs. Any four consecutive groups that are
        // each 0–255 mask the whole run (`1.10.0.0.1`, `Chrome/142.0.0.0`
        // included); three or more groups next to an ellipsis are a cut
        // address.
        let mut k = i;
        let mut hit = false;
        while k < j {
            if !b[k].is_ascii_digit() || (k > 0 && (b[k - 1].is_ascii_digit())) {
                k += 1;
                continue;
            }
            let mut e = k;
            while e < b.len() && (b[e].is_ascii_digit() || b[e] == b'.') {
                e += 1;
            }
            let num = s[k..e].trim_end_matches('.');
            let groups: Vec<&str> = num.split('.').collect();
            let octet =
                |g: &str| !g.is_empty() && g.len() <= 3 && g.parse::<u16>().is_ok_and(|v| v <= 255);
            let four = groups.windows(4).any(|w| w.iter().all(|g| octet(g)));
            let cut = groups.len() >= 3
                && groups.iter().all(|g| g.is_empty() || g.len() <= 3)
                && (ell_at(e) || ell_before(k));
            if four || cut {
                out.push((k, k + num.len()));
                hit = true;
                i = k + num.len();
                break;
            }
            k = e.max(k + 1);
        }
        if !hit {
            i = j.max(i + 1);
        }
    }
    out
}

// ---------------------------------------------------------------- the guard

/// Scan every row of a rendered frame and overwrite any e-mail, IP address
/// or key-like token (see [`sensitive_spans`]) with a placeholder of the
/// same width. Returns how many spans were masked. This is the last line
/// of defence in privacy mode: it runs on the final buffer, so no pane can
/// bypass it.
pub fn guard_buffer(buf: &mut Buffer) -> usize {
    let area = buf.area;
    let mut hits = 0;
    for y in area.top()..area.bottom() {
        // The row's text, with the column each character starts at.
        let mut text = String::new();
        let mut cols: Vec<(usize, u16)> = Vec::new();
        for x in area.left()..area.right() {
            let sym = buf[(x, y)].symbol();
            if sym.is_empty() {
                continue;
            }
            cols.push((text.len(), x));
            text.push_str(sym);
        }
        let spans = sensitive_spans(&text);
        if spans.is_empty() {
            continue;
        }
        for (a, b, kind) in spans {
            let first = cols.iter().find(|c| c.0 >= a).map(|c| c.1);
            let next = cols
                .iter()
                .find(|c| c.0 >= b)
                .map(|c| c.1)
                .unwrap_or(area.right());
            let Some(first) = first else { continue };
            let w = next.saturating_sub(first) as usize;
            let label: Vec<char> = kind.placeholder().chars().collect();
            for (n, x) in (first..next).enumerate() {
                let ch = if w >= label.len() {
                    label.get(n).copied().unwrap_or('·')
                } else {
                    '•'
                };
                buf[(x, y)].set_char(ch);
            }
            hits += 1;
        }
    }
    hits
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
        assert!(s.contains("‹ip›") && s.contains("‹email›"), "{s}");
        // Model names and versions are not identifiers.
        assert_eq!(
            a.privacy_text("claude-opus-4-1-20250805 v2.1.0"),
            "claude-opus-4-1-20250805 v2.1.0"
        );
        // Stable aliases.
        assert_eq!(a.account("a@b.c"), a.account("a@b.c"));
        assert_ne!(a.account("a@b.c"), a.account("d@e.f"));
        assert_eq!(a.ip("127.0.0.1"), "‹ip›");
    }

    /// Adversarial inputs: (input, fragments that must not survive).
    const LEAKS: &[(&str, &[&str])] = &[
        ("http://10.0.0.5:8080/x", &["10.0.0.5"]),
        ("https://192.168.1.20/keeper", &["192.168"]),
        ("via proxy/10.9.8.7 failed", &["10.9.8.7"]),
        ("via Proxy/10.20.30.40 failed", &["10.20.30.40"]),
        ("http://[::1]:8318/api", &["::1"]),
        ("[fe80::1%eth0]:443", &["fe80"]),
        ("[FE80::1]:443 and 2001:DB8::1", &["FE80", "DB8"]),
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
        ("cut 10.20.30.… here", &["10.20.30"]),
        ("cut 2406:b400:d1… here", &["2406"]),
        ("ver 1.10.0.0.1 x", &["10.0.0.1"]),
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
        (
            "lower abcdefghijklmnopqrstuvwxyzab tok",
            &["abcdefghijklmnop"],
        ),
        ("masked abc*********defghi key", &["abc*", "defghi"]),
        ("cut sk-abc… key", &["sk-abc"]),
        ("mailto:alice@example.org", &["alice", "example.org"]),
        (
            "http://localhost:1/alice@example.org",
            &["alice", "example.org"],
        ),
        ("<bob.smith@corp.example>", &["bob.smith", "corp.example"]),
        ("\"carol@x.io\"", &["carol"]),
        ("account dave@x.io: limit", &["dave"]),
        ("Dave.Smith@Example.ORG", &["Dave", "Example"]),
        ("cut eve@exam…", &["eve", "exam"]),
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
            "Qwen3-Coder-480B-A35B-Instruct",
            "models/gemini-2.5-pro:generateContent",
            "DeepSeekCoder-v2-Instruct",
            "context canceled",
            "HTTP 503 credential validation failed",
            "Post \"https://api.anthropic.com/v1/messages?beta=true\": EOF",
            "localhost:8318",
            "v2.1.0 at 18:00:25",
            "2026-11-13T18:00:25+05:30",
            "claude-cli/2.1.280 (external, cli)",
            "12:30 · 1,234,567 tok · $1.25 · 99.5%",
        ] {
            assert_eq!(a.privacy_text(s), s, "{s:?} changed");
        }
    }

    /// Deterministic pseudo-random generator for the fuzz tests.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
        fn pick<'a>(&mut self, v: &[&'a str]) -> &'a str {
            v[self.below(v.len() as u64) as usize]
        }
        fn word(&mut self, alphabet: &str, min: u64, max: u64) -> String {
            let a: Vec<char> = alphabet.chars().collect();
            let n = min + self.below(max - min + 1);
            (0..n)
                .map(|_| a[self.below(a.len() as u64) as usize])
                .collect()
        }
        fn case(&mut self, s: &str) -> String {
            s.chars()
                .map(|c| {
                    if self.below(3) == 0 {
                        c.to_ascii_uppercase()
                    } else {
                        c
                    }
                })
                .collect()
        }
    }

    /// One random identifier and the fragments of it that must not
    /// survive.
    fn secret(r: &mut Rng) -> (String, Vec<String>) {
        const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
        const ALNUM: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        match r.below(6) {
            0 => {
                let user = r.word(LOWER, 3, 10);
                let user = format!("{user}.{}", r.word(LOWER, 2, 6));
                let dom = format!(
                    "{}.{}",
                    r.word(LOWER, 3, 9),
                    r.pick(&["com", "org", "io", "dev"])
                );
                let e = r.case(&format!("{user}@{dom}"));
                let frags = vec![
                    e.split('@').next().unwrap().to_string(),
                    e.split('@').nth(1).unwrap().to_string(),
                ];
                (e, frags)
            }
            1 => {
                let ip = format!(
                    "{}.{}.{}.{}",
                    1 + r.below(254),
                    r.below(256),
                    r.below(256),
                    1 + r.below(254)
                );
                (ip.clone(), vec![ip])
            }
            2 => {
                let g: Vec<String> = (0..4)
                    .map(|_| format!("{:x}", 0x1000 + r.below(0xefff)))
                    .collect();
                let ip = r.case(&format!("{}:{}::{}:{}", g[0], g[1], g[2], g[3]));
                (ip.clone(), vec![g[0].clone(), g[1].clone()])
            }
            3 => {
                let body = r.word(ALNUM, 24, 40);
                let k = format!("{}{body}", r.pick(&["sk-", "sk-proj-", "cpa-", "", "ghp_"]));
                (k, vec![body[..12].to_string()])
            }
            4 => {
                let pre = r.word(ALNUM, 3, 5);
                let suf = r.word(ALNUM, 6, 8);
                (
                    format!("{pre}{}{suf}", "*".repeat(6 + r.below(8) as usize)),
                    vec![suf],
                )
            }
            _ => {
                let body = r.word("0123456789abcdef", 32, 40);
                (body.clone(), vec![body[..10].to_string()])
            }
        }
    }

    /// Embed random identifiers in random text, URLs and punctuation.
    fn embed(r: &mut Rng, secret: &str) -> String {
        let pre = r.pick(&[
            "",
            "error: ",
            "http://",
            "https://host.example:8080/",
            "dial tcp ",
            "Proxy/",
            "proxy/",
            "x=",
            "\"",
            "<",
            "(",
            "[",
            "for ",
            "Bearer ",
            "mailto:",
            "user ",
            "Key:",
        ]);
        let post = r.pick(&[
            "", ":443", "/path", "\"", ">", ")", "]", ", retry", ".", "…", " failed", "?q=1",
            "%eth0",
        ]);
        format!(
            "{}{pre}{secret}{post}{}",
            r.word("abc def ", 0, 6),
            r.word(" xyz.", 0, 4)
        )
    }

    #[test]
    fn fuzzed_identifiers_never_survive() {
        let a = app();
        let mut r = Rng(0x9e37_79b9_7f4a_7c15);
        for _ in 0..5000 {
            let (s, frags) = secret(&mut r);
            let text = embed(&mut r, &s);
            // Only fragments that the surrounding random text does not
            // contain by chance.
            let around = text.replacen(&s, " ", 1).to_lowercase();
            let frags: Vec<&String> = frags
                .iter()
                .filter(|f| !around.contains(&f.to_lowercase()))
                .collect();
            let out = a.privacy_text(&text);
            for f in &frags {
                assert!(
                    !out.to_lowercase().contains(&f.to_lowercase()),
                    "{text:?} -> {out:?} leaks {f:?}"
                );
            }
            // The guard finds the same identifiers in a rendered row.
            let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, 120, 1));
            buf.set_string(0, 0, &text, ratatui::style::Style::default());
            guard_buffer(&mut buf);
            let row: String = (0..120).map(|x| buf[(x, 0)].symbol().to_string()).collect();
            for f in &frags {
                if f.len() + 10 < 120 && text.len() < 110 {
                    assert!(
                        !row.to_lowercase().contains(&f.to_lowercase()),
                        "guard: {text:?} -> {row:?} leaks {f:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn base_url_shows_origin_only() {
        let mut a = app();
        for (url, host, base) in [
            (
                "http://localhost:1/alice@example.org",
                "localhost:1",
                "http://localhost:1",
            ),
            (
                "http://localhost:1/Proxy/10.20.30.40",
                "localhost:1",
                "http://localhost:1",
            ),
            (
                "http://10.0.0.5:8318/keeper",
                "‹ip›:8318",
                "http://‹ip›:8318",
            ),
            ("http://[::1]:8318", "‹ip›:8318", "http://‹ip›:8318"),
            ("http://127.0.0.1:8318", "‹ip›:8318", "http://‹ip›:8318"),
            (
                "https://bob@keeper.example.com/x",
                "keeper.example.com",
                "https://keeper.example.com",
            ),
        ] {
            a.settings.base_url = url.into();
            a.host = crate::api::client::display_host(url);
            assert_eq!(a.host_label(), host, "{url}");
            assert_eq!(a.base_url_label(), base, "{url}");
        }
        a.settings.privacy = false;
        a.settings.base_url = "http://localhost:8318".into();
        a.host = "localhost:8318".into();
        assert_eq!(a.host_label(), "localhost:8318");
    }

    #[test]
    fn structured_fields_are_replaced() {
        let a = app();
        assert!(a.account("ci-bot").starts_with("account-"));
        assert!(a.account("10.2.3.4").starts_with("account-"));
        assert_eq!(private_key_name("4"), "key #4");
        assert_eq!(a.key_name("4", "sk-abc"), "key #4");
        assert_eq!(
            a.user_agent("claude-cli/2.1.280 (external, alice@x.io)"),
            "claude-cli/2.1.280 …"
        );
        assert_eq!(a.user_agent("Mozilla/5.0 AppleWebKit"), "Mozilla/5.0 …");
        assert_eq!(a.user_agent("bob@x.io/1.0"), HIDDEN);
        assert_eq!(a.user_agent("Chrome/142.0.0.0"), "Chrome");
        assert_eq!(a.endpoint("POST /v1/messages"), "POST /v1/messages");
        assert_eq!(
            a.endpoint("POST /v1beta/models/gemini-2.5-pro:generateContent"),
            "POST /v1beta/models/gemini-2.5-pro:generateContent"
        );
        assert_eq!(a.endpoint("GET /u/alice@x.io/v1/messages"), HIDDEN);
        assert_eq!(a.endpoint("POST /proxy/10.0.0.1"), HIDDEN);
        assert_eq!(a.opaque_id("01a1d497-22c1"), HIDDEN);
    }

    #[test]
    fn keyed_tags_are_stable_and_keyed() {
        assert_eq!(keyed_tag(7, "a@b.c"), keyed_tag(7, "a@b.c"));
        assert_ne!(keyed_tag(7, "a@b.c"), keyed_tag(8, "a@b.c"));
        assert_eq!(keyed_tag(7, "a@b.c").len(), 3);
    }

    #[test]
    fn guard_masks_in_place() {
        let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, 60, 2));
        buf.set_string(
            0,
            0,
            "│ host 10.20.30.40 │ ok │",
            ratatui::style::Style::default(),
        );
        buf.set_string(
            0,
            1,
            "│ claude-opus-4-1-20250805 · 12:30 │",
            ratatui::style::Style::default(),
        );
        assert_eq!(guard_buffer(&mut buf), 1);
        let row: String = (0..60).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert!(row.starts_with("│ host ‹ip›······· │ ok │"), "{row}");
    }
}
