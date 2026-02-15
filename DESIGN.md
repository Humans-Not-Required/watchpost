# Watchpost — Design Document

## What Is This?

**Watchpost** is an agent-native monitoring service. Think Uptime Kuma or Pingdom, but designed from the ground up for AI agents to manage.

Agents register endpoints, configure checks, receive structured alerts, and view incident history — all via REST API. Humans get a clean dashboard.

## Core Concepts

### Monitor
A **monitor** is a check definition: what to probe, how often, and what counts as success.

| Field | Type | Description |
|-------|------|-------------|
| id | UUID | Unique identifier |
| name | String | Human-readable label |
| url | String | Target to check (HTTP: URL, TCP: host:port, DNS: hostname) |
| monitor_type | Enum | http (default), tcp, dns |
| dns_record_type | String | DNS record type: A (default), AAAA, CNAME, MX, TXT, NS, SOA, PTR, SRV, CAA |
| dns_expected | Option<String> | Expected resolved value for DNS (null = any resolution is OK) |
| sla_target | Option<f64> | SLA uptime target percentage (0-100, null = no SLA) |
| sla_period_days | Option<u32> | Rolling SLA period in days (1-365, default 30) |
| method | Enum | GET, HEAD, POST (HTTP only) |
| interval_seconds | u32 | Check frequency (min: 600, default: 600) |
| timeout_ms | u32 | Request timeout (default: 10000) |
| expected_status | u16 | Expected HTTP status (default: 200) |
| body_contains | Option<String> | Optional substring match on response body |
| headers | Option<JSON> | Custom headers to send with check |
| is_public | bool | Visible on public status page (default: false) |
| is_paused | bool | Skip checks when paused (default: false) |
| created_at | Timestamp | |
| updated_at | Timestamp | |

### Check Result (Heartbeat)
Each probe execution produces a **check result** stored in `heartbeats`:

| Field | Type | Description |
|-------|------|-------------|
| id | UUID | |
| monitor_id | UUID | FK to monitor |
| status | Enum | up, down, degraded |
| response_time_ms | u32 | End-to-end latency |
| status_code | Option<u16> | HTTP status received |
| error_message | Option<String> | Error detail if down |
| checked_at | Timestamp | When the check ran |

### Incident
An **incident** is an unresolved period of downtime. Created automatically when a monitor transitions from `up` to `down`. Resolved automatically when it transitions back.

| Field | Type | Description |
|-------|------|-------------|
| id | UUID | |
| monitor_id | UUID | FK to monitor |
| started_at | Timestamp | First failure |
| resolved_at | Option<Timestamp> | When it recovered (null = ongoing) |
| cause | String | First error message |
| acknowledgement | Option<String> | Manual ack note |
| acknowledged_by | Option<String> | Who acked |

### Notification Channel
A **notification channel** defines where alerts go.

| Field | Type | Description |
|-------|------|-------------|
| id | UUID | |
| name | String | Label |
| channel_type | Enum | webhook, email |
| config | JSON | URL for webhook, address for email |
| is_enabled | bool | Active toggle |

### Monitor ↔ Notification (M2M)
Join table: `monitor_notifications(monitor_id, notification_id)`

## Auth Model

Per the HNR design principles: **tokens tied to resources, not users.**

- `POST /api/v1/monitors` → returns `manage_key` (shown once)
- Read endpoints (list monitors, view status, heartbeats, incidents) → **no auth** (just need the monitor UUID)
- Write endpoints (update, delete, pause, manage notifications) → require `manage_key`
- Token accepted via: `Authorization: Bearer`, `X-API-Key` header, or `?key=` query param
- Public status page → no auth, shows only `is_public=true` monitors
- IP-based rate limiting on monitor creation (10/hr default)

**Batch management:** For agents managing many monitors, a future "workspace" concept could group monitors under one key. Not in v1.

## API Endpoints

### Monitors
| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | /api/v1/monitors | ❌ | Create monitor → returns manage_key |
| GET | /api/v1/monitors | ❌ | List public monitors (status page) |
| GET | /api/v1/monitors/:id | ❌ | Get monitor details + current status |
| PATCH | /api/v1/monitors/:id | 🔑 | Update monitor config |
| DELETE | /api/v1/monitors/:id | 🔑 | Delete monitor + all data |
| POST | /api/v1/monitors/:id/pause | 🔑 | Pause checks |
| POST | /api/v1/monitors/:id/resume | 🔑 | Resume checks |

### Heartbeats (Check Results)
| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | /api/v1/monitors/:id/heartbeats | ❌ | Paginated check history |
| GET | /api/v1/monitors/:id/uptime | ❌ | Uptime stats (24h, 7d, 30d, 90d) |

