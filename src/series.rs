//! Calendar-aligned time axes for the Keeper's sparse series.
//!
//! The analysis endpoints return only buckets that have data (see API.md,
//! "Sparse buckets"), and daily buckets can carry odd timestamps (two
//! buckets on the same day). Drawn as-is, a 30-day range with data on two
//! days becomes a two-bar chart whose average divides by two. These helpers
//! rebuild a dense axis between the range start and end (clipped to now),
//! put every server bucket into its slot and fill the rest with zeros:
//!
//! * hourly axes step through **UTC hours** — the server cuts hourly buckets
//!   on UTC hours, so in a +05:30 zone they are labelled `HH:30`;
//! * daily axes step through local calendar days (midnight in the range's
//!   offset); several server buckets on the same day are summed.

use crate::api::models::{Analysis, TokenUsageBucket};
use crate::fmt::parse_ts;
use chrono::{DateTime, Duration, FixedOffset, NaiveDate, TimeZone, Utc};
use std::collections::BTreeMap;

/// Longest axis that is rebuilt; longer daily series are already dense and
/// downsampled by the server.
const MAX_SLOTS: usize = 400;

/// Slot key: UTC instant of the slot start.
type Key = DateTime<Utc>;

/// Start of the UTC hour containing `t`.
fn utc_hour(t: DateTime<Utc>) -> DateTime<Utc> {
    let s = t.timestamp();
    Utc.timestamp_opt(s - s.rem_euclid(3600), 0)
        .single()
        .unwrap_or(t)
}

/// Midnight of a calendar day in `off`, as a UTC instant.
fn midnight(d: NaiveDate, off: FixedOffset) -> Option<DateTime<Utc>> {
    off.from_local_datetime(&d.and_hms_opt(0, 0, 0)?)
        .single()
        .map(|t| t.with_timezone(&Utc))
}

/// The calendar day of a bucket label (`YYYY-MM-DD` or an RFC 3339 time in
/// its own offset).
fn label_day(s: &str) -> Option<NaiveDate> {
    if s.len() == 10 {
        return NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
    }
    parse_ts(s).map(|t| t.date_naive())
}

/// A dense axis: slot keys plus the label of each slot (RFC 3339 in `off`).
pub struct Axis {
    keys: Vec<Key>,
    pub labels: Vec<String>,
    daily: bool,
    off: FixedOffset,
}

impl Axis {
    /// Axis for `[start, end)` clipped to `now`, extended to cover every
    /// label in `extra` so no server bucket is ever dropped.
    pub fn build(
        start: DateTime<FixedOffset>,
        end: DateTime<FixedOffset>,
        daily: bool,
        now: DateTime<Utc>,
        extra: &[String],
    ) -> Option<Axis> {
        let off = *start.offset();
        let end = end.with_timezone(&Utc).min(now);
        let mut keys: Vec<Key> = Vec::new();
        if daily {
            let mut d = start.date_naive();
            let last = (end - Duration::nanoseconds(1))
                .with_timezone(&off)
                .date_naive();
            while d <= last && keys.len() <= MAX_SLOTS {
                keys.push(midnight(d, off)?);
                d = d.succ_opt()?;
            }
        } else {
            let mut t = utc_hour(start.with_timezone(&Utc));
            while t < end && keys.len() <= MAX_SLOTS {
                keys.push(t);
                t += Duration::hours(1);
            }
        }
        if keys.len() > MAX_SLOTS {
            return None;
        }
        let mut axis = Axis {
            keys,
            labels: vec![],
            daily,
            off,
        };
        for l in extra {
            if let Some(k) = axis.key_of(l)
                && !axis.keys.contains(&k)
            {
                axis.keys.push(k);
            }
        }
        axis.keys.sort();
        axis.labels = axis
            .keys
            .iter()
            .map(|k| k.with_timezone(&off).to_rfc3339())
            .collect();
        Some(axis)
    }

    /// Slot key of a server bucket label.
    fn key_of(&self, label: &str) -> Option<Key> {
        if self.daily {
            midnight(label_day(label)?, self.off)
        } else {
            parse_ts(label).map(|t| utc_hour(t.with_timezone(&Utc)))
        }
    }

