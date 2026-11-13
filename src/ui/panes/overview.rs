//! Overview tab (admin and API-key viewer).

use super::{
    Series, chart, chart_head, cursor, cursor_time, dim_tabs, frame, placeholder, stale_marker,
    table_in, time_labels,
};
use crate::api::models::{Activity, ComparisonItem};
use crate::app::{App, Hit};
use crate::fmt;
use crate::layout::{PaneId, Size};
use crate::ui::table::{Align, Col, TRow, col, flex, grow, num, text};
use crate::ui::widgets::{self, flow, heat_cell, meter, stat};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::collections::HashMap;

fn totals_items(app: &App, width: u16) -> Vec<Vec<Span<'static>>> {
    let t = app.theme();
    let Some(o) = app.data.overview.get() else {
        return vec![];
    };
    let u = &o.usage;
    let s = &o.summary;
    let success = fmt::ratio_pct(u.success_count, u.total_requests);
    let cache = fmt::ratio_pct(s.cache_read_tokens, s.input_tokens);
    let mw = if width >= 120 { 12 } else { 8 };
    let mut items = vec![
        stat(t, "requests", fmt::int(u.total_requests), None),
        vec![
            Span::styled("✓ ", Style::default().fg(t.ok)),
            Span::styled(fmt::int(u.success_count), Style::default().fg(t.fg)),
            Span::styled("  ✗ ", Style::default().fg(t.err)),
            Span::styled(
                fmt::int(u.failure_count),
                Style::default().fg(if u.failure_count > 0.0 { t.err } else { t.fg }),
            ),
        ],
        {
            let mut v = vec![Span::styled("success ", Style::default().fg(t.graph_text))];
            let pct = success.unwrap_or(0.0);
            v.extend(meter(pct / 100.0, mw, t, Some(t.good_pct(pct, 95.0, 80.0))));
            v.push(Span::styled(
                format!(" {}", success.map(|p| fmt::pct(p, 2)).unwrap_or("-".into())),
                Style::default().fg(t.title).add_modifier(Modifier::BOLD),
            ));
            v
        },
        stat(t, "tokens", fmt::compact(u.total_tokens), None),
        stat(t, "in", fmt::compact(s.input_tokens), Some(t.fg)),
        stat(
            t,
            "out",
            fmt::compact((u.total_tokens - s.input_tokens).max(0.0)),
            Some(t.fg),
        ),
        stat(t, "cache r", fmt::compact(s.cache_read_tokens), Some(t.fg)),
        stat(
            t,
            "cache w",
            fmt::compact(s.cache_creation_tokens),
            Some(t.fg),
        ),
        stat(t, "reasoning", fmt::compact(s.reasoning_tokens), Some(t.fg)),
        {
            let mut v = stat(t, "cost", fmt::usd(s.total_cost), Some(t.accent));
            if !s.cost_available {
                v.push(Span::styled(" (partial)", Style::default().fg(t.warn)));
            }
            v
        },
        stat(t, "rpm", fmt::per_minute(s.rpm), None),
        stat(t, "tpm", fmt::compact(s.tpm), None),
        {
            let mut v = vec![Span::styled("cache ", Style::default().fg(t.graph_text))];
            v.extend(meter(cache.unwrap_or(0.0) / 100.0, mw, t, None));
            v.push(Span::styled(
                format!(" {}", cache.map(|p| fmt::pct(p, 1)).unwrap_or("-".into())),
                Style::default().fg(t.title).add_modifier(Modifier::BOLD),
            ));
            v
        },
    ];
    if let (Some(r), Some(tk), Some(c)) = (
        s.daily_average_requests,
        s.daily_average_tokens,
        s.daily_average_cost,
    ) {
        items.push(stat(
            t,
            "per day",
            format!(
                "{} req · {} tok · {}",
                fmt::count(r),
                fmt::compact(tk),
                fmt::usd(c)
            ),
            None,
        ));
    }
    items
}

