use crate::db::Db;
use crate::notifications::{self, WebhookPayload, WebhookMonitor, WebhookIncident};
use crate::routes::is_in_maintenance;
use crate::sse::{EventBroadcaster, SseEvent};
use rusqlite::params;
use std::sync::Arc;
use std::time::Duration;
use tokio::time;

/// Heartbeat retention: delete heartbeats older than this many days.
/// Configurable via HEARTBEAT_RETENTION_DAYS env var. Default: 90.
fn retention_days() -> u32 {
    std::env::var("HEARTBEAT_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(90)
}

/// Prune old heartbeats. Returns the number of rows deleted.
pub fn prune_heartbeats(db: &Db, days: u32) -> usize {
    let conn = db.conn.lock().unwrap();
    conn.execute(
        "DELETE FROM heartbeats WHERE checked_at < datetime('now', ?1)",
        params![format!("-{} days", days)],
    )
    .unwrap_or(0)
}

/// Background check scheduler. Runs in a tokio task.
pub async fn run_checker(db: Arc<Db>, broadcaster: Arc<EventBroadcaster>, shutdown: rocket::Shutdown) {
    // Wait 30s for server to warm up
    tokio::select! {
        _ = time::sleep(Duration::from_secs(30)) => {},
        _ = shutdown.clone() => return,
    }

    let client_follow = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .timeout(Duration::from_secs(60))
        .build()
        .expect("Failed to build HTTP client (follow redirects)");

    let client_no_follow = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(60))
        .build()
        .expect("Failed to build HTTP client (no redirects)");

    // Track last retention run so we only prune once per hour
    let mut last_retention = std::time::Instant::now() - Duration::from_secs(3600);

    loop {
        // Run heartbeat retention every hour
        if last_retention.elapsed() >= Duration::from_secs(3600) {
            let days = retention_days();
            let deleted = prune_heartbeats(&db, days);
            if deleted > 0 {
                println!("🗑️  Retention: pruned {} heartbeats older than {} days", deleted, days);
            }
            last_retention = std::time::Instant::now();
        }
        // Find the next monitor due for a check
        let monitor = {
            let conn = db.conn.lock().unwrap();
            conn.query_row(
                "SELECT id, name, url, method, timeout_ms, expected_status, body_contains, headers, confirmation_threshold, consecutive_failures, current_status, interval_seconds, response_time_threshold_ms, follow_redirects, COALESCE(monitor_type, 'http'), COALESCE(dns_record_type, 'A'), dns_expected
                 FROM monitors
                 WHERE is_paused = 0
                   AND (last_checked_at IS NULL OR datetime(last_checked_at, '+' || interval_seconds || ' seconds') <= datetime('now'))
                 ORDER BY last_checked_at ASC NULLS FIRST
                 LIMIT 1",
                [],
                |row| {
                    let headers_str: Option<String> = row.get(7)?;
                    Ok(MonitorCheck {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        url: row.get(2)?,
                        method: row.get(3)?,
                        timeout_ms: row.get(4)?,
                        expected_status: row.get(5)?,
                        body_contains: row.get(6)?,
                        headers: headers_str,
                        confirmation_threshold: row.get(8)?,
                        consecutive_failures: row.get(9)?,
                        current_status: row.get(10)?,
                        interval_seconds: row.get(11)?,
                        response_time_threshold_ms: row.get(12)?,
                        follow_redirects: row.get::<_, i32>(13).unwrap_or(1) != 0,
                        monitor_type: row.get(14)?,
                        dns_record_type: row.get(15)?,
                        dns_expected: row.get(16)?,
                    })
                },
            ).ok()
        };

        match monitor {
            Some(m) => {
                if m.monitor_type == "tcp" {
                    run_tcp_check(&db, &broadcaster, &m).await;
                } else if m.monitor_type == "dns" {
                    run_dns_check(&db, &broadcaster, &m).await;
                } else {
                    let client = if m.follow_redirects { &client_follow } else { &client_no_follow };
                    run_check(client, &db, &broadcaster, &m).await;
                }
            }
            None => {
                // No monitors due — sleep a bit before checking again
                tokio::select! {
                    _ = time::sleep(Duration::from_secs(10)) => {},
                    _ = shutdown.clone() => return,
                }
            }
        }

        // Brief yield between checks
        tokio::select! {
            _ = time::sleep(Duration::from_millis(100)) => {},
            _ = shutdown.clone() => return,
        }
    }
}

