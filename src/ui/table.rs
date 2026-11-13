//! A small table renderer: content-aware column widths, priority-based
//! column dropping on narrow panes, sorting, filtering and selection.
//!
//! Width allocation, in order:
//! 1. columns are dropped (highest `prio` number first) until the minimum
//!    widths fit;
//! 2. every column gets its minimum, then spare width goes to columns whose
//!    content is wider than their share (truncated text first, round-robin
//!    by priority) — text columns never grow past their longest cell, so
//!    numbers stay next to the names they describe;
//! 3. what is left widens [`Col::grow`] columns (meters), then the gaps.

use crate::app::TableState;
use crate::fmt;
use crate::theme::Theme;
use crate::ui::widgets;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::borrow::Cow;
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

#[derive(Debug, Clone)]
pub struct Col {
    pub title: Cow<'static, str>,
    /// Minimum width.
    pub width: u16,
    /// May grow up to its longest cell.
    pub flex: bool,
    /// Absorbs width left over after every column fits its content (meters).
    pub grow: bool,
    pub align: Align,
    /// 0 = always shown; higher numbers are dropped first when space is short.
    pub prio: u8,
    /// Truncate in the middle (names: the tail tells them apart).
    pub mid: bool,
    /// A sparkline column: after meters reach a readable length it takes
    /// the remaining width, stretching its buckets.
    pub spark: bool,
}

pub const fn col(title: &'static str, width: u16, align: Align, prio: u8) -> Col {
    Col {
        title: Cow::Borrowed(title),
        width,
        flex: true,
        grow: false,
        align,
        prio,
        mid: false,
        spark: false,
    }
}

/// A text column that is always shown and grows to its content.
pub const fn flex(title: &'static str, width: u16) -> Col {
    Col {
        title: Cow::Borrowed(title),
        width,
        flex: true,
        grow: false,
        align: Align::Left,
        prio: 0,
        mid: true,
        spark: false,
    }
}

/// A sparkline column that takes left-over width.
pub const fn spark(title: &'static str, width: u16, prio: u8) -> Col {
    Col {
        title: Cow::Borrowed(title),
        width,
        flex: true,
        grow: true,
        align: Align::Left,
        prio,
        mid: false,
        spark: true,
    }
}

