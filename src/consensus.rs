use crate::db::Db;
use crate::notifications::{self, WebhookPayload, WebhookMonitor, WebhookIncident};
use crate::sse::{EventBroadcaster, SseEvent};
use rusqlite::params;
use std::sync::Arc;

/// Per-location latest heartbeat data used for consensus evaluation.
struct LocationHeartbeat {
    location_id: Option<String>,
    status: String,
    response_time_ms: u32,
    checked_at: String,
}

/// Result of consensus evaluation.
pub struct ConsensusResult {
    pub effective_status: String,
    pub up_count: u32,
    pub down_count: u32,
    pub degraded_count: u32,
    pub unknown_count: u32,
    pub total_locations: u32,
}

/// Evaluate multi-region consensus for a monitor and update its status + incident lifecycle.
///
/// Call this after storing heartbeats (from local checker or probe submission)
/// when the monitor has `consensus_threshold` set.
///
/// Returns the consensus result, or None if the monitor has no consensus configured
/// or no heartbeat data exists.
pub async fn evaluate_and_apply(
    db: &Db,
    broadcaster: &EventBroadcaster,
    http_client: &reqwest::Client,
    monitor_id: &str,
) -> Option<ConsensusResult> {
    let webhook_event: Option<WebhookPayload>;
    let consensus: ConsensusResult;

    {
        let conn = db.conn.lock().unwrap();

        // Get monitor info
        let monitor_info: Option<(String, String, u32, String)> = conn.query_row(
            "SELECT id, name, url, consensus_threshold, current_status FROM monitors WHERE id = ?1",
            params![monitor_id],
            |row| {
                let ct: Option<u32> = row.get(3)?;
                match ct {
                    Some(threshold) => Ok(Some((
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        threshold,
                        row.get::<_, String>(4)?,
                    ))),
                    None => Ok(None),
                }
            },
        ).ok()??;

        let (name, url, threshold, current_status) = monitor_info;

        // Get latest heartbeat per location (including local where location_id IS NULL)
        // Uses a window function to get the most recent heartbeat for each location
        let mut stmt = conn.prepare(
            "SELECT location_id, status, response_time_ms, checked_at
             FROM (
                 SELECT location_id, status, response_time_ms, checked_at,
                        ROW_NUMBER() OVER (PARTITION BY COALESCE(location_id, '__local__') ORDER BY checked_at DESC) as rn
                 FROM heartbeats
                 WHERE monitor_id = ?1
             )
             WHERE rn = 1"
        ).ok()?;

        let heartbeats: Vec<LocationHeartbeat> = stmt.query_map(params![monitor_id], |row| {
            Ok(LocationHeartbeat {
                location_id: row.get(0)?,
                status: row.get(1)?,
                response_time_ms: row.get(2)?,
                checked_at: row.get(3)?,
            })
        }).ok()?
        .filter_map(|r| r.ok())
        .collect();

        if heartbeats.is_empty() {
            return None;
        }

        // Count statuses
        let mut up_count = 0u32;
        let mut down_count = 0u32;
        let mut degraded_count = 0u32;
        let mut unknown_count = 0u32;

        for hb in &heartbeats {
            match hb.status.as_str() {
                "up" => up_count += 1,
                "down" => down_count += 1,
                "degraded" => degraded_count += 1,
                _ => unknown_count += 1,
            }
        }

        let total_locations = heartbeats.len() as u32;

        // Determine effective status based on consensus threshold
        let effective_status = if down_count >= threshold {
            "down".to_string()
        } else if degraded_count > 0 && (down_count + degraded_count) >= threshold {
            "degraded".to_string()
        } else if up_count > 0 {
            "up".to_string()
        } else if degraded_count > 0 {
            "degraded".to_string()
        } else {
            "unknown".to_string()
        };

        consensus = ConsensusResult {
            effective_status: effective_status.clone(),
            up_count,
            down_count,
            degraded_count,
            unknown_count,
            total_locations,
        };

        // Only update if status actually changed
        if effective_status == current_status {
            return Some(consensus);
        }

        // Update monitor status
        let _ = conn.execute(
            "UPDATE monitors SET current_status = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![effective_status, monitor_id],
        );

        // Incident lifecycle based on status transition
        let now_str = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let mk_monitor = |status: &str| WebhookMonitor {
            id: monitor_id.to_string(),
            name: name.clone(),
            url: url.clone(),
            current_status: status.to_string(),
        };

        webhook_event = if current_status != "down" && current_status != "maintenance" && effective_status == "down" {
            // → down: create incident
            let inc_id = uuid::Uuid::new_v4().to_string();
            let cause = format!(
                "Consensus: {}/{} locations report down (threshold: {})",
                down_count, total_locations, threshold
            );
            let inc_seq: i64 = conn
                .query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM incidents", [], |r| r.get(0))
                .unwrap_or(1);
            let _ = conn.execute(
                "INSERT INTO incidents (id, monitor_id, cause, seq) VALUES (?1, ?2, ?3, ?4)",
                params![inc_id, monitor_id, cause, inc_seq],
            );
            Some(WebhookPayload {
                event: "incident.created".to_string(),
                monitor: mk_monitor("down"),
                incident: Some(WebhookIncident {
                    id: inc_id,
                    cause,
                    started_at: now_str.clone(),
                    resolved_at: None,
                }),
                timestamp: now_str,
            })
        } else if current_status == "down" && effective_status != "down" && effective_status != "maintenance" {
            // down → recovered: resolve incidents
            let _ = conn.execute(
                "UPDATE incidents SET resolved_at = datetime('now') WHERE monitor_id = ?1 AND resolved_at IS NULL",
                params![monitor_id],
            );
            let incident_info: Option<(String, String, String)> = conn
                .query_row(
                    "SELECT id, cause, started_at FROM incidents WHERE monitor_id = ?1 ORDER BY started_at DESC LIMIT 1",
                    params![monitor_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .ok();
            Some(WebhookPayload {
                event: "incident.resolved".to_string(),
                monitor: mk_monitor(&effective_status),
                incident: incident_info.map(|(id, cause, started_at)| WebhookIncident {
                    id,
                    cause,
                    started_at,
                    resolved_at: Some(now_str.clone()),
                }),
                timestamp: now_str,
            })
        } else if current_status != "degraded" && effective_status == "degraded" {
            Some(WebhookPayload {
                event: "monitor.degraded".to_string(),
                monitor: mk_monitor("degraded"),
                incident: None,
                timestamp: now_str,
            })
        } else if current_status == "degraded" && effective_status == "up" {
            Some(WebhookPayload {
                event: "monitor.recovered".to_string(),
                monitor: mk_monitor("up"),
                incident: None,
                timestamp: now_str,
            })
        } else {
            None
        };
    } // DB lock released

    // Fire notifications outside the lock
    if let Some(ref payload) = webhook_event {
        broadcaster.send(SseEvent {
            event_type: payload.event.clone(),
            monitor_id: monitor_id.to_string(),
            data: serde_json::to_value(payload).unwrap_or_default(),
        });

        let urls = notifications::get_webhook_urls(db, monitor_id);
        if !urls.is_empty() {
            notifications::fire_webhooks(http_client, &urls, payload).await;
        }

        let emails = notifications::get_email_addresses(db, monitor_id);
        if !emails.is_empty() {
            notifications::fire_emails(&emails, payload).await;
        }
    }

    Some(consensus)
}

/// Query the current consensus status for a monitor without modifying anything.
/// Used by the GET /monitors/:id/consensus endpoint.
pub fn get_consensus_status(
    db: &Db,
    monitor_id: &str,
) -> Option<crate::models::ConsensusStatus> {
    let conn = db.conn.lock().unwrap();

    // Get monitor consensus_threshold
    let threshold: u32 = conn.query_row(
        "SELECT consensus_threshold FROM monitors WHERE id = ?1",
        params![monitor_id],
        |row| row.get::<_, Option<u32>>(0),
    ).ok()??;

    // Get latest heartbeat per location
    let mut stmt = conn.prepare(
        "SELECT h.location_id, COALESCE(cl.name, 'Local'), cl.region, h.status, h.response_time_ms, h.checked_at
         FROM (
             SELECT location_id, status, response_time_ms, checked_at,
                    ROW_NUMBER() OVER (PARTITION BY COALESCE(location_id, '__local__') ORDER BY checked_at DESC) as rn
             FROM heartbeats
             WHERE monitor_id = ?1
         ) h
         LEFT JOIN check_locations cl ON cl.id = h.location_id
         WHERE h.rn = 1
         ORDER BY COALESCE(cl.name, 'Local') ASC"
    ).ok()?;

    let locations: Vec<crate::models::ConsensusLocationDetail> = stmt.query_map(params![monitor_id], |row| {
        Ok(crate::models::ConsensusLocationDetail {
            location_id: row.get(0)?,
            location_name: row.get(1)?,
            region: row.get(2)?,
            last_status: row.get(3)?,
            last_response_time_ms: row.get(4)?,
            last_checked_at: row.get(5)?,
        })
    }).ok()?
    .filter_map(|r| r.ok())
    .collect();

    let mut up_count = 0u32;
    let mut down_count = 0u32;
    let mut degraded_count = 0u32;
    let mut unknown_count = 0u32;

    for loc in &locations {
        match loc.last_status.as_str() {
            "up" => up_count += 1,
            "down" => down_count += 1,
            "degraded" => degraded_count += 1,
            _ => unknown_count += 1,
        }
    }

    let total_locations = locations.len() as u32;
    let effective_status = if down_count >= threshold {
        "down".to_string()
    } else if degraded_count > 0 && (down_count + degraded_count) >= threshold {
        "degraded".to_string()
    } else if up_count > 0 {
        "up".to_string()
    } else if degraded_count > 0 {
        "degraded".to_string()
    } else {
        "unknown".to_string()
    };

    Some(crate::models::ConsensusStatus {
        monitor_id: monitor_id.to_string(),
        consensus_threshold: threshold,
        total_locations,
        up_count,
        down_count,
        degraded_count,
        unknown_count,
        effective_status,
        locations,
    })
}