/// Lines the totals need at an inner width.
pub fn totals_lines(app: &App, inner_w: u16) -> u16 {
    flow(totals_items(app, inner_w), inner_w, 3).len() as u16
}

pub fn totals(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    let t = app.theme();
    let l = &app.data.overview;
    let right = {
        let mut r = stale_marker(app, l);
        if r.is_empty() {
            r.push(Span::styled(
                app.range.label(),
                Style::default().fg(t.graph_text),
            ));
        }
        r
    };
    let inner = frame(f, app, PaneId::OvTotals, area, focused, right, None);
    if placeholder(f, app, inner, l) {
        return;
    }
    let w = inner.width.saturating_sub(1);
    let lines = flow(totals_items(app, w), w, 3);
    f.render_widget(
        Paragraph::new(lines),
        Rect::new(inner.x + 1, inner.y, w, inner.height),
    );
}

/// Bucket timestamps, tokens, requests and whether buckets are daily.
type Trend = (Vec<String>, Vec<f64>, Vec<f64>, bool);

/// Dense token series (from comparisons) and requests aligned to it.
fn trend_series(app: &App) -> Option<Trend> {
    let cmp = app.data.comparisons.get();
    let ov = app.data.overview.get();
    if let Some(c) = cmp.filter(|c| !c.buckets.is_empty()) {
        let n = c.buckets.len();
        let mut tokens = vec![0.0; n];
        for m in &c.models {
            for (i, v) in m.token_series.iter().enumerate().take(n) {
                tokens[i] += v;
            }
        }
        let mut reqs = vec![0.0; n];
        if let Some(o) = ov {
            let idx: HashMap<&str, usize> = c
                .buckets
                .iter()
                .enumerate()
                .map(|(i, b)| (b.as_str(), i))
                .collect();
            for (b, r) in o.series.buckets.iter().zip(&o.series.requests) {
                let key = if c.granularity == "daily" && b.len() > 10 {
                    &b[..10]
                } else {
                    b.as_str()
                };
                if let Some(i) = idx.get(key).or_else(|| idx.get(b.as_str())) {
                    reqs[*i] += r;
                }
            }
        }
        return Some((c.buckets.clone(), tokens, reqs, c.granularity == "daily"));
    }
    ov.map(|o| {
        (
            o.series.buckets.clone(),
            o.series.tokens.clone(),
            o.series.requests.clone(),
            app.range.is_daily(),
        )
    })
}

