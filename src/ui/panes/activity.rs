//! Overview activity heatmaps: request health and tokens per time cell.
//!
//! The server sends 364 blocks (7×52) of a fixed size. Drawn row-major as
//! the web app does, rows start at odd times (03:25, 06:51) and the grid
//! never grows with the pane. Here the blocks are re-gridded in local time:
//! each row covers a whole span (1 h … 12 h, 1 d … for longer windows) that
//! starts on a whole hour or midnight, and the span and cell width are
//! chosen to fill the pane — more rows, wider cells, or both.

use super::{frame, placeholder, stale_marker};
use crate::api::models::Activity;
use crate::app::App;
use crate::fmt;
use crate::layout::{PaneId, Size};
use crate::theme::Theme;
use crate::ui::widgets::{flow, health_glyph, heat_cell, stat};
use chrono::{DateTime, FixedOffset, TimeZone};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::borrow::Cow;

const H: i64 = 3600;
const D: i64 = 86_400;
/// Row-label column: `23:00` / `Thu 06` / `11-13` plus a gap.
const LABEL_W: u16 = 7;

/// Row spans to try for a grain, finest first.
fn spans_for(grain: &str) -> &'static [i64] {
    match grain {
        "short" => &[H, 2 * H, 3 * H, 4 * H, 6 * H, 8 * H, 12 * H],
        "medium" => &[2 * H, 3 * H, 4 * H, 6 * H, 8 * H, 12 * H, D],
        "long" => &[D, 2 * D, 3 * D, 5 * D, 6 * D, 10 * D],
        _ => &[7 * D, 14 * D, 28 * D, 52 * D],
    }
}

/// Blocks as `(start, success, failure, tokens)` in unix seconds, plus the
/// window and the server's local offset.
struct Blocks {
    list: Vec<(i64, f64, f64, f64)>,
    start: i64,
    end: i64,
    block: i64,
    off: FixedOffset,
    grain: String,
}

impl Blocks {
    fn new(a: &Activity, fallback: FixedOffset) -> Option<Blocks> {
        let list: Vec<(i64, f64, f64, f64)> = a
            .blocks
            .iter()
            .filter_map(|b| {
                let s = fmt::parse_ts(&b.start_time)?.timestamp();
                Some((s, b.success, b.failure, b.total_tokens))
            })
            .collect();
        let ws = fmt::parse_ts(&a.window_start)?;
        let we = fmt::parse_ts(&a.window_end)?;
        // Medium/long grids come in UTC; rows still start at local times.
        let off = if a.window_start.ends_with('Z') {
            fallback
        } else {
            *ws.offset()
        };
        Some(Blocks {
            list,
            start: ws.timestamp(),
            end: we.timestamp(),
            block: (a.bucket_seconds.round() as i64).max(1),
            off,
            grain: a.grain.clone(),
        })
    }

    fn local(&self, t: i64) -> DateTime<FixedOffset> {
        self.off
            .timestamp_opt(t, 0)
            .single()
            .unwrap_or_else(|| self.off.timestamp_opt(0, 0).unwrap())
    }

    /// Row spans to try, finest first: those of the grain no longer than
    /// the window (a six-day range gets week rows, not 52-day ones).
    fn spans(&self) -> Vec<i64> {
        let all = spans_for(&self.grain);
        let len = (self.end - self.start).max(all[0]);
        all.iter().copied().filter(|s| *s <= len).collect()
    }

    /// Local midnight at or before `t`.
    fn midnight(&self, t: i64) -> i64 {
        let l = self.local(t);
        let secs = l.time().num_seconds_from_midnight() as i64;
        t - secs
    }
}

use chrono::Timelike;

/// A re-gridded layout: `rows` rows of `span` seconds from `origin`, `cols`
/// cells per row, each `cw` characters wide and `rh` lines tall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Grid {
    origin: i64,
    span: i64,
    rows: usize,
    cols: usize,
    cw: u16,
    rh: u16,
    /// More blocks than cells per row: several blocks share a cell.
    pooled: bool,
}

impl Grid {
    /// The grid for a row span with cells as wide as fit (up to `max_cw`).
    fn fit(b: &Blocks, span: i64, avail: u16, rh: u16, max_cw: u16) -> Grid {
        let mut g = Grid::new(b, span, avail, rh);
        if !g.pooled {
            let per_row = g.cols as u16;
            g.cw = (avail / per_row.max(1)).clamp(1, max_cw);
        }
        g
    }

