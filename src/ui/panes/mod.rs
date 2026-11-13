//! Per-pane renderers, pane sizing and shared pane helpers.

mod activity;
mod analysis;
mod credentials;
mod events;
mod keys;
mod models;
mod overview;
mod quota;
mod realtime;

use super::table::{self, Col, TRow};
use super::widgets::{self, Axes, Graph, pane_block};
use crate::api::Role;
use crate::app::{App, Hit, Loaded};
use crate::config::GraphSymbols;
use crate::fmt;
use crate::layout::{self, PaneId, Size};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub fn draw(f: &mut Frame, app: &App, hit: &mut Hit, id: PaneId, area: Rect, focused: bool) {
    use PaneId::*;
    match id {
        OvTotals => overview::totals(f, app, area, focused),
        OvTrend => overview::trend(f, app, hit, area, focused),
        OvActivity => activity::activity(f, app, area, focused),
        OvDist => overview::distribution(f, app, hit, area, focused),
        RtSummary => realtime::summary(f, app, area, focused),
        RtThroughput => realtime::throughput(f, app, hit, area, focused),
        RtOutcomes => realtime::outcomes(f, app, hit, area, focused),
        RtLatency => realtime::latency(f, app, area, focused),
        RtShare => realtime::share(f, app, hit, area, focused),
        RtSpeed => realtime::speed(f, app, hit, area, focused),
        RtCache => realtime::cache(f, app, hit, area, focused),
        AnTokens => analysis::tokens(f, app, hit, area, focused),
        AnComposition => analysis::composition(f, app, hit, area, focused),
        AnModels => analysis::models(f, app, hit, area, focused),
        AnLatency => analysis::latency(f, app, area, focused),
        AnSpeed => analysis::speed(f, app, hit, area, focused),
        AnEfficiency => analysis::efficiency(f, app, hit, area, focused),
        AnHeatmap => analysis::heatmap(f, app, hit, area, focused),
        EvLog => events::log(f, app, hit, area, focused),
        EvDetail => events::detail(f, app, hit, area, focused),
        CrList => credentials::list(f, app, hit, area, focused),
        CrDetail => credentials::detail(f, app, hit, area, focused),
        QuInspection => quota::inspection(f, app, area, focused),
        QuList => quota::list(f, app, hit, area, focused),
        QuHistory => quota::history(f, app, hit, area, focused),
        MoPricing => models::pricing(f, app, hit, area, focused),
        MoCatalog => models::catalog(f, app, hit, area, focused),
        MoRecalc => models::recalc(f, app, area, focused),
        StKeys => keys::api_keys(f, app, hit, area, focused),
        StSessions => keys::sessions(f, app, hit, area, focused),
        StServer => keys::server(f, app, area, focused),
    }
}

/// Tallest useful single-chart graph pane (outer rows).
const GRAPH_MAX: u16 = 18;
/// Tallest useful pane holding two stacked charts.
const GRAPH2_MAX: u16 = 32;

fn with_width(s: Size, w: Option<u16>) -> Size {
    match w {
        Some(w) => s.max_width(w),
        None => s,
    }
}

/// Height needs of a pane at outer width `w` (see [`layout::compute`]).
pub fn size(app: &App, id: PaneId, w: u16) -> Size {
    use PaneId::*;
    let inner_w = w.saturating_sub(3);
    // A table: header + rows + borders, at least a few rows.
    let table = |rows: Option<usize>| match rows {
        Some(n) => Size::range(5, (n as u16).saturating_add(3).max(5)),
        None => Size::at_least(5),
    };
    match id {
        OvTotals => Size::exact(overview::totals_lines(app, inner_w).max(1) + 2),
        // Graphs stop growing at a readable height on large terminals; the
        // rows go to tables, charts of the selected row and detail panes.
        OvTrend => Size::range(9, GRAPH2_MAX),
        OvActivity => activity::activity_size(app, w),
        // Spare rows chart the selected row's tokens.
        OvDist => table(overview::dist_len(app)).soaking(),
        RtSummary => Size::exact(realtime::summary_lines(app, inner_w).max(1) + 2),
        RtThroughput => Size::range(8, GRAPH2_MAX),
        RtOutcomes | RtCache => Size::range(7, GRAPH_MAX),
        RtLatency | AnLatency => Size::range(7, GRAPH_MAX).plot(),
        RtShare => table(realtime::share_len(app)),
        RtSpeed => table(realtime::speed_len(app)),
        AnTokens => Size::range(9, GRAPH_MAX + 2),
        AnComposition => table(analysis::composition_len(app)),
        AnModels => table(analysis::models_len(app)),
        // Short tables leave the rest of their row to the neighbour.
        AnSpeed => with_width(table(analysis::speed_len(app)), analysis::speed_width(app)),
        AnEfficiency => table(analysis::efficiency_len(app)),
        AnHeatmap => with_width(
            table(analysis::heatmap_len(app)),
            analysis::heatmap_width(app),
        ),
        EvLog => Size::at_least(8),
        EvDetail => Size::range(6, events::detail_lines(app, w.saturating_sub(3)) + 2),
        CrList => table(app.data.identities.get().map(|p| p.identities.len())),
        CrDetail => Size::at_least(8),
        QuInspection => Size::exact(quota::inspection_lines(app, inner_w).max(1) + 2),
        // Spare rows chart the selected credential's recent requests.
        QuList if quota::list_has_chart(app) => table(quota::list_len(app)).soaking(),
        QuList => table(quota::list_len(app)),
        QuHistory => quota::history_size(app),
        MoPricing | MoCatalog => Size::at_least(6),
        MoRecalc => with_width(Size::range(5, 10), models::recalc_width(app)),
        // Spare rows chart the selected key's tokens.
        StKeys => table(app.data.api_keys.get().map(|k| k.items.len())).soaking(),
        StSessions => table(app.data.sessions.get().map(|s| s.items.len())),
        StServer => Size::range(5, keys::server_lines(app, w.saturating_sub(4)) + 2)
            .max_width(keys::server_width(app)),
    }
}

