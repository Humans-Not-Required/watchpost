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

### Incidents
| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | /api/v1/monitors/:id/incidents | ❌ | Incident history |
| GET | /api/v1/incidents/:id | ❌ | Single incident detail |
| POST | /api/v1/incidents/:id/acknowledge | 🔑 | Ack incident with note |

### Notification Channels
| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | /api/v1/monitors/:id/notifications | 🔑 | Add notification channel to monitor |
| GET | /api/v1/monitors/:id/notifications | 🔑 | List notification channels |
| PATCH | /api/v1/notifications/:id | 🔑 | Update channel config |
| DELETE | /api/v1/notifications/:id | 🔑 | Remove channel |

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
- UDP checks
- Multi-region checking
- Maintenance windows
- Custom incident severity
- SLA tracking
- Alerting rules (escalation)
