//! Time ranges and realtime windows understood by the Keeper API.
//!
//! Besides the presets (`today`, `yesterday`, rolling `5h..24h` and `1d..30d`)
//! the server accepts `range=custom` with either inclusive calendar days
//! (`unit=day&start=YYYY-MM-DD&end=YYYY-MM-DD`) or hour-aligned RFC 3339
//! timestamps (`unit=hour`, 5..24 hourly slots within the last 24 hours).
//! Days and hours are in the *server's* time zone, so custom ranges carry the
//! server offset they were built with.

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveTime, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// The `range` query parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum TimeRange {
    Today,
    Yesterday,
    /// Rolling hours, 5..=24.
    Hours(u8),
    /// Rolling days, 1..=30.
    Days(u8),
    /// `range=custom`.
    Custom(Custom),
}

/// A custom range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Custom {
    /// Inclusive calendar days in the server's time zone.
    Days { start: NaiveDate, end: NaiveDate },
    /// Inclusive hourly slots: `start` and `end` are hour starts (server offset).
    Hours {
        start: DateTime<FixedOffset>,
        end: DateTime<FixedOffset>,
    },
}

/// Server limits for custom ranges (see `API.md`, "Common time-range query params").
pub mod limits {
    /// Longest custom day range on overview, activity and analysis.
    pub const MAX_DAYS: i64 = 365;
    /// Longest custom day range on the request log.
    pub const MAX_DAYS_EVENTS: i64 = 90;
    pub const MIN_HOURS: i64 = 5;
    pub const MAX_HOURS: i64 = 24;
}

/// The ranges cycled with `[`, `]` and `r`.
pub const PRESETS: [TimeRange; 9] = [
    TimeRange::Today,
    TimeRange::Yesterday,
    TimeRange::Hours(5),
    TimeRange::Hours(8),
    TimeRange::Hours(12),
    TimeRange::Hours(24),
    TimeRange::Days(7),
    TimeRange::Days(14),
    TimeRange::Days(30),
];

impl TimeRange {
    /// Query parameters for this range. `1d` is sent as `today`, like the web app.
    pub fn params(self) -> Vec<(&'static str, String)> {
        match self {
            TimeRange::Custom(c) => c.params(),
            r => vec![("range", r.preset_param())],
        }
    }

    fn preset_param(self) -> String {
        match self {
            TimeRange::Today | TimeRange::Days(1) => "today".into(),
            TimeRange::Yesterday => "yesterday".into(),
            TimeRange::Hours(h) => format!("{h}h"),
            TimeRange::Days(d) => format!("{d}d"),
            TimeRange::Custom(_) => "custom".into(),
        }
    }

    /// A stable key identifying the range (used in poller scopes).
    pub fn key(self) -> String {
        self.to_string()
    }

    /// Whether the server answers with daily buckets for this range.
    pub fn is_daily(self) -> bool {
        match self {
            TimeRange::Days(d) => d >= 7,
            TimeRange::Custom(Custom::Days { start, end }) => (end - start).num_days() >= 6,
            _ => false,
        }
    }

    /// Length of the range in minutes, the way the server computes RPM, TPM
    /// and daily averages: ranges ending today count only the time elapsed
    /// so far; rolling ranges are exactly N hours or days.
    pub fn window_minutes(self, now: DateTime<FixedOffset>) -> f64 {
        let off = *now.offset();
        let midnight = |d: NaiveDate| {
            off.from_local_datetime(&d.and_time(NaiveTime::MIN))
                .single()
                .unwrap_or(now)
        };
        let elapsed =
            |from: DateTime<FixedOffset>| (now - from).num_seconds().max(60) as f64 / 60.0;
        let today = now.date_naive();
        match self {
            TimeRange::Today | TimeRange::Days(1) => elapsed(midnight(today)),
            TimeRange::Yesterday => 1440.0,
            TimeRange::Hours(h) => h as f64 * 60.0,
            TimeRange::Days(d) => d as f64 * 1440.0,
            TimeRange::Custom(Custom::Days { start, end }) if end >= today => {
                elapsed(midnight(start))
            }
            TimeRange::Custom(Custom::Days { start, end }) => {
                ((end - start).num_days() + 1) as f64 * 1440.0
            }
            TimeRange::Custom(Custom::Hours { start, end }) => {
                let stop = (end + Duration::hours(1)).min(now);
                ((stop - start).num_seconds().max(60)) as f64 / 60.0
            }
        }
    }

