use rocket::{get, post, patch, delete, serde::json::Json, State, http::Status};
use rocket::response::stream::{Event, EventStream};
use rocket::http::ContentType;
use crate::db::Db;
use crate::models::{
    Monitor, CreateMonitor, UpdateMonitor, CreateMonitorResponse,
    Heartbeat, Incident, AcknowledgeIncident, UptimeStats,
    StatusOverview, StatusMonitor, NotificationChannel, CreateNotification,
    BulkCreateMonitors, BulkCreateResponse, BulkError, ExportedMonitor,
    DashboardOverview, StatusCounts, DashboardIncident, SlowMonitor,
    UptimeHistoryDay,
};
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
    let url_trimmed = data.url.trim().to_lowercase();
    if !url_trimmed.starts_with("http://") && !url_trimmed.starts_with("https://") {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "URL must start with http:// or https://", "code": "VALIDATION_ERROR"
        }))));
    }
    if let Some(ref headers) = data.headers {
        if !headers.is_object() {
            return Err((Status::BadRequest, Json(serde_json::json!({
                "error": "Headers must be a JSON object", "code": "VALIDATION_ERROR"
            }))));
        }
    }
    let method = data.method.to_uppercase();
    if !["GET", "HEAD", "POST"].contains(&method.as_str()) {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "Method must be GET, HEAD, or POST", "code": "VALIDATION_ERROR"
        }))));
    }
    let interval = data.interval_seconds.unwrap_or(600).max(600);
    let timeout = data.timeout_ms.unwrap_or(10000).max(1000).min(60000);
    let expected_status = data.expected_status.unwrap_or(200);
    let confirmation = data.confirmation_threshold.unwrap_or(2).max(1).min(10);

    let id = uuid::Uuid::new_v4().to_string();
    let manage_key = generate_key();
    let key_hash = hash_key(&manage_key);
    let tags_str = tags_to_string(&data.tags);
    // Validate response_time_threshold_ms: must be >= 100ms if provided
    let rt_threshold = data.response_time_threshold_ms.map(|v| v.max(100));
    let follow_redirects = data.follow_redirects.unwrap_or(true);

    let conn = db.conn.lock().unwrap();
    conn.execute(
        "INSERT INTO monitors (id, name, url, method, interval_seconds, timeout_ms, expected_status, body_contains, headers, manage_key_hash, is_public, confirmation_threshold, tags, response_time_threshold_ms, follow_redirects)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
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
            follow_redirects as i32,
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

// ── Bulk Create Monitors ──

#[post("/monitors/bulk", format = "json", data = "<input>")]
pub fn bulk_create_monitors(
    input: Json<BulkCreateMonitors>,
    db: &State<Arc<Db>>,
    rate_limiter: &State<RateLimiter>,
    client_ip: ClientIp,
) -> Result<Json<BulkCreateResponse>, (Status, Json<serde_json::Value>)> {
    let data = input.into_inner();

    if data.monitors.is_empty() {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "monitors array is empty", "code": "VALIDATION_ERROR"
        }))));
    }
    if data.monitors.len() > 50 {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "Maximum 50 monitors per bulk request", "code": "VALIDATION_ERROR"
        }))));
    }

    let total = data.monitors.len();
    let mut created = Vec::new();
    let mut errors = Vec::new();
    let conn = db.conn.lock().unwrap();

    for (idx, monitor_data) in data.monitors.into_iter().enumerate() {
        // Rate limit per monitor
        if !rate_limiter.check(&client_ip.0) {
            errors.push(BulkError {
                index: idx,
                error: "Rate limit exceeded".into(),
                code: "RATE_LIMIT_EXCEEDED".into(),
            });
            continue;
        }

        // Validate
        if monitor_data.name.trim().is_empty() {
            errors.push(BulkError { index: idx, error: "Name is required".into(), code: "VALIDATION_ERROR".into() });
            continue;
        }
        if monitor_data.url.trim().is_empty() {
            errors.push(BulkError { index: idx, error: "URL is required".into(), code: "VALIDATION_ERROR".into() });
            continue;
        }
        let url_trimmed = monitor_data.url.trim().to_lowercase();
        if !url_trimmed.starts_with("http://") && !url_trimmed.starts_with("https://") {
            errors.push(BulkError { index: idx, error: "URL must start with http:// or https://".into(), code: "VALIDATION_ERROR".into() });
            continue;
        }
        if let Some(ref headers) = monitor_data.headers {
            if !headers.is_object() {
                errors.push(BulkError { index: idx, error: "Headers must be a JSON object".into(), code: "VALIDATION_ERROR".into() });
                continue;
            }
        }
        let method = monitor_data.method.to_uppercase();
        if !["GET", "HEAD", "POST"].contains(&method.as_str()) {
            errors.push(BulkError { index: idx, error: "Method must be GET, HEAD, or POST".into(), code: "VALIDATION_ERROR".into() });
            continue;
        }

        let interval = monitor_data.interval_seconds.unwrap_or(600).max(600);
        let timeout = monitor_data.timeout_ms.unwrap_or(10000).max(1000).min(60000);
        let expected_status = monitor_data.expected_status.unwrap_or(200);
        let confirmation = monitor_data.confirmation_threshold.unwrap_or(2).max(1).min(10);
        let rt_threshold = monitor_data.response_time_threshold_ms.map(|v| v.max(100));

        let id = uuid::Uuid::new_v4().to_string();
        let manage_key = generate_key();
        let key_hash = hash_key(&manage_key);
        let tags_str = tags_to_string(&monitor_data.tags);
        let follow_redirects = monitor_data.follow_redirects.unwrap_or(true);

        match conn.execute(
            "INSERT INTO monitors (id, name, url, method, interval_seconds, timeout_ms, expected_status, body_contains, headers, manage_key_hash, is_public, confirmation_threshold, tags, response_time_threshold_ms, follow_redirects)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                id,
                monitor_data.name.trim(),
                monitor_data.url.trim(),
                method,
                interval,
                timeout,
                expected_status,
                monitor_data.body_contains,
                monitor_data.headers.map(|h| h.to_string()),
                key_hash,
                monitor_data.is_public as i32,
                confirmation,
                tags_str,
                rt_threshold,
                follow_redirects as i32,
            ],
        ) {
            Ok(_) => {
                match get_monitor_from_db(&conn, &id) {
                    Ok(monitor) => {
                        created.push(CreateMonitorResponse {
                            monitor,
                            manage_key: manage_key.clone(),
                            manage_url: format!("/monitor/{}?key={}", id, manage_key),
                            view_url: format!("/monitor/{}", id),
                            api_base: format!("/api/v1/monitors/{}", id),
                        });
                    }
                    Err(e) => {
                        errors.push(BulkError { index: idx, error: format!("DB read error: {}", e), code: "INTERNAL_ERROR".into() });
                    }
                }
            }
            Err(e) => {
                errors.push(BulkError { index: idx, error: format!("DB error: {}", e), code: "INTERNAL_ERROR".into() });
            }
        }
    }

    let succeeded = created.len();
    let failed = errors.len();

    Ok(Json(BulkCreateResponse { created, errors, total, succeeded, failed }))
}

