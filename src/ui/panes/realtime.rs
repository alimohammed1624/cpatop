//! Realtime tab.

use super::{
    Series, chart, chart_head, cursor, cursor_time, dim_tabs, frame, nice_max, placeholder,
    stale_marker, table_in, time_labels,
};
use crate::api::models::{LatencyPoint, Realtime, TopItem};
use crate::app::{App, Hit};
use crate::fmt;
use crate::layout::PaneId;
use crate::ui::table::{Align, Col, TRow, col, flex, grow, num, text};
use crate::ui::widgets::{self, Graph, Scatter, flow, meter, stat};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

fn rt(app: &App) -> Option<&Realtime> {
    app.data.realtime.get()
}

fn window_badge(app: &App) -> Vec<Span<'static>> {
    let mut r = stale_marker(app, &app.data.realtime);
    if r.is_empty() {
        r.push(Span::styled(
            app.window.param().to_string(),
            Style::default().fg(app.theme().graph_text),
        ));
    }
    r
}

/// Latest, average and second-half-vs-first-half trend of a series.
fn trend_stats(v: &[f64]) -> (f64, f64, Option<f64>) {
    if v.is_empty() {
        return (0.0, 0.0, None);
    }
    let latest = *v.last().unwrap_or(&0.0);
    let avg = v.iter().sum::<f64>() / v.len() as f64;
    let h = v.len() / 2;
    let a = v[..h].iter().sum::<f64>() / h.max(1) as f64;
    let b = v[h..].iter().sum::<f64>() / (v.len() - h).max(1) as f64;
    let trend = (a > 0.0).then(|| (b - a) / a * 100.0);
    (latest, avg, trend)
}

fn trend_span(app: &App, tr: Option<f64>) -> Span<'static> {
    let t = app.theme();
    match tr {
        Some(x) => Span::styled(
            format!("{}{:.1}%", if x >= 0.0 { "↑" } else { "↓" }, x.abs()),
            Style::default().fg(if x >= 0.0 { t.warn } else { t.ok }),
        ),
        None => Span::styled("–", Style::default().fg(t.graph_text)),
    }
}

fn summary_items(app: &App, width: u16) -> Vec<Vec<Span<'static>>> {
    let t = app.theme();
    let Some(r) = rt(app) else { return vec![] };
    let s = r
        .insights
        .as_ref()
        .map(|i| i.summary.clone())
        .unwrap_or_default();
    let reach = fmt::ratio_pct(s.cached_requests, s.token_requests);
    let fail_rate = fmt::ratio_pct(s.failures, s.requests);
    let mw = if width >= 120 { 12 } else { 8 };
    vec![
        stat(t, "requests", fmt::int(s.requests), None),
        vec![
            Span::styled("failures ", Style::default().fg(t.graph_text)),
            Span::styled(
                format!(
                    "{} ({})",
                    fmt::int(s.failures),
                    fail_rate.map(|p| fmt::pct(p, 1)).unwrap_or("-".into())
                ),
                Style::default()
                    .fg(if s.failures > 0.0 { t.err } else { t.title })
                    .add_modifier(Modifier::BOLD),
            ),
        ],
        stat(t, "tokens", fmt::compact(s.total_tokens), None),
        stat(t, "in", fmt::compact(s.input_tokens), Some(t.fg)),
        stat(t, "out", fmt::compact(s.output_tokens), Some(t.fg)),
        {
            let mut v = vec![Span::styled(
                "cache reach ",
                Style::default().fg(t.graph_text),
            )];
            v.extend(meter(reach.unwrap_or(0.0) / 100.0, mw, t, None));
            v.push(Span::styled(
                format!(" {}", reach.map(|p| fmt::pct(p, 2)).unwrap_or("-".into())),
                Style::default().fg(t.title).add_modifier(Modifier::BOLD),
            ));
            v
        },
        stat(t, "cost", fmt::usd_opt(s.cost), Some(t.accent)),
        stat(t, "p95 ttft", fmt::ms(r.latency_scatter.p95_ttft_ms), None),
        stat(
            t,
            "p95 latency",
            fmt::ms(r.latency_scatter.p95_latency_ms),
            None,
        ),
    ]
}

