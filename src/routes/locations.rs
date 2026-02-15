use rocket::{get, post, delete, http::Status, serde::json::Json, State};
use rusqlite::params;
use uuid::Uuid;
use std::sync::Arc;

use crate::db::Db;
use crate::auth::{ManageToken, hash_key, generate_key};
use crate::sse::EventBroadcaster;
use crate::models::{
    CheckLocation, CreateCheckLocation, CreateCheckLocationResponse,
    ProbeSubmission, ProbeSubmissionResponse, ProbeError,
    MonitorLocationStatus, ConsensusStatus,
};

fn stale_threshold_minutes() -> u32 {
    std::env::var("PROBE_STALE_MINUTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30)
}

// ── Verify admin key against settings table ──
fn verify_admin_key(conn: &rusqlite::Connection, token: &str) -> Result<(), (Status, Json<serde_json::Value>)> {
    let stored_hash: String = conn.query_row(
        "SELECT value FROM settings WHERE key = 'admin_key_hash'",
        [],
        |row| row.get(0),
    ).map_err(|_| (Status::InternalServerError, Json(serde_json::json!({
        "error": "Admin key not configured", "code": "SERVER_ERROR"
    }))))?;

    if hash_key(token) != stored_hash {
        return Err((Status::Forbidden, Json(serde_json::json!({
            "error": "Invalid admin key", "code": "FORBIDDEN"
        }))));
    }
    Ok(())
}

// ── Verify probe key against check_locations table ──
fn verify_probe_key(conn: &rusqlite::Connection, token: &str) -> Result<String, (Status, Json<serde_json::Value>)> {
    let token_hash = hash_key(token);
    let location_id: String = conn.query_row(
        "SELECT id FROM check_locations WHERE probe_key_hash = ?1 AND is_active = 1",
        params![token_hash],
        |row| row.get(0),
    ).map_err(|_| (Status::Unauthorized, Json(serde_json::json!({
        "error": "Invalid or inactive probe key", "code": "UNAUTHORIZED"
    }))))?;
    Ok(location_id)
}

/// POST /api/v1/locations — Register a new check location (admin key required)
#[post("/locations", data = "<body>")]
pub fn create_location(
    body: Json<CreateCheckLocation>,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<CreateCheckLocationResponse>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    verify_admin_key(&conn, &token.0)?;

    let name = body.name.trim();
    if name.is_empty() || name.len() > 200 {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "Name must be 1-200 characters", "code": "VALIDATION_ERROR"
        }))));
    }

    if let Some(ref region) = body.region {
        if region.len() > 200 {
            return Err((Status::BadRequest, Json(serde_json::json!({
                "error": "Region must be at most 200 characters", "code": "VALIDATION_ERROR"
            }))));
        }
    }

    // Check for duplicate name
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) FROM check_locations WHERE name = ?1",
        params![name],
        |r| r.get::<_, i64>(0),
    ).unwrap_or(0) > 0;

    if exists {
        return Err((Status::Conflict, Json(serde_json::json!({
            "error": "A check location with this name already exists", "code": "DUPLICATE_NAME"
        }))));
    }

    let id = Uuid::new_v4().to_string();
    let probe_key = generate_key();
    let probe_key_hash = hash_key(&probe_key);

    conn.execute(
        "INSERT INTO check_locations (id, name, region, probe_key_hash, is_active, created_at) VALUES (?1, ?2, ?3, ?4, 1, datetime('now'))",
        params![id, name, body.region, probe_key_hash],
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({
        "error": format!("Failed to create location: {}", e), "code": "SERVER_ERROR"
    }))))?;

    let last_seen_at: Option<String> = None;
    let location = CheckLocation {
        id,
        name: name.to_string(),
        region: body.region.clone(),
        health_status: CheckLocation::compute_health(true, &last_seen_at, stale_threshold_minutes()),
        is_active: true,
        last_seen_at,
        created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    };

    Ok(Json(CreateCheckLocationResponse {
        location,
        probe_key,
    }))
}

/// GET /api/v1/locations — List all check locations (public)
#[get("/locations")]
pub fn list_locations(db: &State<Arc<Db>>) -> Json<Vec<CheckLocation>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, name, region, is_active, last_seen_at, created_at FROM check_locations ORDER BY created_at ASC"
    ).unwrap();

    let stale_min = stale_threshold_minutes();
    let locations = stmt.query_map([], |row| {
        let is_active = row.get::<_, i32>(3)? != 0;
        let last_seen_at: Option<String> = row.get(4)?;
        let health_status = CheckLocation::compute_health(is_active, &last_seen_at, stale_min);
        Ok(CheckLocation {
            id: row.get(0)?,
            name: row.get(1)?,
            region: row.get(2)?,
            is_active,
            last_seen_at,
            health_status,
            created_at: row.get(5)?,
        })
    }).unwrap().filter_map(|r| r.ok()).collect();

    Json(locations)
}

