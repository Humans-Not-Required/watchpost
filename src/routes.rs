use rocket::{get, post, patch, delete, serde::json::Json, State, http::Status};
use rocket::response::stream::{Event, EventStream};
use rocket::http::ContentType;
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
    let tags_str = tags_to_string(&data.tags);
    // Validate response_time_threshold_ms: must be >= 100ms if provided
    let rt_threshold = data.response_time_threshold_ms.map(|v| v.max(100));

    let conn = db.conn.lock().unwrap();
    conn.execute(
        "INSERT INTO monitors (id, name, url, method, interval_seconds, timeout_ms, expected_status, body_contains, headers, manage_key_hash, is_public, confirmation_threshold, tags, response_time_threshold_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
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
            tags_str,
            rt_threshold,
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

#[get("/monitors?<search>&<status>&<tag>")]
pub fn list_monitors(search: Option<&str>, status: Option<&str>, tag: Option<&str>, db: &State<Arc<Db>>) -> Result<Json<Vec<Monitor>>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();

    let mut sql = String::from(
        "SELECT id, name, url, method, interval_seconds, timeout_ms, expected_status, body_contains, headers, is_public, is_paused, current_status, last_checked_at, confirmation_threshold, created_at, updated_at, tags, response_time_threshold_ms
         FROM monitors WHERE is_public = 1"
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(q) = search {
        let q = q.trim();
        if !q.is_empty() {
            param_values.push(Box::new(format!("%{}%", q)));
            sql.push_str(&format!(" AND (name LIKE ?{n} OR url LIKE ?{n})", n = param_values.len()));
        }
    }
    if let Some(s) = status {
        let s = s.trim().to_lowercase();
        if !s.is_empty() && ["up", "down", "degraded", "unknown"].contains(&s.as_str()) {
            param_values.push(Box::new(s));
            sql.push_str(&format!(" AND current_status = ?{}", param_values.len()));
        }
    }
    if let Some(t) = tag {
        let t = t.trim().to_lowercase();
        if !t.is_empty() {
            // Match tag as comma-separated substring: exact match, starts with, ends with, or in middle
            param_values.push(Box::new(t.clone()));
            param_values.push(Box::new(format!("{},%", t)));
            param_values.push(Box::new(format!("%,{}", t)));
            param_values.push(Box::new(format!("%,{},%", t)));
            let n = param_values.len();
            sql.push_str(&format!(
                " AND (tags = ?{} OR tags LIKE ?{} OR tags LIKE ?{} OR tags LIKE ?{})",
                n - 3, n - 2, n - 1, n
            ));
        }
    }
    sql.push_str(" ORDER BY name");

    let mut stmt = conn.prepare(&sql)
        .map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;
    let params: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|v| v.as_ref()).collect();

    let monitors = stmt.query_map(params.as_slice(), |row| {
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

    if let Some(ref tags) = data.tags {
        updates.push(format!("tags = ?{}", values.len() + 1));
        values.push(Box::new(tags_to_string(tags)));
    }

    // response_time_threshold_ms: Some(Some(val)) = set, Some(None) = clear, None = no change
    if let Some(ref rt_opt) = data.response_time_threshold_ms {
        updates.push(format!("response_time_threshold_ms = ?{}", values.len() + 1));
        match rt_opt {
            Some(val) => values.push(Box::new(Some((*val).max(100)))),
            None => values.push(Box::new(None::<u32>)),
        }
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
        // Cursor mode: return rows with seq > after, ascending
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
        // Default: newest first (DESC), no cursor
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

// ── Incidents ──

#[get("/monitors/<id>/incidents?<limit>&<after>")]
pub fn get_incidents(
    id: &str,
    limit: Option<u32>,
    after: Option<i64>,
    db: &State<Arc<Db>>,
) -> Result<Json<Vec<Incident>>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    get_monitor_from_db(&conn, id)
        .map_err(|_| (Status::NotFound, Json(serde_json::json!({"error": "Monitor not found", "code": "NOT_FOUND"}))))?;

    let limit = limit.unwrap_or(20).min(100);
    let err_map = |e: rusqlite::Error| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()})));

    let row_to_inc = |row: &rusqlite::Row| -> rusqlite::Result<Incident> {
        Ok(Incident {
            id: row.get(0)?,
            monitor_id: row.get(1)?,
            started_at: row.get(2)?,
            resolved_at: row.get(3)?,
            cause: row.get(4)?,
            acknowledgement: row.get(5)?,
            acknowledged_by: row.get(6)?,
            acknowledged_at: row.get(7)?,
            seq: row.get(8)?,
        })
    };

    let incidents: Vec<Incident> = if let Some(after_seq) = after {
        let mut stmt = conn.prepare(
            "SELECT id, monitor_id, started_at, resolved_at, cause, acknowledgement, acknowledged_by, acknowledged_at, seq
             FROM incidents WHERE monitor_id = ?1 AND seq > ?2 ORDER BY seq ASC LIMIT ?3"
        ).map_err(err_map)?;
        let results: Vec<Incident> = stmt.query_map(params![id, after_seq, limit], row_to_inc)
            .map_err(err_map)?
            .filter_map(|r| r.ok())
            .collect();
        results
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, monitor_id, started_at, resolved_at, cause, acknowledgement, acknowledged_by, acknowledged_at, seq
             FROM incidents WHERE monitor_id = ?1 ORDER BY seq DESC LIMIT ?2"
        ).map_err(err_map)?;
        let results: Vec<Incident> = stmt.query_map(params![id, limit], row_to_inc)
            .map_err(err_map)?
            .filter_map(|r| r.ok())
            .collect();
        results
    };

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

