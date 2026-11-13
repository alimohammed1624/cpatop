//! Login, connecting, startup and offline screens.

use crate::api::LoginMode;
use crate::app::{App, Screen};
use crate::fmt;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget, Wrap};

/// Wrap `text` to `width` columns; continuation lines get `indent` spaces.
pub fn wrap_indent(text: &str, width: usize, indent: usize) -> Vec<String> {
    let width = width.max(indent + 8);
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let limit = |out: &Vec<String>| if out.is_empty() { width } else { width - indent };
    for word in text.split_whitespace() {
        let ww = fmt::width(word);
        if ww > limit(&out) / 2 {
            // Long words (paths, URLs) start on the current line and are
            // split wherever the line ends.
            if !cur.is_empty() {
                if fmt::width(&cur) + 2 > limit(&out) {
                    out.push(std::mem::take(&mut cur));
                } else {
                    cur.push(' ');
                }
            }
            let mut rest: String = word.to_string();
            while fmt::width(&cur) + fmt::width(&rest) > limit(&out) {
                let room = limit(&out).saturating_sub(fmt::width(&cur)).max(1);
                let head: String = rest.chars().take(room).collect();
                rest = rest.chars().skip(room).collect();
                cur.push_str(&head);
                out.push(std::mem::take(&mut cur));
            }
            cur.push_str(&rest);
            continue;
        }
        if !cur.is_empty() && fmt::width(&cur) + 1 + ww > limit(&out) {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out.into_iter()
        .enumerate()
        .map(|(i, l)| {
            if i == 0 {
                l
            } else {
                format!("{}{l}", " ".repeat(indent))
            }
        })
        .collect()
}

pub fn draw(f: &mut Frame, app: &App) {
    let t = app.theme();
    let area = f.area();
    let w = 64.min(area.width.saturating_sub(2)).max(24.min(area.width));
    // Box borders, the body margin and the one-space line prefix.
    let text_w = w.saturating_sub(5) as usize;
    // Warnings (e.g. a world-readable secret file) live inside the box.
    let warn_lines: Vec<String> = app
        .warnings
        .iter()
        .take(3)
        .flat_map(|m| wrap_indent(&format!("⚠ {m}"), text_w, 2))
        .collect();
    let error_lines = match &app.screen {
        Screen::Login(form) => form
            .error
            .as_deref()
            .map(|e| wrap_indent(&format!("✗ {e}"), text_w, 2).len())
            .unwrap_or(1),
        _ => 1,
    } as u16;
    let extra = warn_lines.len() as u16 + if warn_lines.is_empty() { 0 } else { 1 };
    let h = (12 + error_lines + extra).min(area.height);
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
    let wrap = |s: &str| -> Vec<String> { wrap_indent(s, text_w, 2) };
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
            for l in wrap(&app.privacy_text(message)) {
                lines.push(Line::from(Span::styled(l, Style::default().fg(t.fg))));
            }
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
                for l in wrap(&format!("✗ {}", app.privacy_text(e))) {
                    lines.push(Line::from(Span::styled(
                        format!(" {l}"),
                        Style::default().fg(t.err).add_modifier(Modifier::BOLD),
                    )));
                }
            } else {
                let sub = match form.mode {
                    LoginMode::Admin => " Full dashboard (all tabs).",
                    LoginMode::ApiKey => " Per-key dashboard for a native CPA API key.",
                };
                lines.push(Line::from(Span::styled(sub, dim)));
            }
            hint = "Enter sign in · Tab switch mode · Esc clear · Ctrl-C quit";
        }
        Screen::Dashboard => {}
    }
    if !warn_lines.is_empty() {
        lines.push(Line::from(""));
        for l in &warn_lines {
            lines.push(Line::from(Span::styled(
                format!(" {l}"),
                Style::default().fg(t.warn),
            )));
        }
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
}

#[cfg(test)]
mod tests {
    use super::wrap_indent;

    #[test]
    fn wraps_with_indent_and_splits_long_words() {
        let v = wrap_indent(
            "⚠ /a/very/long/path/that/goes/on/and/on is readable by others",
            24,
            2,
        );
        assert!(v.len() >= 3, "{v:?}");
        for l in &v[1..] {
            assert!(l.starts_with("  "), "{l:?}");
        }
        for l in &v {
            assert!(crate::fmt::width(l) <= 24, "{l:?}");
        }
    }
}