    fn new(b: &Blocks, span: i64, avail: u16, rh: u16) -> Grid {
        let mid = b.midnight(b.start);
        let origin = if span < D {
            mid + (b.start - mid).div_euclid(span) * span
        } else {
            mid
        };
        let rows = ((b.end - origin) + span - 1).div_euclid(span).max(1) as usize;
        let per_row = ((span as f64 / b.block as f64).round() as usize).max(1);
        let avail = avail as usize;
        let (cols, cw, pooled) = if per_row * 2 <= avail {
            (per_row, 2, false)
        } else if per_row <= avail {
            (per_row, 1, false)
        } else {
            (avail.max(1), 1, true)
        };
        Grid {
            origin,
            span,
            rows,
            cols,
            cw,
            rh,
            pooled,
        }
    }

    fn cell_of(&self, t: i64) -> Option<(usize, usize)> {
        let d = t - self.origin;
        if d < 0 {
            return None;
        }
        let r = (d / self.span) as usize;
        let c = ((d % self.span) as i128 * self.cols as i128 / self.span as i128) as usize;
        (r < self.rows).then_some((r, c.min(self.cols - 1)))
    }

    fn cell_span(&self, r: usize, c: usize) -> (i64, i64) {
        let base = self.origin + r as i64 * self.span;
        let s = base + (c as i64 * self.span) / self.cols as i64;
        let e = base + ((c as i64 + 1) * self.span) / self.cols as i64;
        (s, e)
    }

    fn lines(&self) -> u16 {
        self.rows as u16 * self.rh
    }

    fn width(&self) -> u16 {
        self.cols as u16 * self.cw
    }
}

/// The layout plan: grid, whether both maps fit (stacked, or side by side
/// on wide panes), and whether each gets a title line (otherwise the names
/// go into the legend).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plan {
    grid: Grid,
    both: bool,
    side: bool,
    titled: bool,
}

/// Pick the grid that fills `w × h` best: both maps first, then no pooled
/// cells, then the largest drawn area, then section titles, then more rows.
/// Cells keep a sane shape: one or two lines tall, at least as wide (in
/// cells of two characters) as they are tall.
fn plan(b: &Blocks, w: u16, h: u16) -> Plan {
    plan_with(b, w, h, 2)
}

/// [`plan`] with cells at most `max_rh` lines tall.
fn plan_with(b: &Blocks, w: u16, h: u16, max_rh: u16) -> Plan {
    let full = w.saturating_sub(LABEL_W + 1);
    let half = (w.saturating_sub(1) / 2).saturating_sub(LABEL_W + 1);
    // (both maps, no pooling, drawn area, titled, rows): higher is better.
    type Score = (bool, bool, u32, bool, usize);
    let mut best: Option<(Plan, Score)> = None;
    for span in b.spans() {
        for rh in (1..=max_rh).filter(|r| *r <= 2) {
            for side in [false, true] {
                let avail = if side { half } else { full };
                if avail < 8 {
                    continue;
                }
                let g = Grid::fit(b, span, avail, rh, 2 * rh);
                if g.cw < 2 * rh - 1 || (rh == 2 && g.cw < 4) {
                    continue;
                }
                for (both, titled) in [(true, true), (true, false), (false, true), (false, false)] {
                    // Two maps are told apart by their titles.
                    if (side && !both) || (both && !titled) {
                        continue;
                    }
                    let n = if both && !side { 2 } else { 1 };
                    if (g.lines() + titled as u16) * n > h {
                        continue;
                    }
                    let maps = if both { 2 } else { 1 };
                    let area = g.lines() as u32 * g.width() as u32 * maps;
                    let score = (both, !g.pooled, area, titled, g.rows);
                    if best.as_ref().is_none_or(|(_, s)| score > *s) {
                        best = Some((
                            Plan {
                                grid: g,
                                both,
                                side,
                                titled,
                            },
                            score,
                        ));
                    }
                }
            }
        }
    }
    best.map(|(p, _)| p).unwrap_or_else(|| {
        // Nothing fits: the coarsest grid, clipped to the rows available.
        let span = *b.spans().last().unwrap_or(&D);
        Plan {
            grid: Grid::new(b, span, full, 1),
            both: false,
            side: false,
            titled: false,
        }
    })
}

