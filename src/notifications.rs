use crate::db::Db;
use rusqlite::params;
use serde::Serialize;

#[derive(Debug, Serialize, Clone)]
pub struct WebhookPayload {
    pub event: String,
    pub monitor: WebhookMonitor,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incident: Option<WebhookIncident>,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct WebhookMonitor {
    pub id: String,
    pub name: String,
    pub url: String,
    pub current_status: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct WebhookIncident {
    pub id: String,
    pub cause: String,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
}

/// Fetch enabled webhook URLs for a monitor.
pub fn get_webhook_urls(db: &Db, monitor_id: &str) -> Vec<String> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = match conn.prepare(
        "SELECT config FROM notification_channels WHERE monitor_id = ?1 AND channel_type = 'webhook' AND is_enabled = 1"
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let rows: Vec<String> = match stmt.query_map(params![monitor_id], |row| {
        let config_str: String = row.get(0)?;
        Ok(config_str)
    }) {
        Ok(mapped) => mapped.filter_map(|r| r.ok()).collect(),
        Err(_) => return vec![],
    };

    rows.into_iter()
        .filter_map(|config_str| {
            serde_json::from_str::<serde_json::Value>(&config_str)
                .ok()
                .and_then(|v| v["url"].as_str().map(|s| s.to_string()))
        })
        .collect()
}

/// Fire webhook notifications (async, best-effort).
pub async fn fire_webhooks(client: &reqwest::Client, urls: &[String], payload: &WebhookPayload) {
    for url in urls {
        let _ = client
            .post(url)
            .json(payload)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await;
        // Best-effort: log nothing on failure for now. Could add retry logic later.
    }
}

// ─── Email Notifications ────────────────────────────────────────────────────

use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use std::sync::OnceLock;

/// SMTP configuration loaded from environment variables once.
#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from_address: String,
    pub tls_mode: TlsMode,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TlsMode {
    StartTls,
    Tls,
    None,
}

static SMTP_CONFIG: OnceLock<Option<SmtpConfig>> = OnceLock::new();

/// Load SMTP config from env vars. Returns None if SMTP_HOST is not set.
pub fn get_smtp_config() -> &'static Option<SmtpConfig> {
    SMTP_CONFIG.get_or_init(|| {
        let host = std::env::var("SMTP_HOST").ok()?;
        let port = std::env::var("SMTP_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(587);
        let username = std::env::var("SMTP_USERNAME").unwrap_or_default();
        let password = std::env::var("SMTP_PASSWORD").unwrap_or_default();
        let from_address = std::env::var("SMTP_FROM")
            .unwrap_or_else(|_| format!("watchpost@{}", host));
        let tls_mode = match std::env::var("SMTP_TLS").unwrap_or_default().to_lowercase().as_str() {
            "tls" | "implicit" => TlsMode::Tls,
            "none" | "off" | "false" => TlsMode::None,
            _ => TlsMode::StartTls, // default
        };

        println!("📧 SMTP configured: {}:{} (from: {}, tls: {:?})", host, port, from_address, tls_mode);

        Some(SmtpConfig {
            host,
            port,
            username,
            password,
            from_address,
            tls_mode,
        })
    })
}

/// Fetch enabled email addresses for a monitor.
pub fn get_email_addresses(db: &Db, monitor_id: &str) -> Vec<String> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = match conn.prepare(
        "SELECT config FROM notification_channels WHERE monitor_id = ?1 AND channel_type = 'email' AND is_enabled = 1"
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let rows: Vec<String> = match stmt.query_map(params![monitor_id], |row| {
        let config_str: String = row.get(0)?;
        Ok(config_str)
    }) {
        Ok(mapped) => mapped.filter_map(|r| r.ok()).collect(),
        Err(_) => return vec![],
    };

    rows.into_iter()
        .filter_map(|config_str| {
            serde_json::from_str::<serde_json::Value>(&config_str)
                .ok()
                .and_then(|v| v["address"].as_str().map(|s| s.to_string()))
        })
        .collect()
}

/// Build email subject line from a webhook payload.
fn email_subject(payload: &WebhookPayload) -> String {
    let status_emoji = match payload.event.as_str() {
        "incident.created" => "🔴",
        "incident.resolved" => "🟢",
        "monitor.degraded" => "🟡",
        "monitor.recovered" => "🟢",
        "maintenance.started" => "🔧",
        "maintenance.ended" => "✅",
        _ => "ℹ️",
    };

    let event_label = match payload.event.as_str() {
        "incident.created" => "DOWN",
        "incident.resolved" => "RECOVERED",
        "monitor.degraded" => "DEGRADED",
        "monitor.recovered" => "RECOVERED",
        "maintenance.started" => "MAINTENANCE",
        "maintenance.ended" => "MAINTENANCE ENDED",
        _ => &payload.event,
    };

    format!(
        "{} [Watchpost] {} — {}",
        status_emoji, event_label, payload.monitor.name
    )
}

/// Build email body (plain text) from a webhook payload.
fn email_body_text(payload: &WebhookPayload) -> String {
    let mut body = String::new();

    body.push_str(&format!("Monitor: {}\n", payload.monitor.name));
    body.push_str(&format!("URL: {}\n", payload.monitor.url));
    body.push_str(&format!("Status: {}\n", payload.monitor.current_status));
    body.push_str(&format!("Event: {}\n", payload.event));
    body.push_str(&format!("Time: {}\n", payload.timestamp));

    if let Some(ref incident) = payload.incident {
        body.push_str("\n--- Incident ---\n");
        body.push_str(&format!("ID: {}\n", incident.id));
        body.push_str(&format!("Cause: {}\n", incident.cause));
        body.push_str(&format!("Started: {}\n", incident.started_at));
        if let Some(ref resolved) = incident.resolved_at {
            body.push_str(&format!("Resolved: {}\n", resolved));
        }
    }

    body.push_str("\n--\nSent by Watchpost\n");
    body
}

/// Build email body (HTML) from a webhook payload.
fn email_body_html(payload: &WebhookPayload) -> String {
    let status_color = match payload.event.as_str() {
        "incident.created" => "#e74c3c",
        "incident.resolved" | "monitor.recovered" | "maintenance.ended" => "#2ecc71",
        "monitor.degraded" => "#f39c12",
        "maintenance.started" => "#3498db",
        _ => "#95a5a6",
    };

    let event_label = match payload.event.as_str() {
        "incident.created" => "DOWN",
        "incident.resolved" => "RECOVERED",
        "monitor.degraded" => "DEGRADED",
        "monitor.recovered" => "RECOVERED",
        "maintenance.started" => "MAINTENANCE",
        "maintenance.ended" => "MAINTENANCE ENDED",
        _ => &payload.event,
    };

    let mut html = format!(
        r#"<!DOCTYPE html>
<html>
<body style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; background: #1a1a2e; color: #e0e0e0; padding: 24px;">
<div style="max-width: 560px; margin: 0 auto;">
  <div style="background: {color}; color: #fff; padding: 16px 20px; border-radius: 8px 8px 0 0; font-size: 18px; font-weight: 600;">
    {label} — {name}
  </div>
  <div style="background: #16213e; padding: 20px; border-radius: 0 0 8px 8px; border: 1px solid #0f3460; border-top: none;">
    <table style="width: 100%; border-collapse: collapse; color: #e0e0e0;">
      <tr><td style="padding: 6px 0; color: #8899aa;">Monitor</td><td style="padding: 6px 0;">{name}</td></tr>
      <tr><td style="padding: 6px 0; color: #8899aa;">URL</td><td style="padding: 6px 0;"><a href="{url}" style="color: #5dade2;">{url}</a></td></tr>
      <tr><td style="padding: 6px 0; color: #8899aa;">Status</td><td style="padding: 6px 0; font-weight: 600; color: {color};">{status}</td></tr>
      <tr><td style="padding: 6px 0; color: #8899aa;">Time</td><td style="padding: 6px 0;">{time}</td></tr>
    </table>"#,
        color = status_color,
        label = event_label,
        name = html_escape(&payload.monitor.name),
        url = html_escape(&payload.monitor.url),
        status = payload.monitor.current_status,
        time = payload.timestamp,
    );

    if let Some(ref incident) = payload.incident {
        html.push_str(&format!(
            r#"
    <hr style="border: none; border-top: 1px solid #0f3460; margin: 16px 0;">
    <table style="width: 100%; border-collapse: collapse; color: #e0e0e0;">
      <tr><td style="padding: 6px 0; color: #8899aa;">Incident</td><td style="padding: 6px 0; font-family: monospace; font-size: 13px;">{id}</td></tr>
      <tr><td style="padding: 6px 0; color: #8899aa;">Cause</td><td style="padding: 6px 0;">{cause}</td></tr>
      <tr><td style="padding: 6px 0; color: #8899aa;">Started</td><td style="padding: 6px 0;">{started}</td></tr>"#,
            id = &incident.id[..8],
            cause = html_escape(&incident.cause),
            started = incident.started_at,
        ));

        if let Some(ref resolved) = incident.resolved_at {
            html.push_str(&format!(
                r#"
      <tr><td style="padding: 6px 0; color: #8899aa;">Resolved</td><td style="padding: 6px 0; color: #2ecc71;">{}</td></tr>"#,
                resolved
            ));
        }

        html.push_str("    </table>");
    }

    html.push_str(
        r#"
  </div>
  <div style="text-align: center; margin-top: 16px; color: #555; font-size: 12px;">
    Sent by Watchpost · Agent-Native Monitoring
  </div>
</div>
</body>
</html>"#,
    );

    html
}

/// Minimal HTML escaping for email body.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Fire email notifications (async, best-effort).
pub async fn fire_emails(addresses: &[String], payload: &WebhookPayload) {
    let config = match get_smtp_config() {
        Some(c) => c,
        None => {
            println!("⚠️  Email notification skipped: SMTP not configured (set SMTP_HOST)");
            return;
        }
    };

    let subject = email_subject(payload);
    let text_body = email_body_text(payload);
    let html_body = email_body_html(payload);

    // Build SMTP transport
    let transport = match build_transport(config) {
        Ok(t) => t,
        Err(e) => {
            println!("⚠️  Email notification failed: could not build SMTP transport: {}", e);
            return;
        }
    };

    for address in addresses {
        let email = match Message::builder()
            .from(config.from_address.parse().unwrap_or_else(|_| {
                "watchpost@localhost".parse().unwrap()
            }))
            .to(match address.parse() {
                Ok(a) => a,
                Err(e) => {
                    println!("⚠️  Skipping invalid email address '{}': {}", address, e);
                    continue;
                }
            })
            .subject(&subject)
            .multipart(
                lettre::message::MultiPart::alternative()
                    .singlepart(
                        lettre::message::SinglePart::builder()
                            .header(ContentType::TEXT_PLAIN)
                            .body(text_body.clone()),
                    )
                    .singlepart(
                        lettre::message::SinglePart::builder()
                            .header(ContentType::TEXT_HTML)
                            .body(html_body.clone()),
                    ),
            ) {
            Ok(m) => m,
            Err(e) => {
                println!("⚠️  Failed to build email to '{}': {}", address, e);
                continue;
            }
        };

        match transport.send(email).await {
            Ok(_) => println!("📧 Email sent to {} for {}", address, payload.event),
            Err(e) => println!("⚠️  Failed to send email to '{}': {}", address, e),
        }
    }
}

/// Build an async SMTP transport from config.
fn build_transport(config: &SmtpConfig) -> Result<AsyncSmtpTransport<Tokio1Executor>, String> {
    let creds = if !config.username.is_empty() {
        Some(Credentials::new(
            config.username.clone(),
            config.password.clone(),
        ))
    } else {
        None
    };

    let builder = match config.tls_mode {
        TlsMode::Tls => {
            AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)
                .map_err(|e| format!("TLS relay error: {}", e))?
                .port(config.port)
        }
        TlsMode::StartTls => {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
                .map_err(|e| format!("STARTTLS relay error: {}", e))?
                .port(config.port)
        }
        TlsMode::None => {
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host)
                .port(config.port)
        }
    };

    let builder = if let Some(c) = creds {
        builder.credentials(c)
    } else {
        builder
    };

    Ok(builder.build())
}