### SLA
| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | /api/v1/monitors/:id/sla | ❌ | SLA status with error budget |

### Incidents
| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | /api/v1/monitors/:id/incidents | ❌ | Incident history |
| GET | /api/v1/incidents/:id | ❌ | Single incident detail (includes notes_count) |
| POST | /api/v1/incidents/:id/acknowledge | 🔑 | Ack incident with note |
| POST | /api/v1/incidents/:id/notes | 🔑 | Add investigation note |
| GET | /api/v1/incidents/:id/notes | ❌ | List notes (chronological) |

### Notification Channels
| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | /api/v1/monitors/:id/notifications | 🔑 | Add notification channel to monitor |
| GET | /api/v1/monitors/:id/notifications | 🔑 | List notification channels |
| PATCH | /api/v1/notifications/:id | 🔑 | Update channel config |
| DELETE | /api/v1/notifications/:id | 🔑 | Remove channel |
| GET | /api/v1/monitors/:id/webhook-deliveries | 🔑 | Webhook delivery audit log |

### Status Page
| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | /api/v1/status | ❌ | Public status overview (all public monitors) |

### System
| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | /api/v1/health | ❌ | Service health |
| GET | /api/v1/llms.txt | ❌ | AI agent discovery |
| GET | /api/v1/openapi.json | ❌ | OpenAPI spec |

### SSE Events
| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | /api/v1/monitors/:id/events | ❌ | Real-time events for a monitor |
| GET | /api/v1/events | ❌ | Global event stream (all public monitors) |

Event types: `check.completed`, `monitor.down`, `monitor.up`, `monitor.degraded`, `incident.created`, `incident.resolved`, `incident.acknowledged`

## Background Checker

The core of the service: a background tokio task that runs checks on schedule.

**Architecture:**
- On startup, load all non-paused monitors from DB
- Each monitor's interval drives a tick; use a single scheduler loop that wakes at the next-due check
- Use `reqwest` for HTTP checks (configurable timeout, follow redirects: off)
- Determine status: `up` (expected status + optional body match), `degraded` (slow but correct, >5s), `down` (error/wrong status)
- Write heartbeat to DB
- If status changed: create/resolve incident, fire notifications, emit SSE event
- Re-check DB periodically for new/changed monitors (or use a channel to notify the scheduler)

**Check evaluation:**
1. Send HTTP request with configured method, headers, timeout
2. If request fails (timeout, DNS, connection refused) → `down`
3. If status code != expected → `down`
4. If body_contains set and body doesn't contain it → `down`
5. If response_time > 5000ms but otherwise OK → `degraded`
6. Otherwise → `up`

**Incident lifecycle:**
- Monitor transitions `up → down`: create incident, fire `monitor.down` + `incident.created`
- Monitor transitions `down → up`: resolve incident, fire `monitor.up` + `incident.resolved`
- Requires `confirmation_threshold` consecutive failures before declaring down (default: 2, prevents flap)

## Tech Stack

- **Backend:** Rust + Rocket 0.5 + SQLite (rusqlite)
- **HTTP client:** reqwest
- **Frontend:** React + Vite (same dark theme as HNR suite)
- **Deployment:** Single binary, Docker multi-stage build
- **Port:** 8000 internal, 3007 external

## Data Retention

Heartbeats are the main storage cost. Default retention: 90 days. Older heartbeats auto-pruned by a background task. Incidents kept indefinitely.

## What Makes This Agent-First?

1. **Structured JSON everywhere** — agents can reason about responses, not scrape HTML
2. **SSE event streams** — agents subscribe and react in real-time
3. **llms.txt** — agents discover the API without reading docs
4. **No signup** — create a monitor, get a token, start monitoring
5. **Programmatic notifications** — webhooks with structured JSON payloads
6. **Self-describing** — OpenAPI spec, consistent error codes, clear status enums

## MVP Scope (v0.1)

**In:**
- Monitor CRUD with manage_key auth
- Background HTTP checker
- Heartbeat storage + history
- Incident auto-detection + resolution
- Uptime stats endpoint
- Webhook notifications (on status change)
- Public status page (GET /status)
- SSE event stream
- Health, llms.txt, OpenAPI
- React frontend
- Docker build

