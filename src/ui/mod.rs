//! Rendering.

mod login;
mod overlay;
mod panes;
pub mod table;
pub mod widgets;

use crate::api::Role;
use crate::app::{App, Cadence, Hit, InputMode, Overlay, Screen};
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
    hit.graph_last.clear();
    hit.selected.clear();
    hit.picker.clear();
    hit.row_fields.clear();
    // Every table on screen gets its own state, so the row count it records
    // while drawing is there for the first key press (not lost in the shared
    // empty default).
    for d in layout::panes(app.tab).iter().filter(|d| d.table) {
        app.table_mut(d.id);
    }
    render(f, app, &mut hit);
    app.hit = hit;
    // Privacy mode: the last line of defence runs on the finished frame.
    if app.settings.privacy {
        let n = crate::app::guard_buffer(f.buffer_mut());
        app.privacy_guard_hits.set(n);
    }
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
        Some(Overlay::Row(id)) => overlay::row_detail(f, app, hit, *id),
        None => {}
    }
}

/// How far the header has been squeezed to fit (see [`header_segments`]).
/// Each level keeps everything the previous one dropped out and gives up one
/// more thing, least useful first.
const HEADER_LEVELS: u8 = 10;

/// Header cadence of tabs refreshed only on demand (`R`), and its short
/// form for narrow terminals.
const MANUAL: &str = "manual (R)";
const MANUAL_SHORT: &str = "manual";

/// The header's segments at a squeeze level: 0 is everything in full; then
/// the age loses "ago", `manual (R)` shortens to `manual`, the refresh
/// cadence goes, the host goes, a key viewer's key is named by its number
/// only, the age goes, the update badge shrinks to `↑` (it never goes),
/// then the role, the range and the key filter.
fn header_segments(app: &App, level: u8) -> Vec<Vec<Span<'static>>> {
    let t = app.theme();
    let dim = Style::default().fg(t.graph_text);
    let val = Style::default().fg(t.title);
    let mut segs: Vec<Vec<Span<'static>>> = Vec::new();
    if level < 4 {
        segs.push(vec![
            Span::styled("⌂ ", dim),
            Span::styled(app.host_label(), val),
        ]);
    }
    let key_id = app
        .session
        .as_ref()
        .and_then(|s| s.api_key.as_ref())
        .map(|k| k.id.clone())
        .unwrap_or_default();
    let role = match app.role {
        Some(Role::Admin) => "admin".to_string(),
        Some(Role::KeyViewer) if level >= 5 => format!("key #{key_id}"),
        Some(Role::KeyViewer) if app.settings.privacy => app.session_key_name(),
        Some(Role::KeyViewer) => format!("key {}", app.session_key_name()),
        None => "-".into(),
    };
    if level < 8 {
        segs.push(vec![Span::styled(
            role,
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        )]);
    }
    let range = match app.tab {
        layout::Tab::Realtime => format!("window {}", app.window.param()),
        layout::Tab::Credentials | layout::Tab::Quota | layout::Tab::Models => "all time".into(),
        _ => app.range.label(),
    };
    if level < 9 {
        segs.push(vec![Span::styled("◷ ", dim), Span::styled(range, val)]);
    }
    if let Some(k) = &app.api_key
        && level < 10
    {
        segs.push(vec![
            Span::styled("⚿ ", dim),
            Span::styled(
                app.key_name(&k.id, &k.label),
                Style::default().fg(t.hi).add_modifier(Modifier::BOLD),
            ),
        ]);
    }
    // The refresh cadence of the tab on screen, not a global setting.
    let cadence = match app.cadence() {
        Cadence::Every(d) if d.as_secs() >= 60 && d.as_secs() % 60 == 0 => {
            format!("{}m", d.as_secs() / 60)
        }
        Cadence::Every(d) => format!("{}s", d.as_secs()),
        Cadence::Live => "live".into(),
        Cadence::OnDemand if level >= 2 => MANUAL_SHORT.into(),
        Cadence::OnDemand => MANUAL.into(),
    };
    if level < 3 {
        segs.push(vec![Span::styled("⟳ ", dim), Span::styled(cadence, val)]);
    }
    let at = app.last_update();
    let age = at
        .map(|at| {
            let a = fmt::age(app.now().saturating_duration_since(at).as_secs());
            if level < 1 { a + " ago" } else { a }
        })
        .unwrap_or_else(|| "—".into());
    let age_style = if app.is_stale(at).is_some() {
        Style::default().fg(t.warn).add_modifier(Modifier::BOLD)
    } else {
        val
    };
    if level < 6 {
        segs.push(vec![Span::styled("↻ ", dim), Span::styled(age, age_style)]);
    }
    // A newer cpatop release (details in the help overlay): shortened when
    // space is short, never hidden.
    if let Some(tag) = &app.update_available {
        let badge = Style::default().fg(t.ok).add_modifier(Modifier::BOLD);
        segs.push(if level < 7 {
            vec![
                Span::styled("↑ ", badge),
                Span::styled(format!("v{}", crate::update::display_version(tag)), badge),
            ]
        } else {
            vec![Span::styled("↑", badge)]
        });
    }
    segs
}

