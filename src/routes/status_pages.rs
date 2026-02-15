use rocket::{get, post, patch, delete, serde::json::Json, State, http::Status};
use crate::db::Db;
use crate::auth::{ManageToken, generate_key, hash_key};
use crate::models::{
    StatusPage, CreateStatusPage, UpdateStatusPage, CreateStatusPageResponse,
    StatusPageDetail, StatusMonitor, AddMonitorsToPage,
};
use super::parse_tags;
use rusqlite::params;
use std::sync::Arc;
use uuid::Uuid;

// ── Helpers ──

fn get_status_page(conn: &rusqlite::Connection, slug_or_id: &str) -> Result<(StatusPage, String), (Status, Json<serde_json::Value>)> {
    // Try by slug first, then by id
    let result = conn.query_row(
        "SELECT id, slug, title, description, logo_url, custom_domain, is_public, manage_key_hash, created_at, updated_at
         FROM status_pages WHERE slug = ?1 OR id = ?1",
        params![slug_or_id],
        |row| {
            let id: String = row.get(0)?;
            let manage_key_hash: String = row.get(7)?;
            let monitor_count: u32 = 0; // will be filled below
            Ok((StatusPage {
                id: id.clone(),
                slug: row.get(1)?,
                title: row.get(2)?,
                description: row.get(3)?,
                logo_url: row.get(4)?,
                custom_domain: row.get(5)?,
                is_public: row.get::<_, i32>(6)? != 0,
                monitor_count,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            }, manage_key_hash))
        },
    ).map_err(|_| (Status::NotFound, Json(serde_json::json!({
        "error": "Status page not found", "code": "NOT_FOUND"
    }))))?;

    // Get monitor count
    let count: u32 = conn.query_row(
        "SELECT COUNT(*) FROM status_page_monitors WHERE status_page_id = ?1",
        params![&result.0.id],
        |row| row.get(0),
    ).unwrap_or(0);

    Ok((StatusPage { monitor_count: count, ..result.0 }, result.1))
}

fn verify_page_key(conn: &rusqlite::Connection, slug_or_id: &str, token: &str) -> Result<StatusPage, (Status, Json<serde_json::Value>)> {
    let (page, stored_hash) = get_status_page(conn, slug_or_id)?;
    if hash_key(token) != stored_hash {
        return Err((Status::Forbidden, Json(serde_json::json!({
            "error": "Invalid manage key", "code": "FORBIDDEN"
        }))));
    }
    Ok(page)
}

fn validate_slug(slug: &str) -> Result<(), (Status, Json<serde_json::Value>)> {
    if slug.is_empty() || slug.len() > 100 {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "Slug must be 1-100 characters", "code": "VALIDATION_ERROR"
        }))));
    }
    if !slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "Slug must contain only alphanumeric characters, hyphens, and underscores",
            "code": "VALIDATION_ERROR"
        }))));
    }
    Ok(())
}

// ── Create Status Page ──

