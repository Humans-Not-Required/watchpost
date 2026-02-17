use serde::{Deserialize, Serialize};

/// Deserialize tags that can be either a JSON array of strings or a comma-separated string.
/// Examples: `["a","b"]` → `["a","b"]`, `"a,b"` → `["a","b"]`, absent → `[]`
fn deserialize_flexible_tags<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct TagsVisitor;
    impl<'de> de::Visitor<'de> for TagsVisitor {
        type Value = Vec<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string or array of strings")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Vec<String>, E> {
            Ok(v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        }
        fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<String>, A::Error> {
            let mut tags = Vec::new();
            while let Some(tag) = seq.next_element::<String>()? {
                tags.push(tag);
            }
            Ok(tags)
        }
        fn visit_none<E: de::Error>(self) -> Result<Vec<String>, E> { Ok(Vec::new()) }
        fn visit_unit<E: de::Error>(self) -> Result<Vec<String>, E> { Ok(Vec::new()) }
    }
    deserializer.deserialize_any(TagsVisitor)
}

/// Optional version of flexible tags: absent → None, string → Some(vec), array → Some(vec)
fn deserialize_optional_flexible_tags<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct OptTagsVisitor;
    impl<'de> de::Visitor<'de> for OptTagsVisitor {
        type Value = Option<Vec<String>>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("null, a string, or array of strings")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Option<Vec<String>>, E> {
            Ok(Some(v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()))
        }
        fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Option<Vec<String>>, A::Error> {
            let mut tags = Vec::new();
            while let Some(tag) = seq.next_element::<String>()? {
                tags.push(tag);
            }
            Ok(Some(tags))
        }
        fn visit_none<E: de::Error>(self) -> Result<Option<Vec<String>>, E> { Ok(None) }
        fn visit_unit<E: de::Error>(self) -> Result<Option<Vec<String>>, E> { Ok(None) }
    }
    deserializer.deserialize_any(OptTagsVisitor)
}

/// Deserialize a double-option field: absent → None, null → Some(None), value → Some(Some(v))
fn deserialize_optional_nullable<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    // If serde calls this, the field was present in JSON
    Ok(Some(Option::deserialize(deserializer)?))
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Monitor {
    pub id: String,
    pub name: String,
    pub url: String,
    pub monitor_type: String,
    pub method: String,
    pub interval_seconds: u32,
    pub timeout_ms: u32,
    pub expected_status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_contains: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<serde_json::Value>,
    pub is_public: bool,
    pub is_paused: bool,
    pub current_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<String>,
    pub confirmation_threshold: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_time_threshold_ms: Option<u32>,
    pub follow_redirects: bool,
    pub dns_record_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns_expected: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sla_target: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sla_period_days: Option<u32>,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consensus_threshold: Option<u32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateMonitor {
    pub name: String,
    pub url: String,
    #[serde(default = "default_monitor_type")]
    pub monitor_type: Option<String>,
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default = "default_interval")]
    pub interval_seconds: Option<u32>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: Option<u32>,
    #[serde(default = "default_status")]
    pub expected_status: Option<u16>,
    pub body_contains: Option<String>,
    pub headers: Option<serde_json::Value>,
    #[serde(default)]
    pub is_public: bool,
    pub confirmation_threshold: Option<u32>,
    pub response_time_threshold_ms: Option<u32>,
    #[serde(default = "default_follow_redirects")]
    pub follow_redirects: Option<bool>,
    #[serde(default = "default_dns_record_type")]
    pub dns_record_type: Option<String>,
    pub dns_expected: Option<String>,
    pub sla_target: Option<f64>,
    pub sla_period_days: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_flexible_tags")]
    pub tags: Vec<String>,
    pub group_name: Option<String>,
    pub consensus_threshold: Option<u32>,
}

fn default_follow_redirects() -> Option<bool> { Some(true) }
fn default_monitor_type() -> Option<String> { Some("http".into()) }
fn default_dns_record_type() -> Option<String> { Some("A".into()) }
fn default_method() -> String { "GET".into() }
fn default_interval() -> Option<u32> { Some(600) }
fn default_timeout() -> Option<u32> { Some(10000) }
fn default_status() -> Option<u16> { Some(200) }

