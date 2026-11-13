//! Tabs, panes and layout presets.
//!
//! A preset is a list of rows; each row holds panes with horizontal weights.
//! Hidden panes are removed, panes that do not fit their minimum width are
//! dropped (lowest priority first) and rows that do not fit vertically are
//! dropped by priority, so every layout degrades gracefully on small terminals.

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
}

const fn p(id: PaneId, name: &'static str, min_w: u16, min_h: u16, table: bool) -> PaneDef {
    PaneDef {
        id,
        name,
        min_w,
        min_h,
        table,
    }
}

use PaneId::*;

const OVERVIEW: [PaneDef; 4] = [
    p(OvTotals, "totals", 30, 4, false),
    p(OvTrend, "trend", 24, 5, false),
    p(OvActivity, "activity", 24, 6, false),
    p(OvDist, "distribution", 36, 5, true),
];
const REALTIME: [PaneDef; 7] = [
    p(RtSummary, "window", 30, 4, false),
    p(RtThroughput, "throughput", 24, 5, false),
    p(RtOutcomes, "outcomes", 20, 5, false),
    p(RtLatency, "latency", 24, 6, false),
    p(RtShare, "share", 34, 5, true),
    p(RtSpeed, "speed", 34, 5, true),
    p(RtCache, "cache", 20, 5, false),
];
const ANALYSIS: [PaneDef; 7] = [
    p(AnTokens, "tokens", 30, 6, false),
    p(AnComposition, "composition", 36, 5, true),
    p(AnModels, "models", 34, 5, true),
    p(AnLatency, "latency", 24, 6, false),
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
            row(Fill(3), 1, &[(OvTrend, 3), (OvActivity, 2)]),
            row(Fill(4), 2, &[(OvDist, 1)]),
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

/// Compute pane rectangles for a tab.
pub fn compute(tab: Tab, preset: usize, hidden: &[PaneId], area: Rect) -> Vec<(PaneId, Rect)> {
    let presets = presets(tab);
    let preset = &presets[preset % presets.len()];
    let def = |id: PaneId| panes(tab).iter().find(|d| d.id == id).copied();

    // Visible panes per row.
    let mut rows: Vec<(RowDef, Vec<(PaneDef, u16)>)> = preset
        .rows
        .iter()
        .map(|r| {
            let ps = r
                .panes
                .iter()
                .filter(|(id, _)| !hidden.contains(id))
                .filter_map(|(id, w)| def(*id).map(|d| (d, *w)))
                .collect::<Vec<_>>();
            (*r, ps)
        })
        .filter(|(_, ps)| !ps.is_empty())
        .collect();

    // Drop rows (highest prio number first) until minimum heights fit.
    let min_h = |r: &(RowDef, Vec<(PaneDef, u16)>)| -> u16 {
        let pane_min = r.1.iter().map(|(d, _)| d.min_h).max().unwrap_or(3);
        match r.0.height {
            Fixed(h) => h,
            Fill(_) => pane_min,
        }
    };
    while rows.len() > 1 && rows.iter().map(min_h).sum::<u16>() > area.height {
        let (idx, _) = rows
            .iter()
            .enumerate()
            .max_by_key(|(_, r)| r.0.prio)
            .expect("non-empty");
        rows.remove(idx);
    }

    // Vertical split.
    let fixed: u16 = rows
        .iter()
        .map(|r| match r.0.height {
            Fixed(h) => h,
            Fill(_) => 0,
        })
        .sum();
    let fill_weights: u16 = rows
        .iter()
        .map(|r| match r.0.height {
            Fill(w) => w,
            Fixed(_) => 0,
        })
        .sum();
    let flexible = area.height.saturating_sub(fixed);
    let mut heights: Vec<u16> = rows
        .iter()
        .map(|r| match r.0.height {
            Fixed(h) => h.min(area.height),
            Fill(w) => {
                if fill_weights == 0 {
                    0
                } else {
                    (flexible as u32 * w as u32 / fill_weights as u32) as u16
                }
            }
        })
        .collect();
    // Hand rounding leftovers to the last fill row.
    let used: u16 = heights.iter().sum();
    if let Some(i) = rows.iter().rposition(|r| matches!(r.0.height, Fill(_))) {
        heights[i] += area.height.saturating_sub(used);
    } else if let Some(last) = heights.last_mut() {
        *last += area.height.saturating_sub(used);
    }

    let mut out = Vec::new();
    let mut y = area.y;
    for ((_, mut ps), h) in rows.into_iter().zip(heights) {
        if h == 0 {
            continue;
        }
        // Drop panes from the right until minimum widths fit.
        while ps.len() > 1 && ps.iter().map(|(d, _)| d.min_w).sum::<u16>() > area.width {
            ps.pop();
        }
        let total_w: u32 = ps.iter().map(|(_, w)| *w as u32).sum::<u32>().max(1);
        let mut x = area.x;
        let n = ps.len();
        for (i, (d, w)) in ps.into_iter().enumerate() {
            let width = if i + 1 == n {
                area.x + area.width - x
            } else {
                (area.width as u32 * w as u32 / total_w) as u16
            };
            out.push((d.id, Rect::new(x, y, width, h)));
            x += width;
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
    fn layout_fills_area_and_degrades() {
        for t in ALL_TABS {
            for i in 0..presets(t).len() {
                for (w, h) in [(80, 21), (160, 45), (220, 57), (40, 10)] {
                    let area = Rect::new(0, 2, w, h);
                    let rects = compute(t, i, &[], area);
                    assert!(!rects.is_empty(), "{t:?} preset {i} at {w}x{h}");
                    for (_, r) in &rects {
                        assert!(r.right() <= area.right() && r.bottom() <= area.bottom());
                    }
                }
            }
        }
    }

    #[test]
    fn hidden_panes_are_skipped() {
        let area = Rect::new(0, 0, 160, 40);
        let rects = compute(Tab::Overview, 0, &[PaneId::OvTrend], area);
        assert!(rects.iter().all(|(id, _)| *id != PaneId::OvTrend));
        let act = rects
            .iter()
            .find(|(id, _)| *id == PaneId::OvActivity)
            .unwrap();
        assert_eq!(act.1.width, 160);
    }
}
