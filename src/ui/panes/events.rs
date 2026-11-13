//! Live request log (admin).

use super::{frame, table_in};
use crate::app::{App, Hit, LiveStatus, ResultFilter};
use crate::fmt;
use crate::layout::PaneId;
use crate::ui::table::{Align, Col, TRow, col, flex, num, text};
use crate::ui::widgets;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

const COLS: [Col; 12] = [
    col("time", 8, Align::Left, 0),
    col("key", 6, Align::Left, 6),
    col("source", 12, Align::Left, 4),
    flex("model", 12),
    col("effort", 6, Align::Left, 8),
    col("result", 6, Align::Left, 0),
    col("latency", 7, Align::Right, 1),
    col("ttft", 7, Align::Right, 5),
    col("speed", 9, Align::Right, 7),
    col("tokens", 7, Align::Right, 2),
    col("cache", 6, Align::Right, 9),
    col("cost", 8, Align::Right, 3),
];

fn status_spans(app: &App) -> Vec<Span<'static>> {
    let t = app.theme();
    let l = &app.live;
    let mut v = match &l.status {
        LiveStatus::Live => vec![Span::styled(
            "● live",
            Style::default().fg(t.ok).add_modifier(Modifier::BOLD),
        )],
        LiveStatus::Busy => vec![Span::styled(
            "● live (costs updating)",
            Style::default().fg(t.warn),
        )],
        LiveStatus::Connecting => vec![Span::styled("○ connecting", Style::default().fg(t.warn))],
        LiveStatus::Idle => vec![Span::styled("○ idle", Style::default().fg(t.graph_text))],
        LiveStatus::Retrying { message, until } => {
            let s = until.saturating_duration_since(app.now()).as_secs();
            vec![Span::styled(
                format!("▲ {} · retry {s}s", fmt::trunc(&app.privacy_text(message), 28)),
                Style::default().fg(t.err),
            )]
        }
        LiveStatus::Failed(m) => vec![Span::styled(
            format!("✗ {}", fmt::trunc(&app.privacy_text(m), 40)),
            Style::default().fg(t.err),
        )],
    };
    let total = if l.total >= 0 {
        fmt::int(l.total as f64)
    } else {
        "?".into()
    };
    v.push(Span::styled(
        format!(
            " · {}/{total} in {}",
            fmt::int(l.events.len() as f64),
            app.range.label()
        ),
        Style::default().fg(t.graph_text),
    ));
    if l.loading_more {
        v.push(Span::styled(
            " · loading older…",
            Style::default().fg(t.warn),
        ));
    }
    // Active server-side filters.
    let f = &l.filters;
    let hi = Style::default().fg(t.hi).add_modifier(Modifier::BOLD);
    match f.result {
        ResultFilter::All => {}
        ResultFilter::Success => v.push(Span::styled(" · ✓ success", hi)),
        ResultFilter::Failed => v.push(Span::styled(" · ✗ failed", hi)),
    }
    if let Some(m) = &f.model {
        v.push(Span::styled(format!(" · {m}"), hi));
    }
    if let Some((_, label)) = &f.source {
        v.push(Span::styled(format!(" · {}", app.privacy(label)), hi));
    }
    v
}

/// Lines of the detail pane at an inner width (fields wrap when narrow).
pub fn detail_lines(width: u16) -> u16 {
    if width >= 76 {
        8
    } else if width >= 56 {
        10
    } else {
        12
    }
}

pub fn log(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let bottom = (app.live.has_more && !app.live.loading_more)
        .then(|| ("↓ scroll for older".to_string(), String::new()));
    let inner = frame(
        f,
        app,
        PaneId::EvLog,
        area,
        focused,
        status_spans(app),
        bottom,
    );
    let rows: Vec<TRow> = app
        .live
        .events
        .iter()
        .map(|e| {
            let key = e
                .api_key
                .as_deref()
                .map(|k| app.key_name_by_label(k))
                .unwrap_or("-".into());
            let source = app.privacy(&e.source);
            let result = if e.failed {
                Span::styled(
                    format!("✗ {}", e.status_code.unwrap_or(0)),
                    Style::default().fg(t.err).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    format!("✓ {}", e.status_code.unwrap_or(200)),
                    Style::default().fg(t.ok),
                )
            };
            let mut model = e.model.clone();
            if let Some(rm) = e
                .response_model
                .as_ref()
                .filter(|m| **m != e.model && !m.is_empty())
            {
                model.push_str(&format!(" → {rm}"));
            }
            let cache = fmt::ratio_pct(e.tokens.cache_read_tokens, e.tokens.input_tokens);
            TRow::new(e.id.clone())
                .styled(
                    fmt::ts_fmt(&e.timestamp, "%H:%M:%S"),
                    Style::default().fg(t.graph_text),
                    text(&e.timestamp),
                )
                .text(key.clone(), text(&key))
                .text(source.clone(), text(&source))
                .text(model.clone(), text(&model))
                .text(
                    e.reasoning_effort.clone().unwrap_or("-".into()),
                    text(e.reasoning_effort.as_deref().unwrap_or("")),
                )
                .spans(vec![result], num(if e.failed { 1.0 } else { 0.0 }))
                .text(fmt::ms(e.latency_ms), num(e.latency_ms))
                .text(
                    e.ttft_ms
                        .filter(|v| *v > 0.0)
                        .map(fmt::ms)
                        .unwrap_or("-".into()),
                    num(e.ttft_ms.unwrap_or(-1.0)),
                )
                .text(
                    e.speed_tps.map(fmt::tps).unwrap_or("-".into()),
                    num(e.speed_tps.unwrap_or(-1.0)),
                )
                .styled(
                    fmt::compact(e.tokens.total_tokens),
                    Style::default().fg(t.title),
                    num(e.tokens.total_tokens),
                )
                .text(
                    cache.map(|c| fmt::pct(c, 1)).unwrap_or("-".into()),
                    num(cache.unwrap_or(-1.0)),
                )
                .text(
                    if e.cost_available {
                        fmt::usd(e.cost_usd)
                    } else {
                        "-".into()
                    },
                    num(e.cost_usd),
                )
                .filter(format!(
                    "{} {} {} {}",
                    e.model,
                    source,
                    key,
                    e.endpoint.as_deref().unwrap_or("")
                ))
        })
        .collect();
    let empty = match &app.live.status {
        LiveStatus::Connecting | LiveStatus::Idle => "waiting for the live stream…",
        _ if !app.live.filters.is_empty() => "no requests match these filters (c clears)",
        _ => "no requests in this range",
    };
    table_in(
        f,
        app,
        hit,
        PaneId::EvLog,
        inner,
        focused,
        &COLS,
        rows,
        empty,
    );
}