pub fn trend(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let series = trend_series(app);
    let right = match &series {
        Some((b, tok, _, daily)) if !b.is_empty() => {
            let peak = tok.iter().copied().fold(0.0, f64::max);
            let unit = if *daily { "/day" } else { "/h" };
            vec![Span::styled(
                format!(
                    "{} · peak {}{unit} · Σ {}",
                    app.range.label(),
                    fmt::compact(peak),
                    fmt::compact(tok.iter().sum())
                ),
                Style::default().fg(t.graph_text),
            )]
        }
        _ => vec![],
    };
    let inner = frame(f, app, PaneId::OvTrend, area, focused, right, None);
    let Some((buckets, tokens, reqs, daily)) = series else {
        if !placeholder(f, app, inner, &app.data.comparisons) {
            widgets::message(f.buffer_mut(), inner, "no data", t.graph_text);
        }
        return;
    };
    if tokens.iter().all(|v| *v == 0.0) && reqs.iter().all(|v| *v == 0.0) {
        widgets::message(
            f.buffer_mut(),
            inner,
            "no usage in this range",
            t.graph_text,
        );
        return;
    }
    let labels = time_labels(&buckets, daily, false);
    let n = buckets.len();
    let cur = cursor(app, PaneId::OvTrend, n);
    let label = Style::default().fg(t.graph_text);
    let at = |i: usize| {
        vec![Span::styled(
            format!("  @ {}", cursor_time(&buckets[i], daily)),
            Style::default().fg(t.hi),
        )]
    };
    let head = |name: &str, v: &[f64], f2: fn(f64) -> String| match cur {
        Some(i) => chart_head(app, name, f2(v[i]), at(i)),
        None => chart_head(
            app,
            name,
            f2(v.iter().sum()),
            vec![Span::styled(
                format!("  peak {}", f2(v.iter().copied().fold(0.0, f64::max))),
                label,
            )],
        ),
    };
    let x = inner.x + 1;
    let w = inner.width.saturating_sub(1);
    if inner.height >= 12 {
        let th = inner.height * 3 / 5;
        f.buffer_mut()
            .set_line(x, inner.y, &head("tokens", &tokens, fmt::compact), w);
        chart(
            f,
            app,
            hit,
            PaneId::OvTrend,
            Rect::new(x, inner.y + 1, w, th - 1),
            &labels,
            &Series {
                values: &tokens,
                fmt: fmt::compact,
                color: None,
                max: None,
                floor: 0.0,
            },
            app.settings.graph_symbols,
            true,
        );
        let ry = inner.y + th;
        let rh = inner.height - th;
        f.buffer_mut()
            .set_line(x, ry, &head("requests", &reqs, fmt::int), w);
        chart(
            f,
            app,
            hit,
            PaneId::OvTrend,
            Rect::new(x, ry + 1, w, rh.saturating_sub(1)),
            &labels,
            &Series {
                values: &reqs,
                fmt: fmt::count,
                color: Some(t.accent),
                max: None,
                floor: 0.0,
            },
            app.settings.graph_symbols,
            false,
        );
    } else {
        f.buffer_mut()
            .set_line(x, inner.y, &head("tokens", &tokens, fmt::compact), w);
        chart(
            f,
            app,
            hit,
            PaneId::OvTrend,
            Rect::new(x, inner.y + 1, w, inner.height.saturating_sub(1)),
            &labels,
            &Series {
                values: &tokens,
                fmt: fmt::compact,
                color: None,
                max: None,
                floor: 0.0,
            },
            app.settings.graph_symbols,
            true,
        );
    }
}

// ------------------------------------------------------------ activity

