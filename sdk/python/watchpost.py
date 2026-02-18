#!/usr/bin/env python3
"""
watchpost — Python SDK for Watchpost Monitoring Service

Zero-dependency client library for the Watchpost API.
Works with Python 3.8+ using only the standard library.

Quick start:
    from watchpost import Watchpost

    wp = Watchpost("http://localhost:3007")

    # Create a monitor
    mon = wp.create_monitor("My API", "https://api.example.com/health")
    print(f"Monitor ID: {mon['id']}, Key: {mon['manage_key']}")

    # Check status
    status = wp.get_monitor(mon["id"])
    print(f"Status: {status['current_status']}")

    # Uptime
    uptime = wp.get_uptime(mon["id"])
    print(f"24h uptime: {uptime['uptime_24h']}%")

Full docs: GET /api/v1/llms.txt or /.well-known/skills/watchpost/SKILL.md
"""

from __future__ import annotations

import json
import os
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import (
    Any,
    Dict,
    Generator,
    Iterator,
    List,
    Optional,
    Tuple,
    Union,
)


__version__ = "1.0.0"


# ---------------------------------------------------------------------------
# Exceptions
# ---------------------------------------------------------------------------


class WatchpostError(Exception):
    """Base exception for Watchpost API errors."""

    def __init__(self, message: str, status_code: int = 0, body: Any = None):
        super().__init__(message)
        self.status_code = status_code
        self.body = body


class NotFoundError(WatchpostError):
    """Resource not found (404)."""
    pass


class AuthError(WatchpostError):
    """Manage key required or invalid (401/403)."""
    pass


class RateLimitError(WatchpostError):
    """Rate limited (429). Check retry_after."""

    def __init__(self, message: str, retry_after: float = 0, **kwargs):
        super().__init__(message, **kwargs)
        self.retry_after = retry_after


class ValidationError(WatchpostError):
    """Invalid input (400/422)."""
    pass


class ConflictError(WatchpostError):
    """Conflict — e.g. circular dependency (409)."""
    pass


# ---------------------------------------------------------------------------
# SSE helpers
# ---------------------------------------------------------------------------


@dataclass
class SSEEvent:
    """A single Server-Sent Event."""
    event: str = "message"
    data: str = ""
    id: Optional[str] = None
    retry: Optional[int] = None

    @property
    def json(self) -> Any:
        """Parse data as JSON. Returns None on failure."""
        try:
            return json.loads(self.data)
        except (json.JSONDecodeError, TypeError):
            return None


def _iter_sse(response) -> Generator[SSEEvent, None, None]:
    """Iterate SSE events from an HTTP response."""
    current = SSEEvent()
    for raw_line in response:
        line = raw_line.decode("utf-8", errors="replace").rstrip("\n\r")
        if line == "":
            if current.data or current.event != "message":
                yield current
            current = SSEEvent()
            continue
        if line.startswith(":"):
            continue
        if ":" in line:
            field, _, value = line.partition(":")
            if value.startswith(" "):
                value = value[1:]
        else:
            field, value = line, ""

        if field == "event":
            current.event = value
        elif field == "data":
            current.data = (current.data + "\n" + value) if current.data else value
        elif field == "id":
            current.id = value
        elif field == "retry":
            try:
                current.retry = int(value)
            except ValueError:
                pass
    if current.data or current.event != "message":
        yield current


# ---------------------------------------------------------------------------
# Client
# ---------------------------------------------------------------------------


