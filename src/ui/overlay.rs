//! Overlays: help, pickers and the custom range editor.

use crate::app::{App, Hit, Picker, RangeEditor, Unit};
use crate::fmt;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget};

fn boxed(f: &mut Frame, t: &Theme, r: Rect, title: &str, bottom: &str) -> Rect {
    f.render_widget(Clear, r);
    let bs = Style::default().fg(t.border_focus);
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(bs)
        .style(Style::default().bg(t.bg).fg(t.fg))
        .title(Line::from(vec![
            Span::styled("─", bs),
            Span::styled(
                format!(" {title} "),
                Style::default().fg(t.title).add_modifier(Modifier::BOLD),
            ),
        ]));
    if !bottom.is_empty() {
        block = block.title_bottom(Line::from(vec![
            Span::styled("─", bs),
            Span::styled(
                format!(
                    " {} ",
                    fmt::trunc(bottom, r.width.saturating_sub(6) as usize)
                ),
                Style::default().fg(t.graph_text),
            ),
        ]));
    }
    let inner = block.inner(r);
    block.render(r, f.buffer_mut());
    inner
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(2)).max(10.min(area.width));
    let h = h.min(area.height.saturating_sub(2)).max(3.min(area.height));
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height.saturating_sub(h)) / 2,
        w,
        h,
    )
}

const QUIT_KEYS: &str = "q  Ctrl-C";

/// What quitting does to the server session (`--logout-on-exit`).
fn quit_text(logout_on_exit: bool) -> &'static str {
    if logout_on_exit {
        "quit and log out on the server"
    } else {
        "quit (the server session is kept)"
    }
}

const HELP: &[(&str, &[(&str, &str)])] = &[
    (
        "navigate",
        &[
            ("Tab S-Tab ←→", "next / previous tab (or click)"),
            ("F2…F9", "jump to a tab"),
            ("f / F", "focus next / previous table or graph"),
            ("1-9", "hide / show pane by number"),
            ("0", "show every hidden pane"),
            ("p l / P", "next / previous layout"),
            ("Esc", "close / clear the innermost thing"),
            (QUIT_KEYS, "quit (the server session is kept)"),
        ],
    ),
    (
        "tables",
        &[
            ("↑↓ j k", "move selection (mouse wheel too)"),
            ("Enter", "every field of the selected row"),
            ("  ↑↓ [ ]", "  scroll it / previous, next row"),
            ("PgUp PgDn g G", "page / top / bottom"),
            ("/", "find in the focused table"),
            ("s / S", "sort by next column / reverse"),
            ("d", "switch dimension (models, keys, …)"),
        ],
    ),
    (
        "graphs (focused)",
        &[
            ("← →", "move the cursor, read exact values"),
            ("g / G", "first / last bucket · click to place"),
            ("Esc", "remove the cursor"),
        ],
    ),
    (
        "data",
        &[
            ("r", "range picker: presets, custom ranges"),
            ("[ ]", "previous / next preset range"),
            ("a", "API-key filter picker"),
            ("w", "realtime window 15m / 30m / 60m"),
            ("R", "refresh now"),
            ("+ / -", "slower / faster overview refresh"),
        ],
    ),
    (
        "requests tab",
        &[
            ("e", "result: all / success / failed"),
            ("m / o", "model / source picker"),
            ("c", "clear request filters"),
        ],
    ),
    (
        "credential names",
        &[
            ("name · cl", "one account on several providers;"),
            ("", "the provider is abbreviated when short"),
            ("cl cx ag", "claude  codex  antigravity"),
            ("gm gc vx as", "gemini  gemini-cli  vertex  aistudio"),
            ("oa oc", "openai  openai-compatibility"),
            ("qw if km xa", "qwen  iflow  kimi  xai"),
            ("… api  #12", "API-key credential · credential id"),
        ],
    ),
    (
        "display & session",
        &[
            ("t / T", "next / previous theme"),
            ("b", "graph symbols braille / block / ascii"),
            ("x", "privacy: hide e-mails, API keys, IPs"),
            ("", "also typed filters and searches"),
            ("", "header shows ◐ private while on"),
            ("X X", "log out on the server (twice in 3 s)"),
            ("?", "this help"),
        ],
    ),
    (
        "offline / login screen",
        &[("b · ^S", "change the server URL")],
    ),
    (
        "reading the numbers",
        &[
            ("buckets", "the server cuts hours at UTC hours:"),
            ("", "at +05:30 Overview counts today from"),
            ("", "00:00, Analysis from 00:30; Overview"),
            ("", "days run 00:30 → 00:30"),
            ("Σ rolling", "activity grid: last N×24 h, not days"),
            ("cache-hit", "requests that read cached tokens"),
            ("cache read", "input tokens read from the cache"),
        ],
    ),
];