fn activity_head(app: &App, a: &Activity) -> Vec<Vec<Span<'static>>> {
    let t = app.theme();
    let rate = if a.total_success + a.total_failure > 0.0 {
        fmt::pct(a.success_rate, 1)
    } else {
        "--".into()
    };
    vec![
        vec![
            Span::styled("health ", Style::default().fg(t.graph_text)),
            Span::styled(
                rate,
                Style::default()
                    .fg(t.good_pct(a.success_rate, 95.0, 80.0))
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
        stat(t, "tokens", fmt::compact(a.total_tokens), None),
    ]
}

/// Width of the row-label column of the activity grids.
const ROW_LABEL: u16 = 8;

pub fn activity_size(app: &App, w: u16) -> Size {
    let Some(a) = app.data.activity.get() else {
        return Size::at_least(6);
    };
    let head = flow(activity_head(app, a), w.saturating_sub(3), 2).len() as u16;
    let rows = a.rows.max(1) as u16;
    // Head, health grid and legend at least; both grids at most.
    Size::range(head + rows + 1 + 2, head + 2 * rows + 2 + 2)
}

pub fn activity(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    let t = app.theme();
    let l = &app.data.activity;
    let mut right = stale_marker(app, l);
    if let Some(a) = l.get()
        && right.is_empty()
    {
        let pat = match a.grain.as_str() {
            "daily" => "%Y-%m-%d",
            "long" => "%m-%d",
            _ => "%m-%d %H:%M",
        };
        right.push(Span::styled(
            format!(
                "{} → {}",
                fmt::ts_fmt(&a.window_start, pat),
                fmt::ts_fmt(&a.window_end, pat)
            ),
            Style::default().fg(t.graph_text),
        ));
    }
    let inner = frame(f, app, PaneId::OvActivity, area, focused, right, None);
    if placeholder(f, app, inner, l) {
        return;
    }
    let Some(a) = l.get() else { return };
    let head = flow(activity_head(app, a), inner.width.saturating_sub(1), 2);
    let hh = (head.len() as u16).min(inner.height);
    f.render_widget(
        Paragraph::new(head),
        Rect::new(inner.x + 1, inner.y, inner.width.saturating_sub(1), hh),
    );
    let rows = a.rows.max(1) as usize;
    let cols = a.columns.max(1) as usize;
    let body = Rect::new(
        inner.x,
        inner.y + hh,
        inner.width,
        inner.height.saturating_sub(hh),
    );
    if body.height < 2 {
        return;
    }
    let now = app.clock.utc();
    let future: Vec<bool> = a
        .blocks
        .iter()
        .map(|b| fmt::parse_ts(&b.start_time).is_some_and(|s| s.with_timezone(&chrono::Utc) > now))
        .collect();
    // Legend takes the last row; grids get the rest. Prefer both grids,
    // then a title line per grid, then double-height cells.
    let legend_y = body.bottom() - 1;
    let grid_h = body.height - 1;
    let r16 = rows as u16;
    let (both, titled, rh) = if grid_h >= 2 * (2 * r16 + 1) {
        (true, true, 2)
    } else if grid_h >= 4 * r16 {
        (true, false, 2)
    } else if grid_h >= 2 * (r16 + 1) {
        (true, true, 1)
    } else if grid_h >= 2 * r16 {
        (true, false, 1)
    } else if grid_h > 2 * r16 {
        (false, true, 2)
    } else {
        (false, grid_h > r16, 1)
    };
    let block_h = (r16 * rh + titled as u16).min(grid_h);
    heat(
        f,
        app,
        Rect::new(body.x, body.y, body.width, block_h),
        a,
        rows,
        cols,
        rh,
        titled,
        &future,
        false,
    );
    if both {
        let ty = body.y + block_h;
        heat(
            f,
            app,
            Rect::new(body.x, ty, body.width, block_h.min(grid_h - block_h)),
            a,
            rows,
            cols,
            rh,
            titled,
            &future,
            true,
        );
    }
    legend(
        f,
        app,
        a,
        Rect::new(body.x + 1, legend_y, body.width.saturating_sub(1), 1),
        both,
    );
}

fn legend(f: &mut Frame, app: &App, a: &Activity, area: Rect, tokens: bool) {
    let t = app.theme();
    let dim = Style::default().fg(t.graph_text);
    let key = |c: Color, level: Option<f64>, label: &str| -> Vec<Span<'static>> {
        let (ch, col) = heat_cell(level, c, t);
        vec![
            Span::styled(ch.to_string(), Style::default().fg(col)),
            Span::styled(format!("{label} "), dim),
        ]
    };
    let mut v = Vec::new();
    v.extend(key(t.ok, Some(1.0), "ok"));
    v.extend(key(t.warn, Some(0.66), "65%+"));
    v.extend(key(t.err, Some(0.33), "<50%"));
    v.extend(key(t.meter_bg, None, "idle"));
    if tokens {
        v.push(Span::styled("tokens ", dim));
        for k in 0..5 {
            let lv = k as f64 / 4.0;
            let (ch, col) = heat_cell(Some(lv), t.gradient(0.15 + lv as f32 * 0.85), t);
            v.push(Span::styled(ch.to_string(), Style::default().fg(col)));
        }
        v.push(Span::styled(" low→high ", dim));
    }
    let cell = fmt::secs(a.bucket_seconds);
    let row = fmt::secs(a.bucket_seconds * a.columns.max(1) as f64);
    v.push(Span::styled(format!("cell {cell} · row {row}"), dim));
    let v = super::fit_spans(v, area.width as usize);
    f.buffer_mut()
        .set_line(area.x, area.y, &Line::from(v), area.width);
}

