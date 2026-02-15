use rocket::{get, post, delete, serde::json::Json, State, http::Status};
use crate::db::Db;
use crate::auth::ManageToken;
use super::verify_manage_key;
use rusqlite::params;
use std::sync::Arc;

// ── Maintenance Windows ──

#[post("/monitors/<monitor_id>/maintenance", format = "json", data = "<input>")]
pub fn create_maintenance_window(
    monitor_id: &str,
    input: Json<crate::models::CreateMaintenanceWindow>,
    db: &State<Arc<Db>>,
    token: ManageToken,
) -> Result<Json<crate::models::MaintenanceWindow>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    verify_manage_key(&conn, monitor_id, &token.0)?;

    let data = input.into_inner();

    if data.title.trim().is_empty() {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "Title is required", "code": "VALIDATION_ERROR"
        }))));
    }

    let starts = chrono::NaiveDateTime::parse_from_str(&data.starts_at, "%Y-%m-%dT%H:%M:%SZ")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(&data.starts_at, "%Y-%m-%dT%H:%M:%S"))
        .map_err(|_| (Status::BadRequest, Json(serde_json::json!({
            "error": "starts_at must be ISO-8601 format (e.g. 2026-02-10T14:00:00Z)", "code": "VALIDATION_ERROR"
        }))))?;
    let ends = chrono::NaiveDateTime::parse_from_str(&data.ends_at, "%Y-%m-%dT%H:%M:%SZ")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(&data.ends_at, "%Y-%m-%dT%H:%M:%S"))
        .map_err(|_| (Status::BadRequest, Json(serde_json::json!({
            "error": "ends_at must be ISO-8601 format (e.g. 2026-02-10T15:00:00Z)", "code": "VALIDATION_ERROR"
        }))))?;
    if ends <= starts {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "ends_at must be after starts_at", "code": "VALIDATION_ERROR"
        }))));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    conn.execute(
        "INSERT INTO maintenance_windows (id, monitor_id, title, starts_at, ends_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, monitor_id, data.title.trim(), data.starts_at, data.ends_at],
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({
        "error": format!("DB error: {}", e), "code": "INTERNAL_ERROR"
    }))))?;

    let active = is_time_in_window(&now, &data.starts_at, &data.ends_at);

    Ok(Json(crate::models::MaintenanceWindow {
        id,
        monitor_id: monitor_id.to_string(),
        title: data.title.trim().to_string(),
        starts_at: data.starts_at,
        ends_at: data.ends_at,
        active,
        created_at: now,
    }))
}

#[get("/monitors/<monitor_id>/maintenance")]
pub fn list_maintenance_windows(
    monitor_id: &str,
    db: &State<Arc<Db>>,
) -> Result<Json<Vec<crate::models::MaintenanceWindow>>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();

    let _: String = conn.query_row(
        "SELECT id FROM monitors WHERE id = ?1", params![monitor_id], |r| r.get(0)
    ).map_err(|_| (Status::NotFound, Json(serde_json::json!({
        "error": "Monitor not found", "code": "NOT_FOUND"
    }))))?;

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let mut stmt = conn.prepare(
        "SELECT id, monitor_id, title, starts_at, ends_at, created_at FROM maintenance_windows WHERE monitor_id = ?1 ORDER BY starts_at DESC"
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    let windows: Vec<crate::models::MaintenanceWindow> = stmt.query_map(params![monitor_id], |row| {
        let starts_at: String = row.get(3)?;
        let ends_at: String = row.get(4)?;
        Ok(crate::models::MaintenanceWindow {
            id: row.get(0)?,
            monitor_id: row.get(1)?,
            title: row.get(2)?,
            starts_at: starts_at.clone(),
            ends_at: ends_at.clone(),
            active: is_time_in_window(&now, &starts_at, &ends_at),
            created_at: row.get(5)?,
        })
    }).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?
    .filter_map(|r| r.ok())
    .collect();

    Ok(Json(windows))
}

#[delete("/maintenance/<id>")]
pub fn delete_maintenance_window(
    id: &str,
    db: &State<Arc<Db>>,
    token: ManageToken,
) -> Result<Json<serde_json::Value>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();

    let monitor_id: String = conn.query_row(
        "SELECT monitor_id FROM maintenance_windows WHERE id = ?1", params![id], |r| r.get(0)
    ).map_err(|_| (Status::NotFound, Json(serde_json::json!({
        "error": "Maintenance window not found", "code": "NOT_FOUND"
    }))))?;

    verify_manage_key(&conn, &monitor_id, &token.0)?;

    conn.execute("DELETE FROM maintenance_windows WHERE id = ?1", params![id])
        .map_err(|e| (Status::InternalServerError, Json(serde_json::json!({
            "error": format!("DB error: {}", e), "code": "INTERNAL_ERROR"
        }))))?;

    Ok(Json(serde_json::json!({"message": "Maintenance window deleted"})))
}

/// Check if a given ISO-8601 timestamp falls within a window
fn is_time_in_window(now: &str, starts_at: &str, ends_at: &str) -> bool {
    now >= starts_at && now < ends_at
}

/// Check if a monitor currently has an active maintenance window.
/// Used by the checker to suppress incident creation.
pub fn is_in_maintenance(db: &Db, monitor_id: &str) -> bool {
    let conn = db.conn.lock().unwrap();
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM maintenance_windows WHERE monitor_id = ?1 AND starts_at <= ?2 AND ends_at > ?2",
        params![monitor_id, now],
        |r| r.get(0),
    ).unwrap_or(0);
    count > 0
}
