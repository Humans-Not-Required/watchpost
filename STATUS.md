# Watchpost - Status

## Current State: Deployed to Staging ✅

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
- **SSE event streams** ✅:
  - Global stream: `GET /api/v1/events` (all monitors)
  - Per-monitor stream: `GET /api/v1/monitors/:id/events`
  - Event types: `check.completed`, `incident.created`, `incident.resolved`
- **OpenAPI spec** ✅: `GET /api/v1/openapi.json`
- **JSON error catchers** ✅ (400, 401, 403, 404, 422, 429, 500)
- **React frontend** ✅ (commit b9d484c):
  - Dark theme, responsive design, consistent with HNR suite
  - **Status page:** Overall status banner, monitor cards with uptime/response/incident stats
  - **Monitor detail:** Config display, uptime bar visualization (last 60 checks), heartbeat table, incident list, tabs (Overview/Heartbeats/Incidents)
  - **Create monitor:** Full form with all options, manage key display on success with API reference
  - **Auto-refresh:** Status page + detail page poll every 30s
  - **SPA fallback** route for hash-based client-side routing
  - **CORS** via rocket_cors for dev mode
  - **Unified serving:** Frontend dist served from Rust binary (STATIC_DIR env configurable)
- **Dockerfile** ✅ (commit 75dd595):
  - Multi-stage: frontend (bun) → backend (rust:1-slim) → runtime (debian-slim)
  - Port 8000 internal, data volume at /app/data
  - Fixed: uses `rust:1-slim` (tracks latest stable) — `time@0.3.47` requires rustc 1.88+
- **docker-compose.yml** ✅: Port 3007 external, persistent volume (watchpost-data)
- **GitHub Actions CI** ✅: test → build → push to ghcr.io (:dev tag on main)
- **README** ✅ (commit 75dd595): Quick start, API reference, Docker usage, env config, architecture
- **Staging deploy** ✅:
  - Docker Compose on 192.168.0.79:3007
  - Nginx reverse proxy: watch.hnrstage.xyz
  - Watchtower auto-pull enabled
  - Added to backup-dbs.sh
  - Health: `curl http://192.168.0.79:3007/api/v1/health` ✅
- **Seq-based cursor pagination** ✅ (commit 456d21d):
  - Heartbeats + incidents now use `?after=<seq>` cursor instead of `?offset=`
  - Monotonic seq column with backfill migration
  - Default (no cursor) = newest first; with cursor = forward scan (ASC)
  - OpenAPI spec updated
- Test suite: **32 tests passing** (`cargo test -- --test-threads=1`) — includes 3 heartbeat retention tests + 2 notification toggle tests

### What's Next (Priority Order)

1. **Monitor search/filter** — Filter monitors on status page by name or status
2. **Monitor tags/groups** — Group monitors by tag for multi-service status pages
3. **Response time alerts** — Notify when response time exceeds threshold (slow but not down)

### ✅ Completed (most recent)

- **Response time chart + notification toggle** (commit e4d7708) — SVG response time chart on Overview tab (last 100 checks, avg line, nice axis ticks, color-coded dots, no external deps). PATCH /notifications/:id endpoint for enable/disable toggle. Toggle button in UI. OpenAPI updated. 32 tests (was 30).
- **Notification channel management UI** (commit 5239e11) — Add/list/delete webhook/email notification channels from the frontend. New "🔔 Notifications" tab visible when manage key present.
- **Inline edit monitor settings** (commit ca0a446) — Edit all monitor config fields (name, URL, method, interval, timeout, expected status, confirmation threshold, body contains, public/private) from UI with manage key. Only sends changed fields via PATCH.
- **Loading skeleton screens** (commit 488bd55) — Shimmer skeleton loading states for status page and monitor detail, replacing plain spinners
- **Heartbeat retention** (commit ab480d4) — Auto-prune heartbeats older than 90 days (configurable via HEARTBEAT_RETENTION_DAYS env var). Runs hourly in checker loop. 3 new tests.
- **DNS for watch.hnrstage.xyz** — Cloudflare wildcard resolves, HTTPS working
- **Manage key integration** (commit 6ac08cf) — Pause/Resume, Delete (with confirmation), Incident Acknowledgement from UI when `?key=` present

### ⚠️ Gotchas

- Tests should run with `--test-threads=1` (shared env vars / SQLite file path patterns)
- Checker currently uses a single DB connection mutex; fine for MVP, revisit for higher concurrency.
- CI workflow files may need `workflow` token scope to push — if CI doesn't trigger, manually add workflow via GitHub UI.
- Frontend uses hash-based routing (#/monitor/:id) — no server-side route matching needed beyond SPA fallback.
- Nginx on staging not managed by systemd (manual start). Reload via `sudo kill -HUP $(pgrep -f "nginx: master")`.

## Tech Stack

- Rust + Rocket 0.5 + rocket_cors
- SQLite (rusqlite bundled)
- HTTP checks via `reqwest` (rustls-tls)
- SSE via `rocket::response::stream` + `tokio::sync::broadcast`
- React + Vite (dark theme)
- Docker multi-stage build
