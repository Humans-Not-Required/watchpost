# Watchpost - Status

## Current State: Deployed to Staging ✅

**Watchpost** is an agent-native monitoring service (Uptime Kuma vibe) designed for AI agents.

### What's Done

- **Security hardening** (commit 7b734e7) — Comprehensive security pass across 22 source files. (1) Added `Db::conn()` method with mutex poison recovery — if any request panics while holding the DB lock, subsequent requests recover gracefully instead of propagating the panic. Replaced 72+ `db.conn.lock().unwrap()` calls across all route files, checker, consensus, and notification modules. (2) Fixed error information leakage: replaced 50+ instances of `e.to_string()` in error responses with generic "Internal server error" messages — prevents leaking SQL table names, column details, or query structures to API clients. (3) Converted `prepare().unwrap()` and `query_map().unwrap()` calls to graceful error handling in locations.rs, dependencies.rs, and the circular dependency checker. (4) Fixed RateLimiter mutex with poison recovery. Zero clippy warnings, all 315 tests passing.
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

1. ~~**Multi-region checks**~~ ✅ Done (backend, 2026-02-15) — Check locations CRUD + probe submission API + per-location status endpoint. 23 new tests (193 total).
2. ~~**Multi-region consensus**~~ ✅ Done (2026-02-15) — Configurable consensus_threshold, automatic evaluation after probes and local checks, incident lifecycle integration, consensus API endpoint. 15 new tests (208 total).
3. ~~**Multi-region frontend**~~ ✅ Done (2026-02-15) — Locations management page (admin CRUD with probe key reveal), Regions tab on MonitorDetail (per-location status cards, consensus summary with threshold/status bar/location breakdown), consensus_threshold field on Create/Edit forms.
4. ~~**Custom domain support**~~ ✅ Done (2026-02-15) — Status Pages: named monitor collections with branding and custom domains. 28 new tests (236 total).
5. ~~**Alerting rules / escalation**~~ ✅ Done (2026-02-15) — Alert rules per monitor (repeat notifications every N minutes, max repeats cap, escalation if unacknowledged). Alert audit log with filters. Checker integration for incident.reminder and incident.escalated events. 18 new tests (254 total).
6. ~~**Frontend alert rules UI**~~ ✅ Done (2026-02-15) — AlertRulesManager component: create/edit/delete alert rules, alert log table with type badges. New "Alerts" tab on monitor detail (manage key required).

### Future Improvements (if revisiting)
- ~~Frontend status page manager~~ ✅ Done (2026-02-15) — Edit page settings, delete with confirmation, add/remove monitors via searchable picker, manage key input with localStorage persistence. Full CRUD now available through frontend.
- ~~Webhook delivery retry with exponential backoff~~ ✅ Done (2026-02-15) — 3 attempts, 2s/4s backoff, delivery audit log API.
- ~~Monitor dependency chains~~ ✅ Done (2026-02-15) — CRUD API + circular detection + checker alert suppression. 18 new tests.
- ~~Probe agent health tracking~~ ✅ Done (2026-02-15) — Auto-disable stale locations, health_status field, PROBE_STALE_MINUTES env var, frontend health badges. 6 new tests.
- ~~Frontend dependency management UI~~ ✅ Done (2026-02-15) — Dependencies tab with searchable monitor picker, upstream/downstream lists with status badges, dependency graph visualization, add/remove with manage key auth, alert suppression indicator.
- ~~Dark/light theme toggle~~ ✅ Done (2026-02-16)

### ⚠️ Jordan's Questions
- ~~**Task ef781225:** Jordan asked "What is this about?"~~ — Stale, no further context. Board manager should close if no update.
- ~~**Task b446f607 (Follow 301 redirects):** Completed (commit a7fc268). Monitors follow redirects by default.~~

