//! Human-friendly number, money, duration and identifier formatting.

use chrono::{DateTime, FixedOffset, Utc};

/// Format `x` (>= 0) with three significant digits, choosing the number of
/// decimals from the *rounded* value so carries are handled: `9.996` → `10.0`,
/// `99.96` → `100`. Returns the text and the rounded value.
fn three_sig(x: f64) -> (String, f64) {
    for decimals in [2usize, 1, 0] {
        let s = format!("{x:.decimals$}");
        let r: f64 = s.parse().unwrap_or(x);
        let limit = match decimals {
            2 => 10.0,
            1 => 100.0,
            _ => f64::INFINITY,
        };
        if r < limit {
            return (s, r);
        }
    }
    unreachable!("the last step has no limit")
}

/// Compact number with three significant digits: `999`, `1.23k`, `12.3k`, `123k`, `1.23M`.
pub fn compact(v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    let a = v.abs();
    let sign = if v < 0.0 { "-" } else { "" };
    if a.round() < 1000.0 {
        return format!("{sign}{}", a.round() as i64);
    }
    const UNITS: [(f64, &str); 4] = [(1e3, "k"), (1e6, "M"), (1e9, "B"), (1e12, "T")];
    // Pick the largest unit the value reaches, then promote while rounding
    // would print 1000 of it ("999.96k" → "1.00M").
    let mut i = UNITS.iter().rposition(|(s, _)| a >= *s).unwrap_or(0);
    loop {
        let (scale, unit) = UNITS[i];
        let (s, r) = three_sig(a / scale);
        if r >= 1000.0 && i + 1 < UNITS.len() {
            i += 1;
            continue;
        }
        return format!("{sign}{s}{unit}");
    }
}

/// Integer with thousands separators: `12,345`.
pub fn int(v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    let n = v.round() as i64;
    let s = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// Request counts: exact below 100k, compact above.
pub fn count(v: f64) -> String {
    if v.abs() < 100_000.0 {
        int(v)
    } else {
        compact(v)
    }
}

/// A count with its noun: `1 key`, `6 models`, `1,204 rows`.
pub fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", int(n as f64), if n == 1 { one } else { many })
}

/// USD: `$0.0123` below one dollar, `$3.41`, `$1,234.56`, `$123k` for very large values.
///
/// The amount is rounded as a whole before it is split into dollars and cents,
/// so `$1.999` prints `$2.00` and `$0.99996` prints `$1.00`.
pub fn usd(v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    let a = v.abs();
    let sign = if v < 0.0 { "-" } else { "" };
    if a == 0.0 {
        return "$0.00".into();
    }
    let sub_dollar = format!("{a:.4}");
    if sub_dollar.parse::<f64>().unwrap_or(a) < 1.0 {
        return format!("{sign}${sub_dollar}");
    }
    let cents_total = (a * 100.0).round() as i64;
    let (whole, cents) = (cents_total / 100, cents_total % 100);
    if whole >= 100_000 {
        return format!("{sign}${}", compact(a));
    }
    format!("{sign}${}.{cents:02}", int(whole as f64))
}

/// Optional USD; `—` when the cost is unknown.
pub fn usd_opt(v: Option<f64>) -> String {
    v.map(usd).unwrap_or_else(|| "—".into())
}

/// Percentage with the given number of decimals.
pub fn pct(v: f64, decimals: usize) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    format!("{v:.decimals$}%")
}

/// `part / whole` as a percentage, `None` when `whole` is zero.
pub fn ratio_pct(part: f64, whole: f64) -> Option<f64> {
    (whole > 0.0).then(|| part / whole * 100.0)
}

/// Per-minute rates: 0 decimals above 100, 1 above 10, otherwise 2.
pub fn per_minute(v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    let a = v.abs();
    let sign = if v < 0.0 { "-" } else { "" };
    let (s, r) = three_sig(a);
    if r >= 1000.0 {
        return compact(v);
    }
    format!("{sign}{s}")
}

