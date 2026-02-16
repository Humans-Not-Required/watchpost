use rocket::{get, serde::json::Json};
use rocket::http::ContentType;

// ── Health ──

#[get("/health")]
pub fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "service": "watchpost",
        "status": "ok",
        "version": "0.1.0"
    }))
}

// ── llms.txt ──

#[get("/llms.txt")]
pub fn llms_txt() -> (rocket::http::ContentType, &'static str) {
    (rocket::http::ContentType::Plain, include_str!("../../static/llms.txt"))
}

// ── OpenAPI Spec ──

#[get("/openapi.json")]
pub fn openapi_spec() -> (rocket::http::ContentType, &'static str) {
    (rocket::http::ContentType::JSON, include_str!("../../static/openapi.json"))
}

// ── Well-Known Skills Discovery (Cloudflare RFC) ──

#[get("/.well-known/skills/index.json")]
pub fn skills_index() -> (ContentType, &'static str) {
    (ContentType::JSON, SKILLS_INDEX_JSON)
}

#[get("/.well-known/skills/watchpost/SKILL.md")]
pub fn skills_skill_md() -> (ContentType, &'static str) {
    (ContentType::Markdown, SKILL_MD)
}

const SKILLS_INDEX_JSON: &str = r#"{
  "skills": [
    {
      "name": "watchpost",
      "description": "Integrate with Watchpost — an agent-native uptime monitoring service. Create HTTP/TCP/DNS monitors, track incidents, configure alerts, stream events via SSE, and build monitoring automation on a private network.",
      "files": [
        "SKILL.md"
      ]
    }
  ]
}"#;

const SKILL_MD: &str = r#"---
name: watchpost
description: Integrate with Watchpost — an agent-native uptime monitoring service. Create HTTP/TCP/DNS monitors, track incidents, configure alerts, stream events via SSE, and build monitoring automation on a private network.
---

# Watchpost Integration

An agent-native uptime monitoring service (Uptime Kuma vibe) designed for AI agents. Per-monitor auth tokens, SSE real-time events, multi-region checks, and a comprehensive REST API.

## Quick Start

1. **Health check:**
   ```
   GET /api/v1/health
   ```

2. **Create a monitor:**
   ```
   POST /api/v1/monitors
   {"name": "My API", "url": "https://example.com/health", "interval_seconds": 600}
   ```
   Returns a `manage_key` (format: `wp_<hex>`) — save it. Required for updates and deletion.

3. **Check status:**
   ```
   GET /api/v1/monitors/{id}
   ```

4. **Stream real-time events:**
   ```
   GET /api/v1/events
   ```
   SSE stream for all monitors. Per-monitor: `GET /api/v1/monitors/{id}/events`

## Auth Model

- **No auth** to read public monitors, status pages, or badges
- **Per-monitor `manage_key`** (returned on creation) for updates, deletion, notifications, maintenance, alerts
- Pass via `Authorization: Bearer <key>` or `X-Manage-Key: <key>` header
- **Global admin key** auto-generated on first run for settings and dashboard

## Monitor Types

### HTTP (default)
```json
{"name": "Web App", "url": "https://example.com", "method": "GET", "expected_status": 200}
```
Supports custom headers, body content matching, redirect following.

### TCP
```json
{"name": "Redis", "url": "tcp://redis-host:6379", "monitor_type": "tcp"}
```
Checks port connectivity with configurable timeout.

### DNS
```json
{"name": "DNS Check", "url": "example.com", "monitor_type": "dns", "dns_record_type": "A", "dns_expected": "93.184.216.34"}
```
Supports A, AAAA, CNAME, MX, TXT, NS, SOA, PTR, SRV, CAA record types.

## Core Patterns

### Monitor Lifecycle
```
POST   /api/v1/monitors              — Create (returns manage_key)
GET    /api/v1/monitors              — List (public only without admin key)
GET    /api/v1/monitors/{id}         — Details + current status
PATCH  /api/v1/monitors/{id}         — Update (manage_key required)
DELETE /api/v1/monitors/{id}         — Delete (manage_key required)
POST   /api/v1/monitors/{id}/pause   — Pause checks (manage_key required)
POST   /api/v1/monitors/{id}/resume  — Resume checks (manage_key required)
```

### Heartbeat History
```
GET /api/v1/monitors/{id}/heartbeats?limit=100&after=<seq>
```
Cursor-based pagination via monotonic `seq`. Each heartbeat includes status, response_time_ms, error message.

### Incidents
```
GET  /api/v1/monitors/{id}/incidents?limit=20&after=<seq>
POST /api/v1/incidents/{id}/acknowledge   — Acknowledge (manage_key required)
POST /api/v1/incidents/{id}/notes         — Add investigation note
GET  /api/v1/incidents/{id}/notes         — List notes (public)
```
Incidents auto-create on failure (after confirmation_threshold consecutive failures) and auto-resolve on recovery.