fn header(f: &mut Frame, app: &App, area: Rect) {
    let t = app.theme();
    let sep = Span::styled(" │ ", Style::default().fg(t.border));
    let dim = Style::default().fg(t.graph_text);
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
    // A long message must not push the title, the status and the update
    // badge off the line: it is cut instead.
    let keep = 8 + 5 + if app.update_available.is_some() { 4 } else { 0 };
    let right = panes::fit_spans(right, (area.width as usize).saturating_sub(keep));
    let right_w: usize = right.iter().map(|s| s.width()).sum();
    let title = vec![Span::styled(
        " cpatop ",
        Style::default().fg(t.hi).add_modifier(Modifier::BOLD),
    )];
    let budget = (area.width as usize).saturating_sub(right_w + 1);
    let width_of = |v: &[Vec<Span<'static>>]| -> usize {
        v.iter()
            .map(|s| s.iter().map(|x| x.width()).sum::<usize>() + 3)
            .sum()
    };
    // Measured as if the age always had three digits, so the header does not
    // change shape as `9s` becomes `10s`.
    let age_slack = app
        .last_update()
        .map(|at| {
            let age = fmt::age(app.now().saturating_duration_since(at).as_secs());
            3usize.saturating_sub(fmt::width(&age))
        })
        .unwrap_or(0);
    let slack = |v: &[Vec<Span<'static>>]| {
        if v.iter()
            .any(|s| s.first().is_some_and(|x| x.content == "↻ "))
        {
            age_slack
        } else {
            0
        }
    };
    let status_w: usize = status.iter().map(|s| s.width()).sum();
    // Squeeze until everything fits (the status always stays).
    let mut segs = (0..=HEADER_LEVELS)
        .map(|l| header_segments(app, l))
        .find(|s| 8 + width_of(s) + slack(s) + status_w <= budget)
        .unwrap_or_else(|| header_segments(app, HEADER_LEVELS));
    // Still too wide (a long error): keep only the update badge.
    if 8 + width_of(&segs) + status_w > budget {
        segs.retain(|s| s.first().is_some_and(|x| x.content.starts_with('↑')));
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
    let tabs = app.tabs();
    // Three densities: bracketed, plain, abbreviated.
    let title_w: usize = tabs.iter().map(|t| t.title().len()).sum();
    let bracketed = title_w + tabs.len() * 4 + 2 <= area.width as usize;
    let plain = title_w + tabs.len() * 2 + 2 <= area.width as usize;
    let edge = if bracketed { 1 } else { 0 };
    let labels: Vec<String> = tabs
        .iter()
        .map(|tab| {
            let title = if bracketed || plain {
                tab.title()
            } else {
                tab.short_title()
            };
            // Without colour some terminals drop bold/reverse too: the
            // active tab is also marked by brackets.
            if t.no_color && *tab == app.tab {
                format!("[{title}]")
            } else {
                format!(" {title} ")
            }
        })
        .collect();
    let widths: Vec<u16> = labels
        .iter()
        .map(|l| l.chars().count() as u16 + 2 * edge)
        .collect();
    let active = tabs.iter().position(|x| *x == app.tab).unwrap_or(0);
    let (first, last) = tab_window(&widths, active, area.width.saturating_sub(1));
    let more_style = Style::default().fg(t.hi).add_modifier(Modifier::BOLD);
    let mut x = area.x + 1;
    // Tabs scrolled off either end: `‹`/`›` mark them, and a click on the
    // marker goes to the nearest hidden tab.
    if first > 0 {
        buf.set_string(area.x, area.y, "‹", more_style);
        hit.tabs
            .push((tabs[first - 1], Rect::new(area.x, area.y, 1, 1)));
    }
    for i in first..=last {
        let (tab, label, w) = (tabs[i], &labels[i], widths[i]);
        let style = if tab == app.tab {
            t.selected().add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.inactive)
        };
        if bracketed {
            buf.set_string(x, area.y, "┤", line_style);
            buf.set_string(x + w - 1, area.y, "├", line_style);
        }
        buf.set_string(x + edge, area.y, label, style);
        hit.tabs.push((tab, Rect::new(x, area.y, w, 1)));
        x += w;
    }
    if last + 1 < tabs.len() {
        buf.set_string(x, area.y, "›", more_style);
        hit.tabs.push((tabs[last + 1], Rect::new(x, area.y, 1, 1)));
        x += 1;
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

/// The tabs to show (`first..=last`) when they are `widths` wide and `avail`
/// columns are free: as many as fit, starting as far left as possible while
/// still including the `active` one. A `›` after the last costs a column
/// when tabs remain to its right (the `‹` sits on the line's first column).
fn tab_window(widths: &[u16], active: usize, avail: u16) -> (usize, usize) {
    let n = widths.len();
    for first in 0..=active.min(n.saturating_sub(1)) {
        let mut used = 0u16;
        let mut last = None;
        for (j, w) in widths.iter().enumerate().skip(first) {
            used = used.saturating_add(*w);
            let need = used + u16::from(j + 1 < n);
            if need > avail {
                break;
            }
            last = Some(j);
        }
        if let Some(last) = last.filter(|l| *l >= active) {
            return (first, last);
        }
    }
    (active, active)
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

#[cfg(test)]
mod tests {
    use super::tab_window;

    #[test]
    fn tab_strip_scrolls_to_keep_the_active_tab() {
        // Eight 7-column tabs in 40 columns: five fit with a `›`.
        let w = [7u16; 8];
        assert_eq!(tab_window(&w, 0, 40), (0, 4));
        assert_eq!(tab_window(&w, 4, 40), (0, 4));
        // Further right, the strip scrolls just far enough.
        assert_eq!(tab_window(&w, 5, 40), (1, 5));
        // The last tab needs no `›`.
        assert_eq!(tab_window(&w, 7, 40), (3, 7));
        // Everything fits: no scrolling.
        assert_eq!(tab_window(&w, 7, 56), (0, 7));
        // Not even one fits: the active tab alone.
        assert_eq!(tab_window(&w, 3, 4), (3, 3));
    }
}
