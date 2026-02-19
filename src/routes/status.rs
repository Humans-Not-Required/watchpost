use rocket::{get, serde::json::Json, State, http::Status};
use crate::db::Db;
use crate::models::{StatusOverview, StatusMonitor};
use super::{parse_tags, settings::{load_branding, branding_is_empty}};
use rusqlite::params;
use std::sync::Arc;

// ── Status Page ──

/// GET /api/v1/status — public status page.
///
/// Supports `?ids=id1,id2,id3` to filter to specific monitors (batch status check).
/// Also supports ?search=, ?status=, ?tag=, ?group= filters.
#[get("/status?<search>&<status>&<tag>&<group>&<ids>")]
pub fn status_page(search: Option<&str>, status: Option<&str>, tag: Option<&str>, group: Option<&str>, ids: Option<&str>, db: &State<Arc<Db>>) -> Result<Json<StatusOverview>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn();

    let mut sql = String::from("SELECT id, name, url, current_status, last_checked_at, tags, group_name FROM monitors WHERE is_public = 1");
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
    // ?ids=id1,id2,id3 — filter to specific monitors (batch status check)
    if let Some(ids_str) = ids {
        let id_list: Vec<String> = ids_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !id_list.is_empty() {
            let placeholders: Vec<String> = id_list
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", param_values.len() + i + 1))
                .collect();
            sql.push_str(&format!(" AND id IN ({})", placeholders.join(",")));
            for id_val in id_list {
                param_values.push(Box::new(id_val));
            }
        }
    }

    sql.push_str(" ORDER BY group_name NULLS LAST, name");

    let mut stmt = conn.prepare(&sql)
        .map_err(|_| (Status::InternalServerError, Json(serde_json::json!({"error": "Internal server error"}))))?;
    let params_vec: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|v| v.as_ref()).collect();

    let monitors: Vec<StatusMonitor> = stmt.query_map(params_vec.as_slice(), |row| {
        let id: String = row.get(0)?;
        let status: String = row.get(3)?;
        let tags_str: String = row.get::<_, String>(5).unwrap_or_default();
        let group_name: Option<String> = row.get::<_, Option<String>>(6).unwrap_or(None);
        Ok((id, row.get(1)?, row.get(2)?, status, row.get::<_, Option<String>>(4)?, tags_str, group_name))
    }).map_err(|_| (Status::InternalServerError, Json(serde_json::json!({"error": "Internal server error"}))))?
    .filter_map(|r| r.ok())
    .map(|(id, name, url, status, last_checked, tags_str, group_name)| {
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
            group_name,
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
        "unknown".to_string()
    } else {
        "degraded".to_string()
    };

    let branding = load_branding(&conn);
    let branding = if branding_is_empty(&branding) { None } else { Some(branding) };

    Ok(Json(StatusOverview { monitors, overall, branding }))
}
