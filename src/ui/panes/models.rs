//! Models tab: pricing, catalog and recalculation status.

use super::{frame, placeholder, stale_marker, table_in};
use crate::app::{App, Hit};
use crate::fmt;
use crate::layout::PaneId;
use crate::ui::table::{Align, Col, TRow, col, flex, num, text};
use crate::ui::widgets::meter;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

const PRICE_COLS: [Col; 8] = [
    flex("model", 14),
    col("style", 7, Align::Left, 3),
    col("input", 8, Align::Right, 0),
    col("output", 8, Align::Right, 0),
    col("cache r", 8, Align::Right, 1),
    col("cache w", 8, Align::Right, 1),
    col("mult", 5, Align::Right, 2),
    col("rules", 9, Align::Right, 4),
];

fn price(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("${s}")
}

pub fn pricing(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let l = &app.data.pricing;
    let mut right = stale_marker(app, l);
    if let Some(p) = l.get() {
        right.push(Span::styled(
            format!(
                "{} priced · USD per 1M tokens · rev {}",
                p.models.len(),
                p.config_revision
            ),
            Style::default().fg(t.graph_text),
        ));
    }
    let inner = frame(f, app, PaneId::MoPricing, area, focused, right, None);
    if placeholder(f, app, inner, l) {
        return;
    }
    let Some(p) = l.get() else { return };
    let rows = p
        .models
        .iter()
        .map(|m| {
            let b = &m.base_prices;
            TRow::new(m.model.clone())
                .text(m.model.clone(), text(&m.model))
                .text(m.pricing_style.clone(), text(&m.pricing_style))
                .styled(price(b.input), Style::default().fg(t.title), num(b.input))
                .styled(price(b.output), Style::default().fg(t.title), num(b.output))
                .text(price(b.cache_read), num(b.cache_read))
                .text(price(b.cache_write), num(b.cache_write))
                .text(format!("×{}", m.model_multiplier), num(m.model_multiplier))
                .text(
                    format!(
                        "{}b · {}c",
                        m.branches.len(),
                        m.conditional_multipliers.len()
                    ),
                    num((m.branches.len() + m.conditional_multipliers.len()) as f64),
                )
                .filter(m.model.clone())
        })
        .collect();
    table_in(
        f,
        app,
        hit,
        PaneId::MoPricing,
        inner,
        focused,
        &PRICE_COLS,
        rows,
        "no model prices configured",
    );
}

/// Provider label derived from a model id, like the web catalog.
pub fn provider_of(id: &str, owned_by: Option<&str>) -> String {
    let last = id.rsplit('/').next().unwrap_or(id).to_lowercase();
    let pairs: [(&[&str], &str); 12] = [
        (&["gpt", "chatgpt", "codex", "o1", "o3", "o4"], "OpenAI"),
        (&["claude"], "Claude"),
        (&["gemini", "gemma"], "Gemini"),
        (&["grok"], "xAI"),
        (&["kimi", "moonshot"], "Kimi"),
        (&["llama"], "Meta"),
        (&["deepseek"], "DeepSeek"),
        (&["muse"], "Muse"),
        (&["qwen"], "Qwen"),
        (
            &["mistral", "codestral", "magistral", "devstral", "ministral"],
            "Mistral",
        ),
        (&["minimax"], "MiniMax"),
        (&["glm"], "Z.ai"),
    ];
    for (prefixes, name) in pairs {
        if prefixes.iter().any(|p| last.starts_with(p)) {
            return name.into();
        }
    }
    match owned_by.unwrap_or("") {
        "anthropic" => "Claude".into(),
        "google" => "Gemini".into(),
        "x-ai" => "xAI".into(),
        "" => "other".into(),
        o => o.to_string(),
    }
}

const CAT_COLS: [Col; 3] = [
    flex("model", 14),
    col("provider", 9, Align::Left, 0),
    col("owner", 10, Align::Left, 1),
];

