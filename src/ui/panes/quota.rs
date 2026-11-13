//! Quota tab: inspection summary, quota windows per credential and history.

use super::credentials::{ident_name, plan};
use super::{RowChart, frame, placeholder, stale_marker, table_in, table_with_chart};
use crate::api::models::{Identity, QuotaRow};
use crate::app::{App, Hit};
use crate::fmt;
use crate::layout::PaneId;
use crate::ui::table::{Align, Col, TRow, col, flex, grow, num, text};
use crate::theme::Theme;
use crate::ui::widgets::{self, flow, meter, stat};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::Paragraph;

fn inspection_items(app: &App) -> Vec<Vec<Span<'static>>> {
    let t = app.theme();
    let Some(q) = app.data.inspection.get() else {
        return vec![];
    };
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
    items
}

/// Lines the inspection summary needs at an inner width.
pub fn inspection_lines(app: &App, inner_w: u16) -> u16 {
    flow(inspection_items(app), inner_w, 3).len() as u16
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
    let w = inner.width.saturating_sub(1);
    let lines = flow(inspection_items(app), w, 3);
    f.render_widget(
        Paragraph::new(lines),
        Rect::new(inner.x + 1, inner.y, w, inner.height),
    );
}

// Plan and status stay at every width (status shrinks to a glyph), so a
// reached limit never disappears.
const QUOTA_COLS: [Col; 8] = [
    flex("credential", 14),
    col("plan", 4, Align::Left, 0),
    col("window", 10, Align::Left, 0),
    grow("left", 12, 0),
    col("resets in", 8, Align::Right, 1),
    col("tokens", 7, Align::Right, 3),
    col("cost", 8, Align::Right, 4),
    col("status", 6, Align::Left, 0),
];

/// Status cell: the words, or a glyph when the column is narrow.
fn status_cell(app: &App, status: Option<&str>) -> Vec<Vec<Span<'static>>> {
    let t = app.theme();
    let (glyph, short, color) = match status {
        Some("normal") => ("✓", "ok", t.ok),
        Some("limit_reached") => ("⚠", "limit", t.warn),
        Some(s) if s.contains("401") => ("✗", "401", t.err),
        Some(s) if s.contains("402") => ("✗", "402", t.err),
        Some(_) => ("✗", "failed", t.err),
        None => return vec![vec![]],
    };
    let words = status.unwrap_or_default().replace('_', " ");
    let st = Style::default().fg(color);
    vec![
        vec![Span::styled(format!("{glyph} {words}"), st)],
        vec![Span::styled(format!("{glyph} {short}"), st)],
        vec![Span::styled(glyph.to_string(), st)],
    ]
}

/// A window with less than this percentage left counts as exhausted.
const EXHAUSTED_LEFT: f64 = 0.5;

/// Bar colour by percentage left: plenty, low (< 25%), about none (< 5%).
fn left_color(t: &Theme, left: f64) -> Color {
    t.good_pct(left, 25.0, 5.0)
}

/// A credential's quota status, derived from the same windows as the bars.
#[derive(Debug, Clone, PartialEq)]
enum WindowStatus {
    /// Windows (indices into the rows) with about nothing left.
    Exhausted(Vec<usize>),
    /// Every window has quota left.
    Ok,
    /// Every window has quota left but the server's cached result flags a
    /// reached limit; seconds since that result, when known.
    Flagged(Option<u64>),
}

fn window_status(rows: &[&QuotaRow], flagged: bool, age: Option<u64>) -> WindowStatus {
    let out: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| 100.0 - r.used_pct().unwrap_or(0.0) < EXHAUSTED_LEFT)
        .map(|(i, _)| i)
        .collect();
    if !out.is_empty() {
        WindowStatus::Exhausted(out)
    } else if flagged || rows.iter().any(|r| r.limit_reached == Some(true)) {
        WindowStatus::Flagged(age)
    } else {
        WindowStatus::Ok
    }
}

/// Status cell for a cached limit flag the windows do not confirm.
fn flag_cell(app: &App, age: Option<u64>) -> Vec<Vec<Span<'static>>> {
    let st = Style::default().fg(app.theme().graph_text);
    let (long, short) = match age {
        Some(a) => (
            format!("◌ limit flag (cached {} ago)", fmt::age(a)),
            format!("◌ flag {}", fmt::age(a)),
        ),
        None => ("◌ limit flag (cached)".into(), "◌ flag".into()),
    };
    vec![
        vec![Span::styled(long, st)],
        vec![Span::styled(short, st)],
        vec![Span::styled("◌", st)],
    ]
}