#[post("/status-pages", data = "<body>")]
pub fn create_status_page(
    body: Json<CreateStatusPage>,
    db: &State<Arc<Db>>,
) -> Result<(Status, Json<CreateStatusPageResponse>), (Status, Json<serde_json::Value>)> {
    let input = body.into_inner();

    // Validate slug
    validate_slug(&input.slug)?;

    // Validate title
    if input.title.trim().is_empty() || input.title.len() > 200 {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "Title must be 1-200 characters", "code": "VALIDATION_ERROR"
        }))));
    }

    // Validate optional fields
    if let Some(ref desc) = input.description {
        if desc.len() > 2000 {
            return Err((Status::BadRequest, Json(serde_json::json!({
                "error": "Description must be at most 2000 characters", "code": "VALIDATION_ERROR"
            }))));
        }
    }
    if let Some(ref url) = input.logo_url {
        if url.len() > 2000 {
            return Err((Status::BadRequest, Json(serde_json::json!({
                "error": "Logo URL must be at most 2000 characters", "code": "VALIDATION_ERROR"
            }))));
        }
    }
    if let Some(ref domain) = input.custom_domain {
        if domain.is_empty() || domain.len() > 253 || domain.contains(' ') {
            return Err((Status::BadRequest, Json(serde_json::json!({
                "error": "Invalid custom domain", "code": "VALIDATION_ERROR"
            }))));
        }
    }

    let conn = db.conn.lock().unwrap();

    // Check slug uniqueness
    let slug_exists: bool = conn.query_row(
        "SELECT COUNT(*) FROM status_pages WHERE slug = ?1",
        params![&input.slug],
        |row| row.get::<_, i64>(0),
    ).map(|c| c > 0).unwrap_or(false);

    if slug_exists {
        return Err((Status::Conflict, Json(serde_json::json!({
            "error": "A status page with this slug already exists", "code": "SLUG_CONFLICT"
        }))));
    }

    // Check custom domain uniqueness
    if let Some(ref domain) = input.custom_domain {
        let domain_exists: bool = conn.query_row(
            "SELECT COUNT(*) FROM status_pages WHERE custom_domain = ?1",
            params![domain],
            |row| row.get::<_, i64>(0),
        ).map(|c| c > 0).unwrap_or(false);

        if domain_exists {
            return Err((Status::Conflict, Json(serde_json::json!({
                "error": "A status page with this custom domain already exists", "code": "DOMAIN_CONFLICT"
            }))));
        }
    }

    let id = Uuid::new_v4().to_string();
    let manage_key = generate_key();
    let manage_key_hash = hash_key(&manage_key);

    conn.execute(
        "INSERT INTO status_pages (id, slug, title, description, logo_url, custom_domain, is_public, manage_key_hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            &id,
            &input.slug,
            &input.title.trim(),
            &input.description,
            &input.logo_url,
            &input.custom_domain,
            input.is_public as i32,
            &manage_key_hash,
        ],
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    let page = StatusPage {
        id,
        slug: input.slug,
        title: input.title.trim().to_string(),
        description: input.description,
        logo_url: input.logo_url,
        custom_domain: input.custom_domain,
        is_public: input.is_public,
        monitor_count: 0,
        created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        updated_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    };

    Ok((Status::Created, Json(CreateStatusPageResponse {
        status_page: page,
        manage_key,
    })))
}

// ── List Status Pages ──