struct MonitorCheck {
    id: String,
    name: String,
    url: String,
    method: String,
    timeout_ms: u32,
    expected_status: u16,
    body_contains: Option<String>,
    headers: Option<String>,
    confirmation_threshold: u32,
    consecutive_failures: u32,
    current_status: String,
    #[allow(dead_code)]
    interval_seconds: u32,
    response_time_threshold_ms: Option<u32>,
    follow_redirects: bool,
    monitor_type: String,
    dns_record_type: String,
    dns_expected: Option<String>,
}

async fn run_check(client: &reqwest::Client, db: &Db, broadcaster: &EventBroadcaster, monitor: &MonitorCheck) {
    let start = std::time::Instant::now();

    // Build request
    let mut req = match monitor.method.as_str() {
        "HEAD" => client.head(&monitor.url),
        "POST" => client.post(&monitor.url),
        _ => client.get(&monitor.url),
    };

    req = req.timeout(Duration::from_millis(monitor.timeout_ms as u64));

    // Add custom headers
    if let Some(ref headers_json) = monitor.headers {
        if let Ok(headers) = serde_json::from_str::<serde_json::Value>(headers_json) {
            if let Some(obj) = headers.as_object() {
                for (k, v) in obj {
                    if let Some(val) = v.as_str() {
                        req = req.header(k.as_str(), val);
                    }
                }
            }
        }
    }

    // Execute
    let result = req.send().await;
    let elapsed_ms = start.elapsed().as_millis() as u32;

    // Use per-monitor response time threshold if configured, otherwise no degraded-by-latency
    let rt_threshold = monitor.response_time_threshold_ms;

    let (status, status_code, error_message) = match result {
        Ok(resp) => {
            let code = resp.status().as_u16();
            if code != monitor.expected_status {
                ("down".to_string(), Some(code), Some(format!("Expected {}, got {}", monitor.expected_status, code)))
            } else if let Some(ref expected_body) = monitor.body_contains {
                match resp.text().await {
                    Ok(body) if body.contains(expected_body) => {
                        if let Some(threshold) = rt_threshold {
                            if elapsed_ms > threshold {
                                ("degraded".to_string(), Some(code), Some(format!("Response time {}ms exceeds {}ms threshold", elapsed_ms, threshold)))
                            } else {
                                ("up".to_string(), Some(code), None)
                            }
                        } else {
                            ("up".to_string(), Some(code), None)
                        }
                    }
                    Ok(_) => ("down".to_string(), Some(code), Some("Body match failed".to_string())),
                    Err(e) => ("down".to_string(), Some(code), Some(format!("Body read error: {}", e))),
                }
            } else if let Some(threshold) = rt_threshold {
                if elapsed_ms > threshold {
                    ("degraded".to_string(), Some(code), Some(format!("Response time {}ms exceeds {}ms threshold", elapsed_ms, threshold)))
                } else {
                    ("up".to_string(), Some(code), None)
                }
            } else {
                ("up".to_string(), Some(code), None)
            }
        }
        Err(e) => {
            let msg = if e.is_timeout() {
                "Request timed out".to_string()
            } else if e.is_connect() {
                "Connection refused".to_string()
            } else {
                format!("Request failed: {}", e)
            };
            ("down".to_string(), None, Some(msg))
        }
    };

    // Check maintenance window status BEFORE acquiring DB lock (is_in_maintenance acquires its own)
    let in_maintenance = is_in_maintenance(db, &monitor.id);

    // Collect webhook data while holding DB lock, then release before firing
    let webhook_event: Option<WebhookPayload>;

    {
        // Scoped DB lock
        let conn = db.conn.lock().unwrap();
        let hb_id = uuid::Uuid::new_v4().to_string();
        let hb_seq: i64 = conn.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM heartbeats", [], |r| r.get(0)).unwrap_or(1);
        let _ = conn.execute(
            "INSERT INTO heartbeats (id, monitor_id, status, response_time_ms, status_code, error_message, seq) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![hb_id, monitor.id, status, elapsed_ms, status_code, error_message, hb_seq],
        );

        // Update consecutive failures and determine status transition
        let (new_consecutive, mut effective_status) = if status == "down" {
            let new_count = monitor.consecutive_failures + 1;
            if new_count >= monitor.confirmation_threshold {
                (new_count, "down".to_string())
            } else {
                (new_count, monitor.current_status.clone())
            }
        } else {
            (0, status.clone())
        };

        // If in maintenance window and would be "down", set status to "maintenance" instead
        if in_maintenance && effective_status == "down" {
            effective_status = "maintenance".to_string();
        }

        let _ = conn.execute(
            "UPDATE monitors SET current_status = ?1, last_checked_at = datetime('now'), consecutive_failures = ?2, updated_at = datetime('now') WHERE id = ?3",
            params![effective_status, new_consecutive, monitor.id],
        );

        // Handle incident lifecycle
        let prev_status = &monitor.current_status;
        let now_str = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

        if prev_status != "down" && prev_status != "maintenance" && effective_status == "down" {
            // Transition to down (not in maintenance) — create incident
            let inc_id = uuid::Uuid::new_v4().to_string();
            let cause = error_message.clone().unwrap_or_else(|| "Monitor is down".to_string());
            let inc_seq: i64 = conn.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM incidents", [], |r| r.get(0)).unwrap_or(1);
            let _ = conn.execute(
                "INSERT INTO incidents (id, monitor_id, cause, seq) VALUES (?1, ?2, ?3, ?4)",
                params![inc_id, monitor.id, cause, inc_seq],
            );

            webhook_event = Some(WebhookPayload {
                event: "incident.created".to_string(),
                monitor: WebhookMonitor {
                    id: monitor.id.clone(),
                    name: monitor.name.clone(),
                    url: monitor.url.clone(),
                    current_status: "down".to_string(),
                },
                incident: Some(WebhookIncident {
                    id: inc_id,
                    cause,
                    started_at: now_str.clone(),
                    resolved_at: None,
                }),
                timestamp: now_str,
            });
        } else if effective_status == "maintenance" && prev_status != "maintenance" {
            // Entering maintenance — no incident, but emit SSE event
            webhook_event = Some(WebhookPayload {
                event: "maintenance.started".to_string(),
                monitor: WebhookMonitor {
                    id: monitor.id.clone(),
                    name: monitor.name.clone(),
                    url: monitor.url.clone(),
                    current_status: "maintenance".to_string(),
                },
                incident: None,
                timestamp: now_str,
            });
        } else if prev_status != "degraded" && effective_status == "degraded" {
            // Transition to degraded — notify but don't create incident
            webhook_event = Some(WebhookPayload {
                event: "monitor.degraded".to_string(),
                monitor: WebhookMonitor {
                    id: monitor.id.clone(),
                    name: monitor.name.clone(),
                    url: monitor.url.clone(),
                    current_status: "degraded".to_string(),
                },
                incident: None,
                timestamp: now_str,
            });
        } else if prev_status == "degraded" && effective_status == "up" {
            // Recovered from degraded — notify
            webhook_event = Some(WebhookPayload {
                event: "monitor.recovered".to_string(),
                monitor: WebhookMonitor {
                    id: monitor.id.clone(),
                    name: monitor.name.clone(),
                    url: monitor.url.clone(),
                    current_status: "up".to_string(),
                },
                incident: None,
                timestamp: now_str,
            });
        } else if prev_status == "maintenance" && effective_status == "up" {
            // Recovered from maintenance — no incidents to resolve, just notify
            webhook_event = Some(WebhookPayload {
                event: "maintenance.ended".to_string(),
                monitor: WebhookMonitor {
                    id: monitor.id.clone(),
                    name: monitor.name.clone(),
                    url: monitor.url.clone(),
                    current_status: "up".to_string(),
                },
                incident: None,
                timestamp: now_str,
            });
        } else if prev_status == "down" && effective_status != "down" && effective_status != "maintenance" {
            // Transition from down — resolve open incidents
            let _ = conn.execute(
                "UPDATE incidents SET resolved_at = datetime('now') WHERE monitor_id = ?1 AND resolved_at IS NULL",
                params![monitor.id],
            );

            // Get the most recently resolved incident for the payload
            let incident_info: Option<(String, String, String)> = conn.query_row(
                "SELECT id, cause, started_at FROM incidents WHERE monitor_id = ?1 ORDER BY started_at DESC LIMIT 1",
                params![monitor.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ).ok();

            webhook_event = Some(WebhookPayload {
                event: "incident.resolved".to_string(),
                monitor: WebhookMonitor {
                    id: monitor.id.clone(),
                    name: monitor.name.clone(),
                    url: monitor.url.clone(),
                    current_status: effective_status.clone(),
                },
                incident: incident_info.map(|(id, cause, started_at)| WebhookIncident {
                    id,
                    cause,
                    started_at,
                    resolved_at: Some(now_str.clone()),
                }),
                timestamp: now_str,
            });
        } else {
            webhook_event = None;
        }
    } // DB lock released here

    // Fire webhooks and SSE events outside the DB lock
    if let Some(ref payload) = webhook_event {
        // SSE broadcast
        broadcaster.send(SseEvent {
            event_type: payload.event.clone(),
            monitor_id: monitor.id.clone(),
            data: serde_json::to_value(payload).unwrap_or_default(),
        });

        // Webhooks
        let urls = notifications::get_webhook_urls(db, &monitor.id);
        if !urls.is_empty() {
            notifications::fire_webhooks(client, &urls, payload).await;
        }

        // Emails
        let emails = notifications::get_email_addresses(db, &monitor.id);
        if !emails.is_empty() {
            notifications::fire_emails(&emails, payload).await;
        }
    }

    // Always emit check.completed SSE event
    broadcaster.send(SseEvent {
        event_type: "check.completed".to_string(),
        monitor_id: monitor.id.clone(),
        data: serde_json::json!({
            "status": status,
            "response_time_ms": elapsed_ms,
            "status_code": status_code,
        }),
    });
}

/// Run a TCP connectivity check (connect to host:port, measure latency)
async fn run_tcp_check(db: &Db, broadcaster: &EventBroadcaster, monitor: &MonitorCheck) {
    use tokio::net::TcpStream;

    let start = std::time::Instant::now();

    // Parse host:port, stripping optional tcp:// prefix
    let addr_str = monitor.url.strip_prefix("tcp://").unwrap_or(&monitor.url);

    let result = tokio::time::timeout(
        Duration::from_millis(monitor.timeout_ms as u64),
        TcpStream::connect(addr_str),
    ).await;

    let elapsed_ms = start.elapsed().as_millis() as u32;

    let rt_threshold = monitor.response_time_threshold_ms;

    let (status, error_message) = match result {
        Ok(Ok(_stream)) => {
            // Connection successful — check response time threshold
            if let Some(threshold) = rt_threshold {
                if elapsed_ms > threshold {
                    ("degraded".to_string(), Some(format!("TCP connect time {}ms exceeds {}ms threshold", elapsed_ms, threshold)))
                } else {
                    ("up".to_string(), None)
                }
            } else {
                ("up".to_string(), None)
            }
        }
        Ok(Err(e)) => {
            let msg = if e.kind() == std::io::ErrorKind::ConnectionRefused {
                "Connection refused".to_string()
            } else {
                format!("TCP connect failed: {}", e)
            };
            ("down".to_string(), Some(msg))
        }
        Err(_) => {
            ("down".to_string(), Some("TCP connect timed out".to_string()))
        }
    };

    // Check maintenance window status BEFORE acquiring DB lock
    let in_maintenance = is_in_maintenance(db, &monitor.id);

    // Reuse the same incident lifecycle logic as HTTP checks
    let webhook_event: Option<WebhookPayload>;

    {
        let conn = db.conn.lock().unwrap();
        let hb_id = uuid::Uuid::new_v4().to_string();
        let hb_seq: i64 = conn.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM heartbeats", [], |r| r.get(0)).unwrap_or(1);
        let _ = conn.execute(
            "INSERT INTO heartbeats (id, monitor_id, status, response_time_ms, status_code, error_message, seq) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![hb_id, monitor.id, status, elapsed_ms, Option::<u16>::None, error_message, hb_seq],
        );

        let (new_consecutive, mut effective_status) = if status == "down" {
            let new_count = monitor.consecutive_failures + 1;
            if new_count >= monitor.confirmation_threshold {
                (new_count, "down".to_string())
            } else {
                (new_count, monitor.current_status.clone())
            }
        } else {
            (0, status.clone())
        };

        if in_maintenance && effective_status == "down" {
            effective_status = "maintenance".to_string();
        }

        let _ = conn.execute(
            "UPDATE monitors SET current_status = ?1, last_checked_at = datetime('now'), consecutive_failures = ?2, updated_at = datetime('now') WHERE id = ?3",
            params![effective_status, new_consecutive, monitor.id],
        );

        let prev_status = &monitor.current_status;
        let now_str = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

        if prev_status != "down" && prev_status != "maintenance" && effective_status == "down" {
            let inc_id = uuid::Uuid::new_v4().to_string();
            let cause = error_message.clone().unwrap_or_else(|| "Monitor is down".to_string());
            let inc_seq: i64 = conn.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM incidents", [], |r| r.get(0)).unwrap_or(1);
            let _ = conn.execute(
                "INSERT INTO incidents (id, monitor_id, cause, seq) VALUES (?1, ?2, ?3, ?4)",
                params![inc_id, monitor.id, cause, inc_seq],
            );
            webhook_event = Some(WebhookPayload {
                event: "incident.created".to_string(),
                monitor: WebhookMonitor { id: monitor.id.clone(), name: monitor.name.clone(), url: monitor.url.clone(), current_status: "down".to_string() },
                incident: Some(WebhookIncident { id: inc_id, cause, started_at: now_str.clone(), resolved_at: None }),
                timestamp: now_str,
            });
        } else if effective_status == "maintenance" && prev_status != "maintenance" {
            webhook_event = Some(WebhookPayload {
                event: "maintenance.started".to_string(),
                monitor: WebhookMonitor { id: monitor.id.clone(), name: monitor.name.clone(), url: monitor.url.clone(), current_status: "maintenance".to_string() },
                incident: None,
                timestamp: now_str,
            });
        } else if prev_status != "degraded" && effective_status == "degraded" {
            webhook_event = Some(WebhookPayload {
                event: "monitor.degraded".to_string(),
                monitor: WebhookMonitor { id: monitor.id.clone(), name: monitor.name.clone(), url: monitor.url.clone(), current_status: "degraded".to_string() },
                incident: None,
                timestamp: now_str,
            });
        } else if prev_status == "degraded" && effective_status == "up" {
            webhook_event = Some(WebhookPayload {
                event: "monitor.recovered".to_string(),
                monitor: WebhookMonitor { id: monitor.id.clone(), name: monitor.name.clone(), url: monitor.url.clone(), current_status: "up".to_string() },
                incident: None,
                timestamp: now_str,
            });
        } else if prev_status == "maintenance" && effective_status == "up" {
            webhook_event = Some(WebhookPayload {
                event: "maintenance.ended".to_string(),
                monitor: WebhookMonitor { id: monitor.id.clone(), name: monitor.name.clone(), url: monitor.url.clone(), current_status: "up".to_string() },
                incident: None,
                timestamp: now_str,
            });
        } else if prev_status == "down" && effective_status != "down" && effective_status != "maintenance" {
            let _ = conn.execute(
                "UPDATE incidents SET resolved_at = datetime('now') WHERE monitor_id = ?1 AND resolved_at IS NULL",
                params![monitor.id],
            );
            let incident_info: Option<(String, String, String)> = conn.query_row(
                "SELECT id, cause, started_at FROM incidents WHERE monitor_id = ?1 ORDER BY started_at DESC LIMIT 1",
                params![monitor.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ).ok();
            webhook_event = Some(WebhookPayload {
                event: "incident.resolved".to_string(),
                monitor: WebhookMonitor { id: monitor.id.clone(), name: monitor.name.clone(), url: monitor.url.clone(), current_status: effective_status.clone() },
                incident: incident_info.map(|(id, cause, started_at)| WebhookIncident { id, cause, started_at, resolved_at: Some(now_str.clone()) }),
                timestamp: now_str,
            });
        } else {
            webhook_event = None;
        }
    }

    // Fire webhooks and SSE events outside the DB lock
    if let Some(ref payload) = webhook_event {
        broadcaster.send(SseEvent {
            event_type: payload.event.clone(),
            monitor_id: monitor.id.clone(),
            data: serde_json::to_value(payload).unwrap_or_default(),
        });

        // TCP checks don't have an HTTP client, create one for webhooks if needed
        let urls = notifications::get_webhook_urls(db, &monitor.id);
        if !urls.is_empty() {
            let wh_client = reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap();
            notifications::fire_webhooks(&wh_client, &urls, payload).await;
        }

        let emails = notifications::get_email_addresses(db, &monitor.id);
        if !emails.is_empty() {
            notifications::fire_emails(&emails, payload).await;
        }
    }

    // Always emit check.completed SSE event (no status_code for TCP)
    broadcaster.send(SseEvent {
        event_type: "check.completed".to_string(),
        monitor_id: monitor.id.clone(),
        data: serde_json::json!({
            "monitor_type": "tcp",
            "status": status,
            "response_time_ms": elapsed_ms,
        }),
    });
}

/// Run a DNS resolution check
async fn run_dns_check(db: &Db, broadcaster: &EventBroadcaster, monitor: &MonitorCheck) {
    use trust_dns_resolver::TokioAsyncResolver;
    use trust_dns_resolver::config::{ResolverConfig, ResolverOpts};

    let start = std::time::Instant::now();

    // Strip optional dns:// prefix
    let hostname = monitor.url.strip_prefix("dns://").unwrap_or(&monitor.url);

    // Build resolver with default config (Google DNS)
    let resolver = TokioAsyncResolver::tokio(ResolverConfig::default(), ResolverOpts::default());

    // Perform the lookup based on record type
    let record_type = monitor.dns_record_type.to_uppercase();
    let result = tokio::time::timeout(
        Duration::from_millis(monitor.timeout_ms as u64),
        dns_lookup(&resolver, hostname, &record_type),
    ).await;

    let elapsed_ms = start.elapsed().as_millis() as u32;
    let rt_threshold = monitor.response_time_threshold_ms;

    let (status, error_message, resolved_values) = match result {
        Ok(Ok(values)) => {
            if values.is_empty() {
                ("down".to_string(), Some(format!("No {} records found for {}", record_type, hostname)), None)
            } else if let Some(ref expected) = monitor.dns_expected {
                // Check if any resolved value matches the expected value (case-insensitive)
                let expected_lower = expected.to_lowercase();
                let matched = values.iter().any(|v| v.to_lowercase() == expected_lower || v.to_lowercase().trim_end_matches('.') == expected_lower.trim_end_matches('.'));
                if matched {
                    if let Some(threshold) = rt_threshold {
                        if elapsed_ms > threshold {
                            ("degraded".to_string(), Some(format!("DNS resolution time {}ms exceeds {}ms threshold", elapsed_ms, threshold)), Some(values))
                        } else {
                            ("up".to_string(), None, Some(values))
                        }
                    } else {
                        ("up".to_string(), None, Some(values))
                    }
                } else {
                    ("down".to_string(), Some(format!("Expected '{}', got: {}", expected, values.join(", "))), Some(values))
                }
            } else {
                // No expected value — just check that resolution succeeds
                if let Some(threshold) = rt_threshold {
                    if elapsed_ms > threshold {
                        ("degraded".to_string(), Some(format!("DNS resolution time {}ms exceeds {}ms threshold", elapsed_ms, threshold)), Some(values))
                    } else {
                        ("up".to_string(), None, Some(values))
                    }
                } else {
                    ("up".to_string(), None, Some(values))
                }
            }
        }
        Ok(Err(e)) => {
            ("down".to_string(), Some(format!("DNS lookup failed: {}", e)), None)
        }
        Err(_) => {
            ("down".to_string(), Some("DNS lookup timed out".to_string()), None)
        }
    };

    record_dns_result(db, broadcaster, monitor, &status, elapsed_ms, error_message, resolved_values.as_deref()).await;
}

/// Perform DNS lookup for a specific record type, returning resolved values as strings
async fn dns_lookup(
    resolver: &trust_dns_resolver::TokioAsyncResolver,
    hostname: &str,
    record_type: &str,
) -> Result<Vec<String>, String> {
    use trust_dns_resolver::proto::rr::RecordType;
    use trust_dns_resolver::Name;

    let name = Name::from_ascii(hostname).map_err(|e| format!("Invalid hostname: {}", e))?;

    match record_type {
        "A" => {
            let response = resolver.ipv4_lookup(name.clone()).await.map_err(|e| e.to_string())?;
            Ok(response.iter().map(|ip| ip.to_string()).collect())
        }
        "AAAA" => {
            let response = resolver.ipv6_lookup(name.clone()).await.map_err(|e| e.to_string())?;
            Ok(response.iter().map(|ip| ip.to_string()).collect())
        }
        "MX" => {
            let response = resolver.mx_lookup(name.clone()).await.map_err(|e| e.to_string())?;
            Ok(response.iter().map(|mx| format!("{} {}", mx.preference(), mx.exchange())).collect())
        }
        "TXT" => {
            let response = resolver.txt_lookup(name.clone()).await.map_err(|e| e.to_string())?;
            Ok(response.iter().map(|txt| txt.to_string()).collect())
        }
        "NS" => {
            let response = resolver.ns_lookup(name.clone()).await.map_err(|e| e.to_string())?;
            Ok(response.iter().map(|ns| ns.to_string()).collect())
        }
        "SOA" => {
            let response = resolver.soa_lookup(name.clone()).await.map_err(|e| e.to_string())?;
            Ok(response.iter().map(|soa| format!("{} {} {} {} {} {} {}", soa.mname(), soa.rname(), soa.serial(), soa.refresh(), soa.retry(), soa.expire(), soa.minimum())).collect())
        }
        "CNAME" | "PTR" | "SRV" | "CAA" => {
            // Use generic lookup for these types
            let rtype = match record_type {
                "CNAME" => RecordType::CNAME,
                "PTR" => RecordType::PTR,
                "SRV" => RecordType::SRV,
                "CAA" => RecordType::CAA,
                _ => unreachable!(),
            };
            let response = resolver.lookup(name, rtype).await.map_err(|e| e.to_string())?;
            Ok(response.iter().map(|r| r.to_string()).collect())
        }
        _ => Err(format!("Unsupported record type: {}", record_type)),
    }
}

/// Record DNS check result — same incident lifecycle logic as HTTP/TCP
async fn record_dns_result(
    db: &Db,
    broadcaster: &EventBroadcaster,
    monitor: &MonitorCheck,
    status: &str,
    elapsed_ms: u32,
    error_message: Option<String>,
    resolved_values: Option<&[String]>,
) {
    let in_maintenance = crate::routes::is_in_maintenance(db, &monitor.id);
    let webhook_event: Option<WebhookPayload>;

    {
        let conn = db.conn.lock().unwrap();
        let hb_id = uuid::Uuid::new_v4().to_string();
        let hb_seq: i64 = conn.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM heartbeats", [], |r| r.get(0)).unwrap_or(1);
        let _ = conn.execute(
            "INSERT INTO heartbeats (id, monitor_id, status, response_time_ms, status_code, error_message, seq) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![hb_id, monitor.id, status, elapsed_ms, Option::<u16>::None, error_message, hb_seq],
        );

        let (new_consecutive, mut effective_status) = if status == "down" {
            let new_count = monitor.consecutive_failures + 1;
            if new_count >= monitor.confirmation_threshold {
                (new_count, "down".to_string())
            } else {
                (new_count, monitor.current_status.clone())
            }
        } else {
            (0, status.to_string())
        };

        if in_maintenance && effective_status == "down" {
            effective_status = "maintenance".to_string();
        }

        let _ = conn.execute(
            "UPDATE monitors SET current_status = ?1, last_checked_at = datetime('now'), consecutive_failures = ?2, updated_at = datetime('now') WHERE id = ?3",
            params![effective_status, new_consecutive, monitor.id],
        );

        let prev_status = &monitor.current_status;
        let now_str = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

        if prev_status != "down" && prev_status != "maintenance" && effective_status == "down" {
            let inc_id = uuid::Uuid::new_v4().to_string();
            let cause = error_message.clone().unwrap_or_else(|| "DNS check failed".to_string());
            let inc_seq: i64 = conn.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM incidents", [], |r| r.get(0)).unwrap_or(1);
            let _ = conn.execute(
                "INSERT INTO incidents (id, monitor_id, cause, seq) VALUES (?1, ?2, ?3, ?4)",
                params![inc_id, monitor.id, cause, inc_seq],
            );
            webhook_event = Some(WebhookPayload {
                event: "incident.created".to_string(),
                monitor: WebhookMonitor { id: monitor.id.clone(), name: monitor.name.clone(), url: monitor.url.clone(), current_status: "down".to_string() },
                incident: Some(WebhookIncident { id: inc_id, cause, started_at: now_str.clone(), resolved_at: None }),
                timestamp: now_str,
            });
        } else if effective_status == "maintenance" && prev_status != "maintenance" {
            webhook_event = Some(WebhookPayload {
                event: "maintenance.started".to_string(),
                monitor: WebhookMonitor { id: monitor.id.clone(), name: monitor.name.clone(), url: monitor.url.clone(), current_status: "maintenance".to_string() },
                incident: None,
                timestamp: now_str,
            });
        } else if prev_status != "degraded" && effective_status == "degraded" {
            webhook_event = Some(WebhookPayload {
                event: "monitor.degraded".to_string(),
                monitor: WebhookMonitor { id: monitor.id.clone(), name: monitor.name.clone(), url: monitor.url.clone(), current_status: "degraded".to_string() },
                incident: None,
                timestamp: now_str,
            });
        } else if prev_status == "degraded" && effective_status == "up" {
            webhook_event = Some(WebhookPayload {
                event: "monitor.recovered".to_string(),
                monitor: WebhookMonitor { id: monitor.id.clone(), name: monitor.name.clone(), url: monitor.url.clone(), current_status: "up".to_string() },
                incident: None,
                timestamp: now_str,
            });
        } else if prev_status == "maintenance" && effective_status == "up" {
            webhook_event = Some(WebhookPayload {
                event: "maintenance.ended".to_string(),
                monitor: WebhookMonitor { id: monitor.id.clone(), name: monitor.name.clone(), url: monitor.url.clone(), current_status: "up".to_string() },
                incident: None,
                timestamp: now_str,
            });
        } else if prev_status == "down" && effective_status != "down" && effective_status != "maintenance" {
            let _ = conn.execute(
                "UPDATE incidents SET resolved_at = datetime('now') WHERE monitor_id = ?1 AND resolved_at IS NULL",
                params![monitor.id],
            );
            let incident_info: Option<(String, String, String)> = conn.query_row(
                "SELECT id, cause, started_at FROM incidents WHERE monitor_id = ?1 ORDER BY started_at DESC LIMIT 1",
                params![monitor.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ).ok();
            webhook_event = Some(WebhookPayload {
                event: "incident.resolved".to_string(),
                monitor: WebhookMonitor { id: monitor.id.clone(), name: monitor.name.clone(), url: monitor.url.clone(), current_status: effective_status.clone() },
                incident: incident_info.map(|(id, cause, started_at)| WebhookIncident { id, cause, started_at, resolved_at: Some(now_str.clone()) }),
                timestamp: now_str,
            });
        } else {
            webhook_event = None;
        }
    }

    // Fire webhooks and SSE events outside the DB lock
    if let Some(ref payload) = webhook_event {
        broadcaster.send(SseEvent {
            event_type: payload.event.clone(),
            monitor_id: monitor.id.clone(),
            data: serde_json::to_value(payload).unwrap_or_default(),
        });

        let urls = notifications::get_webhook_urls(db, &monitor.id);
        if !urls.is_empty() {
            let wh_client = reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap();
            notifications::fire_webhooks(&wh_client, &urls, payload).await;
        }

        let emails = notifications::get_email_addresses(db, &monitor.id);
        if !emails.is_empty() {
            notifications::fire_emails(&emails, payload).await;
        }
    }

    // Always emit check.completed SSE event
    broadcaster.send(SseEvent {
        event_type: "check.completed".to_string(),
        monitor_id: monitor.id.clone(),
        data: serde_json::json!({
            "monitor_type": "dns",
            "status": status,
            "response_time_ms": elapsed_ms,
            "dns_record_type": monitor.dns_record_type,
            "resolved_values": resolved_values.unwrap_or(&[]),
        }),
    });
}