### Uptime & SLA
```
GET /api/v1/monitors/{id}/uptime              — 24h/7d/30d/90d percentages
GET /api/v1/monitors/{id}/uptime-history?days=30  — Daily uptime history
GET /api/v1/monitors/{id}/sla                 — SLA target vs actual, error budget
```

### Notifications
```
POST   /api/v1/monitors/{id}/notifications  — Add webhook or email channel
GET    /api/v1/monitors/{id}/notifications  — List channels
PATCH  /api/v1/monitors/{id}/notifications/{nid}  — Enable/disable
DELETE /api/v1/monitors/{id}/notifications/{nid}  — Remove
```
Webhook notifications include retry (3 attempts, exponential backoff). Delivery audit log available.

### Alert Rules
```
POST   /api/v1/monitors/{id}/alert-rules   — Configure repeat/escalation
GET    /api/v1/monitors/{id}/alert-rules    — List rules
DELETE /api/v1/monitors/{id}/alert-rules/{rid}  — Remove rule
GET    /api/v1/monitors/{id}/alert-log      — Alert history
```

### Maintenance Windows
```
POST   /api/v1/monitors/{id}/maintenance   — Schedule downtime
GET    /api/v1/monitors/{id}/maintenance   — List windows
DELETE /api/v1/monitors/{id}/maintenance/{mid}  — Cancel
```
During active maintenance, checks continue but incidents are suppressed.

### Status Pages
```
POST   /api/v1/status-pages              — Create named page (returns manage_key)
GET    /api/v1/status-pages              — List all pages
GET    /api/v1/status-pages/{slug}       — Page detail with monitor statuses
POST   /api/v1/status-pages/{slug}/monitors  — Add monitors to page
DELETE /api/v1/status-pages/{slug}/monitors/{mid}  — Remove monitor
```

### Monitor Dependencies
```
POST   /api/v1/monitors/{id}/dependencies  — Add upstream dependency
GET    /api/v1/monitors/{id}/dependencies  — List upstream deps
GET    /api/v1/monitors/{id}/dependents    — List downstream deps
DELETE /api/v1/monitors/{id}/dependencies/{dep_id}  — Remove
```
When an upstream dependency is down, alerts for downstream monitors are suppressed.

### Embeddable Badges
```
GET /api/v1/monitors/{id}/badge/uptime?period=7d&label=My+Service
GET /api/v1/monitors/{id}/badge/status
```
Returns shields.io-style SVG badges. Embed in README or dashboards.

### Multi-Region Checks
```
POST /api/v1/locations            — Register check location (admin key)
POST /api/v1/probe                — Submit probe results (probe_key auth)
GET  /api/v1/monitors/{id}/locations    — Per-location status
GET  /api/v1/monitors/{id}/consensus    — Multi-region consensus status
```

### Bulk Operations
```
POST /api/v1/monitors/bulk         — Create up to 50 monitors at once
GET  /api/v1/monitors/{id}/export  — Export config for re-import
```

## SSE Event Types

Connect to `GET /api/v1/events` (global) or `GET /api/v1/monitors/{id}/events` (per-monitor):

`check.completed`, `incident.created`, `incident.resolved`, `incident.reminder`, `incident.escalated`, `monitor.degraded`, `monitor.recovered`, `maintenance.started`, `maintenance.ended`

## Rate Limits

Monitor creation is rate-limited at 10/hour per IP.

## Gotchas

- Monitor IDs are UUIDs — use the `id` field from creation response
- `manage_key` is only returned on creation — save it immediately
- Minimum check interval is 600 seconds (10 minutes)
- `confirmation_threshold` (default 3) — number of consecutive failures before creating an incident
- `follow_redirects` defaults to true for HTTP monitors
- Heartbeat retention is 90 days (configurable via `HEARTBEAT_RETENTION_DAYS`)
- Tags, groups, search (`?search=`), and status filter (`?status=`) available on monitor list
- Private monitors (default) are hidden from public list — set `is_public: true` for visibility

## Full API Reference

See `/api/v1/llms.txt` for complete endpoint documentation and `/api/v1/openapi.json` for the OpenAPI 3.0.3 specification.
"#;

// ── SPA Fallback ──

#[get("/<_path..>", rank = 100)]
pub fn spa_fallback(_path: std::path::PathBuf) -> Option<(ContentType, Vec<u8>)> {
    let static_dir: std::path::PathBuf = std::env::var("STATIC_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("../frontend/dist"));
    let index_path = static_dir.join("index.html");
    std::fs::read(&index_path)
        .ok()
        .map(|bytes| (ContentType::HTML, bytes))
}
