//! btop-style building blocks: pane borders, gradient graphs, meters,
//! sparklines, scatter plots and heat cells.

use crate::config::GraphSymbols;
use crate::fmt;
use crate::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Widget};

/// Rounded pane border with `─¹ name ─` embedded in the top edge.
pub fn pane_block<'a>(
    t: &Theme,
    num: usize,
    name: &str,
    focused: bool,
    right: Vec<Span<'a>>,
    bottom: Option<(String, String)>,
) -> Block<'a> {
    let border = if focused { t.border_focus } else { t.border };
    let bs = Style::default().fg(border);
    let mut title = vec![
        Span::styled("─", bs),
        Span::styled(
            fmt::superscript(num),
            Style::default().fg(t.hi).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {name} "),
            Style::default().fg(t.title).add_modifier(Modifier::BOLD),
        ),
    ];
    if focused {
        title.push(Span::styled("◆ ", Style::default().fg(t.border_focus)));
    }
    let mut b = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(bs)
        .style(Style::default().bg(t.bg).fg(t.fg))
        .title(Line::from(title));
    if !right.is_empty() {
        let mut r = vec![Span::styled(" ", bs)];
        r.extend(right);
        r.push(Span::styled(" ─", bs));
        b = b.title(Line::from(r).right_aligned());
    }
    if let Some((l, r)) = bottom {
        let gs = Style::default().fg(t.graph_text);
        if !l.is_empty() {
            b = b.title_bottom(Line::from(vec![
                Span::styled("─", bs),
                Span::styled(format!(" {l} "), gs),
            ]));
        }
        if !r.is_empty() {
            b = b.title_bottom(
                Line::from(vec![
                    Span::styled(format!(" {r} "), gs),
                    Span::styled("─", bs),
                ])
                .right_aligned(),
            );
        }
    }
    b
}

/// Resample a series to `n` points without inventing data: max-pool when
/// shrinking, repeat each bucket over its columns when stretching (so bars
/// stay crisp and a cursor maps to exactly one bucket). `NaN` marks a
/// missing value and stays `NaN` unless a pooled group has real values.
pub fn resample(data: &[f64], n: usize) -> Vec<f64> {
    if n == 0 {
        return vec![];
    }
    let len = data.len();
    if len == 0 {
        return vec![f64::NAN; n];
    }
    if len >= n {
        (0..n)
            .map(|j| {
                let a = j * len / n;
                let b = ((j + 1) * len / n).max(a + 1).min(len);
                data[a..b]
                    .iter()
                    .copied()
                    .filter(|v| v.is_finite())
                    .fold(f64::NAN, f64::max)
            })
            .collect()
    } else {
        (0..n).map(|j| data[(j * len / n).min(len - 1)]).collect()
    }
}

/// Column range `[start, end)` that bucket `i` of `n` occupies in `width` columns.
pub fn bucket_cols(i: usize, n: usize, width: usize) -> (usize, usize) {
    if n == 0 || width == 0 {
        return (0, 0);
    }
    if n <= width {
        // Inverse of `resample`'s stretch: column j shows bucket j*n/width.
        let start = (i * width).div_ceil(n);
        let end = ((i + 1) * width).div_ceil(n).max(start + 1).min(width);
        (start.min(width - 1), end)
    } else {
        let c = (i * width / n).min(width - 1);
        (c, c + 1)
    }
}

/// Filled area graph with a vertical low→high colour gradient.
pub struct Graph<'a> {
    pub data: &'a [f64],
    pub theme: &'a Theme,
    pub symbols: GraphSymbols,
    pub max: Option<f64>,
    /// Use one colour instead of the gradient.
    pub color: Option<Color>,
}

