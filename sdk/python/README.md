# Watchpost Python SDK

Zero-dependency Python client for the [Watchpost](https://github.com/Humans-Not-Required/watchpost) monitoring API.

Works with **Python 3.8+** using only the standard library. No pip install needed — just copy `watchpost.py` into your project.

## Quick Start

```python
from watchpost import Watchpost

wp = Watchpost("http://localhost:3007")

# Create a monitor (save the manage_key!)
mon = wp.create_monitor("My API", "https://api.example.com/health", is_public=True)
print(f"Monitor: {mon['id']}")
print(f"Key: {mon['manage_key']}")  # Save this!

# Check status
status = wp.get_monitor(mon["id"])
print(f"Status: {status['current_status']}")

# Get uptime stats
uptime = wp.get_uptime(mon["id"])
print(f"24h: {uptime['uptime_24h']}%")
```

## Features

### Monitor Management

```python
# Create monitors (HTTP, TCP, DNS)
http = wp.create_monitor("Web", "https://example.com/health")
tcp = wp.create_monitor("Database", "db.example.com:5432", monitor_type="tcp")
dns = wp.create_monitor("DNS", "example.com", monitor_type="dns", dns_record_type="A")

# With all options
mon = wp.create_monitor(
    "Production API",
    "https://api.example.com/health",
    is_public=True,
    tags=["prod", "api"],
    group_name="Production",
    sla_target=99.9,
    response_time_threshold_ms=2000,
    confirmation_threshold=3,
)

# List, filter, search
all_monitors = wp.list_monitors()
prod = wp.list_monitors(tag="prod")
down = wp.list_monitors(status="down")
apis = wp.list_monitors(search="api")

# Update
wp.update_monitor(mon["id"], mon["manage_key"], name="Updated Name")

# Pause/resume
wp.pause_monitor(mon["id"], mon["manage_key"])
wp.resume_monitor(mon["id"], mon["manage_key"])

# Bulk create (up to 50)
result = wp.bulk_create_monitors([
    {"name": "Service A", "url": "https://a.example.com/health"},
    {"name": "Service B", "url": "https://b.example.com/health"},
])

# Cleanup
wp.delete_monitor(mon["id"], mon["manage_key"])
```

### Uptime & SLA

```python
# Uptime stats
uptime = wp.get_uptime(monitor_id)
print(f"24h: {uptime['uptime_24h']}%, 7d: {uptime['uptime_7d']}%")

# Daily history
history = wp.get_uptime_history(monitor_id, days=30)

# Aggregate across all monitors
aggregate = wp.get_uptime_history(days=7)

# SLA tracking (requires sla_target on monitor)
sla = wp.get_sla(monitor_id)
print(f"Status: {sla['status']}")  # met, at_risk, breached
print(f"Budget remaining: {sla['budget_remaining_seconds']}s")
```

### Incidents

```python
# List incidents
incidents = wp.list_incidents(monitor_id)

# Get incident detail
incident = wp.get_incident(incident_id)

# Acknowledge
wp.acknowledge_incident(incident_id, manage_key, note="Looking into it", acknowledged_by="Nanook")

# Investigation notes
wp.add_incident_note(incident_id, "Root cause: DNS timeout", manage_key, author="Nanook")
notes = wp.list_incident_notes(incident_id)
```

### Notifications

```python
# Webhook notification
wp.create_notification(
    monitor_id, "Slack Alert", "webhook",
    {"url": "https://hooks.slack.com/..."},
    manage_key,
)

# Chat-format webhook (for Local Agent Chat)
wp.create_notification(
    monitor_id, "Chat Alert", "webhook",
    {"url": "http://chat:3006/api/v1/hook/TOKEN", "payload_format": "chat"},
    manage_key,
)

# Email notification
wp.create_notification(
    monitor_id, "Ops Email", "email",
    {"address": "ops@example.com"},
    manage_key,
)

# Delivery audit log
deliveries = wp.list_webhook_deliveries(monitor_id, manage_key)
```

### Maintenance Windows

```python
wp.create_maintenance(
    monitor_id,
    "Deploy v2.0",
    "2026-02-20T14:00:00Z",
    "2026-02-20T15:00:00Z",
    manage_key,
)
windows = wp.list_maintenance(monitor_id)
```

### Alert Rules

```python
wp.set_alert_rules(
    monitor_id, manage_key,
    repeat_interval_minutes=15,
    max_repeats=5,
    escalation_after_minutes=30,
)
rules = wp.get_alert_rules(monitor_id, manage_key)
log = wp.list_alert_log(monitor_id, manage_key)
```

### Dependencies

```python
# Database → API → Web App
wp.add_dependency(api_id, db_id, api_key)       # API depends on DB
wp.add_dependency(web_id, api_id, web_key)       # Web depends on API
# When DB is down, API and Web alerts are suppressed

deps = wp.list_dependencies(api_id)
dependents = wp.list_dependents(db_id)
```

### Status Pages

```python
# Create a custom status page
page = wp.create_status_page("production", "Production Status",
    description="Public-facing service availability")

# Add monitors
wp.add_monitors_to_page("production", [mon1_id, mon2_id], page["manage_key"])

# View
status = wp.get_status_page("production")
```

### Badges

```python
# SVG badges for READMEs
uptime_svg = wp.get_uptime_badge(monitor_id, period="7d")
status_svg = wp.get_status_badge(monitor_id)

# With custom label
svg = wp.get_uptime_badge(monitor_id, period="30d", label="My Service")
```

### SSE Real-time Events

```python
# Stream all events
for event in wp.stream_events():
    data = event.json
    print(f"[{event.event}] {data}")

# Per-monitor events
for event in wp.stream_events(monitor_id=monitor_id):
    if event.event == "incident.created":
        print(f"🔴 Incident: {event.json}")
```

### Convenience Helpers

```python
# Quick checks
wp.is_up(monitor_id)       # True/False
wp.all_up()                 # All public monitors up?
wp.wait_for_up(monitor_id, timeout=300)  # Block until up

# Downtime summary
summary = wp.get_downtime_summary(monitor_id)
# {"is_down": False, "current_status": "up", "uptime_24h": 99.9, ...}

# Quick monitor creation
mon = wp.quick_monitor("My Service", "https://service.example.com/health")
```

## Error Handling

```python
from watchpost import (
    WatchpostError,    # Base class (catches all)
    NotFoundError,     # 404
    AuthError,         # 401/403
    ValidationError,   # 400/422
    ConflictError,     # 409
    RateLimitError,    # 429 (has retry_after attribute)
)

try:
    wp.get_monitor("nonexistent-id")
except NotFoundError as e:
    print(f"Not found: {e} (status={e.status_code})")
except WatchpostError as e:
    print(f"API error: {e} (status={e.status_code})")
```

## Configuration

```python
# Via constructor
wp = Watchpost("http://localhost:3007", timeout=60)

# Via environment variable
# export WATCHPOST_URL=http://monitoring.example.com:3007
wp = Watchpost()  # Uses WATCHPOST_URL
```

## Running Tests

```bash
# Against staging
WATCHPOST_URL=http://192.168.0.79:3007 python3 test_sdk.py

# Against local
python3 test_sdk.py
```

77 integration tests covering all API endpoints.

## Requirements

- Python 3.8+
- No external dependencies (stdlib only)

## License

MIT
