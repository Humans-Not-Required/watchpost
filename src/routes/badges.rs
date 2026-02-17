use rocket::{get, serde::json::Json, State, http::{Status, ContentType}};
use crate::db::Db;
use super::get_monitor_from_db;
use rusqlite::params;
use std::sync::Arc;

// ── Status Badges ──

/// Generate a shields.io-style SVG badge
fn render_badge(label: &str, value: &str, color: &str) -> String {
    let label_width = (label.len() as f64 * 6.5 + 10.0) as u32;
    let value_width = (value.len() as f64 * 6.5 + 10.0) as u32;
    let total_width = label_width + value_width;
    let lx = ((label_width as f64 / 2.0 + 1.0) * 10.0) as u32;
    let vx = ((label_width as f64 + value_width as f64 / 2.0 - 1.0) * 10.0) as u32;
    let lt = ((label_width as f64 - 10.0) * 10.0) as u32;
    let vt = ((value_width as f64 - 10.0) * 10.0) as u32;

    let mut s = String::with_capacity(1200);
    s.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"20\" role=\"img\" aria-label=\"{}: {}\">",
        total_width, label, value
    ));
    s.push_str(&format!("<title>{}: {}</title>", label, value));
    s.push_str("<linearGradient id=\"s\" x2=\"0\" y2=\"100%\"><stop offset=\"0\" stop-color=\"#bbb\" stop-opacity=\".1\"/><stop offset=\"1\" stop-opacity=\".1\"/></linearGradient>");
    s.push_str(&format!(
        "<clipPath id=\"r\"><rect width=\"{}\" height=\"20\" rx=\"3\" fill=\"#fff\"/></clipPath>",
        total_width
    ));
    s.push_str(&format!(
        "<g clip-path=\"url(#r)\"><rect width=\"{}\" height=\"20\" fill=\"#555\"/><rect x=\"{}\" width=\"{}\" height=\"20\" fill=\"{}\"/><rect width=\"{}\" height=\"20\" fill=\"url(#s)\"/></g>",
        label_width, label_width, value_width, color, total_width
    ));
    s.push_str("<g fill=\"#fff\" text-anchor=\"middle\" font-family=\"Verdana,Geneva,DejaVu Sans,sans-serif\" text-rendering=\"geometricPrecision\" font-size=\"11\">");
    s.push_str(&format!(
        "<text aria-hidden=\"true\" x=\"{}\" y=\"150\" fill=\"#010101\" fill-opacity=\".3\" transform=\"scale(.1)\" textLength=\"{}\">{}</text>",
        lx, lt, label
    ));
    s.push_str(&format!(
        "<text x=\"{}\" y=\"140\" transform=\"scale(.1)\" fill=\"#fff\" textLength=\"{}\">{}</text>",
        lx, lt, label
    ));
    s.push_str(&format!(
        "<text aria-hidden=\"true\" x=\"{}\" y=\"150\" fill=\"#010101\" fill-opacity=\".3\" transform=\"scale(.1)\" textLength=\"{}\">{}</text>",
        vx, vt, value
    ));
    s.push_str(&format!(
        "<text x=\"{}\" y=\"140\" transform=\"scale(.1)\" fill=\"#fff\" textLength=\"{}\">{}</text>",
        vx, vt, value
    ));
    s.push_str("</g></svg>");
    s
}

fn uptime_color(pct: f64) -> &'static str {
    if pct >= 99.9 { "#4c1" }
    else if pct >= 99.0 { "#97ca00" }
    else if pct >= 95.0 { "#dfb317" }
    else if pct >= 90.0 { "#fe7d37" }
    else { "#e05d44" }
}

fn status_color(status: &str) -> &'static str {
    match status {
        "up" => "#4c1",
        "degraded" => "#dfb317",
        "maintenance" => "#9f9f9f",
        "paused" => "#9f9f9f",
        "down" => "#e05d44",
        _ => "#9f9f9f",
    }
}

#[get("/monitors/<id>/badge/uptime?<period>&<label>")]
pub fn monitor_uptime_badge(
    id: &str,
    period: Option<&str>,
    label: Option<&str>,
    db: &State<Arc<Db>>,
) -> Result<(ContentType, String), (Status, Json<serde_json::Value>)> {
    let conn = db.conn();
    get_monitor_from_db(&conn, id)
        .map_err(|_| (Status::NotFound, Json(serde_json::json!({"error": "Monitor not found", "code": "NOT_FOUND"}))))?;

    let hours = match period.unwrap_or("24h") {
        "7d" => 168,
        "30d" => 720,
        "90d" => 2160,
        _ => 24,
    };

    let total: u32 = conn.query_row(
        "SELECT COUNT(*) FROM heartbeats WHERE monitor_id = ?1 AND checked_at > datetime('now', ?2)",
        params![id, format!("-{} hours", hours)],
        |row| row.get(0),
    ).unwrap_or(0);

    let up: u32 = conn.query_row(
        "SELECT COUNT(*) FROM heartbeats WHERE monitor_id = ?1 AND status = 'up' AND checked_at > datetime('now', ?2)",
        params![id, format!("-{} hours", hours)],
        |row| row.get(0),
    ).unwrap_or(0);

    let pct = if total > 0 { (up as f64 / total as f64) * 100.0 } else { 100.0 };
    let value = format!("{:.1}%", pct);
    let color = uptime_color(pct);
    let period_str = period.unwrap_or("24h");
    let default_label = format!("uptime {}", period_str);
    let badge_label = label.unwrap_or(&default_label);

    let svg = render_badge(badge_label, &value, color);
    let ct = ContentType::new("image", "svg+xml");
    Ok((ct, svg))
}

#[get("/monitors/<id>/badge/status?<label>")]
pub fn monitor_status_badge(
    id: &str,
    label: Option<&str>,
    db: &State<Arc<Db>>,
) -> Result<(ContentType, String), (Status, Json<serde_json::Value>)> {
    let conn = db.conn();
    let monitor = get_monitor_from_db(&conn, id)
        .map_err(|_| (Status::NotFound, Json(serde_json::json!({"error": "Monitor not found", "code": "NOT_FOUND"}))))?;

    let status = &monitor.current_status;
    let color = status_color(status);
    let badge_label = label.unwrap_or("status");

    let svg = render_badge(badge_label, status, color);
    let ct = ContentType::new("image", "svg+xml");
    Ok((ct, svg))
}
