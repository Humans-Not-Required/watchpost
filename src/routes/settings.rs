use rocket::{get, put, serde::json::Json, State, http::Status};
use crate::db::Db;
use crate::models::{SettingsResponse, UpdateSettings, StatusPageBranding};
use crate::auth::{ManageToken, hash_key};
use rusqlite::params;
use std::sync::Arc;

// ── Settings Helpers ──

pub(crate) fn get_setting(conn: &rusqlite::Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![key],
        |row| row.get(0),
    ).ok()
}

fn set_setting(conn: &rusqlite::Connection, key: &str, value: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![key, value],
    )?;
    Ok(())
}

fn delete_setting(conn: &rusqlite::Connection, key: &str) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
    Ok(())
}

pub(crate) fn load_branding(conn: &rusqlite::Connection) -> StatusPageBranding {
    StatusPageBranding {
        title: get_setting(conn, "branding_title"),
        description: get_setting(conn, "branding_description"),
        logo_url: get_setting(conn, "branding_logo_url"),
    }
}

pub(crate) fn branding_is_empty(b: &StatusPageBranding) -> bool {
    b.title.is_none() && b.description.is_none() && b.logo_url.is_none()
}

// ── Settings Endpoints ──

#[get("/settings")]
pub fn get_settings(db: &State<Arc<Db>>) -> Result<Json<SettingsResponse>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    Ok(Json(SettingsResponse {
        title: get_setting(&conn, "branding_title"),
        description: get_setting(&conn, "branding_description"),
        logo_url: get_setting(&conn, "branding_logo_url"),
    }))
}

#[put("/settings", data = "<body>")]
pub fn update_settings(
    body: Json<UpdateSettings>,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<SettingsResponse>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();

    let stored_hash: String = conn.query_row(
        "SELECT value FROM settings WHERE key = 'admin_key_hash'",
        [],
        |row| row.get(0),
    ).map_err(|_| (Status::InternalServerError, Json(serde_json::json!({"error": "Admin key not configured"}))))?;

    let provided_hash = hash_key(&token.0);
    if provided_hash != stored_hash {
        return Err((Status::Forbidden, Json(serde_json::json!({"error": "Invalid admin key"}))));
    }

    if let Some(ref title) = body.title {
        if title.is_empty() {
            delete_setting(&conn, "branding_title").ok();
        } else {
            set_setting(&conn, "branding_title", title)
                .map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;
        }
    }
    if let Some(ref desc) = body.description {
        if desc.is_empty() {
            delete_setting(&conn, "branding_description").ok();
        } else {
            set_setting(&conn, "branding_description", desc)
                .map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;
        }
    }
    if let Some(ref logo) = body.logo_url {
        if logo.is_empty() {
            delete_setting(&conn, "branding_logo_url").ok();
        } else {
            set_setting(&conn, "branding_logo_url", logo)
                .map_err(|e| (Status::InternalServerError, Json(serde_json::json!({"error": e.to_string()}))))?;
        }
    }

    Ok(Json(SettingsResponse {
        title: get_setting(&conn, "branding_title"),
        description: get_setting(&conn, "branding_description"),
        logo_url: get_setting(&conn, "branding_logo_url"),
    }))
}