/// A meter column that takes left-over width.
pub const fn grow(title: &'static str, width: u16, prio: u8) -> Col {
    Col {
        title: Cow::Borrowed(title),
        width,
        flex: true,
        grow: true,
        align: Align::Left,
        prio,
        mid: false,
        spark: false,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SortKey {
    Num(f64),
    Text(String),
}

impl SortKey {
    fn cmp(&self, o: &SortKey) -> Ordering {
        match (self, o) {
            (SortKey::Num(a), SortKey::Num(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
            (SortKey::Text(a), SortKey::Text(b)) => a.to_lowercase().cmp(&b.to_lowercase()),
            (SortKey::Num(_), SortKey::Text(_)) => Ordering::Less,
            (SortKey::Text(_), SortKey::Num(_)) => Ordering::Greater,
        }
    }
}

pub fn num(v: f64) -> SortKey {
    SortKey::Num(if v.is_finite() { v } else { f64::MIN })
}
pub fn text(s: &str) -> SortKey {
    SortKey::Text(s.to_string())
}

/// A cell: fixed spans, or a widget drawn at the column's final width.
#[derive(Debug, Clone)]
pub enum Cell {
    Spans(Vec<Span<'static>>),
    /// Sparkline, one character per bucket (pooled when narrower).
    Spark {
        values: Vec<f64>,
        color: Color,
    },
    /// Alternatives from widest to narrowest; the widest that fits is drawn
    /// (e.g. `limit reached` or just `⚠`).
    Choice(Vec<Vec<Span<'static>>>),
    /// Meter filling the column, followed by `label`.
    Meter {
        frac: f64,
        color: Option<Color>,
        label: String,
    },
}

impl Cell {
    /// The cell's full text (no truncation); sparklines have none.
    pub fn text(&self) -> Option<String> {
        let join = |v: &[Span<'static>]| v.iter().map(|s| s.content.as_ref()).collect::<String>();
        match self {
            Cell::Spans(s) => Some(join(s)),
            Cell::Choice(alts) => alts.first().map(|a| join(a)),
            Cell::Spark { .. } => None,
            Cell::Meter { label, .. } => Some(label.trim().to_string()),
        }
    }

    /// Width this cell would like.
    fn want(&self) -> usize {
        match self {
            Cell::Spans(s) => s.iter().map(|x| x.width()).sum(),
            Cell::Choice(alts) => alts
                .first()
                .map(|s| s.iter().map(|x| x.width()).sum())
                .unwrap_or(0),
            Cell::Spark { values, .. } => values.len().min(48),
            Cell::Meter { label, .. } => 6 + fmt::width(label),
        }
    }

    /// Spans at width `w`; `level` is the narrowest alternative every cell
    /// of a [`Cell::Choice`] column needs, so a column never mixes forms.
    fn spans(&self, w: usize, t: &Theme, level: usize) -> Vec<Span<'static>> {
        match self {
            Cell::Spans(s) => s.clone(),
            Cell::Choice(alts) => alts.get(level).or(alts.last()).cloned().unwrap_or_default(),
            Cell::Spark { values, color } => widgets::spark(values, w, t, *color),
            Cell::Meter { frac, color, label } => {
                let lw = fmt::width(label);
                let mw = w.saturating_sub(lw).max(1);
                let mut v = widgets::meter(*frac, mw, t, *color);
                v.push(Span::styled(label.clone(), Style::default().fg(t.fg)));
                v
            }
        }
    }
}

/// One row: rendered cells plus sort keys (one per column) and filter text.
#[derive(Debug, Clone)]
pub struct TRow {
    pub id: String,
    pub cells: Vec<Cell>,
    pub sort: Vec<SortKey>,
    pub filter: String,
    pub style: Option<Style>,
}

impl TRow {
    pub fn new(id: impl Into<String>) -> TRow {
        TRow {
            id: id.into(),
            cells: Vec::new(),
            sort: Vec::new(),
            filter: String::new(),
            style: None,
        }
    }
    /// Append a plain-text cell.
    pub fn text(mut self, s: impl Into<String>, key: SortKey) -> TRow {
        self.cells.push(Cell::Spans(vec![Span::raw(s.into())]));
        self.sort.push(key);
        self
    }
    /// Append a styled text cell.
    pub fn styled(mut self, s: impl Into<String>, style: Style, key: SortKey) -> TRow {
        self.cells
            .push(Cell::Spans(vec![Span::styled(s.into(), style)]));
        self.sort.push(key);
        self
    }
    /// Append a cell made of spans.
    pub fn spans(mut self, spans: Vec<Span<'static>>, key: SortKey) -> TRow {
        self.cells.push(Cell::Spans(spans));
        self.sort.push(key);
        self
    }
    /// Append a cell with narrower alternatives (widest first).
    pub fn choice(mut self, alts: Vec<Vec<Span<'static>>>, key: SortKey) -> TRow {
        self.cells.push(Cell::Choice(alts));
        self.sort.push(key);
        self
    }
    /// Append a sparkline cell.
    pub fn spark(mut self, values: Vec<f64>, color: Color, key: SortKey) -> TRow {
        self.cells.push(Cell::Spark { values, color });
        self.sort.push(key);
        self
    }
    /// Append a meter cell (bar plus label).
    pub fn meter(
        mut self,
        frac: f64,
        color: Option<Color>,
        label: impl Into<String>,
        key: SortKey,
    ) -> TRow {
        self.cells.push(Cell::Meter {
            frac,
            color,
            label: label.into(),
        });
        self.sort.push(key);
        self
    }
    pub fn filter(mut self, s: impl Into<String>) -> TRow {
        self.filter = s.into().to_lowercase();
        self
    }
    pub fn style(mut self, s: Style) -> TRow {
        self.style = Some(s);
        self
    }
}

/// Drop those of the `candidates` columns whose cells are blank (`""`,
/// `-`, `—`) in every row, e.g. names and keys that privacy mode hides.
/// Sort keys go with them.
pub fn drop_blank_cols(cols: &[Col], rows: &mut [TRow], candidates: &[usize]) -> Vec<Col> {
    let blank = |c: &Cell| match c {
        Cell::Spans(v) => {
            let s: String = v.iter().map(|s| s.content.as_ref()).collect();
            matches!(s.trim(), "" | "-" | "—")
        }
        _ => false,
    };
    if rows.is_empty() {
        return cols.to_vec();
    }
    let keep: Vec<bool> = (0..cols.len())
        .map(|i| {
            !candidates.contains(&i) || !rows.iter().all(|r| r.cells.get(i).is_some_and(blank))
        })
        .collect();
    for r in rows.iter_mut() {
        let mut i = 0;
        r.cells.retain(|_| {
            i += 1;
            keep.get(i - 1).copied().unwrap_or(true)
        });
        let mut i = 0;
        r.sort.retain(|_| {
            i += 1;
            keep.get(i - 1).copied().unwrap_or(true)
        });
    }
    cols.iter()
        .zip(&keep)
        .filter(|(_, k)| **k)
        .map(|(c, _)| c.clone())
        .collect()
}

/// Apply the state's filter and sort.
pub fn order(mut rows: Vec<TRow>, st: &TableState) -> Vec<TRow> {
    if !st.filter.is_empty() {
        let f = st.filter.to_lowercase();
        rows.retain(|r| r.filter.contains(&f));
    }
    if let Some((c, desc)) = st.sort {
        rows.sort_by(|a, b| {
            let o = match (a.sort.get(c), b.sort.get(c)) {
                (Some(x), Some(y)) => x.cmp(y),
                _ => Ordering::Equal,
            };
            if desc { o.reverse() } else { o }
        });
    }
    rows
}

/// Longest content per column (header included).
pub fn content_widths(cols: &[Col], rows: &[TRow]) -> Vec<usize> {
    cols.iter()
        .enumerate()
        .map(|(i, c)| {
            rows.iter()
                .filter_map(|r| r.cells.get(i))
                .map(Cell::want)
                .max()
                .unwrap_or(0)
                .max(fmt::width(&c.title) + 1)
        })
        .collect()
}

/// A placed column: index, x offset within the table, width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub col: usize,
    pub x: u16,
    pub width: u16,
}

/// Choose which columns fit and their widths (see the module docs).
pub fn fit(cols: &[Col], content: &[usize], width: u16) -> Vec<Placed> {
    let mut keep: Vec<usize> = (0..cols.len()).collect();
    let total = |k: &[usize]| -> u16 {
        k.iter().map(|i| cols[*i].width).sum::<u16>() + k.len().saturating_sub(1) as u16
    };
    // Drop the highest-priority-number column (rightmost on ties).
    let drop_one = |keep: &mut Vec<usize>, min_prio: u8| -> bool {
        let victim = keep
            .iter()
            .enumerate()
            .filter(|(_, i)| cols[**i].prio > 0 && cols[**i].prio >= min_prio)
            .max_by_key(|(p, i)| (cols[**i].prio, *p))
            .map(|(p, _)| p);
        match victim {
            Some(p) => {
                keep.remove(p);
                true
            }
            None => false,
        }
    };
    while total(&keep) > width && keep.len() > 1 {
        if !drop_one(&mut keep, 0) {
            keep.pop();
        }
    }
    loop {
        let placed = allocate(cols, content, width, &keep);
        // Names matter more than optional statistics: while an always-shown
        // text column is cut (names: at all — the cut part is what tells
        // `account-12 · codex` from `account-14 · codex`; other text by
        // several characters), drop optional columns (priority 4 and up).
        let starved = placed.iter().any(|p| {
            let c = &cols[p.col];
            let slack = if c.mid { 0 } else { 3 };
            c.prio == 0
                && !c.grow
                && c.align == Align::Left
                && content.get(p.col).copied().unwrap_or(0) as u16 > p.width + slack
        });
        if starved && drop_one(&mut keep, 4) {
            continue;
        }
        return placed;
    }
}

fn allocate(cols: &[Col], content: &[usize], width: u16, keep: &[usize]) -> Vec<Placed> {
    let total =
        keep.iter().map(|i| cols[*i].width).sum::<u16>() + keep.len().saturating_sub(1) as u16;
    let mut widths: Vec<u16> = keep.iter().map(|i| cols[*i].width).collect();
    let mut spare = width.saturating_sub(total);
    let want = |k: usize| -> u16 {
        let c = &cols[keep[k]];
        let w = content.get(keep[k]).copied().unwrap_or(0) as u16;
        if c.flex || c.grow {
            w.max(c.width)
        } else {
            c.width
        }
    };
    // Round-robin one column at a time, most important columns first, so
    // every truncated column gets a fair share before any is fully satisfied.
    let mut by_prio: Vec<usize> = (0..keep.len()).collect();
    by_prio.sort_by_key(|k| (cols[keep[*k]].prio, *k));
    while spare > 0 {
        let mut gave = false;
        for &k in &by_prio {
            if spare == 0 {
                break;
            }
            if widths[k] < want(k) {
                widths[k] += 1;
                spare -= 1;
                gave = true;
            }
        }
        if !gave {
            break;
        }
    }
    // Meters take what is left up to a readable length, then sparklines
    // (which can use any width), then meters up to their maximum.
    const METER_SOFT: u16 = 32;
    const MAX_GROW: u16 = 48;
    const MAX_SPARK: u16 = 160;
    let meters: Vec<usize> = (0..keep.len())
        .filter(|k| cols[keep[*k]].grow && !cols[keep[*k]].spark)
        .collect();
    let sparks: Vec<usize> = (0..keep.len()).filter(|k| cols[keep[*k]].spark).collect();
    // A sparkline stretches to at most three characters per bucket; wider
    // it turns into blocks that no longer read as a trend.
    let cap_of = |k: usize, cap: u16| -> u16 {
        if cols[keep[k]].spark {
            let n = content.get(keep[k]).copied().unwrap_or(0) as u16;
            cap.min(n.saturating_mul(3)).max(cols[keep[k]].width)
        } else {
            cap
        }
    };
    let mut grow = |set: &[usize], cap: u16, spare: &mut u16| {
        let mut gave = true;
        while *spare > 0 && gave {
            gave = false;
            for &k in set {
                if *spare > 0 && widths[k] < cap_of(k, cap) {
                    widths[k] += 1;
                    *spare -= 1;
                    gave = true;
                }
            }
        }
    };
    if sparks.is_empty() {
        grow(&meters, MAX_GROW, &mut spare);
    } else {
        grow(&meters, METER_SOFT, &mut spare);
        grow(&sparks, MAX_SPARK, &mut spare);
        grow(&meters, MAX_GROW, &mut spare);
    }
    // Then breathing room between columns (up to two extra cells per gap).
    let gaps = keep.len().saturating_sub(1) as u16;
    let extra = spare.checked_div(gaps).unwrap_or(0).min(2);
    let mut x = 0u16;
    keep.iter()
        .zip(widths)
        .map(|(i, w)| {
            let p = Placed {
                col: *i,
                x,
                width: w,
            };
            x += w + 1 + extra;
            p
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn put(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    w: u16,
    spans: &[Span<'static>],
    align: Align,
    base: Style,
    mid: bool,
) {
    if w == 0 {
        return;
    }
    let content_w: usize = spans.iter().map(|s| s.width()).sum();
    let mut line_spans: Vec<Span<'static>> = Vec::new();
    if content_w > w as usize {
        // Truncate: flatten to text keeping the first span's style.
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        let style = spans.first().map(|s| s.style).unwrap_or_default();
        let cut = if mid {
            fmt::trunc_mid(&text, w as usize)
        } else {
            fmt::trunc(&text, w as usize)
        };
        line_spans.push(Span::styled(cut, style));
    } else {
        line_spans.extend(spans.iter().cloned());
    }
    let lw: usize = line_spans.iter().map(|s| s.width()).sum();
    let dx = if align == Align::Right {
        w.saturating_sub(lw as u16)
    } else {
        0
    };
    let line = Line::from(line_spans).style(base);
    buf.set_line(x + dx, y, &line, w - dx);
}

/// Render a table into `area`; returns the body rectangle (for mouse hits).
pub fn render(
    buf: &mut Buffer,
    area: Rect,
    t: &Theme,
    cols: &[Col],
    rows: &[TRow],
    st: &TableState,
    focused: bool,
) -> Rect {
    st.cols.set(cols.len());
    st.len.set(rows.len());
    if area.height == 0 || area.width == 0 {
        return area;
    }
    let content = content_widths(cols, rows);
    let layout = fit(cols, &content, area.width);
    // Header.
    let hs = Style::default().fg(t.title).add_modifier(Modifier::BOLD);
    for p in &layout {
        let c = &cols[p.col];
        let mut title = c.title.to_string();
        if let Some((sc, desc)) = st.sort
            && sc == p.col
        {
            title.push(if desc { '↓' } else { '↑' });
        }
        put(
            buf,
            area.x + p.x,
            area.y,
            p.width,
            &[Span::styled(title, hs)],
            c.align,
            Style::default(),
            false,
        );
    }
    let body = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(1),
    );
    st.page.set(body.height as usize);
    if body.height == 0 {
        return body;
    }
    // Keep the selection visible.
    let sel = st.selected.min(rows.len().saturating_sub(1));
    let mut off = st.offset.get().min(rows.len().saturating_sub(1));
    if sel < off {
        off = sel;
    } else if sel >= off + body.height as usize {
        off = sel + 1 - body.height as usize;
    }
    st.offset.set(off);
    let sel_style = t.selected();
    // Alternative level per column: the first at which every cell fits.
    let level: Vec<usize> = (0..cols.len())
        .map(|c| {
            let w = layout
                .iter()
                .find(|p| p.col == c)
                .map(|p| p.width as usize)
                .unwrap_or(0);
            (0..4)
                .find(|k| {
                    rows.iter().all(|r| match r.cells.get(c) {
                        Some(Cell::Choice(alts)) => alts
                            .get(*k)
                            .or(alts.last())
                            .is_none_or(|a| a.iter().map(|x| x.width()).sum::<usize>() <= w),
                        _ => true,
                    })
                })
                .unwrap_or(3)
        })
        .collect();
    for (row_i, r) in rows.iter().enumerate().skip(off).take(body.height as usize) {
        let y = body.y + (row_i - off) as u16;
        let selected = row_i == sel;
        let mut base = r.style.unwrap_or_default();
        if selected {
            base = sel_style;
            if !focused {
                base = base.add_modifier(Modifier::DIM);
            }
            buf.set_style(Rect::new(area.x, y, area.width, 1), base);
        }
        for p in &layout {
            if let Some(cell) = r.cells.get(p.col) {
                let spans = cell.spans(p.width as usize, t, level[p.col]);
                let spans: Vec<Span<'static>> = if selected && !t.no_color {
                    spans
                        .into_iter()
                        .map(|s| {
                            let mut st = s.style;
                            st.bg = Some(t.selected_bg);
                            if st.fg.is_none() {
                                st.fg = Some(t.selected_fg);
                            }
                            Span::styled(s.content, st)
                        })
                        .collect()
                } else {
                    spans
                };
                put(
                    buf,
                    area.x + p.x,
                    y,
                    p.width,
                    &spans,
                    cols[p.col].align,
                    base,
                    cols[p.col].mid,
                );
            }
        }
    }
    // Scroll indicator, drawn on the pane's bottom border.
    let mut right_edge = area.right().saturating_sub(1);
    if rows.len() > body.height as usize && area.width > 12 && area.bottom() < buf.area.bottom() {
        let shown_end = (off + body.height as usize).min(rows.len());
        let s = format!(" {}-{}/{} ", off + 1, shown_end, rows.len());
        let w = fmt::width(&s) as u16;
        let x = area.right().saturating_sub(w + 1);
        buf.set_string(x, area.bottom(), s, Style::default().fg(t.graph_text));
        right_edge = x;
    }
    // The selected row's full name when its name column is cut, on the
    // bottom border (right-aligned, at most half the width).
    if focused
        && area.bottom() < buf.area.bottom()
        && let Some(p) = layout.iter().find(|p| cols[p.col].mid)
        && let Some(Cell::Spans(sp)) = rows.get(sel).and_then(|r| r.cells.get(p.col))
    {
        let full: String = sp.iter().map(|s| s.content.as_ref()).collect();
        if fmt::width(&full) > p.width as usize {
            // Free border to the right of whatever the frame drew there
            // (key hints on the left).
            let y = area.bottom();
            let used = (area.x..right_edge)
                .rev()
                .find(|x| !matches!(buf[(*x, y)].symbol(), "─" | " "))
                .map(|x| x + 2)
                .unwrap_or(area.x + 1);
            let room = right_edge.saturating_sub(used) as usize;
            let s = fmt::trunc_mid(&format!(" ▸ {full} "), room);
            let w = fmt::width(&s) as u16;
            if w > 6 {
                buf.set_string(
                    right_edge.saturating_sub(w),
                    area.bottom(),
                    s,
                    Style::default().fg(t.title),
                );
            }
        }
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widths(p: &[Placed]) -> Vec<(usize, u16)> {
        p.iter().map(|p| (p.col, p.width)).collect()
    }

    #[test]
    fn columns_drop_by_priority() {
        let cols = [
            flex("name", 10),
            col("a", 6, Align::Right, 2),
            col("b", 6, Align::Right, 1),
        ];
        let f = fit(&cols, &[10, 6, 6], 18);
        assert_eq!(f.iter().map(|x| x.col).collect::<Vec<_>>(), vec![0, 2]);
    }

    #[test]
    fn names_are_capped_at_their_content() {
        // A 160-column table with 16-character names must not stretch the
        // name column to 140 and push the numbers to the far right.
        let cols = [
            flex("name", 12),
            col("tokens", 7, Align::Right, 0),
            col("req", 7, Align::Right, 0),
        ];
        let f = fit(&cols, &[16, 6, 5], 160);
        assert_eq!(widths(&f), vec![(0, 16), (1, 7), (2, 7)]);
        // Gaps widen by at most two cells.
        assert_eq!(f[1].x, 16 + 3);
    }

    #[test]
    fn spare_width_goes_to_truncated_columns_first() {
        let cols = [
            col("time", 8, Align::Left, 0),
            col("source", 10, Align::Left, 4),
            flex("model", 12),
            col("tokens", 7, Align::Right, 2),
        ];
        // Model content 20 wide, source 30: both get what they need when
        // there is room (8+30+20+7 + 3 gaps = 68).
        let f = fit(&cols, &[8, 30, 20, 6], 68);
        assert_eq!(widths(&f), vec![(0, 8), (1, 30), (2, 20), (3, 7)]);
        // With less room the optional column gives way rather than cut the
        // name (the cut part is what tells names apart).
        let f = fit(&cols, &[8, 30, 20, 6], 50);
        let w = widths(&f);
        assert!(w.contains(&(2, 20)), "{w:?}");
        assert!(!w.iter().any(|c| c.0 == 1), "{w:?}");
        // Without optional columns both are cut, neither is starved.
        let mut cols2 = cols.clone();
        cols2[1].prio = 0;
        let f = fit(&cols2, &[8, 30, 20, 6], 50);
        let w = widths(&f);
        assert!(w[1].1 > 10 && w[2].1 > 12, "{w:?}");
        assert_eq!(f.last().map(|p| p.x + p.width), Some(50));
    }

    #[test]
    fn optional_columns_make_room_for_names() {
        let cols = [
            flex("name", 12),
            col("tokens", 7, Align::Right, 0),
            col("rpm", 6, Align::Right, 5),
            col("tpm", 7, Align::Right, 5),
        ];
        // 30-character names in a 40-column table: rpm and tpm go first.
        let f = fit(&cols, &[30, 6, 4, 5], 40);
        assert_eq!(f.iter().map(|p| p.col).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(f[0].width, 30);
    }

    #[test]
    fn meters_absorb_the_rest() {
        let cols = [
            flex("name", 8),
            grow("share", 12, 1),
            col("req", 5, Align::Right, 0),
        ];
        let f = fit(&cols, &[10, 12, 4], 60);
        assert_eq!(f[0].width, 10);
        assert_eq!(f[2].width, 5);
        assert_eq!(f[1].width, 60 - 10 - 5 - 2);
        // ...but not without limit.
        let f = fit(&cols, &[10, 12, 4], 200);
        assert_eq!(f[1].width, 48);
    }

    #[test]
    fn sorting_and_filtering() {
        let rows = vec![
            TRow::new("1")
                .text("b", text("b"))
                .text("2", num(2.0))
                .filter("bravo"),
            TRow::new("2")
                .text("a", text("a"))
                .text("9", num(9.0))
                .filter("alpha"),
        ];
        let st = TableState {
            sort: Some((1, true)),
            ..Default::default()
        };
        let o = order(rows.clone(), &st);
        assert_eq!(o[0].id, "2");
        let st = TableState {
            filter: "BRA".into(),
            ..Default::default()
        };
        assert_eq!(order(rows, &st).len(), 1);
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    #[test]
    fn selection_is_reverse_video_without_colour() {
        let t = Theme {
            no_color: true,
            ..Default::default()
        };
        let cols = [flex("name", 6), col("n", 3, Align::Right, 0)];
        let rows = vec![
            TRow::new("a").text("alpha", text("a")).text("1", num(1.0)),
            TRow::new("b").text("beta", text("b")).text("2", num(2.0)),
        ];
        let st = TableState::default();
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 4));
        render(
            &mut buf,
            Rect::new(0, 0, 20, 3),
            &t,
            &cols,
            &rows,
            &st,
            true,
        );
        assert!(buf[(0, 1)].modifier.contains(Modifier::REVERSED));
        assert!(!buf[(0, 2)].modifier.contains(Modifier::REVERSED));
    }
}
