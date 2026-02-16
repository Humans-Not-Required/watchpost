# Watchpost

Agent-native monitoring service. Think Uptime Kuma, but designed for AI agents.

Monitors HTTP, TCP, and DNS endpoints. Tracks uptime with SLA compliance, detects incidents with confirmation thresholds, supports multi-region consensus checks, and sends structured alerts via webhooks and email — all via REST API. Comes with a clean dashboard for humans too.

## Features

- **HTTP, TCP, and DNS monitoring** — endpoint health, port connectivity, DNS resolution
- **Zero signup** — create a monitor, get a manage token. No accounts.
- **Multi-region checks** — register probe locations, submit results from distributed agents
- **Consensus-based status** — require N locations to agree before marking down
- **SLA tracking** — uptime targets with error budget tracking and compliance status
- **Incident management** — auto-detection, acknowledgement, investigation notes
- **Alert rules** — repeat notifications, escalation policies, alert audit log
- **Maintenance windows** — scheduled downtime without false alerts
- **Monitor dependencies** — upstream/downstream chains with alert suppression
- **Status pages** — named collections with custom branding and slugs
- **Monitor groups and tags** — organize and filter monitors
- **SVG badges** — uptime and status badges for READMEs
- **Webhook notifications** — with automatic retry (3 attempts, exponential backoff)
- **Email notifications** — formatted HTML + plain text via SMTP
- **SSE event streams** — real-time status changes
- **Bulk import/export** — create up to 50 monitors at once, export configs
- **Structured JSON everywhere** — agents parse responses, not HTML
- **OpenAPI spec + llms.txt** — self-describing API for AI agents
- **Privacy-aware dashboard** — individual monitor data requires admin key
- **Dark/light theme toggle** — system preference detection with manual override

## Quick Start

### Docker (recommended)

```bash
docker compose up -d
```

The service will be available at `http://localhost:3007`.

### From Source

```bash
# Backend
cargo build --release

# Frontend
cd frontend && bun install && bun run build && cd ..

# Run (serves frontend from ./frontend/dist)
./target/release/watchpost
```

## Usage

### Create a Monitor

```bash
# HTTP monitor
curl -X POST http://localhost:3007/api/v1/monitors \
  -H "Content-Type: application/json" \
  -d '{
    "name": "My API",
    "url": "https://api.example.com/health",
    "interval_seconds": 600,
    "is_public": true
  }'

# TCP monitor
curl -X POST http://localhost:3007/api/v1/monitors \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Database",
    "monitor_type": "tcp",
    "url": "db.example.com:5432"
  }'

# DNS monitor
curl -X POST http://localhost:3007/api/v1/monitors \
  -H "Content-Type: application/json" \
  -d '{
    "name": "DNS Check",
    "monitor_type": "dns",
    "url": "example.com",
    "dns_record_type": "A",
    "dns_expected": "93.184.216.34"
  }'
```

Response includes a `manage_key` — save it. It's shown once and required for updates/deletes.

### Check Status

```bash
# Public status page (all public monitors)
curl http://localhost:3007/api/v1/status

# Single monitor detail
curl http://localhost:3007/api/v1/monitors/{id}

# Uptime stats (24h/7d/30d/90d)
curl http://localhost:3007/api/v1/monitors/{id}/uptime

# Daily uptime history
curl http://localhost:3007/api/v1/monitors/{id}/uptime-history?days=30

# Heartbeat history
curl http://localhost:3007/api/v1/monitors/{id}/heartbeats
```

### Manage a Monitor

Pass the manage key via header or query param:

```bash
# Update
curl -X PATCH http://localhost:3007/api/v1/monitors/{id} \
  -H "Authorization: Bearer {manage_key}" \
  -H "Content-Type: application/json" \
  -d '{"interval_seconds": 600}'

# Pause/Resume
curl -X POST http://localhost:3007/api/v1/monitors/{id}/pause \
  -H "Authorization: Bearer {manage_key}"

curl -X POST http://localhost:3007/api/v1/monitors/{id}/resume \
  -H "Authorization: Bearer {manage_key}"

# Delete
curl -X DELETE http://localhost:3007/api/v1/monitors/{id} \
  -H "Authorization: Bearer {manage_key}"
```

### Monitor Types

