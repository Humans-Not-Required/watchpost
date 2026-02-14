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
  - Nginx reverse proxy: <staging-domain>
  - Watchtower auto-pull enabled
  - Added to backup-dbs.sh
  - Health: `curl http://192.168.0.79:3007/api/v1/health` ✅
- **Seq-based cursor pagination** ✅ (commit 456d21d):
  - Heartbeats + incidents now use `?after=<seq>` cursor instead of `?offset=`
  - Monotonic seq column with backfill migration
  - Default (no cursor) = newest first; with cursor = forward scan (ASC)
  - OpenAPI spec updated
### What's Next (Priority Order)

1. **Multi-region checks** — Check from multiple locations, consensus-based status
2. **Status page branding** — Custom title, description, logo for the status page
3. **Custom domain support** — Allow monitors to specify custom status page domain

### ⚠️ Jordan's Questions
- ~~**Task ef781225:** Jordan asked "What is this about?"~~ — Stale, no further context. Board manager should close if no update.
- ~~**Task b446f607 (Follow 301 redirects):** Completed (commit a7fc268). Monitors follow redirects by default.~~

- Test suite: **109 tests passing** (`cargo test`) — parallel-safe, includes 7 badge tests, 3 dashboard tests, 7 maintenance window tests, 4 response time alert tests, 6 tag tests, 3 search/filter tests, 3 heartbeat retention tests, 2 notification toggle tests, 5 follow_redirects tests, 13 validation/coverage tests, 7 email notification tests

### ✅ Completed (most recent)