// ── Export Monitor Config ──

#[get("/monitors/<id>/export")]
pub fn export_monitor(
    id: &str,
    db: &State<Arc<Db>>,
    token: ManageToken,
) -> Result<Json<ExportedMonitor>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    verify_manage_key(&conn, id, &token.0)?;

    let monitor = get_monitor_from_db(&conn, id).map_err(|_| {
        (Status::NotFound, Json(serde_json::json!({
            "error": "Monitor not found", "code": "NOT_FOUND"
        })))
    })?;

    Ok(Json(ExportedMonitor {
        name: monitor.name,
        url: monitor.url,
        method: monitor.method,
        interval_seconds: monitor.interval_seconds,
        timeout_ms: monitor.timeout_ms,
        expected_status: monitor.expected_status,
        body_contains: monitor.body_contains,
        headers: monitor.headers,
        is_public: monitor.is_public,
        confirmation_threshold: monitor.confirmation_threshold,
        response_time_threshold_ms: monitor.response_time_threshold_ms,
        follow_redirects: monitor.follow_redirects,
        tags: monitor.tags,
    }))
}

// ── List Monitors (public only) ──

#[get("/monitors?<search>&<status>&<tag>")]
pub fn list_monitors(search: Option<&str>, status: Option<&str>, tag: Option<&str>, db: &State<Arc<Db>>) -> Result<Json<Vec<Monitor>>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();

    let mut sql = String::from(
        "SELECT id, name, url, method, interval_seconds, timeout_ms, expected_status, body_contains, headers, is_public, is_paused, current_status, last_checked_at, confirmation_threshold, created_at, updated_at, tags, response_time_threshold_ms, follow_redirects
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

    let mut data = input.into_inner();
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

    // Validate URL scheme if provided
    if let Some(ref url) = data.url {
        let url_lower = url.trim().to_lowercase();
        if !url_lower.starts_with("http://") && !url_lower.starts_with("https://") {
            return Err((Status::BadRequest, Json(serde_json::json!({
                "error": "URL must start with http:// or https://", "code": "VALIDATION_ERROR"
            }))));
        }
    }
    // Validate headers is a JSON object if provided
    if let Some(ref headers) = data.headers {
        if !headers.is_object() {
            return Err((Status::BadRequest, Json(serde_json::json!({
                "error": "Headers must be a JSON object", "code": "VALIDATION_ERROR"
            }))));
        }
    }

    // Clamp interval_seconds to minimum 600 (10 minutes)
    if let Some(interval) = data.interval_seconds {
        data.interval_seconds = Some(interval.max(600));
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

    if let Some(follow) = data.follow_redirects {
        updates.push(format!("follow_redirects = ?{}", values.len() + 1));
        values.push(Box::new(follow as i32));
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

// ── Dashboard ──

#[get("/dashboard")]
pub fn dashboard(db: &State<Arc<Db>>) -> Result<Json<DashboardOverview>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();

    // Total monitors
    let total_monitors: u32 = conn.query_row("SELECT COUNT(*) FROM monitors", [], |r| r.get(0)).unwrap_or(0);
    let public_monitors: u32 = conn.query_row("SELECT COUNT(*) FROM monitors WHERE is_public = 1", [], |r| r.get(0)).unwrap_or(0);
    let paused_monitors: u32 = conn.query_row("SELECT COUNT(*) FROM monitors WHERE is_paused = 1", [], |r| r.get(0)).unwrap_or(0);

    // Status counts
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

    // Active incidents
    let active_incidents: u32 = conn.query_row(
        "SELECT COUNT(*) FROM incidents WHERE resolved_at IS NULL", [], |r| r.get(0)
    ).unwrap_or(0);

    // Average uptime across all monitors (24h)
    let total_checks_24h: u32 = conn.query_row(
        "SELECT COUNT(*) FROM heartbeats WHERE checked_at > datetime('now', '-24 hours')", [], |r| r.get(0)
    ).unwrap_or(0);
    let up_checks_24h: u32 = conn.query_row(
        "SELECT COUNT(*) FROM heartbeats WHERE status = 'up' AND checked_at > datetime('now', '-24 hours')", [], |r| r.get(0)
    ).unwrap_or(0);
    let avg_uptime_24h = if total_checks_24h > 0 { (up_checks_24h as f64 / total_checks_24h as f64) * 100.0 } else { 100.0 };

    // Average uptime (7d)
    let total_checks_7d: u32 = conn.query_row(
        "SELECT COUNT(*) FROM heartbeats WHERE checked_at > datetime('now', '-7 days')", [], |r| r.get(0)
    ).unwrap_or(0);
    let up_checks_7d: u32 = conn.query_row(
        "SELECT COUNT(*) FROM heartbeats WHERE status = 'up' AND checked_at > datetime('now', '-7 days')", [], |r| r.get(0)
    ).unwrap_or(0);
    let avg_uptime_7d = if total_checks_7d > 0 { (up_checks_7d as f64 / total_checks_7d as f64) * 100.0 } else { 100.0 };

    // Average response time (24h, up checks only)
    let avg_response_ms_24h: Option<f64> = conn.query_row(
        "SELECT AVG(response_time_ms) FROM heartbeats WHERE status = 'up' AND checked_at > datetime('now', '-24 hours')",
        [], |r| r.get(0)
    ).ok();

    // Recent incidents (last 10, with monitor names)
    let recent_incidents: Vec<DashboardIncident> = {
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
    };

    // Slowest monitors (top 5 by avg response time, 24h, up checks only)
    let slowest_monitors: Vec<SlowMonitor> = {
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

    // Verify monitor exists
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
    } else if monitors.iter().all(|m| m.current_status == "up" || m.current_status == "maintenance") {
        "operational".to_string()
    } else if monitors.iter().any(|m| m.current_status == "unknown") {
        // If anything hasn't been checked yet, the overall status is unknown.
        "unknown".to_string()
    } else {
        // Remaining case: some degraded, none down, or in maintenance.
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

    // Validate title
    if data.title.trim().is_empty() {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "Title is required", "code": "VALIDATION_ERROR"
        }))));
    }

    // Validate timestamps (must be ISO-8601 parseable and ends_at > starts_at)
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

    // Verify monitor exists
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

    // Get the monitor_id for this maintenance window
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
GET /api/v1/dashboard — Aggregate dashboard (total monitors, uptime averages, active incidents, recent incidents, slowest monitors)
GET /api/v1/uptime-history?days=30 — Daily uptime percentages over time (aggregate across all monitors, max 90 days)
GET /api/v1/monitors/:id/uptime-history?days=30 — Daily uptime percentages for a specific monitor

