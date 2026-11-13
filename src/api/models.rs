//! Typed, tolerant serde models for the Keeper v2 API.
//!
//! Every struct uses `#[serde(default)]` so missing fields never fail a whole
//! payload. IDs accept strings or numbers. Field casing follows the server,
//! which mixes snake_case and camelCase (see `API.md`).

use serde::{Deserialize, Deserializer};
use std::collections::HashMap;

/// Accept a string, number or null and produce a `String`.
pub fn de_id<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Id {
        S(String),
        I(i64),
        U(u64),
        F(f64),
        B(bool),
    }
    Ok(match Option::<Id>::deserialize(d)? {
        Some(Id::S(s)) => s,
        Some(Id::I(i)) => i.to_string(),
        Some(Id::U(u)) => u.to_string(),
        Some(Id::F(f)) => f.to_string(),
        Some(Id::B(b)) => b.to_string(),
        None => String::new(),
    })
}

/// Treat `null` as the type's default value.
pub fn nd<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

// ---------------------------------------------------------------- auth/status

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SessionResponse {
    pub authenticated: bool,
    pub role: Option<String>,
    pub api_key: Option<SessionKey>,
    pub oidc_login_enabled: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SessionKey {
    #[serde(deserialize_with = "de_id")]
    pub id: String,
    #[serde(deserialize_with = "nd")]
    pub display_key: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct StartupStatus {
    #[serde(deserialize_with = "nd")]
    pub phase: String,
    #[serde(deserialize_with = "nd")]
    pub message: String,
    pub progress: Option<StartupProgress>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct StartupProgress {
    pub processed_count: f64,
    pub total_count: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Health {
    #[serde(deserialize_with = "nd")]
    pub status: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct VersionInfo {
    #[serde(deserialize_with = "nd")]
    pub version: String,
    pub update_check_enabled: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ServerStatus {
    pub running: bool,
    pub sync_running: bool,
    #[serde(deserialize_with = "nd")]
    pub timezone: String,
    pub cpa_public_url: Option<String>,
    pub cpa_request_log_access_enabled: bool,
    pub last_error: Option<String>,
    pub last_warning: Option<String>,
    pub last_status: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct UpdateCheck {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub can_compare: bool,
    pub message: String,
}

/// Error body: `{"error":...}` or `{"code":...,"message":...}`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ErrorBody {
    pub error: Option<String>,
    pub code: Option<String>,
    pub message: Option<String>,
    pub detail: Option<String>,
}

// ---------------------------------------------------------------- overview

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Overview {
    pub usage: OverviewUsage,
    pub summary: OverviewSummary,
    pub series: OverviewSeries,
    #[serde(deserialize_with = "nd")]
    pub timezone: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct OverviewUsage {
    pub total_requests: f64,
    pub success_count: f64,
    pub failure_count: f64,
    pub total_tokens: f64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct OverviewSummary {
    pub rpm: f64,
    pub tpm: f64,
    pub total_cost: f64,
    pub cost_available: bool,
    pub input_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    pub reasoning_tokens: f64,
    pub daily_average_requests: Option<f64>,
    pub daily_average_tokens: Option<f64>,
    pub daily_average_cost: Option<f64>,
    pub daily_average_range_days: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct OverviewSeries {
    #[serde(deserialize_with = "nd")]
    pub buckets: Vec<String>,
    #[serde(deserialize_with = "nd")]
    pub requests: Vec<f64>,
    #[serde(deserialize_with = "nd")]
    pub tokens: Vec<f64>,
    #[serde(deserialize_with = "nd")]
    pub rpm: Vec<f64>,
    #[serde(deserialize_with = "nd")]
    pub tpm: Vec<f64>,
    #[serde(deserialize_with = "nd")]
    pub cost: Vec<f64>,
    #[serde(deserialize_with = "nd")]
    pub cache_read_rate: Vec<Option<f64>>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Comparisons {
    #[serde(deserialize_with = "nd")]
    pub buckets: Vec<String>,
    #[serde(deserialize_with = "nd")]
    pub granularity: String,
    #[serde(deserialize_with = "nd")]
    pub timezone: String,
    #[serde(deserialize_with = "nd")]
    pub models: Vec<ComparisonItem>,
    #[serde(deserialize_with = "nd")]
    pub api_keys: Vec<ComparisonItem>,
    #[serde(deserialize_with = "nd")]
    pub credentials: Vec<ComparisonItem>,
    #[serde(deserialize_with = "nd")]
    pub ai_providers: Vec<ComparisonItem>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ComparisonItem {
    #[serde(deserialize_with = "nd")]
    pub token_series: Vec<f64>,
    #[serde(deserialize_with = "de_id")]
    pub key: String,
    #[serde(deserialize_with = "nd")]
    pub label: String,
    pub requests: f64,
    pub failures: f64,
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    pub reasoning_tokens: f64,
    pub total_tokens: f64,
    pub cost: Option<f64>,
}

// ---------------------------------------------------------------- realtime

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Realtime {
    pub insights: Option<RtInsights>,
    #[serde(deserialize_with = "nd")]
    pub window: String,
    #[serde(deserialize_with = "nd")]
    pub timezone: String,
    pub bucket_seconds: f64,
    pub window_start: Option<String>,
    pub window_end: Option<String>,
    #[serde(deserialize_with = "nd")]
    pub token_velocity: Vec<RtTokenPoint>,
    #[serde(deserialize_with = "nd")]
    pub request_level: Vec<RtRequestPoint>,
    #[serde(deserialize_with = "nd")]
    pub cache_level: Vec<RtCachePoint>,
    pub latency_scatter: LatencyScatter,
    pub model_speed: RtModelSpeed,
    pub current_usage: CurrentUsage,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct RtInsights {
    pub summary: RtSummary,
    #[serde(deserialize_with = "nd")]
    pub outcomes: Vec<RtOutcome>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct RtSummary {
    pub requests: f64,
    pub failures: f64,
    pub token_requests: f64,
    pub cached_requests: f64,
    pub total_tokens: f64,
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub reasoning_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    pub cost: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct RtOutcome {
    pub bucket: String,
    pub requests: f64,
    pub failures: f64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct RtTokenPoint {
    pub bucket: String,
    pub tokens_per_minute: f64,
    pub tokens: f64,
    pub cost: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct RtRequestPoint {
    pub bucket: String,
    pub requests_per_minute: f64,
    pub requests: f64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct RtCachePoint {
    pub bucket: String,
    pub cache_read_rate: Option<f64>,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    pub input_tokens: f64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct LatencyPoint {
    pub ttft_ms: f64,
    pub latency_ms: f64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct LatencyScatter {
    #[serde(deserialize_with = "nd")]
    pub points: Vec<LatencyPoint>,
    pub total_points: f64,
    pub p95_ttft_ms: f64,
    pub p95_latency_ms: f64,
    pub max_ttft_ms: f64,
    pub max_latency_ms: f64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct RtModelSpeed {
    pub available: bool,
    pub start: Option<String>,
    pub mid: Option<String>,
    pub end: Option<String>,
    #[serde(deserialize_with = "nd")]
    pub series: Vec<RtSpeedSeries>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct RtSpeedSeries {
    pub model: String,
    pub total_tokens: f64,
    pub speed_sample_count: f64,
    pub first_half: SpeedSample,
    pub second_half: SpeedSample,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SpeedSample {
    pub speed_tps_sum: f64,
    pub speed_sample_count: f64,
    pub mean_tps: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct CurrentUsage {
    #[serde(deserialize_with = "nd")]
    pub models: Vec<TopItem>,
    #[serde(deserialize_with = "nd")]
    pub api_keys: Vec<TopItem>,
    #[serde(deserialize_with = "nd")]
    pub credentials: Vec<TopItem>,
    #[serde(deserialize_with = "nd")]
    pub ai_providers: Vec<TopItem>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct TopItem {
    #[serde(deserialize_with = "de_id")]
    pub key: String,
    #[serde(deserialize_with = "nd")]
    pub label: String,
    pub tokens: f64,
    pub requests: f64,
    pub cost: Option<f64>,
    pub share: f64,
}

// ---------------------------------------------------------------- activity

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Activity {
    #[serde(deserialize_with = "nd")]
    pub window: String,
    #[serde(deserialize_with = "nd")]
    pub grain: String,
    #[serde(deserialize_with = "nd")]
    pub timezone: String,
    pub rows: u32,
    pub columns: u32,
    pub bucket_seconds: f64,
    #[serde(deserialize_with = "nd")]
    pub window_start: String,
    #[serde(deserialize_with = "nd")]
    pub window_end: String,
    pub total_success: f64,
    pub total_failure: f64,
    pub success_rate: f64,
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub reasoning_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    pub total_tokens: f64,
    #[serde(deserialize_with = "nd")]
    pub blocks: Vec<ActivityBlock>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ActivityBlock {
    pub start_time: String,
    pub end_time: String,
    pub success: f64,
    pub failure: f64,
    /// Fraction 0..1, or -1 for no requests.
    pub rate: f64,
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub reasoning_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    pub total_tokens: f64,
}

// ---------------------------------------------------------------- analysis

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Analysis {
    #[serde(deserialize_with = "nd")]
    pub granularity: String,
    #[serde(deserialize_with = "nd")]
    pub timezone: String,
    pub range_start: Option<String>,
    pub range_end: Option<String>,
    #[serde(deserialize_with = "nd")]
    pub token_usage: Vec<TokenUsageBucket>,
    pub model_speed: AnModelSpeed,
    pub model_usage: ModelUsage,
    #[serde(deserialize_with = "nd")]
    pub api_key_composition: Vec<CompositionItem>,
    #[serde(deserialize_with = "nd")]
    pub model_composition: Vec<CompositionItem>,
    #[serde(deserialize_with = "nd")]
    pub credentials_composition: Vec<CompositionItem>,
    #[serde(deserialize_with = "nd")]
    pub ai_provider_composition: Vec<CompositionItem>,
    pub heatmap: Heatmap,
    pub cost_summary: CostSummary,
    #[serde(deserialize_with = "nd")]
    pub model_efficiency: Vec<ModelEfficiency>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct TokenUsageBucket {
    pub bucket: String,
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    pub reasoning_tokens: f64,
    pub total_tokens: f64,
    pub requests: f64,
    pub cost_usd: f64,
    pub cost_available: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct AnModelSpeed {
    pub range_start: Option<String>,
    pub range_end: Option<String>,
    #[serde(deserialize_with = "nd")]
    pub buckets: Vec<SpeedBucket>,
    #[serde(deserialize_with = "nd")]
    pub series: Vec<AnSpeedSeries>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SpeedBucket {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct AnSpeedSeries {
    pub model: String,
    pub total_tokens: f64,
    pub speed_sample_count: f64,
    #[serde(deserialize_with = "nd")]
    pub samples: Vec<SpeedSample>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ModelUsage {
    #[serde(deserialize_with = "nd")]
    pub buckets: Vec<String>,
    #[serde(deserialize_with = "nd")]
    pub series: Vec<ModelUsageSeries>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ModelUsageSeries {
    pub model: String,
    #[serde(deserialize_with = "nd")]
    pub total_tokens: Vec<f64>,
    #[serde(deserialize_with = "nd")]
    pub requests: Vec<f64>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct CompositionItem {
    #[serde(deserialize_with = "de_id")]
    pub key: String,
    #[serde(deserialize_with = "nd")]
    pub label: String,
    pub total_tokens: f64,
    pub requests: f64,
    pub percent: f64,
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    pub reasoning_tokens: f64,
    pub cost_usd: f64,
    pub cost_available: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Heatmap {
    #[serde(deserialize_with = "nd")]
    pub api_keys: Vec<String>,
    #[serde(deserialize_with = "nd")]
    pub api_key_labels: HashMap<String, String>,
    #[serde(deserialize_with = "nd")]
    pub models: Vec<String>,
    #[serde(deserialize_with = "nd")]
    pub cells: Vec<HeatmapCell>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct HeatmapCell {
    #[serde(deserialize_with = "de_id")]
    pub api_key: String,
    pub model: String,
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    pub reasoning_tokens: f64,
    pub total_tokens: f64,
    pub requests: f64,
    pub cost_usd: f64,
    pub cost_available: bool,
    pub intensity: f64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct CostSummary {
    pub total_cost_usd: f64,
    pub cost_available: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ModelEfficiency {
    pub model: String,
    pub requests: f64,
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    pub reasoning_tokens: f64,
    pub total_tokens: f64,
    pub cost_usd: f64,
    pub cost_available: bool,
    pub cost_per_request_usd: f64,
    pub output_tokens_per_request: f64,
    /// Fraction 0..1.
    pub cache_read_rate: f64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct LatencyDiagnostics {
    pub supported: bool,
    pub unsupported_reason: Option<String>,
    #[serde(deserialize_with = "nd")]
    pub points: Vec<LatencyPoint>,
    pub total_points: f64,
    pub sampled: bool,
    pub p95_ttft_ms: f64,
    pub p95_latency_ms: f64,
    pub max_ttft_ms: f64,
    pub max_latency_ms: f64,
}

// ---------------------------------------------------------------- api keys / models

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ApiKeys {
    #[serde(deserialize_with = "nd")]
    pub items: Vec<ApiKeyItem>,
}

/// A CPA API key as listed by `/usage/api-keys`. The `apiKey` field returned by
/// `/usage/api-keys/settings` is deliberately not modelled so it can never be
/// rendered.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ApiKeyItem {
    #[serde(deserialize_with = "de_id")]
    pub id: String,
    #[serde(deserialize_with = "nd")]
    pub key_alias: String,
    #[serde(deserialize_with = "nd")]
    pub display_key: String,
    #[serde(deserialize_with = "nd")]
    pub label: String,
    pub last_synced_at: Option<String>,
    #[serde(deserialize_with = "nd")]
    pub source_type: String,
    #[serde(deserialize_with = "nd")]
    pub access_provider: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ApiKeyOptions {
    #[serde(deserialize_with = "nd")]
    pub options: Vec<ApiKeyOption>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ApiKeyOption {
    #[serde(deserialize_with = "de_id")]
    pub id: String,
    #[serde(deserialize_with = "nd")]
    pub label: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ModelCatalog {
    pub object: Option<String>,
    #[serde(deserialize_with = "nd")]
    pub data: Vec<CatalogModel>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct CatalogModel {
    pub id: String,
    pub object: Option<String>,
    pub created: Option<i64>,
    pub owned_by: Option<String>,
}

// ---------------------------------------------------------------- events

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct EventsPage {
    #[serde(deserialize_with = "nd")]
    pub events: Vec<UsageEvent>,
    pub total_count: i64,
    pub page: i64,
    pub page_size: i64,
    pub total_pages: i64,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct UsageEvent {
    #[serde(deserialize_with = "de_id")]
    pub id: String,
    pub timestamp: String,
    pub api_key: Option<String>,
    #[serde(deserialize_with = "nd")]
    pub model: String,
    pub model_alias: Option<String>,
    pub response_model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub effective_service_tier: Option<String>,
    pub response_service_tier: Option<String>,
    pub client_ip: Option<String>,
    pub x_forwarded_for: Option<String>,
    pub user_agent: Option<String>,
    pub executor_type: Option<String>,
    pub endpoint: Option<String>,
    #[serde(deserialize_with = "nd")]
    pub source: String,
    pub source_type: Option<String>,
    pub auth_index: Option<String>,
    pub request_id: Option<String>,
    #[serde(rename = "isDelete", alias = "is_delete")]
    pub is_delete: bool,
    pub failed: bool,
    pub status_code: Option<i64>,
    pub stream: Option<bool>,
    pub latency_ms: f64,
    pub ttft_ms: Option<f64>,
    pub speed_tps: Option<f64>,
    pub tokens: EventTokens,
    pub cost_usd: f64,
    pub cost_available: bool,
    pub pricing_style: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct EventTokens {
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub reasoning_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    pub total_tokens: f64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct EventModelFilters {
    #[serde(deserialize_with = "nd")]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct EventSourceFilters {
    #[serde(deserialize_with = "nd")]
    pub sources: Vec<EventSource>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct EventSource {
    pub value: String,
    pub label: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
}

/// One row of `/usage/events/export?format=json`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ExportPage {
    #[serde(deserialize_with = "nd")]
    pub events: Vec<ExportRow>,
    pub total_count: i64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ExportRow {
    #[serde(deserialize_with = "de_id")]
    pub id: String,
    pub timestamp: String,
    pub model: String,
    pub result: String,
    pub latency_ms: f64,
    pub ttft_ms: Option<f64>,
    pub total_tokens: f64,
    pub cost_usd: f64,
    pub cache_read_rate: Option<f64>,
}

/// Bounds shared by live-stream frames.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct LiveBounds {
    pub range_start: String,
    pub range_end: String,
    pub end_exclusive: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct LiveSnapshot {
    #[serde(deserialize_with = "nd")]
    pub events: Vec<UsageEvent>,
    pub total_count: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
    pub range_start: String,
    pub range_end: String,
    pub end_exclusive: bool,
    pub snapshot_at: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct LiveAppend {
    #[serde(deserialize_with = "nd")]
    pub events: Vec<UsageEvent>,
    pub range_start: String,
    pub range_end: String,
    pub end_exclusive: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct LiveHeartbeat {
    pub range_start: String,
    pub range_end: String,
    pub end_exclusive: bool,
    pub busy: bool,
    pub total_count: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct LiveCode {
    pub code: String,
    pub retry_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct RequestLog {
    #[serde(deserialize_with = "de_id")]
    pub event_id: String,
    pub available: bool,
    pub previewable: bool,
    pub too_large: bool,
    #[serde(deserialize_with = "nd")]
    pub sections: Vec<RequestLogSection>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct RequestLogSection {
    pub title: String,
    pub content: String,
}

// ---------------------------------------------------------------- identities

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Identities {
    #[serde(deserialize_with = "nd")]
    pub identities: Vec<Identity>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct IdentitiesPage {
    #[serde(deserialize_with = "nd")]
    pub identities: Vec<Identity>,
    pub total_count: i64,
    pub page: i64,
    pub page_size: i64,
    pub total_pages: i64,
    #[serde(deserialize_with = "nd")]
    pub type_counts: Vec<TypeCount>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct TypeCount {
    #[serde(rename = "type")]
    pub kind: String,
    pub count: i64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Identity {
    #[serde(deserialize_with = "de_id")]
    pub id: String,
    #[serde(deserialize_with = "nd")]
    pub name: String,
    pub alias: Option<String>,
    #[serde(
        rename = "displayName",
        alias = "display_name",
        deserialize_with = "nd"
    )]
    pub display_name: String,
    pub auth_type: i64,
    #[serde(deserialize_with = "nd")]
    pub auth_type_name: String,
    #[serde(deserialize_with = "nd")]
    pub identity: String,
    #[serde(rename = "type", deserialize_with = "nd")]
    pub kind: String,
    #[serde(deserialize_with = "nd")]
    pub provider: String,
    pub file_name: Option<String>,
    pub priority: Option<i64>,
    pub disabled: bool,
    pub note: Option<String>,
    pub subscription: Option<Subscription>,
    pub active_start: Option<String>,
    pub active_until: Option<String>,
    pub total_requests: f64,
    pub success_count: f64,
    pub failure_count: f64,
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub reasoning_tokens: f64,
    pub cache_read_tokens: f64,
    pub total_tokens: f64,
    pub first_used_at: Option<String>,
    pub last_used_at: Option<String>,
    pub stats_updated_at: Option<String>,
    pub stats_reset_at: Option<String>,
    pub period_stats: Option<PeriodStats>,
    pub is_deleted: bool,
    pub credential_health: Option<CredentialHealth>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Subscription {
    pub provider: String,
    pub plan: String,
    pub tier_id: Option<String>,
    pub tier_name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct PeriodStats {
    pub total_requests: f64,
    pub success_count: f64,
    pub failure_count: f64,
    pub input_tokens: f64,
    pub cache_read_tokens: f64,
    pub total_tokens: f64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct CredentialHealth {
    pub window_seconds: f64,
    pub bucket_seconds: f64,
    pub window_start: String,
    pub window_end: String,
    pub total_success: f64,
    pub total_failure: f64,
    pub success_rate: f64,
    pub input_tokens: f64,
    pub cache_read_tokens: f64,
    #[serde(deserialize_with = "nd")]
    pub buckets: Vec<HealthBucket>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct HealthBucket {
    pub start_time: String,
    pub end_time: String,
    pub success: f64,
    pub failure: f64,
    pub rate: f64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct IdentityErrors {
    #[serde(deserialize_with = "nd")]
    pub events: Vec<ErrorEvent>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ErrorEvent {
    #[serde(deserialize_with = "de_id")]
    pub id: String,
    pub timestamp: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub status_code: i64,
    #[serde(deserialize_with = "nd")]
    pub body_summary: String,
    pub body_truncated: bool,
    pub code: Option<String>,
    pub retryable: bool,
    pub credential_retry_after: Option<String>,
    pub model_retry_after: Option<String>,
}

// ---------------------------------------------------------------- quota

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct QuotaInspection {
    pub total: f64,
    pub cached: f64,
    pub running: bool,
    pub completed: bool,
    pub completed_at: Option<String>,
    pub normal: f64,
    pub limit_reached: f64,
    pub unauthorized_401: f64,
    pub payment_required_402: f64,
    pub unauthorized_401_402: f64,
    pub other_failed: f64,
    pub unknown: f64,
    #[serde(deserialize_with = "nd")]
    pub results: Vec<InspectionResult>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct InspectionResult {
    pub auth_index: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub file_name: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub http_status_code: Option<i64>,
    pub refreshed_at: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct QuotaRefreshTask {
    #[serde(rename = "authIndex", alias = "auth_index")]
    pub auth_index: String,
    pub file_name: Option<String>,
    pub status: String,
    pub quota: Option<QuotaCheck>,
    pub error: Option<String>,
    pub http_status_code: Option<i64>,
    pub refreshed_at: Option<String>,
    #[serde(rename = "expiresAt", alias = "expires_at")]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct QuotaCheck {
    #[serde(deserialize_with = "de_id")]
    pub id: String,
    #[serde(deserialize_with = "nd")]
    pub quota: Vec<QuotaRow>,
    pub subscription: Option<Subscription>,
    #[serde(rename = "rateLimitResetCreditsAvailableCount")]
    pub reset_credits_available: Option<f64>,
    #[serde(rename = "claudeResetGrants")]
    pub claude_reset_grants: Option<ClaudeResetGrantStatus>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct QuotaRow {
    pub key: String,
    pub label: Option<String>,
    pub scope: Option<String>,
    pub metric: Option<String>,
    pub group_key: Option<String>,
    pub group_label: Option<String>,
    pub used: Option<f64>,
    pub limit: Option<f64>,
    pub remaining: Option<f64>,
    pub used_percent: Option<f64>,
    pub remaining_fraction: Option<f64>,
    pub allowed: Option<bool>,
    pub limit_reached: Option<bool>,
    pub window: Option<QuotaWindow>,
    pub reset_at: Option<String>,
    pub reset_after_seconds: Option<f64>,
    #[serde(rename = "window_usage_tokens")]
    pub window_usage_tokens: Option<f64>,
    #[serde(rename = "window_usage_cost")]
    pub window_usage_cost: Option<f64>,
    #[serde(deserialize_with = "nd")]
    pub usage_breakdown: Vec<UsageBreakdown>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct UsageBreakdown {
    pub product: String,
    pub used_percent: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct QuotaWindow {
    pub duration: Option<f64>,
    pub unit: Option<String>,
    pub seconds: Option<f64>,
}

impl QuotaRow {
    /// Percentage of the quota already used, if known.
    pub fn used_pct(&self) -> Option<f64> {
        if let Some(p) = self.used_percent {
            return Some(p.clamp(0.0, 100.0));
        }
        if let Some(f) = self.remaining_fraction {
            return Some((100.0 - f * 100.0).clamp(0.0, 100.0));
        }
        match (self.used, self.limit) {
            (Some(u), Some(l)) if l > 0.0 => Some((u / l * 100.0).clamp(0.0, 100.0)),
            _ => None,
        }
    }

    /// Human label normalised the way the web app does.
    pub fn display_label(&self) -> String {
        if let Some(secs) = self.window.as_ref().and_then(|w| w.seconds) {
            match secs as i64 {
                18_000 => return "5h".into(),
                604_800 => return "Weekly".into(),
                2_592_000 | 2_628_000 => return "Monthly".into(),
                _ => {}
            }
        }
        if self.scope.as_deref() == Some("quota_group")
            && let Some(g) = &self.group_label
        {
            return g.clone();
        }
        if let Some(l) = self.label.as_ref().filter(|l| !l.is_empty()) {
            return l.clone();
        }
        if self.key.ends_with("primary_window") {
            return "Primary".into();
        }
        if self.key.ends_with("secondary_window") {
            return "Secondary".into();
        }
        self.metric
            .clone()
            .or_else(|| self.scope.clone())
            .unwrap_or_else(|| self.key.clone())
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ClaudeResetGrantStatus {
    pub eligible: bool,
    pub ineligible_reason: Option<String>,
    pub at_limit: bool,
    #[serde(deserialize_with = "nd")]
    pub grants: Vec<ClaudeResetGrant>,
    pub next_grant_id: Option<String>,
    pub weekly_resets_at: Option<String>,
    pub cooldown_until: Option<String>,
    pub available_count: f64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ClaudeResetGrant {
    pub id: String,
    pub label: Option<String>,
    pub resets_total: f64,
    pub resets_left: f64,
    pub starts_at: Option<String>,
    pub ends_at: Option<String>,
    #[serde(deserialize_with = "nd")]
    pub clears: Vec<String>,
    pub paused: bool,
    pub usable_now: bool,
    pub use_requires_limit: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ClaudeResetGrants {
    pub auth_index: String,
    pub status: Option<ClaudeResetGrantStatus>,
    pub selected_grant_id: Option<String>,
    pub organization_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ResetCredits {
    pub auth_index: String,
    pub available_count: Option<f64>,
    #[serde(deserialize_with = "nd")]
    pub credits: Vec<ResetCredit>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ResetCredit {
    pub id: String,
    pub status: String,
    pub granted_at: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct AutoRefreshSettings {
    pub enabled: bool,
    pub schedule: Option<AutoRefreshSchedule>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct AutoRefreshSchedule {
    pub unit: String,
    pub value: f64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct QuotaHistory {
    pub generated_at: String,
    pub range_start: String,
    #[serde(deserialize_with = "nd")]
    pub windows: Vec<QuotaHistoryWindow>,
    pub selected_window: Option<QuotaHistoryWindow>,
    #[serde(deserialize_with = "nd")]
    pub cycles: Vec<QuotaCycle>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct QuotaHistoryWindow {
    pub window_role: String,
    pub window_kind: Option<String>,
    pub window_seconds: f64,
    pub has_current_cycle: bool,
    pub last_observed_at: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct QuotaCycle {
    #[serde(deserialize_with = "de_id")]
    pub id: String,
    pub status: String,
    pub window_seconds: f64,
    pub window_started_at: String,
    pub reset_at: String,
    pub effective_started_at: String,
    pub effective_ended_at: String,
    pub first_observed_at: String,
    pub last_observed_at: String,
    pub first_remaining_percent: Option<f64>,
    pub last_remaining_percent: Option<f64>,
    pub observation_count: f64,
    pub usage: QuotaUsage,
    #[serde(deserialize_with = "nd")]
    pub transitions: Vec<QuotaTransition>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct QuotaUsage {
    pub requests: f64,
    pub successful_requests: f64,
    pub failed_requests: f64,
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub reasoning_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    pub total_tokens: f64,
    pub total_cost_usd: f64,
    pub cost_available: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct QuotaTransition {
    pub from_remaining_percent: f64,
    pub to_remaining_percent: f64,
    pub percentage_points: f64,
    pub is_direct: bool,
    pub interval_started_at: String,
    pub interval_ended_at: String,
    pub usage: QuotaUsage,
    pub tokens_per_point: f64,
    pub cost_per_point: f64,
    pub cost_per_point_available: bool,
}

// ---------------------------------------------------------------- pricing

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct PricingModels {
    #[serde(deserialize_with = "nd")]
    pub models: Vec<ModelPricing>,
    pub config_revision: i64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ModelPricing {
    pub model: String,
    pub pricing_style: String,
    pub base_prices: BasePrices,
    pub model_multiplier: f64,
    #[serde(deserialize_with = "nd")]
    pub conditional_multipliers: Vec<serde_json::Value>,
    #[serde(deserialize_with = "nd")]
    pub branches: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct BasePrices {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct PricingModelOptions {
    #[serde(deserialize_with = "nd")]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct RecalcOptions {
    pub timezone: String,
    pub earliest_start: Option<String>,
    pub latest_start: Option<String>,
    pub step_seconds: f64,
    pub max_days: f64,
    pub config_revision: i64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct RecalcTask {
    pub task_id: String,
    pub status: String,
    pub stage: String,
    pub start_at: String,
    pub end_at: String,
    pub config_revision: i64,
    pub processed_count: f64,
    pub total_count: Option<f64>,
    pub updated_at: String,
    pub error: Option<RecalcError>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct RecalcError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct PricingSyncFetch {
    pub source: String,
    #[serde(deserialize_with = "nd")]
    pub matches: Vec<PricingSyncMatch>,
    #[serde(deserialize_with = "nd")]
    pub unmatched_models: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct PricingSyncMatch {
    pub model: String,
    pub matched_model: String,
    pub provider: String,
    pub pricing_style: String,
    pub base_prices: BasePrices,
}

// ---------------------------------------------------------------- sessions

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct AuthSessions {
    #[serde(deserialize_with = "nd")]
    pub items: Vec<AuthSessionItem>,
}

/// An entry of `/auth/sessions`. The `id` (a hash of the session token) is
/// intentionally not modelled: it is a revocation handle and never shown.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AuthSessionItem {
    pub kind: String,
    pub role: String,
    pub source: String,
    pub current: bool,
    pub login_at: Option<String>,
    pub last_seen_at: Option<String>,
    pub expires_at: Option<String>,
    pub login_ip: Option<String>,
    pub last_seen_ip: Option<String>,
    pub user_agent: Option<String>,
    pub alias: Option<String>,
    #[serde(deserialize_with = "de_id")]
    pub api_key_id: String,
    pub label: Option<String>,
    pub display_key: Option<String>,
}
