//! Rendering.

mod login;
mod panes;
pub mod table;
pub mod widgets;

use crate::api::Role;
use crate::app::{App, Hit, InputMode, Screen};
use crate::fmt;
use crate::layout::{self, PaneId};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget};

/// Draw a full frame and record hit-test regions in `app.hit`.
pub fn draw(f: &mut Frame, app: &mut App) {
    let mut hit = std::mem::take(&mut app.hit);
    hit.tabs.clear();
    hit.panes.clear();
    hit.bodies.clear();
    hit.selected.clear();
    render(f, app, &mut hit);
    app.hit = hit;
}

fn render(f: &mut Frame, app: &App, hit: &mut Hit) {
    let area = f.area();
    let t = app.theme();
    f.buffer_mut()
        .set_style(area, Style::default().bg(t.bg).fg(t.fg));
    match &app.screen {
        Screen::Dashboard => dashboard(f, app, hit),
        _ => login::draw(f, app),
    }
}

fn dashboard(f: &mut Frame, app: &App, hit: &mut Hit) {
    let area = f.area();
    if area.height < 4 || area.width < 20 {
        f.buffer_mut().set_string(
            area.x,
            area.y,
            fmt::trunc("terminal too small", area.width as usize),
            Style::default().fg(app.theme().err),
        );
        return;
    }
    header(f, app, Rect::new(area.x, area.y, area.width, 1));
    tab_bar(f, app, hit, Rect::new(area.x, area.y + 1, area.width, 1));
    let body = Rect::new(area.x, area.y + 2, area.width, area.height - 2);
    let rects = layout::compute(app.tab, app.preset(app.tab), app.hidden(app.tab), body);
    let visible: Vec<PaneId> = rects.iter().map(|(id, _)| *id).collect();
    let focus = app.focused_in(&visible);
    hit.panes = rects.clone();
    for (id, r) in rects {
        panes::draw(f, app, hit, id, r, focus == Some(id));
    }
    if let InputMode::Filter(id) = app.mode {
        filter_prompt(f, app, id);
    }
    if app.help {
        help(f, app);
    }
}

fn header(f: &mut Frame, app: &App, area: Rect) {
    let t = app.theme();
    let sep = Span::styled(" │ ", Style::default().fg(t.border));
    let dim = Style::default().fg(t.graph_text);
    let val = Style::default().fg(t.title);
    let mut segs: Vec<Vec<Span<'static>>> = Vec::new();
    segs.push(vec![
        Span::styled("⌂ ", dim),
        Span::styled(app.host.clone(), val),
    ]);
    let role = match app.role {
        Some(Role::Admin) => "admin".to_string(),
        Some(Role::KeyViewer) => {
            let k = app
                .session
                .as_ref()
                .and_then(|s| s.api_key.as_ref())
                .map(|k| {
                    k.alias
                        .clone()
                        .filter(|a| !a.is_empty())
                        .unwrap_or_else(|| fmt::redact_key(&k.display_key))
                })
                .unwrap_or_default();
            format!("key {k}")
        }
        None => "-".into(),
    };
    segs.push(vec![Span::styled(
        role,
        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
    )]);
    let range = match app.tab {
        layout::Tab::Realtime => format!("window {}", app.window.param()),
        layout::Tab::Credentials
        | layout::Tab::Quota
        | layout::Tab::Models
        | layout::Tab::Settings => "all time".into(),
        _ => app.range.to_string(),
    };
    segs.push(vec![Span::styled("◷ ", dim), Span::styled(range, val)]);
    if let Some(k) = &app.api_key {
        segs.push(vec![
            Span::styled("key ", dim),
            Span::styled(fmt::redact_key(&k.label), val),
        ]);
    }
    segs.push(vec![
        Span::styled("⟳ ", dim),
        Span::styled(format!("{}s", app.settings.refresh_secs), val),
    ]);
    let age = app
        .last_update()
        .map(|at| fmt::age(app.now().saturating_duration_since(at).as_secs()) + " ago")
        .unwrap_or_else(|| "—".into());
    segs.push(vec![Span::styled("↻ ", dim), Span::styled(age, val)]);
    let status: Vec<Span<'static>> = match app.current_error() {
        Some(e) => vec![Span::styled(
            format!("▲ stale: {}", fmt::trunc(e, 60)),
            Style::default().fg(t.err).add_modifier(Modifier::BOLD),
        )],
        None => vec![Span::styled("● ok", Style::default().fg(t.ok))],
    };

    let right: Vec<Span<'static>> = match &app.flash {
        Some((m, _)) => vec![Span::styled(
            format!("{m} "),
            Style::default().fg(t.hi).add_modifier(Modifier::BOLD),
        )],
        None => vec![
            Span::styled("?", Style::default().fg(t.hi).add_modifier(Modifier::BOLD)),
            Span::styled(" help ", dim),
        ],
    };
    let right_w: usize = right.iter().map(|s| s.width()).sum();
    let title = vec![Span::styled(
        " cpatop ",
        Style::default().fg(t.hi).add_modifier(Modifier::BOLD),
    )];
    let budget = (area.width as usize).saturating_sub(right_w + 1);
    // Drop low-priority segments until everything fits (status always stays).
    let width_of = |v: &[Vec<Span<'static>>]| -> usize {
        v.iter()
            .map(|s| s.iter().map(|x| x.width()).sum::<usize>() + 3)
            .sum()
    };
    let status_w: usize = status.iter().map(|s| s.width()).sum();
    while !segs.is_empty() && 8 + width_of(&segs) + status_w > budget {
        // Remove in order: refresh, age, key filter, range, role, host.
        let idx = if segs.len() > 3 {
            segs.len() - 2
        } else {
            segs.len() - 1
        };
        segs.remove(idx);
    }
    let mut spans = title;
    for s in segs {
        spans.extend(s);
        spans.push(sep.clone());
    }
    spans.extend(status);
    f.buffer_mut()
        .set_line(area.x, area.y, &Line::from(spans), area.width);
    let rx = area.x + area.width.saturating_sub(right_w as u16);
    f.buffer_mut()
        .set_line(rx, area.y, &Line::from(right), right_w as u16);
}