## Auth
- Create monitor: no auth (returns manage_key, save it!)
- Read: no auth (use monitor UUID)
- Write: manage_key via Bearer header, X-API-Key, or ?key= param

## Validation
- URL must start with http:// or https://
- Headers must be a JSON object (not array or string)
- interval_seconds: min 600 (10 minutes), default 600
- timeout_ms: min 1000, max 60000, default 10000
- confirmation_threshold: min 1, max 10, default 2
- response_time_threshold_ms: min 100 (if set)

## Monitor Methods
GET, HEAD, POST

## Redirect Handling
By default, monitors follow HTTP redirects (301, 302, etc.) up to 10 hops.
Set follow_redirects: false on create/update to disable redirect following (useful for monitoring that a redirect is in place).
When follow_redirects is true (default), the final response after all redirects is evaluated against expected_status.

## Check Statuses
up, down, degraded (response time exceeds threshold), unknown (never checked)

## Response Time Alerts
Set response_time_threshold_ms on a monitor to get degraded status when response time exceeds threshold.
Triggers monitor.degraded / monitor.recovered webhook events.
Set to null to disable. Minimum: 100ms.

## Notification Types
webhook (POST JSON to URL), email (SMTP)

### Webhook Notifications
POST /api/v1/monitors/:id/notifications with {"name": "Slack", "channel_type": "webhook", "config": {"url": "https://hooks.slack.com/..."}}
On incident, POSTs JSON with event, monitor info, and incident details to the URL.