/// Row label: the start time of the first block of each grid row.
fn row_label(a: &Activity, r: usize, cols: usize) -> String {
    let Some(b) = a.blocks.get(r * cols) else {
        return String::new();
    };
    match a.grain.as_str() {
        "short" => fmt::ts_fmt(&b.start_time, "%H:%M"),
        "medium" => fmt::ts_fmt(&b.start_time, "%a %H"),
        _ => fmt::ts_fmt(&b.start_time, "%m-%d"),
    }
}

#[allow(clippy::too_many_arguments)]
fn heat(
    f: &mut Frame,
    app: &App,
    area: Rect,
    a: &Activity,
    rows: usize,
    cols: usize,
    rh: u16,
    titled: bool,
    future: &[bool],
    tokens: bool,
) {
    let t = app.theme();
    if area.width < ROW_LABEL + 4 || area.height == 0 {
        return;
    }
    let dim = Style::default().fg(t.graph_text);
    let title = if tokens { "tokens" } else { "health" };
    let area = if titled {
        let what = if tokens {
            "tokens per cell"
        } else {
            "success rate per cell"
        };
        f.buffer_mut().set_line(
            area.x + 1,
            area.y,
            &Line::from(vec![
                Span::styled(
                    title,
                    Style::default().fg(t.title).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" · {what} · rows start at"), dim),
            ]),
            area.width.saturating_sub(1),
        );
        Rect::new(
            area.x,
            area.y + 1,
            area.width,
            area.height.saturating_sub(1),
        )
    } else {
        area
    };
    let avail = (area.width - ROW_LABEL - 1) as usize;
    let (cell_w, group) = if avail >= cols * 2 {
        (2usize, 1usize)
    } else if avail >= cols {
        (1, 1)
    } else {
        (1, cols.div_ceil(avail.max(1)))
    };
    // Log-scaled token levels between P5 and P95 of non-zero blocks.
    let mut nz: Vec<f64> = a
        .blocks
        .iter()
        .map(|b| b.total_tokens)
        .filter(|v| *v > 0.0)
        .collect();
    nz.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let pick = |q: f64| -> f64 {
        if nz.is_empty() {
            1.0
        } else {
            nz[((nz.len() - 1) as f64 * q).round() as usize]
        }
    };
    let (lo, hi) = (pick(0.05).max(1.0).ln(), pick(0.95).max(1.0).ln());
    let buf = f.buffer_mut();
    for r in 0..rows {
        let y0 = area.y + r as u16 * rh;
        if y0 >= area.bottom() {
            break;
        }
        // Row label: the row's start time (the grid name on the first row
        // when there is no title line).
        let name_row = !titled && r == 0 && rh == 1;
        if !titled && r == 0 && rh == 2 {
            buf.set_string(
                area.x + 1,
                y0 + 1,
                title,
                Style::default().fg(t.title).add_modifier(Modifier::BOLD),
            );
        }
        let lbl = if name_row {
            title.to_string()
        } else {
            row_label(a, r, cols)
        };
        buf.set_string(
            area.x + 1,
            y0,
            fmt::trunc(&lbl, (ROW_LABEL - 1) as usize),
            if name_row {
                Style::default().fg(t.title)
            } else {
                dim
            },
        );
        let mut x = area.x + ROW_LABEL;
        let mut c = 0;
        while c < cols {
            let (mut s, mut fl, mut tk, mut all_future) = (0.0, 0.0, 0.0, true);
            for k in c..(c + group).min(cols) {
                let i = r * cols + k;
                if let Some(b) = a.blocks.get(i) {
                    s += b.success;
                    fl += b.failure;
                    tk += b.total_tokens;
                    all_future &= future.get(i).copied().unwrap_or(false);
                }
            }
            c += group;
            if x >= area.right() {
                break;
            }
            let (ch, color) = if all_future {
                (' ', t.bg)
            } else if tokens {
                if tk <= 0.0 {
                    heat_cell(None, t.meter_bg, t)
                } else {
                    let lv = if hi > lo {
                        ((tk.ln() - lo) / (hi - lo)).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    heat_cell(Some(lv), t.gradient(0.15 + lv as f32 * 0.85), t)
                }
            } else {
                match widgets::health_color(t, s, fl) {
                    Some(col) => {
                        let rate = s / (s + fl);
                        if t.no_color {
                            let ch = if rate >= 0.95 {
                                '■'
                            } else if rate >= 0.5 {
                                '▲'
                            } else {
                                '✗'
                            };
                            (ch, col)
                        } else {
                            ('■', col)
                        }
                    }
                    None => heat_cell(None, t.meter_bg, t),
                }
            };
            for dy in 0..rh {
                let yy = y0 + dy;
                if yy < area.bottom() {
                    buf[(x, yy)].set_char(ch).set_fg(color);
                }
            }
            x += cell_w as u16;
        }
    }
}