fn activity_head(app: &App, a: &Activity) -> Vec<Vec<Span<'static>>> {
    let t = app.theme();
    let any = a.total_success + a.total_failure > 0.0;
    let rate = if any {
        fmt::pct(a.success_rate, 1)
    } else {
        "--".into()
    };
    vec![
        vec![
            Span::styled("success ", Style::default().fg(t.graph_text)),
            Span::styled(
                rate,
                Style::default()
                    .fg(if any {
                        t.good_pct(a.success_rate, 95.0, 80.0)
                    } else {
                        t.graph_text
                    })
                    .add_modifier(Modifier::BOLD),
            ),
        ],
        vec![
            Span::styled(
                format!("✓{}", fmt::int(a.total_success)),
                Style::default().fg(t.ok),
            ),
            Span::styled(
                format!(" ✗{}", fmt::int(a.total_failure)),
                Style::default().fg(if a.total_failure > 0.0 {
                    t.err
                } else {
                    t.graph_text
                }),
            ),
        ],
        stat(t, "Σ tokens", fmt::compact(a.total_tokens), None),
    ]
}

/// The server's grid has a fixed span per window and ignores the selected
/// range's own span: any custom day range gets the last year, 2–7 days the
/// last week, 8–29 days the last 30 days, 5–23 hours the last 24 hours.
/// What is shown is cropped to the selected range (totals recomputed from
/// the blocks inside it); the second value names the server's window when
/// it was cropped.
fn shown<'a>(app: &App, a: &'a Activity) -> (Cow<'a, Activity>, Option<&'static str>) {
    let (rs, re) = app.range.bounds(app.server_now());
    match crop(a, rs, re) {
        Some((c, server)) => (Cow::Owned(c), Some(server)),
        None => (Cow::Borrowed(a), None),
    }
}

/// `a` cut to `[rs, re)`, with the server window's name, or `None` when the
/// server's window already matches the range (within half a block).
fn crop(
    a: &Activity,
    rs: DateTime<FixedOffset>,
    re: DateTime<FixedOffset>,
) -> Option<(Activity, &'static str)> {
    let ws = fmt::parse_ts(&a.window_start)?.timestamp();
    let we = fmt::parse_ts(&a.window_end)?.timestamp();
    let half = (a.bucket_seconds / 2.0).max(1.0) as i64;
    let (s, e) = (ws.max(rs.timestamp()), we.min(re.timestamp()));
    if (s - ws <= half && we - e <= half) || e <= s {
        return None;
    }
    let server = match a.window.as_str() {
        "day" => "24h",
        "week" => "7d",
        "month" => "30d",
        "year" => "1y",
        _ => "?",
    };
    let mut c = a.clone();
    // A block belongs to the range when most of it is inside.
    c.blocks.retain(|b| {
        let (Some(bs), Some(be)) = (fmt::parse_ts(&b.start_time), fmt::parse_ts(&b.end_time))
        else {
            return false;
        };
        let mid = (bs.timestamp() + be.timestamp()) / 2;
        (s..e).contains(&mid)
    });
    let sum =
        |f: fn(&crate::api::models::ActivityBlock) -> f64| c.blocks.iter().map(f).sum::<f64>();
    c.total_success = sum(|b| b.success);
    c.total_failure = sum(|b| b.failure);
    let n = c.total_success + c.total_failure;
    c.success_rate = if n > 0.0 {
        c.total_success / n * 100.0
    } else {
        0.0
    };
    c.input_tokens = sum(|b| b.input_tokens);
    c.output_tokens = sum(|b| b.output_tokens);
    c.reasoning_tokens = sum(|b| b.reasoning_tokens);
    c.cache_read_tokens = sum(|b| b.cache_read_tokens);
    c.cache_creation_tokens = sum(|b| b.cache_creation_tokens);
    c.total_tokens = sum(|b| b.total_tokens);
    let off = *rs.offset();
    let ts = |t: i64| {
        off.timestamp_opt(t, 0)
            .single()
            .map(|d| d.to_rfc3339())
            .unwrap_or_default()
    };
    c.window_start = ts(s);
    c.window_end = ts(e);
    Some((c, server))
}

