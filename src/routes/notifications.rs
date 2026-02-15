use rocket::{get, post, patch, delete, serde::json::Json, State, http::Status};
use crate::db::Db;
use crate::models::{NotificationChannel, CreateNotification};
use crate::auth::ManageToken;
use super::verify_manage_key;
use rusqlite::params;
use std::sync::Arc;

// ── Notification Channels ──

#[post("/monitors/<id>/notifications", format = "json", data = "<input>")]
pub fn create_notification(
    id: &str,
    input: Json<CreateNotification>,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<NotificationChannel>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    verify_manage_key(&conn, id, &token.0)?;

    let data = input.into_inner();
    if !["webhook", "email"].contains(&data.channel_type.as_str()) {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "channel_type must be 'webhook' or 'email'", "code": "VALIDATION_ERROR"
        }))));
    }

    let nid = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO notification_channels (id, monitor_id, name, channel_type, config) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![nid, id, data.name, data.channel_type, data.config.to_string()],
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    Ok(Json(NotificationChannel {
        id: nid,
        monitor_id: id.to_string(),
        name: data.name,
        channel_type: data.channel_type,
        config: data.config,
        is_enabled: true,
        created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    }))
}

#[get("/monitors/<id>/notifications")]
pub fn list_notifications(
    id: &str,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<Vec<NotificationChannel>>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    verify_manage_key(&conn, id, &token.0)?;

    let mut stmt = conn.prepare(
        "SELECT id, monitor_id, name, channel_type, config, is_enabled, created_at FROM notification_channels WHERE monitor_id = ?1"
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    let channels = stmt.query_map(params![id], |row| {
        let config_str: String = row.get(4)?;
        Ok(NotificationChannel {
            id: row.get(0)?,
            monitor_id: row.get(1)?,
            name: row.get(2)?,
            channel_type: row.get(3)?,
            config: serde_json::from_str(&config_str).unwrap_or(serde_json::Value::Null),
            is_enabled: row.get(5)?,
            created_at: row.get(6)?,
        })
    }).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?
    .filter_map(|r| r.ok())
    .collect();

    Ok(Json(channels))
}

#[delete("/notifications/<id>")]
pub fn delete_notification(
    id: &str,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<serde_json::Value>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();

    let monitor_id: String = conn.query_row(
        "SELECT monitor_id FROM notification_channels WHERE id = ?1",
        params![id],
        |row| row.get(0),
    ).map_err(|_| (Status::NotFound, Json(serde_json::json!({"error": "Notification not found", "code": "NOT_FOUND"}))))?;

    verify_manage_key(&conn, &monitor_id, &token.0)?;

    conn.execute("DELETE FROM notification_channels WHERE id = ?1", params![id])
        .map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    Ok(Json(serde_json::json!({"message": "Notification channel deleted"})))
}

#[derive(serde::Deserialize)]
pub struct UpdateNotification {
    pub is_enabled: Option<bool>,
    pub name: Option<String>,
}

#[patch("/notifications/<id>", format = "json", data = "<input>")]
pub fn update_notification(
    id: &str,
    input: Json<UpdateNotification>,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<serde_json::Value>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();

    let monitor_id: String = conn.query_row(
        "SELECT monitor_id FROM notification_channels WHERE id = ?1",
        params![id],
        |row| row.get(0),
    ).map_err(|_| (Status::NotFound, Json(serde_json::json!({"error": "Notification not found", "code": "NOT_FOUND"}))))?;

    verify_manage_key(&conn, &monitor_id, &token.0)?;

    let data = input.into_inner();
    if let Some(enabled) = data.is_enabled {
        conn.execute(
            "UPDATE notification_channels SET is_enabled = ?1 WHERE id = ?2",
            params![enabled, id],
        ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;
    }
    if let Some(name) = &data.name {
        conn.execute(
            "UPDATE notification_channels SET name = ?1 WHERE id = ?2",
            params![name, id],
        ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;
    }

    Ok(Json(serde_json::json!({"message": "Notification channel updated"})))
}