fn tab_bar(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect) {
    let t = app.theme();
    let buf = f.buffer_mut();
    let line_style = Style::default().fg(t.border);
    buf.set_string(area.x, area.y, "─".repeat(area.width as usize), line_style);
    let mut x = area.x + 1;
    let tabs = app.tabs();
    // Three densities: bracketed, plain, abbreviated.
    let title_w: usize = tabs.iter().map(|t| t.title().len()).sum();
    let bracketed = title_w + tabs.len() * 4 + 2 <= area.width as usize;
    let plain = title_w + tabs.len() * 2 + 2 <= area.width as usize;
    for tab in tabs {
        let title = if bracketed || plain {
            tab.title().to_string()
        } else {
            tab.short_title().to_string()
        };
        let label = format!(" {title} ");
        let w = label.chars().count() as u16;
        let edge = if bracketed { 1 } else { 0 };
        if x + w + 2 * edge > area.right() {
            break;
        }
        let style = if *tab == app.tab {
            Style::default()
                .bg(t.selected_bg)
                .fg(t.selected_fg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.inactive)
        };
        if bracketed {
            buf.set_string(x, area.y, "┤", line_style);
            buf.set_string(x + w + 1, area.y, "├", line_style);
        }
        buf.set_string(x + edge, area.y, &label, style);
        hit.tabs.push((*tab, Rect::new(x, area.y, w + 2 * edge, 1)));
        x += w + 2 * edge;
    }
    // Right side: layout preset name.
    let preset = layout::presets(app.tab)[app.preset(app.tab)].name;
    let info = format!(" layout {preset} ");
    let iw = info.chars().count() as u16;
    if x + iw + 2 < area.right() {
        buf.set_string(
            area.right() - iw - 1,
            area.y,
            info,
            Style::default().fg(t.graph_text),
        );
    }
}

fn filter_prompt(f: &mut Frame, app: &App, id: PaneId) {
    let t = app.theme();
    let area = f.area();
    let text = format!(
        " filter {}: {}█  (Enter keep · Esc clear) ",
        layout::pane_def(id).2.name,
        app.table(id).filter
    );
    let w = (fmt::width(&text) as u16).min(area.width);
    let r = Rect::new(area.x, area.bottom() - 1, w, 1);
    f.render_widget(Clear, r);
    f.buffer_mut().set_string(
        r.x,
        r.y,
        fmt::trunc(&text, w as usize),
        Style::default()
            .bg(t.selected_bg)
            .fg(t.selected_fg)
            .add_modifier(Modifier::BOLD),
    );
}

const HELP: &[(&str, &str)] = &[
    ("Tab ←/→", "next / previous tab (or click)"),
    ("1-9", "toggle pane by its number"),
    ("p / l, P", "next / previous layout preset"),
    ("f", "focus next table (or click a pane)"),
    ("↑↓ j/k PgUp/PgDn", "move selection; g/G top/bottom"),
    ("/", "filter focused table; Esc clears"),
    ("s / S", "sort by next column / flip direction"),
    ("d", "switch dimension (models, keys, …)"),
    ("[ ] / r", "previous / next time range"),
    ("w", "realtime window 15m/30m/60m"),
    ("a", "API-key filter (admin)"),
    ("e", "request log: all / success / failed"),
    ("+ / -", "slower / faster refresh"),
    ("R", "refresh now"),
    ("t / T", "next / previous theme"),
    ("b", "graph symbols braille/block/ascii"),
    ("x", "privacy mode (mask e-mails)"),
    ("X X", "log out"),
    ("? / Esc", "help / close; q quits"),
];

fn help(f: &mut Frame, app: &App) {
    let t = app.theme();
    let area = f.area();
    let w = 62.min(area.width.saturating_sub(2));
    let h = (HELP.len() as u16 + 4).min(area.height.saturating_sub(1));
    let r = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height.saturating_sub(h)) / 2,
        w,
        h,
    );
    f.render_widget(Clear, r);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.border_focus))
        .style(Style::default().bg(t.bg).fg(t.fg))
        .title(Line::from(vec![
            Span::styled("─", Style::default().fg(t.border_focus)),
            Span::styled(
                " help ",
                Style::default().fg(t.title).add_modifier(Modifier::BOLD),
            ),
        ]))
        .title_bottom(
            Line::from(Span::styled(
                format!(" cpatop {} ", env!("CARGO_PKG_VERSION")),
                Style::default().fg(t.graph_text),
            ))
            .right_aligned(),
        );
    let lines: Vec<Line> = HELP
        .iter()
        .map(|(k, d)| {
            Line::from(vec![
                Span::styled(
                    format!(" {k:<18}"),
                    Style::default().fg(t.hi).add_modifier(Modifier::BOLD),
                ),
                Span::styled(d.to_string(), Style::default().fg(t.fg)),
            ])
        })
        .collect();
    let inner = block.inner(r);
    block.render(r, f.buffer_mut());
    Paragraph::new(lines).render(
        Rect::new(
            inner.x,
            inner.y + 1,
            inner.width,
            inner.height.saturating_sub(1),
        ),
        f.buffer_mut(),
    );
}