/// Keys a pane reacts to, shown in its bottom border while focused.
fn hints(app: &App, id: PaneId) -> &'static str {
    use PaneId::*;
    let admin = app.role == Some(Role::Admin);
    match id {
        OvTrend | RtThroughput | RtOutcomes | RtCache | AnTokens => {
            "←→ cursor · g/G ends · Esc clear · Tab next tab"
        }
        OvDist | AnComposition | RtShare if crate::app::dims_for(id, app.role).len() > 1 => {
            "d dimension · s/S sort · / filter · ↑↓ select · ⏎ row"
        }
        EvLog if admin => "e result · m model · o source · a key · c clear · / find",
        CrList | QuList => "↑↓ select · ⏎ row · s/S sort · / filter · f focus",
        _ if layout::pane_def(id).2.table => "↑↓ select · ⏎ row · s/S sort · / filter",
        _ => "",
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
    let (_, _, def) = layout::pane_def(id);
    let num = app.pane_number(id);
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
    // The focused pane lists its keys in the bottom border (btop-style).
    let bottom = match (focused, hints(app, id)) {
        (true, h) if !h.is_empty() => {
            let r = bottom.map(|b| b.1).unwrap_or_default();
            let room = (area.width as usize).saturating_sub(fmt::width(&r) + 10);
            Some((fmt::trunc(h, room), r))
        }
        _ => bottom,
    };
    let block = pane_block(t, num, def.name, focused, right, bottom);
    let inner = block.inner(area);
    f.render_widget(block, area);
    inner
}

/// A pane collapsed to a single title row for lack of space.
pub fn collapsed(f: &mut Frame, app: &App, id: PaneId, area: Rect) {
    let t = app.theme();
    let (_, _, def) = layout::pane_def(id);
    let num = app.pane_number(id);
    let bs = Style::default().fg(t.border);
    let mut spans = vec![
        Span::styled("─", bs),
        Span::styled(
            fmt::superscript(num),
            Style::default().fg(t.hi).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {} ", def.name),
            Style::default().fg(t.title).add_modifier(Modifier::BOLD),
        ),
    ];
    let used: usize = spans.iter().map(|s| s.width()).sum();
    // A one-line summary keeps the pane's key numbers visible.
    let note = match collapsed_summary(app, id) {
        Some(s) => format!(" ▸ {s} "),
        None => " ▸ more rows needed ".to_string(),
    };
    let note = fmt::trunc(&note, (area.width as usize).saturating_sub(used + 2));
    let fill = (area.width as usize).saturating_sub(used + fmt::width(&note) + 1);
    spans.push(Span::styled(note, Style::default().fg(t.fg)));
    spans.push(Span::styled("─".repeat(fill), bs));
    spans.push(Span::styled("─", bs));
    let spans = fit_spans(spans, area.width as usize);
    f.buffer_mut()
        .set_line(area.x, area.y, &Line::from(spans), area.width);
}