    /// Index of a server bucket label on this axis.
    pub fn index(&self, label: &str) -> Option<usize> {
        let k = self.key_of(label)?;
        self.keys.binary_search(&k).ok()
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Spread `values` (aligned with `labels`) over the axis, summing values
    /// that share a slot and leaving zeros elsewhere.
    pub fn spread(&self, labels: &[String], values: &[f64]) -> Vec<f64> {
        let mut out = vec![0.0; self.len()];
        for (l, v) in labels.iter().zip(values) {
            if let Some(i) = self.index(l) {
                out[i] += v;
            }
        }
        out
    }
}

/// Rebuild the analysis series on a dense, calendar-aligned axis clipped to
/// `now`: `token_usage` and `model_usage` get one entry per hour (or day),
/// with zeros where the server returned nothing.
pub fn densify_analysis(a: &mut Analysis, now: DateTime<Utc>) {
    let (Some(start), Some(end)) = (
        a.range_start.as_deref().and_then(parse_ts),
        a.range_end.as_deref().and_then(parse_ts),
    ) else {
        return;
    };
    let daily = a.granularity == "daily";
    // Buckets outside the axis are kept only when they carry data (the
    // `today` series is zero-filled to midnight; those future zeros go).
    let extra: Vec<String> = a
        .token_usage
        .iter()
        .filter(|b| b.total_tokens > 0.0 || b.requests > 0.0)
        .map(|b| b.bucket.clone())
        .collect();
    let Some(axis) = Axis::build(start, end, daily, now, &extra) else {
        return;
    };
    // The slots between now and the range end (the rest of `today`).
    a.future_buckets = Axis::build(start, end, daily, end.with_timezone(&Utc), &[])
        .map(|full| {
            let last = axis.keys.last().copied();
            full.keys
                .iter()
                .zip(&full.labels)
                .filter(|(k, _)| last.is_none_or(|l| **k > l))
                .map(|(_, l)| l.clone())
                .collect()
        })
        .unwrap_or_default();
    // Token usage: sum buckets per slot.
    let mut slots: BTreeMap<usize, TokenUsageBucket> = BTreeMap::new();
    for b in &a.token_usage {
        let Some(i) = axis.index(&b.bucket) else {
            continue;
        };
        let s = slots.entry(i).or_insert_with(|| TokenUsageBucket {
            cost_available: true,
            ..Default::default()
        });
        s.input_tokens += b.input_tokens;
        s.output_tokens += b.output_tokens;
        s.cache_read_tokens += b.cache_read_tokens;
        s.cache_creation_tokens += b.cache_creation_tokens;
        s.reasoning_tokens += b.reasoning_tokens;
        s.total_tokens += b.total_tokens;
        s.requests += b.requests;
        s.cost_usd += b.cost_usd;
        s.cost_available &= b.cost_available;
    }
    a.token_usage = axis
        .labels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let mut b = slots.remove(&i).unwrap_or(TokenUsageBucket {
                cost_available: true,
                ..Default::default()
            });
            b.bucket = l.clone();
            b
        })
        .collect();
    // Per-model series, on the same axis.
    let old = std::mem::take(&mut a.model_usage.buckets);
    for s in &mut a.model_usage.series {
        s.total_tokens = axis.spread(&old, &s.total_tokens);
        s.requests = axis.spread(&old, &s.requests);
    }
    a.model_usage.buckets = axis.labels.clone();
    if !daily {
        let off = *start.offset();
        let mut labels: Vec<String> = a.token_usage.iter().map(|b| b.bucket.clone()).collect();
        clip_first(&mut labels, start.with_timezone(&off));
        for (b, l) in a.token_usage.iter_mut().zip(&labels) {
            b.bucket = l.clone();
        }
        a.model_usage.buckets = labels;
    }
}

/// The server buckets hourly series by UTC hour, so with a half-hour
/// offset (+05:30) a `today` axis starts with a bucket labelled 23:30 of
/// the previous day that only holds data from local midnight on. That
/// bucket is relabelled to the range start (`00:00`), which is what it
/// covers; nothing else changes.
pub fn clip_first(labels: &mut [String], start: DateTime<FixedOffset>) {
    let Some(first) = labels.first().and_then(|l| parse_ts(l)) else {
        return;
    };
    let second = labels.get(1).and_then(|l| parse_ts(l));
    if first < start && second.is_none_or(|s| s > start) {
        labels[0] = start.to_rfc3339();
    }
}