### Email Notifications
POST /api/v1/monitors/:id/notifications with {"name": "Ops Email", "channel_type": "email", "config": {"address": "ops@example.com"}}
Sends formatted email (HTML + plain text) on incident creation, resolution, degraded, and maintenance events.
Requires SMTP server configuration via environment variables:
  SMTP_HOST — SMTP server hostname (required to enable email)
  SMTP_PORT — Port (default: 587)
  SMTP_USERNAME — Auth username
  SMTP_PASSWORD — Auth password
  SMTP_FROM — Sender address (default: watchpost@<SMTP_HOST>)
  SMTP_TLS — "starttls" (default), "tls", or "none"

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

## Bulk Operations
POST /api/v1/monitors/bulk — create up to 50 monitors at once
  Body: {"monitors": [{"name": "...", "url": "..."}, ...]}
  Returns: {"created": [...], "errors": [...], "total": N, "succeeded": N, "failed": N}
  Each created monitor includes its manage_key (save them!)
  Partial success: some monitors may fail while others succeed

## Export
GET /api/v1/monitors/:id/export — export monitor config (auth required)
  Returns monitor settings in a format you can re-import via POST /monitors or /monitors/bulk
  Useful for backup, migration, or cloning monitors

## Endpoints
POST /api/v1/monitors — create monitor
POST /api/v1/monitors/bulk — bulk create monitors (up to 50)
GET /api/v1/monitors/:id/export — export monitor config (auth)
GET /api/v1/monitors — list public monitors (supports ?search= and ?status= filters)
GET /api/v1/monitors/:id — get monitor
PATCH /api/v1/monitors/:id — update (auth)
DELETE /api/v1/monitors/:id — delete (auth)
POST /api/v1/monitors/:id/pause — pause checks (auth)
POST /api/v1/monitors/:id/resume — resume checks (auth)
GET /api/v1/monitors/:id/heartbeats — check history
GET /api/v1/monitors/:id/uptime — uptime stats
GET /api/v1/monitors/:id/uptime-history — daily uptime history (?days=N, max 90)
GET /api/v1/uptime-history — aggregate daily uptime history (?days=N, max 90)
GET /api/v1/monitors/:id/incidents — incidents
POST /api/v1/incidents/:id/acknowledge — ack incident (auth)
POST /api/v1/monitors/:id/notifications — add notification (auth)
GET /api/v1/monitors/:id/notifications — list notifications (auth)
DELETE /api/v1/notifications/:id — remove notification (auth)
PATCH /api/v1/notifications/:id — enable/disable notification (auth)
POST /api/v1/monitors/:id/maintenance — create maintenance window (auth)
GET /api/v1/monitors/:id/maintenance — list maintenance windows
DELETE /api/v1/maintenance/:id — delete maintenance window (auth)
GET /api/v1/tags — list all unique tags (public monitors)
GET /api/v1/monitors/:id/badge/uptime — SVG uptime badge (?period=24h|7d|30d|90d, ?label=)
GET /api/v1/monitors/:id/badge/status — SVG status badge (?label=)
GET /api/v1/events — global SSE event stream
GET /api/v1/monitors/:id/events — per-monitor SSE event stream
GET /api/v1/status — public status page (supports ?tag= filter)
GET /api/v1/health — service health

