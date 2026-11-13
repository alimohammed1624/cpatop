//! Modal overlays: help, pickers (API key, model, source, range) and the
//! custom range editor.

use crate::range::{Custom, TimeRange};

/// What a picker selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickKind {
    ApiKey,
    Model,
    Source,
    Range,
}

/// One entry of a picker. `value` is `None` for the "all …" entry.
#[derive(Debug, Clone, PartialEq)]
pub struct PickItem {
    pub value: Option<String>,
    pub label: String,
    /// Dim text after the label (ids, usage, hints).
    pub detail: String,
    /// The currently active choice.
    pub current: bool,
}

impl PickItem {
    pub fn new(value: Option<String>, label: impl Into<String>, detail: impl Into<String>) -> Self {
        PickItem {
            value,
            label: label.into(),
            detail: detail.into(),
            current: false,
        }
    }
}

/// A searchable list: type to filter, ↑↓ to move, Enter to pick, Esc to cancel.
#[derive(Debug, Clone, PartialEq)]
pub struct Picker {
    pub kind: PickKind,
    pub title: String,
    pub items: Vec<PickItem>,
    pub query: String,
    /// Index into [`Picker::visible`].
    pub selected: usize,
}

impl Picker {
    pub fn new(kind: PickKind, title: impl Into<String>, items: Vec<PickItem>) -> Picker {
        let selected = items.iter().position(|i| i.current).unwrap_or(0);
        Picker {
            kind,
            title: title.into(),
            items,
            query: String::new(),
            selected,
        }
    }

    /// Indices of the items matching the query (case-insensitive substring
    /// on label and detail; every whitespace-separated word must match).
    pub fn visible(&self) -> Vec<usize> {
        let words: Vec<String> = self
            .query
            .split_whitespace()
            .map(|w| w.to_lowercase())
            .collect();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, it)| {
                let hay = format!("{} {}", it.label, it.detail).to_lowercase();
                words.iter().all(|w| hay.contains(w.as_str()))
            })
            .map(|(i, _)| i)
            .collect()
    }

    pub fn move_by(&mut self, delta: isize) {
        let n = self.visible().len();
        if n == 0 {
            self.selected = 0;
            return;
        }
        let cur = self.selected.min(n - 1) as isize;
        self.selected = (cur + delta).clamp(0, n as isize - 1) as usize;
    }

    pub fn type_char(&mut self, c: char) {
        self.query.push(c);
        self.selected = 0;
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.selected = 0;
    }

    /// The highlighted item, if any matches.
    pub fn chosen(&self) -> Option<&PickItem> {
        let v = self.visible();
        v.get(self.selected.min(v.len().saturating_sub(1)))
            .map(|i| &self.items[*i])
    }
}

/// Unit of a custom range being edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Day,
    Hour,
}

/// Two-field editor for a custom range.
#[derive(Debug, Clone, PartialEq)]
pub struct RangeEditor {
    pub unit: Unit,
    pub start: String,
    pub end: String,
    /// 0 = start, 1 = end.
    pub field: usize,
    pub error: Option<String>,
    /// Longest allowed day span on the current tab.
    pub max_days: i64,
}

impl RangeEditor {
    pub fn field_mut(&mut self) -> &mut String {
        if self.field == 0 {
            &mut self.start
        } else {
            &mut self.end
        }
    }

    /// Pre-filled editor for `unit`, starting from the current range when it
    /// is a custom range of the same unit.
    pub fn new(
        unit: Unit,
        current: TimeRange,
        now: chrono::DateTime<chrono::FixedOffset>,
        max_days: i64,
    ) -> RangeEditor {
        let c = match (unit, current) {
            (Unit::Day, TimeRange::Custom(c @ Custom::Days { .. })) => c,
            (Unit::Hour, TimeRange::Custom(c @ Custom::Hours { .. })) => c,
            (Unit::Day, _) => Custom::default_days(now),
            (Unit::Hour, _) => Custom::default_hours(now),
        };
        let (start, end) = match c {
            Custom::Days { start, end } => (
                start.format("%Y-%m-%d").to_string(),
                end.format("%Y-%m-%d").to_string(),
            ),
            Custom::Hours { start, end } => (
                start.format("%Y-%m-%d %H:00").to_string(),
                end.format("%Y-%m-%d %H:00").to_string(),
            ),
        };
        RangeEditor {
            unit,
            start,
            end,
            field: 0,
            error: None,
            max_days,
        }
    }

    /// Parse and validate the fields.
    pub fn build(&self, now: chrono::DateTime<chrono::FixedOffset>) -> Result<Custom, String> {
        let c = match self.unit {
            Unit::Day => Custom::Days {
                start: Custom::parse_day(&self.start, now)?,
                end: Custom::parse_day(&self.end, now)?,
            },
            Unit::Hour => Custom::Hours {
                start: Custom::parse_hour(&self.start, now)?,
                end: Custom::parse_hour(&self.end, now)?,
            },
        };
        c.validate(now, self.max_days)?;
        Ok(c)
    }
}

/// The open overlay, if any.
#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    Help,
    Picker(Picker),
    Range(RangeEditor),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picker() -> Picker {
        let mut items = vec![
            PickItem::new(None, "all keys", ""),
            PickItem::new(Some("1".into()), "#1 dem…", "native"),
            PickItem::new(Some("2".into()), "#2 dem…", "native"),
            PickItem::new(Some("3".into()), "#3 ci-bot tst…", "native"),
        ];
        items[2].current = true;
        Picker::new(PickKind::ApiKey, "API key", items)
    }

    #[test]
    fn starts_on_current_and_filters() {
        let mut p = picker();
        assert_eq!(p.chosen().unwrap().value.as_deref(), Some("2"));
        p.type_char('c');
        p.type_char('i');
        assert_eq!(p.visible(), vec![3]);
        assert_eq!(p.chosen().unwrap().value.as_deref(), Some("3"));
        p.backspace();
        p.backspace();
        p.move_by(-10);
        assert_eq!(p.chosen().unwrap().value, None);
        p.move_by(10);
        assert_eq!(p.chosen().unwrap().value.as_deref(), Some("3"));
        p.query = "zzz".into();
        assert!(p.chosen().is_none());
    }

    #[test]
    fn multi_word_queries_match_all_words() {
        let mut p = picker();
        p.query = "#2 native".into();
        assert_eq!(p.visible(), vec![2]);
    }
}