| Type | URL Format | Description |
|------|-----------|-------------|
| `http` (default) | `https://example.com/health` | HTTP/HTTPS endpoint (GET, HEAD, POST) |
| `tcp` | `example.com:5432` | TCP port connectivity check |
| `dns` | `example.com` | DNS record resolution |

**HTTP monitors** support `method` (GET/HEAD/POST), `headers` (JSON object), `expected_status` (default 200), `expected_body` (substring match), and `follow_redirects` (default true, up to 10 hops).

**TCP monitors** validate that a connection can be established to host:port within the timeout.

**DNS monitors** accept `dns_record_type` (A, AAAA, CNAME, MX, TXT, NS, SOA, PTR, SRV, CAA) and optional `dns_expected` (value to match). If `dns_expected` is omitted, any successful resolution passes.

### Validation Rules

| Field | Constraint |
|-------|-----------|
| `interval_seconds` | min 600 (10 min), default 600 |
| `timeout_ms` | min 1000, max 60000, default 10000 |
| `confirmation_threshold` | min 1, max 10, default 2 |
| `response_time_threshold_ms` | min 100 (if set) |
| `headers` | must be JSON object (not array) |

### Notifications

#### Webhooks

```bash
curl -X POST http://localhost:3007/api/v1/monitors/{id}/notifications \
  -H "Authorization: Bearer {manage_key}" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Slack Alert",
    "channel_type": "webhook",
    "config": {"url": "https://hooks.slack.com/..."}
  }'
```

Webhooks fire on incident creation, resolution, degraded, and maintenance events. Delivery includes automatic retry: up to 3 attempts with exponential backoff (2s, 4s delays). Every attempt is logged for audit via `GET /monitors/{id}/webhook-deliveries`.

#### Email

```bash
curl -X POST http://localhost:3007/api/v1/monitors/{id}/notifications \
  -H "Authorization: Bearer {manage_key}" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Ops Team",
    "channel_type": "email",
    "config": {"address": "ops@example.com"}
  }'
```

Sends formatted HTML + plain text emails. Requires SMTP configuration via environment variables (see Configuration).

### Alert Rules

Configure repeat notifications and escalation policies per monitor:

```bash
curl -X PUT http://localhost:3007/api/v1/monitors/{id}/alert-rules \
  -H "Authorization: Bearer {manage_key}" \
  -H "Content-Type: application/json" \
  -d '{
    "repeat_interval_minutes": 15,
    "max_repeats": 10,
    "escalation_after_minutes": 30
  }'
```

- **repeat_interval_minutes** — re-send notifications every N minutes while incident is open (min 5, 0 = disabled)
- **max_repeats** — cap on repeat notifications per incident (default 10, max 100)
- **escalation_after_minutes** — send escalation alert if not acknowledged within N minutes (min 5, 0 = disabled)

View alert notification history: `GET /monitors/{id}/alert-log`

### Incidents

```bash
# List incidents for a monitor
curl http://localhost:3007/api/v1/monitors/{id}/incidents

# Single incident detail (includes notes_count)
curl http://localhost:3007/api/v1/incidents/{id}

# Acknowledge an incident
curl -X POST http://localhost:3007/api/v1/incidents/{id}/acknowledge \
  -H "Authorization: Bearer {manage_key}" \
  -H "Content-Type: application/json" \
  -d '{"note": "Looking into it", "acknowledged_by": "nanook"}'

# Add investigation note
curl -X POST http://localhost:3007/api/v1/incidents/{id}/notes \
  -H "Authorization: Bearer {manage_key}" \
  -H "Content-Type: application/json" \
  -d '{"content": "Root cause: DNS timeout", "author": "nanook"}'

# View investigation timeline
curl http://localhost:3007/api/v1/incidents/{id}/notes
```

### SLA Tracking

Set an uptime target on any monitor to track SLA compliance with error budgets:

```bash
# Set 99.9% uptime target over 30 days
curl -X POST http://localhost:3007/api/v1/monitors \
  -H "Content-Type: application/json" \
  -d '{"name": "Critical API", "url": "...", "sla_target": 99.9, "sla_period_days": 30}'

# Check SLA status
curl http://localhost:3007/api/v1/monitors/{id}/sla
# → target_pct, current_pct, budget_remaining_seconds, status (met|at_risk|breached)
```

### Maintenance Windows

Schedule downtime so checks still run but incidents are suppressed:

```bash
curl -X POST http://localhost:3007/api/v1/monitors/{id}/maintenance \
  -H "Authorization: Bearer {manage_key}" \
  -H "Content-Type: application/json" \
  -d '{
    "title": "Deploy v2",
    "starts_at": "2026-02-10T14:00:00Z",
    "ends_at": "2026-02-10T15:00:00Z"
  }'
```

During an active window, monitor status shows "maintenance" instead of "down". Heartbeats are still recorded.

### Monitor Groups and Tags

```bash
# Create with group and tags
curl -X POST http://localhost:3007/api/v1/monitors \
  -H "Content-Type: application/json" \
  -d '{"name": "API", "url": "...", "group_name": "Infrastructure", "tags": ["prod", "api"]}'

# Filter by group or tag
curl http://localhost:3007/api/v1/monitors?group=Infrastructure
curl http://localhost:3007/api/v1/monitors?tag=prod
curl http://localhost:3007/api/v1/status?group=Infrastructure&tag=prod

# List all groups and tags
curl http://localhost:3007/api/v1/groups
curl http://localhost:3007/api/v1/tags
```

### SVG Badges

Embed uptime and status badges in READMEs:

```markdown
![uptime](http://localhost:3007/api/v1/monitors/{id}/badge/uptime?period=7d)
![status](http://localhost:3007/api/v1/monitors/{id}/badge/status)
```

Uptime badge: `?period=24h|7d|30d|90d`, `?label=custom+text`
Status badge: `?label=custom+text` — color-coded (green=up, yellow=degraded, grey=paused, red=down)

### Monitor Dependencies

Define upstream/downstream relationships with automatic alert suppression:

```bash
# API depends on Database
curl -X POST http://localhost:3007/api/v1/monitors/{api_id}/dependencies \
  -H "Authorization: Bearer {manage_key}" \
  -H "Content-Type: application/json" \
  -d '{"depends_on_id": "{database_id}"}'

# List dependencies and dependents
curl http://localhost:3007/api/v1/monitors/{id}/dependencies
curl http://localhost:3007/api/v1/monitors/{id}/dependents
```

When an upstream monitor is down, downstream monitor incidents are suppressed to prevent alert storms. Heartbeats are still recorded honestly. Circular dependencies are prevented at creation time.

### Multi-Region Monitoring

Register check locations and submit probe results from distributed agents:

```bash
# Register a location (admin key required)
curl -X POST http://localhost:3007/api/v1/locations \
  -H "Authorization: Bearer {admin_key}" \
  -H "Content-Type: application/json" \
  -d '{"name": "US East", "region": "us-east-1"}'
# → returns probe_key (save it!)

# Submit probe results
curl -X POST http://localhost:3007/api/v1/probe \
  -H "Authorization: Bearer {probe_key}" \
  -H "Content-Type: application/json" \
  -d '{"results": [{"monitor_id": "...", "status": "up", "response_time_ms": 50}]}'

# Per-location status
curl http://localhost:3007/api/v1/monitors/{id}/locations

# Consensus status
curl http://localhost:3007/api/v1/monitors/{id}/consensus
```

**Consensus:** Set `consensus_threshold` on a monitor to require N+ locations to agree on "down" before creating an incident. Prevents false positives from single-location issues.

**Probe health tracking:** Locations include `health_status` (healthy/new/stale/disabled). Stale locations auto-disabled after `PROBE_STALE_MINUTES` (default 30).

### Status Pages

Named monitor collections with branding:

```bash
# Create a status page
curl -X POST http://localhost:3007/api/v1/status-pages \
  -H "Content-Type: application/json" \
  -d '{"slug": "production", "title": "Production Status"}'
# → returns manage_key

# Add monitors to the page
curl -X POST http://localhost:3007/api/v1/status-pages/production/monitors \
  -H "Authorization: Bearer {manage_key}" \
  -H "Content-Type: application/json" \
  -d '{"monitor_ids": ["id1", "id2"]}'

# View the status page
curl http://localhost:3007/api/v1/status-pages/production
```

### Bulk Operations

```bash
# Bulk create up to 50 monitors
curl -X POST http://localhost:3007/api/v1/monitors/bulk \
  -H "Content-Type: application/json" \
  -d '{"monitors": [{"name": "API", "url": "..."}, {"name": "Web", "url": "..."}]}'

# Export monitor config
curl http://localhost:3007/api/v1/monitors/{id}/export \
  -H "Authorization: Bearer {manage_key}"
```

