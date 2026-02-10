use rocket::{get, post, patch, delete, serde::json::Json, State, http::Status};
use rocket::response::stream::{Event, EventStream};
use crate::db::Db;
use crate::models::*;
use crate::auth::{ManageToken, ClientIp, hash_key, generate_key};
use crate::sse::EventBroadcaster;
use rusqlite::params;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

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

// ── Health ──

#[get("/health")]
pub fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "service": "watchpost",
        "status": "ok",
        "version": "0.1.0"
    }))
}

// ── Create Monitor ──

#[post("/monitors", format = "json", data = "<input>")]
pub fn create_monitor(
    input: Json<CreateMonitor>,
    db: &State<Arc<Db>>,
    rate_limiter: &State<RateLimiter>,
    client_ip: ClientIp,
) -> Result<Json<CreateMonitorResponse>, (Status, Json<serde_json::Value>)> {
    if !rate_limiter.check(&client_ip.0) {
        return Err((Status::TooManyRequests, Json(serde_json::json!({
            "error": "Rate limit exceeded",
            "code": "RATE_LIMIT_EXCEEDED"
        }))));
    }

    let data = input.into_inner();

    // Validate
    if data.name.trim().is_empty() {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "Name is required", "code": "VALIDATION_ERROR"
        }))));
    }
    if data.url.trim().is_empty() {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "URL is required", "code": "VALIDATION_ERROR"
        }))));
    }
    let method = data.method.to_uppercase();
    if !["GET", "HEAD", "POST"].contains(&method.as_str()) {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "Method must be GET, HEAD, or POST", "code": "VALIDATION_ERROR"
        }))));
    }
    let interval = data.interval_seconds.unwrap_or(300).max(30);
    let timeout = data.timeout_ms.unwrap_or(10000).max(1000).min(60000);
    let expected_status = data.expected_status.unwrap_or(200);
    let confirmation = data.confirmation_threshold.unwrap_or(2).max(1).min(10);

    let id = uuid::Uuid::new_v4().to_string();
    let manage_key = generate_key();
    let key_hash = hash_key(&manage_key);

    let conn = db.conn.lock().unwrap();
    conn.execute(
        "INSERT INTO monitors (id, name, url, method, interval_seconds, timeout_ms, expected_status, body_contains, headers, manage_key_hash, is_public, confirmation_threshold)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            id,
            data.name.trim(),
            data.url.trim(),
            method,
            interval,
            timeout,
            expected_status,
            data.body_contains,
            data.headers.map(|h| h.to_string()),
            key_hash,
            data.is_public as i32,
            confirmation,
        ],
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({
        "error": format!("DB error: {}", e), "code": "INTERNAL_ERROR"
    }))))?;

    let monitor = get_monitor_from_db(&conn, &id).map_err(|e| {
        (Status::InternalServerError, Json(serde_json::json!({
            "error": format!("DB error: {}", e), "code": "INTERNAL_ERROR"
        })))
    })?;

    Ok(Json(CreateMonitorResponse {
        monitor,
        manage_key: manage_key.clone(),
        manage_url: format!("/monitor/{}?key={}", id, manage_key),
        view_url: format!("/monitor/{}", id),
        api_base: format!("/api/v1/monitors/{}", id),
    }))
}

// ── List Monitors (public only) ──

#[get("/monitors")]
pub fn list_monitors(db: &State<Arc<Db>>) -> Result<Json<Vec<Monitor>>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, name, url, method, interval_seconds, timeout_ms, expected_status, body_contains, headers, is_public, is_paused, current_status, last_checked_at, confirmation_threshold, created_at, updated_at
         FROM monitors WHERE is_public = 1 ORDER BY name"
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    let monitors = stmt.query_map([], |row| {
        Ok(row_to_monitor(row))
    }).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?
    .filter_map(|r| r.ok())
    .collect();

    Ok(Json(monitors))
}

// ── Get Monitor ──

#[get("/monitors/<id>")]
pub fn get_monitor(id: &str, db: &State<Arc<Db>>) -> Result<Json<Monitor>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    let monitor = get_monitor_from_db(&conn, id)
        .map_err(|_| (Status::NotFound, Json(serde_json::json!({
            "error": "Monitor not found", "code": "NOT_FOUND"
        }))))?;
    Ok(Json(monitor))
}

// ── Update Monitor ──

