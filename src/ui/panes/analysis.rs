//! Analysis tab.

use super::realtime::scatter_pane;
use super::{dim_tabs, frame, key_label, placeholder, stale_marker, table_in};
use crate::api::models::{Analysis, CompositionItem};
use crate::app::{App, Hit};
use crate::fmt;
use crate::layout::PaneId;
use crate::ui::table::{Align, Col, TRow, col, flex, num, text};
use crate::ui::widgets::{self, Graph, flow, meter, spark, stat};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use std::borrow::Cow;

fn an(app: &App) -> Option<&Analysis> {
    app.data.analysis.get()
}

fn daily(a: &Analysis) -> bool {
    a.granularity == "daily"
}

/// Window length in minutes for RPM/TPM.
fn window_minutes(a: &Analysis) -> Option<f64> {
    let s = fmt::parse_ts(a.range_start.as_deref()?)?;
    let e = fmt::parse_ts(a.range_end.as_deref()?)?;
    let m = (e - s).num_seconds() as f64 / 60.0;
    (m > 0.0).then_some(m)
}

pub fn tokens(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    let t = app.theme();
    let l = &app.data.analysis;
    let mut right = stale_marker(app, l);
    let mut bottom = None;
    if let Some(a) = an(app) {
        if right.is_empty() {
            right.push(Span::styled(
                format!("{} · {}", app.range, a.granularity),
                Style::default().fg(t.graph_text),
            ));
        }
        if let (Some(first), Some(last)) = (a.token_usage.first(), a.token_usage.last()) {
            let pat = if daily(a) { "%m-%d" } else { "%m-%d %H:%M" };
            bottom = Some((
                fmt::ts_fmt(&first.bucket, pat),
                fmt::ts_fmt(&last.bucket, pat),
            ));
        }
    }
    let inner = frame(f, app, PaneId::AnTokens, area, focused, right, bottom);
    if placeholder(f, app, inner, l) {
        return;
    }
    let Some(a) = an(app) else { return };
    let total: f64 = a.token_usage.iter().map(|b| b.total_tokens).sum();
    let reqs: f64 = a.token_usage.iter().map(|b| b.requests).sum();
    let cost = a.cost_summary.total_cost_usd;
    let blended = if total > 0.0 { cost / total * 1e6 } else { 0.0 };
    let avg = if a.token_usage.is_empty() {
        0.0
    } else {
        total / a.token_usage.len() as f64
    };
    let mut cost_item = stat(t, "cost", fmt::usd(cost), Some(t.accent));
    if !a.cost_summary.cost_available {
        cost_item.push(Span::styled(" (partial)", Style::default().fg(t.warn)));
    }
    let items = vec![
        stat(t, "tokens", fmt::compact(total), None),
        stat(t, "requests", fmt::int(reqs), None),
        cost_item,
        stat(t, "$/1M", fmt::usd(blended), None),
        stat(t, "avg/bucket", fmt::compact(avg), None),
    ];
    let lines = flow(items, inner.width.saturating_sub(1), 3);
    let lh = (lines.len() as u16).min(inner.height);
    f.render_widget(
        Paragraph::new(lines),
        Rect::new(inner.x + 1, inner.y, inner.width.saturating_sub(1), lh),
    );
    let series: Vec<f64> = a.token_usage.iter().map(|b| b.total_tokens).collect();
    let g = Rect::new(
        inner.x,
        inner.y + lh,
        inner.width,
        inner.height.saturating_sub(lh),
    );
    if series.iter().all(|v| *v == 0.0) {
        widgets::message(f.buffer_mut(), g, "no usage in this range", t.graph_text);
        return;
    }
    f.render_widget(
        Graph {
            data: &series,
            theme: t,
            symbols: app.settings.graph_symbols,
            max: None,
            color: None,
        },
        g,
    );
}

