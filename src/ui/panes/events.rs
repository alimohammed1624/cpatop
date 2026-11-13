//! Live request log (admin).

use super::{frame, table_in, token_details, when, yes_no};
use crate::api::models::UsageEvent;
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
                format!(
                    "▲ {} · retry {s}s",
                    fmt::trunc(&app.privacy_text(message), 28)
                ),
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
            " · {} loaded of {total} · {}",
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
    if let Some((value, label)) = &f.source {
        v.push(Span::styled(
            format!(" · {}", app.credential_name(value, label)),
            hi,
        ));
    }
    v
}

/// Every field of a request for the row popup, through the privacy
/// helpers (addresses, agent, endpoint and request id included).
fn event_fields(app: &App, e: &UsageEvent, key: &str, source: &str) -> Vec<(&'static str, String)> {
    let opt = |o: &Option<String>| o.clone().unwrap_or_default();
    let tk = &e.tokens;
    let mut v = vec![
        ("time", when(app, Some(&e.timestamp))),
        ("API key", key.to_string()),
        ("source", source.to_string()),
        ("source type", app.privacy(&opt(&e.source_type))),
        ("auth index", app.opaque_id(&opt(&e.auth_index))),
        ("model", app.privacy(&e.model)),
        ("model alias", app.privacy(&opt(&e.model_alias))),
        ("upstream model", app.privacy(&opt(&e.response_model))),
        ("reasoning effort", app.privacy(&opt(&e.reasoning_effort))),
        (
            "result",
            format!(
                "{} {}",
                if e.failed {
                    "✗ failed"
                } else {
                    "✓ success"
                },
                e.status_code.map(|c| c.to_string()).unwrap_or_default()
            ),
        ),
        ("endpoint", app.endpoint(&opt(&e.endpoint))),
        ("stream", e.stream.map(yes_no).unwrap_or_default()),
        ("latency", format!("{} ms", fmt::int(e.latency_ms))),
        (
            "time to first token",
            e.ttft_ms
                .filter(|v| *v > 0.0)
                .map(|v| format!("{} ms", fmt::int(v)))
                .unwrap_or_default(),
        ),
        (
            "output speed",
            e.speed_tps.map(fmt::tps).unwrap_or_default(),
        ),
        ("service tier", app.privacy(&opt(&e.service_tier))),
        (
            "effective tier",
            app.privacy(&opt(&e.effective_service_tier)),
        ),
        ("response tier", app.privacy(&opt(&e.response_service_tier))),
        ("executor", app.privacy(&opt(&e.executor_type))),
    ];
    v.extend(token_details(
        tk.input_tokens,
        tk.output_tokens,
        tk.cache_read_tokens,
        tk.cache_creation_tokens,
        tk.reasoning_tokens,
        tk.total_tokens,
    ));
    v.extend([
        (
            "cost",
            if e.cost_available {
                format!("${:.6}", e.cost_usd)
            } else {
                "not priced".into()
            },
        ),
        ("pricing style", app.privacy(&opt(&e.pricing_style))),
        ("client IP", app.ip(&opt(&e.client_ip))),
        ("X-Forwarded-For", app.ip(&opt(&e.x_forwarded_for))),
        ("user agent", app.user_agent(&opt(&e.user_agent))),
        ("request id", app.opaque_id(&opt(&e.request_id))),
        ("event id", format!("#{}", e.id)),
    ]);
    if e.is_delete {
        v.push(("identity", "deleted".into()));
    }
    // A failed request's error text, as in the detail pane (untruncated).
    if let Some(l) = error_line(app, e, 2000) {
        let text: String = l.spans.iter().skip(1).map(|s| s.content.as_ref()).collect();
        v.insert(10, ("error", text.trim_end().to_string()));
    }
    v
}

/// Lines of the detail pane at an inner width (fields wrap when narrow),
/// plus two for the error text of a failed request.
pub fn detail_lines(app: &App, width: u16) -> u16 {
    let base = if width >= TWO_COLUMNS {
        4
    } else if width >= 76 {
        8
    } else if width >= 56 {
        10
    } else {
        12
    };
    let failed = app
        .live
        .selected
        .as_ref()
        .and_then(|id| app.live.events.iter().find(|e| &e.id == id))
        .is_some_and(|e| e.failed);
    base + if failed { 2 } else { 0 }
}

/// The readable part of an upstream error body: `type: message` of a JSON
/// error (`{"error":{"type":…,"message":…}}`), else the body as it is.
fn error_text(body: &str) -> String {
    let body = body.replace(['\n', '\r'], " ");
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body.trim()) else {
        return body;
    };
    let err = v.get("error").filter(|e| e.is_object()).unwrap_or(&v);
    let s = |k: &str| {
        err.get(k)
            .and_then(|x| x.as_str())
            .filter(|x| !x.is_empty())
    };
    match (s("type").or(s("status")).or(s("code")), s("message")) {
        (Some(t), Some(m)) => format!("{t}: {m}"),
        (None, Some(m)) => m.to_string(),
        _ => body,
    }
}

