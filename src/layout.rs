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
const REALTIME: [PaneDef; 7] = [
    p(RtSummary, "window", 30, 4, false),
    g(RtThroughput, "throughput", 24, 7),
    g(RtOutcomes, "outcomes", 20, 6),
    p(RtLatency, "latency", 24, 8, false),
    p(RtShare, "share", 34, 5, true),
    p(RtSpeed, "speed", 34, 5, true),
    g(RtCache, "cache", 20, 6),
];
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
            row(Fill(3), 2, &[(RtShare, 3), (RtLatency, 2)]),
            row(Fill(3), 3, &[(RtSpeed, 3), (RtCache, 2)]),
        ],
    },
    Preset {
        name: "graphs",
        rows: &[
            row(Fixed(4), 0, &[(RtSummary, 1)]),
            row(Fill(3), 1, &[(RtThroughput, 1)]),
            row(Fill(2), 2, &[(RtOutcomes, 1), (RtCache, 1), (RtLatency, 1)]),
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
            row(Fixed(10), 1, &[(EvDetail, 1)]),
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
            row(Fill(2), 0, &[(StKeys, 3), (StServer, 2)]),
            row(Fill(3), 1, &[(StSessions, 1)]),
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
}

impl Size {
    pub fn at_least(min: u16) -> Size {
        Size { min, max: None }
    }
    pub fn exact(h: u16) -> Size {
        Size {
            min: h,
            max: Some(h),
        }
    }
    pub fn range(min: u16, max: u16) -> Size {
        Size {
            min,
            max: Some(max.max(min)),
        }
    }
}

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
    weight: u16,
    collapsed: bool,
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
            let sizes: Vec<Size> = placed.iter().map(|(d, r)| size(d.id, r.width)).collect();
            let min = sizes.iter().map(|s| s.min).max().unwrap_or(3).max(3);
            let max = sizes
                .iter()
                .map(|s| s.max)
                .collect::<Option<Vec<u16>>>()
                .map(|v| v.into_iter().max().unwrap_or(min).max(min));
            let weight = match r.height {
                Height::Fill(w) => w.max(1),
                Height::Fixed(_) => 0,
            };
            Some(RowPlan {
                def: *r,
                panes: placed,
                min,
                max,
                weight,
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
    let grow_pass = |heights: &mut Vec<u16>, spare: &mut u16, respect_max: bool| {
        while *spare > 0 {
            let pick = rows
                .iter()
                .enumerate()
                .filter(|(i, r)| {
                    !r.collapsed
                        && (r.weight > 0 || !respect_max)
                        && (!respect_max || r.max.is_none_or(|m| heights[*i] < m))
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
    };
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
    grow_pass(&mut heights, &mut spare, true);
    grow_pass(&mut heights, &mut spare, false);
    if spare > 0
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