pub fn activity_size(app: &App, w: u16) -> Size {
    let Some(a) = app.data.activity.get() else {
        return Size::at_least(6);
    };
    let (a, _) = shown(app, a);
    let a = a.as_ref();
    let head = flow(activity_head(app, a), w.saturating_sub(3), 2).len() as u16;
    let Some(b) = Blocks::new(a, *app.server_now().offset()) else {
        return Size::at_least(6);
    };
    let span = *b.spans().last().unwrap_or(&D);
    let rows = Grid::new(&b, span, w, 1).rows as u16;
    // Borders, head, legend and the coarsest single map at least.
    let min = 2 + head + 1 + rows;
    // Rows beyond what the maps can use at one line per row stay blank, so
    // the pane asks for no more (taller cells only come from rows nobody
    // else can use).
    let inner_w = w.saturating_sub(2);
    // The finest rows, one line each: side by side when both maps fit the
    // width, stacked otherwise.
    let finest = b.spans()[0];
    let half = (inner_w.saturating_sub(1) / 2).saturating_sub(LABEL_W + 1);
    let full = inner_w.saturating_sub(LABEL_W + 1);
    let side = Grid::fit(&b, finest, half, 1, 2);
    let maps_h = if half >= 8 && !side.pooled {
        side.lines() + 1
    } else {
        2 * (Grid::fit(&b, finest, full, 1, 2).lines() + 1)
    };
    let p = plan_with(&b, inner_w, maps_h, 1);
    let legend = flow(legend_items(app.theme(), &p), inner_w.saturating_sub(1), 1).len() as u16;
    let pref = 2 + head + maps_h + legend;
    Size::range(min, pref)
}

/// Legend items: map names (when untitled), health glyphs, the token ramp
/// and the cell/row sizes. Glyphs come from the same functions as the maps.
fn legend_items(t: &Theme, p: &Plan) -> Vec<Vec<Span<'static>>> {
    let dim = Style::default().fg(t.graph_text);
    let name = |s: &str| Span::styled(s.to_string(), Style::default().fg(t.title));
    // Small items so the legend wraps between entries, never inside one.
    let mut items: Vec<Vec<Span<'static>>> = Vec::new();
    if !p.titled {
        items.push(vec![name("health")]);
    }
    let samples: &[(f64, f64, &str)] = if t.no_color {
        &[
            (100.0, 0.0, "≥95%"),
            (70.0, 30.0, "≥50%"),
            (10.0, 90.0, "<50%"),
        ]
    } else {
        &[
            (100.0, 0.0, "ok"),
            (85.0, 15.0, "≥80%"),
            (60.0, 40.0, "≥50%"),
            (10.0, 90.0, "<50%"),
        ]
    };
    for (s, f, label) in samples {
        if let Some((ch, c)) = health_glyph(t, *s, *f) {
            items.push(vec![
                Span::styled(ch.to_string(), Style::default().fg(c)),
                Span::styled(label.to_string(), dim),
            ]);
        }
    }
    let (ch, c) = heat_cell(None, t.meter_bg, t);
    items.push(vec![
        Span::styled(ch.to_string(), Style::default().fg(c)),
        Span::styled("idle ", dim),
    ]);
    if p.both {
        let mut tok = Vec::new();
        if !p.titled {
            tok.push(name("tokens "));
        }
        for k in 0..4 {
            let lv = k as f64 / 3.0;
            let (ch, c) = heat_cell(Some(lv), t.ramp(lv as f32), t);
            tok.push(Span::styled(ch.to_string(), Style::default().fg(c)));
        }
        tok.push(Span::styled(" low→high ", dim));
        items.push(tok);
    }
    let exact = p.grid.span % p.grid.cols as i64 == 0;
    items.push(vec![Span::styled(
        format!(
            "cell {}{} · row {}",
            if exact { "" } else { "≈" },
            short_dur(p.grid.span / p.grid.cols as i64),
            short_dur(p.grid.span)
        ),
        dim,
    )]);
    items
}

/// `4m`, `2h`, `3d` — rounded to the largest unit.
fn short_dur(s: i64) -> String {
    if s >= D && s % D == 0 {
        format!("{}d", s / D)
    } else if s >= H && s % H == 0 {
        format!("{}h", s / H)
    } else if s < 90 {
        format!("{s}s")
    } else if s < 90 * 60 {
        format!("{}m", (s + 30) / 60)
    } else if s < 36 * H {
        format!("{}h", (s + H / 2) / H)
    } else {
        format!("{}d", (s + D / 2) / D)
    }
}