#[get("/status?<search>&<status>&<tag>")]
pub fn status_page(search: Option<&str>, status: Option<&str>, tag: Option<&str>, db: &State<Arc<Db>>) -> Result<Json<StatusOverview>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();

    let mut sql = String::from("SELECT id, name, url, current_status, last_checked_at, tags FROM monitors WHERE is_public = 1");
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(q) = search {
        let q = q.trim();
        if !q.is_empty() {
            param_values.push(Box::new(format!("%{}%", q)));
            sql.push_str(&format!(" AND (name LIKE ?{n} OR url LIKE ?{n})", n = param_values.len()));
        }
    }
    if let Some(s) = status {
        let s = s.trim().to_lowercase();
        if !s.is_empty() && ["up", "down", "degraded", "unknown"].contains(&s.as_str()) {
            param_values.push(Box::new(s));
            sql.push_str(&format!(" AND current_status = ?{}", param_values.len()));
        }
    }
    if let Some(t) = tag {
        let t = t.trim().to_lowercase();
        if !t.is_empty() {
            param_values.push(Box::new(t.clone()));
            param_values.push(Box::new(format!("{},%", t)));
            param_values.push(Box::new(format!("%,{}", t)));
            param_values.push(Box::new(format!("%,{},%", t)));
            let n = param_values.len();
            sql.push_str(&format!(
                " AND (tags = ?{} OR tags LIKE ?{} OR tags LIKE ?{} OR tags LIKE ?{})",
                n - 3, n - 2, n - 1, n
            ));
        }
    }
    sql.push_str(" ORDER BY name");

    let mut stmt = conn.prepare(&sql)
        .map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;
    let params: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|v| v.as_ref()).collect();

    let monitors: Vec<StatusMonitor> = stmt.query_map(params.as_slice(), |row| {
        let id: String = row.get(0)?;
        let status: String = row.get(3)?;
        let tags_str: String = row.get::<_, String>(5).unwrap_or_default();
        Ok((id, row.get(1)?, row.get(2)?, status, row.get::<_, Option<String>>(4)?, tags_str))
    }).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?
    .filter_map(|r| r.ok())
    .map(|(id, name, url, status, last_checked, tags_str)| {
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
            tags: parse_tags(&tags_str),
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

// ── Tags ──

#[get("/tags")]
pub fn list_tags(db: &State<Arc<Db>>) -> Result<Json<Vec<String>>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT DISTINCT tags FROM monitors WHERE is_public = 1 AND tags != ''"
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    let mut all_tags: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let rows: Vec<String> = stmt.query_map([], |row| row.get(0))
        .map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?
        .filter_map(|r| r.ok())
        .collect();

    for tags_str in rows {
        for tag in parse_tags(&tags_str) {
            all_tags.insert(tag);
        }
    }

    Ok(Json(all_tags.into_iter().collect()))
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
up, down, degraded (response time exceeds threshold), unknown (never checked)

## Response Time Alerts
Set response_time_threshold_ms on a monitor to get degraded status when response time exceeds threshold.
Triggers monitor.degraded / monitor.recovered webhook events.
Set to null to disable. Minimum: 100ms.

## Notification Types
webhook (POST JSON to URL), email

## SSE Event Streams (real-time)
GET /api/v1/events — global event stream (all monitors)
GET /api/v1/monitors/:id/events — per-monitor event stream

Event types: check.completed, incident.created, incident.resolved

## Tags
POST /api/v1/monitors with "tags": ["api", "prod"] — tag monitors on creation
PATCH /api/v1/monitors/:id with "tags": ["api", "staging"] — update tags
GET /api/v1/tags — list all unique tags across public monitors
GET /api/v1/monitors?tag=prod — filter monitors by tag
GET /api/v1/status?tag=prod — filter status page by tag

## Search & Filter
GET /api/v1/monitors?search=keyword — filter by name/URL
GET /api/v1/monitors?status=up — filter by status (up/down/degraded/unknown)
GET /api/v1/status?search=keyword&status=down — combined filters

## Endpoints
POST /api/v1/monitors — create monitor
GET /api/v1/monitors — list public monitors (supports ?search= and ?status= filters)
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
PATCH /api/v1/notifications/:id — enable/disable notification (auth)
GET /api/v1/tags — list all unique tags (public monitors)
GET /api/v1/events — global SSE event stream
GET /api/v1/monitors/:id/events — per-monitor SSE event stream
GET /api/v1/status — public status page (supports ?tag= filter)
GET /api/v1/health — service health
"#)
}

// ── OpenAPI Spec ──

#[get("/openapi.json")]
pub fn openapi_spec() -> (rocket::http::ContentType, &'static str) {
    (rocket::http::ContentType::JSON, OPENAPI_JSON)
}

const OPENAPI_JSON: &str = r##"{
  "openapi": "3.0.3",
  "info": {
    "title": "Watchpost",
    "description": "Agent-native monitoring service. Create monitors, track uptime, receive alerts — all via REST API. No signup required.",
    "version": "0.1.0",
    "license": { "name": "MIT" }
  },
  "servers": [{ "url": "/api/v1" }],
  "paths": {
    "/health": {
      "get": {
        "summary": "Service health check",
        "operationId": "health",
        "tags": ["system"],
        "responses": {
          "200": { "description": "Service is healthy", "content": { "application/json": { "schema": { "type": "object", "properties": { "service": { "type": "string" }, "status": { "type": "string" }, "version": { "type": "string" } } } } } }
        }
      }
    },
    "/monitors": {
      "get": {
        "summary": "List public monitors",
        "operationId": "listMonitors",
        "tags": ["monitors"],
        "parameters": [
          { "name": "search", "in": "query", "schema": { "type": "string" }, "description": "Filter monitors by name or URL (case-insensitive substring match)" },
          { "name": "status", "in": "query", "schema": { "type": "string", "enum": ["up", "down", "degraded", "unknown"] }, "description": "Filter by current status" },
          { "name": "tag", "in": "query", "schema": { "type": "string" }, "description": "Filter monitors by tag" }
        ],
        "responses": {
          "200": { "description": "List of public monitors", "content": { "application/json": { "schema": { "type": "array", "items": { "$ref": "#/components/schemas/Monitor" } } } } }
        }
      },
      "post": {
        "summary": "Create a monitor",
        "operationId": "createMonitor",
        "tags": ["monitors"],
        "description": "Creates a new monitor and returns a manage_key (shown once). Save this key to manage the monitor later.",
        "requestBody": {
          "required": true,
          "content": { "application/json": { "schema": { "$ref": "#/components/schemas/CreateMonitor" } } }
        },
        "responses": {
          "200": { "description": "Monitor created", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/CreateMonitorResponse" } } } },
          "400": { "$ref": "#/components/responses/ValidationError" },
          "429": { "$ref": "#/components/responses/RateLimitError" }
        }
      }
    },
    "/monitors/{id}": {
      "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }],
      "get": {
        "summary": "Get monitor details",
        "operationId": "getMonitor",
        "tags": ["monitors"],
        "responses": {
          "200": { "description": "Monitor details", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Monitor" } } } },
          "404": { "$ref": "#/components/responses/NotFound" }
        }
      },
      "patch": {
        "summary": "Update monitor config",
        "operationId": "updateMonitor",
        "tags": ["monitors"],
        "security": [{ "manageKey": [] }],
        "requestBody": {
          "required": true,
          "content": { "application/json": { "schema": { "$ref": "#/components/schemas/UpdateMonitor" } } }
        },
        "responses": {
          "200": { "description": "Monitor updated" },
          "403": { "$ref": "#/components/responses/Forbidden" },
          "404": { "$ref": "#/components/responses/NotFound" }
        }
      },
      "delete": {
        "summary": "Delete monitor and all data",
        "operationId": "deleteMonitor",
        "tags": ["monitors"],
        "security": [{ "manageKey": [] }],
        "responses": {
          "200": { "description": "Monitor deleted" },
          "403": { "$ref": "#/components/responses/Forbidden" },
          "404": { "$ref": "#/components/responses/NotFound" }
        }
      }
    },
    "/monitors/{id}/pause": {
      "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }],
      "post": {
        "summary": "Pause monitor checks",
        "operationId": "pauseMonitor",
        "tags": ["monitors"],
        "security": [{ "manageKey": [] }],
        "responses": {
          "200": { "description": "Monitor paused" },
          "403": { "$ref": "#/components/responses/Forbidden" }
        }
      }
    },
    "/monitors/{id}/resume": {
      "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }],
      "post": {
        "summary": "Resume monitor checks",
        "operationId": "resumeMonitor",
        "tags": ["monitors"],
        "security": [{ "manageKey": [] }],
        "responses": {
          "200": { "description": "Monitor resumed" },
          "403": { "$ref": "#/components/responses/Forbidden" }
        }
      }
    },
    "/monitors/{id}/heartbeats": {
      "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }],
      "get": {
        "summary": "Get check history",
        "operationId": "getHeartbeats",
        "tags": ["heartbeats"],
        "parameters": [
          { "name": "limit", "in": "query", "schema": { "type": "integer", "default": 50, "maximum": 200 }, "description": "Max results to return" },
          { "name": "after", "in": "query", "schema": { "type": "integer" }, "description": "Return heartbeats with seq > this value (cursor-based pagination)" }
        ],
        "responses": {
          "200": { "description": "Check history (use last item's seq as after= for next page)", "content": { "application/json": { "schema": { "type": "array", "items": { "$ref": "#/components/schemas/Heartbeat" } } } } },
          "404": { "$ref": "#/components/responses/NotFound" }
        }
      }
    },
    "/monitors/{id}/uptime": {
      "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }],
      "get": {
        "summary": "Get uptime statistics",
        "operationId": "getUptime",
        "tags": ["heartbeats"],
        "responses": {
          "200": { "description": "Uptime stats", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/UptimeStats" } } } },
          "404": { "$ref": "#/components/responses/NotFound" }
        }
      }
    },
    "/monitors/{id}/incidents": {
      "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }],
      "get": {
        "summary": "Get incident history",
        "operationId": "getIncidents",
        "tags": ["incidents"],
        "parameters": [
          { "name": "limit", "in": "query", "schema": { "type": "integer", "default": 20, "maximum": 100 }, "description": "Max results to return" },
          { "name": "after", "in": "query", "schema": { "type": "integer" }, "description": "Return incidents with seq > this value (cursor-based pagination)" }
        ],
        "responses": {
          "200": { "description": "Incident list (use last item's seq as after= for next page)", "content": { "application/json": { "schema": { "type": "array", "items": { "$ref": "#/components/schemas/Incident" } } } } },
          "404": { "$ref": "#/components/responses/NotFound" }
        }
      }
    },
    "/incidents/{id}/acknowledge": {
      "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }],
      "post": {
        "summary": "Acknowledge an incident",
        "operationId": "acknowledgeIncident",
        "tags": ["incidents"],
        "security": [{ "manageKey": [] }],
        "requestBody": {
          "required": true,
          "content": { "application/json": { "schema": { "$ref": "#/components/schemas/AcknowledgeIncident" } } }
        },
        "responses": {
          "200": { "description": "Incident acknowledged" },
          "403": { "$ref": "#/components/responses/Forbidden" },
          "404": { "$ref": "#/components/responses/NotFound" }
        }
      }
    },
    "/monitors/{id}/notifications": {
      "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }],
      "post": {
        "summary": "Add notification channel",
        "operationId": "createNotification",
        "tags": ["notifications"],
        "security": [{ "manageKey": [] }],
        "requestBody": {
          "required": true,
          "content": { "application/json": { "schema": { "$ref": "#/components/schemas/CreateNotification" } } }
        },
        "responses": {
          "200": { "description": "Notification channel created", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/NotificationChannel" } } } },
          "403": { "$ref": "#/components/responses/Forbidden" }
        }
      },
      "get": {
        "summary": "List notification channels",
        "operationId": "listNotifications",
        "tags": ["notifications"],
        "security": [{ "manageKey": [] }],
        "responses": {
          "200": { "description": "Notification channels", "content": { "application/json": { "schema": { "type": "array", "items": { "$ref": "#/components/schemas/NotificationChannel" } } } } },
          "403": { "$ref": "#/components/responses/Forbidden" }
        }
      }
    },
    "/notifications/{id}": {
      "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }],
      "delete": {
        "summary": "Remove notification channel",
        "operationId": "deleteNotification",
        "tags": ["notifications"],
        "security": [{ "manageKey": [] }],
        "responses": {
          "200": { "description": "Notification deleted" },
          "403": { "$ref": "#/components/responses/Forbidden" },
          "404": { "$ref": "#/components/responses/NotFound" }
        }
      },
      "patch": {
        "summary": "Enable or disable a notification channel",
        "operationId": "updateNotification",
        "tags": ["notifications"],
        "security": [{ "manageKey": [] }],
        "requestBody": {
          "required": true,
          "content": {
            "application/json": {
              "schema": {
                "type": "object",
                "properties": {
                  "is_enabled": { "type": "boolean" },
                  "name": { "type": "string" }
                }
              }
            }
          }
        },
        "responses": {
          "200": { "description": "Notification channel updated" },
          "403": { "$ref": "#/components/responses/Forbidden" },
          "404": { "$ref": "#/components/responses/NotFound" }
        }
      }
    },
    "/status": {
      "get": {
        "summary": "Public status page",
        "operationId": "statusPage",
        "tags": ["status"],
        "parameters": [
          { "name": "search", "in": "query", "schema": { "type": "string" }, "description": "Filter monitors by name or URL (case-insensitive substring match)" },
          { "name": "status", "in": "query", "schema": { "type": "string", "enum": ["up", "down", "degraded", "unknown"] }, "description": "Filter by current status" },
          { "name": "tag", "in": "query", "schema": { "type": "string" }, "description": "Filter monitors by tag" }
        ],
        "responses": {
          "200": { "description": "Status overview", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/StatusOverview" } } } }
        }
      }
    },
    "/tags": {
      "get": {
        "summary": "List all unique tags",
        "operationId": "listTags",
        "tags": ["monitors"],
        "description": "Returns sorted list of unique tags across all public monitors.",
        "responses": {
          "200": { "description": "List of tags", "content": { "application/json": { "schema": { "type": "array", "items": { "type": "string" } } } } }
        }
      }
    },
    "/events": {
      "get": {
        "summary": "Global SSE event stream",
        "operationId": "globalEvents",
        "tags": ["events"],
        "responses": {
          "200": { "description": "SSE stream", "content": { "text/event-stream": {} } }
        }
      }
    },
    "/monitors/{id}/events": {
      "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }],
      "get": {
        "summary": "Per-monitor SSE event stream",
        "operationId": "monitorEvents",
        "tags": ["events"],
        "responses": {
          "200": { "description": "SSE stream", "content": { "text/event-stream": {} } }
        }
      }
    },
    "/llms.txt": {
      "get": {
        "summary": "AI agent discovery document",
        "operationId": "llmsTxt",
        "tags": ["system"],
        "responses": {
          "200": { "description": "Agent-readable API summary", "content": { "text/plain": {} } }
        }
      }
    },
    "/openapi.json": {
      "get": {
        "summary": "OpenAPI specification",
        "operationId": "openapiSpec",
        "tags": ["system"],
        "responses": {
          "200": { "description": "This document", "content": { "application/json": {} } }
        }
      }
    }
  },
  "components": {
    "securitySchemes": {
      "manageKey": {
        "type": "http",
        "scheme": "bearer",
        "description": "Monitor manage_key. Also accepted as X-API-Key header or ?key= query param."
      }
    },
    "schemas": {
      "Monitor": {
        "type": "object",
        "properties": {
          "id": { "type": "string", "format": "uuid" },
          "name": { "type": "string" },
          "url": { "type": "string" },
          "method": { "type": "string", "enum": ["GET", "HEAD", "POST"] },
          "interval_seconds": { "type": "integer", "minimum": 30 },
          "timeout_ms": { "type": "integer" },
          "expected_status": { "type": "integer" },
          "body_contains": { "type": "string", "nullable": true },
          "headers": { "type": "object", "nullable": true },
          "is_public": { "type": "boolean" },
          "is_paused": { "type": "boolean" },
          "current_status": { "type": "string", "enum": ["unknown", "up", "down", "degraded"] },
          "last_checked_at": { "type": "string", "nullable": true },
          "confirmation_threshold": { "type": "integer" },
          "response_time_threshold_ms": { "type": "integer", "nullable": true, "description": "Mark as degraded when response time exceeds this (ms). Null = disabled." },
          "tags": { "type": "array", "items": { "type": "string" }, "description": "Freeform tags for grouping monitors" },
          "created_at": { "type": "string" },
          "updated_at": { "type": "string" }
        }
      },
      "CreateMonitor": {
        "type": "object",
        "required": ["name", "url"],
        "properties": {
          "name": { "type": "string" },
          "url": { "type": "string" },
          "method": { "type": "string", "enum": ["GET", "HEAD", "POST"], "default": "GET" },
          "interval_seconds": { "type": "integer", "minimum": 30, "default": 300 },
          "timeout_ms": { "type": "integer", "default": 10000 },
          "expected_status": { "type": "integer", "default": 200 },
          "body_contains": { "type": "string" },
          "headers": { "type": "object" },
          "is_public": { "type": "boolean", "default": false },
          "confirmation_threshold": { "type": "integer", "minimum": 1, "maximum": 10, "default": 2 },
          "response_time_threshold_ms": { "type": "integer", "minimum": 100, "nullable": true, "description": "Alert when response time exceeds this threshold (ms). Null = disabled." },
          "tags": { "type": "array", "items": { "type": "string" }, "description": "Freeform tags for grouping" }
        }
      },
      "UpdateMonitor": {
        "type": "object",
        "properties": {
          "name": { "type": "string" },
          "url": { "type": "string" },
          "method": { "type": "string" },
          "interval_seconds": { "type": "integer" },
          "timeout_ms": { "type": "integer" },
          "expected_status": { "type": "integer" },
          "body_contains": { "type": "string" },
          "headers": { "type": "object" },
          "is_public": { "type": "boolean" },
          "confirmation_threshold": { "type": "integer" },
          "response_time_threshold_ms": { "type": "integer", "minimum": 100, "nullable": true, "description": "Set to threshold value, or null to disable." },
          "tags": { "type": "array", "items": { "type": "string" } }
        }
      },
      "CreateMonitorResponse": {
        "type": "object",
        "properties": {
          "monitor": { "$ref": "#/components/schemas/Monitor" },
          "manage_key": { "type": "string", "description": "Save this! Only shown once." },
          "manage_url": { "type": "string" },
          "view_url": { "type": "string" },
          "api_base": { "type": "string" }
        }
      },
      "Heartbeat": {
        "type": "object",
        "properties": {
          "id": { "type": "string", "format": "uuid" },
          "monitor_id": { "type": "string", "format": "uuid" },
          "status": { "type": "string", "enum": ["up", "down", "degraded"] },
          "response_time_ms": { "type": "integer" },
          "status_code": { "type": "integer", "nullable": true },
          "error_message": { "type": "string", "nullable": true },
          "checked_at": { "type": "string" },
          "seq": { "type": "integer", "description": "Monotonic sequence number for cursor-based pagination" }
        }
      },
      "Incident": {
        "type": "object",
        "properties": {
          "id": { "type": "string", "format": "uuid" },
          "monitor_id": { "type": "string", "format": "uuid" },
          "started_at": { "type": "string" },
          "resolved_at": { "type": "string", "nullable": true },
          "cause": { "type": "string" },
          "acknowledgement": { "type": "string", "nullable": true },
          "acknowledged_by": { "type": "string", "nullable": true },
          "acknowledged_at": { "type": "string", "nullable": true },
          "seq": { "type": "integer", "description": "Monotonic sequence number for cursor-based pagination" }
        }
      },
      "AcknowledgeIncident": {
        "type": "object",
        "required": ["note"],
        "properties": {
          "note": { "type": "string" },
          "actor": { "type": "string", "default": "anonymous" }
        }
      },
      "UptimeStats": {
        "type": "object",
        "properties": {
          "monitor_id": { "type": "string" },
          "uptime_24h": { "type": "number" },
          "uptime_7d": { "type": "number" },
          "uptime_30d": { "type": "number" },
          "uptime_90d": { "type": "number" },
          "total_checks_24h": { "type": "integer" },
          "total_checks_7d": { "type": "integer" },
          "total_checks_30d": { "type": "integer" },
          "total_checks_90d": { "type": "integer" },
          "avg_response_ms_24h": { "type": "number", "nullable": true }
        }
      },
      "StatusOverview": {
        "type": "object",
        "properties": {
          "monitors": { "type": "array", "items": { "$ref": "#/components/schemas/StatusMonitor" } },
          "overall": { "type": "string", "enum": ["operational", "degraded", "major_outage", "unknown"] }
        }
      },
      "StatusMonitor": {
        "type": "object",
        "properties": {
          "id": { "type": "string" },
          "name": { "type": "string" },
          "url": { "type": "string" },
          "current_status": { "type": "string" },
          "last_checked_at": { "type": "string", "nullable": true },
          "uptime_24h": { "type": "number" },
          "uptime_7d": { "type": "number" },
          "avg_response_ms_24h": { "type": "number", "nullable": true },
          "active_incident": { "type": "boolean" },
          "tags": { "type": "array", "items": { "type": "string" } }
        }
      },
      "NotificationChannel": {
        "type": "object",
        "properties": {
          "id": { "type": "string", "format": "uuid" },
          "monitor_id": { "type": "string", "format": "uuid" },
          "name": { "type": "string" },
          "channel_type": { "type": "string", "enum": ["webhook", "email"] },
          "config": { "type": "object" },
          "is_enabled": { "type": "boolean" },
          "created_at": { "type": "string" }
        }
      },
      "CreateNotification": {
        "type": "object",
        "required": ["name", "channel_type", "config"],
        "properties": {
          "name": { "type": "string" },
          "channel_type": { "type": "string", "enum": ["webhook", "email"] },
          "config": { "type": "object", "description": "For webhook: {\"url\": \"...\"}, for email: {\"address\": \"...\"}" }
        }
      },
      "Error": {
        "type": "object",
        "properties": {
          "error": { "type": "string" },
          "code": { "type": "string" }
        }
      }
    },
    "responses": {
      "NotFound": {
        "description": "Resource not found",
        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } }
      },
      "Forbidden": {
        "description": "Invalid manage key",
        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } }
      },
      "ValidationError": {
        "description": "Validation error",
        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } }
      },
      "RateLimitError": {
        "description": "Rate limit exceeded",
        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } }
      }
    }
  }
}"##;

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
        "SELECT id, name, url, method, interval_seconds, timeout_ms, expected_status, body_contains, headers, is_public, is_paused, current_status, last_checked_at, confirmation_threshold, created_at, updated_at, tags, response_time_threshold_ms
         FROM monitors WHERE id = ?1",
        params![id],
        |row| Ok(row_to_monitor(row)),
    )
}

fn parse_tags(raw: &str) -> Vec<String> {
    if raw.is_empty() {
        Vec::new()
    } else {
        raw.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    }
}

fn tags_to_string(tags: &[String]) -> String {
    tags.iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(",")
}

fn row_to_monitor(row: &rusqlite::Row) -> Monitor {
    let headers_str: Option<String> = row.get(8).unwrap_or(None);
    let tags_str: String = row.get(16).unwrap_or_default();
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
        response_time_threshold_ms: row.get::<_, Option<u32>>(17).unwrap_or(None),
        tags: parse_tags(&tags_str),
        created_at: row.get(14).unwrap(),
        updated_at: row.get(15).unwrap(),
    }
}

// ── SPA Fallback ──

#[get("/<_path..>", rank = 100)]
pub fn spa_fallback(_path: std::path::PathBuf) -> Option<(ContentType, Vec<u8>)> {
    let static_dir: std::path::PathBuf = std::env::var("STATIC_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("../frontend/dist"));
    let index_path = static_dir.join("index.html");
    std::fs::read(&index_path)
        .ok()
        .map(|bytes| (ContentType::HTML, bytes))
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