#[patch("/monitors/<id>", format = "json", data = "<input>")]
pub fn update_monitor(
    id: &str,
    input: Json<UpdateMonitor>,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<serde_json::Value>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    verify_manage_key(&conn, id, &token.0)?;

    let data = input.into_inner();
    let mut updates = Vec::new();
    let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    macro_rules! add_update {
        ($field:ident, $col:expr) => {
            if let Some(val) = data.$field {
                updates.push(format!("{} = ?{}", $col, values.len() + 1));
                values.push(Box::new(val));
            }
        };
    }

    add_update!(name, "name");
    add_update!(url, "url");
    add_update!(method, "method");
    add_update!(interval_seconds, "interval_seconds");
    add_update!(timeout_ms, "timeout_ms");
    add_update!(expected_status, "expected_status");
    add_update!(body_contains, "body_contains");
    add_update!(is_public, "is_public");
    add_update!(confirmation_threshold, "confirmation_threshold");

    if let Some(ref headers) = data.headers {
        updates.push(format!("headers = ?{}", values.len() + 1));
        values.push(Box::new(headers.to_string()));
    }

    if updates.is_empty() {
        return Ok(Json(serde_json::json!({"message": "No changes"})));
    }

    updates.push(format!("updated_at = datetime('now')"));
    let sql = format!("UPDATE monitors SET {} WHERE id = ?{}", updates.join(", "), values.len() + 1);
    values.push(Box::new(id.to_string()));

    let params: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();
    conn.execute(&sql, params.as_slice())
        .map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    Ok(Json(serde_json::json!({"message": "Monitor updated"})))
}

// ── Delete Monitor ──

#[delete("/monitors/<id>")]
pub fn delete_monitor(
    id: &str,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<serde_json::Value>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    verify_manage_key(&conn, id, &token.0)?;

    conn.execute("DELETE FROM monitors WHERE id = ?1", params![id])
        .map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    Ok(Json(serde_json::json!({"message": "Monitor deleted"})))
}

// ── Pause / Resume ──

#[post("/monitors/<id>/pause")]
pub fn pause_monitor(
    id: &str,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<serde_json::Value>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    verify_manage_key(&conn, id, &token.0)?;
    conn.execute("UPDATE monitors SET is_paused = 1, updated_at = datetime('now') WHERE id = ?1", params![id])
        .map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;
    Ok(Json(serde_json::json!({"message": "Monitor paused"})))
}

#[post("/monitors/<id>/resume")]
pub fn resume_monitor(
    id: &str,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<serde_json::Value>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    verify_manage_key(&conn, id, &token.0)?;
    conn.execute("UPDATE monitors SET is_paused = 0, updated_at = datetime('now') WHERE id = ?1", params![id])
        .map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;
    Ok(Json(serde_json::json!({"message": "Monitor resumed"})))
}

// ── Heartbeats ──