pub fn activity(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    let t = app.theme();
    let l = &app.data.activity;
    let mut right = stale_marker(app, l);
    let shown = l.get().map(|a| shown(app, a));
    if let Some((a, cropped)) = &shown
        && right.is_empty()
    {
        let pat = match a.grain.as_str() {
            "daily" if area.width >= 60 => "%Y-%m-%d",
            "daily" | "long" => "%m-%d",
            _ if area.width >= 60 => "%m-%d %H:%M",
            _ => "%H:%M",
        };
        let off = *app.server_now().offset();
        let parse = |s: &str| fmt::parse_ts(s).map(|d| d.with_timezone(&off));
        let text = match (parse(&a.window_start), parse(&a.window_end)) {
            // Cropped to the selected range: the range, then the server's
            // window, so it is clear the server answered for a longer span.
            (Some(_), Some(_)) if cropped.is_some() => {
                let server = cropped.unwrap_or_default();
                let range = app.range.label();
                if area.width >= 50 {
                    format!("{range} · server window {server}")
                } else {
                    format!("{range} · of {server}")
                }
            }
            // A whole local day (today, yesterday): just the date.
            (Some(s), Some(e))
                if (e - s).num_seconds() == D && s.time() == chrono::NaiveTime::MIN =>
            {
                s.format("%a %m-%d").to_string()
            }
            // The server's grid has a fixed span per window (it does not
            // follow the selected range: 14d shows the last 30 days), so
            // the span is named.
            (Some(s), Some(e)) => {
                let span = match a.window.as_str() {
                    "day" => "24h",
                    "week" => "7d",
                    "month" => "30d",
                    "year" => "364d",
                    _ => "",
                };
                if span.is_empty() || area.width < 50 {
                    format!("{} → {}", s.format(pat), e.format(pat))
                } else {
                    format!("last {span} · {} → {}", s.format(pat), e.format(pat))
                }
            }
            _ => String::new(),
        };
        right.push(Span::styled(text, Style::default().fg(t.graph_text)));
    }
    let inner = frame(f, app, PaneId::OvActivity, area, focused, right, None);
    if placeholder(f, app, inner, l) {
        return;
    }
    let Some((a, _)) = &shown else { return };
    let a = a.as_ref();
    let head = flow(activity_head(app, a), inner.width.saturating_sub(1), 2);
    let hh = (head.len() as u16).min(inner.height);
    f.render_widget(
        Paragraph::new(head),
        Rect::new(inner.x + 1, inner.y, inner.width.saturating_sub(1), hh),
    );
    let Some(b) = Blocks::new(a, *app.server_now().offset()) else {
        return;
    };
    let body_h = inner.height.saturating_sub(hh);
    if body_h < 2 {
        return;
    }
    // Plan with a one-line legend, then see whether the legend needs two.
    let mut p = plan(&b, inner.width, body_h - 1);
    let lw = inner.width.saturating_sub(1);
    let mut legend = flow(legend_items(t, &p), lw, 1);
    if legend.len() > 1 {
        let p2 = plan(&b, inner.width, body_h.saturating_sub(legend.len() as u16));
        if p2.both == p.both && body_h > legend.len() as u16 {
            p = p2;
            legend = flow(legend_items(t, &p), lw, 1);
        } else {
            // Keep the maps; the legend keeps its first line (the glyphs).
            legend.truncate(1);
        }
    }
    let lh = legend.len() as u16;
    let grid_h = body_h - lh;
    let y0 = inner.y + hh;
    let block_h = ((p.grid.lines() + p.titled as u16).min(grid_h)).max(1);
    let now = app.clock.utc().timestamp();
    let maps = if p.both { 2 } else { 1 };
    let half = inner.width / 2;
    for m in 0..maps {
        let (x, y, w) = if p.side {
            (
                inner.x + m * half,
                y0,
                if m == 0 { half } else { inner.width - half },
            )
        } else {
            (inner.x, y0 + m * block_h, inner.width)
        };
        if y >= y0 + grid_h {
            break;
        }
        let h = block_h.min(y0 + grid_h - y);
        draw_map(
            f.buffer_mut(),
            t,
            Rect::new(x, y, w, h),
            &b,
            &p,
            m == 1,
            now,
        );
    }
    f.render_widget(
        Paragraph::new(legend),
        Rect::new(inner.x + 1, inner.bottom() - lh, lw, lh),
    );
}