/// Help sections the role can use. An API-key session has three tabs, no
/// API-key filter, no dimension switch (its tables have one dimension), no
/// requests tab and no credential names, so none of that is listed.
fn help_for(admin: bool, ntabs: usize) -> Vec<(&'static str, Vec<(String, &'static str)>)> {
    const ADMIN_SECTIONS: [&str; 2] = ["requests tab", "credential names"];
    const ADMIN_KEYS: [&str; 2] = ["a", "d"];
    HELP.iter()
        .filter(|(name, _)| admin || !ADMIN_SECTIONS.contains(name))
        .map(|(name, rows)| {
            let rows = rows
                .iter()
                .filter(|(k, _)| admin || !ADMIN_KEYS.contains(k))
                .map(|(k, d)| {
                    let k = if *k == "F2…F9" {
                        format!("F2…F{}", ntabs + 1)
                    } else {
                        k.to_string()
                    };
                    (k, *d)
                })
                .collect();
            (*name, rows)
        })
        .collect()
}

/// Lines of one help section.
fn help_section(
    t: &Theme,
    name: &str,
    rows: &[(String, &str)],
    logout_on_exit: bool,
) -> Vec<Line<'static>> {
    let mut v = vec![Line::from(Span::styled(
        name.to_string(),
        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
    ))];
    for (k, d) in rows {
        let d = if k == QUIT_KEYS {
            quit_text(logout_on_exit)
        } else {
            d
        };
        v.push(Line::from(vec![
            Span::styled(
                format!(" {k:<14}"),
                Style::default().fg(t.hi).add_modifier(Modifier::BOLD),
            ),
            Span::styled(d.to_string(), Style::default().fg(t.fg)),
        ]));
    }
    v
}

/// Wrap a shell command to `w` columns at spaces; a word longer than a line
/// (a URL) breaks after a `/` where possible.
fn wrap_command(cmd: &str, w: usize) -> Vec<String> {
    let w = w.max(10);
    let mut pieces: Vec<String> = Vec::new();
    for word in cmd.split(' ') {
        let mut rest = word;
        while fmt::width(rest) > w {
            let head: String = rest.chars().take(w).collect();
            let cut = head.rfind('/').map(|i| i + 1).unwrap_or(head.len());
            pieces.push(rest[..cut].to_string());
            rest = &rest[cut..];
        }
        pieces.push(rest.to_string());
    }
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for p in pieces {
        if !cur.is_empty() && fmt::width(&cur) + 1 + fmt::width(&p) > w {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() && !cur.ends_with('/') {
            cur.push(' ');
        }
        cur.push_str(&p);
    }
    out.push(cur);
    out
}

/// "update available": current -> latest and how to upgrade, across the
/// whole help box (`width` columns of text), then a blank line.
fn update_section(t: &Theme, tag: &str, width: usize) -> Vec<Line<'static>> {
    use crate::update::{CARGO_CMD, CURRENT, UPGRADE_CMD, display_version};
    let mut v = vec![Line::from(vec![
        Span::styled(
            "update available  ",
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("cpatop {CURRENT} → {}", display_version(tag)),
            Style::default().fg(t.ok).add_modifier(Modifier::BOLD),
        ),
    ])];
    let w = width.saturating_sub(1);
    for l in wrap_command(UPGRADE_CMD, w) {
        v.push(Line::from(Span::styled(
            format!(" {l}"),
            Style::default().fg(t.hi),
        )));
    }
    for l in wrap_command(&format!("or {CARGO_CMD}"), w) {
        v.push(Line::from(Span::styled(
            format!(" {l}"),
            Style::default().fg(t.graph_text),
        )));
    }
    v.push(Line::from(""));
    v
}

