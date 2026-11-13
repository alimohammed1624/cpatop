//! Every recorded API response parses into its typed model.

use cpatop::api::models::*;
use serde::de::DeserializeOwned;
use std::path::Path;

fn load<T: DeserializeOwned>(name: &str) -> T {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn auth_and_status() {
    let admin: SessionResponse = load("auth-session-admin.json");
    assert!(admin.authenticated);
    assert_eq!(admin.role.as_deref(), Some("admin"));
    let key: SessionResponse = load("auth-session-key.json");
    assert_eq!(key.role.as_deref(), Some("api_key_viewer"));
    assert_eq!(key.api_key.unwrap().id, "1");
    let anon: SessionResponse = load("auth-session-anon.json");
    assert!(!anon.authenticated);
    let s: StartupStatus = load("startup-status.json");
    assert_eq!(s.phase, "ready");
    let r: StartupStatus = load("readyz.json");
    assert_eq!(r.phase, "ready");
    let h: Health = load("healthz.json");
    assert_eq!(h.status, "ok");
    let v: VersionInfo = load("version.json");
    assert!(v.version.starts_with('v'));
    let st: ServerStatus = load("status.json");
    assert_eq!(st.timezone, "Asia/Kolkata");
    let u: UpdateCheck = load("update-check.json");
    assert!(!u.current_version.is_empty());
    let sess: AuthSessions = load("auth-sessions.json");
    assert!(sess.items.iter().any(|i| i.current));
}

#[test]
fn errors() {
    let e: ErrorBody = load("error-400.json");
    assert!(e.error.is_some());
    let e: ErrorBody = load("error-401.json");
    assert_eq!(e.error.as_deref(), Some("authentication required"));
    let e: ErrorBody = load("error-403.json");
    assert_eq!(e.error.as_deref(), Some("forbidden"));
    let e: ErrorBody = load("usage-event-request-log.json");
    assert!(e.error.is_some());
}

#[test]
fn overview_family() {
    for f in ["usage-overview.json", "key-overview.json"] {
        let o: Overview = load(f);
        assert!(o.usage.total_requests > 0.0, "{f}");
        assert_eq!(o.series.buckets.len(), o.series.tokens.len(), "{f}");
    }
    for f in [
        "usage-overview-comparisons.json",
        "key-overview-comparisons.json",
    ] {
        let c: Comparisons = load(f);
        assert!(!c.models.is_empty(), "{f}");
        for m in &c.models {
            assert_eq!(m.token_series.len(), c.buckets.len(), "{f}");
        }
    }
    let k: Comparisons = load("key-overview-comparisons.json");
    assert!(k.credentials.is_empty());
    for f in ["usage-overview-realtime.json", "key-overview-realtime.json"] {
        let r: Realtime = load(f);
        assert_eq!(r.bucket_seconds, 30.0, "{f}");
        assert!(!r.token_velocity.is_empty(), "{f}");
        assert!(r.insights.is_some(), "{f}");
    }
    for f in ["usage-activity.json", "key-activity.json"] {
        let a: Activity = load(f);
        assert_eq!((a.rows, a.columns), (7, 52), "{f}");
        assert!(!a.blocks.is_empty());
    }
}

#[test]
fn analysis_family() {
    for f in ["usage-analysis.json", "key-analysis.json"] {
        let a: Analysis = load(f);
        assert!(!a.token_usage.is_empty(), "{f}");
        assert!(!a.model_composition.is_empty(), "{f}");
        assert!(!a.heatmap.models.is_empty(), "{f}");
    }
    for f in ["usage-analysis-latency.json", "key-analysis-latency.json"] {
        let l: LatencyDiagnostics = load(f);
        assert!(l.supported, "{f}");
    }
    let k: ApiKeys = load("usage-api-keys.json");
    assert_eq!(k.items[0].id, "1");
    // The raw `apiKey` in the settings variant is ignored by the model.
    let s: ApiKeys = load("usage-api-keys-settings.json");
    assert!(!s.items.is_empty());
    let o: ApiKeyOptions = load("usage-api-keys-options.json");
    assert!(!o.options.is_empty());
    let m: ModelCatalog = load("models.json");
    assert!(!m.data.is_empty());
}

#[test]
fn events_family() {
    let e: EventsPage = load("usage-events.json");
    assert!(!e.events.is_empty());
    assert!(e.events.iter().all(|ev| !ev.id.is_empty()));
    let x: ExportPage = load("usage-events-export.json");
    assert!(!x.events.is_empty());
    let fm: EventModelFilters = load("usage-events-filters-models.json");
    assert!(!fm.models.is_empty());
    let fs: EventSourceFilters = load("usage-events-filters-sources.json");
    assert!(!fs.sources.is_empty());
    let csv = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/usage-events-export.csv"),
    )
    .unwrap();
    assert!(csv.starts_with("id,timestamp,api_key"));
}

#[test]
fn identities_family() {
    let i: Identities = load("usage-identities.json");
    assert!(!i.identities.is_empty());
    let p: IdentitiesPage = load("usage-identities-page.json");
    assert!(p.identities[0].credential_health.is_some());
    assert!(!p.type_counts.is_empty());
    let one: Identity = load("usage-identity.json");
    assert!(!one.identity.is_empty());
    let errs: IdentityErrors = load("usage-identity-errors.json");
    assert!(!errs.events.is_empty());
}

#[test]
fn quota_family() {
    let q: QuotaInspection = load("quota-inspection.json");
    assert_eq!(q.total, 7.0);
    let t: QuotaRefreshTask = load("quota-refresh-task.json");
    assert_eq!(t.status, "completed");
    let rows = &t.quota.as_ref().unwrap().quota;
    assert!(rows.iter().any(|r| r.used_pct().is_some()));
    assert_eq!(rows[0].display_label(), "Monthly");
    let c: QuotaRefreshTask = load("quota-refresh-task-claude.json");
    assert!(c.quota.unwrap().claude_reset_grants.is_some());
    let h: QuotaHistory = load("quota-history.json");
    assert!(!h.cycles.is_empty());
    let hc: QuotaHistory = load("quota-history-claude.json");
    assert!(hc.windows.len() >= 2);
    let a: AutoRefreshSettings = load("quota-auto-refresh-settings.json");
    assert!(!a.enabled);
    let rc: ResetCredits = load("quota-reset-credits.json");
    assert!(!rc.auth_index.is_empty());
    let g: ClaudeResetGrants = load("quota-claude-reset-grants.json");
    assert!(g.status.is_some());
}

#[test]
fn pricing_family() {
    let p: PricingModels = load("pricing-models.json");
    assert!(!p.models.is_empty());
    let o: PricingModelOptions = load("pricing-model-options.json");
    assert!(!o.models.is_empty());
    let r: RecalcOptions = load("pricing-recalculations-options.json");
    assert_eq!(r.step_seconds, 3600.0);
    let c: Option<RecalcTask> = load("pricing-recalculations-current.json");
    assert!(c.is_some());
    for f in [
        "pricing-sync-fetch-litellm.json",
        "pricing-sync-fetch-models-dev.json",
    ] {
        let s: PricingSyncFetch = load(f);
        assert!(!s.matches.is_empty(), "{f}");
    }
}

#[test]
fn every_json_fixture_is_covered() {
    // Guard against adding a fixture without a parse test above.
    let src = include_str!("fixtures.rs");
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for entry in std::fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if name.ends_with(".json") {
            assert!(
                src.contains(&format!("\"{name}\"")),
                "{name} has no parse test"
            );
        }
    }
}