    /// The span the range covers, `[start, end)`, in the server's zone:
    /// calendar ranges run midnight to midnight (today's end is tonight),
    /// rolling ranges end now.
    pub fn bounds(
        self,
        now: DateTime<FixedOffset>,
    ) -> (DateTime<FixedOffset>, DateTime<FixedOffset>) {
        let off = *now.offset();
        let midnight = |d: NaiveDate| {
            off.from_local_datetime(&d.and_time(NaiveTime::MIN))
                .single()
                .unwrap_or(now)
        };
        let today = now.date_naive();
        match self {
            TimeRange::Today | TimeRange::Days(1) => {
                (midnight(today), midnight(today + Duration::days(1)))
            }
            TimeRange::Yesterday => (midnight(today - Duration::days(1)), midnight(today)),
            TimeRange::Hours(h) => (now - Duration::hours(h as i64), now),
            TimeRange::Days(d) => (now - Duration::days(d as i64), now),
            TimeRange::Custom(Custom::Days { start, end }) => {
                (midnight(start), midnight(end + Duration::days(1)))
            }
            TimeRange::Custom(Custom::Hours { start, end }) => (start, end + Duration::hours(1)),
        }
    }

    pub fn is_custom(self) -> bool {
        matches!(self, TimeRange::Custom(_))
    }

    /// Step through [`PRESETS`]; custom values continue from `today`.
    pub fn step(self, forward: bool) -> TimeRange {
        let idx = PRESETS.iter().position(|r| *r == self);
        let n = PRESETS.len();
        let next = match (idx, forward) {
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
            (None, _) => 0,
        };
        PRESETS[next]
    }

    /// Short human label (`today`, `8h`, `11-01→11-07`, `11-13 10:00→17:00`).
    pub fn label(self) -> String {
        match self {
            TimeRange::Custom(Custom::Days { start, end }) if start == end => {
                start.format("%m-%d").to_string()
            }
            TimeRange::Custom(Custom::Days { start, end }) => {
                format!("{}→{}", start.format("%m-%d"), end.format("%m-%d"))
            }
            TimeRange::Custom(Custom::Hours { start, end }) => {
                let last = end + Duration::hours(1);
                if start.date_naive() == end.date_naive() {
                    format!(
                        "{} {}→{}",
                        start.format("%m-%d"),
                        start.format("%H:%M"),
                        last.format("%H:%M")
                    )
                } else {
                    format!("{}→{}", start.format("%m-%d %H:%M"), last.format("%H:%M"))
                }
            }
            r => r.to_string(),
        }
    }
}

impl Custom {
    pub fn params(self) -> Vec<(&'static str, String)> {
        match self {
            Custom::Days { start, end } => vec![
                ("range", "custom".into()),
                ("unit", "day".into()),
                ("start", start.format("%Y-%m-%d").to_string()),
                ("end", end.format("%Y-%m-%d").to_string()),
            ],
            Custom::Hours { start, end } => vec![
                ("range", "custom".into()),
                ("unit", "hour".into()),
                ("start", start.to_rfc3339()),
                ("end", end.to_rfc3339()),
            ],
        }
    }

    /// Validate against the server's rules at `now` (in the server offset).
    /// `max_days` is [`limits::MAX_DAYS`] or [`limits::MAX_DAYS_EVENTS`].
    pub fn validate(self, now: DateTime<FixedOffset>, max_days: i64) -> Result<(), String> {
        match self {
            Custom::Days { start, end } => {
                let today = now.date_naive();
                let count = (end - start).num_days() + 1;
                if count < 1 {
                    return Err("start must not be after end".into());
                }
                if count > max_days {
                    return Err(format!("at most {max_days} days"));
                }
                if end > today {
                    return Err("the range cannot end in the future".into());
                }
                if (today - start).num_days() >= max_days {
                    return Err(format!("must lie within the last {max_days} days"));
                }
                Ok(())
            }
            Custom::Hours { start, end } => {
                if start.minute() != 0 || end.minute() != 0 || start.second() != 0 {
                    return Err("hours must start at :00".into());
                }
                let slots = (end - start).num_hours() + 1;
                if end < start {
                    return Err("start must not be after end".into());
                }
                if !(limits::MIN_HOURS..=limits::MAX_HOURS).contains(&slots) {
                    return Err(format!(
                        "pick {}–{} hours (got {slots})",
                        limits::MIN_HOURS,
                        limits::MAX_HOURS
                    ));
                }
                let current = hour_floor(now);
                if end > current {
                    return Err("the range cannot end in the future".into());
                }
                if start < current - Duration::hours(23) {
                    return Err("hour ranges must start within the last 24 hours".into());
                }
                Ok(())
            }
        }
    }