### Search and Filter

```bash
curl http://localhost:3007/api/v1/monitors?search=api&status=up
curl http://localhost:3007/api/v1/status?search=keyword&status=down&group=Infrastructure
```

### Real-Time Events (SSE)

```bash
# All public monitors
curl -N http://localhost:3007/api/v1/events

# Single monitor
curl -N http://localhost:3007/api/v1/monitors/{id}/events
```

Event types: `check.completed`, `incident.created`, `incident.resolved`, `maintenance.started`, `maintenance.ended`, `monitor.degraded`, `monitor.recovered`

### Status Page Branding

```bash
# Get current branding
curl http://localhost:3007/api/v1/settings

# Update branding (admin key required)
curl -X PUT http://localhost:3007/api/v1/settings \
  -H "Authorization: Bearer {admin_key}" \
  -H "Content-Type: application/json" \
  -d '{"title": "Our Status", "description": "Service availability", "logo_url": "https://..."}'
```

### Dashboard Privacy

The dashboard requires an admin key (auto-generated on first run) for individual monitor data:

- **Without admin key:** Public status page with aggregate stats only
- **With admin key:** Full dashboard with individual monitors, recent incidents, slowest monitors

Verify admin key: `GET /api/v1/admin/verify`

## API Quick Reference

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | /monitors | ❌ | Create monitor |
| POST | /monitors/bulk | ❌ | Bulk create (up to 50) |
| GET | /monitors | ❌ | List public monitors |
| GET | /monitors/:id | ❌ | Monitor details |
| PATCH | /monitors/:id | 🔑 | Update monitor |
| DELETE | /monitors/:id | 🔑 | Delete monitor |
| POST | /monitors/:id/pause | 🔑 | Pause checks |
| POST | /monitors/:id/resume | 🔑 | Resume checks |
| GET | /monitors/:id/export | 🔑 | Export config |
| GET | /monitors/:id/heartbeats | ❌ | Check history |
| GET | /monitors/:id/uptime | ❌ | Uptime stats |
| GET | /monitors/:id/uptime-history | ❌ | Daily uptime history |
| GET | /uptime-history | ❌ | Aggregate daily uptime |
| GET | /monitors/:id/incidents | ❌ | Incident history |
| GET | /incidents/:id | ❌ | Incident detail |
| POST | /incidents/:id/acknowledge | 🔑 | Acknowledge incident |
| POST | /incidents/:id/notes | 🔑 | Add investigation note |
| GET | /incidents/:id/notes | ❌ | List notes |
| POST | /monitors/:id/notifications | 🔑 | Add notification |
| GET | /monitors/:id/notifications | 🔑 | List notifications |
| PATCH | /notifications/:id | 🔑 | Enable/disable |
| DELETE | /notifications/:id | 🔑 | Remove notification |
| GET | /monitors/:id/webhook-deliveries | 🔑 | Webhook audit log |
| PUT | /monitors/:id/alert-rules | 🔑 | Set alert rules |
| GET | /monitors/:id/alert-rules | 🔑 | Get alert rules |
| DELETE | /monitors/:id/alert-rules | 🔑 | Remove alert rules |
| GET | /monitors/:id/alert-log | 🔑 | Alert history |
| POST | /monitors/:id/maintenance | 🔑 | Create maintenance window |
| GET | /monitors/:id/maintenance | ❌ | List maintenance windows |
| DELETE | /maintenance/:id | 🔑 | Delete maintenance window |
| GET | /monitors/:id/sla | ❌ | SLA compliance |
| GET | /monitors/:id/badge/uptime | ❌ | SVG uptime badge |
| GET | /monitors/:id/badge/status | ❌ | SVG status badge |
| POST | /monitors/:id/dependencies | 🔑 | Add dependency |
| GET | /monitors/:id/dependencies | ❌ | List dependencies |
| DELETE | /monitors/:id/dependencies/:id | 🔑 | Remove dependency |
| GET | /monitors/:id/dependents | ❌ | List dependents |
| GET | /monitors/:id/locations | ❌ | Per-location status |
| GET | /monitors/:id/consensus | ❌ | Consensus status |
| POST | /locations | 🔑 admin | Register check location |
| GET | /locations | ❌ | List locations |
| GET | /locations/:id | ❌ | Get location |
| DELETE | /locations/:id | 🔑 admin | Remove location |
| POST | /probe | 🔑 probe | Submit probe results |
| POST | /status-pages | ❌ | Create status page |
| GET | /status-pages | ❌ | List status pages |
| GET | /status-pages/:slug | ❌ | Status page detail |
| PATCH | /status-pages/:slug | 🔑 | Update status page |
| DELETE | /status-pages/:slug | 🔑 | Delete status page |
| POST | /status-pages/:slug/monitors | 🔑 | Add monitors |
| DELETE | /status-pages/:slug/monitors/:id | 🔑 | Remove monitor |
| GET | /status-pages/:slug/monitors | ❌ | List page monitors |
| GET | /tags | ❌ | List all tags |
| GET | /groups | ❌ | List all groups |
| GET | /status | ❌ | Public status overview |
| GET | /dashboard | ❌/🔑 | Dashboard stats |
| GET | /admin/verify | ❌ | Verify admin key |
| GET | /settings | ❌ | Status page branding |
| PUT | /settings | 🔑 admin | Update branding |
| GET | /events | ❌ | Global SSE stream |
| GET | /monitors/:id/events | ❌ | Per-monitor SSE stream |
| GET | /health | ❌ | Health check |
| GET | /openapi.json | ❌ | OpenAPI 3.0 spec |
| GET | /llms.txt | ❌ | AI-readable API summary |

