//! Credentials tab: credential list with health strips and a detail pane.

use super::{frame, placeholder, stale_marker, table_in};
use crate::api::models::Identity;
use crate::app::{App, Hit};
use crate::fmt;
use crate::layout::PaneId;
use crate::ui::table::{Align, Col, TRow, col, flex, num, text};
use crate::ui::widgets::{self, flow, health_color, meter, stat};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

pub(super) fn ident_name(app: &App, i: &Identity) -> String {
    let n = if !i.display_name.is_empty() {
        &i.display_name
    } else if !i.name.is_empty() {
        &i.name
    } else {
        &i.identity
    };
    app.privacy(n)
}

pub(super) fn plan(i: &Identity) -> String {
    i.subscription
        .as_ref()
        .map(|s| {
            s.tier_name
                .clone()
                .filter(|t| !t.is_empty())
                .unwrap_or(s.plan.clone())
        })
        .unwrap_or_default()
}

fn health_strip(app: &App, i: &Identity, width: usize) -> Vec<Span<'static>> {
    let t = app.theme();
    let Some(h) = &i.credential_health else {
        return vec![Span::styled("-", Style::default().fg(t.graph_text))];
    };
    let buckets: Vec<(f64, f64)> = h.buckets.iter().map(|b| (b.success, b.failure)).collect();
    let n = buckets.len().max(1);
    let group = n.div_ceil(width.max(1));
    let spans = buckets
        .chunks(group.max(1))
        .map(|c| {
            let (s, fl) = c.iter().fold((0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
            match health_color(t, s, fl) {
                Some(col) => Span::styled("▮", Style::default().fg(col)),
                None => Span::styled("▯", Style::default().fg(t.meter_bg)),
            }
        })
        .collect();
    widgets::merge_spans(spans)
}

const CRED_COLS: [Col; 11] = [
    flex("name", 14),
    col("type", 11, Align::Left, 3),
    col("plan", 8, Align::Left, 5),
    col("P", 3, Align::Right, 8),
    col("state", 8, Align::Left, 4),
    col("req", 7, Align::Right, 0),
    col("success", 15, Align::Left, 1),
    col("tokens", 7, Align::Right, 2),
    col("cache", 6, Align::Right, 7),
    col("health 5h", 15, Align::Left, 6),
    col("last", 5, Align::Right, 9),
];

pub fn list(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let l = &app.data.identities;
    let mut right = stale_marker(app, l);
    if let Some(p) = l.get()
        && right.is_empty()
    {
        let counts: Vec<String> = p
            .type_counts
            .iter()
            .map(|c| format!("{} {}", c.kind, c.count))
            .collect();
        right.push(Span::styled(
            format!("{} total · {}", p.total_count, counts.join(" ")),
            Style::default().fg(t.graph_text),
        ));
    }
    let inner = frame(f, app, PaneId::CrList, area, focused, right, None);
    if placeholder(f, app, inner, l) {
        return;
    }
    let Some(p) = l.get() else { return };
    let now = app.clock.utc();
    let rows = p
        .identities
        .iter()
        .map(|i| {
            let name = ident_name(app, i);
            let ps = i.period_stats.clone().unwrap_or_default();
            let (req, ok, input, cache_r, tok) = if i.period_stats.is_some() {
                (
                    ps.total_requests,
                    ps.success_count,
                    ps.input_tokens,
                    ps.cache_read_tokens,
                    ps.total_tokens,
                )
            } else {
                (
                    i.total_requests,
                    i.success_count,
                    i.input_tokens,
                    i.cache_read_tokens,
                    i.total_tokens,
                )
            };
            let succ = fmt::ratio_pct(ok, req);
            let mut sbar = meter(
                succ.unwrap_or(0.0) / 100.0,
                7,
                t,
                succ.map(|s| t.good_pct(s, 95.0, 80.0)),
            );
            sbar.push(Span::styled(
                format!(" {:>6}", succ.map(|s| fmt::pct(s, 1)).unwrap_or("—".into())),
                Style::default().fg(t.fg),
            ));
            let state = if i.is_deleted {
                Span::styled("deleted", Style::default().fg(t.err))
            } else if i.disabled {
                Span::styled("disabled", Style::default().fg(t.warn))
            } else {
                Span::styled("enabled", Style::default().fg(t.ok))
            };
            let cache = fmt::ratio_pct(cache_r, input);
            let last = i.last_used_at.as_deref().and_then(|s| fmt::since(s, now));
            let kind = if i.auth_type == 2 {
                format!("{} (api)", i.kind)
            } else {
                i.kind.clone()
            };
            TRow::new(i.id.clone())
                .text(name.clone(), text(&name))
                .text(kind.clone(), text(&kind))
                .text(plan(i), text(&plan(i)))
                .text(
                    i.priority.map(|p| p.to_string()).unwrap_or("-".into()),
                    num(i.priority.unwrap_or(i64::MIN) as f64),
                )
                .spans(vec![state], text(if i.disabled { "b" } else { "a" }))
                .styled(fmt::count(req), Style::default().fg(t.title), num(req))
                .spans(sbar, num(succ.unwrap_or(-1.0)))
                .text(fmt::compact(tok), num(tok))
                .text(
                    cache.map(|c| fmt::pct(c, 0)).unwrap_or("—".into()),
                    num(cache.unwrap_or(-1.0)),
                )
                .spans(
                    health_strip(app, i, 15),
                    num(i
                        .credential_health
                        .as_ref()
                        .map(|h| h.success_rate)
                        .unwrap_or(-1.0)),
                )
                .text(
                    last.map(fmt::age).unwrap_or("-".into()),
                    num(-(last.map(|x| x as f64).unwrap_or(f64::MAX))),
                )
                .filter(format!("{name} {} {} {}", i.kind, i.provider, plan(i)))
        })
        .collect();
    table_in(
        f,
        app,
        hit,
        PaneId::CrList,
        inner,
        focused,
        &CRED_COLS,
        rows,
        "no credentials",
    );
}

pub fn detail(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let sel = hit.selected.get(&PaneId::CrList).cloned();
    let ident = sel.as_ref().and_then(|id| {
        app.data
            .identities
            .get()
            .and_then(|p| p.identities.iter().find(|i| &i.id == id))
    });
    let right = ident
        .map(|i| {
            vec![Span::styled(
                format!("{} · {}", i.kind, i.auth_type_name),
                Style::default().fg(t.graph_text),
            )]
        })
        .unwrap_or_default();
    let inner = frame(f, app, PaneId::CrDetail, area, focused, right, None);
    let Some(i) = ident else {
        widgets::message(f.buffer_mut(), inner, "select a credential", t.graph_text);
        return;
    };
    let now = app.clock.utc();
    let mut items = vec![
        vec![Span::styled(
            ident_name(app, i),
            Style::default().fg(t.title).add_modifier(Modifier::BOLD),
        )],
        stat(t, "provider", i.provider.clone(), Some(t.fg)),
    ];
    if !plan(i).is_empty() {
        items.push(stat(t, "plan", plan(i), Some(t.accent)));
    }
    if let Some(until) = i.active_until.as_deref()
        && let Some(s) = fmt::until(until, now)
    {
        items.push(stat(t, "expires in", fmt::secs(s as f64), None));
    }
    items.push(stat(t, "lifetime req", fmt::int(i.total_requests), None));
    items.push(stat(t, "tokens", fmt::compact(i.total_tokens), None));
    if let Some(r) = &i.stats_reset_at {
        items.push(stat(
            t,
            "since reset",
            fmt::ts_fmt(r, "%m-%d %H:%M"),
            Some(t.fg),
        ));
    }
    if let Some(h) = &i.credential_health {
        let s = if h.total_success + h.total_failure > 0.0 {
            fmt::pct(h.success_rate, 1)
        } else {
            "quiet".into()
        };
        items.push(stat(
            t,
            "5h health",
            format!(
                "{s} ✓{} ✗{}",
                fmt::int(h.total_success),
                fmt::int(h.total_failure)
            ),
            Some(t.good_pct(
                if h.total_success + h.total_failure > 0.0 {
                    h.success_rate
                } else {
                    100.0
                },
                95.0,
                80.0,
            )),
        ));
    }
    if let Some(lu) = i.last_used_at.as_deref().and_then(|s| fmt::since(s, now)) {
        items.push(stat(
            t,
            "last used",
            format!("{} ago", fmt::age(lu)),
            Some(t.fg),
        ));
    }
    let mut lines = flow(items, inner.width.saturating_sub(1), 3);
    let mut strip = vec![Span::styled("health ", Style::default().fg(t.graph_text))];
    strip.extend(health_strip(
        app,
        i,
        inner.width.saturating_sub(10) as usize,
    ));
    lines.push(Line::from(strip));
    lines.push(Line::from(Span::styled(
        "recent errors",
        Style::default().fg(t.hi).add_modifier(Modifier::BOLD),
    )));
    match app
        .data
        .identity_errors
        .as_ref()
        .filter(|(id, _)| *id == i.id)
    {
        Some((_, l)) => match (&l.data, &l.error) {
            (Some(errs), _) if errs.events.is_empty() => lines.push(Line::from(Span::styled(
                "no error events reported for this credential",
                Style::default().fg(t.graph_text),
            ))),
            (Some(errs), _) => {
                let w = inner.width.saturating_sub(2) as usize;
                for e in &errs.events {
                    let head = format!(
                        "{} HTTP {} {} ",
                        fmt::ts_fmt(&e.timestamp, "%m-%d %H:%M:%S"),
                        e.status_code,
                        e.model.clone().unwrap_or_default()
                    );
                    let room = w.saturating_sub(fmt::width(&head));
                    let body = fmt::trunc(&e.body_summary.replace(['\n', '\r'], " "), room);
                    lines.push(Line::from(vec![
                        Span::styled(head, Style::default().fg(t.graph_text)),
                        Span::styled(body, Style::default().fg(t.title)),
                    ]));
                }
            }
            (None, Some(err)) => lines.push(Line::from(Span::styled(
                format!("✗ {err}"),
                Style::default().fg(t.err),
            ))),
            (None, None) => lines.push(Line::from(Span::styled(
                "loading…",
                Style::default().fg(t.graph_text),
            ))),
        },
        None => lines.push(Line::from(Span::styled(
            "…",
            Style::default().fg(t.graph_text),
        ))),
    }
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