**Out (future):**
- ~~Email notifications (need SMTP config)~~ ✅ Shipped
- ~~TCP checks~~ ✅ Shipped (monitor_type: tcp)
- ~~DNS checks~~ ✅ Shipped (monitor_type: dns, 10 record types, expected value matching)
- ~~Maintenance windows~~ ✅ Shipped (schedule windows where checks are skipped)
- ~~Monitor groups~~ ✅ Shipped (organize monitors into named groups)
- ~~Tags~~ ✅ Shipped (arbitrary key-value tags on monitors for filtering)
- ~~Status page branding~~ ✅ Shipped (custom title, description, logo via settings API)
- ~~Shields.io-style badges~~ ✅ Shipped (uptime + status SVG badges per monitor)
- ~~Dashboard API~~ ✅ Shipped (aggregated stats: total, up, down, degraded, maintenance, paused)
- ~~Response time alerts~~ ✅ Shipped (configurable thresholds)
- ~~Search/filter~~ ✅ Shipped (filter monitors by name, status, group, tags)
- ~~Heartbeat retention~~ ✅ Shipped (configurable, auto-prune old heartbeats)
- ~~Follow redirects~~ ✅ Shipped (HTTP monitors follow 301/302/etc by default)
- ~~Notification toggle~~ ✅ Shipped (per-monitor notification enable/disable)
- ~~Seq-based cursor pagination~~ ✅ Shipped (heartbeats + incidents use `?after=<seq>`)
- UDP checks
- ~~Multi-region checking~~ ✅ Shipped (check locations + probe API)
- ~~Multi-region consensus~~ ✅ Shipped (configurable threshold, incident lifecycle integration)
- ~~Monitor dependency chains (alert suppression when upstream is down)~~ ✅ Shipped
- Custom incident severity
- ~~SLA tracking~~ ✅ Shipped (per-monitor targets with error budget tracking)
- ~~Alerting rules (escalation)~~ ✅ Shipped (alert rules API + frontend UI)

## Implemented Features Beyond MVP

### Maintenance Windows
- `POST /api/v1/monitors/:id/maintenance` — Create a maintenance window (title, starts_at, ends_at)
- `GET /api/v1/monitors/:id/maintenance` — List maintenance windows for a monitor
- `DELETE /api/v1/maintenance/:id` — Delete a maintenance window
- During active maintenance windows, checks are skipped and monitor status shows as "maintenance"

### Monitor Groups
- Monitors can be assigned to named groups for organization
- Groups allow logical grouping on dashboards (e.g., "Production", "Staging")

### Tags
- Arbitrary key-value tags on monitors (`POST /api/v1/monitors/:id/tags`)
- Filter monitors by tag values
- Useful for categorization beyond groups

### Badges (Shields.io-style SVGs)
- `GET /api/v1/monitors/:id/badge/uptime?period=30d&label=...` — Uptime percentage badge
- `GET /api/v1/monitors/:id/badge/status?label=...` — Current status badge
- Color-coded: green (up), red (down), orange (degraded), grey (maintenance)
- Embeddable in READMEs and dashboards

### Settings / Branding
- `GET /api/v1/settings` — Read status page settings
- `PATCH /api/v1/settings` — Update branding (title, description, logo URL)
- Applied to the public status page

### SLA Tracking
- Per-monitor SLA targets with error budget tracking
- `sla_target` (REAL, 0-100) and `sla_period_days` (INTEGER, 1-365, default 30) columns on monitors
- `GET /api/v1/monitors/:id/sla` — SLA status: target, current uptime, error budget remaining, status (met/at_risk/breached)
- Error budget = total_period_seconds × (1 - target/100)
- Downtime estimated from heartbeat failure ratio × elapsed time
- Status: "breached" when current_pct < target, "at_risk" when budget < 25% remaining, "met" otherwise
- Degraded heartbeats count as successful (service responded, just slow)
- Returns 404 (SLA_NOT_CONFIGURED) when no target is set
- SLA fields visible in monitor list, detail, and export responses
- Settable on create, update, and bulk create; null clears

### Dashboard API
- `GET /api/v1/dashboard` — Aggregated statistics (total monitors, up, down, degraded, maintenance, paused counts)

### Response Time Alerts
- Configurable response time thresholds per monitor
- Alerts fire when response time exceeds threshold

### Multi-Region Check Locations
Remote check locations allow distributed monitoring from multiple geographic regions.
- `POST /api/v1/locations` — Register a check location (admin key required, returns `probe_key`)
- `GET /api/v1/locations` — List all check locations (public)
- `GET /api/v1/locations/:id` — Get a specific check location
- `DELETE /api/v1/locations/:id` — Remove a check location (admin key required)
- `POST /api/v1/probe` — Submit probe results from a remote location (probe_key auth)
  - Up to 100 results per submission, partial success supported
  - Results stored as heartbeats with `location_id` linking to the check location
  - Updates `last_seen_at` on the location
