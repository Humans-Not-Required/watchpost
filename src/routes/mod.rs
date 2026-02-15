// Route modules — decomposed from monolithic routes.rs
// Each module handles a domain of the API.

mod monitors;
mod heartbeats;
mod incidents;
mod dashboard_route;
mod uptime;
mod status;
mod notifications;
mod maintenance;
mod tags;
mod settings;
mod system;
mod badges;
mod sla;
mod stream;
mod locations;

// Re-export all route handlers so main.rs can use routes::* unchanged
pub use monitors::{create_monitor, bulk_create_monitors, export_monitor, list_monitors, get_monitor, update_monitor, delete_monitor, pause_monitor, resume_monitor};
pub use heartbeats::{get_heartbeats, get_uptime};
pub use incidents::{get_incidents, get_incident, acknowledge_incident, create_incident_note, list_incident_notes};
pub use dashboard_route::dashboard;
pub use uptime::{uptime_history, monitor_uptime_history};
pub use status::status_page;
pub use notifications::{create_notification, list_notifications, delete_notification, update_notification};
pub use maintenance::{create_maintenance_window, list_maintenance_windows, delete_maintenance_window, is_in_maintenance};
pub use tags::{list_tags, list_groups};
pub use settings::{get_settings, update_settings};
pub use system::{health, llms_txt, openapi_spec, spa_fallback};
pub use badges::{monitor_uptime_badge, monitor_status_badge};
pub use sla::monitor_sla;
pub use stream::{global_events, monitor_events};
pub use locations::{create_location, list_locations, get_location, delete_location, submit_probe, monitor_location_status};

use rocket::{http::Status, serde::json::Json};
use crate::models::Monitor;
use crate::auth::{hash_key};
use rusqlite::params;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

// ── Rate Limiter ──

pub struct RateLimiter {
    pub windows: Mutex<HashMap<String, (Instant, u32)>>,
    pub limit: u32,
    pub window_secs: u64,
}

impl RateLimiter {
    pub fn new(limit: u32, window_secs: u64) -> Self {
        RateLimiter {
            windows: Mutex::new(HashMap::new()),
            limit,
            window_secs,
        }
    }

    pub fn check(&self, key: &str) -> bool {
        let mut windows = self.windows.lock().unwrap();
        let now = Instant::now();
        let entry = windows.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0).as_secs() >= self.window_secs {
            *entry = (now, 1);
            true
        } else if entry.1 < self.limit {
            entry.1 += 1;
            true
        } else {
            false
        }
    }
}

// ── Shared Helpers ──

/// Valid DNS record types for DNS monitors
pub(crate) const VALID_DNS_RECORD_TYPES: &[&str] = &["A", "AAAA", "CNAME", "MX", "TXT", "NS", "SOA", "PTR", "SRV", "CAA"];

pub(crate) fn get_monitor_from_db(conn: &rusqlite::Connection, id: &str) -> rusqlite::Result<Monitor> {
    conn.query_row(
        "SELECT id, name, url, method, interval_seconds, timeout_ms, expected_status, body_contains, headers, is_public, is_paused, current_status, last_checked_at, confirmation_threshold, created_at, updated_at, tags, response_time_threshold_ms, follow_redirects, group_name, monitor_type, dns_record_type, dns_expected, sla_target, sla_period_days
         FROM monitors WHERE id = ?1",
        params![id],
        |row| Ok(row_to_monitor(row)),
    )
}