    /// Shift the range so it ends now (today / the current hour), keeping its
    /// length — what the web app does after a `409` bounds conflict.
    pub fn clamp(self, now: DateTime<FixedOffset>, max_days: i64) -> Custom {
        match self {
            Custom::Days { start, end } => {
                let len = ((end - start).num_days() + 1).clamp(1, max_days);
                let today = now.date_naive();
                let end = end.min(today);
                let earliest = today - Duration::days(max_days - 1);
                let mut start = end - Duration::days(len - 1);
                if start < earliest {
                    start = earliest;
                }
                Custom::Days {
                    start,
                    end: end.max(start),
                }
            }
            Custom::Hours { start, end } => {
                let slots =
                    ((end - start).num_hours() + 1).clamp(limits::MIN_HOURS, limits::MAX_HOURS);
                let current = hour_floor(now);
                let end = end.min(current);
                let mut start = end - Duration::hours(slots - 1);
                let earliest = current - Duration::hours(23);
                if start < earliest {
                    start = earliest;
                }
                let end = (start + Duration::hours(slots - 1)).min(current);
                Custom::Hours { start, end }
            }
        }
    }

    /// The default custom day range: the last 7 days.
    pub fn default_days(now: DateTime<FixedOffset>) -> Custom {
        let end = now.date_naive();
        Custom::Days {
            start: end - Duration::days(6),
            end,
        }
    }

    /// The default custom hour range: the last 8 hourly slots.
    pub fn default_hours(now: DateTime<FixedOffset>) -> Custom {
        let end = hour_floor(now);
        Custom::Hours {
            start: end - Duration::hours(7),
            end,
        }
    }

    /// Parse user input for one end of a custom range. Days: `YYYY-MM-DD`
    /// or `MM-DD`; hours: `YYYY-MM-DD HH[:00]`, `MM-DD HH`, or `HH[:00]` (today).
    pub fn parse_day(s: &str, now: DateTime<FixedOffset>) -> Result<NaiveDate, String> {
        let s = s.trim();
        if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            return Ok(d);
        }
        if let Ok(d) = NaiveDate::parse_from_str(&format!("{}-{s}", now.year_ce().1), "%Y-%m-%d") {
            return Ok(d);
        }
        Err(format!("{s:?} is not a date (YYYY-MM-DD)"))
    }

    pub fn parse_hour(
        s: &str,
        now: DateTime<FixedOffset>,
    ) -> Result<DateTime<FixedOffset>, String> {
        let s = s.trim();
        let bad = || format!("{s:?} is not an hour (YYYY-MM-DD HH:00)");
        let (date, time) = match s.rsplit_once(' ') {
            Some((d, t)) => (Some(d), t),
            None => (None, s),
        };
        let date = match date {
            Some(d) => Custom::parse_day(d, now).map_err(|_| bad())?,
            None => now.date_naive(),
        };
        let time = time.trim();
        let hour: u32 = match time.split_once(':') {
            Some((h, m)) => {
                if !m.trim_start_matches('0').is_empty() {
                    return Err(format!("{s:?}: hours must start at :00"));
                }
                h.parse().map_err(|_| bad())?
            }
            None => time.parse().map_err(|_| bad())?,
        };
        let t = NaiveTime::from_hms_opt(hour, 0, 0).ok_or_else(bad)?;
        now.offset()
            .from_local_datetime(&date.and_time(t))
            .single()
            .ok_or_else(bad)
    }
}

use chrono::Datelike;

fn hour_floor(t: DateTime<FixedOffset>) -> DateTime<FixedOffset> {
    t.with_minute(0)
        .and_then(|t| t.with_second(0))
        .and_then(|t| t.with_nanosecond(0))
        .unwrap_or(t)
}

/// "Now" in a given server offset.
pub fn server_now(utc: DateTime<Utc>, offset: FixedOffset) -> DateTime<FixedOffset> {
    utc.with_timezone(&offset)
}

impl fmt::Display for TimeRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TimeRange::Today => f.write_str("today"),
            TimeRange::Yesterday => f.write_str("yesterday"),
            TimeRange::Hours(h) => write!(f, "{h}h"),
            TimeRange::Days(d) => write!(f, "{d}d"),
            TimeRange::Custom(Custom::Days { start, end }) => write!(
                f,
                "custom:{}..{}",
                start.format("%Y-%m-%d"),
                end.format("%Y-%m-%d")
            ),
            TimeRange::Custom(Custom::Hours { start, end }) => {
                write!(f, "custom:{}..{}", start.to_rfc3339(), end.to_rfc3339())
            }
        }
    }
}

