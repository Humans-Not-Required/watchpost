use rocket::{get, post, patch, delete, serde::json::Json, State, http::Status};
use crate::db::Db;
use crate::models::{
    Monitor, CreateMonitor, UpdateMonitor, CreateMonitorResponse,
    BulkCreateMonitors, BulkCreateResponse, BulkError, ExportedMonitor,
};
use crate::auth::{ManageToken, ClientIp, generate_key, hash_key};
use super::{
    RateLimiter, get_monitor_from_db, row_to_monitor, tags_to_string,
    verify_manage_key, validate_tcp_address, validate_dns_hostname, VALID_DNS_RECORD_TYPES,
};
use rusqlite::params;
use std::sync::Arc;

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

    let monitor_type = data.monitor_type.as_deref().unwrap_or("http").to_lowercase();
    if !["http", "tcp", "dns"].contains(&monitor_type.as_str()) {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "monitor_type must be 'http', 'tcp', or 'dns'", "code": "VALIDATION_ERROR"
        }))));
    }

    if monitor_type == "tcp" {
        validate_tcp_address(data.url.trim())?;
    } else if monitor_type == "dns" {
        validate_dns_hostname(data.url.trim())?;
        let rt = data.dns_record_type.as_deref().unwrap_or("A").to_uppercase();
        if !VALID_DNS_RECORD_TYPES.contains(&rt.as_str()) {
            return Err((Status::BadRequest, Json(serde_json::json!({
                "error": format!("dns_record_type must be one of: {}", VALID_DNS_RECORD_TYPES.join(", ")),
                "code": "VALIDATION_ERROR"
            }))));
        }
    } else {
        let url_trimmed = data.url.trim().to_lowercase();
        if !url_trimmed.starts_with("http://") && !url_trimmed.starts_with("https://") {
            return Err((Status::BadRequest, Json(serde_json::json!({
                "error": "URL must start with http:// or https://", "code": "VALIDATION_ERROR"
            }))));
        }
    }
    if let Some(ref headers) = data.headers {
        if !headers.is_object() {
            return Err((Status::BadRequest, Json(serde_json::json!({
                "error": "Headers must be a JSON object", "code": "VALIDATION_ERROR"
            }))));
        }
    }
    let method = data.method.to_uppercase();
    if monitor_type == "http" && !["GET", "HEAD", "POST"].contains(&method.as_str()) {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "Method must be GET, HEAD, or POST", "code": "VALIDATION_ERROR"
        }))));
    }
    let interval = data.interval_seconds.unwrap_or(600).max(600);
    let timeout = data.timeout_ms.unwrap_or(10000).clamp(1000, 60000);
    let expected_status = data.expected_status.unwrap_or(200);
    let confirmation = data.confirmation_threshold.unwrap_or(2).clamp(1, 10);

    let id = uuid::Uuid::new_v4().to_string();
    let manage_key = generate_key();
    let key_hash = hash_key(&manage_key);
    let tags_str = tags_to_string(&data.tags);
    let rt_threshold = data.response_time_threshold_ms.map(|v| v.max(100));
    let follow_redirects = data.follow_redirects.unwrap_or(true);

    let group_name = data.group_name.as_deref().map(|g| g.trim()).filter(|g| !g.is_empty()).map(|g| g.to_string());
    let dns_record_type = data.dns_record_type.as_deref().unwrap_or("A").to_uppercase();
    let dns_expected = data.dns_expected.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| s.to_string());

    // Validate SLA target
    let sla_target = data.sla_target;
    if let Some(target) = sla_target {
        if !(0.0..=100.0).contains(&target) {
            return Err((Status::BadRequest, Json(serde_json::json!({
                "error": "sla_target must be between 0 and 100", "code": "VALIDATION_ERROR"
            }))));
        }
    }
    let sla_period_days = data.sla_period_days.map(|d| d.clamp(1, 365));

    // Validate consensus_threshold
    let consensus_threshold = data.consensus_threshold;
    if let Some(ct) = consensus_threshold {
        if ct < 1 {
            return Err((Status::BadRequest, Json(serde_json::json!({
                "error": "consensus_threshold must be at least 1", "code": "VALIDATION_ERROR"
            }))));
        }
    }

    let conn = db.conn.lock().unwrap();
    conn.execute(
        "INSERT INTO monitors (id, name, url, monitor_type, method, interval_seconds, timeout_ms, expected_status, body_contains, headers, manage_key_hash, is_public, confirmation_threshold, tags, response_time_threshold_ms, follow_redirects, group_name, dns_record_type, dns_expected, sla_target, sla_period_days, consensus_threshold)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
        params![
            id,
            data.name.trim(),
            data.url.trim(),
            monitor_type,
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
            group_name,
            dns_record_type,
            dns_expected,
            sla_target,
            sla_period_days,
            consensus_threshold,
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
        if !rate_limiter.check(&client_ip.0) {
            errors.push(BulkError {
                index: idx,
                error: "Rate limit exceeded".into(),
                code: "RATE_LIMIT_EXCEEDED".into(),
            });
            continue;
        }

        if monitor_data.name.trim().is_empty() {
            errors.push(BulkError { index: idx, error: "Name is required".into(), code: "VALIDATION_ERROR".into() });
            continue;
        }
        if monitor_data.url.trim().is_empty() {
            errors.push(BulkError { index: idx, error: "URL is required".into(), code: "VALIDATION_ERROR".into() });
            continue;
        }
        let bulk_monitor_type = monitor_data.monitor_type.as_deref().unwrap_or("http").to_lowercase();
        if !["http", "tcp", "dns"].contains(&bulk_monitor_type.as_str()) {
            errors.push(BulkError { index: idx, error: "monitor_type must be 'http', 'tcp', or 'dns'".into(), code: "VALIDATION_ERROR".into() });
            continue;
        }
        if bulk_monitor_type == "tcp" {
            if validate_tcp_address(monitor_data.url.trim()).is_err() {
                errors.push(BulkError { index: idx, error: "TCP address must be in host:port format".into(), code: "VALIDATION_ERROR".into() });
                continue;
            }
        } else if bulk_monitor_type == "dns" {
            if validate_dns_hostname(monitor_data.url.trim()).is_err() {
                errors.push(BulkError { index: idx, error: "DNS hostname must be a valid domain (e.g., 'example.com' or 'dns://example.com')".into(), code: "VALIDATION_ERROR".into() });
                continue;
            }
            let rt = monitor_data.dns_record_type.as_deref().unwrap_or("A").to_uppercase();
            if !VALID_DNS_RECORD_TYPES.contains(&rt.as_str()) {
                errors.push(BulkError { index: idx, error: format!("dns_record_type must be one of: {}", VALID_DNS_RECORD_TYPES.join(", ")), code: "VALIDATION_ERROR".into() });
                continue;
            }
        } else {
            let url_trimmed = monitor_data.url.trim().to_lowercase();
            if !url_trimmed.starts_with("http://") && !url_trimmed.starts_with("https://") {
                errors.push(BulkError { index: idx, error: "URL must start with http:// or https://".into(), code: "VALIDATION_ERROR".into() });
                continue;
            }
        }
        if let Some(ref headers) = monitor_data.headers {
            if !headers.is_object() {
                errors.push(BulkError { index: idx, error: "Headers must be a JSON object".into(), code: "VALIDATION_ERROR".into() });
                continue;
            }
        }
        let method = monitor_data.method.to_uppercase();
        if bulk_monitor_type == "http" && !["GET", "HEAD", "POST"].contains(&method.as_str()) {
            errors.push(BulkError { index: idx, error: "Method must be GET, HEAD, or POST".into(), code: "VALIDATION_ERROR".into() });
            continue;
        }

        let interval = monitor_data.interval_seconds.unwrap_or(600).max(600);
        let timeout = monitor_data.timeout_ms.unwrap_or(10000).clamp(1000, 60000);
        let expected_status = monitor_data.expected_status.unwrap_or(200);
        let confirmation = monitor_data.confirmation_threshold.unwrap_or(2).clamp(1, 10);
        let rt_threshold = monitor_data.response_time_threshold_ms.map(|v| v.max(100));

        let id = uuid::Uuid::new_v4().to_string();
        let manage_key = generate_key();
        let key_hash = hash_key(&manage_key);
        let tags_str = tags_to_string(&monitor_data.tags);
        let follow_redirects = monitor_data.follow_redirects.unwrap_or(true);
        let group_name = monitor_data.group_name.as_deref().map(|g| g.trim()).filter(|g| !g.is_empty()).map(|g| g.to_string());
        let bulk_dns_record_type = monitor_data.dns_record_type.as_deref().unwrap_or("A").to_uppercase();
        let bulk_dns_expected = monitor_data.dns_expected.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| s.to_string());
        let bulk_sla_target = monitor_data.sla_target;
        if let Some(target) = bulk_sla_target {
            if !(0.0..=100.0).contains(&target) {
                errors.push(BulkError { index: idx, error: "sla_target must be between 0 and 100".into(), code: "VALIDATION_ERROR".into() });
                continue;
            }
        }
        let bulk_sla_period = monitor_data.sla_period_days.map(|d| d.clamp(1, 365));
        let bulk_consensus = monitor_data.consensus_threshold;
        if let Some(ct) = bulk_consensus {
            if ct < 1 {
                errors.push(BulkError { index: idx, error: "consensus_threshold must be at least 1".into(), code: "VALIDATION_ERROR".into() });
                continue;
            }
        }

        match conn.execute(
            "INSERT INTO monitors (id, name, url, monitor_type, method, interval_seconds, timeout_ms, expected_status, body_contains, headers, manage_key_hash, is_public, confirmation_threshold, tags, response_time_threshold_ms, follow_redirects, group_name, dns_record_type, dns_expected, sla_target, sla_period_days, consensus_threshold)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
            params![
                id,
                monitor_data.name.trim(),
                monitor_data.url.trim(),
                bulk_monitor_type,
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
                group_name,
                bulk_dns_record_type,
                bulk_dns_expected,
                bulk_sla_target,
                bulk_sla_period,
                bulk_consensus,
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
        monitor_type: monitor.monitor_type,
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
        dns_record_type: monitor.dns_record_type,
        dns_expected: monitor.dns_expected,
        sla_target: monitor.sla_target,
        sla_period_days: monitor.sla_period_days,
        tags: monitor.tags,
        group_name: monitor.group_name,
        consensus_threshold: monitor.consensus_threshold,
    }))
}