impl Widget for Graph<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let w = area.width as usize;
        let h = area.height as usize;
        // Braille has two dot columns per cell, but only when there are at
        // least that many buckets; fewer buckets are drawn as solid bars.
        let per_cell = if self.symbols == GraphSymbols::Braille && self.data.len() >= w * 2 {
            2
        } else {
            1
        };
        let vals = resample(self.data, w * per_cell);
        let max = self
            .max
            .unwrap_or_else(|| {
                vals.iter()
                    .copied()
                    .filter(|v| v.is_finite())
                    .fold(0.0, f64::max)
            })
            .max(f64::MIN_POSITIVE);
        let color_at = |cy: usize| {
            self.color.unwrap_or_else(|| {
                let t = (h - 1 - cy) as f32 / (h.max(2) - 1) as f32;
                self.theme.gradient(t)
            })
        };
        let units = match self.symbols {
            GraphSymbols::Braille => 4.0,
            GraphSymbols::Block => 8.0,
            GraphSymbols::Ascii => 3.0,
        };
        let height_of = |v: f64| -> usize {
            if v <= 0.0 || !v.is_finite() {
                return 0;
            }
            let x = (v / max * h as f64 * units).round() as usize;
            x.max(1)
        };
        let hs: Vec<usize> = vals.iter().map(|v| height_of(*v)).collect();
        for cy in 0..h {
            let base = (h - 1 - cy) * units as usize;
            let fg = color_at(cy);
            for cx in 0..w {
                let symbol: String = match self.symbols {
                    GraphSymbols::Braille => {
                        const LEFT: [u32; 4] = [0x01, 0x02, 0x04, 0x40];
                        const RIGHT: [u32; 4] = [0x08, 0x10, 0x20, 0x80];
                        let (l, r) = if per_cell == 2 {
                            (hs[cx * 2], hs[cx * 2 + 1])
                        } else {
                            (hs[cx], hs[cx])
                        };
                        let mut bits = 0u32;
                        for (hh, table) in [(l, LEFT), (r, RIGHT)] {
                            let fill = hh.saturating_sub(base).min(4);
                            for i in 0..fill {
                                bits |= table[3 - i];
                            }
                        }
                        if bits == 0 {
                            continue;
                        }
                        char::from_u32(0x2800 + bits).unwrap_or(' ').to_string()
                    }
                    GraphSymbols::Block => {
                        const B: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
                        let fill = hs[cx].saturating_sub(base).min(8);
                        if fill == 0 {
                            continue;
                        }
                        B[fill].to_string()
                    }
                    GraphSymbols::Ascii => {
                        const A: [&str; 4] = [" ", ".", ":", "#"];
                        let fill = hs[cx].saturating_sub(base).min(3);
                        if fill == 0 {
                            continue;
                        }
                        A[fill].to_string()
                    }
                };
                let cell = &mut buf[(area.x + cx as u16, area.y + cy as u16)];
                cell.set_symbol(&symbol).set_fg(fg);
            }
        }
    }
}

/// Axis decorations for a time-series chart.
pub struct Axes<'a> {
    pub theme: &'a Theme,
    /// Bottom and top of the y scale.
    pub y_min: f64,
    pub y_max: f64,
    pub y_fmt: fn(f64) -> String,
    /// Bucket count and a label for each bucket (time).
    pub labels: &'a [String],
}

