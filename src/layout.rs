//! Tabs, panes and layout presets.
//!
//! A preset is a list of rows; each row holds panes with horizontal weights.
//! Hidden panes are removed and panes that do not fit their minimum width are
//! dropped from the right.
//!
//! Heights are content-aware: every pane reports a [`Size`] (minimum and,
//! for panes whose content is finite such as summaries and short tables, a
//! maximum). Rows first get their minimum; spare rows then go to rows that
//! can use them (graphs, long tables) in proportion to their weights. When
//! even the minimums do not fit, the lowest-priority rows collapse to a
//! one-line title bar, and only then are dropped — panes are never squeezed
//! below the height where their content is readable.

use crate::api::Role;
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tab {
    #[default]
    Overview,
    Realtime,
    Analysis,
    Events,
    Credentials,
    Quota,
    Models,
    Settings,
}

impl Tab {
    pub fn title(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Realtime => "Realtime",
            Tab::Analysis => "Analysis",
            Tab::Events => "Requests",
            Tab::Credentials => "Credentials",
            Tab::Quota => "Quota",
            Tab::Models => "Models",
            Tab::Settings => "Keys",
        }
    }
    pub fn short_title(self) -> &'static str {
        match self {
            Tab::Overview => "Ovw",
            Tab::Realtime => "Live",
            Tab::Analysis => "Stats",
            Tab::Events => "Reqs",
            Tab::Credentials => "Creds",
            Tab::Quota => "Quota",
            Tab::Models => "Models",
            Tab::Settings => "Keys",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Tab::Overview => "overview",
            Tab::Realtime => "realtime",
            Tab::Analysis => "analysis",
            Tab::Events => "requests",
            Tab::Credentials => "credentials",
            Tab::Quota => "quota",
            Tab::Models => "models",
            Tab::Settings => "keys",
        }
    }
    pub fn from_key(s: &str) -> Option<Tab> {
        ALL_TABS.iter().copied().find(|t| t.key() == s)
    }
}

pub const ALL_TABS: [Tab; 8] = [
    Tab::Overview,
    Tab::Realtime,
    Tab::Analysis,
    Tab::Events,
    Tab::Credentials,
    Tab::Quota,
    Tab::Models,
    Tab::Settings,
];
const KEY_TABS: [Tab; 3] = [Tab::Overview, Tab::Realtime, Tab::Analysis];