/// GET /api/v1/locations/<id> — Get a single check location
#[get("/locations/<id>")]
pub fn get_location(
    id: &str,
    db: &State<Arc<Db>>,
) -> Result<Json<CheckLocation>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    let stale_min = stale_threshold_minutes();
    let location = conn.query_row(
        "SELECT id, name, region, is_active, last_seen_at, created_at FROM check_locations WHERE id = ?1",
        params![id],
        |row| {
            let is_active = row.get::<_, i32>(3)? != 0;
            let last_seen_at: Option<String> = row.get(4)?;
            let health_status = CheckLocation::compute_health(is_active, &last_seen_at, stale_min);
            Ok(CheckLocation {
                id: row.get(0)?,
                name: row.get(1)?,
                region: row.get(2)?,
                is_active,
                last_seen_at,
                health_status,
                created_at: row.get(5)?,
            })
        },
    ).map_err(|_| (Status::NotFound, Json(serde_json::json!({
        "error": "Check location not found", "code": "NOT_FOUND"
    }))))?;

    Ok(Json(location))
}

/// DELETE /api/v1/locations/<id> — Remove a check location (admin key required)
#[delete("/locations/<id>")]
pub fn delete_location(
    id: &str,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<serde_json::Value>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    verify_admin_key(&conn, &token.0)?;

    let deleted = conn.execute(
        "DELETE FROM check_locations WHERE id = ?1",
        params![id],
    ).unwrap_or(0);

    if deleted == 0 {
        return Err((Status::NotFound, Json(serde_json::json!({
            "error": "Check location not found", "code": "NOT_FOUND"
        }))));
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// POST /api/v1/probe — Submit probe results from a remote check location (probe key auth)
#[post("/probe", data = "<body>")]
pub async fn submit_probe(
    body: Json<ProbeSubmission>,
    token: ManageToken,
    db: &State<Arc<Db>>,
    broadcaster: &State<Arc<EventBroadcaster>>,
) -> Result<Json<ProbeSubmissionResponse>, (Status, Json<serde_json::Value>)> {
    let mut consensus_monitor_ids: Vec<String> = Vec::new();

    let (accepted, rejected, errors) = {
        let conn = db.conn.lock().unwrap();
        let location_id = verify_probe_key(&conn, &token.0)?;

        if body.results.is_empty() {
            return Err((Status::BadRequest, Json(serde_json::json!({
                "error": "No probe results provided", "code": "VALIDATION_ERROR"
            }))));
        }

        if body.results.len() > 100 {
            return Err((Status::BadRequest, Json(serde_json::json!({
                "error": "Maximum 100 probe results per submission", "code": "VALIDATION_ERROR"
            }))));
        }

        let valid_statuses = ["up", "down", "degraded"];
        let mut accepted = 0usize;
        let mut errors: Vec<ProbeError> = Vec::new();

        for (i, result) in body.results.iter().enumerate() {
            // Validate status
            if !valid_statuses.contains(&result.status.as_str()) {
                errors.push(ProbeError {
                    index: i,
                    monitor_id: result.monitor_id.clone(),
                    error: format!("Invalid status '{}'. Must be one of: up, down, degraded", result.status),
                });
                continue;
            }

            // Verify monitor exists and check if consensus is configured
            let monitor_info: Option<Option<u32>> = conn.query_row(
                "SELECT consensus_threshold FROM monitors WHERE id = ?1",
                params![result.monitor_id],
                |r| r.get(0),
            ).ok();

            if monitor_info.is_none() {
                errors.push(ProbeError {
                    index: i,
                    monitor_id: result.monitor_id.clone(),
                    error: "Monitor not found".to_string(),
                });
                continue;
            }

            // Track monitors with consensus for post-submission evaluation
            if let Some(Some(_threshold)) = monitor_info {
                if !consensus_monitor_ids.contains(&result.monitor_id) {
                    consensus_monitor_ids.push(result.monitor_id.clone());
                }
            }

            let heartbeat_id = Uuid::new_v4().to_string();
            let checked_at = result.checked_at.clone()
                .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());

            // Get next seq
            let next_seq: i64 = conn.query_row(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM heartbeats",
                [],
                |r| r.get(0),
            ).unwrap_or(1);

            conn.execute(
                "INSERT INTO heartbeats (id, monitor_id, status, response_time_ms, status_code, error_message, checked_at, seq, location_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    heartbeat_id,
                    result.monitor_id,
                    result.status,
                    result.response_time_ms,
                    result.status_code,
                    result.error_message,
                    checked_at,
                    next_seq,
                    location_id,
                ],
            ).map_err(|e| {
                errors.push(ProbeError {
                    index: i,
                    monitor_id: result.monitor_id.clone(),
                    error: format!("Failed to store: {}", e),
                });
            }).ok();

            if errors.len() <= i { // No error was pushed for this one
                accepted += 1;
            }
        }

        // Update last_seen_at
        conn.execute(
            "UPDATE check_locations SET last_seen_at = datetime('now') WHERE id = ?1",
            params![location_id],
        ).ok();

        let rejected = errors.len();
        (accepted, rejected, errors)
    }; // DB lock released

    // Evaluate consensus for affected monitors (outside DB lock)
    if !consensus_monitor_ids.is_empty() {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        for monitor_id in &consensus_monitor_ids {
            crate::consensus::evaluate_and_apply(db, broadcaster, &client, monitor_id).await;
        }
    }

    Ok(Json(ProbeSubmissionResponse {
        accepted,
        rejected,
        errors,
    }))
}