#[get("/status-pages")]
pub fn list_status_pages(db: &State<Arc<Db>>) -> Result<Json<Vec<StatusPage>>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();

    let mut stmt = conn.prepare(
        "SELECT sp.id, sp.slug, sp.title, sp.description, sp.logo_url, sp.custom_domain, sp.is_public, sp.created_at, sp.updated_at,
                (SELECT COUNT(*) FROM status_page_monitors WHERE status_page_id = sp.id) as monitor_count
         FROM status_pages sp
         WHERE sp.is_public = 1
         ORDER BY sp.title"
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    let pages: Vec<StatusPage> = stmt.query_map([], |row| {
        Ok(StatusPage {
            id: row.get(0)?,
            slug: row.get(1)?,
            title: row.get(2)?,
            description: row.get(3)?,
            logo_url: row.get(4)?,
            custom_domain: row.get(5)?,
            is_public: row.get::<_, i32>(6)? != 0,
            monitor_count: row.get(9)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    }).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?
    .filter_map(|r| r.ok())
    .collect();

    Ok(Json(pages))
}

// ── Get Status Page Detail ──

#[get("/status-pages/<slug_or_id>")]
pub fn get_status_page_detail(
    slug_or_id: &str,
    db: &State<Arc<Db>>,
) -> Result<Json<StatusPageDetail>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();

    let (page, _hash) = get_status_page(&conn, slug_or_id)?;

    // Non-public pages require auth (not enforced here for simplicity — they're just unlisted)
    // The page is accessible by slug/id if you know it, similar to unlisted YouTube videos

    // Get monitors assigned to this page
    let mut stmt = conn.prepare(
        "SELECT m.id, m.name, m.url, m.current_status, m.last_checked_at, m.tags, m.group_name
         FROM monitors m
         INNER JOIN status_page_monitors spm ON spm.monitor_id = m.id
         WHERE spm.status_page_id = ?1
         ORDER BY m.group_name NULLS LAST, m.name"
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    let monitors: Vec<StatusMonitor> = stmt.query_map(params![&page.id], |row| {
        let id: String = row.get(0)?;
        let tags_str: String = row.get::<_, String>(5).unwrap_or_default();
        let group_name: Option<String> = row.get::<_, Option<String>>(6).unwrap_or(None);
        Ok((id, row.get(1)?, row.get(2)?, row.get::<_, String>(3)?, row.get::<_, Option<String>>(4)?, tags_str, group_name))
    }).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?
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

    Ok(Json(StatusPageDetail {
        id: page.id,
        slug: page.slug,
        title: page.title,
        description: page.description,
        logo_url: page.logo_url,
        custom_domain: page.custom_domain,
        is_public: page.is_public,
        monitors,
        overall,
        created_at: page.created_at,
        updated_at: page.updated_at,
    }))
}

// ── Update Status Page ──

#[patch("/status-pages/<slug_or_id>", data = "<body>")]
pub fn update_status_page(
    slug_or_id: &str,
    body: Json<UpdateStatusPage>,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<StatusPage>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    let page = verify_page_key(&conn, slug_or_id, &token.0)?;
    let input = body.into_inner();

    // Validate new slug if provided
    if let Some(ref new_slug) = input.slug {
        validate_slug(new_slug)?;
        if new_slug != &page.slug {
            let exists: bool = conn.query_row(
                "SELECT COUNT(*) FROM status_pages WHERE slug = ?1 AND id != ?2",
                params![new_slug, &page.id],
                |row| row.get::<_, i64>(0),
            ).map(|c| c > 0).unwrap_or(false);
            if exists {
                return Err((Status::Conflict, Json(serde_json::json!({
                    "error": "A status page with this slug already exists", "code": "SLUG_CONFLICT"
                }))));
            }
        }
    }

    if let Some(ref title) = input.title {
        if title.trim().is_empty() || title.len() > 200 {
            return Err((Status::BadRequest, Json(serde_json::json!({
                "error": "Title must be 1-200 characters", "code": "VALIDATION_ERROR"
            }))));
        }
    }

    if let Some(ref domain) = input.custom_domain {
        if !domain.is_empty() {
            if domain.len() > 253 || domain.contains(' ') {
                return Err((Status::BadRequest, Json(serde_json::json!({
                    "error": "Invalid custom domain", "code": "VALIDATION_ERROR"
                }))));
            }
            let exists: bool = conn.query_row(
                "SELECT COUNT(*) FROM status_pages WHERE custom_domain = ?1 AND id != ?2",
                params![domain, &page.id],
                |row| row.get::<_, i64>(0),
            ).map(|c| c > 0).unwrap_or(false);
            if exists {
                return Err((Status::Conflict, Json(serde_json::json!({
                    "error": "A status page with this custom domain already exists", "code": "DOMAIN_CONFLICT"
                }))));
            }
        }
    }

    let new_slug = input.slug.unwrap_or(page.slug);
    let new_title = input.title.map(|t| t.trim().to_string()).unwrap_or(page.title);
    let new_description = input.description.or(page.description);
    let new_logo_url = input.logo_url.or(page.logo_url);
    let new_custom_domain = input.custom_domain.or(page.custom_domain);
    let new_is_public = input.is_public.unwrap_or(page.is_public);

    conn.execute(
        "UPDATE status_pages SET slug = ?1, title = ?2, description = ?3, logo_url = ?4, custom_domain = ?5, is_public = ?6, updated_at = datetime('now')
         WHERE id = ?7",
        params![
            &new_slug,
            &new_title,
            &new_description,
            &new_logo_url,
            &new_custom_domain,
            new_is_public as i32,
            &page.id,
        ],
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    let (updated, _) = get_status_page(&conn, &page.id)?;
    Ok(Json(updated))
}

// ── Delete Status Page ──

#[delete("/status-pages/<slug_or_id>")]
pub fn delete_status_page(
    slug_or_id: &str,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<serde_json::Value>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    let page = verify_page_key(&conn, slug_or_id, &token.0)?;

    conn.execute("DELETE FROM status_pages WHERE id = ?1", params![&page.id])
        .map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    Ok(Json(serde_json::json!({
        "message": "Status page deleted",
        "id": page.id,
        "slug": page.slug
    })))
}

// ── Add Monitors to Status Page ──

#[post("/status-pages/<slug_or_id>/monitors", data = "<body>")]
pub fn add_page_monitors(
    slug_or_id: &str,
    body: Json<AddMonitorsToPage>,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<serde_json::Value>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    let page = verify_page_key(&conn, slug_or_id, &token.0)?;
    let input = body.into_inner();

    if input.monitor_ids.is_empty() {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "monitor_ids must not be empty", "code": "VALIDATION_ERROR"
        }))));
    }

    if input.monitor_ids.len() > 100 {
        return Err((Status::BadRequest, Json(serde_json::json!({
            "error": "Cannot add more than 100 monitors at once", "code": "VALIDATION_ERROR"
        }))));
    }

    let mut added = 0u32;
    let mut skipped = 0u32;
    let mut errors: Vec<serde_json::Value> = Vec::new();

    for mid in &input.monitor_ids {
        // Verify monitor exists
        let exists: bool = conn.query_row(
            "SELECT COUNT(*) FROM monitors WHERE id = ?1",
            params![mid],
            |row| row.get::<_, i64>(0),
        ).map(|c| c > 0).unwrap_or(false);

        if !exists {
            errors.push(serde_json::json!({
                "monitor_id": mid,
                "error": "Monitor not found"
            }));
            continue;
        }

        // Insert (ignore duplicates)
        match conn.execute(
            "INSERT OR IGNORE INTO status_page_monitors (status_page_id, monitor_id) VALUES (?1, ?2)",
            params![&page.id, mid],
        ) {
            Ok(n) if n > 0 => added += 1,
            Ok(_) => skipped += 1,
            Err(e) => errors.push(serde_json::json!({
                "monitor_id": mid,
                "error": e.to_string()
            })),
        }
    }

    Ok(Json(serde_json::json!({
        "added": added,
        "skipped": skipped,
        "errors": errors,
        "total_monitors": conn.query_row(
            "SELECT COUNT(*) FROM status_page_monitors WHERE status_page_id = ?1",
            params![&page.id],
            |row| row.get::<_, u32>(0),
        ).unwrap_or(0)
    })))
}

