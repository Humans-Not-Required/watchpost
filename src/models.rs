use serde::{Deserialize, Serialize};

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
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
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
    #[serde(default)]
    pub tags: Vec<String>,
    pub group_name: Option<String>,
}

fn default_follow_redirects() -> Option<bool> { Some(true) }
fn default_monitor_type() -> Option<String> { Some("http".into()) }
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
    pub tags: Option<Vec<String>>,
    pub group_name: Option<String>,
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
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
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
pub struct UptimeHistoryDay {
    pub date: String,
    pub uptime_pct: f64,
    pub total_checks: u32,
    pub up_checks: u32,
    pub down_checks: u32,
    pub avg_response_ms: Option<f64>,
}