fn comp_items(app: &App) -> &[CompositionItem] {
    let Some(a) = an(app) else { return &[] };
    match crate::app::dims_for(PaneId::AnComposition, app.role)
        .get(app.dim(PaneId::AnComposition))
        .copied()
    {
        Some("api keys") => &a.api_key_composition,
        Some("credentials") => &a.credentials_composition,
        Some("providers") => &a.ai_provider_composition,
        _ => &a.model_composition,
    }
}

const COMP_COLS: [Col; 9] = [
    col("#", 3, Align::Right, 6),
    flex("name", 12),
    col("tokens", 7, Align::Right, 0),
    col("share", 16, Align::Left, 1),
    col("req", 6, Align::Right, 0),
    col("cost", 9, Align::Right, 2),
    col("cache", 6, Align::Right, 4),
    col("rpm", 6, Align::Right, 5),
    col("tpm", 7, Align::Right, 5),
];

pub fn composition(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let right = dim_tabs(app, PaneId::AnComposition, area.width);
    let inner = frame(f, app, PaneId::AnComposition, area, focused, right, None);
    if placeholder(f, app, inner, &app.data.analysis) {
        return;
    }
    let mins = an(app).and_then(window_minutes);
    let is_key = crate::app::dims_for(PaneId::AnComposition, app.role)
        .get(app.dim(PaneId::AnComposition))
        == Some(&"api keys");
    let rows = comp_items(app)
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let name = if is_key {
                key_label(&c.label)
            } else {
                app.privacy(&c.label)
            };
            let cache = fmt::ratio_pct(c.cache_read_tokens, c.input_tokens);
            let mut bar = meter(c.percent / 100.0, 8, t, None);
            bar.push(Span::styled(
                format!(" {:>5.1}%", c.percent),
                Style::default().fg(t.fg),
            ));
            TRow::new(c.key.clone())
                .styled(
                    format!("{}", i + 1),
                    Style::default().fg(t.graph_text),
                    num(i as f64),
                )
                .text(name.clone(), text(&name))
                .styled(
                    fmt::compact(c.total_tokens),
                    Style::default().fg(t.title),
                    num(c.total_tokens),
                )
                .spans(bar, num(c.percent))
                .text(fmt::count(c.requests), num(c.requests))
                .styled(
                    if c.cost_available {
                        fmt::usd(c.cost_usd)
                    } else {
                        format!("{}*", fmt::usd(c.cost_usd))
                    },
                    Style::default().fg(if c.cost_available { t.fg } else { t.warn }),
                    num(c.cost_usd),
                )
                .text(
                    cache.map(|x| fmt::pct(x, 1)).unwrap_or("--".into()),
                    num(cache.unwrap_or(-1.0)),
                )
                .text(
                    mins.map(|m| fmt::per_minute(c.requests / m))
                        .unwrap_or("--".into()),
                    num(c.requests),
                )
                .text(
                    mins.map(|m| fmt::compact(c.total_tokens / m))
                        .unwrap_or("--".into()),
                    num(c.total_tokens),
                )
                .filter(name)
        })
        .collect();
    table_in(
        f,
        app,
        hit,
        PaneId::AnComposition,
        inner,
        focused,
        &COMP_COLS,
        rows,
        "no data",
    );
}

const MODEL_COLS: [Col; 6] = [
    flex("model", 12),
    col("tokens", 7, Align::Right, 0),
    col("share", 6, Align::Right, 1),
    col("req", 6, Align::Right, 2),
    col("cost", 9, Align::Right, 3),
    col("trend", 12, Align::Left, 1),
];