/// Draw one map (health or tokens) with its optional title line.
fn draw_map(buf: &mut Buffer, t: &Theme, area: Rect, b: &Blocks, p: &Plan, tokens: bool, now: i64) {
    let g = &p.grid;
    let dim = Style::default().fg(t.graph_text);
    let mut y = area.y;
    if p.titled {
        let (title, what) = match (tokens, p.side) {
            (true, false) => ("tokens", " · tokens per cell"),
            (false, false) => ("health", " · success rate per cell"),
            (true, true) => ("tokens", " per cell"),
            (false, true) => ("health", " · success rate"),
        };
        // The description goes when it would be cut.
        let room = area.width.saturating_sub(2) as usize;
        let what = if title.len() + what.chars().count() > room {
            match tokens {
                true if room >= 15 => " per cell",
                false if room >= 21 => " · success rate",
                _ => "",
            }
        } else {
            what
        };
        buf.set_line(
            area.x + 1,
            y,
            &Line::from(vec![
                Span::styled(
                    title,
                    Style::default().fg(t.title).add_modifier(Modifier::BOLD),
                ),
                Span::styled(what, dim),
            ]),
            area.width.saturating_sub(2),
        );
        y += 1;
    }
    // Aggregate blocks into cells.
    let mut cells = vec![(0.0f64, 0.0f64, 0.0f64, false); g.rows * g.cols];
    for (s, ok, fail, tok) in &b.list {
        if let Some((r, c)) = g.cell_of(*s) {
            let e = &mut cells[r * g.cols + c];
            e.0 += ok;
            e.1 += fail;
            e.2 += tok;
            e.3 = true;
        }
    }
    // Token levels: log scale between P5 and P95 of non-empty cells.
    let mut nz: Vec<f64> = cells.iter().map(|c| c.2).filter(|v| *v > 0.0).collect();
    nz.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let pick = |q: f64| {
        if nz.is_empty() {
            1.0
        } else {
            nz[((nz.len() - 1) as f64 * q).round() as usize]
        }
    };
    let (lo, hi) = (pick(0.05).max(1.0).ln(), pick(0.95).max(1.0).ln());
    let label_fmt = if g.span >= D {
        "%m-%d"
    } else if b.end - b.start > D + H {
        "%a %H"
    } else {
        "%H:%M"
    };
    for r in 0..g.rows {
        let ry = y + r as u16 * g.rh;
        if ry >= area.bottom() {
            break;
        }
        let label = b.local(g.origin + r as i64 * g.span).format(label_fmt);
        buf.set_string(area.x + 1, ry, label.to_string(), dim);
        let mut x = area.x + LABEL_W;
        for c in 0..g.cols {
            if x + g.cw > area.right() {
                break;
            }
            let (cs, ce) = g.cell_span(r, c);
            let (ok, fail, tok, seen) = cells[r * g.cols + c];
            // Outside the window or in the future: blank.
            let (ch, color) = if ce <= b.start || cs >= b.end || cs > now {
                (' ', t.bg)
            } else if tokens {
                if tok <= 0.0 {
                    heat_cell(None, t.meter_bg, t)
                } else {
                    let lv = if hi > lo {
                        ((tok.ln() - lo) / (hi - lo)).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    heat_cell(Some(lv), t.ramp(lv as f32), t)
                }
            } else {
                match health_glyph(t, ok, fail) {
                    Some(g) => g,
                    None => heat_cell(None, t.meter_bg, t),
                }
            };
            let _ = seen;
            for dy in 0..g.rh {
                for dx in 0..g.cw {
                    let (xx, yy) = (x + dx, ry + dy);
                    // Wide cells keep a one-character gap so they stay distinct.
                    let ch = if g.cw >= 2 && dx == g.cw - 1 { ' ' } else { ch };
                    if yy < area.bottom() && xx < area.right() {
                        buf[(xx, yy)].set_char(ch).set_fg(color);
                    }
                }
            }
            x += g.cw;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks(start: &str, end: &str, block: i64, grain: &str) -> Blocks {
        let s = fmt::parse_ts(start).unwrap();
        let e = fmt::parse_ts(end).unwrap();
        Blocks {
            list: vec![],
            start: s.timestamp(),
            end: e.timestamp(),
            block,
            off: *s.offset(),
            grain: grain.into(),
        }
    }

    #[test]
    fn rows_start_on_whole_hours() {
        let b = blocks(
            "2026-11-13T00:00:00+05:30",
            "2026-11-14T00:00:00+05:30",
            238,
            "short",
        );
        let p = plan(&b, 60, 30);
        assert!(p.both);
        assert_eq!(p.grid.span % H, 0);
        assert_eq!(b.local(p.grid.origin).format("%H:%M").to_string(), "00:00");
        // A rolling 24 h window starting at 19:23 starts its rows at 19:00
        // (or earlier whole hours).
        let b = blocks(
            "2026-11-12T19:23:04+05:30",
            "2026-11-13T19:23:04+05:30",
            238,
            "short",
        );
        let g = Grid::new(&b, 2 * H, 60, 1);
        assert_eq!(b.local(g.origin).format("%M").to_string(), "00");
        assert!(g.origin <= b.start);
    }

    #[test]
    fn custom_days_are_cropped_from_the_server_year() {
        use crate::api::models::ActivityBlock;
        use crate::range::{Custom, TimeRange};
        // The server answers any custom day range with the last 364 days.
        let start = fmt::parse_ts("2025-11-15T00:00:00+05:30").unwrap();
        let blocks: Vec<ActivityBlock> = (0..364)
            .map(|i| {
                let s = start + chrono::Duration::days(i);
                ActivityBlock {
                    start_time: s.to_rfc3339(),
                    end_time: (s + chrono::Duration::days(1)).to_rfc3339(),
                    success: 10.0,
                    failure: if i % 2 == 0 { 1.0 } else { 0.0 },
                    total_tokens: 1000.0,
                    ..Default::default()
                }
            })
            .collect();
        let a = Activity {
            window: "year".into(),
            grain: "daily".into(),
            bucket_seconds: 86400.0,
            window_start: start.to_rfc3339(),
            window_end: "2026-11-14T00:00:00+05:30".into(),
            total_success: 3640.0,
            total_tokens: 364_000.0,
            blocks,
            ..Default::default()
        };
        let now = fmt::parse_ts("2026-11-13T22:00:00+05:30").unwrap();
        let day = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        let r = TimeRange::Custom(Custom::Days {
            start: day("2026-11-08"),
            end: day("2026-11-13"),
        });
        let (rs, re) = r.bounds(now);
        let (c, server) = crop(&a, rs, re).expect("cropped");
        assert_eq!(server, "1y");
        assert_eq!(c.blocks.len(), 6);
        assert_eq!(c.total_tokens, 6000.0);
        assert_eq!(c.total_success, 60.0);
        assert_eq!(c.window_start, "2026-11-08T00:00:00+05:30");
        assert_eq!(c.window_end, "2026-11-14T00:00:00+05:30");
        // The grid now starts at the range: one row of a week.
        let b = Blocks::new(&c, *now.offset()).unwrap();
        let p = plan(&b, 80, 20);
        assert_eq!(p.grid.rows, 1);
        // Today matches the server's window: nothing to crop.
        let today = Activity {
            window: "day".into(),
            bucket_seconds: 238.0,
            window_start: "2026-11-13T00:00:00+05:30".into(),
            window_end: "2026-11-14T00:00:00+05:30".into(),
            ..Default::default()
        };
        let (rs, re) = TimeRange::Today.bounds(now);
        assert!(crop(&today, rs, re).is_none());
        // 8 h of the server's 24 h: cropped.
        let (rs, re) = TimeRange::Hours(8).bounds(now);
        let day24 = Activity {
            window_start: "2026-11-12T22:00:00+05:30".into(),
            window_end: "2026-11-13T22:00:00+05:30".into(),
            ..today
        };
        let (c, server) = crop(&day24, rs, re).unwrap();
        assert_eq!(server, "24h");
        assert_eq!(c.window_start, "2026-11-13T14:00:00+05:30");
    }

    #[test]
    fn bigger_panes_get_more_rows_or_wider_cells() {
        let b = blocks(
            "2026-11-13T00:00:00+05:30",
            "2026-11-14T00:00:00+05:30",
            238,
            "short",
        );
        let small = plan(&b, 40, 12);
        let big = plan(&b, 90, 34);
        let area = |p: &Plan| p.grid.lines() as u32 * p.grid.width() as u32;
        assert!(area(&big) > 2 * area(&small), "{small:?} {big:?}");
        assert!(big.both && big.titled && !big.grid.pooled, "{big:?}");
    }
}
