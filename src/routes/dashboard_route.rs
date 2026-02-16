use rocket::{get, serde::json::Json, State, http::Status};
use crate::db::Db;
use crate::models::{DashboardOverview, StatusCounts, DashboardIncident, SlowMonitor};
use crate::auth::{OptionalManageToken, hash_key};
use crate::routes::settings::get_setting;
use rusqlite::params;
use std::sync::Arc;

/// Check if the provided token matches the admin key.
fn is_admin(conn: &rusqlite::Connection, token: &Option<String>) -> bool {
    let Some(ref key) = token else { return false };
    let Ok(stored_hash) = conn.query_row(
        "SELECT value FROM settings WHERE key = 'admin_key_hash'",
        [],
        |row| row.get::<_, String>(0),
    ) else { return false };
    hash_key(key) == stored_hash
}

// ── Dashboard ──

#[get("/dashboard")]
pub fn dashboard(token: OptionalManageToken, db: &State<Arc<Db>>) -> Result<Json<DashboardOverview>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    let admin = is_admin(&conn, &token.0);

    let total_monitors: u32 = conn.query_row("SELECT COUNT(*) FROM monitors", [], |r| r.get(0)).unwrap_or(0);
    let public_monitors: u32 = conn.query_row("SELECT COUNT(*) FROM monitors WHERE is_public = 1", [], |r| r.get(0)).unwrap_or(0);
    let paused_monitors: u32 = conn.query_row("SELECT COUNT(*) FROM monitors WHERE is_paused = 1", [], |r| r.get(0)).unwrap_or(0);

    let count_status = |s: &str| -> u32 {
        conn.query_row("SELECT COUNT(*) FROM monitors WHERE current_status = ?1", params![s], |r| r.get(0)).unwrap_or(0)
    };
    let status_counts = StatusCounts {
        up: count_status("up"),
        down: count_status("down"),
        degraded: count_status("degraded"),
        unknown: count_status("unknown"),
        maintenance: count_status("maintenance"),
    };

    let active_incidents: u32 = conn.query_row(
        "SELECT COUNT(*) FROM incidents WHERE resolved_at IS NULL", [], |r| r.get(0)
    ).unwrap_or(0);

    let total_checks_24h: u32 = conn.query_row(
        "SELECT COUNT(*) FROM heartbeats WHERE checked_at > datetime('now', '-24 hours')", [], |r| r.get(0)
    ).unwrap_or(0);
    let up_checks_24h: u32 = conn.query_row(
        "SELECT COUNT(*) FROM heartbeats WHERE status = 'up' AND checked_at > datetime('now', '-24 hours')", [], |r| r.get(0)
    ).unwrap_or(0);
    let avg_uptime_24h = if total_checks_24h > 0 { (up_checks_24h as f64 / total_checks_24h as f64) * 100.0 } else { 100.0 };

    let total_checks_7d: u32 = conn.query_row(
        "SELECT COUNT(*) FROM heartbeats WHERE checked_at > datetime('now', '-7 days')", [], |r| r.get(0)
    ).unwrap_or(0);
    let up_checks_7d: u32 = conn.query_row(
        "SELECT COUNT(*) FROM heartbeats WHERE status = 'up' AND checked_at > datetime('now', '-7 days')", [], |r| r.get(0)
    ).unwrap_or(0);
    let avg_uptime_7d = if total_checks_7d > 0 { (up_checks_7d as f64 / total_checks_7d as f64) * 100.0 } else { 100.0 };

    let avg_response_ms_24h: Option<f64> = conn.query_row(
        "SELECT AVG(response_time_ms) FROM heartbeats WHERE status = 'up' AND checked_at > datetime('now', '-24 hours')",
        [], |r| r.get(0)
    ).ok();

    // Only include individual monitor data for admin users
    let recent_incidents: Vec<DashboardIncident> = if admin {
        let mut stmt = conn.prepare(
            "SELECT i.id, i.monitor_id, m.name, i.started_at, i.resolved_at, i.cause \
             FROM incidents i JOIN monitors m ON i.monitor_id = m.id \
             ORDER BY i.started_at DESC LIMIT 10"
        ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;
        let rows: Vec<DashboardIncident> = stmt.query_map([], |row| {
            Ok(DashboardIncident {
                id: row.get(0)?,
                monitor_id: row.get(1)?,
                monitor_name: row.get(2)?,
                started_at: row.get(3)?,
                resolved_at: row.get(4)?,
                cause: row.get(5)?,
            })
        }).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?
        .filter_map(|r| r.ok())
        .collect();
        rows
    } else {
        vec![]
    };

    let slowest_monitors: Vec<SlowMonitor> = if admin {
        let mut stmt = conn.prepare(
            "SELECT m.id, m.name, AVG(h.response_time_ms) as avg_ms, m.current_status \
             FROM heartbeats h JOIN monitors m ON h.monitor_id = m.id \
             WHERE h.status = 'up' AND h.checked_at > datetime('now', '-24 hours') \
             GROUP BY m.id \
             ORDER BY avg_ms DESC LIMIT 5"
        ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;
        let rows: Vec<SlowMonitor> = stmt.query_map([], |row| {
            Ok(SlowMonitor {
                id: row.get(0)?,
                name: row.get(1)?,
                avg_response_ms: row.get(2)?,
                current_status: row.get(3)?,
            })
        }).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?
        .filter_map(|r| r.ok())
        .collect();
        rows
    } else {
        vec![]
    };

    Ok(Json(DashboardOverview {
        total_monitors,
        public_monitors,
        paused_monitors,
        status_counts,
        active_incidents,
        avg_uptime_24h,
        avg_uptime_7d,
        avg_response_ms_24h,
        total_checks_24h,
        recent_incidents,
        slowest_monitors,
    }))
}

// ── Admin Verify ──

#[get("/admin/verify")]
pub fn admin_verify(token: OptionalManageToken, db: &State<Arc<Db>>) -> Result<Json<serde_json::Value>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    if is_admin(&conn, &token.0) {
        Ok(Json(serde_json::json!({"valid": true})))
    } else {
        Ok(Json(serde_json::json!({"valid": false})))
    }
}