/// Help columns: sections in order, each column filled until it is as tall
/// as an even split, so nothing is clipped at any width.
fn help_columns(
    t: &Theme,
    ncols: usize,
    logout_on_exit: bool,
    admin: bool,
    ntabs: usize,
) -> Vec<Vec<Line<'static>>> {
    let help = help_for(admin, ntabs);
    let total: usize = help.iter().map(|(_, r)| r.len() + 2).sum();
    let target = total.div_ceil(ncols);
    let mut cols: Vec<Vec<Line<'static>>> = vec![Vec::new()];
    for (name, rows) in &help {
        let cur = cols.last_mut().expect("one column");
        if !cur.is_empty() && cur.len() + rows.len() + 1 > target && cols.len() < ncols {
            cols.push(Vec::new());
        }
        let cur = cols.last_mut().expect("one column");
        if !cur.is_empty() {
            cur.push(Line::from(""));
        }
        cur.extend(help_section(t, name, rows, logout_on_exit));
    }
    cols
}

/// The help overlay: one to three columns depending on the width, under a
/// full-width "update available" block when a newer release exists; when the
/// content is taller than the screen it scrolls (↑↓ j k PgUp PgDn g G or the
/// mouse wheel) and the bottom border shows the position.
pub fn help(f: &mut Frame, app: &App) {
    let t = app.theme();
    let area = f.area();
    let col_w = 54u16;
    let ncols = (((area.width.saturating_sub(4)) / (col_w + 2)) as usize).clamp(1, 3);
    let admin = app.role == Some(crate::api::Role::Admin);
    let cols = help_columns(
        t,
        ncols,
        app.settings.logout_on_exit,
        admin,
        app.tabs().len(),
    );
    let ncols = cols.len() as u16;
    let w = col_w * ncols + 2 * (ncols - 1) + 3;
    let text_w = w.min(area.width.saturating_sub(2)).saturating_sub(3);
    let head = app
        .update_available
        .as_deref()
        .map(|tag| update_section(t, tag, text_w as usize))
        .unwrap_or_default();
    let head_h = head.len() as u16;
    let content_h = head_h + cols.iter().map(Vec::len).max().unwrap_or(0) as u16;
    let r = centered(area, w, content_h + 2);
    let inner_h = r.height.saturating_sub(2);
    let max = content_h.saturating_sub(inner_h) as usize;
    app.help_max.set(max);
    app.help_page.set(inner_h.saturating_sub(1).max(1) as usize);
    let off = app.help_scroll.min(max) as u16;
    let version = format!("cpatop {}", env!("CARGO_PKG_VERSION"));
    let bottom = if max > 0 {
        let last = (off + inner_h).min(content_h);
        format!(
            "{}{} {}-{}/{} · ↑↓ PgUp PgDn scroll · Esc ? q close",
            if off > 0 { "▲" } else { " " },
            if (off as usize) < max { "▼" } else { " " },
            off + 1,
            last,
            content_h
        )
    } else {
        format!("{version} · Esc ? q close")
    };
    let inner = boxed(f, t, r, "help", &bottom);
    if inner.width < 2 {
        return;
    }
    // The update block scrolls away first, then the columns.
    let shown_head = head_h.saturating_sub(off).min(inner.height);
    if shown_head > 0 {
        Paragraph::new(head).scroll((off, 0)).render(
            Rect::new(inner.x + 1, inner.y, inner.width - 1, shown_head),
            f.buffer_mut(),
        );
    }
    let body = Rect::new(
        inner.x,
        inner.y + shown_head,
        inner.width,
        inner.height - shown_head,
    );
    let col_off = off.saturating_sub(head_h);
    for (i, lines) in cols.into_iter().enumerate() {
        let x = body.x + 1 + i as u16 * (col_w + 2);
        if x >= body.right() {
            break;
        }
        let w = col_w.min(body.right() - x);
        // Lines that do not fit end in `…` instead of being cut.
        let lines: Vec<Line<'static>> = lines
            .into_iter()
            .map(|l| Line::from(super::panes::fit_spans(l.spans, w as usize)))
            .collect();
        Paragraph::new(lines)
            .scroll((col_off, 0))
            .render(Rect::new(x, body.y, w, body.height), f.buffer_mut());
    }
}