/// Latency/durations in milliseconds: `123ms`, `1.23s`, `12.3s`, `1m 05s`, `2h 03m`.
pub fn ms(v: f64) -> String {
    if !v.is_finite() || v < 0.0 {
        return "--".into();
    }
    if v.round() < 1000.0 {
        return format!("{}ms", v.round() as i64);
    }
    let (s, r) = three_sig(v / 1000.0);
    if r < 60.0 {
        return format!("{s}s");
    }
    secs(v / 1000.0)
}

/// Durations in seconds with at most two units.
pub fn secs(s: f64) -> String {
    if !s.is_finite() || s < 0.0 {
        return "--".into();
    }
    let total = s.round() as i64;
    let (d, h, m, sec) = (
        total / 86_400,
        (total % 86_400) / 3600,
        (total % 3600) / 60,
        total % 60,
    );
    if d > 0 {
        format!("{d}d {h:02}h")
    } else if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {sec:02}s")
    } else {
        format!("{sec}s")
    }
}

/// Short age: `5s`, `3m`, `2h`, `4d`.
pub fn age(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86_399 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86_400),
    }
}

/// Tokens per second.
pub fn tps(v: f64) -> String {
    if !v.is_finite() || v <= 0.0 {
        return "-".into();
    }
    format!("{v:.1} t/s")
}

/// Parse an RFC3339 timestamp, keeping the server's offset.
pub fn parse_ts(s: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(s.trim()).ok()
}

/// Format a server timestamp in the server's own offset.
pub fn ts_fmt(s: &str, pattern: &str) -> String {
    match parse_ts(s) {
        Some(t) => t.format(pattern).to_string(),
        None if s.len() >= 16 && s.as_bytes().get(10) == Some(&b'T') => s[11..16].to_string(),
        None => s.to_string(),
    }
}

/// Label for a bucket: `HH:MM` for timestamps, `MM-DD` for plain dates.
pub fn bucket_label(s: &str, daily: bool) -> String {
    if s.len() == 10 {
        return s[5..].to_string();
    }
    if daily {
        ts_fmt(s, "%m-%d")
    } else {
        ts_fmt(s, "%H:%M")
    }
}

/// Seconds elapsed since a server timestamp, relative to `now`.
pub fn since(s: &str, now: DateTime<Utc>) -> Option<u64> {
    let t = parse_ts(s)?;
    let d = now.signed_duration_since(t.with_timezone(&Utc));
    Some(d.num_seconds().max(0) as u64)
}

/// Seconds until a server timestamp, relative to `now` (0 when in the past).
pub fn until(s: &str, now: DateTime<Utc>) -> Option<u64> {
    let t = parse_ts(s)?;
    let d = t.with_timezone(&Utc).signed_duration_since(now);
    Some(d.num_seconds().max(0) as u64)
}

/// Redact a server-provided API key label to `prefix…suffix`, like the web
/// dashboard.
///
/// The Keeper masks keys as `abc*********last6`: the short prefix and the
/// six-character suffix it already exposes are kept (the suffix is what tells
/// keys with the same prefix apart), the stars become `…`. Long opaque strings
/// that look like raw keys are cut to their first six characters.
pub fn redact_key(s: &str) -> String {
    let s = s.trim();
    if let (Some(first), Some(last)) = (s.find('*'), s.rfind('*')) {
        let prefix: String = s[..first].chars().take(6).collect();
        let tail = s[last + 1..].trim_end_matches('…');
        // Never more than the six characters the server shows.
        let n = tail.chars().count();
        let suffix: String = tail.chars().skip(n.saturating_sub(6)).collect();
        return format!("{prefix}…{suffix}");
    }
    if let Some(rest) = s.strip_prefix("legacy:") {
        let p: String = rest.chars().take(6).collect();
        return format!("legacy:{p}…");
    }
    let looks_like_key = s.len() >= 20 && !s.contains(' ') && !s.contains('@') && !s.contains('/');
    if looks_like_key {
        let p: String = s.chars().take(6).collect();
        return format!("{p}…");
    }
    s.to_string()
}