/// One-line summary of a pane collapsed for lack of rows.
fn collapsed_summary(app: &App, id: PaneId) -> Option<String> {
    use PaneId::*;
    let top = |items: Vec<(String, f64)>| -> String {
        let total: f64 = items.iter().map(|i| i.1).sum();
        items
            .iter()
            .take(3)
            .map(|(n, v)| {
                format!(
                    "{} {}",
                    fmt::trunc(&app.privacy(n), 18),
                    fmt::pct(fmt::ratio_pct(*v, total).unwrap_or(0.0), 0)
                )
            })
            .collect::<Vec<_>>()
            .join(" · ")
    };
    let d = &app.data;
    Some(match id {
        RtLatency => {
            let l = &d.realtime.get()?.latency_scatter;
            format!(
                "p95 ttft {} · p95 latency {} · n {}",
                fmt::ms(l.p95_ttft_ms),
                fmt::ms(l.p95_latency_ms),
                fmt::compact(l.total_points)
            )
        }
        AnLatency => {
            let l = d.latency.get()?;
            format!(
                "p95 ttft {} · p95 latency {} · n {}",
                fmt::ms(l.p95_ttft_ms),
                fmt::ms(l.p95_latency_ms),
                fmt::compact(l.total_points)
            )
        }
        RtShare => top(d
            .realtime
            .get()?
            .current_usage
            .models
            .iter()
            .map(|i| (i.label.clone(), i.tokens))
            .collect()),
        RtSpeed => d
            .realtime
            .get()?
            .model_speed
            .series
            .iter()
            .take(3)
            .map(|s| {
                format!(
                    "{} {}",
                    fmt::trunc(&app.privacy(&s.model), 18),
                    s.second_half.mean_tps.map(fmt::tps).unwrap_or("-".into())
                )
            })
            .collect::<Vec<_>>()
            .join(" · "),
        RtCache => {
            let r = d.realtime.get()?;
            let v: Vec<f64> = r
                .cache_level
                .iter()
                .filter_map(|c| c.cache_read_rate)
                .collect();
            format!(
                "cache read {} now · avg {}",
                v.last().map(|x| fmt::pct(*x, 1)).unwrap_or("-".into()),
                fmt::pct(v.iter().sum::<f64>() / v.len().max(1) as f64, 1)
            )
        }
        RtOutcomes => {
            let o = &d.realtime.get()?.insights.as_ref()?.outcomes;
            let r: f64 = o.iter().map(|x| x.requests).sum();
            let f: f64 = o.iter().map(|x| x.failures).sum();
            format!("ok {} · fail {}", fmt::int(r - f), fmt::int(f))
        }
        OvDist => top(d
            .comparisons
            .get()?
            .models
            .iter()
            .map(|i| (i.label.clone(), i.total_tokens))
            .collect()),
        OvActivity => {
            let a = d.activity.get()?;
            format!(
                "health {} · ✓{} ✗{}",
                fmt::pct(a.success_rate, 1),
                fmt::int(a.total_success),
                fmt::int(a.total_failure)
            )
        }
        AnComposition | AnModels | AnEfficiency => top(d
            .analysis
            .get()?
            .model_composition
            .iter()
            .map(|i| (i.label.clone(), i.total_tokens))
            .collect()),
        AnSpeed => {
            let a = d.analysis.get()?;
            a.model_speed
                .series
                .iter()
                .take(3)
                .map(|s| {
                    let sum: f64 = s.samples.iter().map(|x| x.speed_tps_sum).sum();
                    let n: f64 = s.samples.iter().map(|x| x.speed_sample_count).sum();
                    let mean = (n > 0.0).then(|| sum / n);
                    format!(
                        "{} {}",
                        fmt::trunc(&app.privacy(&s.model), 18),
                        mean.map(fmt::tps).unwrap_or("-".into())
                    )
                })
                .collect::<Vec<_>>()
                .join(" · ")
        }
        AnHeatmap => {
            let h = &d.analysis.get()?.heatmap;
            format!(
                "{} × {}",
                fmt::plural(h.api_keys.len(), "key", "keys"),
                fmt::plural(h.models.len(), "model", "models")
            )
        }
        _ if layout::pane_def(id).2.table => {
            let n = app.table(id).len.get();
            fmt::plural(n, "row", "rows")
        }
        _ => return None,
    })
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
        Some(e) => widgets::message(
            f.buffer_mut(),
            inner,
            &format!("✗ {}", app.privacy_text(e)),
            t.err,
        ),
        None => widgets::message(f.buffer_mut(), inner, "loading…", t.graph_text),
    }
    true
}

/// Right-title marker when data is present but the last refresh failed, or
/// when it is older than the tab's refresh cadence allows.
pub fn stale_marker<T>(app: &App, l: &Loaded<T>) -> Vec<Span<'static>> {
    match (&l.data, &l.error) {
        (Some(_), Some(e)) => vec![Span::styled(
            format!("▲ {}", fmt::trunc(&app.privacy_text(e), 30)),
            Style::default().fg(app.theme().err),
        )],
        (Some(_), None) => match app.is_stale(l.at) {
            Some(age) => vec![Span::styled(
                format!("◌ stale {}", fmt::age(age)),
                Style::default().fg(app.theme().warn),
            )],
            None => vec![],
        },
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
    hit.row_fields.insert(id, row_fields(cols, &rows[sel]));
}