impl Axes<'_> {
    /// Width of the y-label gutter.
    fn label(&self, v: f64) -> String {
        if v == 0.0 {
            "0".into()
        } else {
            (self.y_fmt)(v)
        }
    }

    pub fn gutter(&self) -> u16 {
        let mid = (self.y_min + self.y_max) / 2.0;
        let w = [self.y_max, mid, self.y_min]
            .iter()
            .map(|v| fmt::width(&self.label(*v)))
            .max()
            .unwrap_or(1);
        w as u16 + 1
    }

    /// Draw y labels (max/mid/0) on the left and 3–5 time ticks on the bottom
    /// row; returns the plot area.
    pub fn draw(&self, buf: &mut Buffer, area: Rect) -> Rect {
        let t = self.theme;
        let dim = Style::default().fg(t.graph_text);
        let gw = self.gutter();
        if area.height < 3 || area.width <= gw + 8 {
            return area;
        }
        let plot = Rect::new(area.x + gw, area.y, area.width - gw, area.height - 1);
        // Y axis line, labels and ticks.
        for y in plot.y..plot.bottom() {
            buf.set_string(area.x + gw - 1, y, "│", Style::default().fg(t.border));
        }
        // The middle label names the half-way value, on the nearest row.
        let mid_row = (plot.height - 1).div_ceil(2);
        let mid = (self.y_min + self.y_max) / 2.0;
        let ys = [
            (plot.y, self.y_max),
            (plot.y + mid_row, mid),
            (plot.bottom() - 1, self.y_min),
        ];
        for (i, (y, v)) in ys.iter().enumerate() {
            if i == 1 && plot.height < 5 {
                continue;
            }
            let s = self.label(*v);
            let x = area.x + gw - 1 - fmt::width(&s) as u16;
            buf.set_string(x, *y, &s, dim);
            buf.set_string(area.x + gw - 1, *y, "┤", Style::default().fg(t.border));
        }
        // X axis line and ticks.
        let ay = plot.bottom();
        buf.set_string(area.x + gw - 1, ay, "└", Style::default().fg(t.border));
        buf.set_string(
            plot.x,
            ay,
            "─".repeat(plot.width as usize),
            Style::default().fg(t.border),
        );
        let n = self.labels.len();
        if n == 0 {
            return plot;
        }
        let w = plot.width as usize;
        let label_w = self.labels.iter().map(|l| fmt::width(l)).max().unwrap_or(5);
        let max_ticks = (w / (label_w + 3)).clamp(1, 5);
        let ticks = max_ticks.min(n);
        let mut last_end = 0usize;
        for k in 0..ticks {
            let i = if ticks == 1 {
                0
            } else {
                k * (n - 1) / (ticks - 1)
            };
            let (c0, _) = bucket_cols(i, n, w);
            let lw = fmt::width(&self.labels[i]);
            // Labels sit on the axis line, starting at their bucket.
            let start = c0.min(w.saturating_sub(lw));
            if k > 0 && start <= last_end {
                continue;
            }
            if lw <= w {
                buf.set_string(plot.x + start as u16, ay, &self.labels[i], dim);
            }
            last_end = start + lw;
        }
        plot
    }
}

/// Highlight bucket `i` of `n` in a plot area (vertical cursor line).
pub fn draw_cursor(buf: &mut Buffer, plot: Rect, i: usize, n: usize, t: &Theme) {
    if plot.width == 0 || n == 0 {
        return;
    }
    let (c0, c1) = bucket_cols(i, n, plot.width as usize);
    let c = (c0 + c1.saturating_sub(1)) / 2;
    let x = plot.x + c as u16;
    for y in plot.y..plot.bottom() {
        let cell = &mut buf[(x, y)];
        if cell.symbol() == " " {
            cell.set_char('│').set_fg(t.hi);
        } else {
            cell.set_style(
                Style::default()
                    .fg(t.hi)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            );
        }
    }
}

/// Log-log braille scatter plot (TTFT vs latency), coloured by density.
pub struct Scatter<'a> {
    pub points: &'a [(f64, f64)],
    pub theme: &'a Theme,
    pub x_ref: Option<f64>,
    pub y_ref: Option<f64>,
}

impl Scatter<'_> {
    /// Axis bounds in log10 space.
    pub fn bounds(points: &[(f64, f64)]) -> ((f64, f64), (f64, f64)) {
        let lx = |v: f64| v.max(1.0).log10();
        let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for (x, y) in points {
            x0 = x0.min(lx(*x));
            x1 = x1.max(lx(*x));
            y0 = y0.min(lx(*y));
            y1 = y1.max(lx(*y));
        }
        if x1 - x0 < 0.1 {
            x1 = x0 + 0.1;
        }
        if y1 - y0 < 0.1 {
            y1 = y0 + 0.1;
        }
        ((x0, x1), (y0, y1))
    }
}