pub fn picker(f: &mut Frame, app: &App, hit: &mut Hit, p: &Picker) {
    let t = app.theme();
    let area = f.area();
    let vis = p.visible();
    let max_label = p
        .items
        .iter()
        .map(|i| fmt::width(&i.label))
        .max()
        .unwrap_or(10);
    let max_detail = p
        .items
        .iter()
        .map(|i| fmt::width(&i.detail))
        .max()
        .unwrap_or(0);
    let w = ((max_label + max_detail + 9) as u16).clamp(54, 84);
    let h = (p.items.len() as u16 + 3).min(22);
    let r = centered(area, w, h);
    let inner = boxed(
        f,
        t,
        r,
        &p.title,
        "type to filter · ↑↓ · Enter pick · Esc cancel",
    );
    if inner.height < 2 {
        return;
    }
    // Query line.
    let count = format!("{}/{}", vis.len(), p.items.len());
    let q = Line::from(vec![
        Span::styled(
            " › ",
            Style::default().fg(t.hi).add_modifier(Modifier::BOLD),
        ),
        Span::styled(app.typed(&p.query), Style::default().fg(t.title)),
        Span::styled("█", Style::default().fg(t.hi)),
    ]);
    f.buffer_mut().set_line(inner.x, inner.y, &q, inner.width);
    f.buffer_mut().set_string(
        inner.right().saturating_sub(fmt::width(&count) as u16 + 1),
        inner.y,
        &count,
        Style::default().fg(t.graph_text),
    );
    let list = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
    if vis.is_empty() {
        crate::ui::widgets::message(f.buffer_mut(), list, "no match", t.graph_text);
        return;
    }
    let sel = p.selected.min(vis.len() - 1);
    let rows = list.height as usize;
    let off = if sel >= rows { sel + 1 - rows } else { 0 };
    for (row, idx) in vis.iter().enumerate().skip(off).take(rows) {
        let it = &p.items[*idx];
        let y = list.y + (row - off) as u16;
        let selected = row == sel;
        let base = if selected {
            t.selected().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        if selected {
            f.buffer_mut()
                .set_style(Rect::new(list.x, y, list.width, 1), base);
        }
        let mark = if it.current { "● " } else { "  " };
        // " ● " + label + at least two spaces + detail + " ".
        let room = (list.width as usize).saturating_sub(3 + 1);
        let label = fmt::trunc(&it.label, room.saturating_sub(2).max(8));
        let detail = fmt::trunc(&it.detail, room.saturating_sub(fmt::width(&label) + 2));
        let mut spans = vec![
            Span::styled(format!(" {mark}"), base.patch(Style::default().fg(t.ok))),
            Span::styled(
                label,
                base.patch(if selected {
                    Style::default()
                } else {
                    Style::default().fg(t.title)
                }),
            ),
        ];
        let used: usize = spans.iter().map(|s| s.width()).sum();
        let pad = (list.width as usize).saturating_sub(used + fmt::width(&detail) + 1);
        spans.push(Span::styled(" ".repeat(pad), base));
        spans.push(Span::styled(
            detail,
            base.patch(if selected {
                Style::default()
            } else {
                Style::default().fg(t.graph_text)
            }),
        ));
        f.buffer_mut()
            .set_line(list.x, y, &Line::from(spans), list.width);
        hit.picker.push((*idx, Rect::new(list.x, y, list.width, 1)));
    }
}

/// A range error that quotes what was typed (`"x" is not a date`): the
/// quoted part hidden like the field itself in privacy mode.
fn quoted_input(app: &App, e: &str) -> String {
    match (e.find('"'), e.rfind('"')) {
        (Some(a), Some(b)) if app.settings.privacy && b > a => {
            format!("{}{}{}", &e[..a + 1], app.typed_date(&e[a + 1..b]), &e[b..])
        }
        _ => e.to_string(),
    }
}

pub fn range_editor(f: &mut Frame, app: &App, ed: &RangeEditor) {
    let t = app.theme();
    let area = f.area();
    let now = app.server_now();
    let (title, fmt_hint, limit) = match ed.unit {
        Unit::Day => (
            "custom range · days",
            "YYYY-MM-DD or MM-DD, inclusive",
            format!(
                "1–{} days, within the last {} days, not in the future",
                ed.max_days, ed.max_days
            ),
        ),
        Unit::Hour => (
            "custom range · hours",
            "YYYY-MM-DD HH:00, MM-DD HH or HH (today)",
            "5–24 hourly slots, starting within the last 24 hours".to_string(),
        ),
    };
    let r = centered(area, 62, 12);
    let inner = boxed(f, t, r, title, "Tab switch field · Enter apply · Esc back");
    let dim = Style::default().fg(t.graph_text);
    let field = |label: &str, value: &str, active: bool| -> Line<'static> {
        let w = 28usize;
        let shown = format!("{value}{}", if active { "█" } else { "" });
        let style = if active {
            t.selected().add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.title).bg(t.meter_bg)
        };
        Line::from(vec![
            Span::styled(format!(" {label:<7}"), dim),
            Span::styled(format!(" {shown:<w$}"), style),
        ])
    };
    let mut lines = vec![
        Line::from(Span::styled(format!(" {fmt_hint}"), dim)),
        Line::from(""),
        field("start", &app.typed_date(&ed.start), ed.field == 0),
        field("end", &app.typed_date(&ed.end), ed.field == 1),
        Line::from(""),
    ];
    match &ed.error {
        Some(e) => lines.push(Line::from(Span::styled(
            format!(" ✗ {}", quoted_input(app, e)),
            Style::default().fg(t.err).add_modifier(Modifier::BOLD),
        ))),
        None => lines.push(Line::from(Span::styled(format!(" {limit}"), dim))),
    }
    lines.push(Line::from(Span::styled(
        format!(" server time {}", now.format("%Y-%m-%d %H:%M %:z")),
        dim,
    )));
    Paragraph::new(lines)
        .wrap(ratatui::widgets::Wrap { trim: false })
        .render(inner, f.buffer_mut());
}

