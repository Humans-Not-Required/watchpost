use rocket::{get, serde::json::Json, State, http::Status};
use crate::db::Db;
use crate::models::{Heartbeat, UptimeStats};
use super::get_monitor_from_db;
use rusqlite::params;
use std::sync::Arc;

// ── Heartbeats ──

#[get("/monitors/<id>/heartbeats?<limit>&<after>")]
pub fn get_heartbeats(
    id: &str,
    limit: Option<u32>,
    after: Option<i64>,
    db: &State<Arc<Db>>,
) -> Result<Json<Vec<Heartbeat>>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    get_monitor_from_db(&conn, id)
        .map_err(|_| (Status::NotFound, Json(serde_json::json!({"error": "Monitor not found", "code": "NOT_FOUND"}))))?;

    let limit = limit.unwrap_or(50).min(200);
    let err_map = |e: rusqlite::Error| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()})));

    let row_to_hb = |row: &rusqlite::Row| -> rusqlite::Result<Heartbeat> {
        Ok(Heartbeat {
            id: row.get(0)?,
            monitor_id: row.get(1)?,
            status: row.get(2)?,
            response_time_ms: row.get(3)?,
            status_code: row.get(4)?,
            error_message: row.get(5)?,
            checked_at: row.get(6)?,
            seq: row.get(7)?,
        })
    };

    let heartbeats: Vec<Heartbeat> = if let Some(after_seq) = after {
        let mut stmt = conn.prepare(
            "SELECT id, monitor_id, status, response_time_ms, status_code, error_message, checked_at, seq
             FROM heartbeats WHERE monitor_id = ?1 AND seq > ?2 ORDER BY seq ASC LIMIT ?3"
        ).map_err(err_map)?;
        let results: Vec<Heartbeat> = stmt.query_map(params![id, after_seq, limit], row_to_hb)
            .map_err(err_map)?
            .filter_map(|r| r.ok())
            .collect();
        results
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, monitor_id, status, response_time_ms, status_code, error_message, checked_at, seq
             FROM heartbeats WHERE monitor_id = ?1 ORDER BY seq DESC LIMIT ?2"
        ).map_err(err_map)?;
        let results: Vec<Heartbeat> = stmt.query_map(params![id, limit], row_to_hb)
            .map_err(err_map)?
            .filter_map(|r| r.ok())
            .collect();
        results
    };

    Ok(Json(heartbeats))
}

// ── Uptime Stats ──

#[get("/monitors/<id>/uptime")]
pub fn get_uptime(
    id: &str,
    db: &State<Arc<Db>>,
) -> Result<Json<UptimeStats>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    get_monitor_from_db(&conn, id)
        .map_err(|_| (Status::NotFound, Json(serde_json::json!({"error": "Monitor not found", "code": "NOT_FOUND"}))))?;

    let calc_uptime = |hours: u32| -> (f64, u32) {
        let total: u32 = conn.query_row(
            "SELECT COUNT(*) FROM heartbeats WHERE monitor_id = ?1 AND checked_at > datetime('now', ?2)",
            params![id, format!("-{} hours", hours)],
            |row| row.get(0),
        ).unwrap_or(0);
        let up: u32 = conn.query_row(
            "SELECT COUNT(*) FROM heartbeats WHERE monitor_id = ?1 AND status = 'up' AND checked_at > datetime('now', ?2)",
            params![id, format!("-{} hours", hours)],
            |row| row.get(0),
        ).unwrap_or(0);
        let pct = if total > 0 { (up as f64 / total as f64) * 100.0 } else { 100.0 };
        (pct, total)
    };

    let (u24, t24) = calc_uptime(24);
    let (u7d, t7d) = calc_uptime(168);
    let (u30d, t30d) = calc_uptime(720);
    let (u90d, t90d) = calc_uptime(2160);

    let avg_ms: Option<f64> = conn.query_row(
        "SELECT AVG(response_time_ms) FROM heartbeats WHERE monitor_id = ?1 AND status = 'up' AND checked_at > datetime('now', '-24 hours')",
        params![id],
        |row| row.get(0),
    ).ok();

    Ok(Json(UptimeStats {
        monitor_id: id.to_string(),
        uptime_24h: u24,
        uptime_7d: u7d,
        uptime_30d: u30d,
        uptime_90d: u90d,
        total_checks_24h: t24,
        total_checks_7d: t7d,
        total_checks_30d: t30d,
        total_checks_90d: t90d,
        avg_response_ms_24h: avg_ms,
    }))
}