## Status Badges (SVG)
GET /api/v1/monitors/:id/badge/uptime — SVG uptime badge (shields.io style)
  ?period=24h|7d|30d|90d (default: 24h)
  ?label=custom+label (default: "uptime 24h")
  Returns image/svg+xml — embed in README: ![uptime](https://watch.example.com/api/v1/monitors/:id/badge/uptime?period=7d)
GET /api/v1/monitors/:id/badge/status — SVG current status badge
  ?label=custom+label (default: "status")
  Color-coded: green=up, yellow=degraded, grey=paused/maintenance/unknown, red=down

## Maintenance Windows
Schedule downtime so checks still run but incidents are suppressed.
POST /api/v1/monitors/:id/maintenance with {"title": "Deploy v2", "starts_at": "2026-02-10T14:00:00Z", "ends_at": "2026-02-10T15:00:00Z"}
During an active window, monitor status shows "maintenance" instead of "down".
Heartbeats are still recorded. No incidents created. SSE events: maintenance.started, maintenance.ended.
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
    "/monitors/bulk": {
      "post": {
        "summary": "Bulk create monitors",
        "operationId": "bulkCreateMonitors",
        "tags": ["monitors"],
        "description": "Create up to 50 monitors in one request. Each monitor gets its own manage_key. Partial success: some may fail while others succeed. Each monitor counts against rate limit.",
        "requestBody": {
          "required": true,
          "content": { "application/json": { "schema": { "$ref": "#/components/schemas/BulkCreateMonitors" } } }
        },
        "responses": {
          "200": { "description": "Bulk creation results (may include partial failures)", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/BulkCreateResponse" } } } },
          "400": { "$ref": "#/components/responses/ValidationError" }
        }
      }
    },
    "/monitors/{id}/export": {
      "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }],
      "get": {
        "summary": "Export monitor config",
        "operationId": "exportMonitor",
        "tags": ["monitors"],
        "description": "Export monitor configuration in a format compatible with POST /monitors or /monitors/bulk. Useful for backup, migration, or cloning.",
        "security": [{ "manageKey": [] }],
        "responses": {
          "200": { "description": "Exportable monitor config", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ExportedMonitor" } } } },
          "403": { "$ref": "#/components/responses/Forbidden" },
          "404": { "$ref": "#/components/responses/NotFound" }
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
    "/dashboard": {
      "get": {
        "summary": "Dashboard overview with aggregate stats",
        "operationId": "dashboard",
        "tags": ["system"],
        "description": "Returns aggregate statistics across all monitors: counts by status, uptime averages, active incidents, recent incidents with monitor names, and slowest monitors.",
        "responses": {
          "200": {
            "description": "Dashboard data",
            "content": {
              "application/json": {
                "schema": {
                  "type": "object",
                  "properties": {
                    "total_monitors": { "type": "integer" },
                    "public_monitors": { "type": "integer" },
                    "paused_monitors": { "type": "integer" },
                    "status_counts": {
                      "type": "object",
                      "properties": {
                        "up": { "type": "integer" },
                        "down": { "type": "integer" },
                        "degraded": { "type": "integer" },
                        "unknown": { "type": "integer" },
                        "maintenance": { "type": "integer" }
                      }
                    },
                    "active_incidents": { "type": "integer" },
                    "avg_uptime_24h": { "type": "number" },
                    "avg_uptime_7d": { "type": "number" },
                    "avg_response_ms_24h": { "type": "number", "nullable": true },
                    "total_checks_24h": { "type": "integer" },
                    "recent_incidents": {
                      "type": "array",
                      "items": {
                        "type": "object",
                        "properties": {
                          "id": { "type": "string" },
                          "monitor_id": { "type": "string" },
                          "monitor_name": { "type": "string" },
                          "started_at": { "type": "string" },
                          "resolved_at": { "type": "string", "nullable": true },
                          "cause": { "type": "string" }
                        }
                      }
                    },
                    "slowest_monitors": {
                      "type": "array",
                      "items": {
                        "type": "object",
                        "properties": {
                          "id": { "type": "string" },
                          "name": { "type": "string" },
                          "avg_response_ms": { "type": "number" },
                          "current_status": { "type": "string" }
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    },
    "/uptime-history": {
      "get": {
        "summary": "Aggregate uptime history by day",
        "operationId": "uptimeHistory",
        "tags": ["system"],
        "description": "Returns daily uptime percentages, check counts, and average response times aggregated across all monitors.",
        "parameters": [
          { "name": "days", "in": "query", "schema": { "type": "integer", "default": 30, "minimum": 1, "maximum": 90 }, "description": "Number of days of history" }
        ],
        "responses": {
          "200": {
            "description": "Daily uptime data",
            "content": {
              "application/json": {
                "schema": {
                  "type": "array",
                  "items": {
                    "type": "object",
                    "properties": {
                      "date": { "type": "string", "format": "date" },
                      "uptime_pct": { "type": "number" },
                      "total_checks": { "type": "integer" },
                      "up_checks": { "type": "integer" },
                      "down_checks": { "type": "integer" },
                      "avg_response_ms": { "type": "number", "nullable": true }
                    }
                  }
                }
              }
            }
          }
        }
      }
    },
    "/monitors/{id}/uptime-history": {
      "get": {
        "summary": "Per-monitor uptime history by day",
        "operationId": "monitorUptimeHistory",
        "tags": ["monitors"],
        "description": "Returns daily uptime percentages, check counts, and average response times for a specific monitor.",
        "parameters": [
          { "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } },
          { "name": "days", "in": "query", "schema": { "type": "integer", "default": 30, "minimum": 1, "maximum": 90 }, "description": "Number of days of history" }
        ],
        "responses": {
          "200": {
            "description": "Daily uptime data for the monitor",
            "content": {
              "application/json": {
                "schema": {
                  "type": "array",
                  "items": {
                    "type": "object",
                    "properties": {
                      "date": { "type": "string", "format": "date" },
                      "uptime_pct": { "type": "number" },
                      "total_checks": { "type": "integer" },
                      "up_checks": { "type": "integer" },
                      "down_checks": { "type": "integer" },
                      "avg_response_ms": { "type": "number", "nullable": true }
                    }
                  }
                }
              }
            }
          },
          "404": { "description": "Monitor not found" }
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
    "/monitors/{id}/badge/uptime": {
      "parameters": [
        { "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } },
        { "name": "period", "in": "query", "schema": { "type": "string", "enum": ["24h", "7d", "30d", "90d"], "default": "24h" }, "description": "Time period for uptime calculation" },
        { "name": "label", "in": "query", "schema": { "type": "string" }, "description": "Custom badge label (default: uptime {period})" }
      ],
      "get": {
        "summary": "SVG uptime badge (shields.io style)",
        "operationId": "monitorUptimeBadge",
        "tags": ["badges"],
        "description": "Returns an SVG badge showing the monitor uptime percentage. Embed in README markdown: ![uptime](url)",
        "responses": {
          "200": { "description": "SVG badge image", "content": { "image/svg+xml": { "schema": { "type": "string" } } } },
          "404": { "$ref": "#/components/responses/NotFoundError" }
        }
      }
    },
    "/monitors/{id}/badge/status": {
      "parameters": [
        { "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } },
        { "name": "label", "in": "query", "schema": { "type": "string" }, "description": "Custom badge label (default: status)" }
      ],
      "get": {
        "summary": "SVG current status badge",
        "operationId": "monitorStatusBadge",
        "tags": ["badges"],
        "description": "Returns an SVG badge showing the monitor current status (up/down/degraded/unknown). Color-coded.",
        "responses": {
          "200": { "description": "SVG badge image", "content": { "image/svg+xml": { "schema": { "type": "string" } } } },
          "404": { "$ref": "#/components/responses/NotFoundError" }
        }
      }
    },
    "/monitors/{id}/maintenance": {
      "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }],
      "post": {
        "summary": "Create a maintenance window (suppresses incidents during the window)",
        "operationId": "createMaintenanceWindow",
        "tags": ["maintenance"],
        "security": [{ "manageKey": [] }],
        "requestBody": {
          "required": true,
          "content": {
            "application/json": {
              "schema": {
                "type": "object",
                "required": ["title", "starts_at", "ends_at"],
                "properties": {
                  "title": { "type": "string", "description": "Maintenance description" },
                  "starts_at": { "type": "string", "format": "date-time", "description": "Window start (ISO-8601 UTC)" },
                  "ends_at": { "type": "string", "format": "date-time", "description": "Window end (ISO-8601 UTC)" }
                }
              }
            }
          }
        },
        "responses": {
          "200": { "description": "Maintenance window created", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/MaintenanceWindow" } } } },
          "400": { "description": "Validation error" },
          "403": { "description": "Invalid manage key" }
        }
      },
      "get": {
        "summary": "List maintenance windows for a monitor",
        "operationId": "listMaintenanceWindows",
        "tags": ["maintenance"],
        "responses": {
          "200": { "description": "List of maintenance windows", "content": { "application/json": { "schema": { "type": "array", "items": { "$ref": "#/components/schemas/MaintenanceWindow" } } } } },
          "404": { "description": "Monitor not found" }
        }
      }
    },
    "/maintenance/{id}": {
      "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }],
      "delete": {
        "summary": "Delete a maintenance window",
        "operationId": "deleteMaintenanceWindow",
        "tags": ["maintenance"],
        "security": [{ "manageKey": [] }],
        "responses": {
          "200": { "description": "Deleted" },
          "403": { "description": "Invalid manage key" },
          "404": { "description": "Maintenance window not found" }
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
          "interval_seconds": { "type": "integer", "minimum": 600 },
          "timeout_ms": { "type": "integer" },
          "expected_status": { "type": "integer" },
          "body_contains": { "type": "string", "nullable": true },
          "headers": { "type": "object", "nullable": true },
          "is_public": { "type": "boolean" },
          "is_paused": { "type": "boolean" },
          "current_status": { "type": "string", "enum": ["unknown", "up", "down", "degraded", "maintenance"] },
          "last_checked_at": { "type": "string", "nullable": true },
          "confirmation_threshold": { "type": "integer" },
          "response_time_threshold_ms": { "type": "integer", "nullable": true, "description": "Mark as degraded when response time exceeds this (ms). Null = disabled." },
          "follow_redirects": { "type": "boolean", "description": "Whether HTTP redirects are followed (default: true)" },
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
          "url": { "type": "string", "description": "Must start with http:// or https://" },
          "method": { "type": "string", "enum": ["GET", "HEAD", "POST"], "default": "GET" },
          "interval_seconds": { "type": "integer", "minimum": 600, "default": 600 },
          "timeout_ms": { "type": "integer", "default": 10000 },
          "expected_status": { "type": "integer", "default": 200 },
          "body_contains": { "type": "string" },
          "headers": { "type": "object" },
          "is_public": { "type": "boolean", "default": false },
          "confirmation_threshold": { "type": "integer", "minimum": 1, "maximum": 10, "default": 2 },
          "response_time_threshold_ms": { "type": "integer", "minimum": 100, "nullable": true, "description": "Alert when response time exceeds this threshold (ms). Null = disabled." },
          "follow_redirects": { "type": "boolean", "default": true, "description": "Follow HTTP redirects (301, 302, etc.) up to 10 hops. Default: true." },
          "tags": { "type": "array", "items": { "type": "string" }, "description": "Freeform tags for grouping" }
        }
      },
      "BulkCreateMonitors": {
        "type": "object",
        "required": ["monitors"],
        "properties": {
          "monitors": { "type": "array", "items": { "$ref": "#/components/schemas/CreateMonitor" }, "maxItems": 50, "description": "Array of monitors to create (max 50)" }
        }
      },
      "BulkCreateResponse": {
        "type": "object",
        "properties": {
          "created": { "type": "array", "items": { "$ref": "#/components/schemas/CreateMonitorResponse" }, "description": "Successfully created monitors with manage keys" },
          "errors": { "type": "array", "items": { "$ref": "#/components/schemas/BulkError" }, "description": "Failed monitors with error details" },
          "total": { "type": "integer", "description": "Total monitors in request" },
          "succeeded": { "type": "integer", "description": "Number successfully created" },
          "failed": { "type": "integer", "description": "Number that failed" }
        }
      },
      "BulkError": {
        "type": "object",
        "properties": {
          "index": { "type": "integer", "description": "Index in the input array" },
          "error": { "type": "string" },
          "code": { "type": "string" }
        }
      },
      "ExportedMonitor": {
        "type": "object",
        "description": "Monitor config in a format compatible with POST /monitors for re-import",
        "properties": {
          "name": { "type": "string" },
          "url": { "type": "string" },
          "method": { "type": "string" },
          "interval_seconds": { "type": "integer" },
          "timeout_ms": { "type": "integer" },
          "expected_status": { "type": "integer" },
          "body_contains": { "type": "string", "nullable": true },
          "headers": { "type": "object", "nullable": true },
          "is_public": { "type": "boolean" },
          "confirmation_threshold": { "type": "integer" },
          "response_time_threshold_ms": { "type": "integer", "nullable": true },
          "tags": { "type": "array", "items": { "type": "string" } }
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
      "MaintenanceWindow": {
        "type": "object",
        "properties": {
          "id": { "type": "string", "format": "uuid" },
          "monitor_id": { "type": "string", "format": "uuid" },
          "title": { "type": "string" },
          "starts_at": { "type": "string", "format": "date-time" },
          "ends_at": { "type": "string", "format": "date-time" },
          "active": { "type": "boolean", "description": "Whether the window is currently active" },
          "created_at": { "type": "string" }
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

// ── Status Badges ──

/// Generate a shields.io-style SVG badge
fn render_badge(label: &str, value: &str, color: &str) -> String {
    // Approximate text widths (6.5px per char for 11px Verdana)
    let label_width = (label.len() as f64 * 6.5 + 10.0) as u32;
    let value_width = (value.len() as f64 * 6.5 + 10.0) as u32;
    let total_width = label_width + value_width;
    let lx = ((label_width as f64 / 2.0 + 1.0) * 10.0) as u32;
    let vx = ((label_width as f64 + value_width as f64 / 2.0 - 1.0) * 10.0) as u32;
    let lt = ((label_width as f64 - 10.0) * 10.0) as u32;
    let vt = ((value_width as f64 - 10.0) * 10.0) as u32;

    let mut s = String::with_capacity(1200);
    s.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"20\" role=\"img\" aria-label=\"{}: {}\">",
        total_width, label, value
    ));
    s.push_str(&format!("<title>{}: {}</title>", label, value));
    s.push_str("<linearGradient id=\"s\" x2=\"0\" y2=\"100%\"><stop offset=\"0\" stop-color=\"#bbb\" stop-opacity=\".1\"/><stop offset=\"1\" stop-opacity=\".1\"/></linearGradient>");
    s.push_str(&format!(
        "<clipPath id=\"r\"><rect width=\"{}\" height=\"20\" rx=\"3\" fill=\"#fff\"/></clipPath>",
        total_width
    ));
    s.push_str(&format!(
        "<g clip-path=\"url(#r)\"><rect width=\"{}\" height=\"20\" fill=\"#555\"/><rect x=\"{}\" width=\"{}\" height=\"20\" fill=\"{}\"/><rect width=\"{}\" height=\"20\" fill=\"url(#s)\"/></g>",
        label_width, label_width, value_width, color, total_width
    ));
    s.push_str("<g fill=\"#fff\" text-anchor=\"middle\" font-family=\"Verdana,Geneva,DejaVu Sans,sans-serif\" text-rendering=\"geometricPrecision\" font-size=\"11\">");
    s.push_str(&format!(
        "<text aria-hidden=\"true\" x=\"{}\" y=\"150\" fill=\"#010101\" fill-opacity=\".3\" transform=\"scale(.1)\" textLength=\"{}\">{}</text>",
        lx, lt, label
    ));
    s.push_str(&format!(
        "<text x=\"{}\" y=\"140\" transform=\"scale(.1)\" fill=\"#fff\" textLength=\"{}\">{}</text>",
        lx, lt, label
    ));
    s.push_str(&format!(
        "<text aria-hidden=\"true\" x=\"{}\" y=\"150\" fill=\"#010101\" fill-opacity=\".3\" transform=\"scale(.1)\" textLength=\"{}\">{}</text>",
        vx, vt, value
    ));
    s.push_str(&format!(
        "<text x=\"{}\" y=\"140\" transform=\"scale(.1)\" fill=\"#fff\" textLength=\"{}\">{}</text>",
        vx, vt, value
    ));
    s.push_str("</g></svg>");
    s
}

fn uptime_color(pct: f64) -> &'static str {
    if pct >= 99.9 { "#4c1" }        // bright green
    else if pct >= 99.0 { "#97ca00" } // green
    else if pct >= 95.0 { "#dfb317" } // yellow
    else if pct >= 90.0 { "#fe7d37" } // orange
    else { "#e05d44" }                 // red
}

fn status_color(status: &str) -> &'static str {
    match status {
        "up" => "#4c1",
        "degraded" => "#dfb317",
        "maintenance" => "#9f9f9f",
        "paused" => "#9f9f9f",
        "down" => "#e05d44",
        _ => "#9f9f9f", // unknown
    }
}

/// Badge showing uptime percentage for a monitor.
/// Query params: ?period=24h|7d|30d|90d (default 24h), ?label=custom+label
#[get("/monitors/<id>/badge/uptime?<period>&<label>")]
pub fn monitor_uptime_badge(
    id: &str,
    period: Option<&str>,
    label: Option<&str>,
    db: &State<Arc<Db>>,
) -> Result<(ContentType, String), (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    get_monitor_from_db(&conn, id)
        .map_err(|_| (Status::NotFound, Json(serde_json::json!({"error": "Monitor not found", "code": "NOT_FOUND"}))))?;

    let hours = match period.unwrap_or("24h") {
        "7d" => 168,
        "30d" => 720,
        "90d" => 2160,
        _ => 24, // default 24h
    };

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
    let value = format!("{:.1}%", pct);
    let color = uptime_color(pct);
    let period_str = period.unwrap_or("24h");
    let default_label = format!("uptime {}", period_str);
    let badge_label = label.unwrap_or(&default_label);

    let svg = render_badge(badge_label, &value, color);
    let ct = ContentType::new("image", "svg+xml");
    Ok((ct, svg))
}

/// Badge showing current status for a monitor.
/// Query param: ?label=custom+label
#[get("/monitors/<id>/badge/status?<label>")]
pub fn monitor_status_badge(
    id: &str,
    label: Option<&str>,
    db: &State<Arc<Db>>,
) -> Result<(ContentType, String), (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    let monitor = get_monitor_from_db(&conn, id)
        .map_err(|_| (Status::NotFound, Json(serde_json::json!({"error": "Monitor not found", "code": "NOT_FOUND"}))))?;

    let status = &monitor.current_status;
    let color = status_color(status);
    let badge_label = label.unwrap_or("status");

    let svg = render_badge(badge_label, status, color);
    let ct = ContentType::new("image", "svg+xml");
    Ok((ct, svg))
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
        "SELECT id, name, url, method, interval_seconds, timeout_ms, expected_status, body_contains, headers, is_public, is_paused, current_status, last_checked_at, confirmation_threshold, created_at, updated_at, tags, response_time_threshold_ms, follow_redirects
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
        follow_redirects: row.get::<_, i32>(18).unwrap_or(1) != 0,
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
