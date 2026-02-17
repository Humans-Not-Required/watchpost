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

    # ── Error Handling ──────────────────────────────────────────────────
    print("\nError Handling:")

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
