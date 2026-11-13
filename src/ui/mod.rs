//! Rendering.

mod login;
mod overlay;
mod panes;
pub mod table;
pub mod widgets;

use crate::api::Role;
use crate::app::{App, Hit, InputMode, Overlay, Screen};
use crate::fmt;
use crate::layout::{self, PaneId};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Clear;

/// Draw a full frame and record hit-test regions in `app.hit`.
pub fn draw(f: &mut Frame, app: &mut App) {
    let mut hit = std::mem::take(&mut app.hit);
    hit.tabs.clear();
    hit.panes.clear();
    hit.bodies.clear();
    hit.graphs.clear();
    hit.selected.clear();
    hit.picker.clear();
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
    if area.height < 6 || area.width < 30 {
        f.buffer_mut().set_string(
            area.x,
            area.y,
            fmt::trunc("terminal too small (min 30×6)", area.width as usize),
            Style::default().fg(app.theme().err),
        );
        return;
    }
    header(f, app, Rect::new(area.x, area.y, area.width, 1));
    tab_bar(f, app, hit, Rect::new(area.x, area.y + 1, area.width, 1));
    let body = Rect::new(area.x, area.y + 2, area.width, area.height - 2);
    let hidden = app.effective_hidden(app.tab);
    let size = |id: PaneId, w: u16| panes::size(app, id, w);
    let placed = layout::compute(app.tab, app.preset(app.tab), &hidden, body, &size);
    let visible: Vec<PaneId> = placed
        .iter()
        .filter(|p| !p.collapsed)
        .map(|p| p.id)
        .collect();
    let focus = app.focused_in(&visible);
    hit.panes = placed
        .iter()
        .filter(|p| !p.collapsed)
        .map(|p| (p.id, p.rect))
        .collect();
    for p in placed {
        if p.collapsed {
            panes::collapsed(f, app, p.id, p.rect);
        } else {
            panes::draw(f, app, hit, p.id, p.rect, focus == Some(p.id));
        }
    }
    if let InputMode::Filter(id) = app.mode {
        filter_prompt(f, app, id);
    }
    match &app.overlay {
        Some(Overlay::Help) => overlay::help(f, app),
        Some(Overlay::Picker(p)) => overlay::picker(f, app, hit, p),
        Some(Overlay::Range(ed)) => overlay::range_editor(f, app, ed),
        None => {}
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
        Some(Role::KeyViewer) if app.settings.privacy => app.session_key_name(),
        Some(Role::KeyViewer) => format!("key {}", app.session_key_name()),
        None => "-".into(),
    };
    segs.push(vec![Span::styled(
        role,
        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
    )]);
    let range = match app.tab {
        layout::Tab::Realtime => format!("window {}", app.window.param()),
        layout::Tab::Credentials | layout::Tab::Quota | layout::Tab::Models => "all time".into(),
        _ => app.range.label(),
    };
    segs.push(vec![Span::styled("◷ ", dim), Span::styled(range, val)]);
    if let Some(k) = &app.api_key {
        segs.push(vec![
            Span::styled("⚿ ", dim),
            Span::styled(
                app.key_name(&k.id, &k.label),
                Style::default().fg(t.hi).add_modifier(Modifier::BOLD),
            ),
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
            format!("▲ stale: {}", fmt::trunc(&app.privacy_text(e), 60)),
            Style::default().fg(t.err).add_modifier(Modifier::BOLD),
        )],
        None => vec![Span::styled("● ok", Style::default().fg(t.ok))],
    };

    let right: Vec<Span<'static>> = if let Some(secs) = app.logout_countdown() {
        vec![Span::styled(
            format!(" ⚠ press X again to log out ({secs}s) · any key cancels "),
            Style::default()
                .fg(t.err)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )]
    } else {
        match &app.flash {
            Some((m, _)) => vec![Span::styled(
                format!("{} ", app.privacy_text(m)),
                Style::default().fg(t.hi).add_modifier(Modifier::BOLD),
            )],
            None => vec![
                Span::styled("?", Style::default().fg(t.hi).add_modifier(Modifier::BOLD)),
                Span::styled(" help ", dim),
            ],
        }
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
        // Remove in order: refresh, age, then host, role, range, key filter.
        let idx = segs
            .iter()
            .position(|s| s.first().is_some_and(|x| x.content == "⟳ "))
            .or_else(|| {
                segs.iter()
                    .position(|s| s.first().is_some_and(|x| x.content == "↻ "))
            })
            .unwrap_or(0);
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
            t.selected().add_modifier(Modifier::BOLD)
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
        t.selected().add_modifier(Modifier::BOLD),
    );
}