class Watchpost:
    """Client for the Watchpost monitoring API.

    Args:
        base_url: Base URL of the Watchpost server (e.g. "http://localhost:3007").
        timeout: Default request timeout in seconds.
    """

    def __init__(self, base_url: Optional[str] = None, *, timeout: int = 30):
        self.base_url = (base_url or os.environ.get("WATCHPOST_URL", "http://localhost:3007")).rstrip("/")
        self.timeout = timeout

    # ------------------------------------------------------------------
    # HTTP helpers
    # ------------------------------------------------------------------

    def _url(self, path: str, **params) -> str:
        url = f"{self.base_url}{path}"
        filtered = {k: v for k, v in params.items() if v is not None}
        if filtered:
            url += "?" + urllib.parse.urlencode(filtered, doseq=True)
        return url

    def _request(
        self,
        method: str,
        path: str,
        *,
        body: Any = None,
        key: Optional[str] = None,
        params: Optional[Dict[str, Any]] = None,
        raw: bool = False,
    ) -> Any:
        url = self._url(path, **(params or {}))
        headers: Dict[str, str] = {}

        data = None
        if body is not None:
            data = json.dumps(body).encode()
            headers["Content-Type"] = "application/json"

        if key:
            headers["Authorization"] = f"Bearer {key}"

        req = urllib.request.Request(url, data=data, headers=headers, method=method)
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                content = resp.read()
                if raw:
                    return content
                if not content:
                    return None
                ct = resp.headers.get("Content-Type", "")
                if "json" in ct:
                    return json.loads(content)
                if "svg" in ct or "image" in ct or "text" in ct:
                    return content.decode("utf-8", errors="replace")
                return json.loads(content)
        except urllib.error.HTTPError as e:
            body_bytes = e.read()
            try:
                err_body = json.loads(body_bytes)
            except Exception:
                err_body = body_bytes.decode("utf-8", errors="replace") if body_bytes else None

            msg = str(err_body) if err_body else e.reason
            if isinstance(err_body, dict) and "error" in err_body:
                msg = err_body["error"]

            if e.code == 404:
                raise NotFoundError(msg, status_code=404, body=err_body)
            if e.code in (401, 403):
                raise AuthError(msg, status_code=e.code, body=err_body)
            if e.code == 409:
                raise ConflictError(msg, status_code=409, body=err_body)
            if e.code == 429:
                retry = 0
                if isinstance(err_body, dict):
                    retry = err_body.get("retry_after_secs", 0)
                raise RateLimitError(msg, retry_after=retry, status_code=429, body=err_body)
            if e.code in (400, 422):
                raise ValidationError(msg, status_code=e.code, body=err_body)
            raise WatchpostError(msg, status_code=e.code, body=err_body)

    def _get(self, path: str, *, key: Optional[str] = None, params: Optional[Dict] = None, raw: bool = False):
        return self._request("GET", path, key=key, params=params, raw=raw)

    def _post(self, path: str, body: Any = None, *, key: Optional[str] = None, params: Optional[Dict] = None):
        return self._request("POST", path, body=body, key=key, params=params)

    def _patch(self, path: str, body: Any = None, *, key: Optional[str] = None):
        return self._request("PATCH", path, body=body, key=key)

    def _put(self, path: str, body: Any = None, *, key: Optional[str] = None):
        return self._request("PUT", path, body=body, key=key)

    def _delete(self, path: str, *, key: Optional[str] = None, params: Optional[Dict] = None):
        return self._request("DELETE", path, key=key, params=params)

    @staticmethod
    def _flatten_monitor_response(resp: Dict) -> Dict:
        """Flatten a create/update monitor response.

        The API returns {"monitor": {...}, "manage_key": "...", "manage_url": "...", ...}.
        This merges the monitor fields to the top level for convenience,
        so you can access resp["id"] and resp["manage_key"] directly.
        """
        if not isinstance(resp, dict):
            return resp
        if "monitor" in resp and isinstance(resp["monitor"], dict):
            flat = dict(resp["monitor"])
            for k in ("manage_key", "manage_url", "view_url", "api_base"):
                if k in resp:
                    flat[k] = resp[k]
            return flat
        return resp

    # ------------------------------------------------------------------
    # Health
    # ------------------------------------------------------------------

    def health(self) -> Dict:
        """GET /api/v1/health"""
        return self._get("/api/v1/health")

    # ------------------------------------------------------------------
    # Monitors — CRUD
    # ------------------------------------------------------------------

    def create_monitor(
        self,
        name: str,
        url: str,
        *,
        monitor_type: str = "http",
        method: str = "GET",
        interval_seconds: int = 600,
        timeout_ms: int = 10000,
        expected_status: int = 200,
        body_contains: Optional[str] = None,
        headers: Optional[Dict[str, str]] = None,
        is_public: bool = False,
        group_name: Optional[str] = None,
        tags: Optional[List[str]] = None,
        follow_redirects: Optional[bool] = None,
        response_time_threshold_ms: Optional[int] = None,
        confirmation_threshold: Optional[int] = None,
        sla_target: Optional[float] = None,
        sla_period_days: Optional[int] = None,
        consensus_threshold: Optional[int] = None,
        max_messages: Optional[int] = None,
        dns_record_type: Optional[str] = None,
        dns_expected: Optional[str] = None,
    ) -> Dict:
        """Create a new monitor. Returns response including manage_key (save it!).

        Args:
            name: Human-readable monitor name.
            url: Target to check. Format depends on monitor_type:
                 http: "https://example.com/health"
                 tcp: "example.com:5432"
                 dns: "example.com"
            monitor_type: "http" (default), "tcp", or "dns".
        """
        payload: Dict[str, Any] = {
            "name": name,
            "url": url,
            "monitor_type": monitor_type,
            "method": method,
            "interval_seconds": interval_seconds,
            "timeout_ms": timeout_ms,
            "expected_status": expected_status,
            "is_public": is_public,
        }
        if body_contains is not None:
            payload["body_contains"] = body_contains
        if headers is not None:
            payload["headers"] = headers
        if group_name is not None:
            payload["group_name"] = group_name
        if tags is not None:
            payload["tags"] = tags
        if follow_redirects is not None:
            payload["follow_redirects"] = follow_redirects
        if response_time_threshold_ms is not None:
            payload["response_time_threshold_ms"] = response_time_threshold_ms
        if confirmation_threshold is not None:
            payload["confirmation_threshold"] = confirmation_threshold
        if sla_target is not None:
            payload["sla_target"] = sla_target
        if sla_period_days is not None:
            payload["sla_period_days"] = sla_period_days
        if consensus_threshold is not None:
            payload["consensus_threshold"] = consensus_threshold
        if dns_record_type is not None:
            payload["dns_record_type"] = dns_record_type
        if dns_expected is not None:
            payload["dns_expected"] = dns_expected
        return self._flatten_monitor_response(self._post("/api/v1/monitors", payload))

    def list_monitors(
        self,
        *,
        search: Optional[str] = None,
        status: Optional[str] = None,
        group: Optional[str] = None,
        tag: Optional[str] = None,
    ) -> List[Dict]:
        """List public monitors with optional filters.

        Args:
            search: Filter by name/URL substring.
            status: Filter by status (up/down/degraded/unknown).
            group: Filter by group name.
            tag: Filter by tag.
        """
        params: Dict[str, Any] = {}
        if search:
            params["search"] = search
        if status:
            params["status"] = status
        if group:
            params["group"] = group
        if tag:
            params["tag"] = tag
        return self._get("/api/v1/monitors", params=params)

    def get_monitor(self, monitor_id: str) -> Dict:
        """Get monitor details including current status."""
        return self._get(f"/api/v1/monitors/{monitor_id}")

    def update_monitor(self, monitor_id: str, key: str, **fields) -> Dict:
        """Update monitor config. Pass only fields to change.

        Args:
            monitor_id: Monitor UUID.
            key: Manage key.
            **fields: Any monitor fields to update (name, url, interval_seconds, etc.)
        """
        return self._flatten_monitor_response(self._patch(f"/api/v1/monitors/{monitor_id}", fields, key=key))

    def delete_monitor(self, monitor_id: str, key: str) -> None:
        """Delete a monitor and all its data."""
        self._delete(f"/api/v1/monitors/{monitor_id}", key=key)

    def pause_monitor(self, monitor_id: str, key: str) -> Dict:
        """Pause monitoring checks."""
        return self._flatten_monitor_response(self._post(f"/api/v1/monitors/{monitor_id}/pause", key=key))

    def resume_monitor(self, monitor_id: str, key: str) -> Dict:
        """Resume monitoring checks."""
        return self._flatten_monitor_response(self._post(f"/api/v1/monitors/{monitor_id}/resume", key=key))

    def export_monitor(self, monitor_id: str, key: str) -> Dict:
        """Export monitor config for backup/migration."""
        return self._get(f"/api/v1/monitors/{monitor_id}/export", key=key)

    # ------------------------------------------------------------------
    # Bulk operations
    # ------------------------------------------------------------------

    def bulk_create_monitors(self, monitors: List[Dict]) -> Dict:
        """Create up to 50 monitors at once.

        Args:
            monitors: List of monitor dicts (each with name, url, etc.)

        Returns:
            Dict with created (flattened), errors, total, succeeded, failed.
        """
        result = self._post("/api/v1/monitors/bulk", {"monitors": monitors})
        if isinstance(result, dict) and "created" in result:
            result["created"] = [self._flatten_monitor_response(m) for m in result["created"]]
        return result

    # ------------------------------------------------------------------
    # Heartbeats (check history)
    # ------------------------------------------------------------------

    def list_heartbeats(
        self,
        monitor_id: str,
        *,
        after: Optional[int] = None,
        limit: Optional[int] = None,
    ) -> Any:
        """Get check history for a monitor.

        Args:
            monitor_id: Monitor UUID.
            after: Cursor — return heartbeats after this seq.
            limit: Max results to return.
        """
        params: Dict[str, Any] = {}
        if after is not None:
            params["after"] = after
        if limit is not None:
            params["limit"] = limit
        return self._get(f"/api/v1/monitors/{monitor_id}/heartbeats", params=params)

    # ------------------------------------------------------------------
    # Uptime
    # ------------------------------------------------------------------

    def get_uptime(self, monitor_id: str) -> Dict:
        """Get uptime stats (24h/7d/30d/90d) for a monitor."""
        return self._get(f"/api/v1/monitors/{monitor_id}/uptime")

    def get_uptime_history(
        self,
        monitor_id: Optional[str] = None,
        *,
        days: int = 30,
    ) -> Any:
        """Get daily uptime percentages over time.

        Args:
            monitor_id: If provided, per-monitor history. If None, aggregate across all.
            days: Number of days (max 90).
        """
        params = {"days": days}
        if monitor_id:
            return self._get(f"/api/v1/monitors/{monitor_id}/uptime-history", params=params)
        return self._get("/api/v1/uptime-history", params=params)

    # ------------------------------------------------------------------
    # Incidents
    # ------------------------------------------------------------------

    def list_incidents(
        self,
        monitor_id: str,
        *,
        after: Optional[int] = None,
        limit: Optional[int] = None,
    ) -> Any:
        """List incidents for a monitor."""
        params: Dict[str, Any] = {}
        if after is not None:
            params["after"] = after
        if limit is not None:
            params["limit"] = limit
        return self._get(f"/api/v1/monitors/{monitor_id}/incidents", params=params)

    def get_incident(self, incident_id: str) -> Dict:
        """Get single incident detail (includes notes_count)."""
        return self._get(f"/api/v1/incidents/{incident_id}")

    def acknowledge_incident(
        self,
        incident_id: str,
        key: str,
        *,
        note: Optional[str] = None,
        acknowledged_by: Optional[str] = None,
    ) -> Dict:
        """Acknowledge an incident.

        Args:
            incident_id: Incident UUID.
            key: Manage key for the incident's monitor.
            note: Optional acknowledgement note.
            acknowledged_by: Who is acknowledging.
        """
        body: Dict[str, Any] = {}
        if note:
            body["acknowledgement"] = note
        if acknowledged_by:
            body["acknowledged_by"] = acknowledged_by
        return self._post(f"/api/v1/incidents/{incident_id}/acknowledge", body or None, key=key)

    # ------------------------------------------------------------------
    # Incident Notes
    # ------------------------------------------------------------------

    def add_incident_note(
        self,
        incident_id: str,
        content: str,
        key: str,
        *,
        author: Optional[str] = None,
    ) -> Dict:
        """Add an investigation note to an incident.

        Args:
            incident_id: Incident UUID.
            content: Note text (1-10,000 chars).
            key: Manage key.
            author: Note author (defaults to "anonymous").
        """
        body: Dict[str, Any] = {"content": content}
        if author:
            body["author"] = author
        return self._post(f"/api/v1/incidents/{incident_id}/notes", body, key=key)

    def list_incident_notes(
        self,
        incident_id: str,
        *,
        limit: Optional[int] = None,
    ) -> Any:
        """List investigation notes for an incident (chronological)."""
        params: Dict[str, Any] = {}
        if limit is not None:
            params["limit"] = limit
        return self._get(f"/api/v1/incidents/{incident_id}/notes", params=params)

    # ------------------------------------------------------------------
    # Notifications
    # ------------------------------------------------------------------

    def create_notification(
        self,
        monitor_id: str,
        name: str,
        channel_type: str,
        config: Dict,
        key: str,
    ) -> Dict:
        """Add a notification channel to a monitor.

        Args:
            monitor_id: Monitor UUID.
            name: Channel label.
            channel_type: "webhook" or "email".
            config: Channel config — {"url": "..."} for webhook, {"address": "..."} for email.
                    For webhook, optionally add "payload_format": "chat" for simple text payloads.
            key: Manage key.
        """
        return self._post(
            f"/api/v1/monitors/{monitor_id}/notifications",
            {"name": name, "channel_type": channel_type, "config": config},
            key=key,
        )

    def list_notifications(self, monitor_id: str, key: str) -> Any:
        """List notification channels for a monitor (auth required)."""
        return self._get(f"/api/v1/monitors/{monitor_id}/notifications", key=key)

    def update_notification(self, notification_id: str, key: str, **fields) -> Dict:
        """Update a notification channel (enable/disable, change config)."""
        return self._patch(f"/api/v1/notifications/{notification_id}", fields, key=key)

    def delete_notification(self, notification_id: str, key: str) -> None:
        """Delete a notification channel."""
        self._delete(f"/api/v1/notifications/{notification_id}", key=key)

    # ------------------------------------------------------------------
    # Webhook Delivery Log
    # ------------------------------------------------------------------

    def list_webhook_deliveries(
        self,
        monitor_id: str,
        key: str,
        *,
        limit: Optional[int] = None,
        after: Optional[int] = None,
        event: Optional[str] = None,
        status: Optional[str] = None,
    ) -> Dict:
        """List webhook delivery attempts (audit log).

        Args:
            monitor_id: Monitor UUID.
            key: Manage key.
            limit: Max results (1-200, default 50).
            after: Cursor (seq).
            event: Filter by event type (e.g. "incident.created").
            status: Filter by delivery status ("success" or "failed").
        """
        params: Dict[str, Any] = {}
        if limit is not None:
            params["limit"] = limit
        if after is not None:
            params["after"] = after
        if event:
            params["event"] = event
        if status:
            params["status"] = status
        return self._get(f"/api/v1/monitors/{monitor_id}/webhook-deliveries", key=key, params=params)

    # ------------------------------------------------------------------
    # Maintenance Windows
    # ------------------------------------------------------------------

    def create_maintenance(
        self,
        monitor_id: str,
        title: str,
        starts_at: str,
        ends_at: str,
        key: str,
    ) -> Dict:
        """Schedule a maintenance window.

        Args:
            monitor_id: Monitor UUID.
            title: Window title.
            starts_at: ISO-8601 start time.
            ends_at: ISO-8601 end time.
            key: Manage key.
        """
        return self._post(
            f"/api/v1/monitors/{monitor_id}/maintenance",
            {"title": title, "starts_at": starts_at, "ends_at": ends_at},
            key=key,
        )

    def list_maintenance(self, monitor_id: str) -> Any:
        """List maintenance windows for a monitor."""
        return self._get(f"/api/v1/monitors/{monitor_id}/maintenance")

    def delete_maintenance(self, maintenance_id: str, key: str) -> None:
        """Delete a maintenance window."""
        self._delete(f"/api/v1/maintenance/{maintenance_id}", key=key)

    # ------------------------------------------------------------------
    # SLA
    # ------------------------------------------------------------------

    def get_sla(self, monitor_id: str) -> Dict:
        """Get SLA status with error budget tracking.

        Returns target_pct, current_pct, budget_remaining_seconds, status (met|at_risk|breached).
        Raises NotFoundError if no SLA target configured.
        """
        return self._get(f"/api/v1/monitors/{monitor_id}/sla")

    # ------------------------------------------------------------------
    # Badges
    # ------------------------------------------------------------------

    def get_uptime_badge(
        self,
        monitor_id: str,
        *,
        period: str = "24h",
        label: Optional[str] = None,
    ) -> str:
        """Get SVG uptime badge (shields.io style).

        Args:
            period: "24h", "7d", "30d", or "90d".
            label: Custom badge label.

        Returns:
            SVG string.
        """
        params: Dict[str, Any] = {"period": period}
        if label:
            params["label"] = label
        return self._get(f"/api/v1/monitors/{monitor_id}/badge/uptime", params=params, raw=True).decode()

    def get_status_badge(self, monitor_id: str, *, label: Optional[str] = None) -> str:
        """Get SVG status badge.

        Returns:
            SVG string.
        """
        params: Dict[str, Any] = {}
        if label:
            params["label"] = label
        return self._get(f"/api/v1/monitors/{monitor_id}/badge/status", params=params, raw=True).decode()

    # ------------------------------------------------------------------
    # Tags & Groups
    # ------------------------------------------------------------------

    def list_tags(self) -> List[str]:
        """List all unique tags across public monitors."""
        return self._get("/api/v1/tags")

    def list_groups(self) -> List[str]:
        """List all unique group names across public monitors."""
        return self._get("/api/v1/groups")

    # ------------------------------------------------------------------
    # Status / Dashboard
    # ------------------------------------------------------------------

    def get_status(
        self,
        *,
        search: Optional[str] = None,
        status: Optional[str] = None,
        group: Optional[str] = None,
        tag: Optional[str] = None,
    ) -> Dict:
        """Get public status page (includes branding if configured)."""
        params: Dict[str, Any] = {}
        if search:
            params["search"] = search
        if status:
            params["status"] = status
        if group:
            params["group"] = group
        if tag:
            params["tag"] = tag
        return self._get("/api/v1/status", params=params)

    def get_dashboard(self, *, key: Optional[str] = None) -> Dict:
        """Get dashboard stats. With admin key: includes recent incidents, slowest monitors."""
        return self._get("/api/v1/dashboard", key=key)

    def verify_admin(self, key: str) -> Dict:
        """Verify admin key validity."""
        return self._get("/api/v1/admin/verify", key=key)

    # ------------------------------------------------------------------
    # Settings / Branding
    # ------------------------------------------------------------------

    def get_settings(self) -> Dict:
        """Get status page branding (title, description, logo_url)."""
        return self._get("/api/v1/settings")

    def update_settings(self, key: str, **fields) -> Dict:
        """Update status page branding (admin key required).

        Args:
            key: Admin key.
            **fields: title, description, logo_url. Empty string clears a field.
        """
        return self._put("/api/v1/settings", fields, key=key)

    # ------------------------------------------------------------------
    # Multi-region: Locations
    # ------------------------------------------------------------------

    def create_location(self, name: str, region: str, key: str) -> Dict:
        """Register a remote check location (admin key required).

        Returns response including probe_key (save it!).
        """
        return self._post("/api/v1/locations", {"name": name, "region": region}, key=key)

    def list_locations(self) -> List[Dict]:
        """List all check locations (includes health_status)."""
        return self._get("/api/v1/locations")

    def get_location(self, location_id: str) -> Dict:
        """Get a specific check location."""
        return self._get(f"/api/v1/locations/{location_id}")

    def delete_location(self, location_id: str, key: str) -> None:
        """Remove a check location (admin key required)."""
        self._delete(f"/api/v1/locations/{location_id}", key=key)

    # ------------------------------------------------------------------
    # Multi-region: Probes
    # ------------------------------------------------------------------

    def submit_probe(
        self,
        probe_key: str,
        results: List[Dict],
    ) -> Dict:
        """Submit probe results from a remote location.

        Args:
            probe_key: Location's probe key.
            results: List of check results, each with:
                monitor_id, status (up/down/degraded), response_time_ms,
                status_code (optional), error_message (optional), checked_at (optional).
                Max 100 per submission.

        Returns:
            Dict with accepted, rejected, errors counts.
        """
        return self._post("/api/v1/probe", {"results": results}, key=probe_key)

    def get_location_status(self, monitor_id: str) -> List[Dict]:
        """Get per-location status for a monitor.

        Returns latest probe result from each active check location.
        """
        return self._get(f"/api/v1/monitors/{monitor_id}/locations")

    def get_consensus(self, monitor_id: str) -> Dict:
        """Get multi-region consensus status.

        Returns threshold, counts per status, effective_status, and per-location details.
        Raises ValidationError if consensus not configured on this monitor.
        """
        return self._get(f"/api/v1/monitors/{monitor_id}/consensus")

    # ------------------------------------------------------------------
    # Alert Rules
    # ------------------------------------------------------------------

    def set_alert_rules(
        self,
        monitor_id: str,
        key: str,
        *,
        repeat_interval_minutes: int = 0,
        max_repeats: int = 10,
        escalation_after_minutes: int = 0,
    ) -> Dict:
        """Set alert rules for a monitor (upsert).

        Args:
            repeat_interval_minutes: Re-send notifications every N minutes. 0 = disabled. Min 5.
            max_repeats: Cap on repeat notifications per incident. Max 100.
            escalation_after_minutes: Escalate if not acked within N minutes. 0 = disabled. Min 5.
        """
        return self._put(
            f"/api/v1/monitors/{monitor_id}/alert-rules",
            {
                "repeat_interval_minutes": repeat_interval_minutes,
                "max_repeats": max_repeats,
                "escalation_after_minutes": escalation_after_minutes,
            },
            key=key,
        )

    def get_alert_rules(self, monitor_id: str, key: str) -> Dict:
        """Get current alert rules (auth required). 404 if not configured."""
        return self._get(f"/api/v1/monitors/{monitor_id}/alert-rules", key=key)

    def delete_alert_rules(self, monitor_id: str, key: str) -> None:
        """Remove alert rules."""
        self._delete(f"/api/v1/monitors/{monitor_id}/alert-rules", key=key)

    def list_alert_log(
        self,
        monitor_id: str,
        key: str,
        *,
        limit: Optional[int] = None,
        after: Optional[str] = None,
    ) -> Any:
        """View alert notification history."""
        params: Dict[str, Any] = {}
        if limit is not None:
            params["limit"] = limit
        if after:
            params["after"] = after
        return self._get(f"/api/v1/monitors/{monitor_id}/alert-log", key=key, params=params)

    # ------------------------------------------------------------------
    # Dependencies
    # ------------------------------------------------------------------

    def add_dependency(self, monitor_id: str, depends_on_id: str, key: str) -> Dict:
        """Add an upstream dependency.

        When the upstream is down, downstream alerts are suppressed.
        Raises ConflictError for circular or duplicate dependencies.
        """
        return self._post(
            f"/api/v1/monitors/{monitor_id}/dependencies",
            {"depends_on_id": depends_on_id},
            key=key,
        )

    def list_dependencies(self, monitor_id: str) -> List[Dict]:
        """List upstream dependencies (no auth)."""
        return self._get(f"/api/v1/monitors/{monitor_id}/dependencies")

    def delete_dependency(self, monitor_id: str, dep_id: str, key: str) -> None:
        """Remove a dependency."""
        self._delete(f"/api/v1/monitors/{monitor_id}/dependencies/{dep_id}", key=key)

    def list_dependents(self, monitor_id: str) -> List[Dict]:
        """List monitors that depend on this one (no auth)."""
        return self._get(f"/api/v1/monitors/{monitor_id}/dependents")

    # ------------------------------------------------------------------
    # Status Pages (custom)
    # ------------------------------------------------------------------

    def create_status_page(
        self,
        slug: str,
        title: str,
        *,
        description: Optional[str] = None,
        logo_url: Optional[str] = None,
        custom_domain: Optional[str] = None,
        is_public: bool = True,
    ) -> Dict:
        """Create a named status page. Returns manage_key."""
        payload: Dict[str, Any] = {"slug": slug, "title": title, "is_public": is_public}
        if description:
            payload["description"] = description
        if logo_url:
            payload["logo_url"] = logo_url
        if custom_domain:
            payload["custom_domain"] = custom_domain
        return self._post("/api/v1/status-pages", payload)

    def list_status_pages(self) -> List[Dict]:
        """List all public status pages."""
        return self._get("/api/v1/status-pages")

    def get_status_page(self, slug_or_id: str) -> Dict:
        """Get status page detail with monitors and overall status."""
        return self._get(f"/api/v1/status-pages/{slug_or_id}")

    def update_status_page(self, slug_or_id: str, key: str, **fields) -> Dict:
        """Update a status page."""
        return self._patch(f"/api/v1/status-pages/{slug_or_id}", fields, key=key)

    def delete_status_page(self, slug_or_id: str, key: str) -> None:
        """Delete a status page (monitors are not deleted)."""
        self._delete(f"/api/v1/status-pages/{slug_or_id}", key=key)

    def add_monitors_to_page(self, slug_or_id: str, monitor_ids: List[str], key: str) -> Dict:
        """Add monitors to a status page."""
        return self._post(
            f"/api/v1/status-pages/{slug_or_id}/monitors",
            {"monitor_ids": monitor_ids},
            key=key,
        )

    def remove_monitor_from_page(self, slug_or_id: str, monitor_id: str, key: str) -> None:
        """Remove a monitor from a status page."""
        self._delete(f"/api/v1/status-pages/{slug_or_id}/monitors/{monitor_id}", key=key)

    def list_page_monitors(self, slug_or_id: str) -> List[Dict]:
        """List monitors on a status page."""
        return self._get(f"/api/v1/status-pages/{slug_or_id}/monitors")

    # ------------------------------------------------------------------
    # SSE Event Streams
    # ------------------------------------------------------------------

    def stream_events(self, monitor_id: Optional[str] = None) -> Generator[SSEEvent, None, None]:
        """Stream real-time SSE events.

        Args:
            monitor_id: If provided, stream events for one monitor. If None, global stream.

        Yields:
            SSEEvent objects with event type and data.
        """
        if monitor_id:
            path = f"/api/v1/monitors/{monitor_id}/events"
        else:
            path = "/api/v1/events"
        url = self._url(path)
        req = urllib.request.Request(url, headers={"Accept": "text/event-stream"})
        resp = urllib.request.urlopen(req, timeout=self.timeout)
        yield from _iter_sse(resp)

    # ------------------------------------------------------------------
    # Discovery
    # ------------------------------------------------------------------

    def get_llms_txt(self) -> str:
        """Get the llms.txt agent integration guide (API v1 path)."""
        return self._get("/api/v1/llms.txt", raw=True).decode()

    def llms_txt_root(self) -> str:
        """Get the llms.txt from root path."""
        return self._get("/llms.txt", raw=True).decode()

    def get_skills_index(self) -> Dict:
        """Get the well-known skills discovery index."""
        return self._get("/.well-known/skills/index.json")

    def get_skill(self) -> str:
        """Get the SKILL.md integration guide (well-known path)."""
        return self._get("/.well-known/skills/watchpost/SKILL.md", raw=True).decode()

    def skill_md_v1(self) -> str:
        """Get the SKILL.md via API v1 path."""
        return self._get("/api/v1/skills/SKILL.md", raw=True).decode()

    def get_openapi(self) -> Dict:
        """Get the OpenAPI specification."""
        return self._get("/api/v1/openapi.json")

    # ------------------------------------------------------------------
    # Convenience helpers
    # ------------------------------------------------------------------

    def quick_monitor(self, name: str, url: str, *, public: bool = True) -> Dict:
        """Create a simple HTTP monitor with sensible defaults.

        Shorthand for create_monitor with is_public=True.
        Returns the flattened response including id and manage_key.
        """
        return self.create_monitor(name, url, is_public=public)

    def is_up(self, monitor_id: str) -> bool:
        """Quick check: is a monitor currently up?"""
        mon = self.get_monitor(monitor_id)
        return mon.get("current_status") == "up"

    def all_up(self) -> bool:
        """Quick check: are all public monitors up?"""
        monitors = self.list_monitors()
        if not monitors:
            return True
        return all(m.get("current_status") == "up" for m in monitors)

    def wait_for_up(
        self,
        monitor_id: str,
        *,
        timeout: int = 300,
        poll_interval: int = 10,
    ) -> bool:
        """Wait until a monitor is up (or timeout).

        Args:
            monitor_id: Monitor UUID.
            timeout: Max seconds to wait.
            poll_interval: Seconds between checks.

        Returns:
            True if monitor came up, False if timed out.
        """
        deadline = time.time() + timeout
        while time.time() < deadline:
            if self.is_up(monitor_id):
                return True
            time.sleep(poll_interval)
        return False

    def get_downtime_summary(self, monitor_id: str) -> Dict:
        """Get a summary of current downtime status.

        Returns a dict with is_down, current_incident (if any), uptime stats.
        """
        mon = self.get_monitor(monitor_id)
        uptime = self.get_uptime(monitor_id)
        incidents = self.list_incidents(monitor_id, limit=1)

        current_incident = None
        if isinstance(incidents, list) and incidents:
            latest = incidents[0]
            if latest.get("resolved_at") is None:
                current_incident = latest
        elif isinstance(incidents, dict) and incidents.get("incidents"):
            latest = incidents["incidents"][0]
            if latest.get("resolved_at") is None:
                current_incident = latest

        return {
            "is_down": mon.get("current_status") == "down",
            "current_status": mon.get("current_status"),
            "current_incident": current_incident,
            "uptime_24h": uptime.get("uptime_24h"),
            "uptime_7d": uptime.get("uptime_7d"),
            "uptime_30d": uptime.get("uptime_30d"),
        }
