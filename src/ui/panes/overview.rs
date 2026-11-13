//! Overview tab (admin and API-key viewer).

use super::{
    RowChart, Series, chart_head, chart_until, cursor, cursor_time, dim_key_field, dim_tabs, frame,
    placeholder, request_details, stale_marker, table_with_chart, time_labels, token_details,
};
use crate::api::models::ComparisonItem;
use crate::app::{App, Hit};
use crate::fmt;
use crate::layout::PaneId;
use crate::ui::table::{Align, Col, TRow, col, flex, grow, num, spark, text};
use crate::ui::widgets::{self, flow, meter, stat};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
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
            v.extend(meter(
                success.map(|_| pct / 100.0).unwrap_or(f64::NAN),
                mw,
                t,
                Some(t.good_pct(pct, 95.0, 80.0)),
            ));
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
            // Cache hits are good: colour the rate like one.
            v.extend(meter(
                cache.map(|c| c / 100.0).unwrap_or(f64::NAN),
                mw,
                t,
                cache.map(|c| t.good_pct(c, 80.0, 50.0)),
            ));
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

/// Bucket timestamps, tokens, requests, whether buckets are daily, and how
/// many buckets have started (the rest are `NaN`: the remainder of `today`).
type Trend = (Vec<String>, Vec<f64>, Vec<f64>, bool, usize);

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
        // The `today` axis runs to midnight: buckets after the current one
        // keep their slots (so the day reads 00:00 → 24:00) but are blank.
        let keep = crate::series::until_now(&c.buckets, app.server_now())
            .max(1)
            .min(n);
        for v in tokens[keep..].iter_mut().chain(&mut reqs[keep..]) {
            *v = f64::NAN;
        }
        return Some((
            c.buckets.clone(),
            tokens,
            reqs,
            c.granularity == "daily",
            keep,
        ));
    }
    ov.map(|o| {
        (
            o.series.buckets.clone(),
            o.series.tokens.clone(),
            o.series.requests.clone(),
            app.range.is_daily(),
            o.series.buckets.len(),
        )
    })
}

/// Sum of the started buckets.
fn total(v: &[f64]) -> f64 {
    v.iter().filter(|v| v.is_finite()).sum()
}