/// Every field of a table's selected row, untruncated (wrapped). Longer
/// than the screen, it scrolls (↑↓ j k PgUp PgDn g G or the mouse wheel)
/// and the bottom border says how much is left; `[` `]` step through the
/// table's rows.
pub fn row_detail(f: &mut Frame, app: &App, hit: &Hit, id: crate::layout::PaneId) {
    let t = app.theme();
    let area = f.area();
    let Some(fields) = hit.row_fields.get(&id) else {
        return;
    };
    let name = crate::layout::pane_def(id).2.name;
    // Field names over a third of the box wrap their values less.
    let max_w = area.width.saturating_sub(4) as usize;
    let key_w = fields
        .iter()
        .map(|(k, _)| fmt::width(k))
        .max()
        .unwrap_or(4)
        .clamp(4, (max_w / 3).max(8));
    let val_w = fields
        .iter()
        .map(|(_, v)| fmt::width(v))
        .max()
        .unwrap_or(10);
    let w = (key_w + val_w + 5).clamp(44, max_w.max(10)) as u16;
    let text_w = (w as usize).saturating_sub(key_w + 5).max(8);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (k, v) in fields {
        let chunks = wrap_chars(v, text_w);
        for (i, c) in chunks.into_iter().enumerate() {
            let key = if i == 0 {
                fmt::trunc(k, key_w)
            } else {
                String::new()
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{key:<key_w$}  "),
                    Style::default().fg(t.graph_text),
                ),
                Span::styled(c, Style::default().fg(t.title)),
            ]));
        }
    }
    let content_h = lines.len() as u16;
    let h = (content_h + 2).min(area.height.saturating_sub(2));
    let r = centered(area, w, h);
    let inner_h = r.height.saturating_sub(2);
    let max = content_h.saturating_sub(inner_h) as usize;
    app.row_max.set(max);
    app.row_page.set(inner_h.saturating_sub(1).max(1) as usize);
    let off = app.row_scroll.min(max) as u16;
    let st = app.table(id);
    let (sel, n) = (
        st.selected.min(st.len.get().saturating_sub(1)),
        st.len.get(),
    );
    let title = format!("{name} · row {}/{n}", sel + 1);
    let below = content_h.saturating_sub(off + inner_h);
    let bottom = if max > 0 {
        let pos = match (off > 0, below > 0) {
            (true, true) => format!("▲{off} ▼{below} more"),
            (false, _) => format!("▼ {below} more"),
            (true, false) => format!("▲ {off} above"),
        };
        let full = format!("{pos} · ↑↓ PgUp PgDn scroll · [ ] row · Esc close");
        // Narrow: the keys in short, so none is cut off.
        if fmt::width(&full) + 6 > r.width as usize {
            format!("{pos} · ↑↓ · [ ] row · Esc")
        } else {
            full
        }
    } else {
        "[ ] or Shift+↑↓ row · Esc close".to_string()
    };
    let inner = boxed(f, t, r, &title, &bottom);
    Paragraph::new(lines).scroll((off, 0)).render(
        Rect::new(
            inner.x + 1,
            inner.y,
            inner.width.saturating_sub(1),
            inner.height,
        ),
        f.buffer_mut(),
    );
}

