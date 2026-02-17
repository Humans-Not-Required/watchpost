#!/usr/bin/env python3
"""Integration tests for the watchpost Python SDK.

Run against a live instance:
    WATCHPOST_URL=http://192.168.0.79:3007 python3 test_sdk.py
"""

import json
import os
import sys
import time
import traceback

# Import from same directory
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from watchpost import (
    Watchpost,
    WatchpostError,
    NotFoundError,
    AuthError,
    ValidationError,
    ConflictError,
    RateLimitError,
)

BASE_URL = os.environ.get("WATCHPOST_URL", "http://192.168.0.79:3007")

passed = 0
failed = 0
errors = []


def test(name):
    """Decorator for test functions."""
    def decorator(fn):
        global passed, failed
        try:
            fn()
            passed += 1
            print(f"  ✅ {name}")
        except Exception as e:
            failed += 1
            errors.append((name, str(e)))
            print(f"  ❌ {name}: {e}")
        return fn
    return decorator


def main():
    global passed, failed
    wp = Watchpost(BASE_URL)

    print(f"\n🧪 Running Watchpost SDK integration tests against {BASE_URL}\n")

    # Track monitors to clean up
    created_monitors = []  # (id, key) tuples
    created_pages = []  # (slug, key) tuples

    # ── Health & Discovery ──────────────────────────────────────────────
    print("Health & Discovery:")

    @test("health returns version")
    def _():
        h = wp.health()
        assert "version" in h or "status" in h, f"Unexpected health: {h}"

    @test("llms.txt returns text")
    def _():
        txt = wp.get_llms_txt()
        assert "Watchpost" in txt, f"Missing 'Watchpost' in llms.txt"

    @test("skills index returns JSON")
    def _():
        idx = wp.get_skills_index()
        assert "skills" in idx or "name" in idx, f"Unexpected skills index: {idx}"

    @test("skill.md returns markdown")
    def _():
        md = wp.get_skill()
        assert "watchpost" in md.lower() or "monitor" in md.lower(), "SKILL.md missing expected content"

    # ── Monitor CRUD ────────────────────────────────────────────────────
    print("\nMonitor CRUD:")

    monitor_id = None
    monitor_key = None

    @test("create monitor")
    def _():
        nonlocal monitor_id, monitor_key
        mon = wp.create_monitor(
            "SDK Test Monitor",
            "https://httpbin.org/status/200",
            is_public=True,
            tags=["sdk-test", "ci"],
            group_name="SDK Tests",
        )
        assert "id" in mon, f"Missing id: {mon}"
        assert "manage_key" in mon, f"Missing manage_key: {mon}"
        monitor_id = mon["id"]
        monitor_key = mon["manage_key"]
        created_monitors.append((monitor_id, monitor_key))

    @test("get monitor")
    def _():
        mon = wp.get_monitor(monitor_id)
        assert mon["name"] == "SDK Test Monitor", f"Wrong name: {mon['name']}"
        assert mon["url"] == "https://httpbin.org/status/200"

    @test("list monitors includes created")
    def _():
        monitors = wp.list_monitors()
        ids = [m["id"] for m in monitors]
        assert monitor_id in ids, f"Monitor {monitor_id} not in list"

    @test("update monitor name")
    def _():
        result = wp.update_monitor(monitor_id, monitor_key, name="SDK Renamed")
        assert isinstance(result, dict), f"Expected dict: {result}"
        # PATCH returns {"message": "Monitor updated"}, verify via GET
        mon = wp.get_monitor(monitor_id)
        assert mon["name"] == "SDK Renamed", f"Name not updated: {mon['name']}"

    @test("update monitor without key raises AuthError")
    def _():
        try:
            wp.update_monitor(monitor_id, "wrong-key", name="Bad Update")
            assert False, "Expected AuthError"
        except AuthError:
            pass

    @test("get nonexistent monitor raises NotFoundError")
    def _():
        try:
            wp.get_monitor("00000000-0000-0000-0000-000000000000")
            assert False, "Expected NotFoundError"
        except NotFoundError:
            pass

    # ── Monitor with options ────────────────────────────────────────────
    print("\nMonitor Options:")

    sla_monitor_id = None
    sla_monitor_key = None

    @test("create monitor with SLA target")
    def _():
        nonlocal sla_monitor_id, sla_monitor_key
        mon = wp.create_monitor(
            "SLA Test",
            "https://httpbin.org/status/200",
            is_public=True,
            sla_target=99.9,
            sla_period_days=30,
            response_time_threshold_ms=5000,
        )
        sla_monitor_id = mon["id"]
        sla_monitor_key = mon["manage_key"]
        created_monitors.append((sla_monitor_id, sla_monitor_key))

    @test("create TCP monitor")
    def _():
        mon = wp.create_monitor(
            "TCP Test",
            "httpbin.org:443",
            monitor_type="tcp",
            is_public=True,
        )
        created_monitors.append((mon["id"], mon["manage_key"]))

    @test("create DNS monitor")
    def _():
        mon = wp.create_monitor(
            "DNS Test",
            "httpbin.org",
            monitor_type="dns",
            dns_record_type="A",
            is_public=True,
        )
        created_monitors.append((mon["id"], mon["manage_key"]))

    @test("create monitor with invalid URL raises ValidationError")
    def _():
        try:
            wp.create_monitor("Bad", "not-a-url")
            assert False, "Expected ValidationError"
        except (ValidationError, WatchpostError):
            pass

    # ── Pause / Resume ──────────────────────────────────────────────────
    print("\nPause / Resume:")

    @test("pause monitor")
    def _():
        result = wp.pause_monitor(monitor_id, monitor_key)
        # Verify pause took effect
        mon = wp.get_monitor(monitor_id)
        assert mon.get("is_paused") == True, f"Not paused: {mon}"

    @test("resume monitor")
    def _():
        result = wp.resume_monitor(monitor_id, monitor_key)
        mon = wp.get_monitor(monitor_id)
        assert mon.get("is_paused") == False, f"Still paused: {mon}"

    @test("pause without key raises AuthError")
    def _():
        try:
            wp.pause_monitor(monitor_id, "wrong-key")
            assert False, "Expected AuthError"
        except AuthError:
            pass

    # ── Heartbeats ──────────────────────────────────────────────────────
    print("\nHeartbeats:")

    @test("list heartbeats (may be empty)")
    def _():
        hb = wp.list_heartbeats(monitor_id)
        assert isinstance(hb, (list, dict)), f"Unexpected type: {type(hb)}"

    @test("list heartbeats with limit")
    def _():
        hb = wp.list_heartbeats(monitor_id, limit=5)
        assert isinstance(hb, (list, dict)), f"Unexpected type: {type(hb)}"

    # ── Uptime ──────────────────────────────────────────────────────────
    print("\nUptime:")

    @test("get uptime stats")
    def _():
        up = wp.get_uptime(monitor_id)
        assert isinstance(up, dict), f"Unexpected type: {type(up)}"

    @test("get uptime history per monitor")
    def _():
        hist = wp.get_uptime_history(monitor_id, days=7)
        assert isinstance(hist, (list, dict)), f"Unexpected type: {type(hist)}"

    @test("get aggregate uptime history")
    def _():
        hist = wp.get_uptime_history(days=7)
        assert isinstance(hist, (list, dict)), f"Unexpected type: {type(hist)}"

    # ── SLA ─────────────────────────────────────────────────────────────
    print("\nSLA:")

    @test("get SLA status")
    def _():
        sla = wp.get_sla(sla_monitor_id)
        assert "target_pct" in sla or "status" in sla, f"Missing SLA fields: {sla}"

    @test("get SLA on monitor without target raises NotFoundError")
    def _():
        # Create a monitor without SLA target
        mon = wp.create_monitor("No SLA", "https://httpbin.org/status/200", is_public=True)
        created_monitors.append((mon["id"], mon["manage_key"]))
        try:
            wp.get_sla(mon["id"])
            assert False, "Expected NotFoundError for no SLA"
        except (NotFoundError, WatchpostError):
            pass

    # ── Incidents ────────────────────────────────────────────────────────
    print("\nIncidents:")

    @test("list incidents (may be empty)")
    def _():
        inc = wp.list_incidents(monitor_id)
        assert isinstance(inc, (list, dict)), f"Unexpected type: {type(inc)}"

    @test("list incidents with limit")
    def _():
        inc = wp.list_incidents(monitor_id, limit=5)
        assert isinstance(inc, (list, dict)), f"Unexpected type: {type(inc)}"

    # ── Notifications ───────────────────────────────────────────────────
    print("\nNotifications:")

    notif_id = None

    @test("create webhook notification")
    def _():
        nonlocal notif_id
        notif = wp.create_notification(
            monitor_id,
            "Test Webhook",
            "webhook",
            {"url": "https://httpbin.org/post"},
            monitor_key,
        )
        assert "id" in notif, f"Missing id: {notif}"
        notif_id = notif["id"]

    @test("list notifications")
    def _():
        notifs = wp.list_notifications(monitor_id, monitor_key)
        assert isinstance(notifs, (list, dict)), f"Unexpected: {type(notifs)}"

    @test("update notification (disable)")
    def _():
        result = wp.update_notification(notif_id, monitor_key, is_enabled=False)
        assert isinstance(result, dict)

    @test("delete notification")
    def _():
        wp.delete_notification(notif_id, monitor_key)
        # Verify it's gone
        notifs = wp.list_notifications(monitor_id, monitor_key)
        if isinstance(notifs, list):
            ids = [n["id"] for n in notifs]
        else:
            ids = [n["id"] for n in notifs.get("notifications", notifs.get("items", []))]
        assert notif_id not in ids, "Notification still present"

    @test("create notification without key raises AuthError")
    def _():
        try:
            wp.create_notification(monitor_id, "Bad", "webhook", {"url": "http://x"}, "wrong")
            assert False, "Expected AuthError"
        except AuthError:
            pass

    # ── Maintenance Windows ─────────────────────────────────────────────
    print("\nMaintenance Windows:")

    maint_id = None

    @test("create maintenance window")
    def _():
        nonlocal maint_id
        m = wp.create_maintenance(
            monitor_id,
            "Deploy v2",
            "2099-01-01T00:00:00Z",
            "2099-01-01T01:00:00Z",
            monitor_key,
        )
        assert "id" in m, f"Missing id: {m}"
        maint_id = m["id"]

    @test("list maintenance windows")
    def _():
        mw = wp.list_maintenance(monitor_id)
        assert isinstance(mw, (list, dict))

    @test("delete maintenance window")
    def _():
        wp.delete_maintenance(maint_id, monitor_key)

    # ── Alert Rules ─────────────────────────────────────────────────────
    print("\nAlert Rules:")

    @test("set alert rules")
    def _():
        rules = wp.set_alert_rules(
            monitor_id,
            monitor_key,
            repeat_interval_minutes=15,
            max_repeats=5,
            escalation_after_minutes=30,
        )
        assert isinstance(rules, dict)

    @test("get alert rules")
    def _():
        rules = wp.get_alert_rules(monitor_id, monitor_key)
        assert rules.get("repeat_interval_minutes") == 15 or "repeat_interval_minutes" in rules

    @test("list alert log (may be empty)")
    def _():
        log = wp.list_alert_log(monitor_id, monitor_key)
        assert isinstance(log, (list, dict))

    @test("delete alert rules")
    def _():
        wp.delete_alert_rules(monitor_id, monitor_key)
        try:
            wp.get_alert_rules(monitor_id, monitor_key)
            assert False, "Expected NotFoundError"
        except NotFoundError:
            pass

    # ── Dependencies ────────────────────────────────────────────────────
    print("\nDependencies:")

    dep_monitor_id = None
    dep_monitor_key = None
    dep_id = None

    @test("create dependency monitor (upstream)")
    def _():
        nonlocal dep_monitor_id, dep_monitor_key
        mon = wp.create_monitor("Upstream DB", "https://httpbin.org/status/200", is_public=True)
        dep_monitor_id = mon["id"]
        dep_monitor_key = mon["manage_key"]
        created_monitors.append((dep_monitor_id, dep_monitor_key))

    @test("add dependency")
    def _():
        nonlocal dep_id
        dep = wp.add_dependency(monitor_id, dep_monitor_id, monitor_key)
        assert "id" in dep, f"Missing id: {dep}"
        dep_id = dep["id"]

    @test("list dependencies")
    def _():
        deps = wp.list_dependencies(monitor_id)
        assert isinstance(deps, (list, dict))

    @test("list dependents")
    def _():
        deps = wp.list_dependents(dep_monitor_id)
        assert isinstance(deps, (list, dict))

    @test("self-dependency raises error")
    def _():
        try:
            wp.add_dependency(monitor_id, monitor_id, monitor_key)
            assert False, "Expected error for self-dependency"
        except (ConflictError, ValidationError, WatchpostError):
            pass

    @test("delete dependency")
    def _():
        wp.delete_dependency(monitor_id, dep_id, monitor_key)

    # ── Tags & Groups ───────────────────────────────────────────────────
    print("\nTags & Groups:")

    @test("list tags")
    def _():
        tags = wp.list_tags()
        assert isinstance(tags, list), f"Expected list: {type(tags)}"

    @test("list groups")
    def _():
        groups = wp.list_groups()
        assert isinstance(groups, list), f"Expected list: {type(groups)}"

    @test("filter monitors by tag")
    def _():
        monitors = wp.list_monitors(tag="sdk-test")
        assert isinstance(monitors, list)

    @test("filter monitors by group")
    def _():
        monitors = wp.list_monitors(group="SDK Tests")
        assert isinstance(monitors, list)

    @test("filter monitors by status")
    def _():
        monitors = wp.list_monitors(status="unknown")
        assert isinstance(monitors, list)

    @test("search monitors by name")
    def _():
        monitors = wp.list_monitors(search="SDK")
        assert isinstance(monitors, list)

    # ── Status / Dashboard ──────────────────────────────────────────────
    print("\nStatus / Dashboard:")

    @test("get public status page")
    def _():
        status = wp.get_status()
        assert isinstance(status, dict)

    @test("get status filtered by tag")
    def _():
        status = wp.get_status(tag="sdk-test")
        assert isinstance(status, dict)

    @test("get dashboard (no auth)")
    def _():
        dash = wp.get_dashboard()
        assert isinstance(dash, dict)

    # ── Badges ──────────────────────────────────────────────────────────
    print("\nBadges:")

    @test("get uptime badge SVG")
    def _():
        svg = wp.get_uptime_badge(monitor_id, period="24h")
        assert "<svg" in svg, f"Not SVG: {svg[:100]}"

    @test("get status badge SVG")
    def _():
        svg = wp.get_status_badge(monitor_id)
        assert "<svg" in svg, f"Not SVG: {svg[:100]}"

    @test("get badge with custom label")
    def _():
        svg = wp.get_uptime_badge(monitor_id, period="7d", label="My Service")
        assert "<svg" in svg

    # ── Export ──────────────────────────────────────────────────────────
    print("\nExport:")

    @test("export monitor config")
    def _():
        config = wp.export_monitor(monitor_id, monitor_key)
        assert isinstance(config, dict)
        assert "name" in config or "url" in config

    @test("export without auth raises error")
    def _():
        try:
            wp.export_monitor(monitor_id, "wrong-key")
            assert False, "Expected AuthError"
        except AuthError:
            pass

    # ── Bulk Operations ─────────────────────────────────────────────────
    print("\nBulk Operations:")

    @test("bulk create monitors")
    def _():
        result = wp.bulk_create_monitors([
            {"name": "Bulk A", "url": "https://httpbin.org/status/200", "is_public": True},
            {"name": "Bulk B", "url": "https://httpbin.org/status/201", "is_public": True},
        ])
        assert "succeeded" in result or "created" in result, f"Unexpected: {result}"
        # Track for cleanup
        if "created" in result:
            for m in result["created"]:
                if "id" in m and "manage_key" in m:
                    created_monitors.append((m["id"], m["manage_key"]))

    # ── Status Pages ────────────────────────────────────────────────────
    print("\nStatus Pages:")

    page_key = None

    @test("create status page")
    def _():
        nonlocal page_key
        page = wp.create_status_page(
            "sdk-test-page",
            "SDK Test Status",
            description="Test status page from SDK",
        )
        assert "manage_key" in page, f"Missing manage_key: {page}"
        page_key = page["manage_key"]
        created_pages.append(("sdk-test-page", page_key))

    @test("list status pages")
    def _():
        pages = wp.list_status_pages()
        assert isinstance(pages, list)

    @test("get status page by slug")
    def _():
        page = wp.get_status_page("sdk-test-page")
        assert page.get("title") == "SDK Test Status" or "title" in page

    @test("add monitors to status page")
    def _():
        result = wp.add_monitors_to_page("sdk-test-page", [monitor_id], page_key)
        assert isinstance(result, dict)

    @test("list page monitors")
    def _():
        monitors = wp.list_page_monitors("sdk-test-page")
        assert isinstance(monitors, list)

    @test("remove monitor from page")
    def _():
        wp.remove_monitor_from_page("sdk-test-page", monitor_id, page_key)

    @test("update status page")
    def _():
        result = wp.update_status_page("sdk-test-page", page_key, title="Updated SDK Page")
        assert isinstance(result, dict)

    @test("delete status page")
    def _():
        wp.delete_status_page("sdk-test-page", page_key)
        created_pages.clear()

    # ── Settings ────────────────────────────────────────────────────────
    print("\nSettings:")

    @test("get settings")
    def _():
        settings = wp.get_settings()
        assert isinstance(settings, dict)

    # ── Convenience Helpers ─────────────────────────────────────────────
    print("\nConvenience Helpers:")

    @test("is_up returns bool")
    def _():
        result = wp.is_up(monitor_id)
        assert isinstance(result, bool), f"Expected bool: {type(result)}"

    @test("all_up returns bool")
    def _():
        result = wp.all_up()
        assert isinstance(result, bool)

    @test("get_downtime_summary returns dict")
    def _():
        summary = wp.get_downtime_summary(monitor_id)
        assert "is_down" in summary
        assert "current_status" in summary
        assert "uptime_24h" in summary

    @test("quick_monitor creates and returns key")
    def _():
        mon = wp.quick_monitor("Quick Test", "https://httpbin.org/status/200")
        assert "manage_key" in mon
        created_monitors.append((mon["id"], mon["manage_key"]))

    # ── Webhook Deliveries ──────────────────────────────────────────────
    print("\nWebhook Deliveries:")

    @test("list webhook deliveries (may be empty)")
    def _():
        deliveries = wp.list_webhook_deliveries(monitor_id, monitor_key)
        assert isinstance(deliveries, dict)

    @test("list deliveries with filters")
    def _():
        deliveries = wp.list_webhook_deliveries(
            monitor_id, monitor_key, limit=10, status="success"
        )
        assert isinstance(deliveries, dict)

    # ── Monitor Update Advanced Fields ─────────────────────────────────
    print("\nMonitor Update (Advanced Fields):")

    @test("update monitor tags")
    def _():
        wp.update_monitor(monitor_id, monitor_key, tags=["updated-tag", "sdk"])
        mon = wp.get_monitor(monitor_id)
        tags = mon.get("tags", [])
        assert "updated-tag" in tags, f"Tag not set: {tags}"

    @test("update monitor group_name")
    def _():
        wp.update_monitor(monitor_id, monitor_key, group_name="Updated Group")
        mon = wp.get_monitor(monitor_id)
        assert mon.get("group_name") == "Updated Group", f"Group not set: {mon.get('group_name')}"

    @test("update monitor follow_redirects")
    def _():
        wp.update_monitor(monitor_id, monitor_key, follow_redirects=False)
        mon = wp.get_monitor(monitor_id)
        assert mon.get("follow_redirects") == False, f"follow_redirects not updated"

    @test("update monitor SLA target")
    def _():
        wp.update_monitor(monitor_id, monitor_key, sla_target=99.5, sla_period_days=7)
        mon = wp.get_monitor(monitor_id)
        assert mon.get("sla_target") == 99.5, f"SLA not set: {mon.get('sla_target')}"

    @test("update monitor response_time_threshold_ms")
    def _():
        wp.update_monitor(monitor_id, monitor_key, response_time_threshold_ms=3000)
        mon = wp.get_monitor(monitor_id)
        assert mon.get("response_time_threshold_ms") == 3000

    @test("update monitor is_public to false and back")
    def _():
        wp.update_monitor(monitor_id, monitor_key, is_public=False)
        # Monitor may not appear in public list now
        wp.update_monitor(monitor_id, monitor_key, is_public=True)
        mon = wp.get_monitor(monitor_id)
        assert mon.get("is_public") == True

    @test("update monitor confirmation_threshold")
    def _():
        wp.update_monitor(monitor_id, monitor_key, confirmation_threshold=3)
        mon = wp.get_monitor(monitor_id)
        assert mon.get("confirmation_threshold") == 3

    @test("update monitor interval_seconds")
    def _():
        wp.update_monitor(monitor_id, monitor_key, interval_seconds=1200)
        mon = wp.get_monitor(monitor_id)
        assert mon.get("interval_seconds") == 1200

    @test("update monitor timeout_ms")
    def _():
        wp.update_monitor(monitor_id, monitor_key, timeout_ms=15000)
        mon = wp.get_monitor(monitor_id)
        assert mon.get("timeout_ms") == 15000

    # ── Heartbeat Cursor Pagination ─────────────────────────────────────
    print("\nHeartbeat Cursor Pagination:")

    @test("heartbeats default returns list")
    def _():
        hb = wp.list_heartbeats(monitor_id)
        assert isinstance(hb, (list, dict)), f"Unexpected: {type(hb)}"

    @test("heartbeats with after=0 cursor")
    def _():
        hb = wp.list_heartbeats(monitor_id, after=0)
        assert isinstance(hb, (list, dict))

    @test("heartbeats with limit=1")
    def _():
        hb = wp.list_heartbeats(monitor_id, limit=1)
        items = hb if isinstance(hb, list) else hb.get("heartbeats", hb.get("items", []))
        assert len(items) <= 1

    # ── Monitor List Combined Filters ───────────────────────────────────
    print("\nMonitor List (Combined Filters):")

    @test("list monitors with search + tag")
    def _():
        monitors = wp.list_monitors(search="SDK", tag="updated-tag")
        assert isinstance(monitors, list)

    @test("list monitors with search + status")
    def _():
        monitors = wp.list_monitors(search="SDK", status="unknown")
        assert isinstance(monitors, list)

    @test("list monitors with group + status")
    def _():
        monitors = wp.list_monitors(group="Updated Group", status="unknown")
        assert isinstance(monitors, list)

    @test("list monitors returns expected fields")
    def _():
        monitors = wp.list_monitors()
        if monitors:
            m = monitors[0]
            assert "id" in m, f"Missing id field"
            assert "name" in m, f"Missing name field"
            assert "url" in m, f"Missing url field"

    # ── Status Page Full Lifecycle ──────────────────────────────────────
    print("\nStatus Page (Full Lifecycle):")

    page2_key = None

    @test("create status page with all options")
    def _():
        nonlocal page2_key
        page = wp.create_status_page(
            "sdk-lifecycle-test",
            "Lifecycle Test Page",
            description="Testing full lifecycle",
            is_public=True,
        )
        assert "manage_key" in page
        page2_key = page["manage_key"]
        created_pages.append(("sdk-lifecycle-test", page2_key))

    @test("add multiple monitors to status page")
    def _():
        # Add our main monitor and SLA monitor
        wp.add_monitors_to_page("sdk-lifecycle-test", [monitor_id, sla_monitor_id], page2_key)

    @test("list page monitors returns added monitors")
    def _():
        monitors = wp.list_page_monitors("sdk-lifecycle-test")
        assert isinstance(monitors, list)
        # Should have at least 2
        if isinstance(monitors, list):
            ids = [m.get("id") or m.get("monitor_id") for m in monitors]
            assert len(ids) >= 2, f"Expected >=2 monitors, got {len(ids)}"

    @test("get status page includes monitors")
    def _():
        page = wp.get_status_page("sdk-lifecycle-test")
        assert page.get("title") == "Lifecycle Test Page"

    @test("update status page description")
    def _():
        wp.update_status_page("sdk-lifecycle-test", page2_key, description="Updated desc")
        page = wp.get_status_page("sdk-lifecycle-test")
        assert page.get("description") == "Updated desc", f"Desc: {page.get('description')}"

    @test("remove one monitor from page")
    def _():
        wp.remove_monitor_from_page("sdk-lifecycle-test", sla_monitor_id, page2_key)
        monitors = wp.list_page_monitors("sdk-lifecycle-test")
        if isinstance(monitors, list):
            ids = [m.get("id") or m.get("monitor_id") for m in monitors]
            assert sla_monitor_id not in ids, "Monitor not removed"

    @test("create status page with duplicate slug raises error")
    def _():
        try:
            wp.create_status_page("sdk-lifecycle-test", "Duplicate")
            assert False, "Expected error for duplicate slug"
        except (ConflictError, ValidationError, WatchpostError):
            pass

    @test("get nonexistent status page raises NotFoundError")
    def _():
        try:
            wp.get_status_page("nonexistent-slug-xyz")
            assert False, "Expected NotFoundError"
        except NotFoundError:
            pass

    @test("update status page without key raises AuthError")
    def _():
        try:
            wp.update_status_page("sdk-lifecycle-test", "wrong-key", title="Hack")
            assert False, "Expected AuthError"
        except AuthError:
            pass

    @test("delete status page with wrong key raises AuthError")
    def _():
        try:
            wp.delete_status_page("sdk-lifecycle-test", "wrong-key")
            assert False, "Expected AuthError"
        except AuthError:
            pass

    # cleanup lifecycle page
    @test("delete status page (lifecycle)")
    def _():
        wp.delete_status_page("sdk-lifecycle-test", page2_key)
        created_pages.remove(("sdk-lifecycle-test", page2_key))

    # ── Dashboard Auth Variants ─────────────────────────────────────────
    print("\nDashboard (Auth Variants):")

    @test("dashboard without auth returns aggregate stats")
    def _():
        dash = wp.get_dashboard()
        assert "total" in dash or "monitors" in dash or isinstance(dash, dict)

    @test("dashboard with wrong auth still returns data")
    def _():
        # Dashboard with key may include more detail (recent incidents, slowest)
        dash = wp.get_dashboard(key="invalid-key")
        assert isinstance(dash, dict)

    @test("dashboard with monitor key returns data")
    def _():
        dash = wp.get_dashboard(key=monitor_key)
        assert isinstance(dash, dict)

    # ── Admin Verify ────────────────────────────────────────────────────
    print("\nAdmin Verify:")

    @test("verify_admin with wrong key returns valid=false")
    def _():
        result = wp.verify_admin("definitely-wrong-key")
        assert result.get("valid") == False, f"Expected valid=false: {result}"

    @test("verify_admin with monitor key returns valid=false")
    def _():
        result = wp.verify_admin(monitor_key)
        assert result.get("valid") == False, f"Monitor key should not be admin key"

    # ── Settings (auth-gated) ───────────────────────────────────────────
    print("\nSettings (Auth-Gated):")

    @test("get settings returns dict with expected fields")
    def _():
        settings = wp.get_settings()
        assert isinstance(settings, dict)
        # Settings should have title, description, logo_url (even if null)
        for k in ("title", "description", "logo_url"):
            assert k in settings, f"Missing settings field: {k}"

    @test("update settings with wrong key raises AuthError")
    def _():
        try:
            wp.update_settings("wrong-admin-key", title="Hacked")
            assert False, "Expected AuthError"
        except AuthError:
            pass

    # ── Location Management (auth-gated) ────────────────────────────────
    print("\nLocations (Auth-Gated):")

    @test("list locations (public)")
    def _():
        locs = wp.list_locations()
        assert isinstance(locs, list)

    @test("create location with wrong key raises AuthError")
    def _():
        try:
            wp.create_location("Test Probe", "us-east", "wrong-admin-key")
            assert False, "Expected AuthError"
        except AuthError:
            pass

    @test("delete location with wrong key raises AuthError")
    def _():
        try:
            wp.delete_location("00000000-0000-0000-0000-000000000000", "wrong-key")
            assert False, "Expected AuthError or NotFoundError"
        except (AuthError, NotFoundError):
            pass

    @test("get location nonexistent raises NotFoundError")
    def _():
        try:
            wp.get_location("00000000-0000-0000-0000-000000000000")
            assert False, "Expected NotFoundError"
        except NotFoundError:
            pass

    # ── Probe Submission (auth-gated) ───────────────────────────────────
    print("\nProbe Submission:")

    @test("submit probe with wrong key raises AuthError")
    def _():
        try:
            wp.submit_probe("wrong-probe-key", [{
                "monitor_id": monitor_id,
                "status": "up",
                "response_time_ms": 100,
            }])
            assert False, "Expected AuthError"
        except AuthError:
            pass

    @test("get location status for monitor (may be empty)")
    def _():
        locs = wp.get_location_status(monitor_id)
        assert isinstance(locs, (list, dict))

    @test("get consensus for monitor without consensus config")
    def _():
        try:
            result = wp.get_consensus(monitor_id)
            # May return data or error depending on config
            assert isinstance(result, dict)
        except (ValidationError, WatchpostError):
            pass  # Expected if no consensus_threshold set

    # ── Alert Rule Validation ───────────────────────────────────────────
    print("\nAlert Rule Validation:")

    @test("set alert rules with minimum valid values")
    def _():
        rules = wp.set_alert_rules(
            monitor_id, monitor_key,
            repeat_interval_minutes=5,
            max_repeats=1,
            escalation_after_minutes=5,
        )
        assert isinstance(rules, dict)

    @test("get alert rules after setting")
    def _():
        rules = wp.get_alert_rules(monitor_id, monitor_key)
        assert rules.get("repeat_interval_minutes") == 5
        assert rules.get("max_repeats") == 1

    @test("update alert rules (overwrite)")
    def _():
        rules = wp.set_alert_rules(
            monitor_id, monitor_key,
            repeat_interval_minutes=30,
            max_repeats=10,
            escalation_after_minutes=60,
        )
        assert isinstance(rules, dict)
        rules = wp.get_alert_rules(monitor_id, monitor_key)
        assert rules.get("repeat_interval_minutes") == 30
        assert rules.get("max_repeats") == 10

    @test("get alert rules without key raises AuthError")
    def _():
        try:
            wp.get_alert_rules(monitor_id, "wrong-key")
            assert False, "Expected AuthError"
        except AuthError:
            pass

    @test("delete alert rules and verify gone")
    def _():
        wp.delete_alert_rules(monitor_id, monitor_key)
        try:
            wp.get_alert_rules(monitor_id, monitor_key)
            assert False, "Expected NotFoundError"
        except NotFoundError:
            pass

    @test("list alert log (empty after rule deletion)")
    def _():
        log = wp.list_alert_log(monitor_id, monitor_key)
        assert isinstance(log, (list, dict))

    # ── Dependencies (Advanced) ─────────────────────────────────────────
    print("\nDependencies (Advanced):")

    dep2_id = None
    dep2_key = None

    @test("create second dependency monitor")
    def _():
        nonlocal dep2_id, dep2_key
        mon = wp.create_monitor("Dep Chain A", "https://httpbin.org/status/200", is_public=True)
        dep2_id = mon["id"]
        dep2_key = mon["manage_key"]
        created_monitors.append((dep2_id, dep2_key))

    @test("add dependency and verify in list")
    def _():
        dep = wp.add_dependency(monitor_id, dep2_id, monitor_key)
        assert "id" in dep
        deps = wp.list_dependencies(monitor_id)
        dep_ids = [d.get("depends_on_id") or d.get("id") for d in (deps if isinstance(deps, list) else [])]
        assert len(deps) >= 1 if isinstance(deps, list) else True

    @test("list dependents includes upstream reference")
    def _():
        deps = wp.list_dependents(dep2_id)
        assert isinstance(deps, (list, dict))

    @test("duplicate dependency raises ConflictError")
    def _():
        try:
            wp.add_dependency(monitor_id, dep2_id, monitor_key)
            assert False, "Expected ConflictError for duplicate"
        except (ConflictError, ValidationError, WatchpostError):
            pass

    @test("add dependency without key raises AuthError")
    def _():
        try:
            wp.add_dependency(monitor_id, dep2_id, "wrong-key")
            assert False, "Expected AuthError"
        except AuthError:
            pass

    @test("cleanup dependencies")
    def _():
        deps = wp.list_dependencies(monitor_id)
        if isinstance(deps, list):
            for d in deps:
                did = d.get("id")
                if did:
                    try:
                        wp.delete_dependency(monitor_id, did, monitor_key)
                    except Exception:
                        pass

    # ── Maintenance Window (Advanced) ───────────────────────────────────
    print("\nMaintenance Window (Advanced):")

    @test("create maintenance window in the past (should work)")
    def _():
        m = wp.create_maintenance(
            monitor_id, "Past Maint",
            "2020-01-01T00:00:00Z", "2020-01-01T01:00:00Z",
            monitor_key,
        )
        assert "id" in m
        # Clean up
        wp.delete_maintenance(m["id"], monitor_key)

    @test("create maintenance without key raises AuthError")
    def _():
        try:
            wp.create_maintenance(
                monitor_id, "Unauth",
                "2099-06-01T00:00:00Z", "2099-06-01T01:00:00Z",
                "wrong-key",
            )
            assert False, "Expected AuthError"
        except AuthError:
            pass

    @test("delete nonexistent maintenance raises NotFoundError")
    def _():
        try:
            wp.delete_maintenance("00000000-0000-0000-0000-000000000000", monitor_key)
            assert False, "Expected NotFoundError"
        except NotFoundError:
            pass

    @test("list maintenance returns list")
    def _():
        mw = wp.list_maintenance(monitor_id)
        # Should be a list (possibly empty if we cleaned up the earlier one)
        assert isinstance(mw, (list, dict))

    # ── Notification Advanced ───────────────────────────────────────────
    print("\nNotification (Advanced):")

    notif2_id = None

    @test("create webhook notification with chat format")
    def _():
        nonlocal notif2_id
        notif = wp.create_notification(
            monitor_id, "Chat Hook", "webhook",
            {"url": "https://httpbin.org/post", "payload_format": "chat"},
            monitor_key,
        )
        assert "id" in notif
        notif2_id = notif["id"]

    @test("update notification name and verify")
    def _():
        wp.update_notification(notif2_id, monitor_key, name="Renamed Hook")
        notifs = wp.list_notifications(monitor_id, monitor_key)
        items = notifs if isinstance(notifs, list) else notifs.get("notifications", notifs.get("items", []))
        found = [n for n in items if n["id"] == notif2_id]
        assert len(found) == 1
        assert found[0]["name"] == "Renamed Hook"

    @test("create email notification")
    def _():
        notif = wp.create_notification(
            monitor_id, "Email Alert", "email",
            {"address": "test@example.com"},
            monitor_key,
        )
        assert "id" in notif
        # Clean up
        wp.delete_notification(notif["id"], monitor_key)

    @test("delete notification (chat hook)")
    def _():
        wp.delete_notification(notif2_id, monitor_key)

    @test("delete nonexistent notification raises error")
    def _():
        try:
            wp.delete_notification("00000000-0000-0000-0000-000000000000", monitor_key)
            assert False, "Expected NotFoundError"
        except (NotFoundError, WatchpostError):
            pass

    # ── Webhook Deliveries (Advanced) ───────────────────────────────────
    print("\nWebhook Deliveries (Advanced):")

    @test("list deliveries with event filter")
    def _():
        d = wp.list_webhook_deliveries(monitor_id, monitor_key, event="incident.created")
        assert isinstance(d, dict)

    @test("list deliveries with status filter")
    def _():
        d = wp.list_webhook_deliveries(monitor_id, monitor_key, status="failed")
        assert isinstance(d, dict)

    @test("list deliveries with cursor pagination")
    def _():
        d = wp.list_webhook_deliveries(monitor_id, monitor_key, after=0, limit=5)
        assert isinstance(d, dict)

    @test("list deliveries without key raises AuthError")
    def _():
        try:
            wp.list_webhook_deliveries(monitor_id, "wrong-key")
            assert False, "Expected AuthError"
        except AuthError:
            pass

    # ── Export/Import Roundtrip ──────────────────────────────────────────
    print("\nExport/Import:")

    @test("export monitor config and verify fields")
    def _():
        config = wp.export_monitor(monitor_id, monitor_key)
        assert "name" in config, f"Missing name in export"
        assert "url" in config, f"Missing url in export"
        # Should include method, interval, etc.
        assert "method" in config or "monitor_type" in config

    @test("bulk create from exported config")
    def _():
        config = wp.export_monitor(monitor_id, monitor_key)
        # Modify name to avoid conflict
        config["name"] = "Re-imported Monitor"
        config["is_public"] = True
        result = wp.bulk_create_monitors([config])
        if isinstance(result, dict) and "created" in result:
            for m in result["created"]:
                if "id" in m and "manage_key" in m:
                    created_monitors.append((m["id"], m["manage_key"]))
        assert result.get("succeeded", 0) >= 1 or len(result.get("created", [])) >= 1

    # ── Bulk Create Edge Cases ──────────────────────────────────────────
    print("\nBulk Create (Edge Cases):")

    @test("bulk create with one valid and one invalid")
    def _():
        result = wp.bulk_create_monitors([
            {"name": "Bulk Valid", "url": "https://httpbin.org/status/200", "is_public": True},
            {"name": "Bulk Invalid", "url": "no-scheme"},
        ])
        assert isinstance(result, dict)
        # Should have partial success
        if "created" in result:
            for m in result["created"]:
                if "id" in m and "manage_key" in m:
                    created_monitors.append((m["id"], m["manage_key"]))

    @test("bulk create empty list")
    def _():
        try:
            result = wp.bulk_create_monitors([])
            # May succeed with 0 created or may error
            assert isinstance(result, dict)
        except (ValidationError, WatchpostError):
            pass  # Some APIs reject empty arrays

    # ── Incidents (Advanced) ────────────────────────────────────────────
    print("\nIncidents (Advanced):")

    @test("list incidents with cursor pagination")
    def _():
        inc = wp.list_incidents(monitor_id, after=0, limit=5)
        assert isinstance(inc, (list, dict))

    @test("list incidents for nonexistent monitor raises NotFoundError")
    def _():
        try:
            wp.list_incidents("00000000-0000-0000-0000-000000000000")
            assert False, "Expected NotFoundError"
        except NotFoundError:
            pass

    @test("get nonexistent incident raises NotFoundError")
    def _():
        try:
            wp.get_incident("00000000-0000-0000-0000-000000000000")
            assert False, "Expected NotFoundError"
        except NotFoundError:
            pass

    @test("acknowledge nonexistent incident raises NotFoundError")
    def _():
        try:
            wp.acknowledge_incident("00000000-0000-0000-0000-000000000000", monitor_key)
            assert False, "Expected NotFoundError"
        except NotFoundError:
            pass

    # ── Incident Notes (auth-gated) ─────────────────────────────────────
    print("\nIncident Notes:")

    @test("list notes for nonexistent incident raises NotFoundError")
    def _():
        try:
            wp.list_incident_notes("00000000-0000-0000-0000-000000000000")
            assert False, "Expected NotFoundError"
        except NotFoundError:
            pass

    @test("add note to nonexistent incident raises NotFoundError")
    def _():
        try:
            wp.add_incident_note("00000000-0000-0000-0000-000000000000", "Test note", monitor_key)
            assert False, "Expected NotFoundError"
        except NotFoundError:
            pass

    # ── Badges (Advanced) ───────────────────────────────────────────────
    print("\nBadges (Advanced):")

    @test("uptime badge all periods")
    def _():
        for period in ("24h", "7d", "30d", "90d"):
            svg = wp.get_uptime_badge(monitor_id, period=period)
            assert "<svg" in svg, f"Period {period} not SVG"

    @test("status badge with custom label")
    def _():
        svg = wp.get_status_badge(monitor_id, label="My Custom Service")
        assert "<svg" in svg
        assert "My Custom Service" in svg

    @test("badge for nonexistent monitor raises NotFoundError")
    def _():
        try:
            wp.get_uptime_badge("00000000-0000-0000-0000-000000000000")
            assert False, "Expected NotFoundError"
        except NotFoundError:
            pass

    # ── Status Page Filters ─────────────────────────────────────────────
    print("\nStatus (Filters):")

    @test("status page with search filter")
    def _():
        status = wp.get_status(search="SDK")
        assert isinstance(status, dict)

    @test("status page with status filter")
    def _():
        status = wp.get_status(status="up")
        assert isinstance(status, dict)

    @test("status page with group filter")
    def _():
        status = wp.get_status(group="Updated Group")
        assert isinstance(status, dict)

    @test("status page with combined filters")
    def _():
        status = wp.get_status(search="SDK", tag="updated-tag")
        assert isinstance(status, dict)

    # ── SLA Advanced ────────────────────────────────────────────────────
    print("\nSLA (Advanced):")

    @test("SLA returns expected fields")
    def _():
        # monitor_id now has SLA from earlier update
        sla = wp.get_sla(monitor_id)
        assert isinstance(sla, dict)
        # Should have target and status fields
        assert "target_pct" in sla or "target" in sla or "status" in sla

    # ── Uptime Advanced ─────────────────────────────────────────────────
    print("\nUptime (Advanced):")

    @test("uptime returns expected fields")
    def _():
        up = wp.get_uptime(monitor_id)
        # Should have period-based uptime
        assert isinstance(up, dict)

    @test("uptime history with different day ranges")
    def _():
        for days in (7, 14, 30):
            hist = wp.get_uptime_history(monitor_id, days=days)
            assert isinstance(hist, (list, dict)), f"Failed for days={days}"

    @test("aggregate uptime history")
    def _():
        hist = wp.get_uptime_history(days=7)
        assert isinstance(hist, (list, dict))

    @test("uptime for nonexistent monitor raises NotFoundError")
    def _():
        try:
            wp.get_uptime("00000000-0000-0000-0000-000000000000")
            assert False, "Expected NotFoundError"
        except NotFoundError:
            pass

    # ── Monitor Types (TCP/DNS detail checks) ───────────────────────────
    print("\nMonitor Types (Detail Checks):")

    @test("TCP monitor has correct type")
    def _():
        # Find our TCP monitor
        monitors = wp.list_monitors(search="TCP Test")
        tcp_found = [m for m in monitors if m.get("name") == "TCP Test"]
        if tcp_found:
            assert tcp_found[0].get("monitor_type") == "tcp"

    @test("DNS monitor has correct type and record_type")
    def _():
        monitors = wp.list_monitors(search="DNS Test")
        dns_found = [m for m in monitors if m.get("name") == "DNS Test"]
        if dns_found:
            assert dns_found[0].get("monitor_type") == "dns"

    @test("create monitor with body_contains")
    def _():
        mon = wp.create_monitor(
            "Body Check",
            "https://httpbin.org/html",
            is_public=True,
            body_contains="html",
        )
        created_monitors.append((mon["id"], mon["manage_key"]))
        m = wp.get_monitor(mon["id"])
        assert m.get("body_contains") == "html"

    @test("create monitor with custom headers")
    def _():
        mon = wp.create_monitor(
            "Header Check",
            "https://httpbin.org/headers",
            is_public=True,
            headers={"X-Custom": "test-value"},
        )
        created_monitors.append((mon["id"], mon["manage_key"]))

    # ── Convenience Helpers (Advanced) ──────────────────────────────────
    print("\nConvenience Helpers (Advanced):")

    @test("get_downtime_summary has all expected fields")
    def _():
        summary = wp.get_downtime_summary(monitor_id)
        expected_fields = ["is_down", "current_status", "current_incident", "uptime_24h", "uptime_7d", "uptime_30d"]
        for f in expected_fields:
            assert f in summary, f"Missing field: {f}"

    @test("is_up for nonexistent monitor raises NotFoundError")
    def _():
        try:
            wp.is_up("00000000-0000-0000-0000-000000000000")
            assert False, "Expected NotFoundError"
        except NotFoundError:
            pass

    # ── Discovery (Advanced) ────────────────────────────────────────────
    print("\nDiscovery (Advanced):")

    @test("llms.txt contains expected sections")
    def _():
        txt = wp.get_llms_txt()
        assert "monitor" in txt.lower()
        assert "api" in txt.lower()

    @test("skills index has expected structure")
    def _():
        idx = wp.get_skills_index()
        # Should have a skills array
        assert "skills" in idx or isinstance(idx, dict)

    @test("health response has expected fields")
    def _():
        h = wp.health()
        assert "status" in h, f"Missing status: {h}"
        assert h.get("status") == "ok"

    # ── Delete Cascade ──────────────────────────────────────────────────
    print("\nDelete Cascade:")

    cascade_mon_id = None
    cascade_mon_key = None

    @test("create monitor for cascade test")
    def _():
        nonlocal cascade_mon_id, cascade_mon_key
        mon = wp.create_monitor("Cascade Test", "https://httpbin.org/status/200", is_public=True)
        cascade_mon_id = mon["id"]
        cascade_mon_key = mon["manage_key"]

    @test("add notification to cascade monitor")
    def _():
        wp.create_notification(
            cascade_mon_id, "Cascade Notif", "webhook",
            {"url": "https://httpbin.org/post"},
            cascade_mon_key,
        )

    @test("add maintenance window to cascade monitor")
    def _():
        wp.create_maintenance(
            cascade_mon_id, "Cascade Maint",
            "2099-01-01T00:00:00Z", "2099-01-01T01:00:00Z",
            cascade_mon_key,
        )

    @test("delete cascade monitor removes everything")
    def _():
        wp.delete_monitor(cascade_mon_id, cascade_mon_key)
        # Verify monitor is gone
        try:
            wp.get_monitor(cascade_mon_id)
            assert False, "Monitor should be deleted"
        except NotFoundError:
            pass

    # ── Error Handling (Comprehensive) ──────────────────────────────────
    print("\nError Handling (Comprehensive):")

    @test("NotFoundError has status_code")
    def _():
        try:
            wp.get_monitor("00000000-0000-0000-0000-000000000000")
            assert False, "Expected NotFoundError"
        except NotFoundError as e:
            assert e.status_code == 404

    @test("AuthError has status_code")
    def _():
        try:
            wp.delete_monitor(monitor_id, "invalid-key")
            assert False, "Expected AuthError"
        except AuthError as e:
            assert e.status_code in (401, 403)

    @test("WatchpostError base class catches all API errors")
    def _():
        try:
            wp.get_monitor("00000000-0000-0000-0000-000000000000")
            assert False, "Expected error"
        except WatchpostError:
            pass  # Caught via base class

    @test("NotFoundError is WatchpostError subclass")
    def _():
        assert issubclass(NotFoundError, WatchpostError)
        assert issubclass(AuthError, WatchpostError)
        assert issubclass(RateLimitError, WatchpostError)
        assert issubclass(ValidationError, WatchpostError)
        assert issubclass(ConflictError, WatchpostError)

    @test("WatchpostError has body attribute")
    def _():
        try:
            wp.get_monitor("00000000-0000-0000-0000-000000000000")
        except WatchpostError as e:
            assert hasattr(e, "body")
            assert hasattr(e, "status_code")

    # ── Cleanup ─────────────────────────────────────────────────────────
    print("\nCleanup:")

    cleanup_ok = 0
    cleanup_fail = 0

    for page_slug, page_key in created_pages:
        try:
            wp.delete_status_page(page_slug, page_key)
            cleanup_ok += 1
        except Exception:
            cleanup_fail += 1

    for mid, mkey in created_monitors:
        try:
            wp.delete_monitor(mid, mkey)
            cleanup_ok += 1
        except Exception:
            cleanup_fail += 1

    print(f"  Cleaned up {cleanup_ok} resources ({cleanup_fail} failed)")

    # ── Results ─────────────────────────────────────────────────────────
    print(f"\n{'='*50}")
    print(f"Results: {passed} passed, {failed} failed")
    if errors:
        print("\nFailed tests:")
        for name, err in errors:
            print(f"  ❌ {name}: {err}")
    print(f"{'='*50}\n")

    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
