//! Keys tab: API keys with range-scoped usage, sessions and server info.

use super::{frame, placeholder, stale_marker, table_in};
use crate::app::{App, Hit};
use crate::fmt;
use crate::layout::PaneId;
use crate::ui::table::{Align, Col, TRow, col, flex, grow, num, text};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

const KEY_COLS: [Col; 9] = [
    col("id", 4, Align::Right, 0),
    flex("name", 12),
    col("key", 7, Align::Left, 0),
    col("type", 7, Align::Left, 2),
    col("tokens", 7, Align::Right, 1),
    grow("share", 10, 5),
    col("req", 7, Align::Right, 1),
    col("cost", 8, Align::Right, 3),
    col("synced", 7, Align::Right, 4),
];

pub fn api_keys(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let l = &app.data.api_keys;
    let u = &app.data.key_usage;
    let mut right = stale_marker(app, l);
    right.extend(stale_marker(app, u));
    if let Some(k) = l.get() {
        // Usage is fetched by this tab for the selected range.
        let fresh = match (u.at, &u.data) {
            (Some(at), Some(_)) => format!(
                "usage {} · {} ago",
                app.range.label(),
                fmt::age(app.now().saturating_duration_since(at).as_secs())
            ),
            _ if u.error.is_none() => format!("usage {} · loading…", app.range.label()),
            _ => String::new(),
        };
        right.push(Span::styled(
            format!("{} active · {fresh}", k.items.len()),
            Style::default().fg(t.graph_text),
        ));
    }
    let inner = frame(f, app, PaneId::StKeys, area, focused, right, None);
    if placeholder(f, app, inner, l) {
        return;
    }
    let Some(keys) = l.get() else { return };
    let usage = u.get().map(|c| &c.api_keys);
    let total: f64 = usage
        .map(|u| u.iter().map(|c| c.total_tokens).sum())
        .unwrap_or(0.0);
    let now = app.clock.utc();
    let rows = keys
        .items
        .iter()
        .map(|k| {
            let masked = if k.source_type == "plugin" || app.settings.privacy {
                "—".to_string()
            } else {
                fmt::redact_key(&k.display_key)
            };
            let name = if k.key_alias.is_empty()
                || (app.settings.privacy && crate::app::is_identifier(&k.key_alias))
            {
                "—".to_string()
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
            let tok = u.map(|u| u.total_tokens).unwrap_or(0.0);
            let share = fmt::ratio_pct(tok, total).unwrap_or(0.0);
            TRow::new(k.id.clone())
                .styled(
                    format!("#{}", k.id),
                    Style::default().fg(t.hi),
                    num(k.id.parse::<f64>().unwrap_or(0.0)),
                )
                .styled(name.clone(), Style::default().fg(t.title), text(&name))
                .text(masked.clone(), text(&masked))
                .text(kind.clone(), text(&kind))
                .text(
                    u.map(|u| fmt::compact(u.total_tokens))
                        .unwrap_or("-".into()),
                    num(u.map(|u| u.total_tokens).unwrap_or(-1.0)),
                )
                .meter(share / 100.0, None, format!(" {:>5.1}%", share), num(share))
                .text(
                    u.map(|u| fmt::count(u.requests)).unwrap_or("-".into()),
                    num(u.map(|u| u.requests).unwrap_or(-1.0)),
                )
                .text(
                    u.and_then(|u| u.cost).map(fmt::usd).unwrap_or("-".into()),
                    num(u.and_then(|u| u.cost).unwrap_or(-1.0)),
                )
                .text(
                    synced.map(fmt::age).unwrap_or("-".into()),
                    num(synced.map(|s| s as f64).unwrap_or(f64::MAX)),
                )
                .filter(format!("#{} {name} {masked} {kind}", k.id))
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
                    .map(|a| app.privacy(&a))
                    .unwrap_or("admin".into())
            } else {
                let label = it
                    .label
                    .clone()
                    .filter(|l| !l.is_empty())
                    .or(it.display_key.clone())
                    .unwrap_or_default();
                app.key_name(&it.api_key_id, &label)
            };
            let via = match it.source.as_str() {
                "standard" => "keeper",
                "embed" => "cpamc",
                other => other,
            }
            .to_string();
            let ip = app.ip(&it
                .last_seen_ip
                .clone()
                .or(it.login_ip.clone())
                .unwrap_or("-".into()));
            let ua = app.privacy_text(&it.user_agent.clone().unwrap_or_default());
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

fn server_text(app: &App) -> Vec<Line<'static>> {
    let t = app.theme();
    let k = |s: &str| Span::styled(format!("{s:<12}"), Style::default().fg(t.graph_text));
    let v = |s: String| Span::styled(s, Style::default().fg(t.title));
    let yes = |b: bool| {
        Span::styled(
            if b { "yes" } else { "no" },
            Style::default().fg(if b { t.ok } else { t.graph_text }),
        )
    };
    // The host (and sub-path) is enough here; the scheme only adds width.
    let mut lines = vec![Line::from(vec![k("server"), v(app.host.clone())])];
    if let Some(ver) = app.data.version.get() {
        lines.push(Line::from(vec![k("version"), v(ver.version.clone())]));
    }
    match app.data.status.get() {
        Some(s) => {
            lines.push(Line::from(vec![
                k("running"),
                yes(s.running),
                Span::raw("  "),
                Span::styled("syncing ", Style::default().fg(t.graph_text)),
                yes(s.sync_running),
            ]));
            lines.push(Line::from(vec![k("timezone"), v(s.timezone.clone())]));
            lines.push(Line::from(vec![
                k("request log"),
                yes(s.cpa_request_log_access_enabled),
            ]));
            if let Some(ls) = &s.last_status {
                lines.push(Line::from(vec![k("poller"), v(app.privacy_text(ls))]));
            }
            if let Some(w) = s.last_warning.as_ref().filter(|w| !w.is_empty()) {
                lines.push(Line::from(Span::styled(
                    format!("⚠ {}", app.privacy_text(w)),
                    Style::default().fg(t.warn),
                )));
            }
            if let Some(e) = s.last_error.as_ref().filter(|e| !e.is_empty()) {
                lines.push(Line::from(Span::styled(
                    format!("✗ {}", app.privacy_text(e)),
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
    lines
}

/// Rows the server pane needs at an inner width (long lines wrap).
pub fn server_lines(app: &App, width: u16) -> u16 {
    let w = width.max(1) as usize;
    server_text(app)
        .iter()
        .map(|l| l.width().div_ceil(w).max(1) as u16)
        .sum()
}

pub fn server(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    let right = vec![Span::styled(
        format!("cpatop {}", env!("CARGO_PKG_VERSION")),
        Style::default().fg(app.theme().graph_text),
    )];
    let inner = frame(f, app, PaneId::StServer, area, focused, right, None);
    f.render_widget(
        Paragraph::new(server_text(app)).wrap(Wrap { trim: true }),
        Rect::new(
            inner.x + 1,
            inner.y,
            inner.width.saturating_sub(1),
            inner.height,
        ),
    );
}
