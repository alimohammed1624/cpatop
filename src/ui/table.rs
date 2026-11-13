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
}

pub const fn col(title: &'static str, width: u16, align: Align, prio: u8) -> Col {
    Col {
        title: Cow::Borrowed(title),
        width,
        flex: true,
        grow: false,
        align,
        prio,
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
    Spark { values: Vec<f64>, color: Color },
    /// Meter filling the column, followed by `label`.
    Meter {
        frac: f64,
        color: Option<Color>,
        label: String,
    },
}

impl Cell {
    /// Width this cell would like.
    fn want(&self) -> usize {
        match self {
            Cell::Spans(s) => s.iter().map(|x| x.width()).sum(),
            Cell::Spark { values, .. } => values.len().min(48),
            Cell::Meter { label, .. } => 6 + fmt::width(label),
        }
    }

    fn spans(&self, w: usize, t: &Theme) -> Vec<Span<'static>> {
        match self {
            Cell::Spans(s) => s.clone(),
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
        self.cells.push(Cell::Spans(vec![Span::styled(s.into(), style)]));
        self.sort.push(key);
        self
    }
    /// Append a cell made of spans.
    pub fn spans(mut self, spans: Vec<Span<'static>>, key: SortKey) -> TRow {
        self.cells.push(Cell::Spans(spans));
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
        // text column is cut by several characters, drop optional columns
        // (priority 4 and up).
        let starved = placed.iter().any(|p| {
            let c = &cols[p.col];
            c.prio == 0
                && !c.grow
                && c.align == Align::Left
                && content.get(p.col).copied().unwrap_or(0) as u16 > p.width + 3
        });
        if starved && drop_one(&mut keep, 4) {
            continue;
        }
        return placed;
    }
}

fn allocate(cols: &[Col], content: &[usize], width: u16, keep: &[usize]) -> Vec<Placed> {
    let total = keep.iter().map(|i| cols[*i].width).sum::<u16>()
        + keep.len().saturating_sub(1) as u16;
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
    // Meters take what is left.
    let growers: Vec<usize> = (0..keep.len()).filter(|k| cols[keep[*k]].grow).collect();
    if !growers.is_empty() && spare > 0 {
        let each = spare / growers.len() as u16;
        let mut rest = spare % growers.len() as u16;
        for k in growers {
            widths[k] += each + if rest > 0 { 1 } else { 0 };
            rest = rest.saturating_sub(1);
        }
        spare = 0;
    }
    // Then breathing room between columns (up to two extra cells per gap).
    let gaps = keep.len().saturating_sub(1) as u16;
    let extra = if gaps > 0 { (spare / gaps).min(2) } else { 0 };
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

fn put(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    w: u16,
    spans: &[Span<'static>],
    align: Align,
    base: Style,
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
        line_spans.push(Span::styled(fmt::trunc(&text, w as usize), style));
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
                let spans = cell.spans(p.width as usize, t);
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
                put(buf, area.x + p.x, y, p.width, &spans, cols[p.col].align, base);
            }
        }
    }
    // Scroll indicator, drawn on the pane's bottom border.
    if rows.len() > body.height as usize && area.width > 12 && area.bottom() < buf.area.bottom() {
        let shown_end = (off + body.height as usize).min(rows.len());
        let s = format!(" {}-{}/{} ", off + 1, shown_end, rows.len());
        let w = fmt::width(&s) as u16;
        let x = area.right().saturating_sub(w + 1);
        buf.set_string(x, area.bottom(), s, Style::default().fg(t.graph_text));
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
        // With less room both are cut, neither is starved.
        let f = fit(&cols, &[8, 30, 20, 6], 50);
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
        let f = fit(&cols, &[10, 12, 4], 80);
        assert_eq!(f[0].width, 10);
        assert_eq!(f[2].width, 5);
        assert_eq!(f[1].width, 80 - 10 - 5 - 2);
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
