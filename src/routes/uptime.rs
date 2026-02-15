use rocket::{get, serde::json::Json, State, http::Status};
use crate::db::Db;
use crate::models::UptimeHistoryDay;
use super::get_monitor_from_db;
use rusqlite::params;
use std::sync::Arc;

// ── Uptime History ──

#[get("/uptime-history?<days>")]
pub fn uptime_history(
    days: Option<u32>,
    db: &State<Arc<Db>>,
) -> Result<Json<Vec<UptimeHistoryDay>>, (Status, Json<serde_json::Value>)> {
    let days = days.unwrap_or(30).min(90).max(1);
    let conn = db.conn.lock().unwrap();
    let err_map = |e: rusqlite::Error| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()})));

    let mut stmt = conn.prepare(
        "SELECT date(checked_at) as day, \
         COUNT(*) as total, \
         SUM(CASE WHEN status = 'up' THEN 1 ELSE 0 END) as up_count, \
         SUM(CASE WHEN status = 'down' THEN 1 ELSE 0 END) as down_count, \
         AVG(CASE WHEN status = 'up' THEN response_time_ms ELSE NULL END) as avg_rt \
         FROM heartbeats \
         WHERE checked_at > datetime('now', ?1) \
         GROUP BY day ORDER BY day ASC"
    ).map_err(err_map)?;

    let offset_str = format!("-{} days", days);
    let rows: Vec<UptimeHistoryDay> = stmt.query_map(params![offset_str], |row| {
        let total: u32 = row.get(1)?;
        let up: u32 = row.get(2)?;
        let pct = if total > 0 { (up as f64 / total as f64) * 100.0 } else { 100.0 };
        Ok(UptimeHistoryDay {
            date: row.get(0)?,
            uptime_pct: pct,
            total_checks: total,
            up_checks: up,
            down_checks: row.get(3)?,
            avg_response_ms: row.get(4)?,
        })
    }).map_err(err_map)?
    .filter_map(|r| r.ok())
    .collect();

    Ok(Json(rows))
}

#[get("/monitors/<id>/uptime-history?<days>")]
pub fn monitor_uptime_history(
    id: &str,
    days: Option<u32>,
    db: &State<Arc<Db>>,
) -> Result<Json<Vec<UptimeHistoryDay>>, (Status, Json<serde_json::Value>)> {
    let days = days.unwrap_or(30).min(90).max(1);
    let conn = db.conn.lock().unwrap();
    let err_map = |e: rusqlite::Error| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()})));

    get_monitor_from_db(&conn, id)
        .map_err(|_| (Status::NotFound, Json(serde_json::json!({"error": "Monitor not found", "code": "NOT_FOUND"}))))?;

    let mut stmt = conn.prepare(
        "SELECT date(checked_at) as day, \
         COUNT(*) as total, \
         SUM(CASE WHEN status = 'up' THEN 1 ELSE 0 END) as up_count, \
         SUM(CASE WHEN status = 'down' THEN 1 ELSE 0 END) as down_count, \
         AVG(CASE WHEN status = 'up' THEN response_time_ms ELSE NULL END) as avg_rt \
         FROM heartbeats \
         WHERE monitor_id = ?1 AND checked_at > datetime('now', ?2) \
         GROUP BY day ORDER BY day ASC"
    ).map_err(err_map)?;

    let offset_str = format!("-{} days", days);
    let rows: Vec<UptimeHistoryDay> = stmt.query_map(params![id, offset_str], |row| {
        let total: u32 = row.get(1)?;
        let up: u32 = row.get(2)?;
        let pct = if total > 0 { (up as f64 / total as f64) * 100.0 } else { 100.0 };
        Ok(UptimeHistoryDay {
            date: row.get(0)?,
            uptime_pct: pct,
            total_checks: total,
            up_checks: up,
            down_checks: row.get(3)?,
            avg_response_ms: row.get(4)?,
        })
    }).map_err(err_map)?
    .filter_map(|r| r.ok())
    .collect();

    Ok(Json(rows))
}