/// The first whole UTC hour at or after `t`, in `t`'s offset, when `t` is
/// not on one: where the server's first hourly bucket of a range starting
/// at `t` begins (00:30 for local midnight at +05:30). Its daily buckets
/// are cut there too.
pub fn utc_hour_cut(t: DateTime<FixedOffset>) -> Option<DateTime<FixedOffset>> {
    let s = t.timestamp();
    let r = s.rem_euclid(3600);
    if r == 0 && t.timestamp_subsec_nanos() == 0 {
        return None;
    }
    let cut = Utc.timestamp_opt(s - r + 3600, 0).single()?;
    Some(cut.with_timezone(t.offset()))
}

/// Where the buckets of an hourly series sit on a time axis `[start, end)`,
/// as fractions of its width. Each bucket covers its real span: the server
/// cuts hourly buckets on UTC hours, so in a +05:30 zone they run `HH:30` to
/// `HH+1:30`; the first one of a `today` axis covers only 00:00–00:30
/// (clipped at local midnight), the one in progress ends at now, and the
/// last one is cut at the axis end. Drawn on whole-hour slots instead,
/// every bar would sit 30–60 minutes late.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub start: DateTime<FixedOffset>,
    pub end: DateTime<FixedOffset>,
    /// `[start, end)` of each bucket, as fractions of the axis.
    pub spans: Vec<(f64, f64)>,
    /// Where now falls on the axis, when the axis runs past it.
    pub now: Option<f64>,
}

impl Placement {
    /// Placement of hourly buckets (RFC 3339 labels) on `[start, end)`.
    /// `None` when a label does not parse or the axis is empty.
    pub fn hourly(
        buckets: &[String],
        start: DateTime<FixedOffset>,
        end: DateTime<FixedOffset>,
        now: DateTime<Utc>,
    ) -> Option<Placement> {
        let total = (end - start).num_milliseconds() as f64;
        if total <= 0.0 || buckets.is_empty() {
            return None;
        }
        let starts: Vec<DateTime<FixedOffset>> =
            buckets.iter().map(|b| parse_ts(b)).collect::<Option<_>>()?;
        let now = now.with_timezone(start.offset());
        let frac = |t: DateTime<FixedOffset>| {
            ((t - start).num_milliseconds() as f64 / total).clamp(0.0, 1.0)
        };
        let spans = starts
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let mut e = *s + Duration::hours(1);
                if let Some(next) = starts.get(i + 1)
                    && *next > *s
                {
                    e = e.min(*next);
                }
                let s = (*s).max(start);
                let mut e = e.min(end);
                if s <= now && now < e {
                    e = now;
                }
                (frac(s), frac(e).max(frac(s)))
            })
            .collect();
        let now = (now > start && now < end).then(|| frac(now));
        Some(Placement {
            start,
            end,
            spans,
            now,
        })
    }

    /// Whole local hours on the axis: `(fraction, hour)`.
    pub fn hours(&self) -> Vec<(f64, u32)> {
        use chrono::Timelike;
        let total = (self.end - self.start).num_seconds() as f64;
        let mut t = self.start;
        let off = t.minute() as i64 * 60 + t.second() as i64;
        if off != 0 || t.nanosecond() != 0 {
            t = t - Duration::seconds(off) - Duration::nanoseconds(t.nanosecond() as i64)
                + Duration::hours(1);
        }
        let mut out = Vec::new();
        while t < self.end && out.len() < 24 * 40 {
            out.push(((t - self.start).num_seconds() as f64 / total, t.hour()));
            t += Duration::hours(1);
        }
        out
    }
}

/// The span an analysis response actually covers: the server's
/// `range_start` → `range_end`, clipped to `now`. Averages and rates divide
/// by this, not by the nominal range length (a `30d` range is calendar
/// aligned and runs from local midnight 30 days ago to tomorrow's midnight).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    pub start: DateTime<FixedOffset>,
    pub end: DateTime<FixedOffset>,
    /// The range runs past `now`, so `end` is now.
    pub to_now: bool,
}

impl Window {
    /// Window of an analysis response; `None` without bounds or when the
    /// range starts in the future.
    pub fn of(a: &Analysis, now: DateTime<Utc>) -> Option<Window> {
        let start = a.range_start.as_deref().and_then(parse_ts)?;
        let end = a.range_end.as_deref().and_then(parse_ts)?;
        let off = *start.offset();
        let now = now.with_timezone(&off);
        let to_now = end > now;
        let end = if to_now { now } else { end };
        (end > start).then_some(Window { start, end, to_now })
    }

