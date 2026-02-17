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

/// Payload format for webhook notifications.
#[derive(Debug, Clone, PartialEq)]
pub enum PayloadFormat {
    /// Full structured JSON (default).
    Json,
    /// Simple chat message: `{"content": "...", "sender": "Watchpost"}`.
    /// Compatible with Local Agent Chat incoming webhooks, Slack, etc.
    Chat,
}

/// A resolved webhook channel with URL and payload format.
#[derive(Debug, Clone)]
pub struct WebhookChannel {
    pub url: String,
    pub payload_format: PayloadFormat,
}

/// Fetch enabled webhook channels for a monitor.
pub fn get_webhook_channels(db: &Db, monitor_id: &str) -> Vec<WebhookChannel> {
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
            let v: serde_json::Value = serde_json::from_str(&config_str).ok()?;
            let url = v["url"].as_str()?.to_string();
            let payload_format = match v["payload_format"].as_str() {
                Some("chat") => PayloadFormat::Chat,
                _ => PayloadFormat::Json,
            };
            Some(WebhookChannel { url, payload_format })
        })
        .collect()
}

/// Format a webhook payload as a human-readable chat message.
fn format_chat_message(payload: &WebhookPayload) -> String {
    let emoji = match payload.event.as_str() {
        "incident.created" => "🔴",
        "incident.resolved" => "🟢",
        "monitor.degraded" => "🟡",
        "monitor.recovered" => "🟢",
        "maintenance.started" => "🔧",
        "maintenance.ended" => "✅",
        "incident.reminder" => "🔔",
        "incident.escalated" => "🚨",
        _ => "ℹ️",
    };

    let event_label = match payload.event.as_str() {
        "incident.created" => "DOWN",
        "incident.resolved" => "Recovered",
        "monitor.degraded" => "Degraded",
        "monitor.recovered" => "Recovered",
        "maintenance.started" => "Maintenance started",
        "maintenance.ended" => "Maintenance ended",
        "incident.reminder" => "Still down",
        "incident.escalated" => "ESCALATED",
        _ => &payload.event,
    };

    let mut msg = format!(
        "{} **{}** — {}",
        emoji, payload.monitor.name, event_label
    );

    if let Some(ref incident) = payload.incident {
        if !incident.cause.is_empty() {
            msg.push_str(&format!("\nCause: {}", incident.cause));
        }
        if let Some(ref resolved_at) = incident.resolved_at {
            msg.push_str(&format!("\nResolved: {}", resolved_at));
        }
    }

    msg
}

/// Maximum retry attempts for webhook delivery.
const MAX_WEBHOOK_ATTEMPTS: u32 = 3;

/// Backoff durations between retries (attempt 2 waits 2s, attempt 3 waits 4s).
const RETRY_BACKOFFS_MS: [u64; 2] = [2000, 4000];

/// Fire webhook notifications with retry and delivery logging.
///
/// Each channel gets up to MAX_WEBHOOK_ATTEMPTS delivery attempts with exponential
/// backoff. Every attempt is logged to the webhook_deliveries table for audit.
/// Channels with `payload_format: Chat` receive a simple `{"content":"...","sender":"Watchpost"}`
/// payload instead of the full structured JSON.
pub async fn fire_webhooks(
    db: &Db,
    client: &reqwest::Client,
    monitor_id: &str,
    channels: &[WebhookChannel],
    payload: &WebhookPayload,
) {
    for channel in channels {
        let url = &channel.url;
        let delivery_group = uuid::Uuid::new_v4().to_string();

        // Build the appropriate payload body based on format
        let body: serde_json::Value = match channel.payload_format {
            PayloadFormat::Chat => {
                let content = format_chat_message(payload);
                serde_json::json!({
                    "content": content,
                    "sender": "Watchpost"
                })
            }
            PayloadFormat::Json => {
                serde_json::to_value(payload).unwrap_or_default()
            }
        };

        for attempt in 1..=MAX_WEBHOOK_ATTEMPTS {
            // Wait before retry (not on first attempt)
            if attempt > 1 {
                let backoff = RETRY_BACKOFFS_MS[(attempt - 2) as usize];
                tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
            }

            let start = std::time::Instant::now();
            let result = client
                .post(url)
                .json(&body)
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await;
            let elapsed_ms = start.elapsed().as_millis() as i64;

            match result {
                Ok(resp) => {
                    let status_code = resp.status().as_u16() as i64;
                    if resp.status().is_success() {
                        // Success — log and move to next URL
                        log_webhook_delivery(db, &DeliveryLogEntry {
                            delivery_group: &delivery_group, monitor_id, event: &payload.event,
                            url, attempt, status: "success", status_code: Some(status_code),
                            error_message: None, response_time_ms: elapsed_ms,
                        });
                        if attempt > 1 {
                            println!("✅ Webhook delivered to {} after {} attempts", url, attempt);
                        }
                        break;
                    } else {
                        // HTTP error response
                        let error_msg = format!("HTTP {}", status_code);
                        log_webhook_delivery(db, &DeliveryLogEntry {
                            delivery_group: &delivery_group, monitor_id, event: &payload.event,
                            url, attempt, status: "failed", status_code: Some(status_code),
                            error_message: Some(&error_msg), response_time_ms: elapsed_ms,
                        });
                        if attempt == MAX_WEBHOOK_ATTEMPTS {
                            println!(
                                "⚠️  Webhook delivery to {} exhausted after {} attempts (last: {})",
                                url, MAX_WEBHOOK_ATTEMPTS, error_msg
                            );
                        }
                    }
                }
                Err(e) => {
                    let error_msg = format!("{}", e);
                    log_webhook_delivery(db, &DeliveryLogEntry {
                        delivery_group: &delivery_group, monitor_id, event: &payload.event,
                        url, attempt, status: "failed", status_code: None,
                        error_message: Some(&error_msg), response_time_ms: elapsed_ms,
                    });
                    if attempt == MAX_WEBHOOK_ATTEMPTS {
                        println!(
                            "⚠️  Webhook delivery to {} exhausted after {} attempts (last: {})",
                            url, MAX_WEBHOOK_ATTEMPTS, error_msg
                        );
                    }
                }
            }
        }
    }
}