#[derive(Debug, Deserialize)]
pub struct UpdateMonitor {
    pub name: Option<String>,
    pub url: Option<String>,
    pub monitor_type: Option<String>,
    pub method: Option<String>,
    pub interval_seconds: Option<u32>,
    pub timeout_ms: Option<u32>,
    pub expected_status: Option<u16>,
    pub body_contains: Option<String>,
    pub headers: Option<serde_json::Value>,
    pub is_public: Option<bool>,
    pub confirmation_threshold: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_optional_nullable")]
    pub response_time_threshold_ms: Option<Option<u32>>,
    pub follow_redirects: Option<bool>,
    pub dns_record_type: Option<String>,
    pub dns_expected: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_nullable")]
    pub sla_target: Option<Option<f64>>,
    #[serde(default, deserialize_with = "deserialize_optional_nullable")]
    pub sla_period_days: Option<Option<u32>>,
    #[serde(default, deserialize_with = "deserialize_optional_flexible_tags")]
    pub tags: Option<Vec<String>>,
    pub group_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_nullable")]
    pub consensus_threshold: Option<Option<u32>>,
}

#[derive(Debug, Serialize, Clone)]
pub struct Heartbeat {
    pub id: String,
    pub monitor_id: String,
    pub status: String,
    pub response_time_ms: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub checked_at: String,
    pub seq: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_id: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct Incident {
    pub id: String,
    pub monitor_id: String,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
    pub cause: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acknowledgement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acknowledged_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acknowledged_at: Option<String>,
    pub seq: i64,
}

#[derive(Debug, Deserialize)]
pub struct AcknowledgeIncident {
    pub note: String,
    #[serde(default = "default_actor")]
    pub actor: String,
}

fn default_actor() -> String { "anonymous".into() }

#[derive(Debug, Serialize)]
pub struct CreateMonitorResponse {
    pub monitor: Monitor,
    pub manage_key: String,
    pub manage_url: String,
    pub view_url: String,
    pub api_base: String,
}

#[derive(Debug, Serialize)]
pub struct UptimeStats {
    pub monitor_id: String,
    pub uptime_24h: f64,
    pub uptime_7d: f64,
    pub uptime_30d: f64,
    pub uptime_90d: f64,
    pub total_checks_24h: u32,
    pub total_checks_7d: u32,
    pub total_checks_30d: u32,
    pub total_checks_90d: u32,
    pub avg_response_ms_24h: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct StatusOverview {
    pub monitors: Vec<StatusMonitor>,
    pub overall: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branding: Option<StatusPageBranding>,
}

#[derive(Debug, Serialize)]
pub struct StatusMonitor {
    pub id: String,
    pub name: String,
    pub url: String,
    pub current_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<String>,
    pub uptime_24h: f64,
    pub uptime_7d: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_response_ms_24h: Option<f64>,
    pub active_incident: bool,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NotificationChannel {
    pub id: String,
    pub monitor_id: String,
    pub name: String,
    pub channel_type: String,
    pub config: serde_json::Value,
    pub is_enabled: bool,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateNotification {
    pub name: String,
    pub channel_type: String,
    pub config: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct BulkCreateMonitors {
    pub monitors: Vec<CreateMonitor>,
}

#[derive(Debug, Serialize)]
pub struct BulkCreateResponse {
    pub created: Vec<CreateMonitorResponse>,
    pub errors: Vec<BulkError>,
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
}

#[derive(Debug, Serialize)]
pub struct BulkError {
    pub index: usize,
    pub error: String,
    pub code: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct MaintenanceWindow {
    pub id: String,
    pub monitor_id: String,
    pub title: String,
    pub starts_at: String,
    pub ends_at: String,
    pub active: bool,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateMaintenanceWindow {
    pub title: String,
    pub starts_at: String,
    pub ends_at: String,
}

#[derive(Debug, Serialize)]
pub struct DashboardOverview {
    pub total_monitors: u32,
    pub public_monitors: u32,
    pub paused_monitors: u32,
    pub status_counts: StatusCounts,
    pub active_incidents: u32,
    pub avg_uptime_24h: f64,
    pub avg_uptime_7d: f64,
    pub avg_response_ms_24h: Option<f64>,
    pub total_checks_24h: u32,
    pub recent_incidents: Vec<DashboardIncident>,
    pub slowest_monitors: Vec<SlowMonitor>,
}

#[derive(Debug, Serialize)]
pub struct StatusCounts {
    pub up: u32,
    pub down: u32,
    pub degraded: u32,
    pub unknown: u32,
    pub maintenance: u32,
}

#[derive(Debug, Serialize)]
pub struct DashboardIncident {
    pub id: String,
    pub monitor_id: String,
    pub monitor_name: String,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
    pub cause: String,
}

#[derive(Debug, Serialize)]
pub struct SlowMonitor {
    pub id: String,
    pub name: String,
    pub avg_response_ms: f64,
    pub current_status: String,
}

#[derive(Debug, Serialize)]
pub struct ExportedMonitor {
    pub name: String,
    pub url: String,
    pub monitor_type: String,
    pub method: String,
    pub interval_seconds: u32,
    pub timeout_ms: u32,
    pub expected_status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_contains: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<serde_json::Value>,
    pub is_public: bool,
    pub confirmation_threshold: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_time_threshold_ms: Option<u32>,
    pub follow_redirects: bool,
    pub dns_record_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns_expected: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sla_target: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sla_period_days: Option<u32>,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consensus_threshold: Option<u32>,
}

#[derive(Debug, Serialize, Clone)]
pub struct StatusPageBranding {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSettings {
    pub title: Option<String>,
    pub description: Option<String>,
    pub logo_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SettingsResponse {
    pub title: Option<String>,
    pub description: Option<String>,
    pub logo_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SlaStatus {
    pub monitor_id: String,
    pub target_pct: f64,
    pub period_days: u32,
    pub current_pct: f64,
    pub total_checks: u32,
    pub successful_checks: u32,
    pub downtime_estimate_seconds: f64,
    pub budget_total_seconds: f64,
    pub budget_remaining_seconds: f64,
    pub budget_used_pct: f64,
    pub status: String,
    pub period_start: String,
    pub period_end: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct IncidentNote {
    pub id: String,
    pub incident_id: String,
    pub content: String,
    pub author: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateIncidentNote {
    pub content: String,
    #[serde(default = "default_actor")]
    pub author: String,
}

#[derive(Debug, Serialize)]
pub struct UptimeHistoryDay {
    pub date: String,
    pub uptime_pct: f64,
    pub total_checks: u32,
    pub up_checks: u32,
    pub down_checks: u32,
    pub avg_response_ms: Option<f64>,
}

// ── Check Locations (Multi-Region) ──

#[derive(Debug, Serialize, Clone)]
pub struct CheckLocation {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    pub is_active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<String>,
    pub created_at: String,
    /// Computed health status: "healthy" (active + seen recently), "stale" (active but no recent report),
    /// "disabled" (is_active=false), or "new" (never reported).
    pub health_status: String,
}

impl CheckLocation {
    /// Compute health status based on is_active, last_seen_at, and stale threshold.
    pub fn compute_health(is_active: bool, last_seen_at: &Option<String>, stale_minutes: u32) -> String {
        if !is_active {
            return "disabled".to_string();
        }
        match last_seen_at {
            None => "new".to_string(),
            Some(ts) => {
                // Parse last_seen_at and compare with now
                if let Ok(seen) = chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S") {
                    let now = chrono::Utc::now().naive_utc();
                    let elapsed = now.signed_duration_since(seen);
                    if elapsed.num_minutes() > stale_minutes as i64 {
                        "stale".to_string()
                    } else {
                        "healthy".to_string()
                    }
                } else {
                    "unknown".to_string()
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateCheckLocation {
    pub name: String,
    pub region: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateCheckLocationResponse {
    pub location: CheckLocation,
    pub probe_key: String,
}

#[derive(Debug, Deserialize)]
pub struct ProbeResult {
    pub monitor_id: String,
    pub status: String,
    pub response_time_ms: u32,
    pub status_code: Option<u16>,
    pub error_message: Option<String>,
    pub checked_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProbeSubmission {
    pub results: Vec<ProbeResult>,
}

#[derive(Debug, Serialize)]
pub struct ProbeSubmissionResponse {
    pub accepted: usize,
    pub rejected: usize,
    pub errors: Vec<ProbeError>,
}

#[derive(Debug, Serialize)]
pub struct ProbeError {
    pub index: usize,
    pub monitor_id: String,
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct MonitorLocationStatus {
    pub location_id: String,
    pub location_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    pub last_status: String,
    pub last_response_time_ms: u32,
    pub last_checked_at: String,
}

#[derive(Debug, Serialize)]
pub struct ConsensusStatus {
    pub monitor_id: String,
    pub consensus_threshold: u32,
    pub total_locations: u32,
    pub up_count: u32,
    pub down_count: u32,
    pub degraded_count: u32,
    pub unknown_count: u32,
    pub effective_status: String,
    pub locations: Vec<ConsensusLocationDetail>,
}

#[derive(Debug, Serialize)]
pub struct ConsensusLocationDetail {
    pub location_id: Option<String>,
    pub location_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    pub last_status: String,
    pub last_response_time_ms: u32,
    pub last_checked_at: String,
}

// ── Status Pages ──

#[derive(Debug, Serialize, Clone)]
pub struct StatusPage {
    pub id: String,
    pub slug: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_domain: Option<String>,
    pub is_public: bool,
    pub monitor_count: u32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateStatusPage {
    pub slug: String,
    pub title: String,
    pub description: Option<String>,
    pub logo_url: Option<String>,
    pub custom_domain: Option<String>,
    #[serde(default = "default_true")]
    pub is_public: bool,
}

fn default_true() -> bool { true }

#[derive(Debug, Deserialize)]
pub struct UpdateStatusPage {
    pub slug: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub logo_url: Option<String>,
    pub custom_domain: Option<String>,
    pub is_public: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct CreateStatusPageResponse {
    pub status_page: StatusPage,
    pub manage_key: String,
}

#[derive(Debug, Serialize)]
pub struct StatusPageDetail {
    pub id: String,
    pub slug: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_domain: Option<String>,
    pub is_public: bool,
    pub monitors: Vec<StatusMonitor>,
    pub overall: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct AddMonitorsToPage {
    pub monitor_ids: Vec<String>,
}

// ── Alert Rules ──

#[derive(Debug, Serialize, Clone)]
pub struct AlertRule {
    pub monitor_id: String,
    pub repeat_interval_minutes: u32,
    pub max_repeats: u32,
    pub escalation_after_minutes: u32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateAlertRule {
    /// Minutes between repeat notifications while incident is open. 0 = disabled.
    #[serde(default)]
    pub repeat_interval_minutes: u32,
    /// Maximum repeat notifications per incident. Default 10.
    #[serde(default = "default_max_repeats")]
    pub max_repeats: u32,
    /// Minutes before escalation if incident not acknowledged. 0 = disabled.
    #[serde(default)]
    pub escalation_after_minutes: u32,
}

fn default_max_repeats() -> u32 { 10 }

// ── Monitor Dependencies ──

#[derive(Debug, Serialize, Clone)]
pub struct MonitorDependency {
    pub id: String,
    pub monitor_id: String,
    pub depends_on_id: String,
    pub depends_on_name: String,
    pub depends_on_status: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateDependency {
    pub depends_on_id: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct AlertLogEntry {
    pub id: String,
    pub monitor_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incident_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    pub alert_type: String,
    pub event: String,
    pub sent_at: String,
}
