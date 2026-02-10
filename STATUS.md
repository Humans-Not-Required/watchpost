# Watchpost - Status

## Current State: Backend Skeleton + Core Monitor CRUD ✅ (early MVP)

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
- Test suite: **22 HTTP integration tests passing** (`cargo test -- --test-threads=1`)

### What's Next (Priority Order)

1. **Fix Rocket State types cleanup** — ensure all routes consistently use `State<Arc<Db>>` (currently working, but keep tidy)
2. **Webhook notifications** — on incident created/resolved, POST structured JSON to configured webhook channels
3. **SSE event stream** — per-monitor and global stream (status changes + heartbeat summaries)
4. **OpenAPI spec** (`/api/v1/openapi.json`) + consistent JSON error catchers
5. **React frontend** (unified serving) — monitors list, create/edit, incident timeline, status page
6. **Dockerfile + docker-compose** (port 3007 external) + staging deploy via ghcr.io + Watchtower

### ⚠️ Gotchas

- Tests should run with `--test-threads=1` (shared env vars / SQLite file path patterns)
- Checker currently uses a single DB connection mutex; fine for MVP, revisit for higher concurrency.

## Tech Stack

- Rust + Rocket 0.5
- SQLite (rusqlite bundled)
- HTTP checks via `reqwest` (rustls-tls)
