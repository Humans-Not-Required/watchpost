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

/// Probe stale threshold: auto-disable locations that haven't reported in this many minutes.
/// Configurable via PROBE_STALE_MINUTES env var. Default: 30.
fn probe_stale_minutes() -> u32 {
    std::env::var("PROBE_STALE_MINUTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30)
}

/// Check for stale probe locations and auto-disable them.
/// Returns the number of locations disabled.
pub fn disable_stale_locations(db: &Db, stale_minutes: u32) -> usize {
    let conn = db.conn();
    conn.execute(
        "UPDATE check_locations SET is_active = 0 WHERE is_active = 1 AND last_seen_at IS NOT NULL AND last_seen_at < datetime('now', ?1)",
        params![format!("-{} minutes", stale_minutes)],
    )
    .unwrap_or(0)
}

/// Prune old heartbeats. Returns the number of rows deleted.
pub fn prune_heartbeats(db: &Db, days: u32) -> usize {
    let conn = db.conn();
    conn.execute(
        "DELETE FROM heartbeats WHERE checked_at < datetime('now', ?1)",
        params![format!("-{} days", days)],
    )
    .unwrap_or(0)
}

// ─── Monitor Check Model ────────────────────────────────────────────────────

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
    consensus_threshold: Option<u32>,
}

/// Result of executing a check (before incident lifecycle processing).
struct CheckResult {
    status: String,
    response_time_ms: u32,
    status_code: Option<u16>,
    error_message: Option<String>,
    /// Extra data to include in the check.completed SSE event.
    extra_sse_data: Option<serde_json::Value>,
}

// ─── Background Checker Loop ────────────────────────────────────────────────