pub(crate) fn row_to_monitor(row: &rusqlite::Row) -> Monitor {
    let headers_str: Option<String> = row.get(8).unwrap_or(None);
    let tags_str: String = row.get(16).unwrap_or_default();
    Monitor {
        id: row.get(0).unwrap(),
        name: row.get(1).unwrap(),
        url: row.get(2).unwrap(),
        monitor_type: row.get::<_, String>(20).unwrap_or_else(|_| "http".to_string()),
        method: row.get(3).unwrap(),
        interval_seconds: row.get(4).unwrap(),
        timeout_ms: row.get(5).unwrap(),
        expected_status: row.get(6).unwrap(),
        body_contains: row.get(7).unwrap_or(None),
        headers: headers_str.and_then(|s| serde_json::from_str(&s).ok()),
        is_public: row.get::<_, i32>(9).unwrap() != 0,
        is_paused: row.get::<_, i32>(10).unwrap() != 0,
        current_status: row.get(11).unwrap(),
        last_checked_at: row.get(12).unwrap_or(None),
        confirmation_threshold: row.get(13).unwrap(),
        response_time_threshold_ms: row.get::<_, Option<u32>>(17).unwrap_or(None),
        follow_redirects: row.get::<_, i32>(18).unwrap_or(1) != 0,
        dns_record_type: row.get::<_, String>(21).unwrap_or_else(|_| "A".to_string()),
        dns_expected: row.get::<_, Option<String>>(22).unwrap_or(None),
        sla_target: row.get::<_, Option<f64>>(23).unwrap_or(None),
        sla_period_days: row.get::<_, Option<u32>>(24).unwrap_or(None),
        tags: parse_tags(&tags_str),
        group_name: row.get::<_, Option<String>>(19).unwrap_or(None),
        created_at: row.get(14).unwrap(),
        updated_at: row.get(15).unwrap(),
    }
}

pub(crate) fn parse_tags(raw: &str) -> Vec<String> {
    if raw.is_empty() {
        Vec::new()
    } else {
        raw.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    }
}

pub(crate) fn tags_to_string(tags: &[String]) -> String {
    tags.iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) fn verify_manage_key(conn: &rusqlite::Connection, monitor_id: &str, token: &str) -> Result<(), (Status, Json<serde_json::Value>)> {
    let stored_hash: String = conn.query_row(
        "SELECT manage_key_hash FROM monitors WHERE id = ?1",
        params![monitor_id],
        |row| row.get(0),
    ).map_err(|_| (Status::NotFound, Json(serde_json::json!({
        "error": "Monitor not found", "code": "NOT_FOUND"
    }))))?;

    if hash_key(token) != stored_hash {
        return Err((Status::Forbidden, Json(serde_json::json!({
            "error": "Invalid manage key", "code": "FORBIDDEN"
        }))));
    }
    Ok(())
}

/// Validate TCP address format: host:port (port must be 1-65535)
pub(crate) fn validate_tcp_address(addr: &str) -> Result<(), (Status, Json<serde_json::Value>)> {
    let addr = addr.strip_prefix("tcp://").unwrap_or(addr);
    let parts: Vec<&str> = addr.rsplitn(2, ':').collect();
    if parts.len() != 2 || parts[1].is_empty() {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "TCP address must be in host:port format (e.g., 'example.com:443' or 'tcp://example.com:443')",
            "code": "VALIDATION_ERROR"
        }))));
    }
    match parts[0].parse::<u16>() {
        Ok(0) => Err((Status::BadRequest, Json(serde_json::json!({
            "error": "Port must be between 1 and 65535", "code": "VALIDATION_ERROR"
        })))),
        Ok(_) => Ok(()),
        Err(_) => Err((Status::BadRequest, Json(serde_json::json!({
            "error": "Invalid port number in TCP address", "code": "VALIDATION_ERROR"
        })))),
    }
}

/// Validate DNS hostname format (optional dns:// prefix)
pub(crate) fn validate_dns_hostname(host: &str) -> Result<(), (Status, Json<serde_json::Value>)> {
    let host = host.strip_prefix("dns://").unwrap_or(host);
    if host.is_empty() {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "DNS hostname cannot be empty",
            "code": "VALIDATION_ERROR"
        }))));
    }
    if host.contains(' ') || host.contains("://") {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "DNS hostname must be a valid domain (e.g., 'example.com' or 'dns://example.com')",
            "code": "VALIDATION_ERROR"
        }))));
    }
    Ok(())
}