#[get("/monitors/<id>/heartbeats?<limit>&<offset>")]
pub fn get_heartbeats(
    id: &str,
    limit: Option<u32>,
    offset: Option<u32>,
    db: &State<Arc<Db>>,
) -> Result<Json<Vec<Heartbeat>>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    // Verify monitor exists
    get_monitor_from_db(&conn, id)
        .map_err(|_| (Status::NotFound, Json(serde_json::json!({"error": "Monitor not found", "code": "NOT_FOUND"}))))?;

    let limit = limit.unwrap_or(50).min(200);
    let offset = offset.unwrap_or(0);

    let mut stmt = conn.prepare(
        "SELECT id, monitor_id, status, response_time_ms, status_code, error_message, checked_at
         FROM heartbeats WHERE monitor_id = ?1 ORDER BY checked_at DESC LIMIT ?2 OFFSET ?3"
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    let heartbeats = stmt.query_map(params![id, limit, offset], |row| {
        Ok(Heartbeat {
            id: row.get(0)?,
            monitor_id: row.get(1)?,
            status: row.get(2)?,
            response_time_ms: row.get(3)?,
            status_code: row.get(4)?,
            error_message: row.get(5)?,
            checked_at: row.get(6)?,
        })
    }).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?
    .filter_map(|r| r.ok())
    .collect();

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

// ── Incidents ──

#[get("/monitors/<id>/incidents?<limit>&<offset>")]
pub fn get_incidents(
    id: &str,
    limit: Option<u32>,
    offset: Option<u32>,
    db: &State<Arc<Db>>,
) -> Result<Json<Vec<Incident>>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    get_monitor_from_db(&conn, id)
        .map_err(|_| (Status::NotFound, Json(serde_json::json!({"error": "Monitor not found", "code": "NOT_FOUND"}))))?;

    let limit = limit.unwrap_or(20).min(100);
    let offset = offset.unwrap_or(0);

    let mut stmt = conn.prepare(
        "SELECT id, monitor_id, started_at, resolved_at, cause, acknowledgement, acknowledged_by, acknowledged_at
         FROM incidents WHERE monitor_id = ?1 ORDER BY started_at DESC LIMIT ?2 OFFSET ?3"
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    let incidents = stmt.query_map(params![id, limit, offset], |row| {
        Ok(Incident {
            id: row.get(0)?,
            monitor_id: row.get(1)?,
            started_at: row.get(2)?,
            resolved_at: row.get(3)?,
            cause: row.get(4)?,
            acknowledgement: row.get(5)?,
            acknowledged_by: row.get(6)?,
            acknowledged_at: row.get(7)?,
        })
    }).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?
    .filter_map(|r| r.ok())
    .collect();

    Ok(Json(incidents))
}

// ── Acknowledge Incident ──

#[post("/incidents/<id>/acknowledge", format = "json", data = "<input>")]
pub fn acknowledge_incident(
    id: &str,
    input: Json<AcknowledgeIncident>,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<serde_json::Value>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();

    // Get incident's monitor_id
    let monitor_id: String = conn.query_row(
        "SELECT monitor_id FROM incidents WHERE id = ?1",
        params![id],
        |row| row.get(0),
    ).map_err(|_| (Status::NotFound, Json(serde_json::json!({"error": "Incident not found", "code": "NOT_FOUND"}))))?;

    verify_manage_key(&conn, &monitor_id, &token.0)?;

    let data = input.into_inner();
    conn.execute(
        "UPDATE incidents SET acknowledgement = ?1, acknowledged_by = ?2, acknowledged_at = datetime('now') WHERE id = ?3",
        params![data.note, data.actor, id],
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    Ok(Json(serde_json::json!({"message": "Incident acknowledged"})))
}

// ── Status Page ──

#[get("/status")]
pub fn status_page(db: &State<Arc<Db>>) -> Result<Json<StatusOverview>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, name, url, current_status, last_checked_at FROM monitors WHERE is_public = 1 ORDER BY name"
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    let monitors: Vec<StatusMonitor> = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let status: String = row.get(3)?;
        Ok((id, row.get(1)?, row.get(2)?, status, row.get::<_, Option<String>>(4)?))
    }).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?
    .filter_map(|r| r.ok())
    .map(|(id, name, url, status, last_checked)| {
        // Calculate uptime (simplified inline)
        let total_24h: u32 = conn.query_row(
            "SELECT COUNT(*) FROM heartbeats WHERE monitor_id = ?1 AND checked_at > datetime('now', '-24 hours')",
            params![&id], |row| row.get(0),
        ).unwrap_or(0);
        let up_24h: u32 = conn.query_row(
            "SELECT COUNT(*) FROM heartbeats WHERE monitor_id = ?1 AND status = 'up' AND checked_at > datetime('now', '-24 hours')",
            params![&id], |row| row.get(0),
        ).unwrap_or(0);
        let total_7d: u32 = conn.query_row(
            "SELECT COUNT(*) FROM heartbeats WHERE monitor_id = ?1 AND checked_at > datetime('now', '-7 days')",
            params![&id], |row| row.get(0),
        ).unwrap_or(0);
        let up_7d: u32 = conn.query_row(
            "SELECT COUNT(*) FROM heartbeats WHERE monitor_id = ?1 AND status = 'up' AND checked_at > datetime('now', '-7 days')",
            params![&id], |row| row.get(0),
        ).unwrap_or(0);
        let avg_ms: Option<f64> = conn.query_row(
            "SELECT AVG(response_time_ms) FROM heartbeats WHERE monitor_id = ?1 AND status = 'up' AND checked_at > datetime('now', '-24 hours')",
            params![&id], |row| row.get(0),
        ).ok();
        let active_incident = conn.query_row(
            "SELECT COUNT(*) FROM incidents WHERE monitor_id = ?1 AND resolved_at IS NULL",
            params![&id], |row| row.get::<_, u32>(0),
        ).unwrap_or(0) > 0;

        StatusMonitor {
            id,
            name,
            url,
            current_status: status,
            last_checked_at: last_checked,
            uptime_24h: if total_24h > 0 { (up_24h as f64 / total_24h as f64) * 100.0 } else { 100.0 },
            uptime_7d: if total_7d > 0 { (up_7d as f64 / total_7d as f64) * 100.0 } else { 100.0 },
            avg_response_ms_24h: avg_ms,
            active_incident,
        }
    })
    .collect();

    let overall = if monitors.is_empty() {
        "unknown".to_string()
    } else if monitors.iter().any(|m| m.current_status == "down") {
        "major_outage".to_string()
    } else if monitors.iter().all(|m| m.current_status == "up") {
        "operational".to_string()
    } else if monitors.iter().any(|m| m.current_status == "unknown") {
        // If anything hasn't been checked yet, the overall status is unknown.
        "unknown".to_string()
    } else {
        // Remaining case: some degraded, none down.
        "degraded".to_string()
    };

    Ok(Json(StatusOverview { monitors, overall }))
}

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

    // Get notification's monitor_id
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