pub fn tabs_for(role: Option<Role>) -> &'static [Tab] {
    match role {
        Some(Role::Admin) => &ALL_TABS,
        _ => &KEY_TABS,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneId {
    OvTotals,
    OvTrend,
    OvActivity,
    OvDist,
    RtSummary,
    RtThroughput,
    RtOutcomes,
    RtLatency,
    RtShare,
    RtSpeed,
    RtCache,
    AnTokens,
    AnComposition,
    AnModels,
    AnLatency,
    AnSpeed,
    AnEfficiency,
    AnHeatmap,
    EvLog,
    EvDetail,
    CrList,
    CrDetail,
    QuInspection,
    QuList,
    QuHistory,
    MoPricing,
    MoCatalog,
    MoRecalc,
    StKeys,
    StSessions,
    StServer,
}

#[derive(Debug, Clone, Copy)]
pub struct PaneDef {
    pub id: PaneId,
    pub name: &'static str,
    pub min_w: u16,
    pub min_h: u16,
    /// Has a selectable list (receives arrows, filter and sort keys).
    pub table: bool,
    /// Time-series graph with a cursor (←/→ when focused).
    pub graph: bool,
}

const fn p(id: PaneId, name: &'static str, min_w: u16, min_h: u16, table: bool) -> PaneDef {
    PaneDef {
        id,
        name,
        min_w,
        min_h,
        table,
        graph: false,
    }
}

const fn g(id: PaneId, name: &'static str, min_w: u16, min_h: u16) -> PaneDef {
    PaneDef {
        id,
        name,
        min_w,
        min_h,
        table: false,
        graph: true,
    }
}

use PaneId::*;

const OVERVIEW: [PaneDef; 4] = [
    p(OvTotals, "totals", 30, 4, false),
    g(OvTrend, "trend", 24, 7),
    p(OvActivity, "activity", 24, 6, false),
    p(OvDist, "distribution", 36, 5, true),
];
/// Panes are numbered by their position here, so each tab lists them in
/// the reading order (left→right, top→bottom) of every preset.
const REALTIME: [PaneDef; 7] = [
    p(RtSummary, "window", 30, 4, false),
    g(RtThroughput, "throughput", 24, 7),
    g(RtOutcomes, "outcomes", 20, 6),
    p(RtShare, "share", 34, 5, true),
    p(RtSpeed, "speed", 34, 5, true),
    p(RtLatency, "latency", 24, 8, false),
    g(RtCache, "cache", 20, 6),
];

/// Version of the pane numbering stored in the UI state. Version 1 (no
/// version stored) numbered Realtime latency 4, share 5, speed 6.
pub const PANE_ORDER: u32 = 2;

/// Map a pane number saved under numbering `version` to today's number.
pub fn migrate_pane_number(tab: Tab, n: usize, version: u32) -> usize {
    if version < 2 && tab == Tab::Realtime {
        // Old 4 latency, 5 share, 6 speed → 6, 4, 5.
        return match n {
            4 => 6,
            5 => 4,
            6 => 5,
            n => n,
        };
    }
    n
}
const ANALYSIS: [PaneDef; 7] = [
    g(AnTokens, "tokens", 30, 7),
    p(AnComposition, "composition", 36, 5, true),
    p(AnModels, "models", 34, 5, true),
    p(AnLatency, "latency", 24, 8, false),
    p(AnSpeed, "speed", 30, 5, true),
    p(AnEfficiency, "efficiency", 40, 5, true),
    p(AnHeatmap, "heatmap", 30, 5, true),
];
const EVENTS: [PaneDef; 2] = [
    p(EvLog, "requests", 40, 6, true),
    p(EvDetail, "detail", 30, 6, false),
];
const CREDENTIALS: [PaneDef; 2] = [
    p(CrList, "credentials", 40, 6, true),
    p(CrDetail, "detail", 30, 6, false),
];
const QUOTA: [PaneDef; 3] = [
    p(QuInspection, "inspection", 30, 4, false),
    p(QuList, "quota", 40, 6, true),
    p(QuHistory, "history", 40, 5, true),
];
const MODELS: [PaneDef; 3] = [
    p(MoPricing, "pricing", 40, 5, true),
    p(MoCatalog, "catalog", 30, 5, true),
    p(MoRecalc, "recalc", 26, 5, false),
];
const SETTINGS: [PaneDef; 3] = [
    p(StKeys, "api keys", 40, 4, true),
    p(StSessions, "sessions", 40, 4, true),
    p(StServer, "server", 26, 5, false),
];

pub fn panes(tab: Tab) -> &'static [PaneDef] {
    match tab {
        Tab::Overview => &OVERVIEW,
        Tab::Realtime => &REALTIME,
        Tab::Analysis => &ANALYSIS,
        Tab::Events => &EVENTS,
        Tab::Credentials => &CREDENTIALS,
        Tab::Quota => &QUOTA,
        Tab::Models => &MODELS,
        Tab::Settings => &SETTINGS,
    }
}

pub fn pane_def(id: PaneId) -> (Tab, usize, PaneDef) {
    for t in ALL_TABS {
        for (i, d) in panes(t).iter().enumerate() {
            if d.id == id {
                return (t, i + 1, *d);
            }
        }
    }
    unreachable!("pane {id:?} is registered")
}

#[derive(Debug, Clone, Copy)]
pub enum Height {
    Fixed(u16),
    Fill(u16),
}

#[derive(Debug, Clone, Copy)]
pub struct RowDef {
    pub height: Height,
    /// Lower drops later.
    pub prio: u8,
    pub panes: &'static [(PaneId, u16)],
}

#[derive(Debug, Clone, Copy)]
pub struct Preset {
    pub name: &'static str,
    pub rows: &'static [RowDef],
}

const fn row(height: Height, prio: u8, panes: &'static [(PaneId, u16)]) -> RowDef {
    RowDef {
        height,
        prio,
        panes,
    }
}
use Height::{Fill, Fixed};

