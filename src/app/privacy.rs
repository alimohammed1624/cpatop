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

    /// Text the user is typing (a `/` filter, a picker search) or typed
    /// earlier and shown back (a filter in a pane title). In privacy mode it
    /// is hidden from the first character — before an `@` is typed a
    /// half-typed address or key looks like any other word — as bullets
    /// and a character count: `••••• 5`. Filtering still uses the real text.
    pub fn typed(&self, s: &str) -> String {
        if self.settings.privacy {
            hide_typed(s)
        } else {
            s.to_string()
        }
    }

    /// A date or time typed into the custom-range editor: in privacy mode
    /// every character other than digits, `-`, `:`, `.` and blanks is a
    /// bullet, so dates stay readable while anything else stays hidden.
    pub fn typed_date(&self, s: &str) -> String {
        if !self.settings.privacy {
            return s.to_string();
        }
        s.chars()
            .map(|c| {
                if c.is_ascii_digit() || matches!(c, '-' | ':' | '.' | ' ') {
                    c
                } else {
                    '•'
                }
            })
            .collect()
    }

    /// An IP address field (`‹ip›` in privacy mode).
    pub fn ip(&self, s: &str) -> String {
        if self.settings.privacy && !s.is_empty() && s != "-" {
            IP_MASK.into()
        } else {
            s.to_string()
        }
    }

    /// A user agent. In privacy mode only product tokens are kept
    /// (`Mozilla/5.0 (…) AppleWebKit/537.36 (…) Chrome Safari/537.36`):
    /// comments become `(…)`, versions shaped like an address are dropped,
    /// anything else becomes `…` — comments and free tokens can carry host
    /// names, accounts or ids.
    pub fn user_agent(&self, s: &str) -> String {
        if !self.settings.privacy || s.is_empty() || s == "-" {
            return s.to_string();
        }
        let mut out: Vec<String> = Vec::new();
        let mut depth = 0usize;
        for tok in s.split_whitespace() {
            if depth > 0 || tok.starts_with('(') {
                depth += tok.matches('(').count();
                depth = depth.saturating_sub(tok.matches(')').count());
                if out.last().map(String::as_str) != Some("(…)") {
                    out.push("(…)".into());
                }
                continue;
            }
            // Only `product/version` tokens; bare words may be names.
            let Some((name, ver)) = tok.split_once('/') else {
                if out.last().is_none_or(|l| l != "…") {
                    out.push("…".into());
                }
                continue;
            };
            let name_ok = !name.is_empty()
                && name.len() <= 24
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphabetic() || c == '-' || c == '_' || c == '.')
                && !is_sensitive(name);
            let ver_ok = !ver.is_empty()
                && ver.len() <= 16
                && ver.chars().all(|c| c.is_ascii_digit() || c == '.')
                && ver.matches('.').count() <= 2;
            let piece = match (name_ok, ver_ok) {
                (true, true) => format!("{name}/{ver}"),
                (true, false) => name.to_string(),
                _ => "…".to_string(),
            };
            if piece == "…" && out.last().is_some_and(|l| l == "…") {
                continue;
            }
            out.push(piece);
        }
        if out.iter().all(|p| p == "…" || p == "(…)") {
            return HIDDEN.into();
        }
        out.join(" ")
    }

    /// A request endpoint (`POST /v1/messages`). In privacy mode only
    /// well-known API routes are shown, never with their query string or
    /// fragment (`/v1/messages?…`); anything else is hidden.
    pub fn endpoint(&self, s: &str) -> String {
        if !self.settings.privacy || s.is_empty() || s == "-" {
            return s.to_string();
        }
        let (method, target) = match s.trim().split_once(' ') {
            Some((m, p)) if m.chars().all(|c| c.is_ascii_uppercase()) => (Some(m), p.trim()),
            _ => (None, s.trim()),
        };
        let (path, rest) = match target.find(['?', '#']) {
            Some(i) => (&target[..i], "?…"),
            None => (target, ""),
        };
        if !known_route(path) {
            return HIDDEN.into();
        }
        match method {
            Some(m) => format!("{m} {path}{rest}"),
            None => format!("{path}{rest}"),
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
        self.ident_labels(i).0
    }

    /// [`App::ident_label`] and its compact form, where the provider suffix
    /// is abbreviated (`account-1 · ag`) so the account name itself never
    /// has to be cut. Both are the same when there is no suffix.
    pub fn ident_labels(&self, i: &Identity) -> (String, String) {
        let base = raw_ident_name(i);
        let name = self.account(base);
        let Some(page) = self.data.identities.get() else {
            return (name.clone(), name);
        };
        let same: Vec<&Identity> = page
            .identities
            .iter()
            .filter(|o| raw_ident_name(o) == base)
            .collect();
        if same.len() <= 1 {
            return (name.clone(), name);
        }
        let kind = ident_kind(i);
        let short = ident_kind_short(i);
        if same.iter().filter(|o| ident_kind(o) == kind).count() > 1 {
            (
                format!("{name} · {kind} #{}", i.id),
                format!("{name} · {short} #{}", i.id),
            )
        } else {
            (format!("{name} · {kind}"), format!("{name} · {short}"))
        }
    }

    /// Name for a credential known by a usage key and server label.
    pub fn credential_name(&self, key: &str, label: &str) -> String {
        self.credential_labels(key, label).0
    }

    /// [`App::credential_name`] and its compact form (see
    /// [`App::ident_labels`]).
    pub fn credential_labels(&self, key: &str, label: &str) -> (String, String) {
        match self.identity_for(key) {
            Some(i) => self.ident_labels(i),
            None if label.is_empty() || label == "-" => (label.to_string(), label.to_string()),
            None => {
                let n = self.account(label);
                (n.clone(), n)
            }
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

/// [`ident_kind`] abbreviated (`cl`, `cx api`), for names in tight columns.
pub fn ident_kind_short(i: &Identity) -> String {
    let k = provider_abbrev(&i.kind);
    if i.auth_type == 2 {
        format!("{k} api")
    } else {
        k
    }
}

/// Known provider types and their two-letter abbreviations (shown in the
/// help overlay).
pub const PROVIDER_ABBREVS: &[(&str, &str)] = &[
    ("claude", "cl"),
    ("codex", "cx"),
    ("antigravity", "ag"),
    ("gemini", "gm"),
    ("gemini-cli", "gc"),
    ("vertex", "vx"),
    ("aistudio", "as"),
    ("openai", "oa"),
    ("openai-compatibility", "oc"),
    ("qwen", "qw"),
    ("iflow", "if"),
    ("kimi", "km"),
    ("xai", "xa"),
];

/// A provider type's abbreviation: from [`PROVIDER_ABBREVS`], else short
/// names as they are and the first three letters of longer ones (never two
/// letters, so they cannot be mistaken for a known provider).
pub fn provider_abbrev(kind: &str) -> String {
    let k = kind.trim();
    if let Some((_, a)) = PROVIDER_ABBREVS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(k))
    {
        return (*a).to_string();
    }
    if k.chars().count() <= 4 {
        k.to_string()
    } else {
        k.chars().take(3).collect()
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
    /// The part of a URL after `?` or `#`.
    Query,
}

impl Sensitive {
    pub fn placeholder(self) -> &'static str {
        match self {
            Sensitive::Ip => IP_MASK,
            Sensitive::Email => "‹email›",
            Sensitive::Key => "‹key›",
            Sensitive::Query => "…",
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

/// Typed text hidden as bullets (at most eight) and its length in
/// characters: `••••• 5`; empty stays empty.
pub fn hide_typed(s: &str) -> String {
    let n = s.chars().count();
    if n == 0 {
        return String::new();
    }
    format!("{} {n}", "•".repeat(n.min(8)))
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

/// Characters of an opaque token: everything base64, base64url, hex and
/// most key formats are made of.
fn token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+' | '/' | '=')
}

/// Characters that can belong to an e-mail address as typed or shown:
/// anything but whitespace and the drawing characters the UI puts around
/// text (box lines, `·`, `‹›`). Over-long runs are masked whole, so every
/// RFC 5322 local-part character (`!#$%&'*+/=?^_\`{|}~-.`) and quoted
/// local parts are covered.
fn email_char(c: char) -> bool {
    if c.is_whitespace() {
        return false;
    }
    if c.is_ascii() {
        return !c.is_ascii_control();
    }
    c == '…' || c.is_alphanumeric()
}

/// Whether a token reads like a model name or version rather than a
/// secret: every `-`/`.`/`_` segment is a number (≤ 8 digits), a short mix
/// of letters and digits (≤ 8: `4o`, `480B`, `A35B`) or a word (≤ 20
/// letters; from 12 letters on it must read as words — `generateContent`,
/// `DeepSeekCoder` — not random case like `XpEldHTvjaRE`). Only used to
/// decide which server-reported model names are registered as safe (see
/// [`register_safe_name`]) and for API routes; it never exempts free text.
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

/// Model names the server reported in a model field (lower-cased). Long
/// tokens are masked unless they are one of these (or another known-safe
/// shape, see [`safe_token`]).
static SAFE_NAMES: std::sync::LazyLock<std::sync::RwLock<std::collections::HashSet<String>>> =
    std::sync::LazyLock::new(Default::default);

/// Record a model name the server reported (catalog, pricing, usage
/// series, request events…) so that privacy mode shows it. Names that
/// carry anything identifying, or are not shaped like a model id, are
/// ignored and stay masked.
pub fn register_safe_name(name: &str) {
    let name = name.trim();
    let tok = name.trim_start_matches("models/");
    if tok.len() < 2
        || tok.len() > 96
        || !tok.chars().all(token_char)
        || tok.contains('/')
        || !model_shaped(tok)
        || !sensitive_spans_with(tok, false).is_empty()
    {
        return;
    }
    let lower = tok.to_ascii_lowercase();
    if SAFE_NAMES.read().is_ok_and(|s| s.contains(&lower)) {
        return;
    }
    if let Ok(mut s) = SAFE_NAMES.write() {
        s.insert(lower);
    }
}

/// Long UI strings that are never identifiers.
const SAFE_UI: &[&str] = &[
    "yyyy-mm-dd..yyyy-mm-dd",
    "openai-compatibility",
    "cpatop_no_update_check=1",
    "update_check",
    "logout_on_exit",
    "slow_refresh_secs",
    "invalid_request_error",
    "authentication_error",
    "context_length_exceeded",
    "migration_in_progress",
    "rate_limit_exceeded",
    "insufficient_quota",
    "server_is_overloaded",
];

/// Path segments of API routes and project URLs (`/v1/chat/completions`,
/// `github.com/alimohammed1624/cpatop/releases`).
const ROUTE_WORDS: &[&str] = &[
    "api",
    "v1",
    "v1beta",
    "v1alpha",
    "v1beta1",
    "v2",
    "messages",
    "count_tokens",
    "chat",
    "completions",
    "responses",
    "compact",
    "embeddings",
    "models",
    "backend-api",
    "codex",
    "provider",
    "openai",
    "anthropic",
    "gemini",
    "images",
    "generations",
    "usage",
    "events",
    "filters",
    "auth",
    "session",
    "startup",
    "status",
    "keeper",
    "health",
    "version",
    "quota",
    "alimohammed1624",
    "cpatop",
    "releases",
    "latest",
    "download",
    "main",
    "install.sh",
];

/// Top-level domains accepted in host names inside long tokens.
const TLDS: &[&str] = &[
    "com", "org", "net", "io", "dev", "ai", "app", "co", "cloud", "sh", "so",
];

/// A dated or timed value (`2026-11-13T18:00:25.123+05`).
fn timestamp_shaped(t: &str) -> bool {
    let b = t.as_bytes();
    b.len() >= 10
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[7] == b'-'
        && b[8..10].iter().all(u8::is_ascii_digit)
        && t[10..]
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, 'T' | 'Z' | '.' | '+' | '-'))
}

/// A host name with a common top-level domain (`api.anthropic.com`).
fn host_shaped(t: &str) -> bool {
    let labels: Vec<&str> = t.split('.').collect();
    labels.len() >= 2
        && TLDS.contains(&labels[labels.len() - 1].to_ascii_lowercase().as_str())
        && labels.iter().all(|l| {
            !l.is_empty()
                && l.len() <= 24
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

/// A product token `name/1.2.3` of a user agent.
fn product_shaped(t: &str) -> bool {
    let Some((name, ver)) = t.split_once('/') else {
        return false;
    };
    !name.is_empty()
        && name
            .split('-')
            .all(|w| !w.is_empty() && w.len() <= 12 && w.chars().all(|c| c.is_ascii_alphabetic()))
        && !ver.is_empty()
        && ver.len() <= 16
        && ver.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// Whether a long token is a known-safe shape: a model name the server
/// reported (or the start of one, when `cut` by an ellipsis), a known UI
/// string, a timestamp, an API route or project URL path, a host name or a
/// user-agent product. Everything else of 20+ characters is masked.
fn safe_token(tok: &str, cut: bool) -> bool {
    let lower = tok.to_ascii_lowercase();
    if SAFE_UI.contains(&lower.as_str()) || timestamp_shaped(tok) || product_shaped(tok) {
        return true;
    }
    let known = |w: &str| {
        let Ok(set) = SAFE_NAMES.read() else {
            return false;
        };
        set.contains(w) || (cut && w.len() >= 8 && set.iter().any(|n| n.starts_with(w)))
    };
    if !lower.contains('/') {
        return known(&lower) || host_shaped(tok);
    }
    let segs: Vec<&str> = lower.split('/').filter(|s| !s.is_empty()).collect();
    let last = segs.len().saturating_sub(1);
    !segs.is_empty()
        && segs.iter().enumerate().all(|(i, seg)| {
            ROUTE_WORDS.contains(seg)
                || seg.len() <= 4 && seg.chars().all(|c| c.is_ascii_digit() || c == '.')
                || host_shaped(seg)
                || SAFE_NAMES.read().is_ok_and(|s| s.contains(*seg))
                || (i == last && cut && known(seg))
        })
}

/// Byte ranges of every identifier in `s`, sorted and non-overlapping:
///
/// - IPv4 and IPv6 addresses anywhere, in any case and context (URLs,
///   `host:port`, `[v6]`, `proxy/10.0.0.1`, versions that are valid
///   addresses), and dotted/coloned fragments cut by an ellipsis;
/// - e-mail addresses: the whole non-blank run around any `@` (every RFC
///   local-part character and quoted local parts included);
/// - URL query strings and fragments (the part after `?`/`#` of a run
///   with a path), which carry keys as often as not;
/// - masked keys (`abc***def`), keys with a known prefix (`sk-…`), and
///   any token of 20+ `[A-Za-z0-9_.+/=-]` characters that is not a
///   known-safe shape (see [`safe_token`]) — over-masking is preferred to
///   leaking.
pub fn sensitive_spans(s: &str) -> Vec<(usize, usize, Sensitive)> {
    sensitive_spans_with(s, true)
}

fn sensitive_spans_with(s: &str, long_tokens: bool) -> Vec<(usize, usize, Sensitive)> {
    let mut spans: Vec<(usize, usize, Sensitive)> = ip_spans(s)
        .into_iter()
        .map(|(a, b)| (a, b, Sensitive::Ip))
        .collect();
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let end_of = |i: usize| chars.get(i).map(|c| c.0).unwrap_or(s.len());
    // E-mail addresses (and `@handles`): the whole run around each `@`.
    let mut i = 0;
    while i < chars.len() {
        if chars[i].1 != '@' {
            i += 1;
            continue;
        }
        let mut a = local_start(&chars, i);
        let mut b = i + 1;
        while b < chars.len() && email_char(chars[b].1) {
            b += 1;
        }
        // Brackets and quotes around the address are not part of it.
        while a < i && matches!(chars[a].1, '<' | '(' | '[' | '{') {
            a += 1;
        }
        while b > i + 1
            && matches!(
                chars[b - 1].1,
                '>' | ')' | ']' | '}' | ',' | ';' | ':' | '.'
            )
        {
            b -= 1;
        }
        if b - a > 1 {
            spans.push((end_of(a), end_of(b), Sensitive::Email));
        }
        i = b.max(i + 1);
    }
    // Query strings and fragments of URLs and paths.
    for (i, &(_, c)) in chars.iter().enumerate() {
        if c != '?' && c != '#' {
            continue;
        }
        let mut a = i;
        while a > 0 && !chars[a - 1].1.is_whitespace() && chars[a - 1].1 != '"' {
            a -= 1;
        }
        let head: String = chars[a..i].iter().map(|c| c.1).collect();
        let pathish = if c == '?' {
            head.contains('/')
        } else {
            head.contains("://")
        };
        let mut b = i + 1;
        while b < chars.len()
            && !chars[b].1.is_whitespace()
            && !matches!(chars[b].1, '"' | '\'' | ')' | ']' | '>' | '│')
        {
            b += 1;
        }
        while b > i + 1 && matches!(chars[b - 1].1, ',' | ';' | ':' | '.') {
            b -= 1;
        }
        if pathish && b > i + 1 {
            spans.push((end_of(i + 1), end_of(b), Sensitive::Query));
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
        let tok = s[a..b].trim_end_matches(['.', '/']);
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
        let long = long_tokens && tok.len() >= 20 && !safe_token(tok, cut && tok.len() == b - a);
        if prefixed || long {
            spans.push((a, a + tok.len(), Sensitive::Key));
        }
        i = j;
    }
    normalize(spans)
}

/// Characters that end a field whatever the quoting: line breaks and the
/// non-ASCII drawing characters the UI puts between fields (`│`, `·`, `‹›`).
fn hard_delimiter(c: char) -> bool {
    c == '\n' || c == '\r' || (!c.is_whitespace() && !email_char(c))
}

/// Whether the character at `k` ends an unquoted field: a blank (not
/// escaped with `\\`) or `,` `;` `<` `>` `[` `]`. Parentheses, braces and `|`
/// are not: RFC 5322 allows comments `(…)` and `{|}` in local parts.
fn soft_delimiter(chars: &[(usize, char)], k: usize) -> bool {
    let c = chars[k].1;
    (c.is_whitespace() && (k == 0 || chars[k - 1].1 != '\\'))
        || matches!(c, ',' | ';' | '<' | '>' | '[' | ']')
}

/// Where the local part of the e-mail address whose `@` is at char index
/// `at` starts. Without a `"` between the field start and the `@`, the
/// address starts after the last blank, `,`, `;`, `<`, `>`, `[` or `]`.
/// With one, quoting is in play — and quoted local parts may hold blanks, commas and
/// escaped quotes (`"alice \"bob\" smith"@x.org`) — so rather than trust an
/// escape-aware reading of possibly malformed input, the address is taken
/// to start at the first `"` of the field (bounded only by line breaks and
/// drawing characters), plus any unquoted characters glued to it. This
/// over-masks text that merely precedes a quoted address on the same field;
/// it never leaves part of the local part visible.
fn local_start(chars: &[(usize, char)], at: usize) -> usize {
    // The field: back to a hard delimiter.
    let mut field = at;
    while field > 0 && !hard_delimiter(chars[field - 1].1) {
        field -= 1;
    }
    // The unquoted reading: back to a blank or list/bracket character.
    let mut a = at;
    while a > field && !soft_delimiter(chars, a - 1) {
        a -= 1;
    }
    let Some(q) = (field..at).find(|k| chars[*k].1 == '"') else {
        return a;
    };
    let mut a = q.min(a);
    while a > field && !soft_delimiter(chars, a - 1) {
        a -= 1;
    }
    a
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

// ---------------------------------------------------------------- terminal output

/// Whether privacy mode applies to what cpatop prints outside the UI
/// (errors, hints, warnings, reports, panics, subcommand output).
static OUTPUT_PRIVACY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What a file path is replaced with in privacy-mode messages.
pub const PATH_MASK: &str = "‹path›";

/// Turn privacy for printed output on or off (main sets it as soon as the
/// arguments, config and UI state are known; `x` in the app follows it).
pub fn set_output_privacy(on: bool) {
    OUTPUT_PRIVACY.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether printed output is scrubbed.
pub fn output_privacy() -> bool {
    OUTPUT_PRIVACY.load(std::sync::atomic::Ordering::Relaxed)
}

/// A file path for a message: `‹path›` in privacy mode — paths name
/// users, hosts and sometimes e-mail addresses.
pub fn shown_path(p: &std::path::Path) -> String {
    if output_privacy() {
        PATH_MASK.to_string()
    } else {
        p.display().to_string()
    }
}

/// A path for `cpatop paths` in privacy mode: the home directory reads
/// `~`, the user-name segment of any other home directory (`/home/x`,
/// `/Users/x`) `‹user›`, and any segment holding an e-mail address, key
/// or IP address `‹path›`; the rest (`.config/cpatop/config.toml`) stays,
/// so the output is still useful.
pub fn private_path(p: &std::path::Path, home: Option<&std::path::Path>) -> String {
    use std::path::Component;
    let (rest, mut out) = match home.filter(|h| !h.as_os_str().is_empty()) {
        Some(h) if p.starts_with(h) => (p.strip_prefix(h).unwrap_or(p), "~".to_string()),
        _ => (p, String::new()),
    };
    let mut prev = String::new();
    for c in rest.components() {
        let seg = c.as_os_str().to_string_lossy().into_owned();
        let shown = match c {
            Component::RootDir => {
                out.push('/');
                continue;
            }
            Component::Normal(_)
                if matches!(prev.as_str(), "home" | "Users") && out == format!("/{prev}") =>
            {
                "‹user›".to_string()
            }
            Component::Normal(_) if !sensitive_spans(&seg).is_empty() => PATH_MASK.to_string(),
            _ => seg.clone(),
        };
        if !out.is_empty() && !out.ends_with('/') {
            out.push('/');
        }
        out.push_str(&shown);
        prev = seg;
    }
    out
}

/// Text about to be printed: in privacy mode without terminal escape
/// sequences and with every identifier masked (see [`sensitive_spans`]).
pub fn scrub_output(s: &str) -> String {
    if output_privacy() {
        scrub(&strip_ansi(s))
    } else {
        s.to_string()
    }
}

/// Remove ANSI escape sequences (colour codes would split tokens).
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match it.peek() {
            // CSI: parameters and intermediates up to a final byte.
            Some('[') => {
                it.next();
                for c in it.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            // OSC: up to BEL or ST.
            Some(']') => {
                it.next();
                while let Some(c) = it.next() {
                    if c == '\u{7}' || (c == '\u{1b}' && it.peek() == Some(&'\\')) {
                        if c == '\u{1b}' {
                            it.next();
                        }
                        break;
                    }
                }
            }
            Some(_) => {
                it.next();
            }
            None => {}
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

    /// Model names the tests treat as reported by the server.
    fn register_models() {
        for m in [
            "claude-opus-4-1-20250805",
            "gemini-2.5-pro",
            "gemini-2.5-pro-preview-06-05",
            "Qwen3-Coder-480B-A35B-Instruct",
            "DeepSeekCoder-v2-Instruct",
        ] {
            register_safe_name(m);
        }
    }

    #[test]
    fn free_text_loses_emails_keys_and_ips() {
        register_models();
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
        // Query strings and fragments.
        (
            "POST /v1/messages?api_key=abcdef-ghijkl-mnopqr-stuvwx",
            &["abcdef", "stuvwx"],
        ),
        (
            "GET https://h.example/cb?code=Zq1&state=xY2",
            &["Zq1", "xY2"],
        ),
        ("see https://h.example/p#access_token=Qz9", &["Qz9"]),
        ("\"/v1/models?key=AIzaQwErTy\": 403", &["QwErTy"]),
        // E-mail local parts with RFC special characters.
        ("filter alice/bob@example.org", &["alice", "bob", "example"]),
        ("to: o'brien@example.org", &["brien", "example"]),
        ("x{carol}y@example.org", &["carol", "example"]),
        (
            "\"carol smith\"@example.org",
            &["carol", "smith", "example"],
        ),
        ("a|b=c?d^e`f~g@example.org", &["a|b", "f~g", "example"]),
        ("path /tmp/dave@x.io/missing", &["dave", "x.io", "missing"]),
        // Long tokens of any case and base64.
        ("lower abcdefghijklmnopqrst end", &["abcdefghijklmnopqrst"]),
        ("dashed abcdef-ghijkl-mnopqr-stuvwx", &["ghijkl"]),
        ("b64 ab+cd/ef+gh/ij+kl/mn+op== end", &["ef+gh", "mn+op"]),
        ("words_only_lowercase_token_here", &["lowercase_token"]),
    ];

    #[test]
    fn adversarial_free_text_is_scrubbed() {
        let a = app();
        for (input, bad) in LEAKS {
            let out = a.privacy_text(input);
            let plain = scrub(input);
            let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, 100, 1));
            buf.set_string(0, 0, input, ratatui::style::Style::default());
            guard_buffer(&mut buf);
            let row: String = (0..100).map(|x| buf[(x, 0)].symbol().to_string()).collect();
            for frag in *bad {
                assert!(!out.contains(frag), "{input:?} -> {out:?} leaks {frag:?}");
                assert!(!plain.contains(frag), "scrub {input:?} -> {plain:?}");
                assert!(!row.contains(frag), "guard {input:?} -> {row:?}");
            }
        }
    }

    /// Quoted local parts with escaped quotes, blanks and list characters
    /// (P1): no word of the local part or the domain survives, whatever
    /// precedes the address and however the quoting is (mis)formed.
    #[test]
    fn quoted_local_parts_with_escapes_never_leak() {
        let a = app();
        for text in [
            r#""alice \"bob"@example.org"#,
            r#""alice \"bob\" smith"@example.org"#,
            r#"filter "alice \"bob\" smith"@example.org, retry"#,
            r#"--range "alice \"bob"@example.org"#,
            r#"invalid value '"alice \"bob\" smith"@example.org' for '--range'"#,
            r#""alice,bob;smith"@example.org"#,
            r#""alice\\"@example.org"#,
            r#"\"alice bob"@example.org"#,
            r#""alice" bob"@example.org"#,
            r#"x "alice (bob) <smith>"@example.org y"#,
            r#"alice\ bob@example.org"#,
            r#"alice(bob)smith@example.org"#,
            r#"to: "alice @ bob"@example.org"#,
            "│ \"alice \\\"bob\"@example.org │",
        ] {
            let out = [a.privacy_text(text), scrub(text)];
            let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, 100, 1));
            buf.set_string(0, 0, text, ratatui::style::Style::default());
            guard_buffer(&mut buf);
            let row: String = (0..100).map(|x| buf[(x, 0)].symbol().to_string()).collect();
            for o in out.iter().chain([&row]) {
                for frag in ["alice", "bob", "smith", "example"] {
                    assert!(!o.contains(frag), "{text:?} -> {o:?} leaks {frag:?}");
                }
            }
        }
        // Fuzz: random words joined by blanks, escaped quotes, escaped
        // backslashes, commas and brackets inside a quoted local part,
        // after random text.
        let mut r = Rng(0x51ed_270b_9a3c_4f11);
        for _ in 0..5000 {
            let n = 1 + r.below(4);
            let mut words = vec![];
            let mut local = String::new();
            for i in 0..n {
                let w = r.word("klmnopqrstuvw", 5, 8);
                if i > 0 {
                    local.push_str(r.pick(&[
                        " ", "\\\"", " \\\" ", "\\\\", ",", "(", ") ", "<", "@", "  ",
                    ]));
                }
                local.push_str(&w);
                words.push(w);
            }
            if r.below(3) == 0 {
                local.push_str(r.pick(&["\\\"", "\\\\", " "]));
            }
            let pre = r.pick(&[
                "", "filter ", "x: ", "\"", "a \"b\" ", "to <", "(", "[", "--range ",
            ]);
            let post = r.pick(&["", " retry", ">", ", next", "'"]);
            let text = format!("{pre}\"{local}\"@example.org{post}");
            let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, 160, 1));
            buf.set_string(0, 0, &text, ratatui::style::Style::default());
            guard_buffer(&mut buf);
            let row: String = (0..160).map(|x| buf[(x, 0)].symbol().to_string()).collect();
            for o in [a.privacy_text(&text), scrub(&text), row] {
                for w in words.iter().map(String::as_str).chain(["example"]) {
                    assert!(!o.contains(w), "{text:?} -> {o:?} leaks {w:?}");
                }
            }
        }
    }

    #[test]
    fn paths_mask_only_identifying_segments() {
        use std::path::Path;
        let home = Path::new("/tmp/tmp.VMXveRdbhA/home");
        assert_eq!(
            private_path(&home.join(".config/cpatop/config.toml"), Some(home)),
            "~/.config/cpatop/config.toml"
        );
        assert_eq!(
            private_path(
                Path::new("/home/alice/.local/state/cpatop/s.json"),
                Some(home)
            ),
            "/home/‹user›/.local/state/cpatop/s.json"
        );
        assert_eq!(
            private_path(Path::new("/Users/bob/x"), None),
            "/Users/‹user›/x"
        );
        assert_eq!(
            private_path(Path::new("/srv/alice@example.org/10.0.0.1/cfg"), None),
            "/srv/‹path›/‹path›/cfg"
        );
        assert_eq!(private_path(Path::new("/etc/cpatop"), None), "/etc/cpatop");
    }

    #[test]
    fn endpoints_never_keep_queries() {
        let a = app();
        for (input, want) in [
            (
                "POST /v1/messages?api_key=abcdef-ghijkl-mnopqr-stuvwx",
                "POST /v1/messages?…",
            ),
            ("POST /v1/messages#Zx9", "POST /v1/messages?…"),
            ("/v1/chat/completions?k=1", "/v1/chat/completions?…"),
            ("GET /v1/models", "GET /v1/models"),
            ("POST /x/v1/messages?k=1", HIDDEN),
        ] {
            let out = a.endpoint(input);
            assert_eq!(out, want, "{input}");
            assert_eq!(scrub(&out), out, "{input}");
        }
    }

    #[test]
    fn every_rfc_local_part_character_is_masked() {
        let a = app();
        for c in "!#$%&'*+/=?^_`{|}~-.".chars() {
            for local in [
                format!("alpha{c}bravo"),
                format!("{c}alphabravo"),
                format!("alphabravo{c}"),
            ] {
                for text in [
                    format!("{local}@charlie.example"),
                    format!("user {local}@charlie.example, retry"),
                    format!("<{local}@charlie.example>"),
                    format!("/tmp/{local}@charlie.example/x"),
                    format!("filter: {local}@charlie.example│"),
                ] {
                    for out in [a.privacy_text(&text), scrub(&text)] {
                        for frag in ["alpha", "bravo", "charlie"] {
                            assert!(!out.contains(frag), "{text:?} -> {out:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn long_tokens_need_a_known_safe_shape() {
        let a = app();
        // Never reported by the server: masked, whatever the case.
        for t in [
            "abcdefghijklmnopqrst",
            "abcdefghijklmnopqrstuvwxyz",
            "qwerty-uiopas-dfghjk-lzxcvb",
            "zz-unlisted-model-name-0001",
            "ABCDEFGHIJKLMNOPQRST",
        ] {
            assert_eq!(a.privacy_text(t), "‹key›", "{t}");
        }
        // Known-safe shapes stay.
        register_safe_name("zz-listed-model-name-20260101");
        for t in [
            "zz-listed-model-name-20260101",
            "zz-listed-model-name…",
            "2026-11-13T18:00:25.123456Z",
            "2026-11-08..2026-11-13",
            "POST /api/provider/anthropic/v1/messages",
            "api.anthropic.com/v1/messages",
            "python-requests/2.31.0",
        ] {
            assert_eq!(a.privacy_text(t), t, "{t}");
        }
        // A name with an identifier in it is never registered.
        register_safe_name("zz-model-alice@example.org-0000");
        register_safe_name("sk-proj-0123456789abcdefABCD");
        assert_eq!(a.privacy_text("sk-proj-0123456789abcdefABCD"), "‹key›");
    }

    #[test]
    fn harmless_text_is_kept() {
        register_models();
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
            "Post \"https://api.anthropic.com/v1/messages\": EOF",
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
        match r.below(10) {
            6 => {
                // An e-mail address with RFC special characters.
                let specials: Vec<char> = "!#$%&'*+/=?^_`{|}~-.".chars().collect();
                let a = r.word(LOWER, 4, 8);
                let b = r.word(LOWER, 4, 8);
                let c = specials[r.below(specials.len() as u64) as usize];
                let dom = format!("{}.{}", r.word(LOWER, 4, 9), r.pick(&["com", "org"]));
                let e = format!("{a}{c}{b}@{dom}");
                (e, vec![a, b, dom])
            }
            7 => {
                // A long token of lower-case words (once exempt).
                let sep = r.pick(&["-", "_", "", "."]);
                let mut parts: Vec<String> = (0..3).map(|_| r.word(LOWER, 5, 8)).collect();
                while parts.join(sep).len() < 20 {
                    parts.push(r.word(LOWER, 5, 8));
                }
                let t = parts.join(sep);
                (t, vec![parts[1].clone()])
            }
            8 => {
                // A key in a URL query string or fragment.
                let k = r.word(ALNUM, 6, 12);
                let u = format!(
                    "https://host.example/v1/messages{}{}={k}",
                    r.pick(&["?", "?a=1&", "#"]),
                    r.pick(&["key", "api_key", "token", "x"])
                );
                (u, vec![k])
            }
            9 => {
                // Base64 with `+`, `/` and padding.
                let body: Vec<String> = (0..4).map(|_| r.word(ALNUM, 5, 7)).collect();
                let t = format!("{}+{}/{}+{}==", body[0], body[1], body[2], body[3]);
                (t, vec![format!("{}+{}", body[0], body[1])])
            }
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
            "claude-cli/2.1.280 (…)"
        );
        assert_eq!(
            a.user_agent(
                "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36"
            ),
            "Mozilla/5.0 (…) AppleWebKit/537.36 (…) Chrome Safari/537.36"
        );
        assert_eq!(a.user_agent("bob@x.io/1.0"), HIDDEN);
        assert_eq!(a.user_agent("curl/8 host-10.0.0.1 x"), "curl/8 …");
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
        register_models();
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
