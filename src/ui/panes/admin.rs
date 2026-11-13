//! Admin-only tabs: credentials, quota, models/pricing, keys and sessions.

use super::{frame, key_label, placeholder, stale_marker, table_in};
use crate::api::models::{Identity, QuotaRow};
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

fn ident_name(app: &App, i: &Identity) -> String {
    let n = if !i.display_name.is_empty() {
        &i.display_name
    } else if !i.name.is_empty() {
        &i.name
    } else {
        &i.identity
    };
    app.privacy(n)
}

fn plan(i: &Identity) -> String {
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

pub fn credentials(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
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

pub fn credential_detail(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
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

pub fn inspection(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    let t = app.theme();
    let l = &app.data.inspection;
    let mut right = stale_marker(app, l);
    if let Some(a) = app.data.auto_refresh.get() {
        let s = match (&a.schedule, a.enabled) {
            (Some(s), true) => format!("auto refresh every {} {}", s.value, s.unit),
            _ => "auto refresh off".into(),
        };
        right.push(Span::styled(s, Style::default().fg(t.graph_text)));
    }
    let inner = frame(f, app, PaneId::QuInspection, area, focused, right, None);
    if placeholder(f, app, inner, l) {
        return;
    }
    let Some(q) = l.get() else { return };
    let total = q.total.max(1.0);
    let part = |label: &str, v: f64, c| {
        let mut s = vec![Span::styled(
            format!("{label} "),
            Style::default().fg(t.graph_text),
        )];
        s.extend(meter(v / total, 6, t, Some(c)));
        s.push(Span::styled(
            format!(" {}", fmt::int(v)),
            Style::default().fg(t.title).add_modifier(Modifier::BOLD),
        ));
        s
    };
    let state = if q.running {
        Span::styled("inspection running", Style::default().fg(t.warn))
    } else if q.completed {
        Span::styled(
            format!(
                "inspected {}",
                q.completed_at
                    .as_deref()
                    .map(|s| fmt::ts_fmt(s, "%m-%d %H:%M"))
                    .unwrap_or_default()
            ),
            Style::default().fg(t.ok),
        )
    } else {
        Span::styled("cache only", Style::default().fg(t.graph_text))
    };
    let items = vec![
        stat(t, "accounts", fmt::int(q.total), None),
        stat(
            t,
            "checked",
            format!("{}/{}", fmt::int(q.cached), fmt::int(q.total)),
            None,
        ),
        part("normal", q.normal, t.ok),
        part("limited", q.limit_reached, t.warn),
        part("401", q.unauthorized_401, t.err),
        part("402", q.payment_required_402, t.err),
        part("failed", q.other_failed, t.err),
        part("unknown", q.unknown, t.graph_text),
        vec![state],
    ];
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

const QUOTA_COLS: [Col; 8] = [
    flex("credential", 14),
    col("plan", 7, Align::Left, 4),
    col("window", 13, Align::Left, 0),
    col("left", 18, Align::Left, 0),
    col("resets in", 9, Align::Right, 1),
    col("tokens", 7, Align::Right, 2),
    col("cost", 8, Align::Right, 3),
    col("status", 10, Align::Left, 5),
];

pub fn quota(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let l = &app.data.quota;
    let right = stale_marker(app, l);
    let inner = frame(f, app, PaneId::QuList, area, focused, right, None);
    if placeholder(f, app, inner, l) {
        return;
    }
    let Some(tasks) = l.get() else { return };
    let Some(ids) = app.data.identities.get() else {
        placeholder(f, app, inner, &app.data.identities);
        return;
    };
    let now = app.clock.utc();
    let status = app.data.inspection.get();
    let mut rows = Vec::new();
    let mut creds: Vec<&Identity> = ids
        .identities
        .iter()
        .filter(|i| i.auth_type == 1 && !i.is_deleted)
        .collect();
    creds.sort_by_key(|i| ident_name(app, i).to_lowercase());
    for i in creds {
        let name = ident_name(app, i);
        let insp = status.and_then(|s| s.results.iter().find(|r| r.auth_index == i.identity));
        let st_text = insp.map(|r| r.status.replace('_', " ")).unwrap_or_default();
        let st_style = Style::default().fg(match insp.map(|r| r.status.as_str()) {
            Some("normal") => t.ok,
            Some("limit_reached") => t.warn,
            Some(_) => t.err,
            None => t.graph_text,
        });
        let task = tasks.get(&i.identity);
        let quota_rows: Vec<&QuotaRow> = match task {
            Some(Ok(task)) => task
                .quota
                .as_ref()
                .map(|q| q.quota.iter().filter(|r| r.used_pct().is_some()).collect())
                .unwrap_or_default(),
            _ => vec![],
        };
        let plan_s = match task {
            Some(Ok(task)) => task
                .quota
                .as_ref()
                .and_then(|q| q.subscription.as_ref())
                .map(|s| s.plan.clone())
                .unwrap_or_else(|| plan(i)),
            _ => plan(i),
        };
        let filter = format!("{name} {} {plan_s}", i.kind);
        if quota_rows.is_empty() {
            let msg = match task {
                Some(Ok(task)) if task.status == "failed" => {
                    format!(
                        "failed {}",
                        task.http_status_code
                            .map(|c| c.to_string())
                            .unwrap_or_default()
                    )
                }
                Some(Ok(task)) => task.status.clone(),
                Some(Err(e)) => e.clone(),
                None => "not cached".into(),
            };
            rows.push(
                TRow::new(i.identity.clone())
                    .text(name.clone(), text(&name))
                    .text(plan_s.clone(), text(&plan_s))
                    .styled(
                        i.kind.clone(),
                        Style::default().fg(t.graph_text),
                        text(&i.kind),
                    )
                    .styled(
                        fmt::trunc(&msg, 30),
                        Style::default().fg(t.graph_text),
                        num(-1.0),
                    )
                    .text("", num(-1.0))
                    .text("", num(-1.0))
                    .text("", num(-1.0))
                    .styled(st_text.clone(), st_style, text(&st_text))
                    .filter(filter.clone()),
            );
            continue;
        }
        for (k, r) in quota_rows.iter().enumerate() {
            let used = r.used_pct().unwrap_or(0.0);
            let left = 100.0 - used;
            let color = if r.limit_reached == Some(true) || left < 20.0 {
                t.err
            } else if left < 50.0 {
                t.warn
            } else {
                t.ok
            };
            let mut bar = meter(left / 100.0, 10, t, Some(color));
            bar.push(Span::styled(
                format!(" {:>3.0}%", left),
                Style::default().fg(t.title).add_modifier(Modifier::BOLD),
            ));
            let reset = r
                .reset_at
                .as_deref()
                .and_then(|s| fmt::until(s, now))
                .or(r.reset_after_seconds.map(|s| s as u64));
            let first = k == 0;
            let (cell_name, cell_plan) = if first {
                (name.clone(), plan_s.clone())
            } else {
                (String::new(), String::new())
            };
            let (tok, cost) = if r.metric.as_deref() == Some("usd_cents") {
                (
                    String::new(),
                    format!(
                        "${:.2}/${:.2}",
                        r.used.unwrap_or(0.0) / 100.0,
                        r.limit.unwrap_or(0.0) / 100.0
                    ),
                )
            } else {
                (
                    r.window_usage_tokens.map(fmt::compact).unwrap_or_default(),
                    r.window_usage_cost.map(fmt::usd).unwrap_or_default(),
                )
            };
            rows.push(
                TRow::new(i.identity.clone())
                    .styled(cell_name, Style::default().fg(t.title), text(&name))
                    .text(cell_plan, text(&plan_s))
                    .text(r.display_label(), text(&r.display_label()))
                    .spans(bar, num(left))
                    .text(
                        reset.map(|s| fmt::secs(s as f64)).unwrap_or("-".into()),
                        num(reset.unwrap_or(0) as f64),
                    )
                    .text(tok, num(r.window_usage_tokens.unwrap_or(0.0)))
                    .text(cost, num(r.window_usage_cost.unwrap_or(0.0)))
                    .styled(
                        if first {
                            st_text.clone()
                        } else {
                            String::new()
                        },
                        st_style,
                        text(&st_text),
                    )
                    .filter(filter.clone()),
            );
        }
    }
    table_in(
        f,
        app,
        hit,
        PaneId::QuList,
        inner,
        focused,
        &QUOTA_COLS,
        rows,
        "no credentials with quota",
    );
}

const HIST_COLS: [Col; 9] = [
    col("cycle", 9, Align::Left, 0),
    col("window", 7, Align::Left, 3),
    col("started", 11, Align::Left, 4),
    col("resets", 11, Align::Left, 2),
    col("left", 9, Align::Right, 0),
    col("req", 6, Align::Right, 5),
    col("tokens", 7, Align::Right, 1),
    col("cost", 8, Align::Right, 1),
    col("tok/1%", 7, Align::Right, 2),
];

pub fn quota_history(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let sel = hit.selected.get(&PaneId::QuList).cloned();
    let ident = sel.as_ref().and_then(|idx| {
        app.data
            .identities
            .get()
            .and_then(|p| p.identities.iter().find(|i| &i.identity == idx))
    });
    let right = ident
        .map(|i| {
            vec![Span::styled(
                ident_name(app, i),
                Style::default().fg(t.graph_text),
            )]
        })
        .unwrap_or_default();
    let inner = frame(f, app, PaneId::QuHistory, area, focused, right, None);
    let Some(i) = ident else {
        widgets::message(f.buffer_mut(), inner, "select a credential", t.graph_text);
        return;
    };
    if i.kind != "codex" && i.kind != "claude" {
        widgets::message(
            f.buffer_mut(),
            inner,
            "quota history is available for codex and claude credentials",
            t.graph_text,
        );
        return;
    }
    let Some((_, l)) = app
        .data
        .quota_history
        .as_ref()
        .filter(|(idx, _)| *idx == i.identity)
    else {
        widgets::message(f.buffer_mut(), inner, "loading…", t.graph_text);
        return;
    };
    if placeholder(f, app, inner, l) {
        return;
    }
    let Some(h) = l.get() else { return };
    let kind = h
        .selected_window
        .as_ref()
        .and_then(|w| w.window_kind.clone())
        .unwrap_or_default()
        .replace("five_hour", "5h");
    let rows = h
        .cycles
        .iter()
        .map(|c| {
            let mut tpp: Vec<f64> = c.transitions.iter().map(|x| x.tokens_per_point).collect();
            tpp.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let median = (!tpp.is_empty()).then(|| tpp[tpp.len() / 2]);
            let left = match (c.first_remaining_percent, c.last_remaining_percent) {
                (Some(a), Some(b)) => format!("{a:.0}→{b:.0}%"),
                (_, Some(b)) => format!("{b:.0}%"),
                _ => "-".into(),
            };
            let status = if c.status == "current" {
                "● current"
            } else {
                "ended"
            };
            TRow::new(c.id.clone())
                .styled(
                    status,
                    Style::default().fg(if c.status == "current" {
                        t.ok
                    } else {
                        t.graph_text
                    }),
                    text(&c.status),
                )
                .text(kind.clone(), text(&kind))
                .text(
                    fmt::ts_fmt(&c.window_started_at, "%m-%d %H:%M"),
                    text(&c.window_started_at),
                )
                .text(fmt::ts_fmt(&c.reset_at, "%m-%d %H:%M"), text(&c.reset_at))
                .styled(
                    left.clone(),
                    Style::default().fg(t.title),
                    num(c.last_remaining_percent.unwrap_or(-1.0)),
                )
                .text(fmt::count(c.usage.requests), num(c.usage.requests))
                .text(
                    fmt::compact(c.usage.total_tokens),
                    num(c.usage.total_tokens),
                )
                .text(
                    if c.usage.cost_available {
                        fmt::usd(c.usage.total_cost_usd)
                    } else {
                        "—".into()
                    },
                    num(c.usage.total_cost_usd),
                )
                .text(
                    median.map(fmt::compact).unwrap_or("-".into()),
                    num(median.unwrap_or(-1.0)),
                )
                .filter(c.status.clone())
        })
        .collect();
    table_in(
        f,
        app,
        hit,
        PaneId::QuHistory,
        inner,
        focused,
        &HIST_COLS,
        rows,
        "no quota cycles in the last 30 days",
    );
}

const PRICE_COLS: [Col; 8] = [
    flex("model", 14),
    col("style", 7, Align::Left, 3),
    col("input", 8, Align::Right, 0),
    col("output", 8, Align::Right, 0),
    col("cache r", 8, Align::Right, 1),
    col("cache w", 8, Align::Right, 1),
    col("mult", 5, Align::Right, 2),
    col("rules", 9, Align::Right, 4),
];

fn price(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("${s}")
}

pub fn pricing(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let l = &app.data.pricing;
    let mut right = stale_marker(app, l);
    if let Some(p) = l.get() {
        right.push(Span::styled(
            format!(
                "{} priced · USD per 1M tokens · rev {}",
                p.models.len(),
                p.config_revision
            ),
            Style::default().fg(t.graph_text),
        ));
    }
    let inner = frame(f, app, PaneId::MoPricing, area, focused, right, None);
    if placeholder(f, app, inner, l) {
        return;
    }
    let Some(p) = l.get() else { return };
    let rows = p
        .models
        .iter()
        .map(|m| {
            let b = &m.base_prices;
            TRow::new(m.model.clone())
                .text(m.model.clone(), text(&m.model))
                .text(m.pricing_style.clone(), text(&m.pricing_style))
                .styled(price(b.input), Style::default().fg(t.title), num(b.input))
                .styled(price(b.output), Style::default().fg(t.title), num(b.output))
                .text(price(b.cache_read), num(b.cache_read))
                .text(price(b.cache_write), num(b.cache_write))
                .text(format!("×{}", m.model_multiplier), num(m.model_multiplier))
                .text(
                    format!(
                        "{}b · {}c",
                        m.branches.len(),
                        m.conditional_multipliers.len()
                    ),
                    num((m.branches.len() + m.conditional_multipliers.len()) as f64),
                )
                .filter(m.model.clone())
        })
        .collect();
    table_in(
        f,
        app,
        hit,
        PaneId::MoPricing,
        inner,
        focused,
        &PRICE_COLS,
        rows,
        "no model prices configured",
    );
}

/// Provider label derived from a model id, like the web catalog.
pub fn provider_of(id: &str, owned_by: Option<&str>) -> String {
    let last = id.rsplit('/').next().unwrap_or(id).to_lowercase();
    let pairs: [(&[&str], &str); 12] = [
        (&["gpt", "chatgpt", "codex", "o1", "o3", "o4"], "OpenAI"),
        (&["claude"], "Claude"),
        (&["gemini", "gemma"], "Gemini"),
        (&["grok"], "xAI"),
        (&["kimi", "moonshot"], "Kimi"),
        (&["llama"], "Meta"),
        (&["deepseek"], "DeepSeek"),
        (&["muse"], "Muse"),
        (&["qwen"], "Qwen"),
        (
            &["mistral", "codestral", "magistral", "devstral", "ministral"],
            "Mistral",
        ),
        (&["minimax"], "MiniMax"),
        (&["glm"], "Z.ai"),
    ];
    for (prefixes, name) in pairs {
        if prefixes.iter().any(|p| last.starts_with(p)) {
            return name.into();
        }
    }
    match owned_by.unwrap_or("") {
        "anthropic" => "Claude".into(),
        "google" => "Gemini".into(),
        "x-ai" => "xAI".into(),
        "" => "other".into(),
        o => o.to_string(),
    }
}

const CAT_COLS: [Col; 3] = [
    flex("model", 14),
    col("provider", 9, Align::Left, 0),
    col("owner", 10, Align::Left, 1),
];

pub fn catalog(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let l = &app.data.catalog;
    let mut right = stale_marker(app, l);
    if let Some(c) = l.get() {
        right.push(Span::styled(
            format!("{} models", c.data.len()),
            Style::default().fg(t.graph_text),
        ));
    }
    let inner = frame(f, app, PaneId::MoCatalog, area, focused, right, None);
    if placeholder(f, app, inner, l) {
        return;
    }
    let Some(c) = l.get() else { return };
    let mut models = c.data.clone();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    let rows = models
        .iter()
        .map(|m| {
            let p = provider_of(&m.id, m.owned_by.as_deref());
            let owner = m.owned_by.clone().unwrap_or_default();
            TRow::new(m.id.clone())
                .text(m.id.clone(), text(&m.id))
                .styled(p.clone(), Style::default().fg(t.accent), text(&p))
                .text(owner.clone(), text(&owner))
                .filter(format!("{} {p} {owner}", m.id))
        })
        .collect();
    table_in(
        f,
        app,
        hit,
        PaneId::MoCatalog,
        inner,
        focused,
        &CAT_COLS,
        rows,
        "catalog is empty",
    );
}

pub fn recalc(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    let t = app.theme();
    let inner = frame(f, app, PaneId::MoRecalc, area, focused, vec![], None);
    if placeholder(f, app, inner, &app.data.recalc) {
        return;
    }
    let k = |s: &str| Span::styled(format!("{s:<10}"), Style::default().fg(t.graph_text));
    let v = |s: String| Span::styled(s, Style::default().fg(t.title));
    let mut lines = vec![];
    match app.data.recalc.get().and_then(|o| o.as_ref()) {
        Some(task) => {
            let color = match task.status.as_str() {
                "running" => t.warn,
                "failed" => t.err,
                _ => t.ok,
            };
            lines.push(Line::from(vec![
                k("last task"),
                Span::styled(
                    task.status.clone(),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" · {}", task.stage),
                    Style::default().fg(t.graph_text),
                ),
            ]));
            let total = task.total_count.unwrap_or(0.0);
            let mut prog = vec![k("progress")];
            prog.extend(meter(
                if total > 0.0 {
                    task.processed_count / total
                } else {
                    0.0
                },
                10,
                t,
                None,
            ));
            prog.push(v(format!(
                " {}/{}",
                fmt::int(task.processed_count),
                fmt::int(total)
            )));
            lines.push(Line::from(prog));
            lines.push(Line::from(vec![
                k("from"),
                v(fmt::ts_fmt(&task.start_at, "%Y-%m-%d %H:%M")),
            ]));
            lines.push(Line::from(vec![
                k("updated"),
                v(fmt::ts_fmt(&task.updated_at, "%Y-%m-%d %H:%M")),
            ]));
            if let Some(e) = &task.error {
                lines.push(Line::from(Span::styled(
                    format!("✗ {}", e.message),
                    Style::default().fg(t.err),
                )));
            }
        }
        None => lines.push(Line::from(vec![
            k("last task"),
            v("none since server start".into()),
        ])),
    }
    if let Some(o) = app.data.recalc_options.get() {
        lines.push(Line::from(vec![
            k("revision"),
            v(o.config_revision.to_string()),
        ]));
        if let Some(e) = &o.earliest_start {
            lines.push(Line::from(vec![
                k("window"),
                v(format!(
                    "{} → {}",
                    fmt::ts_fmt(e, "%m-%d %H:%M"),
                    o.latest_start
                        .as_deref()
                        .map(|s| fmt::ts_fmt(s, "%m-%d %H:%M"))
                        .unwrap_or_default()
                )),
            ]));
        }
        lines.push(Line::from(vec![k("timezone"), v(o.timezone.clone())]));
    }
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

const KEY_COLS: [Col; 7] = [
    col("id", 4, Align::Right, 3),
    flex("name", 12),
    col("key", 10, Align::Left, 0),
    col("type", 7, Align::Left, 2),
    col("tokens", 7, Align::Right, 1),
    col("req", 7, Align::Right, 1),
    col("synced", 7, Align::Right, 4),
];

pub fn api_keys(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let l = &app.data.api_keys;
    let mut right = stale_marker(app, l);
    if let Some(k) = l.get() {
        right.push(Span::styled(
            format!("{} active", k.items.len()),
            Style::default().fg(t.graph_text),
        ));
    }
    let inner = frame(f, app, PaneId::StKeys, area, focused, right, None);
    if placeholder(f, app, inner, l) {
        return;
    }
    let Some(keys) = l.get() else { return };
    let usage = app.data.comparisons.get().map(|c| &c.api_keys);
    let now = app.clock.utc();
    let rows = keys
        .items
        .iter()
        .map(|k| {
            let masked = if k.source_type == "plugin" {
                k.display_key.clone()
            } else {
                key_label(&k.display_key)
            };
            let name = if k.key_alias.is_empty() {
                masked.clone()
            } else {
                k.key_alias.clone()
            };
            let u = usage.and_then(|u| u.iter().find(|c| c.key == k.id));
            let synced = k.last_synced_at.as_deref().and_then(|s| fmt::since(s, now));
            let kind = if k.source_type == "plugin" {
                format!("plugin {}", k.access_provider.trim_start_matches("plugin:"))
            } else {
                k.source_type.clone()
            };
            TRow::new(k.id.clone())
                .text(k.id.clone(), num(k.id.parse::<f64>().unwrap_or(0.0)))
                .styled(name.clone(), Style::default().fg(t.title), text(&name))
                .text(masked.clone(), text(&masked))
                .text(kind.clone(), text(&kind))
                .text(
                    u.map(|u| fmt::compact(u.total_tokens))
                        .unwrap_or("-".into()),
                    num(u.map(|u| u.total_tokens).unwrap_or(-1.0)),
                )
                .text(
                    u.map(|u| fmt::count(u.requests)).unwrap_or("-".into()),
                    num(u.map(|u| u.requests).unwrap_or(-1.0)),
                )
                .text(
                    synced.map(fmt::age).unwrap_or("-".into()),
                    num(synced.map(|s| s as f64).unwrap_or(f64::MAX)),
                )
                .filter(format!("{name} {kind}"))
        })
        .collect();
    table_in(
        f,
        app,
        hit,
        PaneId::StKeys,
        inner,
        focused,
        &KEY_COLS,
        rows,
        "no API keys",
    );
}

const SESSION_COLS: [Col; 8] = [
    col("kind", 7, Align::Left, 0),
    col("via", 8, Align::Left, 4),
    flex("name", 10),
    col("agent", 22, Align::Left, 5),
    col("ip", 15, Align::Left, 3),
    col("last seen", 11, Align::Left, 1),
    col("expires", 11, Align::Left, 2),
    col("", 3, Align::Left, 0),
];

pub fn sessions(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let l = &app.data.sessions;
    let mut right = stale_marker(app, l);
    if let Some(s) = l.get() {
        right.push(Span::styled(
            format!("{} sessions", s.items.len()),
            Style::default().fg(t.graph_text),
        ));
    }
    let inner = frame(f, app, PaneId::StSessions, area, focused, right, None);
    if placeholder(f, app, inner, l) {
        return;
    }
    let Some(s) = l.get() else { return };
    let short = |s: &Option<String>| {
        s.as_deref()
            .map(|v| v.get(5..16).unwrap_or(v).to_string())
            .unwrap_or("-".into())
    };
    let rows = s
        .items
        .iter()
        .enumerate()
        .map(|(n, it)| {
            let name = if it.kind == "admin" {
                it.alias
                    .clone()
                    .filter(|a| !a.is_empty())
                    .unwrap_or("admin".into())
            } else {
                it.label
                    .clone()
                    .filter(|l| !l.is_empty())
                    .map(|l| key_label(&l))
                    .or(it.display_key.as_deref().map(key_label))
                    .unwrap_or("-".into())
            };
            let via = match it.source.as_str() {
                "standard" => "keeper",
                "embed" => "cpamc",
                other => other,
            }
            .to_string();
            let ip = it
                .last_seen_ip
                .clone()
                .or(it.login_ip.clone())
                .unwrap_or("-".into());
            let ua = it.user_agent.clone().unwrap_or_default();
            TRow::new(format!("{n}"))
                .styled(
                    if it.kind == "admin" {
                        "admin"
                    } else {
                        "api key"
                    },
                    Style::default().fg(if it.kind == "admin" { t.hi } else { t.accent }),
                    text(&it.kind),
                )
                .text(via.clone(), text(&via))
                .text(name.clone(), text(&name))
                .text(ua.clone(), text(&ua))
                .text(ip.clone(), text(&ip))
                .text(
                    short(&it.last_seen_at.clone().or(it.login_at.clone())),
                    text(it.last_seen_at.as_deref().unwrap_or("")),
                )
                .text(
                    short(&it.expires_at),
                    text(it.expires_at.as_deref().unwrap_or("")),
                )
                .styled(
                    if it.current { "●" } else { "" },
                    Style::default().fg(t.ok),
                    num(if it.current { 1.0 } else { 0.0 }),
                )
                .filter(format!("{name} {ua} {ip} {via}"))
        })
        .collect();
    table_in(
        f,
        app,
        hit,
        PaneId::StSessions,
        inner,
        focused,
        &SESSION_COLS,
        rows,
        "no sessions",
    );
}

pub fn server(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    let t = app.theme();
    let inner = frame(f, app, PaneId::StServer, area, focused, vec![], None);
    let k = |s: &str| Span::styled(format!("{s:<12}"), Style::default().fg(t.graph_text));
    let v = |s: String| Span::styled(s, Style::default().fg(t.title));
    let yes = |b: bool| {
        Span::styled(
            if b { "yes" } else { "no" },
            Style::default().fg(if b { t.ok } else { t.graph_text }),
        )
    };
    let mut lines = vec![Line::from(vec![
        k("server"),
        v(app.settings.base_url.clone()),
    ])];
    if let Some(ver) = app.data.version.get() {
        lines.push(Line::from(vec![k("version"), v(ver.version.clone())]));
    }
    match app.data.status.get() {
        Some(s) => {
            lines.push(Line::from(vec![
                k("running"),
                yes(s.running),
                Span::raw("  "),
                k("syncing"),
                yes(s.sync_running),
            ]));
            lines.push(Line::from(vec![k("timezone"), v(s.timezone.clone())]));
            lines.push(Line::from(vec![
                k("request log"),
                yes(s.cpa_request_log_access_enabled),
            ]));
            if let Some(ls) = &s.last_status {
                lines.push(Line::from(vec![k("poller"), v(ls.clone())]));
            }
            if let Some(w) = s.last_warning.as_ref().filter(|w| !w.is_empty()) {
                lines.push(Line::from(Span::styled(
                    format!("⚠ {w}"),
                    Style::default().fg(t.warn),
                )));
            }
            if let Some(e) = s.last_error.as_ref().filter(|e| !e.is_empty()) {
                lines.push(Line::from(Span::styled(
                    format!("✗ {e}"),
                    Style::default().fg(t.err),
                )));
            }
        }
        None => {
            if let Some(e) = &app.data.status.error {
                lines.push(Line::from(Span::styled(
                    format!("✗ {e}"),
                    Style::default().fg(t.err),
                )));
            }
        }
    }
    lines.push(Line::from(vec![
        k("polling"),
        v(format!(
            "{}s / {}s",
            app.settings.refresh_secs, app.settings.slow_refresh_secs
        )),
    ]));
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

#[cfg(test)]
mod tests {
    use super::provider_of;

    #[test]
    fn providers() {
        assert_eq!(provider_of("gpt-6-luna", None), "OpenAI");
        assert_eq!(
            provider_of("vertex_ai/claude-3-5-haiku@20241022", None),
            "Claude"
        );
        assert_eq!(provider_of("bonsai", Some("google")), "Gemini");
        assert_eq!(provider_of("bonsai", None), "other");
    }
}