pub fn list(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
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
        let st_cell = status_cell(app, insp.map(|r| r.status.as_str()));
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
        let plan_s = if plan_s.is_empty() || plan_s == "unknown" {
            "—".to_string()
        } else {
            plan_s
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
                    .choice(st_cell.clone(), text(&st_text))
                    .filter(filter.clone()),
            );
            continue;
        }
        // The status follows the windows: a limit is reached only where a
        // window has about nothing left, and the ⚠ sits on that window. A
        // server flag the windows do not confirm is shown as cached.
        let insp_flag = insp.is_some_and(|r| r.status == "limit_reached");
        let age = insp
            .and_then(|r| r.refreshed_at.as_deref())
            .or(match task {
                Some(Ok(task)) => task.refreshed_at.as_deref(),
                _ => None,
            })
            .and_then(|s| fmt::since(s, now));
        let wst = window_status(&quota_rows, insp_flag, age);
        let insp_failed = insp.is_some_and(|r| r.status != "normal" && r.status != "limit_reached");
        let (st_text, first_cell) = match &wst {
            WindowStatus::Exhausted(_) => ("limit reached".to_string(), vec![vec![]]),
            _ if insp_failed => (st_text.clone(), st_cell.clone()),
            WindowStatus::Ok => ("normal".to_string(), status_cell(app, Some("normal"))),
            WindowStatus::Flagged(a) => ("limit flag".to_string(), flag_cell(app, *a)),
        };
        for (k, r) in quota_rows.iter().enumerate() {
            let used = r.used_pct().unwrap_or(0.0);
            let left = 100.0 - used;
            let color = left_color(t, left);
            let cell = match &wst {
                WindowStatus::Exhausted(ix) if ix.contains(&k) => {
                    status_cell(app, Some("limit_reached"))
                }
                WindowStatus::Exhausted(_) => vec![vec![]],
                _ if k == 0 => first_cell.clone(),
                _ => vec![vec![]],
            };
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
                    .meter(
                        left / 100.0,
                        Some(color),
                        format!(" {left:>3.0}%"),
                        num(left),
                    )
                    .text(
                        reset.map(|s| fmt::secs(s as f64)).unwrap_or("-".into()),
                        num(reset.unwrap_or(0) as f64),
                    )
                    .text(tok, num(r.window_usage_tokens.unwrap_or(0.0)))
                    .text(cost, num(r.window_usage_cost.unwrap_or(0.0)))
                    .choice(cell, text(&st_text))
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

/// Rows of the quota table: one per window, at least one per credential.
pub fn list_len(app: &App) -> Option<usize> {
    let tasks = app.data.quota.get()?;
    let ids = app.data.identities.get()?;
    Some(
        ids.identities
            .iter()
            .filter(|i| i.auth_type == 1 && !i.is_deleted)
            .map(|i| match tasks.get(&i.identity) {
                Some(Ok(t)) => t
                    .quota
                    .as_ref()
                    .map(|q| q.quota.iter().filter(|r| r.used_pct().is_some()).count())
                    .unwrap_or(0)
                    .max(1),
                _ => 1,
            })
            .sum(),
    )
}

pub fn history_len(app: &App) -> Option<usize> {
    app.data
        .quota_history
        .as_ref()
        .and_then(|(_, l)| l.get())
        .map(|h| h.cycles.len())
}

pub fn history(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
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
    // Spare rows chart the tokens each window allowed, oldest first.
    let mut cycles: Vec<_> = h.cycles.iter().collect();
    cycles.sort_by(|a, b| a.window_started_at.cmp(&b.window_started_at));
    let per_window = RowChart {
        title: format!("tokens per {kind} window"),
        labels: cycles
            .iter()
            .map(|c| fmt::ts_fmt(&c.window_started_at, "%m-%d %H:%M"))
            .collect(),
        values: cycles.iter().map(|c| c.usage.total_tokens).collect(),
        fmt: fmt::compact,
        color: None,
    };
    // One or two windows make a meaningless chart.
    let chart_for = |_: &str| -> Option<RowChart> {
        (per_window.values.len() >= 3).then(|| RowChart {
            title: per_window.title.clone(),
            labels: per_window.labels.clone(),
            values: per_window.values.clone(),
            fmt: per_window.fmt,
            color: None,
        })
    };
    table_with_chart(
        f,
        app,
        hit,
        PaneId::QuHistory,
        inner,
        focused,
        &HIST_COLS,
        rows,
        "no quota cycles in the last 30 days",
        &chart_for,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(used: f64, flag: Option<bool>) -> QuotaRow {
        QuotaRow {
            used_percent: Some(used),
            limit_reached: flag,
            ..Default::default()
        }
    }

    #[test]
    fn status_follows_the_windows() {
        // 5h 93% left, weekly 77% left, but the account-level flag is set
        // (codex copies it onto every window): not "limit reached".
        let (a, b) = (row(7.0, Some(true)), row(23.0, Some(true)));
        assert_eq!(
            window_status(&[&a, &b], true, Some(3 * 3600)),
            WindowStatus::Flagged(Some(3 * 3600))
        );
        // The weekly window is used up: the ⚠ goes on that window.
        let (a, b) = (row(7.0, None), row(100.0, None));
        assert_eq!(
            window_status(&[&a, &b], false, None),
            WindowStatus::Exhausted(vec![1])
        );
        let (a, b) = (row(7.0, Some(false)), row(23.0, None));
        assert_eq!(window_status(&[&a, &b], false, None), WindowStatus::Ok);
    }

    #[test]
    fn bars_are_coloured_by_what_is_left() {
        let t = &crate::theme::all_themes(None).0[0];
        assert_eq!(left_color(t, 78.0), t.ok);
        assert_eq!(left_color(t, 30.0), t.ok);
        assert_eq!(left_color(t, 12.0), t.warn);
        assert_eq!(left_color(t, 0.0), t.err);
    }
}