const OV_PRESETS: [Preset; 3] = [
    Preset {
        name: "balanced",
        rows: &[
            row(Fixed(5), 0, &[(OvTotals, 1)]),
            row(Fill(4), 1, &[(OvTrend, 3), (OvActivity, 2)]),
            row(Fill(3), 2, &[(OvDist, 1)]),
        ],
    },
    Preset {
        name: "graphs",
        rows: &[
            row(Fixed(5), 0, &[(OvTotals, 1)]),
            row(Fill(5), 1, &[(OvTrend, 1)]),
            row(Fill(3), 2, &[(OvActivity, 2), (OvDist, 3)]),
        ],
    },
    Preset {
        name: "table",
        rows: &[
            row(Fixed(5), 0, &[(OvTotals, 1)]),
            row(Fill(1), 1, &[(OvDist, 1)]),
        ],
    },
];
const RT_PRESETS: [Preset; 3] = [
    Preset {
        name: "balanced",
        rows: &[
            row(Fixed(4), 0, &[(RtSummary, 1)]),
            row(Fill(3), 1, &[(RtThroughput, 3), (RtOutcomes, 2)]),
            // The two short tables share a row sized to their rows; the
            // graphs below take the height.
            row(Fill(2), 2, &[(RtShare, 1), (RtSpeed, 1)]),
            row(Fill(3), 3, &[(RtLatency, 2), (RtCache, 3)]),
        ],
    },
    Preset {
        name: "graphs",
        rows: &[
            row(Fixed(4), 0, &[(RtSummary, 1)]),
            row(Fill(3), 1, &[(RtThroughput, 1)]),
            row(Fill(2), 2, &[(RtOutcomes, 1), (RtLatency, 1), (RtCache, 1)]),
        ],
    },
    Preset {
        name: "tables",
        rows: &[
            row(Fixed(4), 0, &[(RtSummary, 1)]),
            row(Fill(1), 1, &[(RtShare, 1)]),
            row(Fill(1), 2, &[(RtSpeed, 1)]),
        ],
    },
];
const AN_PRESETS: [Preset; 3] = [
    Preset {
        name: "balanced",
        rows: &[
            row(Fill(3), 0, &[(AnTokens, 1)]),
            row(Fill(4), 1, &[(AnComposition, 1), (AnModels, 1)]),
            row(Fill(3), 2, &[(AnLatency, 2), (AnSpeed, 3)]),
            row(Fill(3), 3, &[(AnEfficiency, 3), (AnHeatmap, 2)]),
        ],
    },
    Preset {
        name: "usage",
        rows: &[
            row(Fill(2), 0, &[(AnTokens, 1)]),
            row(Fill(3), 1, &[(AnComposition, 1), (AnModels, 1)]),
        ],
    },
    Preset {
        name: "diagnostics",
        rows: &[
            row(Fill(1), 0, &[(AnLatency, 2), (AnSpeed, 3)]),
            row(Fill(1), 1, &[(AnEfficiency, 1)]),
            row(Fill(1), 2, &[(AnHeatmap, 1)]),
        ],
    },
];
const EV_PRESETS: [Preset; 3] = [
    Preset {
        name: "stacked",
        rows: &[
            row(Fill(3), 0, &[(EvLog, 1)]),
            row(Fill(1), 1, &[(EvDetail, 1)]),
        ],
    },
    Preset {
        name: "side",
        rows: &[row(Fill(1), 0, &[(EvLog, 3), (EvDetail, 1)])],
    },
    Preset {
        name: "log",
        rows: &[row(Fill(1), 0, &[(EvLog, 1)])],
    },
];
const CR_PRESETS: [Preset; 2] = [
    Preset {
        name: "stacked",
        rows: &[
            row(Fill(3), 0, &[(CrList, 1)]),
            row(Fill(2), 1, &[(CrDetail, 1)]),
        ],
    },
    Preset {
        name: "side",
        rows: &[row(Fill(1), 0, &[(CrList, 3), (CrDetail, 2)])],
    },
];
const QU_PRESETS: [Preset; 2] = [
    Preset {
        name: "stacked",
        rows: &[
            row(Fixed(5), 0, &[(QuInspection, 1)]),
            row(Fill(3), 1, &[(QuList, 1)]),
            row(Fill(2), 2, &[(QuHistory, 1)]),
        ],
    },
    Preset {
        name: "side",
        rows: &[
            row(Fixed(5), 0, &[(QuInspection, 1)]),
            row(Fill(1), 1, &[(QuList, 1), (QuHistory, 1)]),
        ],
    },
];
const MO_PRESETS: [Preset; 2] = [
    Preset {
        name: "stacked",
        rows: &[
            row(Fill(3), 0, &[(MoPricing, 1)]),
            row(Fill(2), 1, &[(MoCatalog, 3), (MoRecalc, 2)]),
        ],
    },
    Preset {
        name: "side",
        rows: &[row(Fill(1), 0, &[(MoPricing, 3), (MoCatalog, 2)])],
    },
];
const ST_PRESETS: [Preset; 2] = [
    Preset {
        name: "stacked",
        rows: &[
            row(Fill(3), 0, &[(StKeys, 1)]),
            row(Fill(2), 1, &[(StSessions, 3), (StServer, 2)]),
        ],
    },
    Preset {
        name: "lists",
        rows: &[
            row(Fill(1), 0, &[(StKeys, 1)]),
            row(Fill(1), 1, &[(StSessions, 1)]),
        ],
    },
];

