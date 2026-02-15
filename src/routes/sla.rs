use rocket::{get, serde::json::Json, State, http::Status};
use crate::db::Db;
use crate::models::SlaStatus;
use super::get_monitor_from_db;
use rusqlite::params;
use std::sync::Arc;

/// GET /api/v1/monitors/:id/sla — SLA status with error budget tracking
#[get("/monitors/<id>/sla")]
pub fn monitor_sla(
    id: &str,
    db: &State<Arc<Db>>,
) -> Result<Json<SlaStatus>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    let err_map = |e: rusqlite::Error| {
        (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()})))
    };

    let monitor = get_monitor_from_db(&conn, id)
        .map_err(|_| (Status::NotFound, Json(serde_json::json!({
            "error": "Monitor not found", "code": "NOT_FOUND"
        }))))?;

    let target = monitor.sla_target.ok_or_else(|| {
        (Status::NotFound, Json(serde_json::json!({
            "error": "No SLA target configured for this monitor",
            "code": "SLA_NOT_CONFIGURED"
        })))
    })?;

    let period_days = monitor.sla_period_days.unwrap_or(30);
    let offset_str = format!("-{} days", period_days);

    // Count total and successful checks in the SLA period
    let (total_checks, successful_checks): (u32, u32) = conn.query_row(
        "SELECT COUNT(*), SUM(CASE WHEN status = 'up' OR status = 'degraded' THEN 1 ELSE 0 END)
         FROM heartbeats
         WHERE monitor_id = ?1 AND checked_at > datetime('now', ?2)",
        params![id, offset_str],
        |row| Ok((row.get(0)?, row.get::<_, u32>(1).unwrap_or(0))),
    ).map_err(err_map)?;

    // Calculate current uptime percentage
    let current_pct = if total_checks > 0 {
        (successful_checks as f64 / total_checks as f64) * 100.0
    } else {
        100.0 // No data = assume 100%
    };

    // Calculate error budget
    let total_period_seconds = period_days as f64 * 86400.0;
    let budget_total_seconds = total_period_seconds * (1.0 - target / 100.0);

    // Estimate actual downtime from heartbeat failure ratio
    // Use actual elapsed time in the period for more accurate calculation
    let elapsed_seconds: f64 = conn.query_row(
        "SELECT CAST((julianday('now') - julianday(MIN(checked_at))) * 86400.0 AS REAL)
         FROM heartbeats
         WHERE monitor_id = ?1 AND checked_at > datetime('now', ?2)",
        params![id, offset_str],
        |row| row.get::<_, Option<f64>>(0),
    ).map_err(err_map)?.unwrap_or(0.0);

    let downtime_estimate_seconds = if total_checks > 0 && elapsed_seconds > 0.0 {
        let failure_ratio = (total_checks - successful_checks) as f64 / total_checks as f64;
        failure_ratio * elapsed_seconds
    } else {
        0.0
    };

    let budget_remaining_seconds = budget_total_seconds - downtime_estimate_seconds;
    let budget_used_pct = if budget_total_seconds > 0.0 {
        ((downtime_estimate_seconds / budget_total_seconds) * 100.0).min(100.0)
    } else {
        // Target is 100% — any downtime is a breach
        if downtime_estimate_seconds > 0.0 { 100.0 } else { 0.0 }
    };

    // Determine SLA status
    // Breached: current uptime is below target (or budget exhausted)
    // At risk: budget remaining is under 25% of total
    // Met: on track
    let status = if current_pct < target || budget_remaining_seconds < 0.0 {
        "breached"
    } else if budget_total_seconds > 0.0 && budget_remaining_seconds < budget_total_seconds * 0.25 {
        "at_risk"
    } else {
        "met"
    };

    // Calculate period boundaries
    let (period_start, period_end): (String, String) = conn.query_row(
        "SELECT datetime('now', ?1), datetime('now')",
        params![offset_str],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).map_err(err_map)?;

    Ok(Json(SlaStatus {
        monitor_id: id.to_string(),
        target_pct: target,
        period_days,
        current_pct: (current_pct * 1000.0).round() / 1000.0, // 3 decimal places
        total_checks,
        successful_checks,
        downtime_estimate_seconds: (downtime_estimate_seconds * 100.0).round() / 100.0,
        budget_total_seconds: (budget_total_seconds * 100.0).round() / 100.0,
        budget_remaining_seconds: (budget_remaining_seconds * 100.0).round() / 100.0,
        budget_used_pct: (budget_used_pct * 100.0).round() / 100.0,
        status: status.to_string(),
        period_start,
        period_end,
    }))
}