/// Truncate to `max` display columns, adding an ellipsis when cut.
pub fn trunc(s: &str, max: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let mut w = 0;
    let mut out = String::new();
    let total: usize = s.chars().map(|c| c.width().unwrap_or(0)).sum();
    if total <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw + 1 > max {
            break;
        }
        w += cw;
        out.push(c);
    }
    out.push('…');
    out
}

/// Truncate to `max` display columns keeping both ends (`claude-…-5-2`), so
/// names that differ only in their tail — model versions, a ` · codex`
/// provider suffix — stay distinguishable. A ` · ` suffix is kept whole
/// when it fits.
pub fn trunc_mid(s: &str, max: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let total = width(s);
    if total <= max {
        return s.to_string();
    }
    if max < 6 {
        return trunc(s, max);
    }
    let take_cols = |chars: &mut dyn Iterator<Item = char>, cols: usize| -> String {
        let mut w = 0;
        let mut out = String::new();
        for c in chars {
            let cw = c.width().unwrap_or(0);
            if w + cw > cols {
                break;
            }
            w += cw;
            out.push(c);
        }
        out
    };
    let budget = max - 1;
    let tail_w = match s.rfind(" · ") {
        Some(i) if width(&s[i..]) + 4 <= budget => width(&s[i..]),
        _ => budget * 2 / 5,
    }
    .min(budget.saturating_sub(3));
    let head = take_cols(&mut s.chars(), budget - tail_w);
    let tail: String = take_cols(&mut s.chars().rev(), tail_w)
        .chars()
        .rev()
        .collect();
    format!("{head}…{tail}")
}

/// Display width of a string.
pub fn width(s: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(s)
}