impl Widget for Scatter<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 2 || area.height < 2 || self.points.is_empty() {
            return;
        }
        let ((x0, x1), (y0, y1)) = Scatter::bounds(self.points);
        let dw = area.width as usize * 2;
        let dh = area.height as usize * 4;
        let mut counts = vec![0u32; dw * dh];
        let lx = |v: f64| v.max(1.0).log10();
        for (x, y) in self.points {
            let px = ((lx(*x) - x0) / (x1 - x0) * (dw - 1) as f64).round() as usize;
            let py = ((lx(*y) - y0) / (y1 - y0) * (dh - 1) as f64).round() as usize;
            let py = dh - 1 - py.min(dh - 1);
            counts[py * dw + px.min(dw - 1)] += 1;
        }
        let ref_col = self.x_ref.map(|r| {
            (((lx(r) - x0) / (x1 - x0)).clamp(0.0, 1.0) * (area.width - 1) as f64).round() as u16
        });
        let ref_row = self.y_ref.map(|r| {
            let f = ((lx(r) - y0) / (y1 - y0)).clamp(0.0, 1.0);
            (area.height - 1) - (f * (area.height - 1) as f64).round() as u16
        });
        let dim = Style::default().fg(self.theme.meter_bg);
        let mut cells = vec![(0u32, 0u32); area.width as usize * area.height as usize];
        let mut max_density = 1u32;
        for py in 0..dh {
            for px in 0..dw {
                let c = counts[py * dw + px];
                if c == 0 {
                    continue;
                }
                let (cx, cy) = (px / 2, py / 4);
                let (sx, sy) = (px % 2, py % 4);
                const BITS: [[u32; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];
                let cell = &mut cells[cy * area.width as usize + cx];
                cell.0 |= BITS[sx][sy];
                cell.1 += c;
                max_density = max_density.max(cell.1);
            }
        }
        for cy in 0..area.height {
            for cx in 0..area.width {
                let (bits, density) = cells[cy as usize * area.width as usize + cx as usize];
                let pos = (area.x + cx, area.y + cy);
                if bits != 0 {
                    let t =
                        ((density as f32).ln_1p() / (max_density as f32).ln_1p()).clamp(0.0, 1.0);
                    let ch = char::from_u32(0x2800 + bits).unwrap_or('•');
                    buf[pos].set_char(ch).set_fg(self.theme.gradient(t));
                } else if Some(cx) == ref_col {
                    buf[pos].set_char('┊').set_style(dim);
                } else if Some(cy) == ref_row {
                    buf[pos].set_char('┈').set_style(dim);
                }
            }
        }
    }
}

/// btop-style meter made of `■` cells. Without colour the empty part uses
/// `·` so the fill level stays readable. `color: None` fills with the
/// single-hue volume ramp (shares); rates pass their good/bad colour. A
/// non-finite `frac` means "no data" and draws a neutral dashed track.
pub fn meter(frac: f64, width: usize, t: &Theme, color: Option<Color>) -> Vec<Span<'static>> {
    if width == 0 {
        return vec![];
    }
    if !frac.is_finite() {
        return vec![Span::styled(
            "╌".repeat(width),
            Style::default().fg(t.meter_bg),
        )];
    }
    let frac = frac.clamp(0.0, 1.0);
    let filled = (frac * width as f64).round() as usize;
    let mut out = Vec::with_capacity(width);
    for i in 0..width {
        if i < filled {
            let c =
                color.unwrap_or_else(|| t.ramp(0.25 + 0.75 * i as f32 / (width.max(2) - 1) as f32));
            out.push(Span::styled("■", Style::default().fg(c)));
        } else {
            let empty = if t.no_color { "·" } else { "■" };
            out.push(Span::styled(empty, Style::default().fg(t.meter_bg)));
        }
    }
    merge_spans(out)
}

