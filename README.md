# Watchpost

Agent-native monitoring service. Think Uptime Kuma, but designed for AI agents.

Monitors HTTP endpoints, tracks uptime, detects incidents, and sends structured alerts — all via REST API. Comes with a clean dashboard for humans too.

## Why Agent-First?

- **No signup.** Create a monitor, get a token. Done.
- **Structured JSON everywhere.** Agents parse responses, not scrape HTML.
- **SSE event streams.** Subscribe to real-time status changes.
- **Webhook notifications.** Structured JSON payloads on incidents.
- **Self-describing.** OpenAPI spec, llms.txt, consistent error codes.
- **Per-resource auth.** Tokens tied to monitors, not user accounts.

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
curl -X POST http://localhost:3007/api/v1/monitors \
  -H "Content-Type: application/json" \
  -d '{
    "name": "My API",
    "url": "https://api.example.com/health",
    "interval_seconds": 60,
    "is_public": true
  }'
```

Response includes a `manage_key` — save it. It's shown once and required for updates/deletes.

### Check Status

```bash
# Public status page (all public monitors)
curl http://localhost:3007/api/v1/status

# Single monitor detail
curl http://localhost:3007/api/v1/monitors/{id}

# Uptime stats
curl http://localhost:3007/api/v1/monitors/{id}/uptime

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
  -d '{"interval_seconds": 30}'

# Pause/Resume
curl -X POST http://localhost:3007/api/v1/monitors/{id}/pause \
  -H "Authorization: Bearer {manage_key}"

curl -X POST http://localhost:3007/api/v1/monitors/{id}/resume \
  -H "Authorization: Bearer {manage_key}"

# Delete
curl -X DELETE http://localhost:3007/api/v1/monitors/{id} \
  -H "Authorization: Bearer {manage_key}"
```

### Webhook Notifications

```bash
curl -X POST http://localhost:3007/api/v1/monitors/{id}/notifications \
  -H "Authorization: Bearer {manage_key}" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "My Webhook",
    "channel_type": "webhook",
    "config": {"url": "https://example.com/webhook"}
  }'
```

Webhooks fire on incident creation and resolution with structured JSON payloads.

### Real-Time Events (SSE)

```bash
# All public monitors
curl -N http://localhost:3007/api/v1/events

# Single monitor
curl -N http://localhost:3007/api/v1/monitors/{id}/events
```

Event types: `check.completed`, `incident.created`, `incident.resolved`

### Incidents

```bash
# List incidents for a monitor
curl http://localhost:3007/api/v1/monitors/{id}/incidents

# Acknowledge an incident
curl -X POST http://localhost:3007/api/v1/incidents/{id}/acknowledge \
  -H "Authorization: Bearer {manage_key}" \
  -H "Content-Type: application/json" \
  -d '{"note": "Looking into it", "acknowledged_by": "nanook"}'
```

## API Reference

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | /api/v1/monitors | ❌ | Create monitor (returns manage_key) |
| GET | /api/v1/monitors | ❌ | List public monitors |
| GET | /api/v1/monitors/:id | ❌ | Monitor details + status |
| PATCH | /api/v1/monitors/:id | 🔑 | Update monitor |
| DELETE | /api/v1/monitors/:id | 🔑 | Delete monitor |
| POST | /api/v1/monitors/:id/pause | 🔑 | Pause checks |
| POST | /api/v1/monitors/:id/resume | 🔑 | Resume checks |
| GET | /api/v1/monitors/:id/heartbeats | ❌ | Check history |
| GET | /api/v1/monitors/:id/uptime | ❌ | Uptime stats (24h/7d/30d/90d) |
| GET | /api/v1/monitors/:id/incidents | ❌ | Incident history |
| POST | /api/v1/incidents/:id/acknowledge | 🔑 | Acknowledge incident |
| POST | /api/v1/monitors/:id/notifications | 🔑 | Add notification channel |
| GET | /api/v1/monitors/:id/notifications | 🔑 | List notification channels |
| DELETE | /api/v1/notifications/:id | 🔑 | Remove notification channel |
| GET | /api/v1/status | ❌ | Public status overview |
| GET | /api/v1/events | ❌ | Global SSE event stream |
| GET | /api/v1/monitors/:id/events | ❌ | Per-monitor SSE events |
| GET | /api/v1/health | ❌ | Health check |
| GET | /api/v1/openapi.json | ❌ | OpenAPI spec |
| GET | /api/v1/llms.txt | ❌ | AI agent discovery |

Auth: `Authorization: Bearer {key}`, `X-API-Key: {key}`, or `?key={key}`

## Configuration

Environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| ROCKET_ADDRESS | 0.0.0.0 | Bind address |
| ROCKET_PORT | 8000 | Server port |
| DATABASE_PATH | watchpost.db | SQLite database path |
| STATIC_DIR | frontend/dist | Frontend static files |
| MONITOR_RATE_LIMIT | 10 | Max monitor creates per hour per IP |
| HEARTBEAT_RETENTION_DAYS | 90 | Auto-prune heartbeats older than N days |

## Tech Stack

- **Backend:** Rust + Rocket 0.5 + SQLite (rusqlite bundled)
- **Frontend:** React + Vite
- **HTTP checks:** reqwest (rustls-tls)
- **Real-time:** SSE via Rocket streams + tokio broadcast
- **Deployment:** Docker multi-stage build

## How It Works

A background checker runs on a schedule. For each active monitor:

1. Sends an HTTP request with the configured method, headers, and timeout
2. Evaluates the response: status code, optional body match, response time
3. Records a heartbeat (up/down/degraded + latency)
4. After consecutive failures exceed the confirmation threshold, creates an incident
5. Fires webhook notifications and SSE events on status changes
6. Auto-resolves incidents when the monitor recovers

## License

MIT
