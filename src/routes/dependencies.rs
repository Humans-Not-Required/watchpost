use rocket::{State, http::Status, serde::json::Json, post, get, delete};
use std::sync::Arc;
use crate::db::Db;
use crate::models::{MonitorDependency, CreateDependency};
use crate::auth::ManageToken;
use rusqlite::params;

use super::verify_manage_key;

/// Add a dependency to a monitor (manage key required).
/// When depends_on monitor is down, this monitor's alerts are suppressed.
#[post("/monitors/<id>/dependencies", data = "<body>")]
pub fn add_dependency(
    id: &str,
    body: Json<CreateDependency>,
    db: &State<Arc<Db>>,
    auth_header: ManageToken,
) -> Result<(Status, Json<serde_json::Value>), (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    verify_manage_key(&conn, id, &auth_header.0)?;

    let depends_on_id = &body.depends_on_id;

    // Validate: monitor exists
    let monitor_exists: bool = conn
        .query_row("SELECT COUNT(*) FROM monitors WHERE id = ?1", params![id], |r| r.get::<_, i64>(0))
        .map(|c| c > 0)
        .unwrap_or(false);
    if !monitor_exists {
        return Err((Status::NotFound, Json(serde_json::json!({
            "error": "Monitor not found", "code": "NOT_FOUND"
        }))));
    }

    // Validate: dependency monitor exists
    let dep_exists: bool = conn
        .query_row("SELECT COUNT(*) FROM monitors WHERE id = ?1", params![depends_on_id], |r| r.get::<_, i64>(0))
        .map(|c| c > 0)
        .unwrap_or(false);
    if !dep_exists {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "Dependency monitor not found", "code": "DEPENDENCY_NOT_FOUND"
        }))));
    }

    // Validate: no self-dependency
    if id == depends_on_id {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "A monitor cannot depend on itself", "code": "SELF_DEPENDENCY"
        }))));
    }

    // Validate: no circular dependency
    // Walk the dependency chain from depends_on_id to see if it eventually reaches id
    if has_circular_dependency(&conn, depends_on_id, id) {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "Adding this dependency would create a circular chain", "code": "CIRCULAR_DEPENDENCY"
        }))));
    }

    // Validate: no duplicate
    let already_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM monitor_dependencies WHERE monitor_id = ?1 AND depends_on_id = ?2",
            params![id, depends_on_id],
            |r| r.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);
    if already_exists {
        return Err((Status::Conflict, Json(serde_json::json!({
            "error": "This dependency already exists", "code": "DUPLICATE_DEPENDENCY"
        }))));
    }

    let dep_id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO monitor_dependencies (id, monitor_id, depends_on_id) VALUES (?1, ?2, ?3)",
        params![dep_id, id, depends_on_id],
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({
        "error": format!("Failed to create dependency: {}", e), "code": "INTERNAL_ERROR"
    }))))?;

    // Fetch the created dependency with joined info
    let dep = get_dependency_row(&conn, &dep_id)
        .map_err(|_| (Status::InternalServerError, Json(serde_json::json!({
            "error": "Failed to read created dependency", "code": "INTERNAL_ERROR"
        }))))?;

    Ok((Status::Created, Json(serde_json::to_value(&dep).unwrap())))
}

/// List all dependencies for a monitor (public).
#[get("/monitors/<id>/dependencies")]
pub fn list_dependencies(
    id: &str,
    db: &State<Arc<Db>>,
) -> Result<Json<Vec<MonitorDependency>>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();

    // Check monitor exists
    let exists: bool = conn
        .query_row("SELECT COUNT(*) FROM monitors WHERE id = ?1", params![id], |r| r.get::<_, i64>(0))
        .map(|c| c > 0)
        .unwrap_or(false);
    if !exists {
        return Err((Status::NotFound, Json(serde_json::json!({
            "error": "Monitor not found", "code": "NOT_FOUND"
        }))));
    }

    let mut stmt = conn.prepare(
        "SELECT d.id, d.monitor_id, d.depends_on_id, m.name, m.current_status, d.created_at
         FROM monitor_dependencies d
         JOIN monitors m ON m.id = d.depends_on_id
         WHERE d.monitor_id = ?1
         ORDER BY d.created_at ASC"
    ).unwrap();

    let deps: Vec<MonitorDependency> = stmt.query_map(params![id], |row| {
        Ok(MonitorDependency {
            id: row.get(0)?,
            monitor_id: row.get(1)?,
            depends_on_id: row.get(2)?,
            depends_on_name: row.get(3)?,
            depends_on_status: row.get(4)?,
            created_at: row.get(5)?,
        })
    }).unwrap().filter_map(|r| r.ok()).collect();

    Ok(Json(deps))
}

