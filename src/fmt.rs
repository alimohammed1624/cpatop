//! Human-friendly number, money, duration and identifier formatting.

use chrono::{DateTime, FixedOffset, Utc};

/// Compact number with three significant digits: `999`, `1.23k`, `12.3k`, `123k`, `1.23M`.
pub fn compact(v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    let a = v.abs();
    let sign = if v < 0.0 { "-" } else { "" };
    if a < 1000.0 {
        return format!("{sign}{}", a.round() as i64);
    }
    const UNITS: [(f64, &str); 4] = [(1e12, "T"), (1e9, "B"), (1e6, "M"), (1e3, "k")];
    for (i, (scale, unit)) in UNITS.iter().enumerate() {
        if a >= *scale {
            let x = a / scale;
            // Avoid "1000k": promote to the next unit when rounding overflows.
            let s = if x >= 999.5 && i > 0 {
                let (s2, u2) = UNITS[i - 1];
                return format!("{sign}{:.2}{u2}", a / s2);
            } else if x >= 99.95 {
                format!("{x:.0}")
            } else if x >= 9.995 {
                format!("{x:.1}")
            } else {
                format!("{x:.2}")
            };
            return format!("{sign}{s}{unit}");
        }
    }
    unreachable!()
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

/// USD: `$0.0123` below one dollar, `$3.41`, `$1,234.56`, `$123k` for very large values.
pub fn usd(v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    let a = v.abs();
    let sign = if v < 0.0 { "-" } else { "" };
    if a == 0.0 {
        "$0.00".into()
    } else if a < 1.0 {
        format!("{sign}${a:.4}")
    } else if a < 100_000.0 {
        let whole = int(a.trunc());
        let cents = ((a.fract() * 100.0).round() as i64).min(99);
        format!("{sign}${whole}.{cents:02}")
    } else {
        format!("{sign}${}", compact(a))
    }
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
    if a >= 1000.0 {
        compact(v)
    } else if a >= 100.0 {
        format!("{v:.0}")
    } else if a >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    }
}

/// Latency/durations in milliseconds: `123ms`, `1.23s`, `12.3s`, `1m 05s`, `2h 03m`.
pub fn ms(v: f64) -> String {
    if !v.is_finite() || v < 0.0 {
        return "--".into();
    }
    if v < 1000.0 {
        return format!("{}ms", v.round() as i64);
    }
    let s = v / 1000.0;
    if s < 10.0 {
        return format!("{s:.2}s");
    }
    if s < 60.0 {
        return format!("{s:.1}s");
    }
    secs(s)
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

/// Redact a server-provided API key label so only a short prefix remains.
///
/// The Keeper masks keys as `abc*********last6`; we drop everything after the
/// prefix so the tail of the real key is never shown. Long opaque strings that
/// look like raw keys are cut to six characters.
pub fn redact_key(s: &str) -> String {
    let s = s.trim();
    if let Some(star) = s.find('*') {
        let prefix: String = s[..star].chars().take(6).collect();
        return format!("{prefix}…");
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

/// Mask an e-mail address for screenshots: `alice@example.com` → `a…@example.com`.
pub fn mask_email(s: &str) -> String {
    match s.split_once('@') {
        Some((local, domain)) if !local.is_empty() => {
            let first: String = local.chars().take(1).collect();
            let dom = domain
                .split_once('.')
                .map(|(d, tld)| {
                    let f: String = d.chars().take(1).collect();
                    format!("{f}….{tld}")
                })
                .unwrap_or_else(|| "…".into());
            format!("{first}…@{dom}")
        }
        _ => s.to_string(),
    }
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
    }

    #[test]
    fn durations() {
        assert_eq!(ms(123.4), "123ms");
        assert_eq!(ms(1234.0), "1.23s");
        assert_eq!(ms(12_345.0), "12.3s");
        assert_eq!(ms(65_000.0), "1m 05s");
        assert_eq!(ms(7_380_000.0), "2h 03m");
        assert_eq!(ms(-1.0), "--");
        assert_eq!(age(5), "5s");
        assert_eq!(age(180), "3m");
        assert_eq!(age(7200), "2h");
    }

    #[test]
    fn rates_and_percent() {
        assert_eq!(per_minute(152.4), "152");
        assert_eq!(per_minute(12.34), "12.3");
        assert_eq!(per_minute(1.254), "1.25");
        assert_eq!(pct(97.531, 2), "97.53%");
        assert_eq!(ratio_pct(1.0, 0.0), None);
        assert_eq!(tps(83.38), "83.4 t/s");
    }

    #[test]
    fn redaction() {
        assert_eq!(redact_key("xyz*********abcdef"), "xyz…");
        assert_eq!(redact_key("my-alias"), "my-alias");
        assert_eq!(redact_key("sk-abcdefghijklmnopqrstuvwxyz"), "sk-abc…");
        assert_eq!(redact_key("legacy:0123456789abcdef"), "legacy:012345…");
        assert_eq!(mask_email("alice@example.com"), "a…@e….com");
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