pub fn models(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let inner = frame(f, app, PaneId::AnModels, area, focused, vec![], None);
    if placeholder(f, app, inner, &app.data.analysis) {
        return;
    }
    let Some(a) = an(app) else { return };
    let total: f64 = a
        .model_usage
        .series
        .iter()
        .map(|s| s.total_tokens.iter().sum::<f64>())
        .sum();
    let rows = a
        .model_usage
        .series
        .iter()
        .map(|s| {
            let tok: f64 = s.total_tokens.iter().sum();
            let comp = a.model_composition.iter().find(|c| c.label == s.model);
            let req = comp
                .map(|c| c.requests)
                .unwrap_or_else(|| s.requests.iter().sum());
            let share = fmt::ratio_pct(tok, total).unwrap_or(0.0);
            TRow::new(s.model.clone())
                .text(s.model.clone(), text(&s.model))
                .styled(fmt::compact(tok), Style::default().fg(t.title), num(tok))
                .text(fmt::pct(share, 1), num(share))
                .text(fmt::count(req), num(req))
                .text(
                    comp.map(|c| fmt::usd(c.cost_usd)).unwrap_or("—".into()),
                    num(comp.map(|c| c.cost_usd).unwrap_or(-1.0)),
                )
                .spans(spark(&s.total_tokens, 12, t), num(tok))
                .filter(s.model.clone())
        })
        .collect();
    table_in(
        f,
        app,
        hit,
        PaneId::AnModels,
        inner,
        focused,
        &MODEL_COLS,
        rows,
        "no data",
    );
}

pub fn latency(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    let l = &app.data.latency;
    let empty = Default::default();
    let d = l.get().unwrap_or(&empty);
    if let Some(ld) = l.get()
        && !ld.supported
    {
        let inner = super::frame(f, app, PaneId::AnLatency, area, focused, vec![], None);
        widgets::message(
            f.buffer_mut(),
            inner,
            "latency diagnostics only cover the last 30 days",
            app.theme().graph_text,
        );
        return;
    }
    let pending = scatter_pane(
        f,
        app,
        PaneId::AnLatency,
        area,
        focused,
        &d.points,
        (d.p95_ttft_ms, d.p95_latency_ms),
        d.total_points,
        l.get().is_some(),
        stale_marker(app, l),
    );
    if let Some(inner) = pending {
        placeholder(f, app, inner, l);
    }
}

const SPEED_COLS: [Col; 4] = [
    flex("model", 12),
    col("mean", 10, Align::Right, 0),
    col("samples", 7, Align::Right, 2),
    col("per bucket", 16, Align::Left, 1),
];

pub fn speed(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let inner = frame(f, app, PaneId::AnSpeed, area, focused, vec![], None);
    if placeholder(f, app, inner, &app.data.analysis) {
        return;
    }
    let Some(a) = an(app) else { return };
    let rows = a
        .model_speed
        .series
        .iter()
        .map(|s| {
            let sum: f64 = s.samples.iter().map(|x| x.speed_tps_sum).sum();
            let cnt: f64 = s.samples.iter().map(|x| x.speed_sample_count).sum();
            let mean = (cnt > 0.0).then(|| sum / cnt);
            let per: Vec<f64> = s
                .samples
                .iter()
                .map(|x| x.mean_tps.unwrap_or(0.0))
                .collect();
            TRow::new(s.model.clone())
                .text(s.model.clone(), text(&s.model))
                .styled(
                    mean.map(fmt::tps).unwrap_or("-".into()),
                    Style::default().fg(t.title),
                    num(mean.unwrap_or(-1.0)),
                )
                .text(fmt::int(s.speed_sample_count), num(s.speed_sample_count))
                .spans(spark(&per, 16, t), num(mean.unwrap_or(-1.0)))
                .filter(s.model.clone())
        })
        .collect();
    table_in(
        f,
        app,
        hit,
        PaneId::AnSpeed,
        inner,
        focused,
        &SPEED_COLS,
        rows,
        "no speed samples",
    );
}

const EFF_COLS: [Col; 8] = [
    flex("model", 12),
    col("tokens", 7, Align::Right, 0),
    col("req", 6, Align::Right, 3),
    col("cost", 9, Align::Right, 0),
    col("$/1M", 8, Align::Right, 1),
    col("$/req", 8, Align::Right, 2),
    col("out/req", 7, Align::Right, 4),
    col("cache", 6, Align::Right, 5),
];