- `GET /api/v1/monitors/:id/locations` — Per-location status showing latest probe from each active location
- Heartbeats include optional `location_id` field (null = local checker)
- Consensus: `consensus_threshold` field on monitors. When set, status is determined by aggregating results across all locations. Down only when N+ locations report failure.
- `GET /api/v1/monitors/:id/consensus` — consensus status with per-location breakdown

### Monitor Dependencies (Alert Suppression)
Define upstream dependencies between monitors to prevent alert storms when shared infrastructure goes down.
- `POST /api/v1/monitors/:id/dependencies` — Add dependency (manage_key auth)
  - Body: `{"depends_on_id": "<upstream_monitor_id>"}`
  - Validates: no self-dependency, no circular chains (BFS graph walk), both monitors exist, no duplicates
  - Returns 201 with dependency details
- `GET /api/v1/monitors/:id/dependencies` — List dependencies (public)
  - Returns dependency info with upstream monitor name and current status
- `DELETE /api/v1/monitors/:id/dependencies/:dep_id` — Remove dependency (manage_key auth)
- `GET /api/v1/monitors/:id/dependents` — Reverse lookup: who depends on this monitor? (public)
- DB: `monitor_dependencies` table with `UNIQUE(monitor_id, depends_on_id)`, CASCADE delete on both sides
- Checker integration: when a monitor transitions to "down", checks if any dependency is currently down.
  If yes, suppresses incident creation and notifications. Heartbeats still recorded honestly.
  When dependency recovers, next check creates the delayed incident if monitor is still down.

### Webhook Delivery Retry
Webhook notifications are delivered with automatic retry and exponential backoff.
- **3 attempts max** per URL per notification dispatch
- **Backoff:** attempt 1 = immediate, attempt 2 = 2s delay, attempt 3 = 4s delay
- **Success:** HTTP 2xx response stops retries and logs success
- **Failure:** Non-2xx response or connection error triggers retry
- **Audit trail:** Every attempt is logged to `webhook_deliveries` table with status, status_code, error_message, and response_time_ms
- **Delivery groups:** A UUID groups all retry attempts for one notification dispatch (same `delivery_group` ID)
- `GET /api/v1/monitors/:id/webhook-deliveries` — View delivery history (manage key required). Supports `?limit=`, `?after=` cursor, `?event=`, `?status=` filters.
- DB: `webhook_deliveries` table with CASCADE delete on monitor removal

### Alert Rules (Repeat & Escalation)
Per-monitor alert policies that control notification behavior during incidents.
- `PUT /api/v1/monitors/:id/alert-rules` — Set/update rules (upsert). Fields: repeat_interval_minutes (0=disabled, min 5), max_repeats (default 10, max 100), escalation_after_minutes (0=disabled, min 5).
- `GET /api/v1/monitors/:id/alert-rules` — Get current rules (404 if none).
- `DELETE /api/v1/monitors/:id/alert-rules` — Remove rules.
- `GET /api/v1/monitors/:id/alert-log` — Alert notification history (limit, after cursor).
- DB tables: `alert_rules` (one row per monitor, UNIQUE on monitor_id), `alert_log` (audit trail of sent notifications).
- Checker integration: on incident.created, fires initial alert. If repeat_interval_minutes > 0, fires reminder alerts every N minutes up to max_repeats. If escalation_after_minutes > 0 and incident not acknowledged, fires escalation alert.
- SSE events: `incident.reminder`, `incident.escalated`.
- Frontend: AlertRulesManager component on "Alerts" tab (manage key required). Shows current rules, edit form, alert log table.

### Status Pages
Named collections of monitors with their own branding, slug, and optional custom domain.
Each status page gets its own manage_key (same per-resource auth pattern as monitors).
- `POST /api/v1/status-pages` — Create a status page (returns manage_key)
  - Fields: slug (UNIQUE, URL-safe), title, description, logo_url, custom_domain (UNIQUE nullable), is_public (default true)
- `GET /api/v1/status-pages` — List public status pages
- `GET /api/v1/status-pages/:slug_or_id` — Status page detail with monitors, uptime, overall status
- `PATCH /api/v1/status-pages/:slug_or_id` — Update (manage_key auth)
- `DELETE /api/v1/status-pages/:slug_or_id` — Delete (manage_key auth, CASCADE removes assignments, not monitors)
- `POST /api/v1/status-pages/:slug_or_id/monitors` — Add monitors by ID array (manage_key auth, up to 100, duplicates ignored)
- `DELETE /api/v1/status-pages/:slug_or_id/monitors/:monitor_id` — Remove monitor (manage_key auth)
- `GET /api/v1/status-pages/:slug_or_id/monitors` — List monitors with status data (public)
- DB tables: `status_pages` (branding + manage_key_hash) + `status_page_monitors` (join, CASCADE on both sides)
