//! Analysis tab.

use super::realtime::scatter_pane;
use super::{
    Series, chart, cursor, cursor_time, dim_tabs, frame, placeholder, stale_marker, table_in,
    time_labels,
};
use crate::api::models::{Analysis, CompositionItem};
use crate::app::{App, Hit};
use crate::fmt;
use crate::layout::PaneId;
use crate::ui::table::{Align, Col, TRow, col, flex, grow, num, text};
use crate::ui::widgets::{self, flow, stat};
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

pub fn tokens(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let l = &app.data.analysis;
    let mut right = stale_marker(app, l);
    if let Some(a) = an(app)
        && right.is_empty()
    {
        right.push(Span::styled(
            format!("{} · {}", app.range.label(), a.granularity),
            Style::default().fg(t.graph_text),
        ));
    }
    let inner = frame(f, app, PaneId::AnTokens, area, focused, right, None);
    if placeholder(f, app, inner, l) {
        return;
    }
    let Some(a) = an(app) else { return };
    let series: Vec<f64> = a.token_usage.iter().map(|b| b.total_tokens).collect();
    let buckets: Vec<String> = a.token_usage.iter().map(|b| b.bucket.clone()).collect();
    let cur = cursor(app, PaneId::AnTokens, series.len());
    let items = match cur {
        Some(i) => {
            let b = &a.token_usage[i];
            vec![
                vec![Span::styled(
                    format!("@ {}", cursor_time(&b.bucket, daily(a))),
                    Style::default().fg(t.hi),
                )],
                stat(t, "tokens", fmt::compact(b.total_tokens), None),
                stat(t, "requests", fmt::int(b.requests), None),
                stat(
                    t,
                    "cost",
                    if b.cost_available {
                        fmt::usd(b.cost_usd)
                    } else {
                        "—".into()
                    },
                    Some(t.accent),
                ),
                stat(t, "in", fmt::compact(b.input_tokens), Some(t.fg)),
                stat(t, "out", fmt::compact(b.output_tokens), Some(t.fg)),
                stat(t, "cache r", fmt::compact(b.cache_read_tokens), Some(t.fg)),
            ]
        }
        None => {
            let total: f64 = series.iter().sum();
            let reqs: f64 = a.token_usage.iter().map(|b| b.requests).sum();
            let cost = a.cost_summary.total_cost_usd;
            let blended = if total > 0.0 { cost / total * 1e6 } else { 0.0 };
            let avg = if series.is_empty() {
                0.0
            } else {
                total / series.len() as f64
            };
            let mut cost_item = stat(t, "cost", fmt::usd(cost), Some(t.accent));
            if !a.cost_summary.cost_available {
                cost_item.push(Span::styled(" (partial)", Style::default().fg(t.warn)));
            }
            vec![
                stat(t, "tokens", fmt::compact(total), None),
                stat(t, "requests", fmt::int(reqs), None),
                cost_item,
                stat(t, "$/1M", fmt::usd(blended), None),
                stat(
                    t,
                    if daily(a) { "avg/day" } else { "avg/hour" },
                    fmt::compact(avg),
                    None,
                ),
            ]
        }
    };
    let lines = flow(items, inner.width.saturating_sub(1), 3);
    let lh = (lines.len() as u16).min(inner.height);
    f.render_widget(
        Paragraph::new(lines),
        Rect::new(inner.x + 1, inner.y, inner.width.saturating_sub(1), lh),
    );
    let g = Rect::new(
        inner.x + 1,
        inner.y + lh,
        inner.width.saturating_sub(1),
        inner.height.saturating_sub(lh),
    );
    if series.iter().all(|v| *v == 0.0) {
        widgets::message(f.buffer_mut(), g, "no usage in this range", t.graph_text);
        return;
    }
    let labels = time_labels(&buckets, daily(a), false);
    chart(
        f,
        app,
        hit,
        PaneId::AnTokens,
        g,
        &labels,
        &Series {
            values: &series,
            fmt: fmt::compact,
            color: None,
            max: None,
            floor: 0.0,
        },
        app.settings.graph_symbols,
        true,
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

pub fn composition_len(app: &App) -> Option<usize> {
    an(app).map(|_| comp_items(app).len())
}

pub fn models_len(app: &App) -> Option<usize> {
    an(app).map(|a| a.model_usage.series.len())
}

pub fn speed_len(app: &App) -> Option<usize> {
    an(app).map(|a| a.model_speed.series.len())
}

pub fn efficiency_len(app: &App) -> Option<usize> {
    an(app).map(|a| a.model_efficiency.len())
}

pub fn heatmap_len(app: &App) -> Option<usize> {
    an(app).map(|a| a.heatmap.api_keys.len())
}

const COMP_COLS: [Col; 9] = [
    col("#", 3, Align::Right, 6),
    flex("name", 12),
    col("tokens", 7, Align::Right, 0),
    grow("share", 14, 1),
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
                app.key_name(&c.key, &c.label)
            } else {
                app.privacy(&c.label)
            };
            let cache = fmt::ratio_pct(c.cache_read_tokens, c.input_tokens);
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
                .meter(
                    c.percent / 100.0,
                    None,
                    format!(" {:>5.1}%", c.percent),
                    num(c.percent),
                )
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
    grow("share", 12, 1),
    col("req", 6, Align::Right, 2),
    col("cost", 9, Align::Right, 3),
    col("trend", 8, Align::Left, 1),
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
                .meter(share / 100.0, None, format!(" {:>5.1}%", share), num(share))
                .text(fmt::count(req), num(req))
                .text(
                    comp.map(|c| fmt::usd(c.cost_usd)).unwrap_or("—".into()),
                    num(comp.map(|c| c.cost_usd).unwrap_or(-1.0)),
                )
                .spark(s.total_tokens.clone(), t.accent, num(tok))
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

const SPEED_COLS: [Col; 5] = [
    flex("model", 12),
    col("mean", 10, Align::Right, 0),
    col("samples", 7, Align::Right, 2),
    col("per bucket", 10, Align::Left, 1),
    col("range", 15, Align::Right, 3),
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
            // Buckets without samples have no speed: show a gap, not zero.
            let per: Vec<f64> = s
                .samples
                .iter()
                .map(|x| x.mean_tps.filter(|v| v.is_finite()).unwrap_or(f64::NAN))
                .collect();
            let known: Vec<f64> = per.iter().copied().filter(|v| v.is_finite()).collect();
            let range = if known.is_empty() {
                "-".to_string()
            } else {
                let lo = known.iter().copied().fold(f64::MAX, f64::min);
                let hi = known.iter().copied().fold(0.0, f64::max);
                if (hi - lo).abs() < 0.5 {
                    format!("{hi:.0} t/s")
                } else {
                    format!("{lo:.0}–{hi:.0} t/s")
                }
            };
            TRow::new(s.model.clone())
                .text(s.model.clone(), text(&s.model))
                .styled(
                    mean.map(fmt::tps).unwrap_or("-".into()),
                    Style::default().fg(t.title),
                    num(mean.unwrap_or(-1.0)),
                )
                .text(fmt::int(s.speed_sample_count), num(s.speed_sample_count))
                .spark(per, t.accent, num(mean.unwrap_or(-1.0)))
                .text(range, num(known.len() as f64))
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
    // Model columns ranked by total tokens: the busiest models are shown
    // first and the quietest are dropped first when the pane is narrow.
    let mut models: Vec<(&String, f64)> = h
        .models
        .iter()
        .map(|m| {
            let tot = h
                .cells
                .iter()
                .filter(|c| &c.model == m)
                .map(|c| c.total_tokens)
                .sum::<f64>();
            (m, tot)
        })
        .collect();
    models.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    // Columns: key, one per model (as many as fit), then totals.
    let mut cols: Vec<Col> = vec![flex("api key", 10)];
    for (rank, (m, _)) in models.iter().enumerate() {
        let mut c = col("", 8, Align::Right, (rank + 1).min(250) as u8);
        // Drop the vendor prefix ("claude-", "gpt-") so columns stay distinguishable.
        let short = m.split_once('-').map(|(_, rest)| rest).unwrap_or(m);
        c.title = Cow::Owned(fmt::trunc(short, 10));
        cols.push(c);
    }
    cols.push(col("total", 8, Align::Right, 0));
    cols.push(col("cost", 9, Align::Right, 0));
    let rows = h
        .api_keys
        .iter()
        .map(|k| {
            let raw = h.api_key_labels.get(k).map(String::as_str).unwrap_or(k);
            let label = app.key_name(k, raw);
            let mut row = TRow::new(k.clone()).text(label.clone(), text(&label));
            let (mut tot, mut cost) = (0.0, 0.0);
            for (m, _) in &models {
                let cell = h.cells.iter().find(|c| &c.api_key == k && &c.model == *m);
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
