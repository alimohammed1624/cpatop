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

const HELP: &[(&str, &[(&str, &str)])] = &[
    (
        "navigate",
        &[
            ("Tab S-Tab ←→", "next / previous tab (or click)"),
            ("F2…F9", "jump to a tab"),
            ("f / F", "focus next / previous table or graph"),
            ("1-9", "hide / show pane by number"),
            ("p l / P", "next / previous layout"),
            ("?", "this help · q quit · Ctrl-C quit"),
            ("Esc", "close / clear the innermost thing"),
        ],
    ),
    (
        "tables",
        &[
            ("↑↓ j k", "move selection (mouse wheel too)"),
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
            ("r", "range picker (presets, custom days/hours)"),
            ("[ ]", "previous / next preset range"),
            ("a", "API-key filter picker (admin)"),
            ("w", "realtime window 15m / 30m / 60m"),
            ("R", "refresh now · + - refresh interval"),
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
        "display & session",
        &[
            ("t / T", "next / previous theme"),
            ("b", "graph symbols braille / block / ascii"),
            ("x", "privacy mode (mask e-mails)"),
            ("X X", "log out (press twice within 3 s)"),
        ],
    ),
];

pub fn help(f: &mut Frame, app: &App) {
    let t = app.theme();
    let area = f.area();
    let section = |name: &str, rows: &[(&str, &str)]| -> Vec<Line<'static>> {
        let mut v = vec![Line::from(Span::styled(
            name.to_string(),
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ))];
        for (k, d) in rows {
            v.push(Line::from(vec![
                Span::styled(
                    format!(" {k:<14}"),
                    Style::default().fg(t.hi).add_modifier(Modifier::BOLD),
                ),
                Span::styled(d.to_string(), Style::default().fg(t.fg)),
            ]));
        }
        v
    };
    let two = area.width >= 112;
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for (i, (name, rows)) in HELP.iter().enumerate() {
        let target = if two && i % 2 == 1 {
            &mut right
        } else {
            &mut left
        };
        if !target.is_empty() {
            target.push(Line::from(""));
        }
        target.extend(section(name, rows));
    }
    let col_w = 54u16;
    let w = if two { col_w * 2 + 3 } else { col_w + 2 };
    let h = left.len().max(right.len()) as u16 + 3;
    let r = centered(area, w, h);
    let inner = boxed(
        f,
        t,
        r,
        "help",
        &format!("cpatop {} · any key closes", env!("CARGO_PKG_VERSION")),
    );
    let lw = if two { col_w } else { inner.width };
    Paragraph::new(left).render(
        Rect::new(inner.x + 1, inner.y, lw.min(inner.width), inner.height),
        f.buffer_mut(),
    );
    if two {
        Paragraph::new(right).render(
            Rect::new(
                inner.x + col_w + 2,
                inner.y,
                inner.width.saturating_sub(col_w + 2),
                inner.height,
            ),
            f.buffer_mut(),
        );
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
        Span::styled(p.query.clone(), Style::default().fg(t.title)),
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
        field("start", &ed.start, ed.field == 0),
        field("end", &ed.end, ed.field == 1),
        Line::from(""),
    ];
    match &ed.error {
        Some(e) => lines.push(Line::from(Span::styled(
            format!(" ✗ {e}"),
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