- **Monitor groups** (commit c33dcf6) — `group_name` field on monitors for organizing into sections on the status page. Full-stack: DB migration, create/update/bulk-create support, GET /groups endpoint, ?group= filter on list + status endpoints, frontend grouped sections with headers and filter chips, group input on create/edit forms, group badge on detail view. OpenAPI spec + llms.txt updated. 9 new tests (109 total).
- **Email notifications** (commit ddd9b02) — SMTP-based email alerts via lettre. Config: SMTP_HOST/PORT/USERNAME/PASSWORD/FROM/TLS env vars. Sends multipart HTML + plain text emails on incident.created, incident.resolved, monitor.degraded, monitor.recovered, maintenance.started, maintenance.ended events. Dark-themed HTML email template. Graceful no-op when SMTP not configured. Frontend fix: email channels now correctly send `{address}` config and display email addresses in channel list. 7 new tests (100 total).
- **Custom SVG icons replacing emojis** (commit 1a6fd84) — Created Icons.jsx with 30+ hand-crafted SVG icon components using currentColor for seamless dark theme integration. Replaced all emojis across every page: nav bar, dashboard stat cards, status indicators, tabs, manage panel, badges, notifications, maintenance, create/bulk-import flows. Stroke-based, 16px default, inline-flex aligned. 93 tests passing.
- **Mobile hamburger menu** — Replaced wrapping nav links with a hamburger menu on mobile (<640px). Three-line icon animates to X when open. Dropdown menu slides below header with full-width nav buttons. Closes on navigation, outside click, or toggle. Desktop layout unchanged. 93 tests passing.
- **Fix broken uptime history chart** (commit e022e6a) — Fixed 3 bugs: (1) Y-axis generated 38+ cramped ticks when uptime dipped low — now uses niceStep() for ~4-8 clean labels; (2) Chart didn't fill missing dates, so 30d/90d range selectors showed same 2 data points — now fills full timeline with "no data" zones; (3) Y-axis always started at 90%, cramping wide ranges — now switches to 0-100% when data dips below 90%. Also added gap-aware line segments and tooltip overflow prevention. 93 tests passing.
- **Mobile responsive 2x2 uptime stats** (commit 2fcf7b0) — Uptime stat boxes (24h/7d/30d/90d) on monitor detail now display as a 2x2 grid on mobile screens (<640px) instead of overflowing. Added `.uptime-stats-grid` CSS class with media query. Skeleton loading also responsive. 93 tests passing.
- **Shareable manage links + private-by-default** (commit 9f049c1) — Redesigned manage UI: monitors now default to private (is_public: false). After creation, shows a bookmarkable manage URL (`#/monitor/<id>?key=<key>`) instead of raw key. Keys auto-saved to localStorage on creation and URL access. "🔗 Copy Link" button in manage panel. Bulk import auto-saves all keys and shows manage links. Raw key still accessible via expandable `<details>`. Hash-based key parsing with backward compat for `?key=` query params.
- **Minimum check interval raised to 10 minutes** (commit d074cbf) — Changed minimum `interval_seconds` from 30 to 600 (10 minutes). Default changed from 300 to 600. Enforced in create, bulk create, and update handlers. Frontend forms updated with new min/default/help text. OpenAPI spec, llms.txt, DESIGN.md all updated. 93 tests passing.
- **Follow redirects** (commit a7fc268) — Monitors now follow HTTP 301/302/etc. redirects (up to 10 hops) by default. New `follow_redirects` boolean field on create/update (default: true). Set to false to check redirect responses directly (e.g. verify a 301 is in place). Two reqwest clients in checker (follow vs no-follow), selected per monitor. DB migration adds column. API, OpenAPI spec, and llms.txt updated. 5 new tests (93 total).
- **Manage key entry UI** (commit f4263a8) — New "🔑 Enter Manage Key" button on monitor detail page lets humans enter their manage key directly in the UI (no more manual `?key=` URL editing). Key validation via auth endpoint test. localStorage persistence per monitor ID (keys survive page reloads). 🔒 Lock button to clear saved key. Fallthrough priority: URL param > entered key > localStorage. type=password input for security.
- **URL scheme + headers validation** (commit a5ab954) — URL must start with http:// or https:// (create, bulk create, update). Headers must be a JSON object (create, bulk create, update). Updated OpenAPI spec and llms.txt with validation section. 13 new tests: URL scheme validation (single/bulk/update), headers validation (create with headers, must-be-object on create/update/bulk), POST/HEAD method, body_contains, interval/timeout/confirmation_threshold clamping. 88 tests total.
- **Status page badges** (commit 40c9479) — Two new SVG badge endpoints: `GET /api/v1/monitors/:id/badge/uptime` (shields.io-style uptime percentage, `?period=24h|7d|30d|90d`, `?label=`, color-coded by uptime level) and `GET /api/v1/monitors/:id/badge/status` (current status, color-coded). Frontend: new "🏷️ Badges" tab on monitor detail with live preview, period selector, and one-click copy for Markdown + HTML embed code. OpenAPI spec + llms.txt updated. 7 new tests (75 total).
- **Uptime history chart** (commit f52475d) — New `GET /api/v1/uptime-history?days=30` (aggregate) and `GET /api/v1/monitors/:id/uptime-history?days=30` (per-monitor) endpoints returning daily uptime percentages, check counts, and avg response times. Frontend: SVG area chart on dashboard with interactive tooltips, color-coded by uptime level, range selector (7d/14d/30d/90d), auto-scaling Y-axis. OpenAPI spec + llms.txt updated. 6 new tests (68 total).
- **Dashboard overview** (commit 3c88ba9) — New `GET /api/v1/dashboard` endpoint with aggregate stats: total/public/paused counts, status breakdown, active incidents, avg uptime 24h/7d, avg response time, recent 10 incidents (with monitor names), top 5 slowest monitors. React frontend: stat cards with color-coded values, horizontal status bar visualization, recent incidents list (clickable → monitor detail), slowest monitors ranking, auto-refresh 30s, responsive grid (4→2→2 col). Dashboard is now the default landing page (/ → dashboard, #/status → status). Nav updated with 📊 Dashboard tab. OpenAPI spec + llms.txt updated. 3 new tests (62 total).
- **Bulk import UI** (commit aa3d850) — New "📦 Bulk Import" page in nav bar. Paste JSON array or upload .json file to create up to 50 monitors at once. Client-side validation (name, url, method, interval, limit). Preview table before submission. Results view with manage keys table and "Copy All Keys as JSON" button. Handles partial failures (shows created + failed). Accepts both `[...]` and `{monitors: [...]}` formats. Uses existing bulk create API endpoint.
- **Maintenance window UI** (commit df53871) — New "🔧 Maintenance" tab on monitor detail page. Lists windows categorized as Active Now (warning badge), Upcoming (accent badge), and Completed (muted badge). Create form with datetime-local inputs that auto-convert to UTC for the API. Delete button with manage key auth. Tab bar now wraps on mobile (flexWrap). API functions added to frontend: getMaintenanceWindows, createMaintenanceWindow, deleteMaintenanceWindow.
- **Maintenance windows** (commit 7264b30) — `POST /api/v1/monitors/:id/maintenance` creates scheduled downtime windows. During active windows, checker suppresses incident creation and sets monitor status to "maintenance" instead of "down". Heartbeats still recorded. SSE events: `maintenance.started`, `maintenance.ended`. CRUD API with auth. Status page treats maintenance as operational. Cascade delete with monitors. Full validation (ISO-8601 timestamps, ordering). OpenAPI spec + llms.txt updated. 7 new tests (59 total).
- **Bulk monitor management** (commit 6677b11) — `POST /api/v1/monitors/bulk` creates up to 50 monitors in one request with partial success handling (some may fail while others succeed, each gets its own manage_key). `GET /api/v1/monitors/:id/export` exports config in importable format (requires auth). Full export→reimport roundtrip tested. OpenAPI spec + llms.txt updated. 7 new tests (52 total).
- **Response time alerts** (commit becb703) — Configurable per-monitor `response_time_threshold_ms` (nullable, min 100ms). Replaces hardcoded 5000ms degraded logic. When response time exceeds threshold, status set to "degraded" with descriptive error message. Fires `monitor.degraded` / `monitor.recovered` webhook + SSE events on transitions. Custom serde double-option deserializer for proper null handling (absent vs null vs value). Frontend: threshold field on create + edit forms, "RT Alert" displayed in monitor config. OpenAPI spec + llms.txt updated. 4 new tests (45 total).
- **Monitor tags** (commit ad6f94e) — Backend: `tags` column, create/update with tags array, `?tag=` filter on GET /monitors and GET /status, `GET /tags` endpoint (unique tags from public monitors). Frontend: tag filter chips on status page, tag badges on monitor cards (clickable to filter), tags input on create/edit forms. OpenAPI + llms.txt updated. 6 new tests (41 total).
- **Monitor search/filter** (commit 201977e) — Backend: `?search=` and `?status=` query params on GET /monitors and GET /status. Frontend: search bar + status filter chips with live counts. 3 new tests (35 total).
- **Response time chart + notification toggle** (commit e4d7708) — SVG response time chart on Overview tab (last 100 checks, avg line, nice axis ticks, color-coded dots, no external deps). PATCH /notifications/:id endpoint for enable/disable toggle. Toggle button in UI. OpenAPI updated. 32 tests (was 30).
- **Notification channel management UI** (commit 5239e11) — Add/list/delete webhook/email notification channels from the frontend. New "🔔 Notifications" tab visible when manage key present.
- **Inline edit monitor settings** (commit ca0a446) — Edit all monitor config fields (name, URL, method, interval, timeout, expected status, confirmation threshold, body contains, public/private) from UI with manage key. Only sends changed fields via PATCH.
- **Loading skeleton screens** (commit 488bd55) — Shimmer skeleton loading states for status page and monitor detail, replacing plain spinners
- **Heartbeat retention** (commit ab480d4) — Auto-prune heartbeats older than 90 days (configurable via HEARTBEAT_RETENTION_DAYS env var). Runs hourly in checker loop. 3 new tests.
- **DNS for <staging-domain>** — Cloudflare wildcard resolves, HTTPS working
- **Manage key integration** (commit 6ac08cf) — Pause/Resume, Delete (with confirmation), Incident Acknowledgement from UI when `?key=` present

### ⚠️ Gotchas

- Tests are now parallel-safe (each test gets its own DB via `test_client_with_db()`). No `--test-threads=1` needed.
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

## Incoming Directions (Work Queue)

<!-- WORK_QUEUE_DIRECTIONS_START -->
(All cleared — 12 directions triaged and confirmed completed 2026-02-14)
<!-- WORK_QUEUE_DIRECTIONS_END -->