/// Fields of a row for the popup: each column under its spelled-out title,
/// with the row's detail fields replacing cells of the same name (exact
/// numbers instead of `3.2M`) and the rest following. Blank values and
/// placeholders (`-`, `—`) are left out.
pub fn row_fields(cols: &[Col], row: &TRow) -> Vec<(String, String)> {
    let mut fields: Vec<(String, String)> = if row.detail_only {
        Vec::new()
    } else {
        cols.iter()
            .zip(&row.cells)
            .filter_map(|(c, cell)| Some((table::long_title(&c.title), cell.text()?)))
            .collect()
    };
    for (k, v) in &row.detail {
        match fields.iter_mut().find(|(f, _)| f == k) {
            Some(f) => f.1 = v.clone(),
            None => fields.push((k.clone(), v.clone())),
        }
    }
    fields
        .into_iter()
        .map(|(k, v)| (k, v.trim().to_string()))
        .filter(|(_, v)| !matches!(v.as_str(), "" | "-" | "—" | "--"))
        .collect()
}

/// A server timestamp in full for the row popup, with how long ago (or how
/// far ahead) it is.
pub fn when(app: &App, s: Option<&str>) -> String {
    let Some(s) = s.filter(|s| !s.is_empty()) else {
        return String::new();
    };
    let now = app.clock.utc();
    // In the offset every other time is shown in (servers send some
    // timestamps in UTC and others in their own zone).
    let off = *app.server_now().offset();
    let full = match fmt::parse_ts(s) {
        Some(t) => t
            .with_timezone(&off)
            .format("%Y-%m-%d %H:%M:%S %:z")
            .to_string(),
        None => fmt::ts_fmt(s, "%Y-%m-%d %H:%M:%S %:z"),
    };
    let full = app.privacy_text(&full);
    match (fmt::since(s, now), fmt::until(s, now)) {
        (Some(a), _) if a > 0 => format!("{full} ({} ago)", fmt::age(a)),
        (_, Some(u)) if u > 0 => format!("{full} (in {})", fmt::secs(u as f64)),
        _ => full,
    }
}

/// Exact token counts for the row popup.
pub fn token_details(
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
    reasoning: f64,
    total: f64,
) -> Vec<(&'static str, String)> {
    vec![
        ("tokens", fmt::int(total)),
        ("input tokens", fmt::int(input)),
        ("output tokens", fmt::int(output)),
        ("cache read tokens", fmt::int(cache_read)),
        ("cache write tokens", fmt::int(cache_write)),
        ("reasoning tokens", fmt::int(reasoning)),
    ]
}

/// The id behind a usage row of a dimension (`api keys`, `credentials`…)
/// for the row popup: key ids as `#3`, credential identities as opaque ids.
pub fn dim_key_field(app: &App, dim: &str, key: &str) -> Vec<(&'static str, String)> {
    match dim {
        "api keys" => vec![("key id", format!("#{key}"))],
        "credentials" => {
            let mut v = vec![("auth index", app.opaque_id(key))];
            if let Some(i) = app.identity_for(key) {
                v.extend([
                    ("credential id", format!("#{}", i.id)),
                    ("type", crate::app::ident_kind(i)),
                    ("provider", app.privacy(&i.provider)),
                ]);
            }
            v
        }
        _ => vec![],
    }
}

/// Request counts with the success rate, for the row popup.
pub fn request_details(requests: f64, failures: f64) -> Vec<(&'static str, String)> {
    vec![
        ("requests", fmt::int(requests)),
        ("failures", fmt::int(failures)),
        (
            "success rate",
            fmt::ratio_pct(requests - failures, requests)
                .map(|p| fmt::pct(p, 2))
                .unwrap_or_default(),
        ),
    ]
}

/// `yes` / `no`.
pub fn yes_no(b: bool) -> String {
    if b { "yes" } else { "no" }.to_string()
}

/// A chart of the selected table row, drawn under the table when the pane
/// has rows to spare (btop-style: leftover space shows more detail).
pub struct RowChart {
    pub title: String,
    /// One label per bucket (time).
    pub labels: Vec<String>,
    pub values: Vec<f64>,
    pub fmt: fn(f64) -> String,
    pub color: Option<Color>,
}

/// Rows a chart needs to be worth drawing (title, plot, axis).
const ROW_CHART_MIN: u16 = 7;