// ── llms.txt ──

#[get("/llms.txt")]
pub fn llms_txt() -> (rocket::http::ContentType, &'static str) {
    (rocket::http::ContentType::Plain, r#"# Watchpost — Agent-Native Monitoring Service
# API Base: /api/v1

## Quick Start
POST /api/v1/monitors — Create a monitor (returns manage_key)
GET /api/v1/monitors/:id — View monitor status
GET /api/v1/monitors/:id/heartbeats — Check history
GET /api/v1/monitors/:id/uptime — Uptime stats (24h/7d/30d/90d)
GET /api/v1/monitors/:id/incidents — Incident history
GET /api/v1/status — Public status page

## Auth
- Create monitor: no auth (returns manage_key, save it!)
- Read: no auth (use monitor UUID)
- Write: manage_key via Bearer header, X-API-Key, or ?key= param

## Monitor Methods
GET, HEAD, POST

## Check Statuses
up, down, degraded (>5s response), unknown (never checked)

## Notification Types
webhook (POST JSON to URL), email

## SSE Event Streams (real-time)
GET /api/v1/events — global event stream (all monitors)
GET /api/v1/monitors/:id/events — per-monitor event stream

Event types: check.completed, incident.created, incident.resolved

## Endpoints
POST /api/v1/monitors — create monitor
GET /api/v1/monitors — list public monitors
GET /api/v1/monitors/:id — get monitor
PATCH /api/v1/monitors/:id — update (auth)
DELETE /api/v1/monitors/:id — delete (auth)
POST /api/v1/monitors/:id/pause — pause checks (auth)
POST /api/v1/monitors/:id/resume — resume checks (auth)
GET /api/v1/monitors/:id/heartbeats — check history
GET /api/v1/monitors/:id/uptime — uptime stats
GET /api/v1/monitors/:id/incidents — incidents
POST /api/v1/incidents/:id/acknowledge — ack incident (auth)
POST /api/v1/monitors/:id/notifications — add notification (auth)
GET /api/v1/monitors/:id/notifications — list notifications (auth)
DELETE /api/v1/notifications/:id — remove notification (auth)
GET /api/v1/events — global SSE event stream
GET /api/v1/monitors/:id/events — per-monitor SSE event stream
GET /api/v1/status — public status page
GET /api/v1/health — service health
"#)
}

// ── SSE Event Streams ──

#[get("/events")]
pub fn global_events(broadcaster: &State<Arc<EventBroadcaster>>) -> EventStream![Event + '_] {
    crate::sse::global_stream(broadcaster)
}

#[get("/monitors/<id>/events")]
pub fn monitor_events<'a>(id: &'a str, broadcaster: &'a State<Arc<EventBroadcaster>>) -> EventStream![Event + 'a] {
    crate::sse::monitor_stream(broadcaster, id.to_string())
}

// ── Helpers ──

fn get_monitor_from_db(conn: &rusqlite::Connection, id: &str) -> rusqlite::Result<Monitor> {
    conn.query_row(
        "SELECT id, name, url, method, interval_seconds, timeout_ms, expected_status, body_contains, headers, is_public, is_paused, current_status, last_checked_at, confirmation_threshold, created_at, updated_at
         FROM monitors WHERE id = ?1",
        params![id],
        |row| Ok(row_to_monitor(row)),
    )
}

fn row_to_monitor(row: &rusqlite::Row) -> Monitor {
    let headers_str: Option<String> = row.get(8).unwrap_or(None);
    Monitor {
        id: row.get(0).unwrap(),
        name: row.get(1).unwrap(),
        url: row.get(2).unwrap(),
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
        created_at: row.get(14).unwrap(),
        updated_at: row.get(15).unwrap(),
    }
}

fn verify_manage_key(conn: &rusqlite::Connection, monitor_id: &str, token: &str) -> Result<(), (Status, Json<serde_json::Value>)> {
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