/// Lines the window summary needs at an inner width.
pub fn summary_lines(app: &App, inner_w: u16) -> u16 {
    flow(summary_items(app, inner_w), inner_w, 3).len() as u16
}

pub fn summary(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    let inner = frame(
        f,
        app,
        PaneId::RtSummary,
        area,
        focused,
        window_badge(app),
        None,
    );
    if placeholder(f, app, inner, &app.data.realtime) {
        return;
    }
    let w = inner.width.saturating_sub(1);
    let lines = flow(summary_items(app, w), w, 3);
    f.render_widget(
        Paragraph::new(lines),
        Rect::new(inner.x + 1, inner.y, w, inner.height),
    );
}

/// Bucket timestamps of the realtime series.
fn buckets(r: &Realtime) -> Vec<String> {
    r.token_velocity.iter().map(|p| p.bucket.clone()).collect()
}

/// Header for a realtime chart: latest / average / trend, or the cursor value.
fn rt_head(
    app: &App,
    name: &str,
    v: &[f64],
    b: &[String],
    cur: Option<usize>,
    fmtf: fn(f64) -> String,
) -> Line<'static> {
    let t = app.theme();
    let label = Style::default().fg(t.graph_text);
    match cur {
        Some(i) if i < v.len() => chart_head(
            app,
            name,
            fmtf(v[i]),
            vec![Span::styled(
                format!(
                    "  ▸ {}",
                    b.get(i).map(|x| cursor_time(x, false)).unwrap_or_default()
                ),
                Style::default().fg(t.hi),
            )],
        ),
        _ => {
            let (latest, avg, tr) = trend_stats(v);
            chart_head(
                app,
                name,
                fmtf(latest),
                vec![
                    Span::styled(format!("  avg {}  ", fmtf(avg)), label),
                    trend_span(app, tr),
                ],
            )
        }
    }
}

pub fn throughput(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let inner = frame(
        f,
        app,
        PaneId::RtThroughput,
        area,
        focused,
        window_badge(app),
        None,
    );
    if placeholder(f, app, inner, &app.data.realtime) {
        return;
    }
    let Some(r) = rt(app) else { return };
    let tpm: Vec<f64> = r
        .token_velocity
        .iter()
        .map(|p| p.tokens_per_minute)
        .collect();
    let rpm: Vec<f64> = r
        .request_level
        .iter()
        .map(|p| p.requests_per_minute)
        .collect();
    let b = buckets(r);
    let labels = time_labels(&b, false, false);
    let cur = cursor(app, PaneId::RtThroughput, tpm.len());
    let x = inner.x + 1;
    let w = inner.width.saturating_sub(1);
    let split = inner.height >= 10;
    let th = if split {
        inner.height - inner.height / 2
    } else {
        inner.height
    };
    f.buffer_mut().set_line(
        x,
        inner.y,
        &rt_head(app, "tokens/min", &tpm, &b, cur, fmt::compact),
        w,
    );
    chart(
        f,
        app,
        hit,
        PaneId::RtThroughput,
        Rect::new(x, inner.y + 1, w, th.saturating_sub(1)),
        &labels,
        &Series {
            values: &tpm,
            fmt: fmt::compact,
            color: None,
            max: None,
            floor: 0.0,
        },
        app.settings.graph_symbols,
        true,
    );
    if split {
        let y = inner.y + th;
        f.buffer_mut().set_line(
            x,
            y,
            &rt_head(app, "requests/min", &rpm, &b, cur, fmt::per_minute),
            w,
        );
        chart(
            f,
            app,
            hit,
            PaneId::RtThroughput,
            Rect::new(x, y + 1, w, (inner.height - th).saturating_sub(1)),
            &labels,
            &Series {
                values: &rpm,
                fmt: fmt::per_minute,
                color: Some(t.accent),
                max: None,
                floor: 0.0,
            },
            app.settings.graph_symbols,
            false,
        );
    }
}