/// [`table_in`], plus a chart of the selected row in the space the table
/// does not need. `chart` maps the selected row id to its series.
#[allow(clippy::too_many_arguments)]
pub fn table_with_chart(
    f: &mut Frame,
    app: &App,
    hit: &mut Hit,
    id: PaneId,
    inner: Rect,
    focused: bool,
    cols: &[Col],
    rows: Vec<TRow>,
    empty: &str,
    chart_for: &dyn Fn(&str) -> Option<RowChart>,
) {
    let need = rows.len() as u16 + 1;
    let spare = inner.height.saturating_sub(need + 1);
    if rows.is_empty() || spare < ROW_CHART_MIN {
        table_in(f, app, hit, id, inner, focused, cols, rows, empty);
        return;
    }
    table_in(
        f,
        app,
        hit,
        id,
        Rect::new(inner.x, inner.y, inner.width, need),
        focused,
        cols,
        rows,
        empty,
    );
    let Some(sel) = hit.selected.get(&id).cloned() else {
        return;
    };
    let Some(c) = chart_for(&sel) else { return };
    let area = Rect::new(
        inner.x + 1,
        inner.y + need + 1,
        inner.width.saturating_sub(1),
        spare,
    );
    draw_row_chart(f, app, hit, id, area, &c);
}

/// Draw a [`RowChart`] (title line plus a chart) into `area`.
pub fn draw_row_chart(
    f: &mut Frame,
    app: &App,
    hit: &mut Hit,
    id: PaneId,
    area: Rect,
    c: &RowChart,
) {
    let t = app.theme();
    let total: f64 = c.values.iter().filter(|v| v.is_finite()).sum();
    let peak = c
        .values
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(0.0, f64::max);
    // Title on the left, the series' totals on the right (after the title
    // when the line is too short to keep them apart).
    let title = Line::from(vec![
        Span::styled("▸ ", Style::default().fg(t.hi)),
        Span::styled(
            c.title.clone(),
            Style::default().fg(t.title).add_modifier(Modifier::BOLD),
        ),
    ]);
    let stats = format!("Σ {}  peak {}", (c.fmt)(total), (c.fmt)(peak));
    let (tw, sw) = (title.width() as u16, fmt::width(&stats) as u16);
    let buf = f.buffer_mut();
    buf.set_line(area.x, area.y, &title, area.width);
    let sx = if tw + 2 + sw <= area.width {
        area.x + area.width - sw
    } else {
        area.x + tw + 2
    };
    if sx < area.right() {
        buf.set_stringn(
            sx,
            area.y,
            &stats,
            (area.right() - sx) as usize,
            Style::default().fg(t.graph_text),
        );
    }
    let g = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(1),
    );
    if peak <= 0.0 {
        widgets::message(f.buffer_mut(), g, "no activity in this range", t.graph_text);
        return;
    }
    chart(
        f,
        app,
        hit,
        id,
        g,
        &c.labels,
        &Series {
            values: &c.values,
            fmt: c.fmt,
            color: c.color,
            max: None,
            floor: 0.0,
        },
        app.settings.graph_symbols,
        false,
    );
}

/// What an empty multi-dimension table says: for the AI-provider
/// dimension, that the server has no API-key AI providers when the
/// credential list shows none.
pub fn empty_dim_text(app: &App, id: PaneId, empty: &str) -> String {
    let dims = crate::app::dims_for(id, app.role);
    if dims.get(app.dim(id)) != Some(&"AI providers") {
        return empty.to_string();
    }
    let none = app
        .data
        .identities
        .get()
        .is_some_and(|p| !p.identities.iter().any(|i| i.auth_type == 2));
    if none {
        "no API-key AI providers configured".into()
    } else {
        format!("{empty} for API-key AI providers")
    }
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

/// The cursor bucket of a graph pane, if one is set.
pub fn cursor(app: &App, id: PaneId, n: usize) -> Option<usize> {
    (n > 0)
        .then(|| app.cursor.get(&id).map(|c| (*c).min(n - 1)))
        .flatten()
}

/// One series of a time chart.
pub struct Series<'a> {
    pub values: &'a [f64],
    pub fmt: fn(f64) -> String,
    pub color: Option<Color>,
    /// Fixed top of the scale (e.g. 100 for percentages).
    pub max: Option<f64>,
    /// Bottom of the scale (values are drawn relative to it).
    pub floor: f64,
}