pub fn catalog(f: &mut Frame, app: &App, hit: &mut Hit, area: Rect, focused: bool) {
    let t = app.theme();
    let l = &app.data.catalog;
    let mut right = stale_marker(app, l);
    if let Some(c) = l.get() {
        right.push(Span::styled(
            format!("{} models", c.data.len()),
            Style::default().fg(t.graph_text),
        ));
    }
    let inner = frame(f, app, PaneId::MoCatalog, area, focused, right, None);
    if placeholder(f, app, inner, l) {
        return;
    }
    let Some(c) = l.get() else { return };
    let mut models = c.data.clone();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    let rows = models
        .iter()
        .map(|m| {
            let p = provider_of(&m.id, m.owned_by.as_deref());
            let owner = m.owned_by.clone().unwrap_or_default();
            TRow::new(m.id.clone())
                .text(m.id.clone(), text(&m.id))
                .styled(p.clone(), Style::default().fg(t.accent), text(&p))
                .text(owner.clone(), text(&owner))
                .filter(format!("{} {p} {owner}", m.id))
        })
        .collect();
    table_in(
        f,
        app,
        hit,
        PaneId::MoCatalog,
        inner,
        focused,
        &CAT_COLS,
        rows,
        "catalog is empty",
    );
}

pub fn recalc(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    let t = app.theme();
    let inner = frame(f, app, PaneId::MoRecalc, area, focused, vec![], None);
    if placeholder(f, app, inner, &app.data.recalc) {
        return;
    }
    let k = |s: &str| Span::styled(format!("{s:<10}"), Style::default().fg(t.graph_text));
    let v = |s: String| Span::styled(s, Style::default().fg(t.title));
    let mut lines = vec![];
    match app.data.recalc.get().and_then(|o| o.as_ref()) {
        Some(task) => {
            let color = match task.status.as_str() {
                "running" => t.warn,
                "failed" => t.err,
                _ => t.ok,
            };
            lines.push(Line::from(vec![
                k("last task"),
                Span::styled(
                    task.status.clone(),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" · {}", task.stage),
                    Style::default().fg(t.graph_text),
                ),
            ]));
            let total = task.total_count.unwrap_or(0.0);
            let mut prog = vec![k("progress")];
            prog.extend(meter(
                if total > 0.0 {
                    task.processed_count / total
                } else {
                    0.0
                },
                10,
                t,
                None,
            ));
            prog.push(v(format!(
                " {}/{}",
                fmt::int(task.processed_count),
                fmt::int(total)
            )));
            lines.push(Line::from(prog));
            lines.push(Line::from(vec![
                k("from"),
                v(fmt::ts_fmt(&task.start_at, "%Y-%m-%d %H:%M")),
            ]));
            lines.push(Line::from(vec![
                k("updated"),
                v(fmt::ts_fmt(&task.updated_at, "%Y-%m-%d %H:%M")),
            ]));
            if let Some(e) = &task.error {
                lines.push(Line::from(Span::styled(
                    format!("✗ {}", e.message),
                    Style::default().fg(t.err),
                )));
            }
        }
        None => lines.push(Line::from(vec![
            k("last task"),
            v("none since server start".into()),
        ])),
    }
    if let Some(o) = app.data.recalc_options.get() {
        lines.push(Line::from(vec![
            k("revision"),
            v(o.config_revision.to_string()),
        ]));
        if let Some(e) = &o.earliest_start {
            lines.push(Line::from(vec![
                k("window"),
                v(format!(
                    "{} → {}",
                    fmt::ts_fmt(e, "%m-%d %H:%M"),
                    o.latest_start
                        .as_deref()
                        .map(|s| fmt::ts_fmt(s, "%m-%d %H:%M"))
                        .unwrap_or_default()
                )),
            ]));
        }
        lines.push(Line::from(vec![k("timezone"), v(o.timezone.clone())]));
    }
    f.render_widget(
        Paragraph::new(lines),
        Rect::new(
            inner.x + 1,
            inner.y,
            inner.width.saturating_sub(1),
            inner.height,
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::provider_of;

    #[test]
    fn providers() {
        assert_eq!(provider_of("gpt-6-luna", None), "OpenAI");
        assert_eq!(
            provider_of("vertex_ai/claude-3-5-haiku@20241022", None),
            "Claude"
        );
        assert_eq!(provider_of("bonsai", Some("google")), "Gemini");
        assert_eq!(provider_of("bonsai", None), "other");
    }
}