impl FromStr for TimeRange {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim().to_ascii_lowercase();
        match s.as_str() {
            "today" => return Ok(TimeRange::Today),
            "yesterday" => return Ok(TimeRange::Yesterday),
            _ => {}
        }
        let err = || {
            format!(
                "unsupported range {s:?} (use today, yesterday, 5h..24h, 1d..30d or \
                 custom:YYYY-MM-DD..YYYY-MM-DD)"
            )
        };
        if let Some(rest) = s.strip_prefix("custom:") {
            let (a, b) = rest.split_once("..").ok_or_else(err)?;
            if let (Ok(start), Ok(end)) = (
                NaiveDate::parse_from_str(a, "%Y-%m-%d"),
                NaiveDate::parse_from_str(b, "%Y-%m-%d"),
            ) {
                if end < start {
                    return Err(err());
                }
                return Ok(TimeRange::Custom(Custom::Days { start, end }));
            }
            let start = DateTime::parse_from_rfc3339(&a.to_uppercase()).map_err(|_| err())?;
            let end = DateTime::parse_from_rfc3339(&b.to_uppercase()).map_err(|_| err())?;
            if end < start {
                return Err(err());
            }
            return Ok(TimeRange::Custom(Custom::Hours { start, end }));
        }
        if let Some(n) = s.strip_suffix('h') {
            let n: u8 = n.parse().map_err(|_| err())?;
            if (5..=24).contains(&n) {
                return Ok(TimeRange::Hours(n));
            }
        } else if let Some(n) = s.strip_suffix('d') {
            let n: u8 = n.parse().map_err(|_| err())?;
            if (1..=30).contains(&n) {
                return Ok(TimeRange::Days(n));
            }
        }
        Err(err())
    }
}

impl TryFrom<String> for TimeRange {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<TimeRange> for String {
    fn from(r: TimeRange) -> String {
        r.to_string()
    }
}

/// Realtime window (`window` query parameter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum RtWindow {
    #[default]
    #[serde(rename = "15m")]
    M15,
    #[serde(rename = "30m")]
    M30,
    #[serde(rename = "60m")]
    M60,
}