pub fn presets(tab: Tab) -> &'static [Preset] {
    match tab {
        Tab::Overview => &OV_PRESETS,
        Tab::Realtime => &RT_PRESETS,
        Tab::Analysis => &AN_PRESETS,
        Tab::Events => &EV_PRESETS,
        Tab::Credentials => &CR_PRESETS,
        Tab::Quota => &QU_PRESETS,
        Tab::Models => &MO_PRESETS,
        Tab::Settings => &ST_PRESETS,
    }
}

/// Height needs of a pane (outer rows, including borders).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub min: u16,
    /// `None`: can use any number of rows (graphs, long tables).
    pub max: Option<u16>,
    /// Puts rows beyond `max` to use (a chart under a table): such rows get
    /// what is left once every row has its maximum, before anything else
    /// grows past its maximum into blank space.
    pub soak: bool,
    /// Widest the content can use (outer columns, e.g. a table with few
    /// columns): a wider share of the row goes to its neighbours.
    pub max_w: Option<u16>,
    /// `max` is where a plot stops gaining from height, not where its
    /// content ends (graph panes are always treated so).
    pub plot: bool,
}

impl Size {
    pub fn at_least(min: u16) -> Size {
        Size {
            min,
            max: None,
            soak: false,
            max_w: None,
            plot: false,
        }
    }
    pub fn exact(h: u16) -> Size {
        Size {
            min: h,
            max: Some(h),
            soak: false,
            max_w: None,
            plot: false,
        }
    }
    pub fn range(min: u16, max: u16) -> Size {
        Size {
            min,
            max: Some(max.max(min)),
            soak: false,
            max_w: None,
            plot: false,
        }
    }
    /// See [`Size::soak`].
    pub fn soaking(self) -> Size {
        Size { soak: true, ..self }
    }
    /// See [`Size::plot`].
    pub fn plot(self) -> Size {
        Size { plot: true, ..self }
    }
    /// See [`Size::max_w`].
    pub fn max_width(self, w: u16) -> Size {
        Size {
            max_w: Some(w),
            ..self
        }
    }
}

/// Rows past its maximum a soaking pane needs before the extra rows show
/// anything (a separator, a title line and a readable chart).
const SOAK_MIN: u16 = 8;

/// Fewest rows a table pane is shown in: borders, header and one row.
const TABLE_MIN: u16 = 4;

/// A placed pane. `collapsed` panes get one row: a title bar only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub id: PaneId,
    pub rect: Rect,
    pub collapsed: bool,
}

struct RowPlan {
    def: RowDef,
    panes: Vec<(PaneDef, Rect)>,
    min: u16,
    max: Option<u16>,
    /// Where the row's content panes (tables, maps) are complete: past it
    /// the extra rows only make a graph taller next to blank rows, so other
    /// rows that can use them come first. Graphs count with two thirds of
    /// their maximum, enough to stay readable.
    fit: Option<u16>,
    weight: u16,
    soak: bool,
    collapsed: bool,
}

/// Narrow panes whose content is narrower than their share of the row
/// ([`Size::max_w`]) and hand the columns to the other panes of the row, by
/// weight. Returns whether anything moved.
fn give_to_neighbours(placed: &mut [(PaneDef, Rect)], sizes: &[Size], weights: &[u16]) -> bool {
    let capped: Vec<Option<u16>> = placed
        .iter()
        .zip(sizes)
        .map(|((d, r), s)| s.max_w.map(|m| m.max(d.min_w)).filter(|m| *m < r.width))
        .collect();
    let takers: Vec<usize> = (0..placed.len()).filter(|i| capped[*i].is_none()).collect();
    if takers.is_empty() || capped.iter().all(Option::is_none) {
        return false;
    }
    let mut freed = 0u16;
    for (i, c) in capped.iter().enumerate() {
        if let Some(m) = c {
            freed += placed[i].1.width - m;
            placed[i].1.width = *m;
        }
    }
    let tw: u32 = takers.iter().map(|i| weights[*i].max(1) as u32).sum();
    let mut left = freed;
    for (j, i) in takers.iter().enumerate() {
        let add = if j + 1 == takers.len() {
            left
        } else {
            (freed as u32 * weights[*i].max(1) as u32 / tw) as u16
        };
        placed[*i].1.width += add;
        left -= add;
    }
    let mut x = placed.first().map(|(_, r)| r.x).unwrap_or(0);
    for (_, r) in placed.iter_mut() {
        r.x = x;
        x += r.width;
    }
    true
}