/// The error line of a failed request: the text its credential's error log
/// has for it (scrubbed in privacy mode, cut to two lines of `width`), or
/// why there is none.
fn error_line(app: &App, e: &UsageEvent, width: usize) -> Option<Line<'static>> {
    if !e.failed {
        return None;
    }
    let t = app.theme();
    let key = Span::styled("error ", Style::default().fg(t.graph_text));
    let note = |s: &str| Span::styled(s.to_string(), Style::default().fg(t.graph_text));
    if app.role != Some(crate::api::Role::Admin) {
        return None;
    }
    let Some(ident) = app.identity_for(e.auth_index.as_deref().unwrap_or("")) else {
        return Some(Line::from(vec![
            key,
            note("not recorded (unknown credential)"),
        ]));
    };
    let slot = app
        .data
        .identity_errors
        .as_ref()
        .filter(|(id, _)| *id == ident.id)
        .map(|(_, l)| l);
    let body = match slot {
        Some(l) => match (&l.data, &l.error) {
            (Some(errs), _) => match crate::app::match_error(e, &errs.events) {
                Some(x) => {
                    let mut text = app.privacy_text(&error_text(&x.body_summary));
                    if x.body_truncated {
                        text.push('…');
                    }
                    let mut v = vec![
                        key,
                        Span::styled(
                            format!("HTTP {} ", x.status_code),
                            Style::default().fg(t.err),
                        ),
                    ];
                    if let Some(c) = x.code.as_deref().filter(|c| !c.is_empty()) {
                        v.push(note(&format!("({}) ", app.privacy_text(c))));
                    }
                    let head: usize = v.iter().map(|s| s.width()).sum();
                    // Two lines at most (wrapping loses a few cells at
                    // word breaks).
                    let room = (width * 2).saturating_sub(head + 8).max(20);
                    v.push(Span::styled(
                        fmt::trunc(&text, room),
                        Style::default().fg(t.title),
                    ));
                    return Some(Line::from(v));
                }
                None => note("not in the credential's recent error log"),
            },
            (None, Some(err)) => Span::styled(
                format!("✗ {}", app.privacy_text(err)),
                Style::default().fg(t.err),
            ),
            (None, None) => note("loading…"),
        },
        None => note("loading…"),
    };
    Some(Line::from(vec![key, body]))
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
            let (source, source_short) =
                app.credential_labels(e.auth_index.as_deref().unwrap_or(""), &e.source);
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
                .name(
                    source.clone(),
                    source_short,
                    Style::default(),
                    text(&source),
                )
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
                    app.endpoint(e.endpoint.as_deref().unwrap_or(""))
                ))
                .details(event_fields(app, e, &key, &source))
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
            v(app.endpoint(&opt(&e.endpoint))),
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
            v(app.credential_name(e.auth_index.as_deref().unwrap_or(""), &e.source)),
            sep(),
            k("type"),
            v(opt(&e.source_type)),
            sep(),
            k("api"),
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
            v(fmt::trunc(&app.user_agent(&opt(&e.user_agent)), 80)),
        ]),
    ];
    // A failed request's error text, right under its result.
    let error = error_line(app, e, inner.width.saturating_sub(1) as usize);
    let body = Rect::new(
        inner.x + 1,
        inner.y,
        inner.width.saturating_sub(1),
        inner.height,
    );
    // Wide panes set the fields side by side (request and tokens on the
    // left, cost and origin on the right) instead of leaving most of the
    // pane blank.
    if body.width >= TWO_COLUMNS && lines.len() == 8 {
        let tw = body.width as usize;
        let (left, right) = lines.split_at(4);
        // The right column starts half-way across (or after the longest
        // left line), so both halves share the width.
        let lw = left
            .iter()
            .map(Line::width)
            .max()
            .unwrap_or(0)
            .max((tw - DETAIL_GAP) / 2)
            .min(tw - DETAIL_GAP - MIN_RIGHT);
        let rx = body.x + (lw + DETAIL_GAP) as u16;
        let rw = tw - lw - DETAIL_GAP;
        let buf = f.buffer_mut();
        for (i, (l, r)) in left.iter().zip(right).enumerate() {
            let y = body.y + i as u16;
            if y >= body.bottom() {
                break;
            }
            let fit =
                |l: &Line<'static>, w: usize| Line::from(super::fit_spans(l.spans.clone(), w));
            buf.set_line(body.x, y, &fit(l, lw), lw as u16);
            buf.set_line(rx, y, &fit(r, rw), rw as u16);
        }
        // The error text spans the full width below the two columns.
        if let Some(l) = error {
            let y = body.y + 4;
            if y < body.bottom() {
                let rest = Rect::new(body.x, y, body.width, body.bottom() - y);
                f.render_widget(Paragraph::new(l).wrap(Wrap { trim: true }), rest);
            }
        }
        return;
    }
    let mut lines = lines;
    if let Some(l) = error {
        lines.insert(2, l);
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), body);
}

/// Text width from which the detail fields are set in two columns.
const TWO_COLUMNS: u16 = 150;
const DETAIL_GAP: usize = 4;
const MIN_RIGHT: usize = 60;

#[cfg(test)]
mod tests {
    use super::error_text;

    #[test]
    fn json_error_bodies_read_as_type_and_message() {
        assert_eq!(
            error_text(
                r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down"},"request_id":"x"}"#
            ),
            "rate_limit_error: slow down"
        );
        assert_eq!(error_text(r#"{"message":"bad"}"#), "bad");
        assert_eq!(error_text("context canceled"), "context canceled");
        // Cut-off JSON stays as it is.
        assert_eq!(
            error_text(r#"{"error":{"message":"cut"#),
            r#"{"error":{"message":"cut"#
        );
    }
}
