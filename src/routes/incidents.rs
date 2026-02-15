use rocket::{get, post, serde::json::Json, State, http::Status};
use crate::db::Db;
use crate::models::{Incident, AcknowledgeIncident};
use crate::auth::ManageToken;
use super::{get_monitor_from_db, verify_manage_key};
use rusqlite::params;
use std::sync::Arc;

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
