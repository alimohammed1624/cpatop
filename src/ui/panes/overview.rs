//! Overview tab (admin and API-key viewer).

use super::{dim_tabs, frame, key_label, placeholder, stale_marker, table_in};
use crate::api::models::ComparisonItem;
use crate::app::{App, Hit};
use crate::fmt;
use crate::layout::PaneId;
use crate::ui::table::{Align, Col, TRow, col, flex, num, text};
use crate::ui::widgets::{self, Graph, flow, meter, spark, stat};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::collections::HashMap;

pub fn totals(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    let t = app.theme();
    let l = &app.data.overview;
    let right = {
        let mut r = stale_marker(app, l);
        if r.is_empty() {
            r.push(Span::styled(
                app.range.to_string(),
                Style::default().fg(t.graph_text),
            ));
        }
        r
    };
    let inner = frame(f, app, PaneId::OvTotals, area, focused, right, None);
    if placeholder(f, app, inner, l) {
        return;
    }
    let Some(o) = l.get() else { return };
    let u = &o.usage;
    let s = &o.summary;
    let success = fmt::ratio_pct(u.success_count, u.total_requests);
    let cache = fmt::ratio_pct(s.cache_read_tokens, s.input_tokens);
    let mw = if inner.width >= 120 { 12 } else { 8 };
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
    let lines = flow(items, inner.width.saturating_sub(1), 3);
    f.render_widget(
        Paragraph::new(lines),
        Rect::new(
            inner.x + 1,
            inner.y,
            inner.width.saturating_sub(1),
            inner.height,
        ),
    );
}

/// Bucket labels, tokens, requests and whether buckets are daily.
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