pub fn efficiency(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let inner = frame(f, app, PaneId::AnEfficiency, area, focused, vec![], None);
    if placeholder(f, app, inner, &app.data.analysis) {
        return;
    }
    let Some(a) = an(app) else { return };
    let rows = a
        .model_efficiency
        .iter()
        .map(|m| {
            let per_m = (m.total_tokens > 0.0).then(|| m.cost_usd / m.total_tokens * 1e6);
            let cost_style = Style::default().fg(if m.cost_available { t.fg } else { t.warn });
            TRow::new(m.model.clone())
                .text(m.model.clone(), text(&m.model))
                .styled(
                    fmt::compact(m.total_tokens),
                    Style::default().fg(t.title),
                    num(m.total_tokens),
                )
                .text(fmt::count(m.requests), num(m.requests))
                .styled(fmt::usd(m.cost_usd), cost_style, num(m.cost_usd))
                .text(
                    per_m.map(fmt::usd).unwrap_or("--".into()),
                    num(per_m.unwrap_or(-1.0)),
                )
                .text(
                    fmt::usd(m.cost_per_request_usd),
                    num(m.cost_per_request_usd),
                )
                .text(
                    fmt::compact(m.output_tokens_per_request),
                    num(m.output_tokens_per_request),
                )
                .text(
                    fmt::pct(m.cache_read_rate * 100.0, 1),
                    num(m.cache_read_rate),
                )
                .filter(m.model.clone())
        })
        .collect();
    table_in(
        f,
        app,
        hit,
        PaneId::AnEfficiency,
        inner,
        focused,
        &EFF_COLS,
        rows,
        "no data",
    );
}

pub fn heatmap(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let inner = frame(
        f,
        app,
        PaneId::AnHeatmap,
        area,
        focused,
        vec![Span::styled(
            "api key × model",
            Style::default().fg(t.graph_text),
        )],
        None,
    );
    if placeholder(f, app, inner, &app.data.analysis) {
        return;
    }
    let Some(a) = an(app) else { return };
    let h = &a.heatmap;
    let max = h.cells.iter().map(|c| c.total_tokens).fold(0.0, f64::max);
    // Columns: key, one per model (as many as fit), then totals.
    let mut cols: Vec<Col> = vec![flex("api key", 10)];
    for m in &h.models {
        let mut c = col("", 8, Align::Right, 1);
        // Drop the vendor prefix ("claude-", "gpt-") so columns stay distinguishable.
        let short = m.split_once('-').map(|(_, rest)| rest).unwrap_or(m);
        c.title = Cow::Owned(fmt::trunc(short, 8));
        cols.push(c);
    }
    cols.push(col("total", 8, Align::Right, 0));
    cols.push(col("cost", 9, Align::Right, 0));
    let rows = h
        .api_keys
        .iter()
        .map(|k| {
            let label = key_label(h.api_key_labels.get(k).map(String::as_str).unwrap_or(k));
            let mut row = TRow::new(k.clone()).text(label.clone(), text(&label));
            let (mut tot, mut cost) = (0.0, 0.0);
            for m in &h.models {
                let cell = h.cells.iter().find(|c| &c.api_key == k && &c.model == m);
                match cell {
                    Some(c) if c.total_tokens > 0.0 => {
                        tot += c.total_tokens;
                        cost += c.cost_usd;
                        let v = 0.05 + 0.95 * (c.total_tokens / max.max(1.0)).powf(0.65);
                        row = row.styled(
                            fmt::compact(c.total_tokens),
                            Style::default().fg(t.gradient(v as f32)),
                            num(c.total_tokens),
                        );
                    }
                    _ => {
                        row = row.styled("·", Style::default().fg(t.meter_bg), num(0.0));
                    }
                }
            }
            row.styled(fmt::compact(tot), Style::default().fg(t.title), num(tot))
                .text(fmt::usd(cost), num(cost))
                .filter(label)
        })
        .collect();
    table_in(
        f,
        app,
        hit,
        PaneId::AnHeatmap,
        inner,
        focused,
        &cols,
        rows,
        "no data",
    );
}