// ── List Monitors (public only) ──

#[get("/monitors?<search>&<status>&<tag>&<group>")]
pub fn list_monitors(search: Option<&str>, status: Option<&str>, tag: Option<&str>, group: Option<&str>, db: &State<Arc<Db>>) -> Result<Json<Vec<Monitor>>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();

    let mut sql = String::from(
        "SELECT id, name, url, method, interval_seconds, timeout_ms, expected_status, body_contains, headers, is_public, is_paused, current_status, last_checked_at, confirmation_threshold, created_at, updated_at, tags, response_time_threshold_ms, follow_redirects, group_name, monitor_type, dns_record_type, dns_expected, sla_target, sla_period_days, consensus_threshold
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
    if let Some(g) = group {
        let g = g.trim();
        if !g.is_empty() {
            param_values.push(Box::new(g.to_string()));
            sql.push_str(&format!(" AND group_name = ?{}", param_values.len()));
        }
    }
    sql.push_str(" ORDER BY group_name NULLS LAST, name");

    let mut stmt = conn.prepare(&sql)
        .map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;
    let params_vec: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|v| v.as_ref()).collect();

    let monitors = stmt.query_map(params_vec.as_slice(), |row| {
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

    if let Some(ref mt) = data.monitor_type {
        let mt_lower = mt.trim().to_lowercase();
        if !["http", "tcp", "dns"].contains(&mt_lower.as_str()) {
            return Err((Status::BadRequest, Json(serde_json::json!({
                "error": "monitor_type must be 'http', 'tcp', or 'dns'", "code": "VALIDATION_ERROR"
            }))));
        }
    }

    let current_type: String = conn.query_row(
        "SELECT COALESCE(monitor_type, 'http') FROM monitors WHERE id = ?1",
        params![id],
        |row| row.get(0),
    ).unwrap_or_else(|_| "http".to_string());
    let effective_type = data.monitor_type.as_deref().unwrap_or(&current_type).to_lowercase();

    if let Some(ref url) = data.url {
        if effective_type == "tcp" {
            validate_tcp_address(url.trim())?;
        } else if effective_type == "dns" {
            validate_dns_hostname(url.trim())?;
        } else {
            let url_lower = url.trim().to_lowercase();
            if !url_lower.starts_with("http://") && !url_lower.starts_with("https://") {
                return Err((Status::BadRequest, Json(serde_json::json!({
                    "error": "URL must start with http:// or https://", "code": "VALIDATION_ERROR"
                }))));
            }
        }
    }
    if let Some(ref rt) = data.dns_record_type {
        let rt_upper = rt.trim().to_uppercase();
        if !VALID_DNS_RECORD_TYPES.contains(&rt_upper.as_str()) {
            return Err((Status::BadRequest, Json(serde_json::json!({
                "error": format!("dns_record_type must be one of: {}", VALID_DNS_RECORD_TYPES.join(", ")),
                "code": "VALIDATION_ERROR"
            }))));
        }
    }
    if let Some(ref headers) = data.headers {
        if !headers.is_object() {
            return Err((Status::BadRequest, Json(serde_json::json!({
                "error": "Headers must be a JSON object", "code": "VALIDATION_ERROR"
            }))));
        }
    }

    if let Some(interval) = data.interval_seconds {
        data.interval_seconds = Some(interval.max(600));
    }

    add_update!(name, "name");
    add_update!(url, "url");
    if let Some(ref mt) = data.monitor_type {
        updates.push(format!("monitor_type = ?{}", values.len() + 1));
        values.push(Box::new(mt.trim().to_lowercase()));
        data.monitor_type = None;
    }
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

    if let Some(ref gn) = data.group_name {
        updates.push(format!("group_name = ?{}", values.len() + 1));
        let trimmed = gn.trim();
        if trimmed.is_empty() {
            values.push(Box::new(None::<String>));
        } else {
            values.push(Box::new(trimmed.to_string()));
        }
    }

    if let Some(ref rt) = data.dns_record_type {
        updates.push(format!("dns_record_type = ?{}", values.len() + 1));
        values.push(Box::new(rt.trim().to_uppercase()));
    }
    if let Some(ref expected) = data.dns_expected {
        updates.push(format!("dns_expected = ?{}", values.len() + 1));
        let trimmed = expected.trim();
        if trimmed.is_empty() {
            values.push(Box::new(None::<String>));
        } else {
            values.push(Box::new(trimmed.to_string()));
        }
    }

    if let Some(ref sla_opt) = data.sla_target {
        updates.push(format!("sla_target = ?{}", values.len() + 1));
        match sla_opt {
            Some(val) => {
                if !(0.0..=100.0).contains(val) {
                    return Err((Status::BadRequest, Json(serde_json::json!({
                        "error": "sla_target must be between 0 and 100", "code": "VALIDATION_ERROR"
                    }))));
                }
                values.push(Box::new(Some(*val)));
            }
            None => values.push(Box::new(None::<f64>)),
        }
    }
    if let Some(ref period_opt) = data.sla_period_days {
        updates.push(format!("sla_period_days = ?{}", values.len() + 1));
        match period_opt {
            Some(val) => values.push(Box::new(Some((*val).clamp(1, 365)))),
            None => values.push(Box::new(None::<u32>)),
        }
    }

    if let Some(ref ct_opt) = data.consensus_threshold {
        updates.push(format!("consensus_threshold = ?{}", values.len() + 1));
        match ct_opt {
            Some(val) => {
                if *val < 1 {
                    return Err((Status::BadRequest, Json(serde_json::json!({
                        "error": "consensus_threshold must be at least 1", "code": "VALIDATION_ERROR"
                    }))));
                }
                values.push(Box::new(Some(*val)));
            }
            None => values.push(Box::new(None::<u32>)),
        }
    }

    if updates.is_empty() {
        return Ok(Json(serde_json::json!({"message": "No changes"})));
    }

    updates.push("updated_at = datetime('now')".to_string());
    let sql = format!("UPDATE monitors SET {} WHERE id = ?{}", updates.join(", "), values.len() + 1);
    values.push(Box::new(id.to_string()));

    let params_vec: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();
    conn.execute(&sql, params_vec.as_slice())
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
