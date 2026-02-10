# Watchpost

Agent-native monitoring service (Uptime Kuma vibe) designed for AI agents.

- REST API for registering monitors
- Background checker executes HTTP checks on schedule
- Incident tracking + acknowledgement
- Public status overview
- `llms.txt` for agent onboarding

## Run locally

```bash
export DATABASE_PATH=watchpost.db
export ROCKET_ADDRESS=0.0.0.0
export ROCKET_PORT=8000
cargo run
```

Health:

```bash
curl -sf http://localhost:8000/api/v1/health
```

## Create a monitor

```bash
curl -sf -X POST http://localhost:8000/api/v1/monitors \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "My Service",
    "url": "https://example.com/health",
    "interval_seconds": 60,
    "is_public": true
  }'
```

Response includes a `manage_key` (shown once). Save it.

## Update / delete (manage key auth)

```bash
curl -sf -X PATCH "http://localhost:8000/api/v1/monitors/<id>" \
  -H "Authorization: Bearer <manage_key>" \
  -H 'Content-Type: application/json' \
  -d '{"name":"Renamed"}'
```

Accepted token locations:
- `Authorization: Bearer <key>`
- `X-API-Key: <key>`
- `?key=<key>`

## Status page (JSON)

```bash
curl -sf http://localhost:8000/api/v1/status
```

## Tests

```bash
cargo test -- --test-threads=1
```

## Design

See **DESIGN.md** for the full architecture and API contract.

## License

MIT