/// Mini sparkline using block eighths, one colour, scaled to the series' own
/// peak. Buckets map to characters 1:1 when they fit (never interpolated);
/// longer series are max-pooled. `NaN` (no data) renders as a blank, zero as
/// a dim baseline.
pub fn spark(values: &[f64], width: usize, t: &Theme, color: Color) -> Vec<Span<'static>> {
    const B: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if width == 0 {
        return vec![];
    }
    let n = values.len().min(width);
    let vals = resample(values, n);
    let max = vals
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(0.0, f64::max);
    let mut out: Vec<Span<'static>> = vals
        .iter()
        .map(|v| {
            if !v.is_finite() {
                Span::raw(" ")
            } else if *v <= 0.0 || max <= 0.0 {
                Span::styled("▁", Style::default().fg(t.meter_bg))
            } else {
                let f = (v / max).clamp(0.0, 1.0);
                let i = ((f * 7.0).round() as usize).min(7);
                Span::styled(B[i].to_string(), Style::default().fg(color))
            }
        })
        .collect();
    if n < width {
        out.push(Span::raw(" ".repeat(width - n)));
    }
    merge_spans(out)
}

/// Heat cell for a level in 0..=1 (`None` = no data). Coloured `■` cells, or
/// shade glyphs when colour is off.
pub fn heat_cell(level: Option<f64>, color: Color, t: &Theme) -> (char, Color) {
    match level {
        None => (if t.no_color { '·' } else { '■' }, t.meter_bg),
        Some(l) if t.no_color => {
            let shades = ['░', '▒', '▓', '█'];
            (
                shades[((l.clamp(0.0, 1.0) * 3.0).round() as usize).min(3)],
                color,
            )
        }
        Some(_) => ('■', color),
    }
}

/// Merge adjacent spans with identical style.
pub fn merge_spans(spans: Vec<Span<'static>>) -> Vec<Span<'static>> {
    let mut out: Vec<Span<'static>> = Vec::with_capacity(spans.len());
    for s in spans {
        if let Some(last) = out.last_mut()
            && last.style == s.style
        {
            let mut c = last.content.to_string();
            c.push_str(&s.content);
            last.content = c.into();
            continue;
        }
        out.push(s);
    }
    out
}

/// Flow `items` into lines no wider than `width`, separated by gaps.
pub fn flow(items: Vec<Vec<Span<'static>>>, width: u16, gap: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut cur: Vec<Span<'static>> = Vec::new();
    let mut cur_w = 0usize;
    for item in items {
        let w: usize = item.iter().map(|s| s.width()).sum();
        if cur_w > 0 && cur_w + gap + w > width as usize {
            lines.push(Line::from(std::mem::take(&mut cur)));
            cur_w = 0;
        }
        if cur_w > 0 {
            cur.push(Span::raw(" ".repeat(gap)));
            cur_w += gap;
        }
        cur_w += w;
        cur.extend(item);
    }
    if !cur.is_empty() {
        lines.push(Line::from(cur));
    }
    lines
}

/// Label/value pair for stat strips.
pub fn stat(t: &Theme, label: &str, value: String, color: Option<Color>) -> Vec<Span<'static>> {
    vec![
        Span::styled(format!("{label} "), Style::default().fg(t.graph_text)),
        Span::styled(
            value,
            Style::default()
                .fg(color.unwrap_or(t.title))
                .add_modifier(Modifier::BOLD),
        ),
    ]
}

/// Centered dim message inside an area (loading / empty / error states).
pub fn message(buf: &mut Buffer, area: Rect, text: &str, color: Color) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let s = fmt::trunc(text, area.width as usize);
    let w = fmt::width(&s) as u16;
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(1) / 2;
    buf.set_string(x, y, s, Style::default().fg(color));
}

/// Health cell glyph and colour for a success/failure count (`None` when
/// there were no requests). Without colour the glyph carries the level:
/// `■` ≥ 95 %, `▲` ≥ 50 %, `✗` below. Maps and legends both use this.
pub fn health_glyph(t: &Theme, success: f64, failure: f64) -> Option<(char, Color)> {
    let c = health_color(t, success, failure)?;
    let rate = success / (success + failure);
    let ch = if !t.no_color || rate >= 0.95 {
        '■'
    } else if rate >= 0.5 {
        '▲'
    } else {
        '✗'
    };
    Some((ch, c))
}