/// Hand `spare` rows out one at a time to the row least served relative to
/// its weight, among those the flags allow (see [`compute`]).
fn grow_pass(
    rows: &[RowPlan],
    heights: &mut [u16],
    spare: &mut u16,
    respect_max: bool,
    fit_only: bool,
    soak_only: bool,
) {
    let any_weighted = rows.iter().any(|r| !r.collapsed && r.weight > 0);
    while *spare > 0 {
        let pick = rows
            .iter()
            .enumerate()
            .filter(|(i, r)| {
                !r.collapsed
                    && (!soak_only || r.soak)
                    && (r.weight > 0 || (!respect_max && !any_weighted))
                    && (!respect_max || r.max.is_none_or(|m| heights[*i] < m))
                    && (!fit_only || r.fit.is_none_or(|m| heights[*i] < m))
            })
            .min_by(|(i, a), (j, b)| {
                // Least served relative to weight first.
                let wa = a.weight.max(1) as u32;
                let wb = b.weight.max(1) as u32;
                let fa = heights[*i].saturating_sub(a.min) as u32 * wb;
                let fb = heights[*j].saturating_sub(b.min) as u32 * wa;
                fa.cmp(&fb)
            })
            .map(|(i, _)| i);
        match pick {
            Some(i) => {
                heights[i] += 1;
                *spare -= 1;
            }
            None => break,
        }
    }
}

