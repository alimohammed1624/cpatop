//! A small table renderer: right-aligned numeric columns, priority-based
//! column dropping on narrow panes, sorting, filtering and selection.

use crate::app::TableState;
use crate::fmt;
use crate::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
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
    /// Minimum (and, unless `flex`, exact) width.
    pub width: u16,
    pub flex: bool,
    pub align: Align,
    /// 0 = always shown; higher numbers are dropped first when space is short.
    pub prio: u8,
}

pub const fn col(title: &'static str, width: u16, align: Align, prio: u8) -> Col {
    Col {
        title: Cow::Borrowed(title),
        width,
        flex: false,
        align,
        prio,
    }
}

pub const fn flex(title: &'static str, width: u16) -> Col {
    Col {
        title: Cow::Borrowed(title),
        width,
        flex: true,
        align: Align::Left,
        prio: 0,
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

/// One row: rendered cells plus sort keys (one per column) and filter text.
#[derive(Debug, Clone)]
pub struct TRow {
    pub id: String,
    pub cells: Vec<Vec<Span<'static>>>,
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
        self.cells.push(vec![Span::raw(s.into())]);
        self.sort.push(key);
        self
    }
    /// Append a styled text cell.
    pub fn styled(mut self, s: impl Into<String>, style: Style, key: SortKey) -> TRow {
        self.cells.push(vec![Span::styled(s.into(), style)]);
        self.sort.push(key);
        self
    }
    /// Append a cell made of spans.
    pub fn spans(mut self, spans: Vec<Span<'static>>, key: SortKey) -> TRow {
        self.cells.push(spans);
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

/// Choose which columns fit and their widths.
fn fit(cols: &[Col], width: u16) -> Vec<(usize, u16)> {
    let mut keep: Vec<usize> = (0..cols.len()).collect();
    let total = |k: &[usize]| -> u16 {
        k.iter().map(|i| cols[*i].width).sum::<u16>() + k.len().saturating_sub(1) as u16
    };
    while total(&keep) > width && keep.len() > 1 {
        // Drop the highest-priority-number column (rightmost on ties).
        let (pos, _) = keep
            .iter()
            .enumerate()
            .filter(|(_, i)| cols[**i].prio > 0)
            .max_by_key(|(p, i)| (cols[**i].prio, *p))
            .unwrap_or((keep.len() - 1, &keep[keep.len() - 1]));
        keep.remove(pos);
    }
    let used = total(&keep);
    let spare = width.saturating_sub(used);
    let flex_count = keep.iter().filter(|i| cols[**i].flex).count() as u16;
    keep.iter()
        .map(|i| {
            let mut w = cols[*i].width;
            if cols[*i].flex && flex_count > 0 {
                w += spare / flex_count;
            }
            (*i, w)
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
    let layout = fit(cols, area.width);
    // Header.
    let hs = Style::default().fg(t.title).add_modifier(Modifier::BOLD);
    let mut x = area.x;
    for (i, w) in &layout {
        let c = &cols[*i];
        let mut title = c.title.to_string();
        if let Some((sc, desc)) = st.sort
            && sc == *i
        {
            title.push(if desc { '↓' } else { '↑' });
        }
        put(
            buf,
            x,
            area.y,
            *w,
            &[Span::styled(title, hs)],
            c.align,
            Style::default(),
        );
        x += w + 1;
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
    for (row_i, r) in rows.iter().enumerate().skip(off).take(body.height as usize) {
        let y = body.y + (row_i - off) as u16;
        let selected = row_i == sel;
        let mut base = r.style.unwrap_or_default();
        if selected {
            base = Style::default().bg(t.selected_bg).fg(t.selected_fg);
            if !focused {
                base = base.add_modifier(Modifier::DIM);
            }
            buf.set_style(Rect::new(area.x, y, area.width, 1), base);
        }
        let mut x = area.x;
        for (i, w) in &layout {
            if let Some(cell) = r.cells.get(*i) {
                let cell: Vec<Span<'static>> = if selected {
                    cell.iter()
                        .map(|s| {
                            let mut st = s.style;
                            st.bg = Some(t.selected_bg);
                            if st.fg.is_none() {
                                st.fg = Some(t.selected_fg);
                            }
                            Span::styled(s.content.clone(), st)
                        })
                        .collect()
                } else {
                    cell.clone()
                };
                put(buf, x, y, *w, &cell, cols[*i].align, base);
            }
            x += w + 1;
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

    #[test]
    fn columns_drop_by_priority() {
        let cols = [
            flex("name", 10),
            col("a", 6, Align::Right, 2),
            col("b", 6, Align::Right, 1),
        ];
        let f = fit(&cols, 18);
        assert_eq!(f.iter().map(|x| x.0).collect::<Vec<_>>(), vec![0, 2]);
        let f = fit(&cols, 40);
        assert_eq!(f[0].1, 40 - 6 - 6 - 2);
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