pub fn outcomes(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let inner = frame(
        f,
        app,
        PaneId::RtOutcomes,
        area,
        focused,
        window_badge(app),
        None,
    );
    if placeholder(f, app, inner, &app.data.realtime) {
        return;
    }
    let Some(r) = rt(app) else { return };
    let out = r
        .insights
        .as_ref()
        .map(|i| i.outcomes.clone())
        .unwrap_or_default();
    let req: Vec<f64> = out.iter().map(|o| o.requests).collect();
    let fail: Vec<f64> = out.iter().map(|o| o.failures).collect();
    let b: Vec<String> = out.iter().map(|o| o.bucket.clone()).collect();
    let cur = cursor(app, PaneId::RtOutcomes, req.len());
    let (tr, tf) = match cur {
        Some(i) => (req[i], fail[i]),
        None => (req.iter().sum(), fail.iter().sum()),
    };
    let mut head = vec![
        Span::styled("ok ", Style::default().fg(t.graph_text)),
        Span::styled(fmt::int(tr - tf), Style::default().fg(t.ok)),
        Span::styled("  fail ", Style::default().fg(t.graph_text)),
        Span::styled(
            fmt::int(tf),
            Style::default().fg(if tf > 0.0 { t.err } else { t.graph_text }),
        ),
        Span::styled(
            format!(
                "  rate {}",
                fmt::ratio_pct(tf, tr)
                    .map(|p| fmt::pct(p, 1))
                    .unwrap_or("-".into())
            ),
            Style::default().fg(t.graph_text),
        ),
    ];
    if let Some(i) = cur {
        head.push(Span::styled(
            format!("  ▸ {}", cursor_time(&b[i], false)),
            Style::default().fg(t.hi),
        ));
    }
    f.buffer_mut().set_line(
        inner.x + 1,
        inner.y,
        &Line::from(head),
        inner.width.saturating_sub(1),
    );
    let g = Rect::new(
        inner.x + 1,
        inner.y + 1,
        inner.width.saturating_sub(1),
        inner.height.saturating_sub(1),
    );
    let peak = req.iter().copied().fold(0.0, f64::max);
    if peak <= 0.0 {
        widgets::message(f.buffer_mut(), g, "no requests in window", t.graph_text);
        return;
    }
    // Outcome bars use one column per bucket, so block symbols read best.
    let symbols = if app.settings.graph_symbols == crate::config::GraphSymbols::Ascii {
        crate::config::GraphSymbols::Ascii
    } else {
        crate::config::GraphSymbols::Block
    };
    let top = nice_max(peak);
    let labels = time_labels(&b, false, false);
    let plot = chart(
        f,
        app,
        hit,
        PaneId::RtOutcomes,
        g,
        &labels,
        &Series {
            values: &req,
            fmt: fmt::int,
            color: Some(t.ok),
            max: Some(top),
            floor: 0.0,
        },
        symbols,
        true,
    );
    if fail.iter().any(|v| *v > 0.0) {
        f.render_widget(
            Graph {
                data: &fail,
                theme: t,
                symbols,
                max: Some(top),
                color: Some(t.err),
            },
            plot,
        );
        if let Some(i) = cur {
            widgets::draw_cursor(f.buffer_mut(), plot, i, req.len(), t);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn scatter_pane(
    f: &mut Frame,
    app: &App,
    id: PaneId,
    area: Rect,
    focused: bool,
    points: &[LatencyPoint],
    p95: (f64, f64),
    total: f64,
    ready: bool,
    right_extra: Vec<Span<'static>>,
) -> Option<Rect> {
    let t = app.theme();
    let pts: Vec<(f64, f64)> = points.iter().map(|p| (p.ttft_ms, p.latency_ms)).collect();
    let mut right = right_extra;
    if ready {
        right.push(Span::styled(
            format!(
                "p95 ttft {} · lat {} · n {}",
                fmt::ms(p95.0),
                fmt::ms(p95.1),
                fmt::compact(total)
            ),
            Style::default().fg(t.graph_text),
        ));
    }
    let bottom = if pts.is_empty() {
        None
    } else {
        let ((x0, x1), _) = Scatter::bounds(&pts);
        Some((
            format!("ttft {} →", fmt::ms(10f64.powf(x0))),
            fmt::ms(10f64.powf(x1)).to_string(),
        ))
    };
    let inner = frame(f, app, id, area, focused, right, bottom);
    if !ready {
        return Some(inner);
    }
    if pts.is_empty() {
        widgets::message(f.buffer_mut(), inner, "no latency samples", t.graph_text);
        return None;
    }
    let (_, (y0, y1)) = Scatter::bounds(&pts);
    let label = Style::default().fg(t.graph_text);
    f.buffer_mut().set_string(
        inner.x,
        inner.y,
        format!("↑{}", fmt::ms(10f64.powf(y1))),
        label,
    );
    f.render_widget(
        Scatter {
            points: &pts,
            theme: t,
            x_ref: Some(p95.0),
            y_ref: Some(p95.1),
        },
        Rect::new(
            inner.x,
            inner.y + 1,
            inner.width,
            inner.height.saturating_sub(2),
        ),
    );
    if inner.height >= 3 {
        f.buffer_mut().set_string(
            inner.x,
            inner.bottom() - 1,
            format!(" {}", fmt::ms(10f64.powf(y0))),
            label,
        );
    }
    None
}

pub fn latency(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    let r = rt(app);
    let empty = Default::default();
    let ls = r.map(|r| &r.latency_scatter).unwrap_or(&empty);
    let pending = scatter_pane(
        f,
        app,
        PaneId::RtLatency,
        area,
        focused,
        &ls.points,
        (ls.p95_ttft_ms, ls.p95_latency_ms),
        ls.total_points,
        r.is_some(),
        vec![],
    );
    if let Some(inner) = pending {
        placeholder(f, app, inner, &app.data.realtime);
    }
}

const SHARE_COLS: [Col; 5] = [
    flex("name", 12),
    grow("share", 14, 1),
    col("tokens", 7, Align::Right, 0),
    col("req", 6, Align::Right, 0),
    col("cost", 9, Align::Right, 2),
];

fn share_items(app: &App) -> Option<&[TopItem]> {
    let r = rt(app)?;
    Some(match app.dim(PaneId::RtShare) {
        0 => &r.current_usage.models,
        1 => &r.current_usage.api_keys,
        2 => &r.current_usage.credentials,
        _ => &r.current_usage.ai_providers,
    })
}

pub fn share_len(app: &App) -> Option<usize> {
    share_items(app).map(|i| i.len())
}

pub fn speed_len(app: &App) -> Option<usize> {
    rt(app).map(|r| r.model_speed.series.len())
}

pub fn share(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let right = dim_tabs(app, PaneId::RtShare, area.width);
    let inner = frame(f, app, PaneId::RtShare, area, focused, right, None);
    if placeholder(f, app, inner, &app.data.realtime) {
        return;
    }
    let Some(items) = share_items(app) else {
        return;
    };
    let dim = app.dim(PaneId::RtShare);
    let rows = items
        .iter()
        .map(|i| {
            let name = if i.key == "__realtime_others__" {
                "Others".to_string()
            } else if dim == 1 {
                app.key_name(&i.key, &i.label)
            } else if dim == 2 {
                app.credential_name(&i.key, &i.label)
            } else {
                app.privacy(&i.label)
            };
            TRow::new(i.key.clone())
                .text(name.clone(), text(&name))
                .meter(
                    i.share / 100.0,
                    None,
                    format!(" {:>5.1}%", i.share),
                    num(i.share),
                )
                .styled(
                    fmt::compact(i.tokens),
                    Style::default().fg(t.title),
                    num(i.tokens),
                )
                .text(fmt::count(i.requests), num(i.requests))
                .text(fmt::usd_opt(i.cost), num(i.cost.unwrap_or(-1.0)))
                .filter(name)
        })
        .collect();
    table_in(
        f,
        app,
        hit,
        PaneId::RtShare,
        inner,
        focused,
        &SHARE_COLS,
        rows,
        "no traffic in window",
    );
}

const SPEED_COLS: [Col; 6] = [
    flex("model", 12),
    col("1st half", 10, Align::Right, 0),
    col("2nd half", 10, Align::Right, 0),
    col("Δ", 7, Align::Right, 1),
    col("samples", 7, Align::Right, 2),
    col("tokens", 7, Align::Right, 3),
];

pub fn speed(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let inner = frame(f, app, PaneId::RtSpeed, area, focused, vec![], None);
    if placeholder(f, app, inner, &app.data.realtime) {
        return;
    }
    let Some(r) = rt(app) else { return };
    if !r.model_speed.available {
        widgets::message(
            f.buffer_mut(),
            inner,
            "model speed unavailable",
            t.graph_text,
        );
        return;
    }
    let rows = r
        .model_speed
        .series
        .iter()
        .map(|s| {
            let a = s.first_half.mean_tps;
            let b = s.second_half.mean_tps;
            let delta = match (a, b) {
                (Some(a), Some(b)) if a > 0.0 => Some((b - a) / a * 100.0),
                _ => None,
            };
            TRow::new(s.model.clone())
                .text(s.model.clone(), text(&s.model))
                .text(
                    a.map(fmt::tps).unwrap_or("-".into()),
                    num(a.unwrap_or(-1.0)),
                )
                .styled(
                    b.map(fmt::tps).unwrap_or("-".into()),
                    Style::default().fg(t.title),
                    num(b.unwrap_or(-1.0)),
                )
                .styled(
                    delta.map(|d| format!("{d:+.1}%")).unwrap_or("-".into()),
                    Style::default().fg(match delta {
                        Some(d) if d < -5.0 => t.err,
                        Some(d) if d > 5.0 => t.ok,
                        _ => t.graph_text,
                    }),
                    num(delta.unwrap_or(0.0)),
                )
                .text(fmt::int(s.speed_sample_count), num(s.speed_sample_count))
                .text(fmt::compact(s.total_tokens), num(s.total_tokens))
                .filter(s.model.clone())
        })
        .collect();
    table_in(
        f,
        app,
        hit,
        PaneId::RtSpeed,
        inner,
        focused,
        &SPEED_COLS,
        rows,
        "no samples in window",
    );
}

pub fn cache(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let inner = frame(
        f,
        app,
        PaneId::RtCache,
        area,
        focused,
        window_badge(app),
        None,
    );
    if placeholder(f, app, inner, &app.data.realtime) {
        return;
    }
    let Some(r) = rt(app) else { return };
    // Buckets without input have no rate: a gap, not a drop to zero.
    let rates: Vec<f64> = r
        .cache_level
        .iter()
        .map(|c| c.cache_read_rate.unwrap_or(f64::NAN))
        .collect();
    let b: Vec<String> = r.cache_level.iter().map(|c| c.bucket.clone()).collect();
    let known: Vec<f64> = rates.iter().copied().filter(|v| v.is_finite()).collect();
    let cur = cursor(app, PaneId::RtCache, rates.len());
    // Plot above a floor so small changes near 100% stay visible.
    let lo = known.iter().copied().fold(100.0, f64::min);
    let floor = ((lo - 5.0) / 10.0).floor().clamp(0.0, 9.0) * 10.0;
    let head = match cur {
        Some(i) => chart_head(
            app,
            "cache read",
            if rates[i].is_finite() {
                fmt::pct(rates[i], 2)
            } else {
                "no input".into()
            },
            vec![Span::styled(
                format!("  ▸ {}", cursor_time(&b[i], false)),
                Style::default().fg(t.hi),
            )],
        ),
        None => {
            let (latest, avg, tr) = trend_stats(&known);
            chart_head(
                app,
                "cache read",
                fmt::pct(latest, 2),
                vec![
                    Span::styled(
                        format!("  avg {}  ", fmt::pct(avg, 1)),
                        Style::default().fg(t.graph_text),
                    ),
                    trend_span(app, tr),
                ],
            )
        }
    };
    f.buffer_mut()
        .set_line(inner.x + 1, inner.y, &head, inner.width.saturating_sub(1));
    let labels = time_labels(&b, false, false);
    chart(
        f,
        app,
        hit,
        PaneId::RtCache,
        Rect::new(
            inner.x + 1,
            inner.y + 1,
            inner.width.saturating_sub(1),
            inner.height.saturating_sub(1),
        ),
        &labels,
        &Series {
            values: &rates,
            fmt: |v| format!("{v:.0}%"),
            color: None,
            max: Some(100.0),
            floor,
        },
        app.settings.graph_symbols,
        true,
    );
}