// ------------------------------------------------------------ distribution

fn dist_items(app: &App) -> &[ComparisonItem] {
    let Some(c) = app.data.comparisons.get() else {
        return &[];
    };
    match app.dim(PaneId::OvDist) {
        0 => &c.models,
        1 => &c.api_keys,
        2 => &c.credentials,
        _ => &c.ai_providers,
    }
}

pub fn dist_len(app: &App) -> Option<usize> {
    app.data.comparisons.get().map(|_| dist_items(app).len())
}

const DIST_COLS: [Col; 8] = [
    flex("name", 12),
    col("tokens", 7, Align::Right, 0),
    grow("share", 12, 1),
    col("req", 7, Align::Right, 0),
    col("fail", 5, Align::Right, 4),
    col("cost", 9, Align::Right, 2),
    col("cache", 6, Align::Right, 5),
    col("trend", 12, Align::Left, 3),
];

pub fn distribution(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let mut right = stale_marker(app, &app.data.comparisons);
    right.extend(dim_tabs(app, PaneId::OvDist, area.width));
    let inner = frame(f, app, PaneId::OvDist, area, focused, right, None);
    if placeholder(f, app, inner, &app.data.comparisons) {
        return;
    }
    let items = dist_items(app);
    let total: f64 = items.iter().map(|i| i.total_tokens).sum();
    let is_key = app.dim(PaneId::OvDist) == 1;
    let rows: Vec<TRow> = items
        .iter()
        .map(|i| {
            let name = if is_key {
                app.key_name(&i.key, &i.label)
            } else {
                app.privacy(&i.label)
            };
            let share = fmt::ratio_pct(i.total_tokens, total).unwrap_or(0.0);
            let cache = fmt::ratio_pct(i.cache_read_tokens, i.input_tokens);
            TRow::new(i.key.clone())
                .text(name.clone(), text(&name))
                .styled(
                    fmt::compact(i.total_tokens),
                    Style::default().fg(t.title),
                    num(i.total_tokens),
                )
                .meter(
                    share / 100.0,
                    None,
                    format!(" {:>5}", fmt::pct(share, 1)),
                    num(share),
                )
                .text(fmt::count(i.requests), num(i.requests))
                .styled(
                    fmt::int(i.failures),
                    Style::default().fg(if i.failures > 0.0 {
                        t.err
                    } else {
                        t.graph_text
                    }),
                    num(i.failures),
                )
                .text(fmt::usd_opt(i.cost), num(i.cost.unwrap_or(-1.0)))
                .text(
                    cache.map(|c| fmt::pct(c, 1)).unwrap_or("-".into()),
                    num(cache.unwrap_or(-1.0)),
                )
                .spark(i.token_series.clone(), t.accent, num(i.total_tokens))
                .filter(format!("{name} {}", i.key))
        })
        .collect();
    table_in(
        f,
        app,
        hit,
        PaneId::OvDist,
        inner,
        focused,
        &DIST_COLS,
        rows,
        "no usage in this range",
    );
}