/// Parameters for logging a webhook delivery attempt.
struct DeliveryLogEntry<'a> {
    delivery_group: &'a str,
    monitor_id: &'a str,
    event: &'a str,
    url: &'a str,
    attempt: u32,
    status: &'a str,
    status_code: Option<i64>,
    error_message: Option<&'a str>,
    response_time_ms: i64,
}

/// Log a single webhook delivery attempt to the database.
fn log_webhook_delivery(db: &Db, entry: &DeliveryLogEntry<'_>) {
    let conn = db.conn.lock().unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let seq: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM webhook_deliveries",
            [],
            |r| r.get(0),
        )
        .unwrap_or(1);
    let _ = conn.execute(
        "INSERT INTO webhook_deliveries (id, delivery_group, monitor_id, event, url, attempt, status, status_code, error_message, response_time_ms, seq) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![id, entry.delivery_group, entry.monitor_id, entry.event, entry.url, entry.attempt, entry.status, entry.status_code, entry.error_message, entry.response_time_ms, seq],
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_payload(event: &str, monitor_name: &str, cause: &str) -> WebhookPayload {
        WebhookPayload {
            event: event.to_string(),
            monitor: WebhookMonitor {
                id: "mon_123".to_string(),
                name: monitor_name.to_string(),
                url: "https://example.com".to_string(),
                current_status: if event.contains("resolved") || event.contains("recovered") {
                    "up".to_string()
                } else {
                    "down".to_string()
                },
            },
            incident: if cause.is_empty() {
                None
            } else {
                Some(WebhookIncident {
                    id: "inc_456".to_string(),
                    cause: cause.to_string(),
                    started_at: "2026-02-17T03:00:00Z".to_string(),
                    resolved_at: if event.contains("resolved") {
                        Some("2026-02-17T03:05:00Z".to_string())
                    } else {
                        None
                    },
                })
            },
            timestamp: "2026-02-17T03:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_chat_message_incident_created() {
        let payload = make_payload("incident.created", "Blog", "Connection refused");
        let msg = format_chat_message(&payload);
        assert!(msg.contains("🔴"));
        assert!(msg.contains("**Blog**"));
        assert!(msg.contains("DOWN"));
        assert!(msg.contains("Connection refused"));
    }

    #[test]
    fn test_chat_message_incident_resolved() {
        let payload = make_payload("incident.resolved", "Blog", "Connection refused");
        let msg = format_chat_message(&payload);
        assert!(msg.contains("🟢"));
        assert!(msg.contains("**Blog**"));
        assert!(msg.contains("Recovered"));
        assert!(msg.contains("Resolved: 2026-02-17T03:05:00Z"));
    }

    #[test]
    fn test_chat_message_degraded() {
        let payload = make_payload("monitor.degraded", "API", "Slow response: 5200ms");
        let msg = format_chat_message(&payload);
        assert!(msg.contains("🟡"));
        assert!(msg.contains("Degraded"));
        assert!(msg.contains("Slow response"));
    }

    #[test]
    fn test_chat_message_maintenance() {
        let payload = make_payload("maintenance.started", "DB Server", "");
        let msg = format_chat_message(&payload);
        assert!(msg.contains("🔧"));
        assert!(msg.contains("Maintenance started"));
    }

    #[test]
    fn test_chat_message_escalated() {
        let payload = make_payload("incident.escalated", "Payment API", "Connection timeout");
        let msg = format_chat_message(&payload);
        assert!(msg.contains("🚨"));
        assert!(msg.contains("ESCALATED"));
    }

    #[test]
    fn test_chat_message_no_incident() {
        let payload = make_payload("monitor.recovered", "CDN", "");
        let msg = format_chat_message(&payload);
        assert!(msg.contains("🟢"));
        assert!(msg.contains("**CDN**"));
        assert!(!msg.contains("Cause:"));
    }

    #[test]
    fn test_payload_format_from_config() {
        // Test that PayloadFormat parsing works for webhook channel config
        let config: serde_json::Value = serde_json::json!({"url": "https://example.com", "payload_format": "chat"});
        let fmt = match config["payload_format"].as_str() {
            Some("chat") => PayloadFormat::Chat,
            _ => PayloadFormat::Json,
        };
        assert_eq!(fmt, PayloadFormat::Chat);
    }

    #[test]
    fn test_payload_format_default_json() {
        let config: serde_json::Value = serde_json::json!({"url": "https://example.com"});
        let fmt = match config["payload_format"].as_str() {
            Some("chat") => PayloadFormat::Chat,
            _ => PayloadFormat::Json,
        };
        assert_eq!(fmt, PayloadFormat::Json);
    }
}