pub fn trend(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    let t = app.theme();
    let series = trend_series(app);
    let (right, bottom) = match &series {
        Some((b, tok, _, daily)) if !b.is_empty() => {
            let peak = tok.iter().copied().fold(0.0, f64::max);
            let unit = if *daily { "/day" } else { "/h" };
            (
                vec![Span::styled(
                    format!(
                        "peak {}{unit} · Σ {}",
                        fmt::compact(peak),
                        fmt::compact(tok.iter().sum())
                    ),
                    Style::default().fg(t.graph_text),
                )],
                Some(if *daily {
                    (
                        fmt::bucket_label(&b[0], true),
                        fmt::bucket_label(&b[b.len() - 1], true),
                    )
                } else {
                    (
                        fmt::ts_fmt(&b[0], "%m-%d %H:%M"),
                        fmt::ts_fmt(&b[b.len() - 1], "%m-%d %H:%M"),
                    )
                }),
            )
        }
        _ => (vec![], None),
    };
    let inner = frame(f, app, PaneId::OvTrend, area, focused, right, bottom);
    let Some((_, tokens, reqs, _)) = series else {
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
    let label = Style::default().fg(t.graph_text);
    if inner.height >= 9 {
        let th = inner.height * 3 / 5;
        f.buffer_mut()
            .set_string(inner.x + 1, inner.y, "tokens", label);
        f.render_widget(
            Graph {
                data: &tokens,
                theme: t,
                symbols: app.settings.graph_symbols,
                max: None,
                color: None,
            },
            Rect::new(inner.x, inner.y + 1, inner.width, th - 1),
        );
        let ry = inner.y + th;
        let rh = inner.height - th;
        let peak = reqs.iter().copied().fold(0.0, f64::max);
        f.buffer_mut().set_line(
            inner.x + 1,
            ry,
            &Line::from(vec![
                Span::styled("requests ", label),
                Span::styled(
                    format!(
                        "peak {} · Σ {}",
                        fmt::count(peak),
                        fmt::int(reqs.iter().sum())
                    ),
                    label,
                ),
            ]),
            inner.width.saturating_sub(1),
        );
        f.render_widget(
            Graph {
                data: &reqs,
                theme: t,
                symbols: app.settings.graph_symbols,
                max: None,
                color: Some(t.accent),
            },
            Rect::new(inner.x, ry + 1, inner.width, rh.saturating_sub(1)),
        );
    } else {
        f.render_widget(
            Graph {
                data: &tokens,
                theme: t,
                symbols: app.settings.graph_symbols,
                max: None,
                color: None,
            },
            inner,
        );
        f.buffer_mut()
            .set_string(inner.x + 1, inner.y, "tokens", label);
    }
}

pub fn activity(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    let t = app.theme();
    let l = &app.data.activity;
    let mut right = stale_marker(app, l);
    let mut bottom = None;
    if let Some(a) = l.get() {
        if right.is_empty() {
            right.push(Span::styled(
                format!("{} · {}", a.window, a.grain),
                Style::default().fg(t.graph_text),
            ));
        }
        let pat = if a.grain == "daily" {
            "%m-%d"
        } else {
            "%m-%d %H:%M"
        };
        bottom = Some((
            fmt::ts_fmt(&a.window_start, pat),
            fmt::ts_fmt(&a.window_end, pat),
        ));
    }
    let inner = frame(f, app, PaneId::OvActivity, area, focused, right, bottom);
    if placeholder(f, app, inner, l) {
        return;
    }
    let Some(a) = l.get() else { return };
    let rate = if a.total_success + a.total_failure > 0.0 {
        fmt::pct(a.success_rate, 1)
    } else {
        "--".into()
    };
    let head = Line::from(vec![
        Span::styled(" health ", Style::default().fg(t.graph_text)),
        Span::styled(
            rate,
            Style::default()
                .fg(t.good_pct(a.success_rate, 95.0, 80.0))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  ✓{}", fmt::int(a.total_success)),
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
        Span::styled("  tokens ", Style::default().fg(t.graph_text)),
        Span::styled(fmt::compact(a.total_tokens), Style::default().fg(t.title)),
    ]);
    f.buffer_mut()
        .set_line(inner.x, inner.y, &head, inner.width);
    if inner.height < 3 {
        return;
    }
    let rows = a.rows.max(1) as usize;
    let cols = a.columns.max(1) as usize;
    let now = app.clock.utc();
    let future: Vec<bool> = a
        .blocks
        .iter()
        .map(|b| fmt::parse_ts(&b.start_time).is_some_and(|s| s.with_timezone(&chrono::Utc) > now))
        .collect();
    let avail_h = inner.height - 1;
    let show_tokens = avail_h as usize > rows * 2;
    let health_area = Rect::new(
        inner.x,
        inner.y + 1,
        inner.width,
        (rows as u16).min(avail_h),
    );
    heat(f, app, health_area, a, rows, cols, &future, false);
    if show_tokens {
        let ty = health_area.bottom();
        f.buffer_mut()
            .set_string(inner.x, ty, " tokens", Style::default().fg(t.graph_text));
        let tok_area = Rect::new(inner.x, ty + 1, inner.width, rows as u16);
        heat(f, app, tok_area, a, rows, cols, &future, true);
    }
}

#[allow(clippy::too_many_arguments)]
fn heat(
    f: &mut Frame,
    app: &App,
    area: Rect,
    a: &crate::api::models::Activity,
    rows: usize,
    cols: usize,
    future: &[bool],
    tokens: bool,
) {
    let t = app.theme();
    if area.width < 2 {
        return;
    }
    let avail = area.width as usize - 1;
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
    for r in 0..rows.min(area.height as usize) {
        let mut x = area.x + 1;
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
                    ('■', t.meter_bg)
                } else {
                    let lv = if hi > lo {
                        ((tk.ln() - lo) / (hi - lo)).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    ('■', t.gradient(0.15 + lv as f32 * 0.85))
                }
            } else {
                match widgets::health_color(t, s, fl) {
                    Some(c) => ('■', c),
                    None => ('■', t.meter_bg),
                }
            };
            buf[(x, area.y + r as u16)].set_char(ch).set_fg(color);
            x += cell_w as u16;
        }
    }
}

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

const DIST_COLS: [Col; 8] = [
    flex("name", 12),
    col("tokens", 7, Align::Right, 0),
    col("share", 6, Align::Right, 1),
    col("req", 7, Align::Right, 0),
    col("fail", 5, Align::Right, 4),
    col("cost", 9, Align::Right, 2),
    col("cache", 6, Align::Right, 5),
    col("trend", 14, Align::Left, 3),
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
                key_label(&i.label)
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
                .text(fmt::pct(share, 1), num(share))
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
                .spans(spark(&i.token_series, 14, t), num(i.total_tokens))
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
