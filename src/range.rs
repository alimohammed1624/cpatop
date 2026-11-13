//! Time ranges and realtime windows understood by the Keeper API.

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
    /// Value for the `range` query parameter. `1d` is sent as `today`, like the web app.
    pub fn param(self) -> String {
        match self {
            TimeRange::Today | TimeRange::Days(1) => "today".into(),
            TimeRange::Yesterday => "yesterday".into(),
            TimeRange::Hours(h) => format!("{h}h"),
            TimeRange::Days(d) => format!("{d}d"),
        }
    }

    /// Whether the server answers with daily buckets for this range.
    pub fn is_daily(self) -> bool {
        matches!(self, TimeRange::Days(d) if d >= 7)
    }

    /// Step through [`PRESETS`]; custom values snap to the nearest preset.
    pub fn step(self, forward: bool) -> TimeRange {
        let idx = PRESETS.iter().position(|r| *r == self).unwrap_or(0);
        let n = PRESETS.len();
        let next = if forward {
            (idx + 1) % n
        } else {
            (idx + n - 1) % n
        };
        PRESETS[next]
    }
}

impl fmt::Display for TimeRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TimeRange::Today => f.write_str("today"),
            TimeRange::Yesterday => f.write_str("yesterday"),
            TimeRange::Hours(h) => write!(f, "{h}h"),
            TimeRange::Days(d) => write!(f, "{d}d"),
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
        let err = || format!("unsupported range {s:?} (use today, yesterday, 5h..24h or 1d..30d)");
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

    #[test]
    fn parse_and_params() {
        assert_eq!("today".parse::<TimeRange>(), Ok(TimeRange::Today));
        assert_eq!("8h".parse::<TimeRange>(), Ok(TimeRange::Hours(8)));
        assert_eq!("30d".parse::<TimeRange>(), Ok(TimeRange::Days(30)));
        assert!("4h".parse::<TimeRange>().is_err());
        assert!("31d".parse::<TimeRange>().is_err());
        assert_eq!(TimeRange::Days(1).param(), "today");
        assert_eq!(TimeRange::Hours(12).param(), "12h");
    }

    #[test]
    fn stepping_wraps() {
        assert_eq!(TimeRange::Today.step(true), TimeRange::Yesterday);
        assert_eq!(TimeRange::Today.step(false), TimeRange::Days(30));
        assert_eq!(TimeRange::Days(30).step(true), TimeRange::Today);
        assert_eq!(RtWindow::M60.next(), RtWindow::M15);
    }
}
