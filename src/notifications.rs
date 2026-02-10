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