/// Compute pane rectangles for a tab. `size` reports each pane's height
/// needs at a given outer width.
pub fn compute(
    tab: Tab,
    preset: usize,
    hidden: &[PaneId],
    area: Rect,
    size: &dyn Fn(PaneId, u16) -> Size,
) -> Vec<Placed> {
    let presets = presets(tab);
    let preset = &presets[preset % presets.len()];
    let def = |id: PaneId| panes(tab).iter().find(|d| d.id == id).copied();

    // Visible panes per row, with widths.
    let mut rows: Vec<RowPlan> = preset
        .rows
        .iter()
        .filter_map(|r| {
            let mut ps = r
                .panes
                .iter()
                .filter(|(id, _)| !hidden.contains(id))
                .filter_map(|(id, w)| def(*id).map(|d| (d, *w)))
                .collect::<Vec<_>>();
            if ps.is_empty() {
                return None;
            }
            // Drop panes from the right until minimum widths fit.
            while ps.len() > 1 && ps.iter().map(|(d, _)| d.min_w).sum::<u16>() > area.width {
                ps.pop();
            }
            let weights: Vec<u16> = ps.iter().map(|(_, w)| *w).collect();
            let total_w: u32 = ps.iter().map(|(_, w)| *w as u32).sum::<u32>().max(1);
            let mut x = area.x;
            let n = ps.len();
            let mut placed = Vec::with_capacity(n);
            for (i, (d, w)) in ps.into_iter().enumerate() {
                let width = if i + 1 == n {
                    area.x + area.width - x
                } else {
                    (area.width as u32 * w as u32 / total_w) as u16
                };
                placed.push((d, Rect::new(x, 0, width, 0)));
                x += width;
            }
            let mut sizes: Vec<Size> = placed.iter().map(|(d, r)| size(d.id, r.width)).collect();
            if give_to_neighbours(&mut placed, &sizes, &weights) {
                sizes = placed.iter().map(|(d, r)| size(d.id, r.width)).collect();
            }
            let min = sizes.iter().map(|s| s.min).max().unwrap_or(3).max(3);
            let max = sizes
                .iter()
                .map(|s| s.max)
                .collect::<Option<Vec<u16>>>()
                .map(|v| v.into_iter().max().unwrap_or(min).max(min));
            // Content panes (tables, maps) are complete at their maximum;
            // a graph's maximum is only where it stops gaining from height.
            let content = placed
                .iter()
                .zip(&sizes)
                .filter(|((d, _), s)| !d.graph && !s.plot)
                .filter_map(|(_, s)| s.max)
                .max();
            let fit = max.map(|m| content.unwrap_or(0).max(m * 2 / 3).clamp(min, m));
            let soak = sizes.iter().any(|s| s.soak);
            let weight = match r.height {
                Height::Fill(w) => w.max(1),
                Height::Fixed(_) => 0,
            };
            Some(RowPlan {
                def: *r,
                panes: placed,
                min,
                max,
                fit,
                weight,
                soak,
                collapsed: false,
            })
        })
        .collect();

    let need = |rows: &[RowPlan]| -> u16 {
        rows.iter()
            .map(|r| if r.collapsed { 1 } else { r.min })
            .sum()
    };
    // Collapse, then drop, rows by priority (highest number first) until the
    // minimums fit.
    while rows.len() > 1 && need(&rows) > area.height {
        let victim = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.collapsed && r.def.prio > 0)
            .max_by_key(|(_, r)| r.def.prio)
            .map(|(i, _)| i);
        match victim {
            Some(i) => rows[i].collapsed = true,
            None => {
                let (i, _) = rows
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, r)| (r.collapsed, r.def.prio))
                    .expect("non-empty");
                rows.remove(i);
            }
        }
    }

    // Heights: minimums first, then spare rows by weight to rows below max.
    let mut heights: Vec<u16> = rows
        .iter()
        .map(|r| if r.collapsed { 1 } else { r.min })
        .collect();
    if rows.len() == 1 && !rows[0].collapsed {
        heights[0] = area.height;
    }
    let mut spare = area.height.saturating_sub(heights.iter().sum());
    // Content-sized (fixed) rows get up to their maximum first; then the
    // weighted rows share the rest.
    for (i, r) in rows.iter().enumerate() {
        if r.collapsed || r.weight > 0 {
            continue;
        }
        if let Some(m) = r.max {
            let add = m.saturating_sub(heights[i]).min(spare);
            heights[i] += add;
            spare -= add;
        }
    }
    // Rows grow until their shortest pane is complete…
    grow_pass(&rows, &mut heights, &mut spare, true, true, false);
    // …then rows that put extra height to use (charts under tables), when
    // there are enough rows for a chart…
    let chart_fits = rows.iter().enumerate().any(|(i, r)| {
        r.soak && !r.collapsed && r.max.is_none_or(|m| heights[i] + spare >= m + SOAK_MIN)
    });
    if chart_fits {
        grow_pass(&rows, &mut heights, &mut spare, false, false, true);
    }
    // …then up to the tallest pane's needs…
    grow_pass(&rows, &mut heights, &mut spare, true, false, false);
    // …and rows left over now would only be blank space inside a box that
    // is already complete: a collapsed row comes back instead when they are
    // enough for it, most important first. A table may give up one row
    // below its minimum (never below a header and one row): a short list
    // beats a one-line title bar.
    let mut order: Vec<usize> = (0..rows.len()).filter(|i| rows[*i].collapsed).collect();
    order.sort_by_key(|i| rows[*i].def.prio);
    for i in order {
        let r = &mut rows[i];
        let tables = r.panes.iter().all(|(d, _)| d.table);
        let need = if tables {
            r.min.saturating_sub(1).max(TABLE_MIN).min(r.min)
        } else {
            r.min
        };
        if spare + 1 >= need {
            r.collapsed = false;
            r.min = need;
            spare -= need - 1;
            heights[i] = need;
        }
    }
    // …then everything else.
    // Rows sized exactly to their content (a one-line message) stay as
    // they are while any other row can take the rest.
    let rigid: Vec<bool> = rows.iter().map(|r| r.max == Some(r.min)).collect();
    if rows
        .iter()
        .zip(&rigid)
        .any(|(r, rg)| !r.collapsed && !rg && r.weight > 0)
    {
        let mut h2: Vec<u16> = heights.clone();
        // Pin rigid rows by treating them as collapsed for this pass.
        let flexible: Vec<usize> = (0..rows.len()).filter(|i| !rigid[*i]).collect();
        while spare > 0 {
            let pick = flexible
                .iter()
                .copied()
                .filter(|i| !rows[*i].collapsed && rows[*i].weight > 0)
                .min_by(|i, j| {
                    let (a, b) = (&rows[*i], &rows[*j]);
                    let fa = h2[*i].saturating_sub(a.min) as u32 * b.weight.max(1) as u32;
                    let fb = h2[*j].saturating_sub(b.min) as u32 * a.weight.max(1) as u32;
                    fa.cmp(&fb)
                });
            match pick {
                Some(i) => {
                    h2[i] += 1;
                    spare -= 1;
                }
                None => break,
            }
        }
        heights = h2;
    }
    // Rows sized exactly to their content never take blank rows: with
    // every other row collapsed, what is left stays below the last row
    // instead of inside a complete box.
    let collapsed: Vec<bool> = rows.iter().map(|r| r.collapsed).collect();
    for (r, rg) in rows.iter_mut().zip(&rigid) {
        r.collapsed |= *rg;
    }
    grow_pass(&rows, &mut heights, &mut spare, false, false, false);
    let last_grows = rows.last().is_some_and(|r| !r.collapsed);
    for (r, c) in rows.iter_mut().zip(collapsed) {
        r.collapsed = c;
    }
    if spare > 0
        && last_grows
        && let Some(last) = heights.last_mut()
    {
        *last += spare;
    }

    let mut out = Vec::new();
    let mut y = area.y;
    for (r, h) in rows.into_iter().zip(heights) {
        let h = h.min(area.bottom().saturating_sub(y));
        if h == 0 {
            continue;
        }
        for (d, rect) in r.panes {
            out.push(Placed {
                id: d.id,
                rect: Rect::new(rect.x, y, rect.width, h),
                collapsed: r.collapsed,
            });
        }
        y += h;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pane_is_reachable_in_some_preset() {
        for t in ALL_TABS {
            for d in panes(t) {
                let found = presets(t).iter().any(|p| {
                    p.rows
                        .iter()
                        .any(|r| r.panes.iter().any(|(id, _)| *id == d.id))
                });
                assert!(found, "{:?} not in any preset", d.id);
            }
        }
    }

    #[test]
    fn panes_are_numbered_in_reading_order() {
        for t in ALL_TABS {
            for (i, p) in presets(t).iter().enumerate() {
                let nums: Vec<usize> = p
                    .rows
                    .iter()
                    .flat_map(|r| r.panes.iter())
                    .map(|(id, _)| pane_def(*id).1)
                    .collect();
                assert!(nums.is_sorted(), "{t:?} preset {i}: {nums:?}");
            }
        }
        // And as placed, at every size.
        for t in ALL_TABS {
            for i in 0..presets(t).len() {
                for (w, h) in [(50, 10), (80, 22), (160, 46), (220, 58)] {
                    let mut placed = compute(t, i, &[], Rect::new(0, 0, w, h), &defaults);
                    placed.sort_by_key(|p| (p.rect.y, p.rect.x));
                    let nums: Vec<usize> = placed.iter().map(|p| pane_def(p.id).1).collect();
                    assert!(nums.is_sorted(), "{t:?} preset {i} at {w}x{h}: {nums:?}");
                }
            }
        }
    }

    #[test]
    fn old_realtime_numbers_migrate() {
        let m = |n| {
            let new = migrate_pane_number(Tab::Realtime, n, 1);
            panes(Tab::Realtime)[new - 1].name
        };
        assert_eq!(m(4), "latency");
        assert_eq!(m(5), "share");
        assert_eq!(m(6), "speed");
        assert_eq!(m(7), "cache");
        assert_eq!(migrate_pane_number(Tab::Overview, 4, 1), 4);
        assert_eq!(migrate_pane_number(Tab::Realtime, 4, PANE_ORDER), 4);
    }

    fn defaults(id: PaneId, _w: u16) -> Size {
        Size::at_least(pane_def(id).2.min_h)
    }

    #[test]
    fn layout_fills_area_and_degrades() {
        for t in ALL_TABS {
            for i in 0..presets(t).len() {
                for (w, h) in [(80, 21), (160, 45), (220, 57), (40, 10)] {
                    let area = Rect::new(0, 2, w, h);
                    let rects = compute(t, i, &[], area, &defaults);
                    assert!(!rects.is_empty(), "{t:?} preset {i} at {w}x{h}");
                    let mut bottom = area.y;
                    for p in &rects {
                        assert!(p.rect.right() <= area.right() && p.rect.bottom() <= area.bottom());
                        bottom = bottom.max(p.rect.bottom());
                    }
                    assert_eq!(bottom, area.bottom(), "{t:?} preset {i} at {w}x{h} fills");
                }
            }
        }
    }

    #[test]
    fn hidden_panes_are_skipped() {
        let area = Rect::new(0, 0, 160, 40);
        let rects = compute(Tab::Overview, 0, &[PaneId::OvTrend], area, &defaults);
        assert!(rects.iter().all(|p| p.id != PaneId::OvTrend));
        let act = rects.iter().find(|p| p.id == PaneId::OvActivity).unwrap();
        assert_eq!(act.rect.width, 160);
    }

    #[test]
    fn content_sized_rows_give_spare_rows_to_graphs() {
        // A 6-row table needs 9 rows; the graph row takes everything else.
        let size = |id: PaneId, _w: u16| match id {
            PaneId::OvTotals => Size::exact(4),
            PaneId::OvDist => Size::range(5, 9),
            PaneId::OvActivity => Size::range(6, 20),
            _ => Size::at_least(7),
        };
        let area = Rect::new(0, 0, 160, 46);
        let rects = compute(Tab::Overview, 0, &[], area, &size);
        let h = |id| rects.iter().find(|p| p.id == id).unwrap().rect.height;
        assert_eq!(h(PaneId::OvTotals), 4);
        assert_eq!(h(PaneId::OvDist), 9);
        assert_eq!(h(PaneId::OvTrend), 46 - 4 - 9);
    }

    #[test]
    fn rows_stop_where_a_pane_runs_out_of_content() {
        // The activity map needs 26 rows; the trend beside it could use 32.
        // The 6 rows in between would be blank under the map, so the
        // distribution's chart below gets them.
        let size = |id: PaneId, _w: u16| match id {
            PaneId::OvTotals => Size::exact(5),
            PaneId::OvTrend => Size::range(9, 32),
            PaneId::OvActivity => Size::range(10, 26),
            _ => Size::range(5, 9).soaking(),
        };
        let area = Rect::new(0, 0, 220, 58);
        let rects = compute(Tab::Overview, 0, &[], area, &size);
        let h = |id| rects.iter().find(|p| p.id == id).unwrap().rect.height;
        assert_eq!(h(PaneId::OvActivity), 26);
        assert_eq!(h(PaneId::OvTrend), 26);
        assert_eq!(h(PaneId::OvDist), 58 - 5 - 26);
        // Four rows are too few for the chart under the table: the graph
        // row keeps them.
        let rects = compute(Tab::Overview, 0, &[], Rect::new(0, 0, 220, 44), &size);
        let h = |id| rects.iter().find(|p| p.id == id).unwrap().rect.height;
        assert_eq!(h(PaneId::OvDist), 9);
        assert_eq!(h(PaneId::OvTrend), 30);
        // A much shorter map does not squash the graph below two thirds of
        // its height.
        let size = |id: PaneId, w: u16| match id {
            PaneId::OvActivity => Size::range(6, 12),
            id => size(id, w),
        };
        let rects = compute(Tab::Overview, 0, &[], area, &size);
        let h = |id| rects.iter().find(|p| p.id == id).unwrap().rect.height;
        assert_eq!(h(PaneId::OvTrend), 32 * 2 / 3);
    }

    #[test]
    fn narrow_content_gives_width_to_neighbours() {
        // Analysis "balanced": efficiency 3 : heatmap 2. A one-key heatmap
        // 60 columns wide hands the rest of its share to efficiency.
        let size = |id: PaneId, _w: u16| match id {
            PaneId::AnHeatmap => Size::range(5, 6).max_width(60),
            id => Size::at_least(pane_def(id).2.min_h),
        };
        let area = Rect::new(0, 0, 220, 58);
        let rects = compute(Tab::Analysis, 0, &[], area, &size);
        let r = |id| rects.iter().find(|p| p.id == id).unwrap().rect;
        assert_eq!(r(PaneId::AnHeatmap).width, 60);
        assert_eq!(r(PaneId::AnEfficiency).width, 160);
        assert_eq!(r(PaneId::AnHeatmap).x, 160);
        // Never below the pane's minimum width, and a pane alone in its
        // row keeps the full width.
        let size = |id: PaneId, _w: u16| {
            Size::at_least(5).max_width(if id == PaneId::AnHeatmap { 3 } else { 10 })
        };
        let rects = compute(Tab::Analysis, 2, &[], area, &size);
        let h = rects.iter().find(|p| p.id == PaneId::AnHeatmap).unwrap();
        assert_eq!(h.rect.width, 220);
    }

    #[test]
    fn low_priority_rows_collapse_before_being_squeezed() {
        let size = |id: PaneId, _w: u16| Size::at_least(pane_def(id).2.min_h.max(8));
        // Analysis "balanced" has four rows of 8 minimum: 32 > 22.
        let area = Rect::new(0, 2, 80, 22);
        let rects = compute(Tab::Analysis, 0, &[], area, &size);
        for p in &rects {
            assert!(
                p.collapsed || p.rect.height >= 8,
                "{:?} squeezed to {}",
                p.id,
                p.rect.height
            );
        }
        assert!(rects.iter().any(|p| p.collapsed));
        let tokens = rects.iter().find(|p| p.id == PaneId::AnTokens).unwrap();
        assert!(!tokens.collapsed);
    }
}
