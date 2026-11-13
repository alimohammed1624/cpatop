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

/// Resample a series to `n` points: max-pool when shrinking, linear
/// interpolation when stretching.
pub fn resample(data: &[f64], n: usize) -> Vec<f64> {
    if n == 0 {
        return vec![];
    }
    match data.len() {
        0 => vec![0.0; n],
        1 => vec![data[0]; n],
        len if len >= n => (0..n)
            .map(|j| {
                let a = j * len / n;
                let b = ((j + 1) * len / n).max(a + 1).min(len);
                data[a..b].iter().copied().fold(f64::MIN, f64::max)
            })
            .collect(),
        len => (0..n)
            .map(|j| {
                if n == 1 {
                    return data[len - 1];
                }
                let pos = j as f64 * (len - 1) as f64 / (n - 1) as f64;
                let i = pos.floor() as usize;
                let f = pos - i as f64;
                if i + 1 >= len {
                    data[len - 1]
                } else {
                    data[i] * (1.0 - f) + data[i + 1] * f
                }
            })
            .collect(),
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
        let per_cell = if self.symbols == GraphSymbols::Braille {
            2
        } else {
            1
        };
        let vals = resample(self.data, w * per_cell);
        let max = self
            .max
            .unwrap_or_else(|| vals.iter().copied().fold(0.0, f64::max))
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
                        let mut bits = 0u32;
                        for (k, table) in [(0, LEFT), (1, RIGHT)] {
                            let hh = hs[cx * 2 + k];
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

/// btop-style meter made of `■` cells.
pub fn meter(frac: f64, width: usize, t: &Theme, color: Option<Color>) -> Vec<Span<'static>> {
    if width == 0 {
        return vec![];
    }
    let frac = if frac.is_finite() {
        frac.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let filled = (frac * width as f64).round() as usize;
    let mut out = Vec::with_capacity(width);
    for i in 0..width {
        if i < filled {
            let c = color.unwrap_or_else(|| t.gradient(i as f32 / (width.max(2) - 1) as f32));
            out.push(Span::styled("■", Style::default().fg(c)));
        } else {
            out.push(Span::styled("■", Style::default().fg(t.meter_bg)));
        }
    }
    merge_spans(out)
}

/// Mini sparkline using block eighths, coloured by level.
pub fn spark(values: &[f64], width: usize, t: &Theme) -> Vec<Span<'static>> {
    const B: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if width == 0 {
        return vec![];
    }
    let vals = resample(values, width);
    let max = vals.iter().copied().fold(0.0, f64::max);
    let out = vals
        .iter()
        .map(|v| {
            if *v <= 0.0 || max <= 0.0 {
                Span::styled("▁", Style::default().fg(t.meter_bg))
            } else {
                let f = (v / max).clamp(0.0, 1.0);
                let i = ((f * 7.0).round() as usize).min(7);
                Span::styled(B[i].to_string(), Style::default().fg(t.gradient(f as f32)))
            }
        })
        .collect();
    merge_spans(out)
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
    fn resampling() {
        assert_eq!(resample(&[], 3), vec![0.0; 3]);
        assert_eq!(resample(&[1.0, 3.0], 3), vec![1.0, 2.0, 3.0]);
        assert_eq!(resample(&[1.0, 5.0, 2.0, 0.0], 2), vec![5.0, 2.0]);
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
        // Low values on the left never reach the top row.
        assert_eq!(buf[(0, 0)].symbol(), " ");
        assert_ne!(buf[(0, 1)].symbol(), " ");
    }
}
