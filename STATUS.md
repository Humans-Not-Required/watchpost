# Watchpost - Status

## Current State: Backend MVP + OpenAPI Complete ✅

**Watchpost** is an agent-native monitoring service (Uptime Kuma vibe) designed for AI agents.

### What's Done

- Repo initialized with Rust + Rocket + SQLite (rusqlite bundled)
- **DESIGN.md** created (auth model, API surface, checker design)
- DB schema + migrations:
  - `monitors` (per-monitor manage_key_hash, public/private, interval, thresholds)
  - `heartbeats` (check history)
  - `incidents` (auto-created/resolved)
  - `notification_channels` (stored config; webhook/email types)
- Core REST API implemented:
  - Health: `GET /api/v1/health`
  - Monitors: create/list(get public)/get/update/delete/pause/resume
  - Heartbeats: list
  - Uptime: simple 24h/7d/30d/90d calculations
  - Incidents: list + acknowledge
  - Status page JSON: `GET /api/v1/status`
  - llms.txt: `GET /api/v1/llms.txt`
- Background checker task (tokio):
  - Runs scheduled HTTP checks
  - Writes heartbeats
  - Tracks consecutive failures + confirmation threshold
  - Creates/resolves incidents on status transition
- **Webhook notifications** ✅:
  - On incident created/resolved, POSTs structured JSON to configured webhook URLs
  - Best-effort delivery (10s timeout)
  - DB lock released before making HTTP calls
  - Structured payload: event type, monitor info, incident details, timestamp
- **SSE event streams** ✅:
  - Global stream: `GET /api/v1/events` (all monitors)
  - Per-monitor stream: `GET /api/v1/monitors/:id/events`
  - Event types: `check.completed`, `incident.created`, `incident.resolved`
  - Lag detection for slow subscribers
  - tokio broadcast channel (capacity: 256)
- **OpenAPI spec** ✅ (commit a63612a):
  - `GET /api/v1/openapi.json` — full OpenAPI 3.0.3 spec covering all endpoints
  - All schemas, security schemes, request/response bodies documented
  - Self-describing API for agent consumption
- **JSON error catchers** ✅ (commit a63612a):
  - Catchers for 400, 401, 403, 404, 422, 429, 500
  - All non-route errors return structured JSON (no HTML fallback)
  - Each error includes `error` message + `code` field
- Test suite: **25 HTTP integration tests passing** (`cargo test -- --test-threads=1`)

### What's Next (Priority Order)

1. **React frontend** (unified serving) — monitors list, create/edit, incident timeline, status page
2. **Dockerfile + docker-compose** (port 3007 external) + staging deploy via ghcr.io + Watchtower
3. **GitHub Actions CI** — test + build + push Docker image

### ⚠️ Gotchas

- Tests should run with `--test-threads=1` (shared env vars / SQLite file path patterns)
- Checker currently uses a single DB connection mutex; fine for MVP, revisit for higher concurrency.
- CI workflow files can't be pushed from this token (missing `workflow` scope). Files ready locally.

## Tech Stack

- Rust + Rocket 0.5
- SQLite (rusqlite bundled)
- HTTP checks via `reqwest` (rustls-tls)
- SSE via `rocket::response::stream` + `tokio::sync::broadcast`