/// Draw a time chart with y-axis labels, x-axis time ticks and the pane's
/// cursor; returns the plot area. `labels` holds one label per bucket.
#[allow(clippy::too_many_arguments)]
pub fn chart(
    f: &mut Frame,
    app: &App,
    hit: &mut Hit,
    id: PaneId,
    area: Rect,
    labels: &[String],
    s: &Series,
    symbols: GraphSymbols,
    record: bool,
) -> Rect {
    let t = app.theme();
    let n = s.values.len();
    let peak = s
        .values
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(0.0, f64::max);
    let whole = integral(s.values) && s.floor.fract() == 0.0;
    let top = s
        .max
        .unwrap_or_else(|| axis_top(peak - s.floor, whole) + s.floor);
    // Hourly buckets that start off the hour get their ticks at whole hours.
    let hours = offset_hours(labels);
    let axes = Axes {
        theme: t,
        y_min: s.floor,
        y_max: top,
        y_fmt: s.fmt,
        integral: whole,
        labels: if hours.is_some() { &[] } else { labels },
    };
    let plot = if area.height >= 4 {
        axes.draw(f.buffer_mut(), area)
    } else {
        area
    };
    if let Some((frac, hours)) = &hours
        && plot != area
    {
        let dim = Style::default().fg(t.graph_text);
        for (x, label) in hour_ticks(hours, frac, plot.width as usize) {
            f.buffer_mut()
                .set_string(plot.x + x as u16, plot.bottom(), label, dim);
        }
    }
    let shifted: Vec<f64> = s
        .values
        .iter()
        .map(|v| {
            if v.is_finite() {
                (v - s.floor).max(0.0)
            } else {
                *v
            }
        })
        .collect();
    f.render_widget(
        Graph {
            data: &shifted,
            theme: t,
            symbols,
            max: Some((top - s.floor).max(f64::MIN_POSITIVE)),
            color: s.color,
        },
        plot,
    );
    if record {
        hit.graphs.push((id, plot, n));
        if let Some(last) = s.values.iter().rposition(|v| v.is_finite() && *v > 0.0) {
            hit.graph_last.insert(id, last);
        }
    }
    if let Some(i) = cursor(app, id, n) {
        widgets::draw_cursor(f.buffer_mut(), plot, i, n, t);
    }
    plot
}

/// An hourly axis whose buckets start off the hour: the server cuts hourly
/// buckets on UTC hours, so in a +05:30 zone they read `23:30`, `00:30`, …
/// Returns, per bucket, where its whole local hour falls inside it (`0.5`
/// for +05:30) and that hour, or `None` for any other axis (on-the-hour
/// buckets, minutes, days, gaps). A first bucket relabelled to local
/// midnight (see `series::clip_first`) has its hour at its start.
fn offset_hours(labels: &[String]) -> Option<(Vec<f64>, Vec<u32>)> {
    if labels.len() < 2 {
        return None;
    }
    let hm = labels
        .iter()
        .map(|l| {
            let (h, m) = l.split_once(':').filter(|_| l.len() == 5)?;
            Some((h.parse::<u32>().ok()?, m.parse::<u32>().ok()?))
        })
        .collect::<Option<Vec<_>>>()?;
    let clipped = hm.len() >= 3 && hm[0].1 == 0 && hm[1].1 != 0 && hm[1].0 == hm[0].0;
    let rest = if clipped { &hm[1..] } else { &hm[..] };
    let m = rest[0].1;
    let hourly = rest
        .windows(2)
        .all(|w| w[1].1 == m && w[1].0 == (w[0].0 + 1) % 24);
    if m == 0 || m >= 60 || rest[0].0 >= 24 || !hourly {
        return None;
    }
    let frac = f64::from(60 - m) / 60.0;
    let mut fracs = Vec::with_capacity(hm.len());
    let mut hours = Vec::with_capacity(hm.len());
    if clipped {
        fracs.push(0.0);
        hours.push(hm[0].0);
    }
    for (h, _) in rest {
        fracs.push(frac);
        hours.push((h + 1) % 24);
    }
    Some((fracs, hours))
}

/// Tick positions (column, label) for whole hours on an offset hourly axis
/// of `width` columns: every 1, 2, 3, 4, 6, 8, 12 or 24 hours, as many as fit
/// (at most five), each label starting at its hour.
fn hour_ticks(hours: &[u32], fracs: &[f64], width: usize) -> Vec<(usize, String)> {
    const LW: usize = 5;
    let n = hours.len();
    if n == 0 || width < LW {
        return vec![];
    }
    let max = (width / (LW + 3)).clamp(1, 5);
    let step = [1, 2, 3, 4, 6, 8, 12, 24]
        .into_iter()
        .find(|s| hours.iter().filter(|h| *h % s == 0).count() <= max)
        .unwrap_or(24);
    let mut out = Vec::new();
    let mut next_free = 0usize;
    for (i, h) in hours.iter().enumerate() {
        if h % step != 0 {
            continue;
        }
        let (c0, c1) = widgets::bucket_cols(i, n, width);
        let frac = fracs.get(i).copied().unwrap_or(0.0);
        let at = c0 + ((c1 - c0) as f64 * frac).round() as usize;
        let x = at.min(width - LW);
        if x < next_free {
            continue;
        }
        out.push((x, format!("{h:02}:00")));
        next_free = x + LW + 1;
    }
    out
}

