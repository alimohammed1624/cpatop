//! Quota tab: inspection summary, quota windows per credential and history.

use super::credentials::{ident_name, identity_fields, plan};
use super::{
    RowChart, frame, placeholder, stale_marker, table_with_chart, time_labels, token_details, when,
    yes_no,
};
use crate::api::models::{Identity, InspectionResult, QuotaRefreshTask, QuotaRow};
use crate::app::{App, Hit};
use crate::fmt;
use crate::layout::PaneId;
use crate::theme::Theme;
use crate::ui::table::{Align, Col, TRow, col, flex, grow, num, text};
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

/// Every field of a quota row for the popup: the window, the cached check,
/// the inspection result and the credential, through the privacy helpers.
fn quota_fields(
    app: &App,
    i: &Identity,
    task: Option<&Result<QuotaRefreshTask, String>>,
    insp: Option<&InspectionResult>,
    r: Option<&QuotaRow>,
    status: &str,
) -> Vec<(&'static str, String)> {
    let opt = |o: &Option<String>| o.clone().unwrap_or_default();
    let mut v = vec![("credential", ident_name(app, i))];
    if let Some(r) = r {
        let used = r.used_pct().unwrap_or(0.0);
        let cents = r.metric.as_deref() == Some("usd_cents");
        let amount = |x: Option<f64>| match x {
            Some(x) if cents => format!("${:.2}", x / 100.0),
            Some(x) => fmt::int(x),
            None => String::new(),
        };
        v.extend([
            ("window", app.privacy(&r.display_label())),
            ("quota left", format!("{:.1}%", 100.0 - used)),
            ("used", format!("{used:.1}%")),
            (
                "used / limit",
                match (r.used, r.limit) {
                    (Some(_), Some(_)) => format!("{} / {}", amount(r.used), amount(r.limit)),
                    _ => String::new(),
                },
            ),
            ("remaining", amount(r.remaining)),
            ("metric", app.privacy(&opt(&r.metric))),
            ("resets in", when(app, r.reset_at.as_deref())),
            (
                "window length",
                r.window
                    .as_ref()
                    .and_then(|w| w.seconds)
                    .map(fmt::secs)
                    .unwrap_or_default(),
            ),
            (
                "tokens",
                r.window_usage_tokens.map(fmt::int).unwrap_or_default(),
            ),
            (
                "cost",
                if cents {
                    String::new()
                } else {
                    r.window_usage_cost
                        .map(|c| format!("${c:.4}"))
                        .unwrap_or_default()
                },
            ),
            (
                "limit reached",
                r.limit_reached.map(yes_no).unwrap_or_default(),
            ),
            ("allowed", r.allowed.map(yes_no).unwrap_or_default()),
            ("group", app.privacy(&opt(&r.group_label))),
            (
                "breakdown",
                r.usage_breakdown
                    .iter()
                    .map(|b| {
                        format!(
                            "{} {}",
                            app.privacy(&b.product),
                            b.used_percent
                                .map(|p| format!("{p:.0}%"))
                                .unwrap_or_default()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" · "),
            ),
        ]);
    }
    v.push(("status", status.to_string()));
    match task {
        Some(Ok(task)) => {
            v.extend([
                ("quota check", app.privacy(&task.status)),
                (
                    "check HTTP status",
                    task.http_status_code
                        .map(|c| c.to_string())
                        .unwrap_or_default(),
                ),
                ("check error", app.privacy_text(&opt(&task.error))),
                ("checked", when(app, task.refreshed_at.as_deref())),
                ("cached until", when(app, task.expires_at.as_deref())),
            ]);
            if let Some(q) = &task.quota {
                if let Some(n) = q.reset_credits_available {
                    v.push(("reset credits", fmt::int(n)));
                }
                if let Some(g) = &q.claude_reset_grants {
                    v.extend([
                        ("reset grants", fmt::int(g.available_count)),
                        ("grant eligible", yes_no(g.eligible)),
                        (
                            "not eligible because",
                            app.privacy_text(&opt(&g.ineligible_reason)),
                        ),
                        ("grants reset", when(app, g.weekly_resets_at.as_deref())),
                        ("grant cooldown", when(app, g.cooldown_until.as_deref())),
                    ]);
                }
            }
        }
        Some(Err(e)) => v.push(("check error", app.privacy_text(e))),
        None => v.push(("quota check", "not cached".into())),
    }
    if let Some(r) = insp {
        v.extend([
            ("inspection", app.privacy(&r.status.replace('_', " "))),
            (
                "inspection HTTP status",
                r.http_status_code
                    .map(|c| c.to_string())
                    .unwrap_or_default(),
            ),
            ("inspection error", app.privacy_text(&opt(&r.error))),
            ("inspected", when(app, r.refreshed_at.as_deref())),
        ]);
    }
    // The credential itself (its name and plan are already listed).
    v.extend(
        identity_fields(app, i)
            .into_iter()
            .filter(|(k, _)| !matches!(*k, "name" | "plan")),
    );
    v
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
        let (name, short) = app.ident_labels(i);
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
        let plan_s = quota_plan(app, i)
            .or_else(|| known_plan(plan(i)))
            .unwrap_or_else(|| "—".to_string());
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
                    .name(name.clone(), short.clone(), Style::default(), text(&name))
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
                    .filter(filter.clone())
                    .detail("plan", plan_s.clone())
                    .detail("window", "—")
                    .detail("quota left", "—")
                    .details(quota_fields(app, i, task, insp, None, &st_text)),
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
            let (cell_name, cell_short, cell_plan) = if first {
                (name.clone(), short.clone(), plan_s.clone())
            } else {
                (String::new(), String::new(), String::new())
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
                    .name(
                        cell_name,
                        cell_short,
                        Style::default().fg(t.title),
                        text(&name),
                    )
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
                    .filter(filter.clone())
                    .detail("plan", plan_s.clone())
                    .details(quota_fields(app, i, task, insp, Some(r), &st_text)),
            );
        }
    }
    // Spare rows chart the selected credential's recent requests (the
    // server's health window), so a quota can be read against its use; a
    // credential without requests in that window shows all credentials
    // instead, titled as such.
    let chart_for = |id: &str| -> Option<RowChart> {
        let ids = &app.data.identities.get()?.identities;
        let i = ids.iter().find(|i| i.identity == id)?;
        let h = i
            .credential_health
            .as_ref()
            .filter(|h| !h.buckets.is_empty())?;
        let starts: Vec<String> = h.buckets.iter().map(|b| b.start_time.clone()).collect();
        let window = fmt::secs(h.window_seconds).replace(" 00m", "");
        let own: Vec<f64> = h.buckets.iter().map(|b| b.success + b.failure).collect();
        if own.iter().sum::<f64>() > 0.0 {
            return Some(RowChart {
                title: format!("{} · requests, last {window}", ident_name(app, i)),
                labels: time_labels(&starts, false, false),
                values: own,
                fmt: fmt::count,
                color: None,
                place: None,
            });
        }
        let all = all_requests(app, &starts)?;
        Some(RowChart {
            title: format!(
                "no requests for {} in {window} — showing all credentials",
                app.ident_labels(i).1
            ),
            labels: time_labels(&starts, false, false),
            values: all,
            fmt: fmt::count,
            color: None,
            place: None,
        })
    };
    table_with_chart(
        f,
        app,
        hit,
        PaneId::QuList,
        inner,
        focused,
        &QUOTA_COLS,
        rows,
        "no credentials with quota",
        &chart_for,
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

/// Every field of a quota cycle for the row popup.
fn cycle_fields(
    app: &App,
    c: &crate::api::models::QuotaCycle,
    median: Option<f64>,
) -> Vec<(&'static str, String)> {
    let u = &c.usage;
    let pct = |p: Option<f64>| p.map(|p| format!("{p:.1}%")).unwrap_or_default();
    let mut v = vec![
        ("cycle", app.privacy(&c.status)),
        ("window length", fmt::secs(c.window_seconds)),
        ("started", when(app, Some(&c.window_started_at))),
        ("resets", when(app, Some(&c.reset_at))),
        ("effective start", when(app, Some(&c.effective_started_at))),
        ("effective end", when(app, Some(&c.effective_ended_at))),
        ("first observed", when(app, Some(&c.first_observed_at))),
        ("last observed", when(app, Some(&c.last_observed_at))),
        ("left at start", pct(c.first_remaining_percent)),
        ("left at end", pct(c.last_remaining_percent)),
        ("observations", fmt::int(c.observation_count)),
        ("drops observed", fmt::int(c.transitions.len() as f64)),
        (
            "tokens per 1% of quota",
            median.map(fmt::int).unwrap_or_default(),
        ),
        ("requests", fmt::int(u.requests)),
        ("successes", fmt::int(u.successful_requests)),
        ("failures", fmt::int(u.failed_requests)),
    ];
    v.extend(token_details(
        u.input_tokens,
        u.output_tokens,
        u.cache_read_tokens,
        u.cache_creation_tokens,
        u.reasoning_tokens,
        u.total_tokens,
    ));
    v.push((
        "cost",
        if u.cost_available {
            format!("${:.4}", u.total_cost_usd)
        } else {
            String::new()
        },
    ));
    v
}

/// A plan name, unless empty or `unknown`.
pub(super) fn known_plan(p: String) -> Option<String> {
    (!p.is_empty() && p != "unknown").then_some(p)
}

/// The plan the credential's cached quota reports (`pro`, `team`…), if any.
pub(super) fn quota_plan(app: &App, i: &Identity) -> Option<String> {
    match app.data.quota.get()?.get(&i.identity)? {
        Ok(task) => known_plan(task.quota.as_ref()?.subscription.as_ref()?.plan.clone()),
        Err(_) => None,
    }
}

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

/// One-line summary of the quota table when it is collapsed: how many
/// credentials and the window with the least quota left.
pub fn list_summary(app: &App) -> Option<String> {
    let tasks = app.data.quota.get()?;
    let ids = app.data.identities.get()?;
    let creds: Vec<&Identity> = ids
        .identities
        .iter()
        .filter(|i| i.auth_type == 1 && !i.is_deleted)
        .collect();
    let n = fmt::plural(creds.len(), "credential", "credentials");
    let lowest = creds
        .iter()
        .filter_map(|i| match tasks.get(&i.identity) {
            Some(Ok(t)) => Some((i, t.quota.as_ref()?)),
            _ => None,
        })
        .flat_map(|(i, q)| {
            q.quota
                .iter()
                .filter_map(move |r| Some((i, r, 100.0 - r.used_pct()?)))
        })
        .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
    Some(match lowest {
        Some((i, r, left)) => format!(
            "{n} · least left {} {} {}",
            ident_name(app, i),
            app.privacy(&r.display_label()),
            fmt::pct(left, 0)
        ),
        None => n,
    })
}

/// One-line summary of the history pane when it is collapsed: the
/// selected credential's windows, or why it has none.
pub fn history_summary(app: &App, hit: &Hit) -> Option<String> {
    let sel = hit.selected.get(&PaneId::QuList)?;
    let i = app
        .data
        .identities
        .get()?
        .identities
        .iter()
        .find(|i| &i.identity == sel)?;
    if i.kind != "codex" && i.kind != "claude" {
        return Some("codex and claude credentials only".into());
    }
    let (idx, l) = app.data.quota_history.as_ref()?;
    let n = l.get().filter(|_| idx == sel)?.cycles.len();
    Some(format!(
        "{} · {}",
        fmt::plural(n, "window", "windows"),
        ident_name(app, i)
    ))
}

/// One-line summary of the inspection when it is collapsed.
pub fn inspection_summary(app: &App) -> Option<String> {
    let q = app.data.inspection.get()?;
    let bad = q.unauthorized_401 + q.payment_required_402 + q.other_failed;
    Some(format!(
        "{} accounts · {} normal · {} limited · {} failed",
        fmt::int(q.total),
        fmt::int(q.normal),
        fmt::int(q.limit_reached),
        fmt::int(bad)
    ))
}

/// Requests per health bucket summed over every quota credential, on the
/// given bucket starts; `None` when there were none.
fn all_requests(app: &App, starts: &[String]) -> Option<Vec<f64>> {
    let mut v = vec![0.0; starts.len()];
    for i in &app.data.identities.get()?.identities {
        let Some(h) = &i.credential_health else {
            continue;
        };
        for b in &h.buckets {
            if let Some(k) = starts.iter().position(|s| *s == b.start_time) {
                v[k] += b.success + b.failure;
            }
        }
    }
    (v.iter().sum::<f64>() > 0.0).then_some(v)
}

/// Whether the quota table has a requests chart to put spare rows to.
pub fn list_has_chart(app: &App) -> bool {
    app.data.identities.get().is_some_and(|p| {
        p.identities.iter().any(|i| {
            i.credential_health
                .as_ref()
                .is_some_and(|h| h.buckets.iter().any(|b| b.success + b.failure > 0.0))
        })
    })
}

/// Height of the history pane: one message line when there is nothing to
/// list, the table plus (with three or more windows) a chart otherwise.
pub fn history_size(app: &App) -> crate::layout::Size {
    use crate::layout::Size;
    match history_len(app) {
        Some(n) if n > 0 => {
            let s = Size::range(5, n as u16 + 3);
            if n >= 3 { s.soaking() } else { s }
        }
        _ => Size::exact(3),
    }
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
                .details(cycle_fields(app, c, median))
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
        place: None,
    };
    // One or two windows make a meaningless chart.
    let chart_for = |_: &str| -> Option<RowChart> {
        (per_window.values.len() >= 3).then(|| RowChart {
            title: per_window.title.clone(),
            labels: per_window.labels.clone(),
            values: per_window.values.clone(),
            fmt: per_window.fmt,
            color: None,
            place: None,
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