/// Split `s` into pieces of at most `w` columns, at spaces when possible.
fn wrap_chars(s: &str, w: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in s.split(' ') {
        let candidate = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if fmt::width(&candidate) <= w {
            cur = candidate;
            continue;
        }
        if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        // A word longer than a line is split.
        let mut piece = String::new();
        for ch in word.chars() {
            if fmt::width(&piece) + fmt::width(&ch.to_string()) > w {
                out.push(std::mem::take(&mut piece));
            }
            piece.push(ch);
        }
        cur = piece;
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(cols: &[Vec<Line<'static>>]) -> String {
        cols.iter()
            .flatten()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn help_describes_what_quitting_does_to_the_session() {
        let t = &crate::theme::all_themes(None).0[0];
        let kept = text(&help_columns(t, 1, false, true, 8));
        assert!(kept.contains("quit (the server session is kept)"));
        let out = text(&help_columns(t, 1, true, true, 8));
        assert!(out.contains("quit and log out on the server"));
        assert!(!out.contains("session is kept"));
    }

    #[test]
    fn help_shows_a_newer_release_and_how_to_upgrade() {
        use crate::update::{CARGO_CMD, CURRENT, UPGRADE_CMD};
        let t = &crate::theme::all_themes(None).0[0];
        for width in [53, 80, 110, 166] {
            let lines = update_section(t, "v9.9.9", width);
            let s = text(std::slice::from_ref(&lines));
            assert!(s.starts_with(&format!("update available  cpatop {CURRENT} → 9.9.9")));
            // Complete, and on one line when there is room.
            let squash = |s: &str| s.replace([' ', '\n'], "");
            assert!(squash(&s).contains(&squash(UPGRADE_CMD)), "{s}");
            assert!(squash(&s).contains(&squash(CARGO_CMD)), "{s}");
            if width > UPGRADE_CMD.len() + 1 {
                assert!(s.contains(UPGRADE_CMD), "{s}");
            }
            for l in &lines {
                assert!(l.width() <= width, "{width}: {l:?}");
                // The privacy guard leaves it alone.
                let txt: String = l.spans.iter().map(|s| s.content.as_ref()).collect();
                assert!(crate::app::sensitive_spans(&txt).is_empty(), "{txt}");
            }
        }
        // A URL longer than the line breaks after a `/`.
        let w = wrap_command(UPGRADE_CMD, 52);
        assert!(
            w.iter()
                .all(|l| l.ends_with('/') || l.ends_with("sh") || l.ends_with("-fsSL")),
            "{w:?}"
        );
    }

    #[test]
    fn api_key_help_lists_only_keys_that_work_for_it() {
        let t = &crate::theme::all_themes(None).0[0];
        let key = text(&help_columns(t, 1, false, false, 3));
        assert!(key.contains("F2…F4"), "{key}");
        for admin_only in [
            "F2…F9",
            "API-key filter",
            "requests tab",
            "model / source",
            "credential names",
            "switch dimension",
        ] {
            assert!(!key.contains(admin_only), "{admin_only} in:\n{key}");
        }
        assert!(key.contains("show every hidden pane"));
        let admin = text(&help_columns(t, 1, false, true, 8));
        for k in [
            "F2…F9",
            "API-key filter",
            "requests tab",
            "credential names",
            "switch dimension",
        ] {
            assert!(admin.contains(k), "{k}");
        }
    }

    #[test]
    fn server_url_change_has_its_own_heading() {
        let h = help_for(true, 8);
        let has = |sec: &str, key: &str| {
            h.iter()
                .any(|(n, rows)| *n == sec && rows.iter().any(|(k, _)| k == key))
        };
        assert!(has("offline / login screen", "b · ^S"));
        assert!(!has("display & session", "b · ^S"));
        assert!(has("display & session", "b"));
        assert!(has("reading the numbers", "cache-hit"));
    }

    #[test]
    fn help_lists_every_provider_abbreviation() {
        let rows = HELP
            .iter()
            .find(|(n, _)| *n == "credential names")
            .map(|(_, r)| *r)
            .unwrap_or_default();
        let words = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
        for (name, abbr) in crate::app::PROVIDER_ABBREVS {
            // In the same row and at the same position.
            let found = rows.iter().any(|(k, d)| {
                let (k, d) = (words(k), words(d));
                k.len() == d.len() && k.iter().zip(&d).any(|(a, n)| a == abbr && n == name)
            });
            assert!(found, "{name} → {abbr}");
        }
    }
}