/// Remove a dependency (manage key required).
#[delete("/monitors/<id>/dependencies/<dep_id>")]
pub fn remove_dependency(
    id: &str,
    dep_id: &str,
    db: &State<Arc<Db>>,
    auth_header: ManageToken,
) -> Result<Json<serde_json::Value>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    verify_manage_key(&conn, id, &auth_header.0)?;

    let deleted = conn.execute(
        "DELETE FROM monitor_dependencies WHERE id = ?1 AND monitor_id = ?2",
        params![dep_id, id],
    ).unwrap_or(0);

    if deleted == 0 {
        return Err((Status::NotFound, Json(serde_json::json!({
            "error": "Dependency not found", "code": "NOT_FOUND"
        }))));
    }

    Ok(Json(serde_json::json!({"deleted": true})))
}

/// List monitors that depend on this monitor (dependents / reverse lookup, public).
#[get("/monitors/<id>/dependents")]
pub fn list_dependents(
    id: &str,
    db: &State<Arc<Db>>,
) -> Result<Json<Vec<MonitorDependency>>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();

    let exists: bool = conn
        .query_row("SELECT COUNT(*) FROM monitors WHERE id = ?1", params![id], |r| r.get::<_, i64>(0))
        .map(|c| c > 0)
        .unwrap_or(false);
    if !exists {
        return Err((Status::NotFound, Json(serde_json::json!({
            "error": "Monitor not found", "code": "NOT_FOUND"
        }))));
    }

    let mut stmt = conn.prepare(
        "SELECT d.id, d.monitor_id, d.depends_on_id, m.name, m.current_status, d.created_at
         FROM monitor_dependencies d
         JOIN monitors m ON m.id = d.monitor_id
         WHERE d.depends_on_id = ?1
         ORDER BY d.created_at ASC"
    ).unwrap();

    let deps: Vec<MonitorDependency> = stmt.query_map(params![id], |row| {
        Ok(MonitorDependency {
            id: row.get(0)?,
            monitor_id: row.get(1)?,
            depends_on_id: row.get(2)?,
            depends_on_name: row.get(3)?,
            depends_on_status: row.get(4)?,
            created_at: row.get(5)?,
        })
    }).unwrap().filter_map(|r| r.ok()).collect();

    Ok(Json(deps))
}

// ── Helpers ──

/// Check if adding a dependency from `from_id` to `target_id` would create a cycle.
/// Walks the dependency chain starting from `from_id` to see if `target_id` is reachable.
fn has_circular_dependency(conn: &rusqlite::Connection, from_id: &str, target_id: &str) -> bool {
    let mut visited = std::collections::HashSet::new();
    let mut queue = vec![from_id.to_string()];

    while let Some(current) = queue.pop() {
        if current == target_id {
            return true;
        }
        if !visited.insert(current.clone()) {
            continue;
        }
        // Get all monitors that `current` depends on
        let mut stmt = conn.prepare(
            "SELECT depends_on_id FROM monitor_dependencies WHERE monitor_id = ?1"
        ).unwrap();
        let deps: Vec<String> = stmt.query_map(params![current], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        queue.extend(deps);
    }
    false
}

fn get_dependency_row(conn: &rusqlite::Connection, dep_id: &str) -> rusqlite::Result<MonitorDependency> {
    conn.query_row(
        "SELECT d.id, d.monitor_id, d.depends_on_id, m.name, m.current_status, d.created_at
         FROM monitor_dependencies d
         JOIN monitors m ON m.id = d.depends_on_id
         WHERE d.id = ?1",
        params![dep_id],
        |row| Ok(MonitorDependency {
            id: row.get(0)?,
            monitor_id: row.get(1)?,
            depends_on_id: row.get(2)?,
            depends_on_name: row.get(3)?,
            depends_on_status: row.get(4)?,
            created_at: row.get(5)?,
        }),
    )
}

/// Check if any of a monitor's dependencies are currently down.
/// Used by the checker to suppress alerts when upstream is down.
pub fn has_dependency_down(conn: &rusqlite::Connection, monitor_id: &str) -> bool {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM monitor_dependencies d
         JOIN monitors m ON m.id = d.depends_on_id
         WHERE d.monitor_id = ?1 AND m.current_status = 'down'",
        params![monitor_id],
        |r| r.get(0),
    ).unwrap_or(0);
    count > 0
}

/// Check if a monitor has any open (unresolved) incidents.
pub fn has_open_incident(conn: &rusqlite::Connection, monitor_id: &str) -> bool {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM incidents WHERE monitor_id = ?1 AND resolved_at IS NULL",
        params![monitor_id],
        |r| r.get(0),
    ).unwrap_or(0);
    count > 0
}