pub fn trend(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let series = trend_series(app);
    let right = match &series {
        Some((b, tok, _, daily, _)) if !b.is_empty() => {
            let peak = tok.iter().copied().fold(0.0, f64::max);
            let unit = if *daily { "/day" } else { "/h" };
            vec![Span::styled(
                format!(
                    "{} · peak {}{unit} · Σ {}",
                    app.range.label(),
                    fmt::compact(peak),
                    fmt::compact(total(tok))
                ),
                Style::default().fg(t.graph_text),
            )]
        }
        _ => vec![],
    };
    let inner = frame(f, app, PaneId::OvTrend, area, focused, right, None);
    let Some((buckets, tokens, reqs, daily, keep)) = series else {
        if !placeholder(f, app, inner, &app.data.comparisons) {
            widgets::message(f.buffer_mut(), inner, "no data", t.graph_text);
        }
        return;
    };
    if total(&tokens) == 0.0 && total(&reqs) == 0.0 {
        widgets::message(
            f.buffer_mut(),
            inner,
            "no usage in this range",
            t.graph_text,
        );
        return;
    }
    let labels = time_labels(&buckets, daily, false);
    let place = if daily {
        None
    } else {
        app.hourly_placement(&buckets)
    };
    let n = buckets.len();
    let future = (keep < n).then_some(keep);
    let cur = cursor(app, PaneId::OvTrend, n);
    let label = Style::default().fg(t.graph_text);
    let at = |i: usize| {
        vec![Span::styled(
            format!("  ▸ {}", cursor_time(&buckets[i], daily)),
            Style::default().fg(t.hi),
        )]
    };
    let head = |name: &str, v: &[f64], f2: fn(f64) -> String| match cur {
        Some(i) => chart_head(app, name, f2(v[i]), at(i)),
        None => chart_head(
            app,
            name,
            f2(total(v)),
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
        chart_until(
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
                place: place.as_ref(),
            },
            app.settings.graph_symbols,
            true,
            future,
        );
        let ry = inner.y + th;
        let rh = inner.height - th;
        f.buffer_mut()
            .set_line(x, ry, &head("requests", &reqs, fmt::int), w);
        chart_until(
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
                place: place.as_ref(),
            },
            app.settings.graph_symbols,
            false,
            future,
        );
    } else {
        f.buffer_mut()
            .set_line(x, inner.y, &head("tokens", &tokens, fmt::compact), w);
        chart_until(
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
                place: place.as_ref(),
            },
            app.settings.graph_symbols,
            true,
            future,
        );
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
    spark("trend", 12, 3),
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
    let keep = app
        .data
        .comparisons
        .get()
        .map(|c| crate::series::until_now(&c.buckets, app.server_now()))
        .unwrap_or(usize::MAX);
    // Buckets that have not started (the rest of `today`) stay on the axis,
    // blank, so a sparkline or row chart reads as a day filling in.
    let started = |v: &[f64]| -> Vec<f64> {
        v.iter()
            .enumerate()
            .map(|(i, x)| if i < keep { *x } else { f64::NAN })
            .collect()
    };
    // Sparklines share the trend's time axis: hourly buckets at their real
    // times, the rest of `today` a dotted baseline.
    let axis = app.data.comparisons.get().and_then(|c| {
        let place = (c.granularity != "daily")
            .then(|| app.hourly_placement(&c.buckets))
            .flatten();
        (keep < c.buckets.len() || place.is_some())
            .then(|| std::sync::Arc::new(widgets::SparkAxis::new(keep, place.as_ref())))
    });
    let dim = app.dim(PaneId::OvDist);
    let dim_name = crate::app::dims_for(PaneId::OvDist, app.role)
        .get(dim)
        .copied()
        .unwrap_or("");
    let rows: Vec<TRow> = items
        .iter()
        .map(|i| {
            let (name, short) = match dim {
                1 => (app.key_name(&i.key, &i.label), String::new()),
                2 => app.credential_labels(&i.key, &i.label),
                _ => (app.privacy(&i.label), String::new()),
            };
            let short = if short.is_empty() {
                name.clone()
            } else {
                short
            };
            let share = fmt::ratio_pct(i.total_tokens, total).unwrap_or(0.0);
            let cache = fmt::ratio_pct(i.cache_read_tokens, i.input_tokens);
            TRow::new(i.key.clone())
                .name(name.clone(), short, Style::default(), text(&name))
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
                .spark_on(
                    started(&i.token_series),
                    t.accent,
                    axis.clone(),
                    num(i.total_tokens),
                )
                .filter(format!("{name} {}", i.key))
                .details(dim_key_field(app, dim_name, &i.key))
                .details(request_details(i.requests, i.failures))
                .details(token_details(
                    i.input_tokens,
                    i.output_tokens,
                    i.cache_read_tokens,
                    i.cache_creation_tokens,
                    i.reasoning_tokens,
                    i.total_tokens,
                ))
                .detail("share", format!("{} of tokens", fmt::pct(share, 2)))
                .detail(
                    "cost",
                    i.cost.map(|c| format!("${c:.4}")).unwrap_or_default(),
                )
        })
        .collect();
    // Spare rows chart the selected row's tokens over the range.
    let chart_for = |id: &str| -> Option<RowChart> {
        let c = app.data.comparisons.get()?;
        let i = items.iter().find(|i| i.key == id)?;
        let n = c.buckets.len().min(i.token_series.len());
        let name = match dim {
            1 => app.key_name(&i.key, &i.label),
            2 => app.credential_name(&i.key, &i.label),
            _ => app.privacy(&i.label),
        };
        Some(RowChart {
            title: format!("{name} · tokens"),
            labels: time_labels(&c.buckets[..n], c.granularity == "daily", false),
            values: started(&i.token_series[..n]),
            fmt: fmt::compact,
            color: None,
            place: (c.granularity != "daily")
                .then(|| app.hourly_placement(&c.buckets[..n]))
                .flatten(),
        })
    };
    table_with_chart(
        f,
        app,
        hit,
        PaneId::OvDist,
        inner,
        focused,
        &DIST_COLS,
        rows,
        &super::empty_dim_text(app, PaneId::OvDist, "no usage in this range"),
        &chart_for,
    );
}
