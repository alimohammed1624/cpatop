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
pub fn until_now(buckets: &[String], now: DateTime<Utc>) -> usize {
    buckets
        .iter()
        .position(|b| {
            let start = if b.len() == 10 {
                label_day(b).and_then(|d| d.and_hms_opt(0, 0, 0).map(|t| t.and_utc()))
            } else {
                parse_ts(b).map(|t| t.with_timezone(&Utc))
            };
            start.is_some_and(|s| s > now)
        })
        .unwrap_or(buckets.len())
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
        assert_eq!(a.token_usage[0].bucket, "2026-11-12T23:30:00+05:30");
        assert_eq!(
            a.token_usage.last().unwrap().bucket,
            "2026-11-13T18:30:00+05:30"
        );
        assert_eq!(a.token_usage.len(), 20);
        assert_eq!(a.token_usage[4].total_tokens, 7.0);
        assert_eq!(a.token_usage[19].total_tokens, 9.0);
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
    fn future_buckets_are_cut() {
        let b: Vec<String> = [
            "2026-11-13T17:30:00+05:30",
            "2026-11-13T18:30:00+05:30",
            "2026-11-13T19:30:00+05:30",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(until_now(&b, now("2026-11-13T13:50:00Z")), 2);
    }
}