/// Colour for an activity-health block (success rate with the web thresholds).
pub fn health_color(t: &Theme, success: f64, failure: f64) -> Option<Color> {
    let total = success + failure;
    if total <= 0.0 {
        return None;
    }
    let rate = success / total;
    let good = (0.9 + 0.045 * (total / 10.0).max(1e-9).log10()).min(0.99);
    Some(if rate < 0.5 {
        t.err
    } else if rate < 0.65 {
        mix(t.err, t.warn)
    } else if rate < 0.8 {
        t.warn
    } else if rate < good {
        mix(t.warn, t.ok)
    } else {
        t.ok
    })
}

fn mix(a: Color, b: Color) -> Color {
    match (a, b) {
        (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => Color::Rgb(
            ((r1 as u16 + r2 as u16) / 2) as u8,
            ((g1 as u16 + g2 as u16) / 2) as u8,
            ((b1 as u16 + b2 as u16) / 2) as u8,
        ),
        _ => b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampling_never_interpolates() {
        assert!(resample(&[], 3).iter().all(|v| v.is_nan()));
        // Stretching repeats buckets instead of inventing values in between.
        assert_eq!(resample(&[1.0, 3.0], 4), vec![1.0, 1.0, 3.0, 3.0]);
        assert_eq!(resample(&[1.0, 5.0, 2.0, 0.0], 2), vec![5.0, 2.0]);
        // Missing values stay missing, pooled groups keep real values.
        let r = resample(&[f64::NAN, 2.0, f64::NAN, f64::NAN], 2);
        assert_eq!(r[0], 2.0);
        assert!(r[1].is_nan());
    }

    #[test]
    fn bucket_columns_match_resampling() {
        for (n, w) in [(3usize, 10usize), (24, 90), (7, 7), (100, 40)] {
            let data: Vec<f64> = (0..n).map(|i| i as f64).collect();
            let cols = resample(&data, w);
            for i in 0..n {
                let (a, b) = bucket_cols(i, n, w);
                assert!(a < b && b <= w, "n={n} w={w} i={i}");
                if n <= w {
                    assert!(
                        cols[a..b].iter().all(|v| *v == i as f64),
                        "n={n} w={w} i={i}"
                    );
                }
            }
        }
    }

    #[test]
    fn sparklines_show_gaps_and_do_not_stretch() {
        let t = Theme::default();
        let s = spark(&[f64::NAN, 1.0, f64::NAN, 2.0], 8, &t, Color::Blue);
        let text: String = s.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text, " ▅ █    ");
    }

    #[test]
    fn meters_are_readable_without_colour() {
        let t = Theme {
            no_color: true,
            ..Default::default()
        };
        let text: String = meter(0.5, 10, &t, None)
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert_eq!(text, "■■■■■·····");
    }

    #[test]
    fn meter_fills() {
        let t = Theme::default();
        let s = meter(0.5, 10, &t, Some(Color::Red));
        let text: String = s.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text.chars().count(), 10);
        assert_eq!(s[0].style.fg, Some(Color::Red));
    }

    #[test]
    fn braille_graph_renders() {
        let t = Theme::default();
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        Graph {
            data: &[0.0, 1.0, 2.0, 4.0],
            theme: &t,
            symbols: GraphSymbols::Braille,
            max: None,
            color: None,
        }
        .render(buf.area, &mut buf);
        // The right-most column reaches the top row.
        assert_ne!(buf[(3, 0)].symbol(), " ");
        // Low values never reach the top row; zero draws nothing.
        assert_eq!(buf[(1, 0)].symbol(), " ");
        assert_ne!(buf[(1, 1)].symbol(), " ");
        assert_eq!(buf[(0, 1)].symbol(), " ");
    }
}