    pub fn minutes(&self) -> f64 {
        ((self.end - self.start).num_seconds().max(60)) as f64 / 60.0
    }

    /// Length, `30.8d` or `19.3h`.
    pub fn length(&self) -> String {
        let m = self.minutes();
        if m >= 2.0 * 1440.0 {
            format!("{:.1}d", m / 1440.0)
        } else {
            format!("{:.1}h", m / 60.0)
        }
    }

    /// Bounds, `Oct 14 → now` or `00:00 → now` (times for spans under two
    /// days).
    pub fn bounds(&self) -> String {
        let pat = if self.minutes() >= 2.0 * 1440.0 {
            "%b %-d"
        } else {
            "%H:%M"
        };
        let end = if self.to_now {
            "now".to_string()
        } else {
            self.end.format(pat).to_string()
        };
        format!("{} → {end}", self.start.format(pat))
    }
}

/// Drop buckets that start after `now` (the `today` axes run to midnight).
/// Returns how many leading buckets to keep.
///
/// `now` must carry the server's offset: a date-only label (`YYYY-MM-DD`)
/// starts at local midnight in that offset, not at UTC midnight — in a
/// +05:30 zone the current day would otherwise be dropped from local
/// midnight until 05:30, and in a −08:00 zone tomorrow's (empty) bucket
/// would be kept from 16:00 on.
pub fn until_now(buckets: &[String], now: DateTime<FixedOffset>) -> usize {
    let off = *now.offset();
    buckets
        .iter()
        .position(|b| bucket_start(b, off).is_some_and(|s| s > now))
        .unwrap_or(buckets.len())
}

