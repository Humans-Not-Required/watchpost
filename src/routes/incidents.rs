use rocket::{get, post, serde::json::Json, State, http::Status};
use crate::db::Db;
use crate::models::{Incident, AcknowledgeIncident, IncidentNote, CreateIncidentNote};
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
    let conn = db.conn();
    get_monitor_from_db(&conn, id)
        .map_err(|_| (Status::NotFound, Json(serde_json::json!({"error": "Monitor not found", "code": "NOT_FOUND"}))))?;

    let limit = limit.unwrap_or(20).min(100);
    let err_map = |_: rusqlite::Error| (Status::InternalServerError, Json(serde_json::json!({"error": "Internal server error"})));

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
    let conn = db.conn();

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
    ).map_err(|_| (Status::InternalServerError, Json(serde_json::json!({"error": "Internal server error"}))))?;

    Ok(Json(serde_json::json!({"message": "Incident acknowledged"})))
}

// ── Single Incident Detail ──

#[get("/incidents/<id>")]
pub fn get_incident(
    id: &str,
    db: &State<Arc<Db>>,
) -> Result<Json<serde_json::Value>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn();

    let incident: Incident = conn.query_row(
        "SELECT id, monitor_id, started_at, resolved_at, cause, acknowledgement, acknowledged_by, acknowledged_at, seq
         FROM incidents WHERE id = ?1",
        params![id],
        |row| Ok(Incident {
            id: row.get(0)?,
            monitor_id: row.get(1)?,
            started_at: row.get(2)?,
            resolved_at: row.get(3)?,
            cause: row.get(4)?,
            acknowledgement: row.get(5)?,
            acknowledged_by: row.get(6)?,
            acknowledged_at: row.get(7)?,
            seq: row.get(8)?,
        }),
    ).map_err(|_| (Status::NotFound, Json(serde_json::json!({"error": "Incident not found", "code": "NOT_FOUND"}))))?;

    // Include notes count
    let notes_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM incident_notes WHERE incident_id = ?1",
        params![id],
        |r| r.get(0),
    ).unwrap_or(0);

    let mut val = serde_json::to_value(&incident).unwrap_or_default();
    val["notes_count"] = serde_json::json!(notes_count);

    Ok(Json(val))
}

// ── Incident Notes ──

#[post("/incidents/<id>/notes", format = "json", data = "<input>")]
pub fn create_incident_note(
    id: &str,
    input: Json<CreateIncidentNote>,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<(Status, Json<IncidentNote>), (Status, Json<serde_json::Value>)> {
    let conn = db.conn();

    // Find incident and verify manage_key against its monitor
    let monitor_id: String = conn.query_row(
        "SELECT monitor_id FROM incidents WHERE id = ?1",
        params![id],
        |row| row.get(0),
    ).map_err(|_| (Status::NotFound, Json(serde_json::json!({"error": "Incident not found", "code": "NOT_FOUND"}))))?;

    verify_manage_key(&conn, &monitor_id, &token.0)?;

    let data = input.into_inner();

    // Validate content
    let content = data.content.trim();
    if content.is_empty() {
        return Err((Status::UnprocessableEntity, Json(serde_json::json!({
            "error": "Note content cannot be empty",
            "code": "EMPTY_CONTENT"
        }))));
    }
    if content.len() > 10_000 {
        return Err((Status::UnprocessableEntity, Json(serde_json::json!({
            "error": "Note content exceeds 10,000 character limit",
            "code": "CONTENT_TOO_LONG"
        }))));
    }

    // Validate author
    let author = data.author.trim();
    if author.is_empty() {
        return Err((Status::UnprocessableEntity, Json(serde_json::json!({
            "error": "Author cannot be empty",
            "code": "EMPTY_AUTHOR"
        }))));
    }
    if author.len() > 200 {
        return Err((Status::UnprocessableEntity, Json(serde_json::json!({
            "error": "Author name exceeds 200 character limit",
            "code": "AUTHOR_TOO_LONG"
        }))));
    }

    let note_id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO incident_notes (id, incident_id, content, author) VALUES (?1, ?2, ?3, ?4)",
        params![note_id, id, content, author],
    ).map_err(|_| (Status::InternalServerError, Json(serde_json::json!({"error": "Internal server error"}))))?;

    let note = conn.query_row(
        "SELECT id, incident_id, content, author, created_at FROM incident_notes WHERE id = ?1",
        params![note_id],
        |row| Ok(IncidentNote {
            id: row.get(0)?,
            incident_id: row.get(1)?,
            content: row.get(2)?,
            author: row.get(3)?,
            created_at: row.get(4)?,
        }),
    ).map_err(|_| (Status::InternalServerError, Json(serde_json::json!({"error": "Internal server error"}))))?;

    Ok((Status::Created, Json(note)))
}

#[get("/incidents/<id>/notes?<limit>")]
pub fn list_incident_notes(
    id: &str,
    limit: Option<u32>,
    db: &State<Arc<Db>>,
) -> Result<Json<Vec<IncidentNote>>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn();

    // Verify incident exists
    conn.query_row(
        "SELECT id FROM incidents WHERE id = ?1",
        params![id],
        |row| row.get::<_, String>(0),
    ).map_err(|_| (Status::NotFound, Json(serde_json::json!({"error": "Incident not found", "code": "NOT_FOUND"}))))?;

    let limit = limit.unwrap_or(50).min(200);

    let mut stmt = conn.prepare(
        "SELECT id, incident_id, content, author, created_at
         FROM incident_notes WHERE incident_id = ?1
         ORDER BY created_at ASC LIMIT ?2"
    ).map_err(|_| (Status::InternalServerError, Json(serde_json::json!({"error": "Internal server error"}))))?;

    let notes: Vec<IncidentNote> = stmt.query_map(params![id, limit], |row| {
        Ok(IncidentNote {
            id: row.get(0)?,
            incident_id: row.get(1)?,
            content: row.get(2)?,
            author: row.get(3)?,
            created_at: row.get(4)?,
        })
    })
    .map_err(|_| (Status::InternalServerError, Json(serde_json::json!({"error": "Internal server error"}))))?
    .filter_map(|r| r.ok())
    .collect();

    Ok(Json(notes))
}