/// GET /api/v1/monitors/<monitor_id>/consensus — Multi-region consensus status
#[get("/monitors/<monitor_id>/consensus")]
pub fn monitor_consensus(
    monitor_id: &str,
    db: &State<Arc<Db>>,
) -> Result<Json<ConsensusStatus>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();

    // Verify monitor exists
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) FROM monitors WHERE id = ?1",
        params![monitor_id],
        |r| r.get::<_, i64>(0),
    ).unwrap_or(0) > 0;

    if !exists {
        return Err((Status::NotFound, Json(serde_json::json!({
            "error": "Monitor not found", "code": "NOT_FOUND"
        }))));
    }

    // Check if consensus is configured
    let threshold: Option<u32> = conn.query_row(
        "SELECT consensus_threshold FROM monitors WHERE id = ?1",
        params![monitor_id],
        |row| row.get(0),
    ).unwrap_or(None);

    if threshold.is_none() {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "Consensus not configured for this monitor. Set consensus_threshold to enable.",
            "code": "CONSENSUS_NOT_CONFIGURED"
        }))));
    }

    drop(conn); // Release lock before calling consensus module

    crate::consensus::get_consensus_status(db, monitor_id)
        .ok_or_else(|| (Status::InternalServerError, Json(serde_json::json!({
            "error": "Failed to evaluate consensus", "code": "SERVER_ERROR"
        }))))
        .map(Json)
}

/// GET /api/v1/monitors/<monitor_id>/locations — Per-location status for a monitor
#[get("/monitors/<monitor_id>/locations")]
pub fn monitor_location_status(
    monitor_id: &str,
    db: &State<Arc<Db>>,
) -> Result<Json<Vec<MonitorLocationStatus>>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();

    // Verify monitor exists
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) FROM monitors WHERE id = ?1",
        params![monitor_id],
        |r| r.get::<_, i64>(0),
    ).unwrap_or(0) > 0;

    if !exists {
        return Err((Status::NotFound, Json(serde_json::json!({
            "error": "Monitor not found", "code": "NOT_FOUND"
        }))));
    }

    // Get the latest heartbeat per location for this monitor
    let mut stmt = conn.prepare(
        "SELECT cl.id, cl.name, cl.region, h.status, h.response_time_ms, h.checked_at
         FROM check_locations cl
         INNER JOIN heartbeats h ON h.location_id = cl.id AND h.monitor_id = ?1
         WHERE h.checked_at = (
             SELECT MAX(h2.checked_at) FROM heartbeats h2
             WHERE h2.monitor_id = ?1 AND h2.location_id = cl.id
         )
         AND cl.is_active = 1
         ORDER BY cl.name ASC"
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({
        "error": format!("Query error: {}", e), "code": "SERVER_ERROR"
    }))))?;

    let statuses = stmt.query_map(params![monitor_id], |row| {
        Ok(MonitorLocationStatus {
            location_id: row.get(0)?,
            location_name: row.get(1)?,
            region: row.get(2)?,
            last_status: row.get(3)?,
            last_response_time_ms: row.get(4)?,
            last_checked_at: row.get(5)?,
        })
    }).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({
        "error": format!("Query error: {}", e), "code": "SERVER_ERROR"
    }))))?
    .filter_map(|r| r.ok())
    .collect();

    Ok(Json(statuses))
}
