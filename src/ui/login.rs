//! Login, connecting, startup and offline screens.

use crate::api::LoginMode;
use crate::app::{App, Screen};
use crate::fmt;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget, Wrap};

pub fn draw(f: &mut Frame, app: &App) {
    let t = app.theme();
    let area = f.area();
    let w = 60.min(area.width.saturating_sub(2)).max(20.min(area.width));
    let h = 13.min(area.height);
    let r = Rect::new(
        area.x + area.width.saturating_sub(w) / 2,
        area.y + area.height.saturating_sub(h) / 2,
        w,
        h,
    );
    let bs = Style::default().fg(t.border_focus);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(bs)
        .style(Style::default().bg(t.bg).fg(t.fg))
        .title(Line::from(vec![
            Span::styled("─", bs),
            Span::styled(
                " cpatop ",
                Style::default().fg(t.hi).add_modifier(Modifier::BOLD),
            ),
        ]))
        .title(
            Line::from(vec![
                Span::styled(format!(" {} ", app.host), Style::default().fg(t.graph_text)),
                Span::styled("─", bs),
            ])
            .right_aligned(),
        );
    let inner = block.inner(r);
    block.render(r, f.buffer_mut());
    let dim = Style::default().fg(t.graph_text);
    let strong = Style::default().fg(t.title).add_modifier(Modifier::BOLD);
    let mut lines: Vec<Line> = Vec::new();
    let mut hint = "q quit · t theme";
    match &app.screen {
        Screen::Connecting => {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Connecting to CPA Usage Keeper…",
                strong,
            )));
            lines.push(Line::from(Span::styled(app.settings.base_url.clone(), dim)));
        }
        Screen::Startup(s) => {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("Keeper is starting up", strong)));
            let mut msg = format!("{} — {}", s.phase, s.message);
            if let Some(p) = &s.progress {
                if let Some(total) = p.total_count.filter(|t| *t > 0.0) {
                    msg.push_str(&format!(
                        " ({:.0}%)",
                        (p.processed_count / total * 100.0).min(100.0)
                    ));
                } else {
                    msg.push_str(&format!(" ({} processed)", fmt::int(p.processed_count)));
                }
            }
            lines.push(Line::from(Span::styled(msg, Style::default().fg(t.warn))));
            lines.push(Line::from(Span::styled("waiting until it is ready…", dim)));
            hint = "r retry now · q quit";
        }
        Screen::Offline { message, retry_at } => {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Cannot load data from the server",
                Style::default().fg(t.err).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                message.clone(),
                Style::default().fg(t.fg),
            )));
            let secs = retry_at.saturating_duration_since(app.now()).as_secs();
            lines.push(Line::from(Span::styled(
                format!("retrying in {secs}s · base URL {}", app.settings.base_url),
                dim,
            )));
            hint = "r retry now · q quit";
        }
        Screen::LoggingIn { mode, source } => {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("Signing in ({})…", mode.label()),
                strong,
            )));
            lines.push(Line::from(Span::styled(
                format!("credential from {source}"),
                dim,
            )));
        }
        Screen::Login(form) => {
            let tab = |m: LoginMode| {
                if m == form.mode {
                    Span::styled(
                        format!(" {} ", m.label()),
                        Style::default()
                            .bg(t.selected_bg)
                            .fg(t.selected_fg)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::styled(format!(" {} ", m.label()), Style::default().fg(t.inactive))
                }
            };
            lines.push(Line::from(vec![
                Span::raw(" "),
                tab(LoginMode::Admin),
                Span::raw("  "),
                tab(LoginMode::ApiKey),
            ]));
            lines.push(Line::from(""));
            let label = match form.mode {
                LoginMode::Admin => "Admin password",
                LoginMode::ApiKey => "CPA API key",
            };
            lines.push(Line::from(Span::styled(format!(" {label}"), dim)));
            let field_w = inner.width.saturating_sub(4) as usize;
            let n = form.input.chars().count().min(field_w.saturating_sub(1));
            let masked = format!("{}█", "•".repeat(n));
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    format!("{masked:<field_w$}"),
                    Style::default().bg(t.meter_bg).fg(t.title),
                ),
            ]));
            lines.push(Line::from(""));
            if let Some(e) = &form.error {
                lines.push(Line::from(Span::styled(
                    format!(" ✗ {e}"),
                    Style::default().fg(t.err).add_modifier(Modifier::BOLD),
                )));
            } else {
                let sub = match form.mode {
                    LoginMode::Admin => " Full dashboard (all tabs).",
                    LoginMode::ApiKey => " Per-key dashboard for a native CPA API key.",
                };
                lines.push(Line::from(Span::styled(sub, dim)));
            }
            hint = "Enter sign in · Tab switch mode · Esc quit";
        }
        Screen::Dashboard => {}
    }
    let body = Rect::new(
        inner.x + 1,
        inner.y,
        inner.width.saturating_sub(2),
        inner.height.saturating_sub(1),
    );
    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .render(body, f.buffer_mut());
    if inner.height > 0 {
        let hy = inner.bottom() - 1;
        f.buffer_mut().set_string(
            inner.x + 1,
            hy,
            fmt::trunc(hint, inner.width.saturating_sub(2) as usize),
            dim,
        );
    }
    if !app.warnings.is_empty() && area.height > h + 2 {
        let y = r.bottom();
        let room = area.bottom().saturating_sub(y) as usize;
        for (i, wmsg) in app.warnings.iter().take(3.min(room)).enumerate() {
            f.buffer_mut().set_string(
                r.x,
                y + i as u16,
                fmt::trunc(&format!("⚠ {wmsg}"), r.width as usize),
                Style::default().fg(t.warn),
            );
        }
    }
}