/// Background check scheduler. Runs in a tokio task.
pub async fn run_checker(db: Arc<Db>, broadcaster: Arc<EventBroadcaster>, shutdown: rocket::Shutdown) {
    println!("🔍 Checker: starting initialization...");

    // Webhook client for consensus evaluation (shared, built once)
    let consensus_client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("❌ Checker: failed to build consensus client: {e}");
            return;
        }
    };

    // Wait 10s for server to warm up (reduced from 30s for faster first check)
    println!("🔍 Checker: waiting 10s for server warmup...");
    tokio::select! {
        _ = time::sleep(Duration::from_secs(10)) => {},
        _ = shutdown.clone() => {
            println!("🔍 Checker: shutdown during warmup");
            return;
        },
    }

    let client_follow = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .timeout(Duration::from_secs(60))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("❌ Checker: failed to build HTTP client (follow redirects): {e}");
            return;
        }
    };

    let client_no_follow = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(60))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("❌ Checker: failed to build HTTP client (no redirects): {e}");
            return;
        }
    };

    // Shared webhook client for TCP/DNS checks (which don't have their own HTTP client)
    let webhook_client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("❌ Checker: failed to build webhook client: {e}");
            return;
        }
    };

    println!("✅ Checker: initialized, entering check loop");

    // Track last retention run so we only prune once per hour
    let mut last_retention = std::time::Instant::now() - Duration::from_secs(3600);
    // Track last probe health check (every 5 minutes)
    let mut last_probe_health = std::time::Instant::now() - Duration::from_secs(300);

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

        // Run probe health check every 5 minutes
        if last_probe_health.elapsed() >= Duration::from_secs(300) {
            let stale_minutes = probe_stale_minutes();
            let disabled = disable_stale_locations(&db, stale_minutes);
            if disabled > 0 {
                println!("🔌 Probe health: auto-disabled {} stale locations (>{} min)", disabled, stale_minutes);
            }
            last_probe_health = std::time::Instant::now();
        }

        // Find the next monitor due for a check
        let monitor = {
            let conn = db.conn();
            conn.query_row(
                "SELECT id, name, url, method, timeout_ms, expected_status, body_contains, headers, confirmation_threshold, consecutive_failures, current_status, interval_seconds, response_time_threshold_ms, follow_redirects, COALESCE(monitor_type, 'http'), COALESCE(dns_record_type, 'A'), dns_expected, consensus_threshold
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
                        consensus_threshold: row.get(17)?,
                    })
                },
            ).ok()
        };

        match monitor {
            Some(m) => {
                println!("🔍 Checking: {} ({})", m.name, m.url);
                // Execute the appropriate check type
                let result = match m.monitor_type.as_str() {
                    "tcp" => execute_tcp_check(&m).await,
                    "dns" => execute_dns_check(&m).await,
                    _ => {
                        let client = if m.follow_redirects { &client_follow } else { &client_no_follow };
                        execute_http_check(client, &m).await
                    }
                };

                if m.consensus_threshold.is_some() {
                    // Consensus mode: write heartbeat + update timing, then defer to consensus
                    process_check_result_heartbeat_only(&db, &broadcaster, &m, result).await;
                    // Evaluate consensus across all locations
                    crate::consensus::evaluate_and_apply(&db, &broadcaster, &consensus_client, &m.id).await;
                } else {
                    // Single-location mode: full incident lifecycle
                    let notif_client = match m.monitor_type.as_str() {
                        "tcp" | "dns" => &webhook_client,
                        _ => if m.follow_redirects { &client_follow } else { &client_no_follow },
                    };
                    process_check_result(&db, &broadcaster, notif_client, &m, result).await;
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

// ─── Check Execution (type-specific) ────────────────────────────────────────

/// Execute an HTTP health check. Returns the raw check result.
async fn execute_http_check(client: &reqwest::Client, monitor: &MonitorCheck) -> CheckResult {
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
    let rt_threshold = monitor.response_time_threshold_ms;

    let (status, status_code, error_message) = match result {
        Ok(resp) => {
            let code = resp.status().as_u16();
            if code != monitor.expected_status {
                ("down".to_string(), Some(code), Some(format!("Expected {}, got {}", monitor.expected_status, code)))
            } else if let Some(ref expected_body) = monitor.body_contains {
                match resp.text().await {
                    Ok(body) if body.contains(expected_body) => {
                        check_rt_threshold(rt_threshold, elapsed_ms, code)
                    }
                    Ok(_) => ("down".to_string(), Some(code), Some("Body match failed".to_string())),
                    Err(e) => ("down".to_string(), Some(code), Some(format!("Body read error: {}", e))),
                }
            } else {
                check_rt_threshold(rt_threshold, elapsed_ms, code)
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

    CheckResult {
        status,
        response_time_ms: elapsed_ms,
        status_code,
        error_message,
        extra_sse_data: None,
    }
}

/// Helper: check response time against optional threshold.
fn check_rt_threshold(threshold: Option<u32>, elapsed_ms: u32, code: u16) -> (String, Option<u16>, Option<String>) {
    if let Some(t) = threshold {
        if elapsed_ms > t {
            ("degraded".to_string(), Some(code), Some(format!("Response time {}ms exceeds {}ms threshold", elapsed_ms, t)))
        } else {
            ("up".to_string(), Some(code), None)
        }
    } else {
        ("up".to_string(), Some(code), None)
    }
}

/// Execute a TCP connectivity check.
async fn execute_tcp_check(monitor: &MonitorCheck) -> CheckResult {
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

    CheckResult {
        status,
        response_time_ms: elapsed_ms,
        status_code: None,
        error_message,
        extra_sse_data: Some(serde_json::json!({"monitor_type": "tcp"})),
    }
}

/// Execute a DNS resolution check.
async fn execute_dns_check(monitor: &MonitorCheck) -> CheckResult {
    use hickory_resolver::TokioAsyncResolver;
    use hickory_resolver::config::{ResolverConfig, ResolverOpts};

    let start = std::time::Instant::now();

    // Strip optional dns:// prefix
    let hostname = monitor.url.strip_prefix("dns://").unwrap_or(&monitor.url);

    let resolver = TokioAsyncResolver::tokio(ResolverConfig::default(), ResolverOpts::default());

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
                let expected_lower = expected.to_lowercase();
                let matched = values.iter().any(|v| {
                    v.to_lowercase() == expected_lower
                        || v.to_lowercase().trim_end_matches('.') == expected_lower.trim_end_matches('.')
                });
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

    CheckResult {
        status,
        response_time_ms: elapsed_ms,
        status_code: None,
        error_message,
        extra_sse_data: Some(serde_json::json!({
            "monitor_type": "dns",
            "dns_record_type": monitor.dns_record_type,
            "resolved_values": resolved_values.unwrap_or_default(),
        })),
    }
}

/// Perform DNS lookup for a specific record type, returning resolved values as strings.
async fn dns_lookup(
    resolver: &hickory_resolver::TokioAsyncResolver,
    hostname: &str,
    record_type: &str,
) -> Result<Vec<String>, String> {
    use hickory_resolver::proto::rr::RecordType;
    use hickory_resolver::Name;

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

// ─── Heartbeat-Only Processing (for consensus-enabled monitors) ─────────────
//
// Writes the heartbeat and updates last_checked_at + consecutive_failures,
// but does NOT process incident lifecycle or update current_status.
// The consensus evaluator handles status transitions separately.

async fn process_check_result_heartbeat_only(
    db: &Db,
    broadcaster: &EventBroadcaster,
    monitor: &MonitorCheck,
    result: CheckResult,
) {
    {
        let conn = db.conn();

        // Write heartbeat
        let hb_id = uuid::Uuid::new_v4().to_string();
        let hb_seq: i64 = conn
            .query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM heartbeats", [], |r| r.get(0))
            .unwrap_or(1);
        let _ = conn.execute(
            "INSERT INTO heartbeats (id, monitor_id, status, response_time_ms, status_code, error_message, seq) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![hb_id, monitor.id, result.status, result.response_time_ms, result.status_code, result.error_message, hb_seq],
        );

        // Update consecutive failures + last_checked_at (but NOT current_status — consensus handles that)
        let new_consecutive = if result.status == "down" {
            monitor.consecutive_failures + 1
        } else {
            0
        };

        let _ = conn.execute(
            "UPDATE monitors SET last_checked_at = datetime('now'), consecutive_failures = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![new_consecutive, monitor.id],
        );
    }

    // Emit check.completed SSE event
    let mut sse_data = serde_json::json!({
        "status": result.status,
        "response_time_ms": result.response_time_ms,
    });
    if let Some(code) = result.status_code {
        sse_data["status_code"] = serde_json::json!(code);
    }
    if let Some(extra) = result.extra_sse_data {
        if let (Some(base), Some(ext)) = (sse_data.as_object_mut(), extra.as_object()) {
            for (k, v) in ext {
                base.insert(k.clone(), v.clone());
            }
        }
    }
    broadcaster.send(SseEvent {
        event_type: "check.completed".to_string(),
        monitor_id: monitor.id.clone(),
        data: sse_data,
    });
}

// ─── Shared Check Result Processing ─────────────────────────────────────────
//
// This is the single place where heartbeats, incident lifecycle, status
// transitions, and notification dispatch happen — regardless of check type.

async fn process_check_result(
    db: &Db,
    broadcaster: &EventBroadcaster,
    http_client: &reqwest::Client,
    monitor: &MonitorCheck,
    result: CheckResult,
) {
    // Check maintenance window status BEFORE acquiring DB lock
    let in_maintenance = is_in_maintenance(db, &monitor.id);

    let webhook_event: Option<WebhookPayload>;

    {
        // ── Scoped DB lock ──────────────────────────────────────────────
        let conn = db.conn();

        // Write heartbeat
        let hb_id = uuid::Uuid::new_v4().to_string();
        let hb_seq: i64 = conn
            .query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM heartbeats", [], |r| r.get(0))
            .unwrap_or(1);
        let _ = conn.execute(
            "INSERT INTO heartbeats (id, monitor_id, status, response_time_ms, status_code, error_message, seq) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![hb_id, monitor.id, result.status, result.response_time_ms, result.status_code, result.error_message, hb_seq],
        );

        // Update consecutive failures and determine effective status
        let (new_consecutive, mut effective_status) = if result.status == "down" {
            let new_count = monitor.consecutive_failures + 1;
            if new_count >= monitor.confirmation_threshold {
                (new_count, "down".to_string())
            } else {
                // Not yet confirmed — keep previous status
                (new_count, monitor.current_status.clone())
            }
        } else {
            (0, result.status.clone())
        };

        // If in maintenance window and would be "down", set to "maintenance" instead
        if in_maintenance && effective_status == "down" {
            effective_status = "maintenance".to_string();
        }

        // ── Dependency chain suppression ────────────────────────────────
        // Check if any upstream dependency is currently down.
        // If so, suppress incident creation but still record the heartbeat
        // and update status honestly.
        let deps_suppressed = if effective_status == "down" {
            crate::routes::has_dependency_down(&conn, &monitor.id)
        } else {
            false
        };

        // Persist status + failure counter
        let _ = conn.execute(
            "UPDATE monitors SET current_status = ?1, last_checked_at = datetime('now'), consecutive_failures = ?2, updated_at = datetime('now') WHERE id = ?3",
            params![effective_status, new_consecutive, monitor.id],
        );

        // ── Incident lifecycle ──────────────────────────────────────────
        let prev = &monitor.current_status;
        let now_str = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

        if deps_suppressed {
            // Dependency is down — suppress incident creation and notifications.
            // Heartbeat is already recorded; status is updated honestly.
            webhook_event = None;
        } else {
            // Check for delayed incident: monitor was previously down with
            // suppressed deps, now deps recovered but monitor is still down.
            // If there's no open incident, create one now.
            let need_delayed_incident = prev == "down" && effective_status == "down"
                && !crate::routes::has_open_incident(&conn, &monitor.id);

            if need_delayed_incident {
                // Create the incident now that dependencies are healthy
                let inc_id = uuid::Uuid::new_v4().to_string();
                let cause = result.error_message.clone()
                    .unwrap_or_else(|| "Monitor is down (dependency recovered)".to_string());
                let inc_seq: i64 = conn
                    .query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM incidents", [], |r| r.get(0))
                    .unwrap_or(1);
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
            } else {
                webhook_event = resolve_transition(
                    &conn, monitor, prev, &effective_status, &result.error_message, &now_str,
                );
            }
        }
    } // DB lock released

    // ── Fire notifications outside the lock ──────────────────────────────
    if let Some(ref payload) = webhook_event {
        // SSE broadcast
        broadcaster.send(SseEvent {
            event_type: payload.event.clone(),
            monitor_id: monitor.id.clone(),
            data: serde_json::to_value(payload).unwrap_or_default(),
        });

        // Webhooks
        let channels = notifications::get_webhook_channels(db, &monitor.id);
        if !channels.is_empty() {
            notifications::fire_webhooks(db, http_client, &monitor.id, &channels, payload).await;
        }

        // Emails
        let emails = notifications::get_email_addresses(db, &monitor.id);
        if !emails.is_empty() {
            notifications::fire_emails(&emails, payload).await;
        }

        // Log to alert_log
        log_alert(db, &monitor.id, payload, "initial");
    }

    // ── Repeat notifications (alert rules) ───────────────────────────────
    process_repeat_notifications(db, monitor, http_client, broadcaster).await;

    // Always emit check.completed SSE event
    let mut sse_data = serde_json::json!({
        "status": result.status,
        "response_time_ms": result.response_time_ms,
    });

    // Merge status_code for HTTP checks
    if let Some(code) = result.status_code {
        sse_data["status_code"] = serde_json::json!(code);
    }

    // Merge type-specific extra data
    if let Some(extra) = result.extra_sse_data {
        if let (Some(base), Some(ext)) = (sse_data.as_object_mut(), extra.as_object()) {
            for (k, v) in ext {
                base.insert(k.clone(), v.clone());
            }
        }
    }

    broadcaster.send(SseEvent {
        event_type: "check.completed".to_string(),
        monitor_id: monitor.id.clone(),
        data: sse_data,
    });
}

/// Determine which status transition occurred and produce the appropriate
/// webhook payload + DB side-effects (incident create/resolve).
///
/// Returns `None` if no notification-worthy transition happened.
fn resolve_transition(
    conn: &rusqlite::Connection,
    monitor: &MonitorCheck,
    prev: &str,
    effective: &str,
    error_message: &Option<String>,
    now_str: &str,
) -> Option<WebhookPayload> {
    let mk_monitor = |status: &str| WebhookMonitor {
        id: monitor.id.clone(),
        name: monitor.name.clone(),
        url: monitor.url.clone(),
        current_status: status.to_string(),
    };

    // Transition: → down (new incident)
    if prev != "down" && prev != "maintenance" && effective == "down" {
        let inc_id = uuid::Uuid::new_v4().to_string();
        let cause = error_message.clone().unwrap_or_else(|| "Monitor is down".to_string());
        let inc_seq: i64 = conn
            .query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM incidents", [], |r| r.get(0))
            .unwrap_or(1);
        let _ = conn.execute(
            "INSERT INTO incidents (id, monitor_id, cause, seq) VALUES (?1, ?2, ?3, ?4)",
            params![inc_id, monitor.id, cause, inc_seq],
        );
        return Some(WebhookPayload {
            event: "incident.created".to_string(),
            monitor: mk_monitor("down"),
            incident: Some(WebhookIncident {
                id: inc_id,
                cause,
                started_at: now_str.to_string(),
                resolved_at: None,
            }),
            timestamp: now_str.to_string(),
        });
    }

    // Transition: → maintenance (entering maintenance window)
    if effective == "maintenance" && prev != "maintenance" {
        return Some(WebhookPayload {
            event: "maintenance.started".to_string(),
            monitor: mk_monitor("maintenance"),
            incident: None,
            timestamp: now_str.to_string(),
        });
    }

    // Transition: → degraded
    if prev != "degraded" && effective == "degraded" {
        return Some(WebhookPayload {
            event: "monitor.degraded".to_string(),
            monitor: mk_monitor("degraded"),
            incident: None,
            timestamp: now_str.to_string(),
        });
    }

    // Transition: degraded → up (recovered from degraded)
    if prev == "degraded" && effective == "up" {
        return Some(WebhookPayload {
            event: "monitor.recovered".to_string(),
            monitor: mk_monitor("up"),
            incident: None,
            timestamp: now_str.to_string(),
        });
    }

    // Transition: maintenance → up (maintenance ended)
    if prev == "maintenance" && effective == "up" {
        return Some(WebhookPayload {
            event: "maintenance.ended".to_string(),
            monitor: mk_monitor("up"),
            incident: None,
            timestamp: now_str.to_string(),
        });
    }

    // Transition: down → recovered (resolve open incidents)
    if prev == "down" && effective != "down" && effective != "maintenance" {
        let resolved_count = conn.execute(
            "UPDATE incidents SET resolved_at = datetime('now') WHERE monitor_id = ?1 AND resolved_at IS NULL",
            params![monitor.id],
        ).unwrap_or(0);

        // Only emit resolved event if there was actually an open incident.
        // If deps were suppressed, there may be no open incident to resolve.
        if resolved_count > 0 {
            let incident_info: Option<(String, String, String)> = conn
                .query_row(
                    "SELECT id, cause, started_at FROM incidents WHERE monitor_id = ?1 ORDER BY started_at DESC LIMIT 1",
                    params![monitor.id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .ok();
            return Some(WebhookPayload {
                event: "incident.resolved".to_string(),
                monitor: mk_monitor(effective),
                incident: incident_info.map(|(id, cause, started_at)| WebhookIncident {
                    id,
                    cause,
                    started_at,
                    resolved_at: Some(now_str.to_string()),
                }),
                timestamp: now_str.to_string(),
            });
        }
        // If no open incident (was suppressed by deps), silently recover
        return None;
    }

    None
}

/// Log a notification to the alert_log table.
fn log_alert(db: &Db, monitor_id: &str, payload: &WebhookPayload, alert_type: &str) {
    let conn = db.conn();
    let id = uuid::Uuid::new_v4().to_string();
    let incident_id = payload.incident.as_ref().map(|i| i.id.clone());
    let _ = conn.execute(
        "INSERT INTO alert_log (id, monitor_id, incident_id, alert_type, event, sent_at)
         VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
        params![id, monitor_id, incident_id, alert_type, payload.event],
    );
}

/// Check if a monitor has repeat alert rules and an open incident, then fire
/// repeat notifications if enough time has passed since the last alert.
async fn process_repeat_notifications(
    db: &Db,
    monitor: &MonitorCheck,
    http_client: &reqwest::Client,
    broadcaster: &EventBroadcaster,
) {
    // Only process if monitor is currently down
    if monitor.current_status != "down" {
        return;
    }

    let (repeat_interval, max_repeats, escalation_after) = {
        let conn = db.conn();
        match conn.query_row(
            "SELECT repeat_interval_minutes, max_repeats, escalation_after_minutes
             FROM alert_rules WHERE monitor_id = ?1",
            params![monitor.id],
            |row| Ok((row.get::<_, u32>(0)?, row.get::<_, u32>(1)?, row.get::<_, u32>(2)?)),
        ) {
            Ok(r) => r,
            Err(_) => return, // No alert rules configured
        }
    };

    // Get the open incident
    let (incident_id, incident_cause, incident_started, is_acked) = {
        let conn = db.conn();
        match conn.query_row(
            "SELECT id, cause, started_at, CASE WHEN acknowledgement IS NOT NULL THEN 1 ELSE 0 END
             FROM incidents WHERE monitor_id = ?1 AND resolved_at IS NULL
             ORDER BY started_at DESC LIMIT 1",
            params![monitor.id],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i32>(3)? != 0,
            )),
        ) {
            Ok(r) => r,
            Err(_) => return, // No open incident
        }
    };

    let now_str = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // ── Repeat notifications ─────────────────────────────────────────────
    if repeat_interval > 0 {
        let (repeat_count, last_repeat_at) = {
            let conn = db.conn();
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM alert_log
                 WHERE monitor_id = ?1 AND incident_id = ?2 AND alert_type IN ('initial', 'repeat')",
                params![monitor.id, incident_id],
                |row| row.get(0),
            ).unwrap_or(0);

            let last_at: Option<String> = conn.query_row(
                "SELECT MAX(sent_at) FROM alert_log
                 WHERE monitor_id = ?1 AND incident_id = ?2 AND alert_type IN ('initial', 'repeat')",
                params![monitor.id, incident_id],
                |row| row.get(0),
            ).unwrap_or(None);

            (count as u32, last_at)
        };

        if repeat_count < max_repeats {
            let should_repeat = match last_repeat_at {
                Some(ref ts) => minutes_since(ts) >= repeat_interval as i64,
                None => true, // No prior alert logged — fire immediately
            };

            if should_repeat {
                let payload = WebhookPayload {
                    event: "incident.reminder".to_string(),
                    monitor: WebhookMonitor {
                        id: monitor.id.clone(),
                        name: monitor.name.clone(),
                        url: monitor.url.clone(),
                        current_status: "down".to_string(),
                    },
                    incident: Some(WebhookIncident {
                        id: incident_id.clone(),
                        cause: incident_cause.clone(),
                        started_at: incident_started.clone(),
                        resolved_at: None,
                    }),
                    timestamp: now_str.clone(),
                };

                // SSE
                broadcaster.send(SseEvent {
                    event_type: "incident.reminder".to_string(),
                    monitor_id: monitor.id.clone(),
                    data: serde_json::to_value(&payload).unwrap_or_default(),
                });

                // Webhooks
                let channels = notifications::get_webhook_channels(db, &monitor.id);
                if !channels.is_empty() {
                    notifications::fire_webhooks(db, http_client, &monitor.id, &channels, &payload).await;
                }

                // Emails
                let emails = notifications::get_email_addresses(db, &monitor.id);
                if !emails.is_empty() {
                    notifications::fire_emails(&emails, &payload).await;
                }

                log_alert(db, &monitor.id, &payload, "repeat");
            }
        }
    }

    // ── Escalation ───────────────────────────────────────────────────────
    if escalation_after > 0 && !is_acked {
        // Check if incident has been open longer than escalation_after minutes
        let incident_age_minutes = minutes_since(&incident_started);
        if incident_age_minutes >= escalation_after as i64 {
            // Check if we already sent an escalation for this incident
            let already_escalated = {
                let conn = db.conn();
                let count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM alert_log
                     WHERE monitor_id = ?1 AND incident_id = ?2 AND alert_type = 'escalation'",
                    params![monitor.id, incident_id],
                    |row| row.get(0),
                ).unwrap_or(0);
                count > 0
            };

            if !already_escalated {
                let payload = WebhookPayload {
                    event: "incident.escalated".to_string(),
                    monitor: WebhookMonitor {
                        id: monitor.id.clone(),
                        name: monitor.name.clone(),
                        url: monitor.url.clone(),
                        current_status: "down".to_string(),
                    },
                    incident: Some(WebhookIncident {
                        id: incident_id.clone(),
                        cause: incident_cause.clone(),
                        started_at: incident_started.clone(),
                        resolved_at: None,
                    }),
                    timestamp: now_str.clone(),
                };

                // SSE
                broadcaster.send(SseEvent {
                    event_type: "incident.escalated".to_string(),
                    monitor_id: monitor.id.clone(),
                    data: serde_json::to_value(&payload).unwrap_or_default(),
                });

                // Fire to ALL channels (escalation = notify everything)
                let channels = notifications::get_webhook_channels(db, &monitor.id);
                if !channels.is_empty() {
                    notifications::fire_webhooks(db, http_client, &monitor.id, &channels, &payload).await;
                }
                let emails = notifications::get_email_addresses(db, &monitor.id);
                if !emails.is_empty() {
                    notifications::fire_emails(&emails, &payload).await;
                }

                log_alert(db, &monitor.id, &payload, "escalation");
            }
        }
    }
}

/// Calculate minutes elapsed since a datetime string.
fn minutes_since(datetime_str: &str) -> i64 {
    use chrono::{NaiveDateTime, Utc};
    let formats = ["%Y-%m-%dT%H:%M:%SZ", "%Y-%m-%d %H:%M:%S"];
    for fmt in &formats {
        if let Ok(dt) = NaiveDateTime::parse_from_str(datetime_str, fmt) {
            let then = dt.and_utc();
            return (Utc::now() - then).num_minutes();
        }
    }
    0 // fallback: treat as "just now"
}