// ── Remove Monitor from Status Page ──

#[delete("/status-pages/<slug_or_id>/monitors/<monitor_id>")]
pub fn remove_page_monitor(
    slug_or_id: &str,
    monitor_id: &str,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<serde_json::Value>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    let page = verify_page_key(&conn, slug_or_id, &token.0)?;

    let deleted = conn.execute(
        "DELETE FROM status_page_monitors WHERE status_page_id = ?1 AND monitor_id = ?2",
        params![&page.id, monitor_id],
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    if deleted == 0 {
        return Err((Status::NotFound, Json(serde_json::json!({
            "error": "Monitor not assigned to this status page", "code": "NOT_FOUND"
        }))));
    }

    Ok(Json(serde_json::json!({
        "message": "Monitor removed from status page",
        "monitor_id": monitor_id,
        "status_page_id": page.id
    })))
}

// ── List Monitors on a Status Page ──

#[get("/status-pages/<slug_or_id>/monitors")]
pub fn list_page_monitors(
    slug_or_id: &str,
    db: &State<Arc<Db>>,
) -> Result<Json<Vec<StatusMonitor>>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    let (page, _) = get_status_page(&conn, slug_or_id)?;

    let mut stmt = conn.prepare(
        "SELECT m.id, m.name, m.url, m.current_status, m.last_checked_at, m.tags, m.group_name
         FROM monitors m
         INNER JOIN status_page_monitors spm ON spm.monitor_id = m.id
         WHERE spm.status_page_id = ?1
         ORDER BY m.group_name NULLS LAST, m.name"
    ).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;

    let monitors: Vec<StatusMonitor> = stmt.query_map(params![&page.id], |row| {
        let id: String = row.get(0)?;
        let tags_str: String = row.get::<_, String>(5).unwrap_or_default();
        let group_name: Option<String> = row.get::<_, Option<String>>(6).unwrap_or(None);
        Ok((id, row.get(1)?, row.get(2)?, row.get::<_, String>(3)?, row.get::<_, Option<String>>(4)?, tags_str, group_name))
    }).map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?
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

    Ok(Json(monitors))
}