- Test suite: **323 Rust tests passing** (307 HTTP + 16 unit) + **180 Python SDK integration tests** — parallel-safe, includes 15 DNS monitor tests, 11 TCP monitor tests, 8 settings/branding tests, 9 monitor group tests, 7 badge tests, 9 dashboard + admin verify tests, 7 maintenance window tests, 4 response time alert tests, 9 tag tests (incl. flexible format), 3 search/filter tests, 3 heartbeat retention tests, 5 notification tests (CRUD + chat format), 5 follow_redirects tests, 13 validation/coverage tests, 7 email notification tests, 13 incident notes tests, 23 multi-region location tests, 15 consensus tests, 28 status pages tests, 18 alert rule tests, 11 webhook delivery tests, 18 dependency tests, 6 probe health tests, 8 chat message format unit tests. SDK tests cover: advanced monitor updates, cursor pagination, combined filters, status page lifecycle, dashboard auth variants, admin verify, settings auth, location/probe auth, alert rule validation, dependency advanced, maintenance edge cases, notification types, webhook delivery filters, export/import roundtrip, bulk create edge cases, incident/note not-found handling, badge variants, delete cascade, error hierarchy

### ✅ Completed (most recent)

- **Chat-format webhook notifications** (2026-02-17) — Cross-service alert integration. Add `payload_format: "chat"` to webhook config to send simple `{"content":"...","sender":"Watchpost"}` payloads instead of structured JSON. Compatible with Local Agent Chat incoming webhooks, Slack, and other chat systems. Chat messages include event emoji, monitor name, event type, cause, and resolution time. Created #alerts room in Local Agent Chat with incoming webhook. Setup script: `scripts/setup-watchpost-chat-alerts.sh`. 11 new tests (8 unit + 3 HTTP). 315 total. Commit: c57bb43.
- **Checker resilience + flexible tag format** (2026-02-17) — Replaced `.expect()` with graceful error handling in checker task initialization (prevents silent tokio task death). Added startup diagnostic logging, per-check logging, and task exit monitoring. Reduced warmup delay from 30s to 10s. Tags now accept both JSON arrays and comma-separated strings (fixes 422 errors from string tags). 3 new tests (304 total). Commit: 77b9e57.
- **Webhook delivery retry** (2026-02-15) — Webhook notifications now retry up to 3 times with exponential backoff (2s, 4s delays). Every attempt logged to `webhook_deliveries` table with status, status_code, error, response_time_ms. `delivery_group` UUID groups retries. New `GET /monitors/:id/webhook-deliveries` endpoint (manage key auth) with limit, cursor, event, and status filters. CASCADE delete on monitor removal. Fixed clippy: double-reference on broadcaster, struct for delivery log params. 11 new tests (265 total). Updated llms.txt, OpenAPI spec, DESIGN.md.
- **Monitor dependency chains** (2026-02-15) — CRUD API for defining upstream dependencies between monitors. POST/GET/DELETE /monitors/:id/dependencies, GET /monitors/:id/dependents (reverse lookup). Validation: no self-dependency, no circular chains (BFS graph walk), both monitors must exist, no duplicates. DB: `monitor_dependencies` table with CASCADE delete on both sides. Checker integration: when a monitor transitions to "down", checks if any dependency is currently down — if so, suppresses incident creation and notifications while still recording heartbeats honestly. When dependency recovers, creates delayed incident if monitor is still down. Recovery from suppressed states handled cleanly. Updated llms.txt, OpenAPI spec (4 new paths), DESIGN.md. 18 new tests (283 total).
- **Frontend dependency management UI** (2026-02-15) — Dependencies tab on MonitorDetail with full CRUD for upstream dependencies. Searchable monitor picker (filters already-added and self), status badges with colored indicators, alert suppression warnings when upstream is down. Reverse lookup section shows dependents. Dependency graph visualization: upstream → current → downstream with color-coded status chips. Add/remove requires manage key. Icons: new IconGitBranch SVG component.
- **Probe agent health tracking** (2026-02-15) — Auto-disable stale check locations. `health_status` computed field on location responses: "healthy" (active, recently reported), "new" (never reported), "stale" (active but overdue), "disabled" (inactive). Checker loop runs stale detection every 5 minutes. `PROBE_STALE_MINUTES` env var (default 30). `disable_stale_locations()` function. Frontend Locations page shows health status badges (colored: green/blue/yellow/red). 6 new tests (289 total).
- **Frontend alert rules UI** (2026-02-15) — AlertRulesManager component: create/edit/delete alert rules per monitor (repeat notifications, max repeats, escalation policies). Alert log table with color-coded type badges (initial/reminder/escalation/resolved), event details, and timestamps. New "Alerts" tab on MonitorDetail (visible when manage key present). Form validation matches backend (min 5m intervals, max 100 repeats). Empty state with setup CTA, confirmation on delete.
- **Privacy redesign** (2026-02-16) — Dashboard now requires admin key for individual monitor data (recent incidents, slowest monitors). Without auth, only aggregate stats returned. Default landing page changed to public Status Page. Admin-only nav items (New Monitor, Bulk Import, Locations, Pages) hidden for unauthenticated users. Admin key persisted in localStorage with sign-out. New `GET /admin/verify` endpoint. `OptionalManageToken` request guard added. 6 new tests (295 total). Commit: ac439cc.
- **Dark/light theme toggle** (2026-02-16) — Full theme system with CSS custom properties. Sun/moon toggle in header. localStorage persistence with prefers-color-scheme system preference fallback. Flash prevention (theme applied before React render). Replaced all hardcoded hex colors across 6 files (chart SVGs, inline styles, logo, skeletons, buttons) with CSS variables. Light theme: warm grays (#f4f5f7), softer accents (#00b894), proper contrast. Fixed stale --card-bg references. Commit: cac89d5.
- **Status Pages** (2026-02-15) — Named monitor collections with branding and custom domains. Full CRUD API: POST/GET/PATCH/DELETE /api/v1/status-pages, plus monitor assignment endpoints (POST/GET/DELETE /api/v1/status-pages/:slug/monitors). Per-page manage_key auth (same pattern as monitors). Slug-based access with optional custom_domain (UNIQUE). Detail endpoint returns monitor status data (uptime, response time, incidents, overall status). Frontend: StatusPages component with list/create/detail views, create form with slug auto-sanitization, detail view with grouped monitors and overall status banner, Pages nav button. DB tables: status_pages + status_page_monitors (CASCADE both sides). 28 new tests (236 total). Updated DESIGN.md, llms.txt, OpenAPI spec.
- **SLA tracking** — Per-monitor uptime targets with error budget tracking. New `sla_target` (0-100%) and `sla_period_days` (1-365, default 30) fields on monitors. New `GET /api/v1/monitors/:id/sla` endpoint returns target, current uptime, error budget (total/remaining/used %), and status (met/at_risk/breached). Degraded checks count as successful. Set via create/update/bulk create, clear via null. Frontend: SLA fields on create/edit forms, SLA card on monitor detail with error budget bar (color-coded progress), status badge. 14 new tests (157 total). Full docs: llms.txt, OpenAPI, DESIGN.md.
- **Multi-region frontend** (2026-02-15) — Locations management page with admin CRUD (probe key reveal on creation, admin key persistence). Regions tab on monitor detail: per-location status cards (status badge, response time, last checked), consensus visualization (threshold indicator, status counts, consensus bar, effective status badge). Consensus threshold field added to both Create and Edit monitor forms. Consensus badge in monitor stat bar when configured. 5 new API client functions.
- **Multi-region consensus** (2026-02-15) — Configurable `consensus_threshold` on monitors determines when to declare down based on multiple location reports. When set, the local checker and probe submissions evaluate consensus after each check. `GET /api/v1/monitors/:id/consensus` returns threshold, per-location status breakdown, and aggregate effective status. Full incident lifecycle: incidents created when consensus reaches threshold, resolved when it recovers. SSE events and webhook/email notifications fire on consensus transitions. Checker writes heartbeats but defers status changes to the consensus evaluator when enabled. Updated: models (create/update/bulk/export), DB migration, OpenAPI spec, llms.txt, DESIGN.md. 15 new tests (208 total).
- **Multi-region check locations (backend)** (2026-02-15) — Remote check locations for distributed monitoring. `POST /locations` (admin key auth, returns probe_key), `GET /locations`, `GET /locations/:id`, `DELETE /locations/:id`. `POST /probe` submits check results from remote agents (probe_key auth, up to 100 per batch, partial success). `GET /monitors/:id/locations` returns per-location latest status. Heartbeats include `location_id` (null for local checker). DB: `check_locations` table + `heartbeats.location_id` column. Updated llms.txt, OpenAPI spec, DESIGN.md. 23 new tests (193 total).
- **Frontend incident notes UI + per-monitor uptime history chart** (commits 7cfb463, 933e8a6) — IncidentNotes component: vertical timeline with dot markers, author, timestamps, collapsible "Notes (N)" toggle on each incident card. Add note form with author + content (manage key required). Notes count refreshes after adding. MonitorUptimeHistoryChart on overview tab: daily uptime area chart with 7d/14d/30d/90d range selector, gap-aware segments, color-coded by uptime level, tooltip with date/uptime/checks/response time. API functions added: getIncidentNotes, createIncidentNote, getIncident, getMonitorUptimeHistory (frontend).
- **Incident notes + checker refactor** (commit 1eeee5f) — Investigation timeline for incidents: POST /incidents/:id/notes (add note, auth), GET /incidents/:id/notes (list chronologically, public), GET /incidents/:id (single incident detail with notes_count). Validation: content 1-10K chars, author 1-200 chars, default "anonymous". CASCADE delete on monitor/incident removal. Also refactored checker.rs: extracted shared CheckResult struct + process_check_result() + resolve_transition() — all 3 check types (HTTP/TCP/DNS) now share a single incident lifecycle path. Eliminated ~190 lines of duplicated code (855 → 665 lines). 13 new tests (170 total). Docs: DESIGN.md, llms.txt, OpenAPI spec updated.
- **Backend route decomposition** (commit 2a0338c) — Monolithic 2963-line `src/routes.rs` split into 14 focused module files under `src/routes/`. Shared types (RateLimiter), helpers (get_monitor_from_db, verify_manage_key, validators) in mod.rs; domain routes in individual files (monitors, heartbeats, incidents, dashboard_route, uptime, status, notifications, maintenance, tags, settings, system, badges, stream). Static content (llms.txt, openapi.json) extracted to `static/` and loaded via `include_str!`. No file over 604 lines. Zero functional changes. All 143 tests pass. Zero warnings.
- **CI fix: static/ in Docker build** (commit 6cd8505) — Added `COPY static/ static/` to Dockerfile backend build stage. Required after route decomposition moved `include_str!` paths for llms.txt and openapi.json.
- **DNS health checks** (commit ff58548) — New `monitor_type: 'dns'` for DNS record resolution monitoring. URL = hostname to resolve (e.g., 'example.com' or 'dns://example.com'). `dns_record_type`: 10 types (A, AAAA, CNAME, MX, TXT, NS, SOA, PTR, SRV, CAA). `dns_expected`: optional value matching (case-insensitive, trailing dot ignored). Full incident lifecycle. Frontend: 3-way monitor type toggle (HTTP/TCP/DNS), DNS-specific fields (record type dropdown, expected value), detail view shows DNS info. DB migration adds dns_record_type + dns_expected columns. Updated OpenAPI spec + llms.txt + DESIGN.md. 15 new tests (143 total).
- **TCP health checks** (commit 42b522d) — New `monitor_type` field: 'http' (default) or 'tcp'. TCP monitors check port connectivity (connect to host:port with timeout). URL format: 'host:port' or 'tcp://host:port'. Full incident lifecycle (down/degraded/maintenance). Frontend: monitor type toggle on create form, conditional fields (HTTP-specific fields hidden for TCP), TCP indicator in detail view. DB migration adds monitor_type column with 'http' default. Updated OpenAPI spec + llms.txt. 11 new tests (128 total).
- **Status page branding** (commit 74ec06e) — Custom title, description, and logo for the public status page. Settings table with auto-generated admin key (printed on first run). GET /api/v1/settings (public), PUT /api/v1/settings (admin key auth). Empty string clears fields, partial updates supported. Branding included in GET /api/v1/status response. Frontend renders branding header above status banner. OpenAPI spec + llms.txt updated. 8 new tests (117 total).
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

- **Python SDK** — Complete zero-dependency Python client library (`sdk/python/watchpost.py`) wrapping all 60+ API endpoints. Features: typed error hierarchy (NotFoundError, AuthError, ValidationError, ConflictError, RateLimitError), response normalization (flattened create responses), SSE streaming, convenience helpers (is_up, all_up, wait_for_up, get_downtime_summary, quick_monitor). 180 integration tests (`test_sdk.py`). README with examples for every feature category.

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