/// The instant a bucket label starts: RFC 3339 labels carry their own
/// offset, date-only labels start at midnight in `off` (the server zone).
pub fn bucket_start(label: &str, off: FixedOffset) -> Option<DateTime<FixedOffset>> {
    if label.len() == 10 {
        let d = NaiveDate::parse_from_str(label, "%Y-%m-%d").ok()?;
        midnight(d, off).map(|t| t.with_timezone(&off))
    } else {
        parse_ts(label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::{ModelUsage, ModelUsageSeries};

    fn tb(bucket: &str, tokens: f64) -> TokenUsageBucket {
        TokenUsageBucket {
            bucket: bucket.into(),
            total_tokens: tokens,
            requests: 1.0,
            cost_available: true,
            ..Default::default()
        }
    }

    fn now(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    #[test]
    fn thirty_days_with_two_active_days_get_thirty_one_slots() {
        // Live shape: a 30d range is calendar-aligned (31 local days) and the
        // server returned three buckets, two of them on the same day.
        let mut a = Analysis {
            granularity: "daily".into(),
            range_start: Some("2026-10-14T00:00:00+05:30".into()),
            range_end: Some("2026-11-14T00:00:00+05:30".into()),
            token_usage: vec![
                tb("2026-11-12T00:00:00+05:30", 0.5e6),
                tb("2026-11-12T10:30:00+05:30", 0.5e6),
                tb("2026-11-13T00:00:00+05:30", 388.5e6),
            ],
            model_usage: ModelUsage {
                buckets: vec![
                    "2026-11-12T00:00:00+05:30".into(),
                    "2026-11-12T10:30:00+05:30".into(),
                    "2026-11-13T00:00:00+05:30".into(),
                ],
                series: vec![ModelUsageSeries {
                    model: "m".into(),
                    total_tokens: vec![1.0, 2.0, 3.0],
                    requests: vec![1.0, 1.0, 1.0],
                }],
            },
            ..Default::default()
        };
        densify_analysis(&mut a, now("2026-11-13T13:50:00Z"));
        assert_eq!(a.token_usage.len(), 31);
        assert_eq!(a.token_usage[0].bucket, "2026-10-14T00:00:00+05:30");
        let last = &a.token_usage[30];
        assert_eq!(last.bucket, "2026-11-13T00:00:00+05:30");
        assert_eq!(last.total_tokens, 388.5e6);
        // The two buckets of 11-12 are one day.
        assert_eq!(a.token_usage[29].total_tokens, 1e6);
        assert_eq!(a.token_usage[29].requests, 2.0);
        assert!(a.token_usage[..29].iter().all(|b| b.total_tokens == 0.0));
        assert_eq!(a.model_usage.buckets.len(), 31);
        assert_eq!(a.model_usage.series[0].total_tokens[29], 3.0);
        assert_eq!(a.model_usage.series[0].total_tokens[30], 3.0);
        // Daily: today is the last slot, nothing is in the future.
        assert!(a.future_buckets.is_empty());
    }

    #[test]
    fn custom_week_has_one_bar_per_day() {
        let mut a = Analysis {
            granularity: "daily".into(),
            range_start: Some("2026-11-07T00:00:00+05:30".into()),
            range_end: Some("2026-11-14T00:00:00+05:30".into()),
            token_usage: vec![
                tb("2026-11-12T00:00:00+05:30", 1.0),
                tb("2026-11-12T10:30:00+05:30", 1.0),
                tb("2026-11-13T00:00:00+05:30", 5.0),
            ],
            ..Default::default()
        };
        densify_analysis(&mut a, now("2026-11-13T13:50:00Z"));
        let days: Vec<String> = a
            .token_usage
            .iter()
            .map(|b| b.bucket[5..10].to_string())
            .collect();
        assert_eq!(
            days,
            [
                "11-07", "11-08", "11-09", "11-10", "11-11", "11-12", "11-13"
            ]
        );
    }

    #[test]
    fn hourly_axis_is_utc_hour_aligned_and_clipped_to_now() {
        // `today` in +05:30: the first slot is 23:30 the day before (the UTC
        // hour containing local midnight); slots end with the current hour.
        let mut a = Analysis {
            granularity: "hourly".into(),
            range_start: Some("2026-11-13T00:00:00+05:30".into()),
            range_end: Some("2026-11-13T23:59:59.999999999+05:30".into()),
            token_usage: vec![
                tb("2026-11-13T03:30:00+05:30", 7.0),
                tb("2026-11-13T18:30:00+05:30", 9.0),
            ],
            ..Default::default()
        };
        // 19:20 local.
        densify_analysis(&mut a, now("2026-11-13T13:50:00Z"));
        // The first UTC-hour slot starts at 23:30 but only holds data from
        // midnight: it is labelled with the range start.
        assert_eq!(a.token_usage[0].bucket, "2026-11-13T00:00:00+05:30");
        assert_eq!(a.token_usage[1].bucket, "2026-11-13T00:30:00+05:30");
        assert_eq!(
            a.token_usage.last().unwrap().bucket,
            "2026-11-13T18:30:00+05:30"
        );
        assert_eq!(a.token_usage.len(), 20);
        assert_eq!(a.token_usage[4].total_tokens, 7.0);
        assert_eq!(a.token_usage[19].total_tokens, 9.0);
        // 19:30 … 23:30 are still to come.
        assert_eq!(a.future_buckets.len(), 5);
        assert_eq!(a.future_buckets[0], "2026-11-13T19:30:00+05:30");
        assert_eq!(a.future_buckets[4], "2026-11-13T23:30:00+05:30");
    }

    #[test]
    fn zero_filled_future_buckets_are_dropped() {
        let mut a = Analysis {
            granularity: "hourly".into(),
            range_start: Some("2026-11-13T00:00:00+05:30".into()),
            range_end: Some("2026-11-13T23:59:59.999999999+05:30".into()),
            token_usage: vec![
                tb("2026-11-13T18:30:00+05:30", 9.0),
                tb("2026-11-13T19:30:00+05:30", 0.0),
                tb("2026-11-13T23:30:00+05:30", 0.0),
            ],
            ..Default::default()
        };
        a.token_usage[1].requests = 0.0;
        a.token_usage[2].requests = 0.0;
        densify_analysis(&mut a, now("2026-11-13T13:50:00Z"));
        assert_eq!(
            a.token_usage.last().unwrap().bucket,
            "2026-11-13T18:30:00+05:30"
        );
    }

    #[test]
    fn rolling_hours_fill_gaps() {
        let mut a = Analysis {
            granularity: "hourly".into(),
            range_start: Some("2026-11-12T19:19:18.99+05:30".into()),
            range_end: Some("2026-11-13T19:19:18.99+05:30".into()),
            token_usage: vec![tb("2026-11-13T02:30:00+05:30", 1.0)],
            ..Default::default()
        };
        densify_analysis(&mut a, now("2026-11-13T13:50:00Z"));
        // 18:30 the day before … 18:30 today.
        assert_eq!(a.token_usage.len(), 25);
        assert_eq!(
            a.token_usage
                .iter()
                .filter(|b| b.total_tokens > 0.0)
                .count(),
            1
        );
    }

    #[test]
    fn analysis_window_uses_server_bounds_clipped_to_now() {
        // 30d: the server returns Oct 14 00:00 → Nov 14 00:00 (+05:30); at
        // 19:20 local on Nov 13 the data covers 30 days 19h 20m, not 30.
        let a = Analysis {
            range_start: Some("2026-10-14T00:00:00+05:30".into()),
            range_end: Some("2026-11-14T00:00:00+05:30".into()),
            ..Default::default()
        };
        let w = Window::of(&a, now("2026-11-13T13:50:00Z")).unwrap();
        assert!(w.to_now);
        assert_eq!(w.minutes(), 30.0 * 1440.0 + 19.0 * 60.0 + 20.0);
        assert_eq!(w.length(), "30.8d");
        assert_eq!(w.bounds(), "Oct 14 → now");
        // A past range keeps its own end.
        let past = Analysis {
            range_start: Some("2026-11-01T00:00:00+05:30".into()),
            range_end: Some("2026-11-08T00:00:00+05:30".into()),
            ..Default::default()
        };
        let w = Window::of(&past, now("2026-11-13T13:50:00Z")).unwrap();
        assert!(!w.to_now);
        assert_eq!(w.minutes(), 7.0 * 1440.0);
        assert_eq!(w.bounds(), "Nov 1 → Nov 8");
        // today: hours since local midnight.
        let today = Analysis {
            range_start: Some("2026-11-13T00:00:00+05:30".into()),
            range_end: Some("2026-11-13T23:59:59.999999999+05:30".into()),
            ..Default::default()
        };
        let w = Window::of(&today, now("2026-11-13T13:50:00Z")).unwrap();
        assert_eq!(w.length(), "19.3h");
        assert_eq!(w.bounds(), "00:00 → now");
        assert!(Window::of(&Analysis::default(), now("2026-11-13T13:50:00Z")).is_none());
    }

    #[test]
    fn first_partial_utc_bucket_is_relabelled_to_midnight() {
        let start = parse_ts("2026-11-13T00:00:00+05:30").unwrap();
        let mut l = vec![
            "2026-11-12T23:30:00+05:30".to_string(),
            "2026-11-13T00:30:00+05:30".to_string(),
        ];
        clip_first(&mut l, start);
        assert_eq!(parse_ts(&l[0]), Some(start));
        assert_eq!(l[1], "2026-11-13T00:30:00+05:30");
        // Whole-hour offsets are untouched.
        let mut l = vec!["2026-11-13T00:00:00+05:30".to_string()];
        clip_first(&mut l, start);
        assert_eq!(l[0], "2026-11-13T00:00:00+05:30");
    }

    #[test]
    fn future_buckets_are_cut() {
        let b: Vec<String> = [
            "2026-11-13T17:30:00+05:30",
            "2026-11-13T18:30:00+05:30",
            "2026-11-13T19:30:00+05:30",
        ]
        .map(String::from)
        .to_vec();
        let at = |s: &str| parse_ts(s).unwrap();
        assert_eq!(until_now(&b, at("2026-11-13T19:20:00+05:30")), 2);
        // The same instant seen in UTC changes nothing for RFC 3339 labels.
        assert_eq!(until_now(&b, at("2026-11-13T13:50:00+00:00")), 2);
    }

    fn days(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn date_only_buckets_start_at_server_midnight_ist() {
        // 7d in +05:30, live shape: the axis ends with today's date.
        let b = days(&["2026-11-12", "2026-11-13", "2026-11-14"]);
        let at = |s: &str| parse_ts(s).unwrap();
        // 00:30 local = 19:00 UTC the day before: today's bucket stays (the
        // old UTC-midnight reading dropped it until 05:30 local).
        assert_eq!(until_now(&b, at("2026-11-14T00:30:00+05:30")), 3);
        // Exactly local midnight: the bucket starts now, it is not future.
        assert_eq!(until_now(&b, at("2026-11-14T00:00:00+05:30")), 3);
        // One second before local midnight: 11-14 is still tomorrow.
        assert_eq!(until_now(&b, at("2026-11-13T23:59:59+05:30")), 2);
        // Around UTC midnight (05:30 local) nothing changes.
        assert_eq!(until_now(&b, at("2026-11-14T05:29:59+05:30")), 3);
        assert_eq!(until_now(&b, at("2026-11-14T05:30:00+05:30")), 3);
    }

    #[test]
    fn date_only_buckets_start_at_server_midnight_pst() {
        // −08:00: UTC midnight is 16:00 local the day before. Reading the
        // labels as UTC midnight kept tomorrow's empty bucket from 16:00.
        let b = days(&["2026-11-12", "2026-11-13", "2026-11-14"]);
        let at = |s: &str| parse_ts(s).unwrap();
        assert_eq!(until_now(&b, at("2026-11-13T16:30:00-08:00")), 2);
        assert_eq!(until_now(&b, at("2026-11-13T15:59:59-08:00")), 2);
        assert_eq!(until_now(&b, at("2026-11-13T23:59:59-08:00")), 2);
        assert_eq!(until_now(&b, at("2026-11-14T00:00:00-08:00")), 3);
        assert_eq!(until_now(&b, at("2026-11-14T00:00:01-08:00")), 3);
    }

    #[test]
    fn date_only_buckets_in_utc() {
        let b = days(&["2026-11-12", "2026-11-13", "2026-11-14"]);
        let at = |s: &str| parse_ts(s).unwrap();
        assert_eq!(until_now(&b, at("2026-11-13T23:59:59+00:00")), 2);
        assert_eq!(until_now(&b, at("2026-11-14T00:00:00+00:00")), 3);
        assert_eq!(until_now(&b, at("2026-11-14T00:00:01+00:00")), 3);
    }

    #[test]
    fn hourly_buckets_around_local_and_utc_midnight() {
        // `today` in +05:30 just after local midnight: UTC-hour buckets
        // (the first relabelled to 00:00) run to the end of the day.
        let b = days(&[
            "2026-11-14T00:00:00+05:30",
            "2026-11-14T00:30:00+05:30",
            "2026-11-14T01:30:00+05:30",
        ]);
        let at = |s: &str| parse_ts(s).unwrap();
        assert_eq!(until_now(&b, at("2026-11-14T00:05:00+05:30")), 1);
        assert_eq!(until_now(&b, at("2026-11-14T00:30:00+05:30")), 2);
        // −08:00 and UTC: on-the-hour buckets, now just past midnight.
        let b = days(&["2026-11-14T00:00:00-08:00", "2026-11-14T01:00:00-08:00"]);
        assert_eq!(until_now(&b, at("2026-11-14T00:01:00-08:00")), 1);
        assert_eq!(until_now(&b, at("2026-11-13T23:59:00-08:00")), 0);
        let b = days(&["2026-11-14T00:00:00+00:00", "2026-11-14T01:00:00+00:00"]);
        assert_eq!(until_now(&b, at("2026-11-14T00:00:00+00:00")), 1);
    }

    #[test]
    fn bucket_start_reads_dates_in_the_server_offset() {
        let ist = FixedOffset::east_opt(5 * 3600 + 1800).unwrap();
        let pst = FixedOffset::west_opt(8 * 3600).unwrap();
        assert_eq!(
            bucket_start("2026-11-14", ist).unwrap().to_rfc3339(),
            "2026-11-14T00:00:00+05:30"
        );
        assert_eq!(
            bucket_start("2026-11-14", pst).unwrap().to_rfc3339(),
            "2026-11-14T00:00:00-08:00"
        );
        // RFC 3339 labels keep their own offset.
        assert_eq!(
            bucket_start("2026-11-14T02:30:00+05:30", pst)
                .unwrap()
                .to_rfc3339(),
            "2026-11-14T02:30:00+05:30"
        );
        assert!(bucket_start("garbage!!!", ist).is_none());
    }

    fn hourly_labels(first: &str, off: &str, n: usize) -> Vec<String> {
        let t0: DateTime<FixedOffset> = DateTime::parse_from_rfc3339(first).unwrap();
        let off: FixedOffset = off.parse().unwrap();
        (0..n)
            .map(|i| {
                (t0 + Duration::hours(i as i64))
                    .with_timezone(&off)
                    .to_rfc3339()
            })
            .collect()
    }

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    #[test]
    fn half_hour_zone_buckets_sit_at_their_real_times() {
        // +05:30 at 02:45: the server cut hourly buckets at UTC hours, so
        // they read 23:30, 00:30, … 23:30; the first was clipped to 00:00.
        let mut labels = hourly_labels("2026-11-13T23:30:00+05:30", "+05:30", 25);
        let start: DateTime<FixedOffset> = "2026-11-14T00:00:00+05:30".parse().unwrap();
        let end: DateTime<FixedOffset> = "2026-11-15T00:00:00+05:30".parse().unwrap();
        clip_first(&mut labels, start);
        let p = Placement::hourly(&labels, start, end, now("2026-11-13T21:15:00Z")).unwrap();
        let h = |x: f64| x / 24.0;
        assert_eq!(p.spans.len(), 25);
        assert!(close(p.spans[0], (0.0, h(0.5))), "{:?}", p.spans[0]);
        assert!(close(p.spans[1], (h(0.5), h(1.5))), "{:?}", p.spans[1]);
        // The bucket in progress (02:30–03:30) ends at now.
        assert!(close(p.spans[3], (h(2.5), h(2.75))), "{:?}", p.spans[3]);
        assert!(close(p.spans[4], (h(3.5), h(4.5))));
        // The last one (23:30–00:30) is cut at midnight.
        assert!(close(p.spans[24], (h(23.5), 1.0)));
        assert!((p.now.unwrap() - h(2.75)).abs() < 1e-9);
        // Whole local hours for the ticks.
        let hours = p.hours();
        assert_eq!(hours.len(), 24);
        assert_eq!(hours[3].1, 3);
        assert!((hours[3].0 - h(3.0)).abs() < 1e-9);
    }

    #[test]
    fn whole_hour_zone_buckets_fill_whole_hours() {
        let labels = hourly_labels("2026-11-14T00:00:00+01:00", "+01:00", 24);
        let start: DateTime<FixedOffset> = "2026-11-14T00:00:00+01:00".parse().unwrap();
        let end: DateTime<FixedOffset> = "2026-11-15T00:00:00+01:00".parse().unwrap();
        let p = Placement::hourly(&labels, start, end, now("2026-11-14T01:45:00Z")).unwrap();
        let h = |x: f64| x / 24.0;
        assert!(close(p.spans[0], (0.0, h(1.0))));
        assert!(close(p.spans[2], (h(2.0), h(2.75))));
        assert!(close(p.spans[23], (h(23.0), 1.0)));
        assert!((p.now.unwrap() - h(2.75)).abs() < 1e-9);
        // A past day has no `now` on its axis.
        let p = Placement::hourly(&labels, start, end, now("2026-11-16T00:00:00Z")).unwrap();
        assert_eq!(p.now, None);
        assert!(close(p.spans[2], (h(2.0), h(3.0))));
    }

    #[test]
    fn a_bucket_starting_before_the_axis_is_clipped_to_it() {
        // A rolling range starts mid-bucket.
        let labels = hourly_labels("2026-11-14T09:30:00+05:30", "+05:30", 3);
        let start: DateTime<FixedOffset> = "2026-11-14T10:00:00+05:30".parse().unwrap();
        let end: DateTime<FixedOffset> = "2026-11-14T12:00:00+05:30".parse().unwrap();
        let p = Placement::hourly(&labels, start, end, now("2026-11-14T06:30:00Z")).unwrap();
        assert!(close(p.spans[0], (0.0, 0.25)));
        assert!(close(p.spans[1], (0.25, 0.75)));
        assert!(close(p.spans[2], (0.75, 1.0)));
    }

    #[test]
    fn utc_hour_cuts() {
        let t = |s: &str| DateTime::parse_from_rfc3339(s).unwrap();
        let cut = utc_hour_cut(t("2026-11-14T00:00:00+05:30")).unwrap();
        assert_eq!(cut.format("%H:%M").to_string(), "00:30");
        let cut = utc_hour_cut(t("2026-11-14T00:00:00+05:45")).unwrap();
        assert_eq!(cut.format("%H:%M").to_string(), "00:45");
        assert_eq!(utc_hour_cut(t("2026-11-14T00:00:00+01:00")), None);
    }
}
