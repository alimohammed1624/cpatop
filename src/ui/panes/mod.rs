//! Per-pane renderers.

mod admin;
mod analysis;
mod events;
mod overview;
mod realtime;

use super::table::{self, Col, TRow};
use super::widgets::{self, pane_block};
use crate::app::{App, Hit, Loaded};
use crate::fmt;
use crate::layout::{self, PaneId};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

pub fn draw(f: &mut Frame, app: &App, hit: &mut Hit, id: PaneId, area: Rect, focused: bool) {
    use PaneId::*;
    match id {
        OvTotals => overview::totals(f, app, area, focused),
        OvTrend => overview::trend(f, app, area, focused),
        OvActivity => overview::activity(f, app, area, focused),
        OvDist => overview::distribution(f, app, hit, area, focused),
        RtSummary => realtime::summary(f, app, area, focused),
        RtThroughput => realtime::throughput(f, app, area, focused),
        RtOutcomes => realtime::outcomes(f, app, area, focused),
        RtLatency => realtime::latency(f, app, area, focused),
        RtShare => realtime::share(f, app, hit, area, focused),
        RtSpeed => realtime::speed(f, app, hit, area, focused),
        RtCache => realtime::cache(f, app, area, focused),
        AnTokens => analysis::tokens(f, app, area, focused),
        AnComposition => analysis::composition(f, app, hit, area, focused),
        AnModels => analysis::models(f, app, hit, area, focused),
        AnLatency => analysis::latency(f, app, area, focused),
        AnSpeed => analysis::speed(f, app, hit, area, focused),
        AnEfficiency => analysis::efficiency(f, app, hit, area, focused),
        AnHeatmap => analysis::heatmap(f, app, hit, area, focused),
        EvLog => events::log(f, app, hit, area, focused),
        EvDetail => events::detail(f, app, hit, area, focused),
        CrList => admin::credentials(f, app, hit, area, focused),
        CrDetail => admin::credential_detail(f, app, hit, area, focused),
        QuInspection => admin::inspection(f, app, area, focused),
        QuList => admin::quota(f, app, hit, area, focused),
        QuHistory => admin::quota_history(f, app, hit, area, focused),
        MoPricing => admin::pricing(f, app, hit, area, focused),
        MoCatalog => admin::catalog(f, app, hit, area, focused),
        MoRecalc => admin::recalc(f, app, area, focused),
        StKeys => admin::api_keys(f, app, hit, area, focused),
        StSessions => admin::sessions(f, app, hit, area, focused),
        StServer => admin::server(f, app, area, focused),
    }
}

/// Draw the pane border and return the inner area.
pub fn frame(
    f: &mut Frame,
    app: &App,
    id: PaneId,
    area: Rect,
    focused: bool,
    mut right: Vec<Span<'static>>,
    bottom: Option<(String, String)>,
) -> Rect {
    let t = app.theme();
    let (_, num, def) = layout::pane_def(id);
    if def.table {
        let st = app.table(id);
        if !st.filter.is_empty() {
            if !right.is_empty() {
                right.push(Span::styled(" ", Style::default()));
            }
            right.push(Span::styled(
                format!("/{}", st.filter),
                Style::default().fg(t.hi).add_modifier(Modifier::BOLD),
            ));
        }
    }
    // Keep the right title from running into the left one.
    let left_w = fmt::width(def.name) + 6 + if focused { 2 } else { 0 };
    let avail = (area.width as usize).saturating_sub(left_w + 4);
    let right = fit_spans(right, avail);
    let block = pane_block(t, num, def.name, focused, right, bottom);
    let inner = block.inner(area);
    f.render_widget(block, area);
    inner
}

/// Truncate spans to `max` display columns.
pub fn fit_spans(spans: Vec<Span<'static>>, max: usize) -> Vec<Span<'static>> {
    let total: usize = spans.iter().map(|s| s.width()).sum();
    if total <= max {
        return spans;
    }
    let mut out = Vec::new();
    let mut used = 0;
    for s in spans {
        let w = s.width();
        if used + w <= max.saturating_sub(1) {
            used += w;
            out.push(s);
        } else {
            let room = max.saturating_sub(used);
            if room > 1 {
                out.push(Span::styled(fmt::trunc(&s.content, room), s.style));
            }
            break;
        }
    }
    out
}

/// Draw a loading / error placeholder when there is no data. Returns true if drawn.
pub fn placeholder<T>(f: &mut Frame, app: &App, inner: Rect, l: &Loaded<T>) -> bool {
    if l.data.is_some() {
        return false;
    }
    let t = app.theme();
    match &l.error {
        Some(e) => widgets::message(f.buffer_mut(), inner, &format!("✗ {e}"), t.err),
        None => widgets::message(f.buffer_mut(), inner, "loading…", t.graph_text),
    }
    true
}

/// Right-title marker when data is present but the last refresh failed.
pub fn stale_marker<T>(app: &App, l: &Loaded<T>) -> Vec<Span<'static>> {
    match (&l.data, &l.error) {
        (Some(_), Some(e)) => vec![Span::styled(
            format!("▲ {}", fmt::trunc(e, 30)),
            Style::default().fg(app.theme().err),
        )],
        _ => vec![],
    }
}

/// Render a table with selection tracking.
#[allow(clippy::too_many_arguments)]
pub fn table_in(
    f: &mut Frame,
    app: &App,
    hit: &mut Hit,
    id: PaneId,
    inner: Rect,
    focused: bool,
    cols: &[Col],
    rows: Vec<TRow>,
    empty: &str,
) {
    let st = app.table(id);
    let rows = table::order(rows, st);
    if rows.is_empty() {
        st.len.set(0);
        st.cols.set(cols.len());
        let msg = if st.filter.is_empty() {
            empty.to_string()
        } else {
            format!("no rows match /{}", st.filter)
        };
        widgets::message(f.buffer_mut(), inner, &msg, app.theme().graph_text);
        return;
    }
    let body = table::render(f.buffer_mut(), inner, app.theme(), cols, &rows, st, focused);
    hit.bodies.push((id, body));
    let sel = st.selected.min(rows.len() - 1);
    hit.selected.insert(id, rows[sel].id.clone());
}

/// Dimension switcher spans (`models│keys│…`) for the right title.
pub fn dim_tabs(app: &App, id: PaneId, width: u16) -> Vec<Span<'static>> {
    let t = app.theme();
    let dims = crate::app::dims_for(id, app.role);
    if dims.len() <= 1 {
        return vec![];
    }
    let cur = app.dim(id);
    let full: usize = dims.iter().map(|d| d.len() + 1).sum();
    let name_w = layout::pane_def(id).2.name.len() + 12;
    if full + name_w > width as usize {
        return vec![
            Span::styled("d ", Style::default().fg(t.graph_text)),
            Span::styled(
                format!("‹{}›", dims[cur]),
                Style::default().fg(t.hi).add_modifier(Modifier::BOLD),
            ),
        ];
    }
    let mut out = Vec::new();
    for (i, d) in dims.iter().enumerate() {
        if i > 0 {
            out.push(Span::styled("│", Style::default().fg(t.border)));
        }
        let style = if i == cur {
            Style::default().fg(t.hi).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.inactive)
        };
        out.push(Span::styled(d.to_string(), style));
    }
    out
}

/// Display name for an API key label: keep aliases, redact masked keys.
pub fn key_label(s: &str) -> String {
    fmt::redact_key(s)
}