pub fn detail(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let sel = hit.selected.get(&PaneId::EvLog).cloned();
    let ev = sel.and_then(|id| app.live.events.iter().find(|e| e.id == id));
    let right = ev
        .map(|e| {
            vec![Span::styled(
                format!(
                    "#{} · {}",
                    e.id,
                    fmt::ts_fmt(&e.timestamp, "%Y-%m-%d %H:%M:%S")
                ),
                Style::default().fg(t.graph_text),
            )]
        })
        .unwrap_or_default();
    let inner = frame(f, app, PaneId::EvDetail, area, focused, right, None);
    let Some(e) = ev else {
        widgets::message(f.buffer_mut(), inner, "select a request", t.graph_text);
        return;
    };
    let k = |s: &str| Span::styled(format!("{s} "), Style::default().fg(t.graph_text));
    let v = |s: String| Span::styled(s, Style::default().fg(t.title));
    let sep = || Span::styled("  ", Style::default());
    let opt = |o: &Option<String>| o.clone().filter(|s| !s.is_empty()).unwrap_or("-".into());
    let tier = e
        .effective_service_tier
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            format!(
                "{} / {}",
                opt(&e.service_tier),
                opt(&e.response_service_tier)
            )
        });
    let tk = &e.tokens;
    let lines = vec![
        Line::from(vec![
            k("model"),
            v(e.model.clone()),
            sep(),
            k("alias"),
            v(opt(&e.model_alias)),
            sep(),
            k("upstream"),
            v(opt(&e.response_model)),
        ]),
        Line::from(vec![
            k("result"),
            if e.failed {
                Span::styled(
                    format!("failure {}", e.status_code.unwrap_or(0)),
                    Style::default().fg(t.err).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    format!("success {}", e.status_code.unwrap_or(200)),
                    Style::default().fg(t.ok),
                )
            },
            sep(),
            k("endpoint"),
            v(opt(&e.endpoint)),
            sep(),
            k("stream"),
            v(e.stream.map(|s| s.to_string()).unwrap_or("-".into())),
        ]),
        Line::from(vec![
            k("latency"),
            v(fmt::ms(e.latency_ms)),
            sep(),
            k("ttft"),
            v(e.ttft_ms.map(fmt::ms).unwrap_or("-".into())),
            sep(),
            k("speed"),
            v(e.speed_tps.map(fmt::tps).unwrap_or("-".into())),
            sep(),
            k("effort"),
            v(opt(&e.reasoning_effort)),
            sep(),
            k("tier"),
            v(tier),
        ]),
        Line::from(vec![
            k("tokens"),
            v(fmt::int(tk.total_tokens)),
            sep(),
            k("in"),
            v(fmt::int(tk.input_tokens)),
            sep(),
            k("out"),
            v(fmt::int(tk.output_tokens)),
            sep(),
            k("reasoning"),
            v(fmt::int(tk.reasoning_tokens)),
            sep(),
            k("cache r/w"),
            v(format!(
                "{} / {}",
                fmt::int(tk.cache_read_tokens),
                fmt::int(tk.cache_creation_tokens)
            )),
        ]),
        Line::from(vec![
            k("cost"),
            v(if e.cost_available {
                fmt::usd(e.cost_usd)
            } else {
                "not priced".into()
            }),
            sep(),
            k("pricing"),
            v(opt(&e.pricing_style)),
            sep(),
            k("executor"),
            v(opt(&e.executor_type)),
        ]),
        Line::from(vec![
            k("source"),
            v(app.privacy(&e.source)),
            sep(),
            k("type"),
            v(opt(&e.source_type)),
            sep(),
            k("key"),
            v(e.api_key
                .as_deref()
                .map(|k| app.key_name_by_label(k))
                .unwrap_or("-".into())),
            if e.is_delete {
                Span::styled("  (identity deleted)", Style::default().fg(t.warn))
            } else {
                Span::raw("")
            },
        ]),
        Line::from(vec![
            k("client"),
            v(fmt::trunc(&app.ip(&opt(&e.client_ip)), 39)),
            sep(),
            k("xff"),
            v(fmt::trunc(&app.ip(&opt(&e.x_forwarded_for)), 48)),
        ]),
        Line::from(vec![
            k("agent"),
            v(fmt::trunc(&app.privacy_text(&opt(&e.user_agent)), 80)),
        ]),
    ];
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }),
        Rect::new(
            inner.x + 1,
            inner.y,
            inner.width.saturating_sub(1),
            inner.height,
        ),
    );
}