/// Superscript digits used in pane titles (`¹ overview`).
pub fn superscript(n: usize) -> String {
    const SUP: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    n.to_string()
        .chars()
        .map(|c| SUP[c.to_digit(10).unwrap_or(0) as usize])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plurals() {
        assert_eq!(plural(1, "key", "keys"), "1 key");
        assert_eq!(plural(0, "key", "keys"), "0 keys");
        assert_eq!(plural(1204, "row", "rows"), "1,204 rows");
    }

    #[test]
    fn middle_truncation_keeps_the_tail() {
        assert_eq!(trunc_mid("claude-fable-5-2", 12), "claude-…-5-2");
        assert_eq!(trunc_mid("claude-opus-5-5", 12), "claude-…-5-5");
        assert_eq!(trunc_mid("short", 12), "short");
        assert_eq!(
            trunc_mid("someone.long@example.com · antigravity", 26),
            "someone.lon… · antigravity"
        );
        for (s, w) in [
            ("abcdefghijklmnopqrstuvwxyz", 10),
            ("x · yyyyyyyyyyyyyyyyyyy", 9),
        ] {
            assert_eq!(width(&trunc_mid(s, w)), w, "{s} {w}");
        }
    }

    #[test]
    fn compact_numbers() {
        assert_eq!(compact(0.0), "0");
        assert_eq!(compact(999.0), "999");
        assert_eq!(compact(1000.0), "1.00k");
        assert_eq!(compact(1234.0), "1.23k");
        assert_eq!(compact(12_345.0), "12.3k");
        assert_eq!(compact(123_456.0), "123k");
        assert_eq!(compact(999_999.0), "1.00M");
        assert_eq!(compact(1_200_000.0), "1.20M");
        assert_eq!(compact(118_467_473.0), "118M");
        assert_eq!(compact(3.2e9), "3.20B");
        assert_eq!(compact(-1500.0), "-1.50k");
        // Rounding carries promote the precision or the unit.
        assert_eq!(compact(999.6), "1.00k");
        assert_eq!(compact(9_996.0), "10.0k");
        assert_eq!(compact(99_960.0), "100k");
        assert_eq!(compact(999_950.0), "1.00M");
        assert_eq!(compact(1_999_500.0), "2.00M");
        assert_eq!(compact(1_994_900.0), "1.99M");
        assert_eq!(compact(9_999_999.0), "10.0M");
        assert_eq!(compact(999_999_999_999.0), "1.00T");
        assert_eq!(compact(2.5e15), "2500T");
    }

    #[test]
    fn integers() {
        assert_eq!(int(0.0), "0");
        assert_eq!(int(999.0), "999");
        assert_eq!(int(12_345.0), "12,345");
        assert_eq!(int(1_234_567.0), "1,234,567");
        assert_eq!(count(123_456.0), "123k");
    }

    #[test]
    fn money() {
        assert_eq!(usd(0.0), "$0.00");
        assert_eq!(usd(0.0123), "$0.0123");
        assert_eq!(usd(3.41), "$3.41");
        assert_eq!(usd(1234.567), "$1,234.57");
        assert_eq!(usd(250_000.0), "$250k");
        assert_eq!(usd_opt(None), "—");
        // Carries across the cents boundary.
        assert_eq!(usd(1.999), "$2.00");
        assert_eq!(usd(1.995), "$2.00");
        assert_eq!(usd(1.994), "$1.99");
        assert_eq!(usd(9.999), "$10.00");
        assert_eq!(usd(999.995), "$1,000.00");
        assert_eq!(usd(0.99996), "$1.00");
        assert_eq!(usd(0.99994), "$0.9999");
        assert_eq!(usd(99_999.996), "$100k");
        assert_eq!(usd(-1.999), "-$2.00");
    }

    #[test]
    fn durations() {
        assert_eq!(ms(123.4), "123ms");
        assert_eq!(ms(1234.0), "1.23s");
        assert_eq!(ms(12_345.0), "12.3s");
        assert_eq!(ms(65_000.0), "1m 05s");
        assert_eq!(ms(7_380_000.0), "2h 03m");
        assert_eq!(ms(-1.0), "--");
        assert_eq!(ms(999.4), "999ms");
        assert_eq!(ms(999.6), "1.00s");
        assert_eq!(ms(9_996.0), "10.0s");
        assert_eq!(ms(59_960.0), "1m 00s");
        assert_eq!(age(5), "5s");
        assert_eq!(age(180), "3m");
        assert_eq!(age(7200), "2h");
    }

    #[test]
    fn rates_and_percent() {
        assert_eq!(per_minute(152.4), "152");
        assert_eq!(per_minute(12.34), "12.3");
        assert_eq!(per_minute(1.254), "1.25");
        assert_eq!(per_minute(9.996), "10.0");
        assert_eq!(per_minute(99.96), "100");
        assert_eq!(per_minute(999.6), "1.00k");
        assert_eq!(pct(97.531, 2), "97.53%");
        assert_eq!(ratio_pct(1.0, 0.0), None);
        assert_eq!(tps(83.38), "83.4 t/s");
    }

    #[test]
    fn redaction() {
        assert_eq!(redact_key("abc*********defghi"), "abc…defghi");
        assert_eq!(redact_key("abc***…"), "abc…");
        assert_eq!(redact_key("abc***0123456789"), "abc…456789");
        assert_eq!(redact_key("my-alias"), "my-alias");
        assert_eq!(redact_key("sk-abcdefghijklmnopqrstuvwxyz"), "sk-abc…");
        assert_eq!(redact_key("legacy:0123456789abcdef"), "legacy:012345…");
    }

    #[test]
    fn timestamps() {
        assert_eq!(bucket_label("2026-11-13T02:30:00+05:30", false), "02:30");
        assert_eq!(bucket_label("2026-11-13", true), "11-13");
        assert_eq!(superscript(12), "¹²");
        assert_eq!(trunc("abcdef", 4), "abc…");
        assert_eq!(trunc("abc", 4), "abc");
    }
}