All paths are under `/api/v1`. Auth: `Authorization: Bearer {key}`, `X-API-Key: {key}`, or `?key={key}`.

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `ROCKET_ADDRESS` | `0.0.0.0` | Bind address |
| `ROCKET_PORT` | `8000` | Server port |
| `DATABASE_PATH` | `watchpost.db` | SQLite database path |
| `STATIC_DIR` | `frontend/dist` | Frontend static files |
| `MONITOR_RATE_LIMIT` | `10` | Max monitor creates per hour per IP |
| `HEARTBEAT_RETENTION_DAYS` | `90` | Auto-prune heartbeats older than N days |
| `PROBE_STALE_MINUTES` | `30` | Auto-disable stale probe locations after N minutes |
| `SMTP_HOST` | *(required for email)* | SMTP server hostname |
| `SMTP_PORT` | `587` | SMTP port |
| `SMTP_USERNAME` | *(empty)* | SMTP auth username |
| `SMTP_PASSWORD` | *(empty)* | SMTP auth password |
| `SMTP_FROM` | `watchpost@<host>` | Sender email address |
| `SMTP_TLS` | `starttls` | TLS mode: `starttls`, `tls`, or `none` |

## Tech Stack

- **Backend:** Rust + Rocket 0.5 + SQLite (rusqlite bundled)
- **Frontend:** React + Vite
- **HTTP checks:** reqwest (rustls-tls)
- **DNS checks:** trust-dns-resolver
- **Real-time:** SSE via Rocket streams + tokio broadcast
- **Deployment:** Docker multi-stage build, Watchtower auto-deploy

## How It Works

A background checker runs on a schedule. For each active monitor:

1. Sends an HTTP/TCP/DNS check with configured parameters
2. Evaluates the response against expected values (status code, body, DNS record, response time)
3. Records a heartbeat (up/down/degraded + latency + location)
4. After consecutive failures exceed the confirmation threshold, creates an incident
5. Fires webhook/email notifications and SSE events on status changes
6. Evaluates multi-region consensus when configured
7. Suppresses alerts when upstream dependencies are down
8. Auto-resolves incidents when the monitor recovers

## Frontend

The React dashboard provides:

- **Public status page** — default landing, aggregate stats
- **Admin dashboard** — individual monitors, incidents, response times (admin key required)
- **Monitor management** — create, edit, pause/resume, delete
- **Incident timeline** — investigation notes, acknowledgement
- **Alert rules** — repeat notifications, escalation policies
- **Dependencies** — upstream/downstream graph visualization
- **Multi-region** — locations management, per-location status, consensus
- **Status pages** — create and manage named monitor collections
- **SVG icons** — custom monitor type and status icons
- **Dark/light theme** — system preference detection, manual toggle, localStorage persistence
- **Mobile responsive** — hamburger menu, touch-friendly

## License

MIT