/// Draw failure counts over a bar chart of request totals: in the error
/// colour, or as `░` without colour so they stay distinguishable.
pub fn overlay_failures(
    f: &mut Frame,
    app: &App,
    plot: Rect,
    fail: &[f64],
    top: f64,
    symbols: GraphSymbols,
) {
    use ratatui::widgets::Widget;
    let t = app.theme();
    let g = Graph {
        data: fail,
        theme: t,
        symbols,
        max: Some(top),
        color: Some(t.err),
    };
    if t.no_color {
        let mut scratch = ratatui::buffer::Buffer::empty(plot);
        g.render(plot, &mut scratch);
        let buf = f.buffer_mut();
        for y in plot.y..plot.bottom() {
            for x in plot.x..plot.right() {
                if scratch[(x, y)].symbol() != " " {
                    buf[(x, y)].set_char('░');
                }
            }
        }
    } else {
        f.render_widget(g, plot);
    }
}

/// Mantissas of the y-axis ladder. Consecutive rungs are at most 1.25×
/// apart, so the smallest rung at or above a peak puts the peak at ≥ 80 %
/// of the axis (1.75, 3.5 and 8 close the 1.5→2, 3→4 and 7.5→10 gaps).
pub const NICE_LADDER: [f64; 14] = [
    1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 3.5, 4.0, 5.0, 6.0, 7.5, 8.0, 10.0,
];

/// Round a scale maximum up to the next rung of [`NICE_LADDER`] × 10ⁿ, so
/// axis labels are tidy (`125M`, `1.75k`, `7.5k`) and the peak fills at
/// least 80 % of the axis.
pub fn nice_max(v: f64) -> f64 {
    if !v.is_finite() || v <= 0.0 {
        return 1.0;
    }
    let exp = v.log10().floor();
    let base = 10f64.powf(exp);
    let m = v / base;
    let nice = NICE_LADDER
        .into_iter()
        .find(|x| m <= *x * (1.0 + 1e-9))
        .unwrap_or(10.0);
    // Scale the rung's three digits by an exact power of ten so the top
    // carries no float noise (`0.1 × 1.25` is not exactly `0.125`).
    let digits = (nice * 100.0).round();
    let e = exp as i32 - 2;
    if e >= 0 {
        digits * 10f64.powi(e)
    } else {
        digits / 10f64.powi(-e)
    }
}

/// The top of an axis for a series peaking at `peak`. Counts (`integral`)
/// keep whole-number tops — their labels have no decimals — so a peak of
/// 7 tops out at 8 (an even top has a whole half-way label), not at a
/// `7.5` that would print as `8`.
pub fn axis_top(peak: f64, integral: bool) -> f64 {
    let top = nice_max(peak);
    if !integral || top.fract() == 0.0 {
        return top;
    }
    let whole = peak.ceil().max(1.0);
    let even = whole + whole % 2.0;
    if peak / even >= 0.8 { even } else { whole }
}

/// Whether every finite value is a whole number (counts, tokens).
pub fn integral(values: &[f64]) -> bool {
    values
        .iter()
        .filter(|v| v.is_finite())
        .all(|v| v.fract() == 0.0)
}

/// Time label for a bucket timestamp: `HH:MM` within a day, `MM-DD` for
/// daily buckets, `HH:MM:SS` for sub-minute buckets.
pub fn time_labels(buckets: &[String], daily: bool, seconds: bool) -> Vec<String> {
    buckets
        .iter()
        .map(|b| {
            if daily {
                fmt::bucket_label(b, true)
            } else if seconds {
                fmt::ts_fmt(b, "%H:%M:%S")
            } else {
                fmt::ts_fmt(b, "%H:%M")
            }
        })
        .collect()
}

/// Full timestamp for the cursor readout.
pub fn cursor_time(bucket: &str, daily: bool) -> String {
    if daily {
        fmt::ts_fmt(bucket, "%a %m-%d")
    } else {
        // Seconds only when they are not zero (`11-13 00:00`, not `11-13 00`).
        let full = fmt::ts_fmt(bucket, "%m-%d %H:%M:%S");
        full.strip_suffix(":00").map(str::to_string).unwrap_or(full)
    }
}