impl RtWindow {
    pub fn param(self) -> &'static str {
        match self {
            RtWindow::M15 => "15m",
            RtWindow::M30 => "30m",
            RtWindow::M60 => "60m",
        }
    }
    pub fn next(self) -> RtWindow {
        match self {
            RtWindow::M15 => RtWindow::M30,
            RtWindow::M30 => RtWindow::M60,
            RtWindow::M60 => RtWindow::M15,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ist(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn parse_and_params() {
        assert_eq!("today".parse::<TimeRange>(), Ok(TimeRange::Today));
        assert_eq!("8h".parse::<TimeRange>(), Ok(TimeRange::Hours(8)));
        assert_eq!("30d".parse::<TimeRange>(), Ok(TimeRange::Days(30)));
        assert!("4h".parse::<TimeRange>().is_err());
        assert!("31d".parse::<TimeRange>().is_err());
        assert_eq!(TimeRange::Days(1).params(), vec![("range", "today".into())]);
        assert_eq!(TimeRange::Hours(12).params(), vec![("range", "12h".into())]);
    }

    #[test]
    fn custom_round_trips_and_params() {
        let r: TimeRange = "custom:2026-11-01..2026-11-07".parse().unwrap();
        assert_eq!(r.to_string(), "custom:2026-11-01..2026-11-07");
        assert_eq!(r.label(), "11-01→11-07");
        assert_eq!(
            r.params(),
            vec![
                ("range", "custom".to_string()),
                ("unit", "day".into()),
                ("start", "2026-11-01".into()),
                ("end", "2026-11-07".into())
            ]
        );
        let h = TimeRange::Custom(Custom::Hours {
            start: ist("2026-11-13T10:00:00+05:30"),
            end: ist("2026-11-13T16:00:00+05:30"),
        });
        let back: TimeRange = h.to_string().parse().unwrap();
        assert_eq!(back, h);
        assert_eq!(h.label(), "11-13 10:00→17:00");
        assert_eq!(h.params()[2], ("start", "2026-11-13T10:00:00+05:30".into()));
        assert!(
            "custom:2026-11-07..2026-11-01"
                .parse::<TimeRange>()
                .is_err()
        );
    }

    #[test]
    fn custom_validation_follows_server_limits() {
        let now = ist("2026-11-13T16:40:00+05:30");
        let days = |a, b| Custom::Days {
            start: d(a),
            end: d(b),
        };
        assert!(days("2026-11-01", "2026-11-13").validate(now, 365).is_ok());
        assert!(days("2026-11-13", "2026-11-14").validate(now, 365).is_err());
        assert!(days("2026-11-07", "2026-11-01").validate(now, 365).is_err());
        assert!(days("2026-07-01", "2026-11-13").validate(now, 90).is_err());
        assert!(days("2026-07-01", "2026-11-13").validate(now, 365).is_ok());
        assert!(days("2025-11-13", "2025-11-14").validate(now, 365).is_err());
        let hours = |a, b| Custom::Hours {
            start: ist(a),
            end: ist(b),
        };
        assert!(
            hours("2026-11-13T10:00:00+05:30", "2026-11-13T16:00:00+05:30")
                .validate(now, 365)
                .is_ok()
        );
        // Ends in the future (17:00 slot has not started).
        assert!(
            hours("2026-11-13T10:00:00+05:30", "2026-11-13T17:00:00+05:30")
                .validate(now, 365)
                .is_err()
        );
        // Fewer than five slots.
        assert!(
            hours("2026-11-13T13:00:00+05:30", "2026-11-13T16:00:00+05:30")
                .validate(now, 365)
                .is_err()
        );
        // Starts more than 23 hours before the current hour.
        assert!(
            hours("2026-11-12T16:00:00+05:30", "2026-11-12T22:00:00+05:30")
                .validate(now, 365)
                .is_err()
        );
    }

    #[test]
    fn clamp_keeps_length_and_ends_now() {
        let now = ist("2026-11-13T16:40:00+05:30");
        let c = Custom::Days {
            start: d("2026-11-10"),
            end: d("2026-11-16"),
        }
        .clamp(now, 365);
        assert_eq!(
            c,
            Custom::Days {
                start: d("2026-11-07"),
                end: d("2026-11-13")
            }
        );
        let h = Custom::Hours {
            start: ist("2026-11-12T08:00:00+05:30"),
            end: ist("2026-11-12T15:00:00+05:30"),
        }
        .clamp(now, 365);
        assert!(h.validate(now, 365).is_ok(), "{h:?}");
        assert_eq!(
            h,
            Custom::Hours {
                start: ist("2026-11-12T17:00:00+05:30"),
                end: ist("2026-11-13T00:00:00+05:30"),
            }
        );
    }

    #[test]
    fn parses_user_input() {
        let now = ist("2026-11-13T16:40:00+05:30");
        assert_eq!(Custom::parse_day("11-02", now), Ok(d("2026-11-02")));
        assert_eq!(
            Custom::parse_hour("2026-11-13 09:00", now),
            Ok(ist("2026-11-13T09:00:00+05:30"))
        );
        assert_eq!(
            Custom::parse_hour("9", now),
            Ok(ist("2026-11-13T09:00:00+05:30"))
        );
        assert!(Custom::parse_hour("9:30", now).is_err());
        assert!(Custom::parse_day("nope", now).is_err());
    }

    #[test]
    fn stepping_wraps() {
        assert_eq!(TimeRange::Today.step(true), TimeRange::Yesterday);
        assert_eq!(TimeRange::Today.step(false), TimeRange::Days(30));
        assert_eq!(TimeRange::Days(30).step(true), TimeRange::Today);
        assert_eq!(RtWindow::M60.next(), RtWindow::M15);
    }

    #[test]
    fn window_minutes_follow_the_server() {
        let now: DateTime<FixedOffset> = "2026-11-13T19:20:00+05:30".parse().unwrap();
        assert_eq!(TimeRange::Today.window_minutes(now), 19.0 * 60.0 + 20.0);
        assert_eq!(TimeRange::Yesterday.window_minutes(now), 1440.0);
        assert_eq!(TimeRange::Hours(8).window_minutes(now), 480.0);
        assert_eq!(TimeRange::Days(30).window_minutes(now), 30.0 * 1440.0);
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        let wk = TimeRange::Custom(Custom::Days {
            start: d("2026-11-07"),
            end: d("2026-11-13"),
        });
        assert_eq!(wk.window_minutes(now), 6.0 * 1440.0 + 19.0 * 60.0 + 20.0);
        let past = TimeRange::Custom(Custom::Days {
            start: d("2026-11-01"),
            end: d("2026-11-07"),
        });
        assert_eq!(past.window_minutes(now), 7.0 * 1440.0);
    }
}
