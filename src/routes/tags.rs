use rocket::{get, serde::json::Json, State, http::Status};
use crate::db::Db;
use super::parse_tags;
use std::sync::Arc;

// ── Tags ──

#[get("/tags")]
pub fn list_tags(db: &State<Arc<Db>>) -> Result<Json<Vec<String>>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT DISTINCT tags FROM monitors WHERE is_public = 1 AND tags != ''"
    ).map_err(|_| (Status::InternalServerError, Json(serde_json::json!({"error": "Internal server error"}))))?;

    let mut all_tags: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let rows: Vec<String> = stmt.query_map([], |row| row.get(0))
        .map_err(|_| (Status::InternalServerError, Json(serde_json::json!({"error": "Internal server error"}))))?
        .filter_map(|r| r.ok())
        .collect();

    for tags_str in rows {
        for tag in parse_tags(&tags_str) {
            all_tags.insert(tag);
        }
    }

    Ok(Json(all_tags.into_iter().collect()))
}

// ── Groups ──

#[get("/groups")]
pub fn list_groups(db: &State<Arc<Db>>) -> Result<Json<Vec<String>>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT DISTINCT group_name FROM monitors WHERE is_public = 1 AND group_name IS NOT NULL AND group_name != '' ORDER BY group_name"
    ).map_err(|_| (Status::InternalServerError, Json(serde_json::json!({"error": "Internal server error"}))))?;

    let groups: Vec<String> = stmt.query_map([], |row| row.get(0))
        .map_err(|_| (Status::InternalServerError, Json(serde_json::json!({"error": "Internal server error"}))))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(groups))
}