/// A `label value` header for a chart, with an optional cursor readout.
pub fn chart_head(app: &App, name: &str, value: String, rest: Vec<Span<'static>>) -> Line<'static> {
    let t = app.theme();
    let mut v = vec![
        Span::styled(format!("{name} "), Style::default().fg(t.graph_text)),
        Span::styled(
            value,
            Style::default().fg(t.title).add_modifier(Modifier::BOLD),
        ),
    ];
    v.extend(rest);
    Line::from(v)
}

#[cfg(test)]
mod tests {
    use super::{hour_ticks, nice_max, offset_hours};

    fn labels(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn half_hour_buckets_get_whole_hour_ticks() {
        // `today` in +05:30: UTC-hour buckets 23:30 … 20:30 local.
        let l: Vec<String> = (0..22)
            .map(|i| format!("{:02}:30", (23 + i) % 24))
            .collect();
        let (fracs, hours) = offset_hours(&l).unwrap();
        assert_eq!(fracs[0], 0.5);
        assert_eq!(hours[0], 0);
        let ticks = hour_ticks(&hours, &fracs, 88);
        let names: Vec<&str> = ticks.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(names, ["00:00", "06:00", "12:00", "18:00"]);
        // 00:00 sits in the middle of the first bucket (4 columns wide).
        assert_eq!(ticks[0].0, 2);
        // At 80 columns the axis is narrower but ticks stay on whole hours.
        assert!(
            hour_ticks(&hours, &fracs, 50)
                .iter()
                .all(|(_, s)| s.ends_with(":00"))
        );
    }

    #[test]
    fn other_axes_keep_their_labels() {
        assert!(offset_hours(&labels(&["00:00", "01:00", "02:00"])).is_none());
        assert!(offset_hours(&labels(&["20:43", "20:44", "20:45"])).is_none());
        assert!(offset_hours(&labels(&["11-12", "11-13"])).is_none());
        // A gap in the hours: not a dense hourly axis.
        assert!(offset_hours(&labels(&["01:30", "03:30"])).is_none());
        // +05:45: the hour is a quarter into the bucket.
        let (fracs, hours) = offset_hours(&labels(&["10:45", "11:45"])).unwrap();
        assert_eq!(fracs, [0.25, 0.25]);
        assert_eq!(hours, [11, 12]);
        // `today` with its first bucket relabelled to midnight.
        let (fracs, hours) = offset_hours(&labels(&["00:00", "00:30", "01:30"])).unwrap();
        assert_eq!(fracs, [0.0, 0.5, 0.5]);
        assert_eq!(hours, [0, 1, 2]);
    }

    #[test]
    fn nice_scale_maxima() {
        assert_eq!(nice_max(0.0), 1.0);
        assert_eq!(nice_max(-3.0), 1.0);
        assert_eq!(nice_max(f64::NAN), 1.0);
        assert_eq!(nice_max(7.3), 7.5);
        assert_eq!(nice_max(76.3e6), 80e6);
        assert_eq!(nice_max(1.9), 2.0);
        assert_eq!(nice_max(2.2), 2.5);
        assert_eq!(nice_max(420.0), 500.0);
        // Reviewed cases: 117M, 663 and a quota peak of 255.
        assert_eq!(nice_max(117e6), 125e6);
        assert_eq!(nice_max(663.0), 750.0);
        assert_eq!(nice_max(255.0), 300.0);
        // Exact rungs stay put.
        assert_eq!(nice_max(1000.0), 1000.0);
        assert_eq!(nice_max(1250.0), 1250.0);
        assert_eq!(nice_max(7500.0), 7500.0);
        assert_eq!(nice_max(0.3), 0.3);
        assert_eq!(nice_max(0.012), 0.0125);
    }

    #[test]
    fn nice_ladder_keeps_peak_above_80_percent() {
        let l = super::NICE_LADDER;
        for w in l.windows(2) {
            assert!(w[0] / w[1] >= 0.8 - 1e-12, "{} → {}", w[0], w[1]);
        }
        // Peaks on, just above and between every rung, over many decades.
        for exp in -3..=13 {
            let base = 10f64.powi(exp);
            for r in &l[..l.len() - 1] {
                for m in [*r, r * 1.0001, r * 1.07, r * 1.2499] {
                    let v = m * base;
                    let top = nice_max(v);
                    assert!(top >= v * (1.0 - 1e-9), "{v} → {top}");
                    assert!(v / top >= 0.8 - 1e-9, "{v} → {top}");
                }
            }
        }
    }

    #[test]
    fn count_axes_keep_whole_tops() {
        use super::axis_top;
        assert_eq!(axis_top(7.0, true), 8.0);
        assert_eq!(axis_top(11.0, true), 12.0);
        assert_eq!(axis_top(17.0, true), 18.0);
        assert_eq!(axis_top(3.0, true), 3.0);
        assert_eq!(axis_top(12.0, true), 12.0);
        assert_eq!(axis_top(13.0, true), 15.0);
        assert_eq!(axis_top(0.0, true), 1.0);
        assert_eq!(axis_top(663.0, true), 750.0);
        // Rates keep the fractional rung.
        assert_eq!(axis_top(7.0, false), 7.5);
        for p in 1..=200 {
            let top = axis_top(p as f64, true);
            assert_eq!(top.fract(), 0.0, "{p}");
            assert!(p as f64 / top >= 0.8, "{p} → {top}");
        }
    }
}
