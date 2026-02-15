use rocket::{get, put, delete, serde::json::Json, State, http::Status};
use crate::db::Db;
use crate::models::{AlertRule, CreateAlertRule, AlertLogEntry};
use crate::auth::ManageToken;
use super::verify_manage_key;
use rusqlite::params;
use std::sync::Arc;

// ── Alert Rules ──

#[put("/monitors/<id>/alert-rules", format = "json", data = "<input>")]
pub fn set_alert_rules(
    id: &str,
    input: Json<CreateAlertRule>,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<AlertRule>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    verify_manage_key(&conn, id, &token.0)?;

    let data = input.into_inner();

    // Validate
    if data.repeat_interval_minutes > 0 && data.repeat_interval_minutes < 5 {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "repeat_interval_minutes must be 0 (disabled) or at least 5",
            "code": "VALIDATION_ERROR"
        }))));
    }
    if data.max_repeats > 100 {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "max_repeats must be 100 or less",
            "code": "VALIDATION_ERROR"
        }))));
    }
    if data.escalation_after_minutes > 0 && data.escalation_after_minutes < 5 {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "escalation_after_minutes must be 0 (disabled) or at least 5",
            "code": "VALIDATION_ERROR"
        }))));
    }

    conn.execute(
        "INSERT INTO alert_rules (monitor_id, repeat_interval_minutes, max_repeats, escalation_after_minutes, updated_at)
         VALUES (?1, ?2, ?3, ?4, datetime('now'))
         ON CONFLICT(monitor_id) DO UPDATE SET
           repeat_interval_minutes = excluded.repeat_interval_minutes,
           max_repeats = excluded.max_repeats,
           escalation_after_minutes = excluded.escalation_after_minutes,
           updated_at = datetime('now')",
        params![id, data.repeat_interval_minutes, data.max_repeats, data.escalation_after_minutes],
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    let rule = conn.query_row(
        "SELECT monitor_id, repeat_interval_minutes, max_repeats, escalation_after_minutes, created_at, updated_at
         FROM alert_rules WHERE monitor_id = ?1",
        params![id],
        |row| Ok(AlertRule {
            monitor_id: row.get(0)?,
            repeat_interval_minutes: row.get(1)?,
            max_repeats: row.get(2)?,
            escalation_after_minutes: row.get(3)?,
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
        }),
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    Ok(Json(rule))
}

#[get("/monitors/<id>/alert-rules")]
pub fn get_alert_rules(
    id: &str,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<AlertRule>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    verify_manage_key(&conn, id, &token.0)?;

    let rule = conn.query_row(
        "SELECT monitor_id, repeat_interval_minutes, max_repeats, escalation_after_minutes, created_at, updated_at
         FROM alert_rules WHERE monitor_id = ?1",
        params![id],
        |row| Ok(AlertRule {
            monitor_id: row.get(0)?,
            repeat_interval_minutes: row.get(1)?,
            max_repeats: row.get(2)?,
            escalation_after_minutes: row.get(3)?,
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
        }),
    ).map_err(|_| (Status::NotFound, Json(serde_json::json!({
        "error": "No alert rules configured for this monitor",
        "code": "NOT_FOUND"
    }))))?;

    Ok(Json(rule))
}

#[delete("/monitors/<id>/alert-rules")]
pub fn delete_alert_rules(
    id: &str,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<serde_json::Value>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    verify_manage_key(&conn, id, &token.0)?;

    let changed = conn.execute(
        "DELETE FROM alert_rules WHERE monitor_id = ?1",
        params![id],
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    if changed == 0 {
        return Err((Status::NotFound, Json(serde_json::json!({
            "error": "No alert rules configured for this monitor",
            "code": "NOT_FOUND"
        }))));
    }

    Ok(Json(serde_json::json!({"message": "Alert rules removed"})))
}

// ── Alert Log ──

#[get("/monitors/<id>/alert-log?<limit>&<after>")]
pub fn get_alert_log(
    id: &str,
    limit: Option<u32>,
    after: Option<String>,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<Vec<AlertLogEntry>>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    verify_manage_key(&conn, id, &token.0)?;

    let limit = limit.unwrap_or(50).min(200);

    let entries: Vec<AlertLogEntry> = if let Some(after_ts) = after {
        let mut stmt = conn.prepare(
            "SELECT id, monitor_id, incident_id, channel_id, alert_type, event, sent_at
             FROM alert_log WHERE monitor_id = ?1 AND sent_at > ?2 ORDER BY sent_at DESC LIMIT ?3"
        ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

        let rows = stmt.query_map(params![id, after_ts, limit], |row| Ok(AlertLogEntry {
            id: row.get(0)?,
            monitor_id: row.get(1)?,
            incident_id: row.get(2)?,
            channel_id: row.get(3)?,
            alert_type: row.get(4)?,
            event: row.get(5)?,
            sent_at: row.get(6)?,
        })).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;
        rows.filter_map(|r| r.ok()).collect()
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, monitor_id, incident_id, channel_id, alert_type, event, sent_at
             FROM alert_log WHERE monitor_id = ?1 ORDER BY sent_at DESC LIMIT ?2"
        ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

        let rows = stmt.query_map(params![id, limit], |row| Ok(AlertLogEntry {
            id: row.get(0)?,
            monitor_id: row.get(1)?,
            incident_id: row.get(2)?,
            channel_id: row.get(3)?,
            alert_type: row.get(4)?,
            event: row.get(5)?,
            sent_at: row.get(6)?,
        })).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    Ok(Json(entries))
}
