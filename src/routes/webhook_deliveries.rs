use rocket::serde::json::Json;
use rocket::{get, State, http::Status};
use serde::Serialize;
use std::sync::Arc;

use crate::auth::ManageToken;
use crate::db::Db;
use super::verify_manage_key;

#[derive(Debug, Serialize)]
pub struct WebhookDeliveryResponse {
    pub id: String,
    pub delivery_group: String,
    pub monitor_id: String,
    pub event: String,
    pub url: String,
    pub attempt: i64,
    pub status: String,
    pub status_code: Option<i64>,
    pub error_message: Option<String>,
    pub response_time_ms: i64,
    pub created_at: String,
    pub seq: i64,
}

#[derive(Debug, Serialize)]
pub struct WebhookDeliveriesListResponse {
    pub deliveries: Vec<WebhookDeliveryResponse>,
    pub total: i64,
}

/// GET /api/v1/monitors/:id/webhook-deliveries — list webhook delivery attempts (manage key required)
#[get("/monitors/<monitor_id>/webhook-deliveries?<limit>&<after>&<event>&<status>")]
pub fn list_webhook_deliveries(
    monitor_id: &str,
    limit: Option<i64>,
    after: Option<i64>,
    event: Option<&str>,
    status: Option<&str>,
    token: ManageToken,
    db: &State<Arc<Db>>,
) -> Result<Json<WebhookDeliveriesListResponse>, (Status, Json<serde_json::Value>)> {
    let conn = db.conn.lock().unwrap();
    verify_manage_key(&conn, monitor_id, &token.0)?;

    let limit = limit.unwrap_or(50).clamp(1, 200);

    // Build WHERE clause dynamically
    let mut where_parts = vec!["monitor_id = ?1".to_string()];
    let mut bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(monitor_id.to_string())];

    if let Some(a) = after {
        bind_values.push(Box::new(a));
        where_parts.push(format!("seq > ?{}", bind_values.len()));
    }
    if let Some(e) = event {
        bind_values.push(Box::new(e.to_string()));
        where_parts.push(format!("event = ?{}", bind_values.len()));
    }
    if let Some(s) = status {
        bind_values.push(Box::new(s.to_string()));
        where_parts.push(format!("status = ?{}", bind_values.len()));
    }

    let where_clause = where_parts.join(" AND ");

    // Count total
    let count_sql = format!("SELECT COUNT(*) FROM webhook_deliveries WHERE {}", where_clause);
    let total: i64 = {
        let mut stmt = conn.prepare(&count_sql).map_err(|_| (Status::InternalServerError, Json(serde_json::json!({"error": "DB error"}))))?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> = bind_values.iter().map(|b| b.as_ref()).collect();
        stmt.query_row(params_ref.as_slice(), |r| r.get(0)).unwrap_or(0)
    };

    // Fetch rows
    bind_values.push(Box::new(limit));
    let fetch_sql = format!(
        "SELECT id, delivery_group, monitor_id, event, url, attempt, status, status_code, error_message, response_time_ms, created_at, seq \
         FROM webhook_deliveries WHERE {} ORDER BY seq DESC LIMIT ?{}",
        where_clause,
        bind_values.len()
    );

    let mut stmt = conn.prepare(&fetch_sql).map_err(|_| (Status::InternalServerError, Json(serde_json::json!({"error": "DB error"}))))?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = bind_values.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(params_ref.as_slice(), |row| {
        Ok(WebhookDeliveryResponse {
            id: row.get(0)?,
            delivery_group: row.get(1)?,
            monitor_id: row.get(2)?,
            event: row.get(3)?,
            url: row.get(4)?,
            attempt: row.get(5)?,
            status: row.get(6)?,
            status_code: row.get(7)?,
            error_message: row.get(8)?,
            response_time_ms: row.get(9)?,
            created_at: row.get(10)?,
            seq: row.get(11)?,
        })
    }).map_err(|_| (Status::InternalServerError, Json(serde_json::json!({"error": "DB error"}))))?;

    let deliveries: Vec<WebhookDeliveryResponse> = rows.filter_map(|r| r.ok()).collect();

    Ok(Json(WebhookDeliveriesListResponse {
        deliveries,
        total,
    }))
}
