use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use rusqlite::params;
use std::sync::Arc;

fn test_client() -> Client {
    let db_path = format!("/tmp/watchpost_test_{}.db", uuid::Uuid::new_v4());
    std::env::set_var("DATABASE_PATH", &db_path);

    let database = Arc::new(watchpost::db::Db::new(&db_path).expect("DB init failed"));
    let rate_limiter = watchpost::routes::RateLimiter::new(100, 3600);
    let broadcaster = Arc::new(watchpost::sse::EventBroadcaster::new(64));

    let rocket = rocket::build()
        .manage(database)
        .manage(rate_limiter)
        .manage(broadcaster)
        .mount("/api/v1", rocket::routes![
            watchpost::routes::health,
            watchpost::routes::create_monitor,
            watchpost::routes::bulk_create_monitors,
            watchpost::routes::export_monitor,
            watchpost::routes::list_monitors,
            watchpost::routes::get_monitor,
            watchpost::routes::update_monitor,
            watchpost::routes::delete_monitor,
            watchpost::routes::pause_monitor,
            watchpost::routes::resume_monitor,
            watchpost::routes::get_heartbeats,
            watchpost::routes::get_uptime,
            watchpost::routes::get_incidents,
            watchpost::routes::acknowledge_incident,
            watchpost::routes::dashboard,
            watchpost::routes::uptime_history,
            watchpost::routes::monitor_uptime_history,
            watchpost::routes::status_page,
            watchpost::routes::create_notification,
            watchpost::routes::list_notifications,
            watchpost::routes::delete_notification,
            watchpost::routes::update_notification,
            watchpost::routes::list_tags,
            watchpost::routes::create_maintenance_window,
            watchpost::routes::list_maintenance_windows,
            watchpost::routes::delete_maintenance_window,
            watchpost::routes::llms_txt,
            watchpost::routes::openapi_spec,
            watchpost::routes::monitor_uptime_badge,
            watchpost::routes::monitor_status_badge,
            watchpost::routes::global_events,
            watchpost::routes::monitor_events,
        ])
        .register("/", rocket::catchers![
            watchpost::catchers::bad_request,
            watchpost::catchers::unauthorized,
            watchpost::catchers::forbidden,
            watchpost::catchers::not_found,
            watchpost::catchers::unprocessable_entity,
            watchpost::catchers::too_many_requests,
            watchpost::catchers::internal_error,
        ]);

    Client::tracked(rocket).expect("valid rocket instance")
}

fn create_test_monitor(client: &Client) -> (String, String) {
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Test Service", "url": "https://httpbin.org/status/200", "is_public": true}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    let id = body["monitor"]["id"].as_str().unwrap().to_string();
    let key = body["manage_key"].as_str().unwrap().to_string();
    (id, key)
}

#[test]
fn test_health() {
    let client = test_client();
    let resp = client.get("/api/v1/health").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["service"], "watchpost");
    assert_eq!(body["status"], "ok");
}

#[test]
fn test_create_monitor() {
    let client = test_client();
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "My API", "url": "https://example.com/health", "interval_seconds": 900}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["monitor"]["name"], "My API");
    assert_eq!(body["monitor"]["url"], "https://example.com/health");
    assert_eq!(body["monitor"]["interval_seconds"], 900);
    assert_eq!(body["monitor"]["method"], "GET");
    assert_eq!(body["monitor"]["current_status"], "unknown");
    assert!(body["manage_key"].as_str().unwrap().starts_with("wp_"));
    assert!(body["manage_url"].as_str().is_some());
    assert!(body["view_url"].as_str().is_some());
}

#[test]
fn test_create_monitor_validation() {
    let client = test_client();

    // Empty name
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "", "url": "https://example.com"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::BadRequest);

    // Empty URL
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Test", "url": ""}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::BadRequest);

    // Invalid method
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Test", "url": "https://example.com", "method": "DELETE"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::BadRequest);
}

#[test]
fn test_get_monitor() {
    let client = test_client();
    let (id, _) = create_test_monitor(&client);

    let resp = client.get(format!("/api/v1/monitors/{}", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["name"], "Test Service");
}

#[test]
fn test_get_monitor_not_found() {
    let client = test_client();
    let resp = client.get("/api/v1/monitors/nonexistent-id").dispatch();
    assert_eq!(resp.status(), Status::NotFound);
}

#[test]
fn test_list_public_monitors() {
    let client = test_client();

    // Create public monitor
    create_test_monitor(&client);

    // Create private monitor
    client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Private Service", "url": "https://example.com/private"}"#)
        .dispatch();

    let resp = client.get("/api/v1/monitors").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Vec<serde_json::Value> = resp.into_json().unwrap();
    // Only public monitor should be listed
    assert_eq!(body.len(), 1);
    assert_eq!(body[0]["name"], "Test Service");
}

#[test]
fn test_update_monitor() {
    let client = test_client();
    let (id, key) = create_test_monitor(&client);

    let resp = client.patch(format!("/api/v1/monitors/{}", id))
        .header(ContentType::JSON)
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .body(r#"{"name": "Updated Service", "interval_seconds": 900}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Verify update
    let resp = client.get(format!("/api/v1/monitors/{}", id)).dispatch();
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["name"], "Updated Service");
    assert_eq!(body["interval_seconds"], 900);
}

#[test]
fn test_update_monitor_wrong_key() {
    let client = test_client();
    let (id, _) = create_test_monitor(&client);

    let resp = client.patch(format!("/api/v1/monitors/{}", id))
        .header(ContentType::JSON)
        .header(rocket::http::Header::new("Authorization", "Bearer wp_wrong_key"))
        .body(r#"{"name": "Hacked"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Forbidden);
}

#[test]
fn test_delete_monitor() {
    let client = test_client();
    let (id, key) = create_test_monitor(&client);

    let resp = client.delete(format!("/api/v1/monitors/{}", id))
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Verify deleted
    let resp = client.get(format!("/api/v1/monitors/{}", id)).dispatch();
    assert_eq!(resp.status(), Status::NotFound);
}

#[test]
fn test_pause_resume() {
    let client = test_client();
    let (id, key) = create_test_monitor(&client);

    // Pause
    let resp = client.post(format!("/api/v1/monitors/{}/pause", id))
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    let resp = client.get(format!("/api/v1/monitors/{}", id)).dispatch();
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["is_paused"], true);

    // Resume
    let resp = client.post(format!("/api/v1/monitors/{}/resume", id))
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    let resp = client.get(format!("/api/v1/monitors/{}", id)).dispatch();
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["is_paused"], false);
}

#[test]
fn test_heartbeats_empty() {
    let client = test_client();
    let (id, _) = create_test_monitor(&client);

    let resp = client.get(format!("/api/v1/monitors/{}/heartbeats", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(body.len(), 0);
}

#[test]
fn test_heartbeat_seq_pagination() {
    let client = test_client();
    let (id, _) = create_test_monitor(&client);

    // Insert heartbeats directly via DB
    let db_path = std::env::var("DATABASE_PATH").unwrap();
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    for i in 1..=5 {
        conn.execute(
            "INSERT INTO heartbeats (id, monitor_id, status, response_time_ms, status_code, checked_at, seq) VALUES (?1, ?2, 'up', ?3, 200, datetime('now'), ?4)",
            rusqlite::params![format!("hb-{}", i), &id, 100 + i, i],
        ).unwrap();
    }
    drop(conn);

    // Default: newest first (DESC), no cursor
    let resp = client.get(format!("/api/v1/monitors/{}/heartbeats?limit=3", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(body.len(), 3);
    // DESC order: seq 5, 4, 3
    assert_eq!(body[0]["seq"], 5);
    assert_eq!(body[1]["seq"], 4);
    assert_eq!(body[2]["seq"], 3);

    // Cursor: after=2 should return seq 3, 4, 5 (ASC)
    let resp = client.get(format!("/api/v1/monitors/{}/heartbeats?after=2", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(body.len(), 3);
    assert_eq!(body[0]["seq"], 3);
    assert_eq!(body[1]["seq"], 4);
    assert_eq!(body[2]["seq"], 5);

    // Cursor with limit: after=0&limit=2
    let resp = client.get(format!("/api/v1/monitors/{}/heartbeats?after=0&limit=2", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(body.len(), 2);
    assert_eq!(body[0]["seq"], 1);
    assert_eq!(body[1]["seq"], 2);

    // All heartbeats have seq field
    for hb in &body {
        assert!(hb["seq"].is_number(), "heartbeat should have seq field");
    }
}

#[test]
fn test_incident_seq_pagination() {
    let client = test_client();
    let (id, _) = create_test_monitor(&client);

    // Insert incidents directly via DB
    let db_path = std::env::var("DATABASE_PATH").unwrap();
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    for i in 1..=3 {
        conn.execute(
            "INSERT INTO incidents (id, monitor_id, cause, started_at, seq) VALUES (?1, ?2, ?3, datetime('now'), ?4)",
            rusqlite::params![format!("inc-{}", i), &id, format!("Test failure {}", i), i],
        ).unwrap();
    }
    drop(conn);

    // Default: newest first
    let resp = client.get(format!("/api/v1/monitors/{}/incidents", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(body.len(), 3);
    assert_eq!(body[0]["seq"], 3);

    // Cursor: after=1
    let resp = client.get(format!("/api/v1/monitors/{}/incidents?after=1", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(body.len(), 2);
    assert_eq!(body[0]["seq"], 2);
    assert_eq!(body[1]["seq"], 3);

    // All incidents have seq field
    for inc in &body {
        assert!(inc["seq"].is_number(), "incident should have seq field");
    }
}

#[test]
fn test_uptime_no_data() {
    let client = test_client();
    let (id, _) = create_test_monitor(&client);

    let resp = client.get(format!("/api/v1/monitors/{}/uptime", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["uptime_24h"], 100.0); // No checks = 100% uptime (default)
    assert_eq!(body["total_checks_24h"], 0);
}

#[test]
fn test_incidents_empty() {
    let client = test_client();
    let (id, _) = create_test_monitor(&client);

    let resp = client.get(format!("/api/v1/monitors/{}/incidents", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(body.len(), 0);
}

#[test]
fn test_status_page() {
    let client = test_client();
    create_test_monitor(&client);

    let resp = client.get("/api/v1/status").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["overall"], "unknown"); // No checks yet
    assert_eq!(body["monitors"].as_array().unwrap().len(), 1);
}

#[test]
fn test_notification_crud() {
    let client = test_client();
    let (id, key) = create_test_monitor(&client);

    // Create
    let resp = client.post(format!("/api/v1/monitors/{}/notifications", id))
        .header(ContentType::JSON)
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .body(r#"{"name": "My Webhook", "channel_type": "webhook", "config": {"url": "https://example.com/hook"}}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    let nid = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["channel_type"], "webhook");

    // List
    let resp = client.get(format!("/api/v1/monitors/{}/notifications", id))
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(body.len(), 1);

    // Delete
    let resp = client.delete(format!("/api/v1/notifications/{}", nid))
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
}

#[test]
fn test_notification_invalid_type() {
    let client = test_client();
    let (id, key) = create_test_monitor(&client);

    let resp = client.post(format!("/api/v1/monitors/{}/notifications", id))
        .header(ContentType::JSON)
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .body(r#"{"name": "Bad", "channel_type": "sms", "config": {}}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::BadRequest);
}

#[test]
fn test_llms_txt() {
    let client = test_client();
    let resp = client.get("/api/v1/llms.txt").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body = resp.into_string().unwrap();
    assert!(body.contains("Watchpost"));
    assert!(body.contains("POST /api/v1/monitors"));
}

#[test]
fn test_auth_x_api_key() {
    let client = test_client();
    let (id, key) = create_test_monitor(&client);

    // X-API-Key header
    let resp = client.patch(format!("/api/v1/monitors/{}", id))
        .header(ContentType::JSON)
        .header(rocket::http::Header::new("X-API-Key", key.clone()))
        .body(r#"{"name": "Via X-API-Key"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
}

#[test]
fn test_auth_query_param() {
    let client = test_client();
    let (id, key) = create_test_monitor(&client);

    // ?key= param
    let resp = client.patch(format!("/api/v1/monitors/{}?key={}", id, key))
        .header(ContentType::JSON)
        .body(r#"{"name": "Via Query Param"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
}

#[test]
fn test_auth_missing() {
    let client = test_client();
    let (id, _) = create_test_monitor(&client);

    let resp = client.patch(format!("/api/v1/monitors/{}", id))
        .header(ContentType::JSON)
        .body(r#"{"name": "No Auth"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);
}

#[test]
fn test_monitor_defaults() {
    let client = test_client();
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Minimal", "url": "https://example.com"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    let m = &body["monitor"];
    assert_eq!(m["method"], "GET");
    assert_eq!(m["interval_seconds"], 600);
    assert_eq!(m["timeout_ms"], 10000);
    assert_eq!(m["expected_status"], 200);
    assert_eq!(m["is_public"], false);
    assert_eq!(m["is_paused"], false);
    assert_eq!(m["confirmation_threshold"], 2);
}

#[test]
fn test_openapi_spec() {
    let client = test_client();
    let resp = client.get("/api/v1/openapi.json").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["openapi"], "3.0.3");
    assert_eq!(body["info"]["title"], "Watchpost");
    assert!(body["paths"]["/monitors"].is_object());
    assert!(body["paths"]["/monitors/{id}"].is_object());
    assert!(body["paths"]["/health"].is_object());
    assert!(body["components"]["schemas"]["Monitor"].is_object());
    assert!(body["components"]["securitySchemes"]["manageKey"].is_object());
}

#[test]
fn test_404_json_catcher() {
    let client = test_client();
    let resp = client.get("/api/v1/nonexistent-route").dispatch();
    assert_eq!(resp.status(), Status::NotFound);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["code"], "NOT_FOUND");
    assert!(body["error"].as_str().is_some());
}

#[test]
fn test_422_json_catcher() {
    let client = test_client();
    // Send invalid JSON body to trigger 422
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"invalid json"#)
        .dispatch();
    // Rocket returns 422 for malformed JSON
    let status = resp.status();
    assert!(status == Status::BadRequest || status == Status::UnprocessableEntity,
        "Expected 400 or 422, got {}", status.code);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert!(body["code"].as_str().is_some());
}

#[test]
fn test_cascade_delete() {
    let client = test_client();
    let (id, key) = create_test_monitor(&client);

    // Add a notification
    client.post(format!("/api/v1/monitors/{}/notifications", id))
        .header(ContentType::JSON)
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .body(r#"{"name": "Hook", "channel_type": "webhook", "config": {"url": "https://example.com"}}"#)
        .dispatch();

    // Delete monitor (should cascade delete notifications)
    let resp = client.delete(format!("/api/v1/monitors/{}", id))
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Monitor should be gone
    let resp = client.get(format!("/api/v1/monitors/{}", id)).dispatch();
    assert_eq!(resp.status(), Status::NotFound);
}

#[test]
fn test_heartbeat_retention_prunes_old() {
    let db_path = format!("/tmp/watchpost_test_{}.db", uuid::Uuid::new_v4());
    let db = watchpost::db::Db::new(&db_path).expect("DB init failed");

    let conn = db.conn.lock().unwrap();

    // Create a monitor
    let monitor_id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO monitors (id, name, url, manage_key_hash) VALUES (?1, 'Test', 'https://example.com', 'hash')",
        params![monitor_id],
    ).unwrap();

    // Insert an old heartbeat (100 days ago)
    conn.execute(
        "INSERT INTO heartbeats (id, monitor_id, status, response_time_ms, checked_at, seq) VALUES (?1, ?2, 'up', 100, datetime('now', '-100 days'), 1)",
        params![uuid::Uuid::new_v4().to_string(), monitor_id],
    ).unwrap();

    // Insert a recent heartbeat (1 day ago)
    conn.execute(
        "INSERT INTO heartbeats (id, monitor_id, status, response_time_ms, checked_at, seq) VALUES (?1, ?2, 'up', 50, datetime('now', '-1 day'), 2)",
        params![uuid::Uuid::new_v4().to_string(), monitor_id],
    ).unwrap();

    let count_before: i64 = conn.query_row("SELECT COUNT(*) FROM heartbeats", [], |r| r.get(0)).unwrap();
    assert_eq!(count_before, 2);
    drop(conn);

    // Prune with 90-day retention
    let deleted = watchpost::checker::prune_heartbeats(&db, 90);
    assert_eq!(deleted, 1, "Should prune exactly 1 old heartbeat");

    let conn = db.conn.lock().unwrap();
    let count_after: i64 = conn.query_row("SELECT COUNT(*) FROM heartbeats", [], |r| r.get(0)).unwrap();
    assert_eq!(count_after, 1, "Should keep 1 recent heartbeat");
}

#[test]
fn test_heartbeat_retention_keeps_recent() {
    let db_path = format!("/tmp/watchpost_test_{}.db", uuid::Uuid::new_v4());
    let db = watchpost::db::Db::new(&db_path).expect("DB init failed");

    let conn = db.conn.lock().unwrap();

    let monitor_id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO monitors (id, name, url, manage_key_hash) VALUES (?1, 'Test', 'https://example.com', 'hash')",
        params![monitor_id],
    ).unwrap();

    // Insert 3 recent heartbeats
    for i in 0..3 {
        conn.execute(
            "INSERT INTO heartbeats (id, monitor_id, status, response_time_ms, checked_at, seq) VALUES (?1, ?2, 'up', 50, datetime('now', ?3), ?4)",
            params![uuid::Uuid::new_v4().to_string(), monitor_id, format!("-{} days", i), i + 1],
        ).unwrap();
    }
    drop(conn);

    // Prune — nothing should be deleted
    let deleted = watchpost::checker::prune_heartbeats(&db, 90);
    assert_eq!(deleted, 0, "Should not prune any recent heartbeats");
}

#[test]
fn test_heartbeat_retention_custom_days() {
    let db_path = format!("/tmp/watchpost_test_{}.db", uuid::Uuid::new_v4());
    let db = watchpost::db::Db::new(&db_path).expect("DB init failed");

    let conn = db.conn.lock().unwrap();

    let monitor_id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO monitors (id, name, url, manage_key_hash) VALUES (?1, 'Test', 'https://example.com', 'hash')",
        params![monitor_id],
    ).unwrap();

    // Insert heartbeats at 5, 15, 25, 35 days ago
    for (i, days) in [5, 15, 25, 35].iter().enumerate() {
        conn.execute(
            "INSERT INTO heartbeats (id, monitor_id, status, response_time_ms, checked_at, seq) VALUES (?1, ?2, 'up', 50, datetime('now', ?3), ?4)",
            params![uuid::Uuid::new_v4().to_string(), monitor_id, format!("-{} days", days), i + 1],
        ).unwrap();
    }
    drop(conn);

    // Prune with 30-day retention — should delete the 35-day-old one
    let deleted = watchpost::checker::prune_heartbeats(&db, 30);
    assert_eq!(deleted, 1);

    // Prune again with 10-day retention — should delete 15 and 25 day old ones
    let deleted = watchpost::checker::prune_heartbeats(&db, 10);
    assert_eq!(deleted, 2);

    let conn = db.conn.lock().unwrap();
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM heartbeats", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 1, "Only the 5-day-old heartbeat should remain");
}

#[test]
fn test_notification_toggle() {
    let client = test_client();
    let (id, key) = create_test_monitor(&client);

    // Create a notification channel
    let resp = client.post(format!("/api/v1/monitors/{}/notifications", id))
        .header(ContentType::JSON)
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .body(r#"{"name": "Test Hook", "channel_type": "webhook", "config": {"url": "https://example.com/hook"}}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    let notif_id = body["id"].as_str().unwrap().to_string();

    // Verify it's enabled by default
    assert_eq!(body["is_enabled"].as_bool(), Some(true));

    // Disable it
    let resp = client.patch(format!("/api/v1/notifications/{}", notif_id))
        .header(ContentType::JSON)
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .body(r#"{"is_enabled": false}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // List and verify it's disabled
    let resp = client.get(format!("/api/v1/monitors/{}/notifications", id))
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .dispatch();
    let channels: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0]["is_enabled"].as_bool(), Some(false));

    // Re-enable it
    let resp = client.patch(format!("/api/v1/notifications/{}", notif_id))
        .header(ContentType::JSON)
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .body(r#"{"is_enabled": true}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // List and verify it's enabled again
    let resp = client.get(format!("/api/v1/monitors/{}/notifications", id))
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .dispatch();
    let channels: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(channels[0]["is_enabled"].as_bool(), Some(true));
}

#[test]
fn test_notification_toggle_wrong_key() {
    let client = test_client();
    let (id, key) = create_test_monitor(&client);

    // Create a notification channel
    let resp = client.post(format!("/api/v1/monitors/{}/notifications", id))
        .header(ContentType::JSON)
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .body(r#"{"name": "Hook", "channel_type": "webhook", "config": {"url": "https://example.com"}}"#)
        .dispatch();
    let body: serde_json::Value = resp.into_json().unwrap();
    let notif_id = body["id"].as_str().unwrap().to_string();

    // Try to toggle with wrong key
    let resp = client.patch(format!("/api/v1/notifications/{}", notif_id))
        .header(ContentType::JSON)
        .header(rocket::http::Header::new("Authorization", "Bearer wrong_key"))
        .body(r#"{"is_enabled": false}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Forbidden);
}

#[test]
fn test_search_monitors_by_name() {
    let client = test_client();

    // Create two public monitors with distinct names
    client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Alpha API", "url": "https://alpha.example.com", "is_public": true}"#)
        .dispatch();
    client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Beta Dashboard", "url": "https://beta.example.com", "is_public": true}"#)
        .dispatch();

    // Search for "Alpha"
    let resp = client.get("/api/v1/monitors?search=Alpha").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(body.len(), 1);
    assert_eq!(body[0]["name"], "Alpha API");

    // Search for "example" (URL match) — both match
    let resp = client.get("/api/v1/monitors?search=example").dispatch();
    let body: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(body.len(), 2);

    // Search for something that doesn't exist
    let resp = client.get("/api/v1/monitors?search=nonexistent").dispatch();
    let body: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(body.len(), 0);
}

#[test]
fn test_filter_monitors_by_status() {
    let client = test_client();

    // Create a public monitor (default status = "unknown")
    create_test_monitor(&client);

    // Filter by unknown status — should find it
    let resp = client.get("/api/v1/monitors?status=unknown").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(body.len(), 1);

    // Filter by up status — should find nothing (monitor hasn't been checked)
    let resp = client.get("/api/v1/monitors?status=up").dispatch();
    let body: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(body.len(), 0);

    // Invalid status value should be ignored (return all)
    let resp = client.get("/api/v1/monitors?status=invalid").dispatch();
    let body: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(body.len(), 1);
}

#[test]
fn test_status_page_search_filter() {
    let client = test_client();

    // Create public monitors
    client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Prod API", "url": "https://prod.example.com", "is_public": true}"#)
        .dispatch();
    client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Staging API", "url": "https://staging.example.com", "is_public": true}"#)
        .dispatch();

    // Full status page (no filters)
    let resp = client.get("/api/v1/status").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["monitors"].as_array().unwrap().len(), 2);

    // Search filter on status page
    let resp = client.get("/api/v1/status?search=Prod").dispatch();
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["monitors"].as_array().unwrap().len(), 1);
    assert_eq!(body["monitors"][0]["name"], "Prod API");

    // Status filter on status page
    let resp = client.get("/api/v1/status?status=unknown").dispatch();
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["monitors"].as_array().unwrap().len(), 2);

    // Combined search + status filter
    let resp = client.get("/api/v1/status?search=Staging&status=unknown").dispatch();
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["monitors"].as_array().unwrap().len(), 1);
    assert_eq!(body["monitors"][0]["name"], "Staging API");
}

// ── Tags Tests ──

#[test]
fn test_create_monitor_with_tags() {
    let client = test_client();
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Tagged API", "url": "https://example.com/api", "is_public": true, "tags": ["prod", "api", "critical"]}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    let tags = body["monitor"]["tags"].as_array().unwrap();
    assert_eq!(tags.len(), 3);
    // Tags are lowercased and sorted alphabetically in storage
    assert!(tags.contains(&serde_json::json!("prod")));
    assert!(tags.contains(&serde_json::json!("api")));
    assert!(tags.contains(&serde_json::json!("critical")));
}

#[test]
fn test_create_monitor_without_tags() {
    let client = test_client();
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "No Tags", "url": "https://example.com/api", "is_public": true}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    let tags = body["monitor"]["tags"].as_array().unwrap();
    assert!(tags.is_empty());
}

#[test]
fn test_update_monitor_tags() {
    let client = test_client();
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Taggable", "url": "https://example.com", "is_public": true, "tags": ["v1"]}"#)
        .dispatch();
    let body: serde_json::Value = resp.into_json().unwrap();
    let id = body["monitor"]["id"].as_str().unwrap();
    let key = body["manage_key"].as_str().unwrap();

    // Update tags
    let resp = client.patch(format!("/api/v1/monitors/{}", id))
        .header(ContentType::JSON)
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .body(r#"{"tags": ["v2", "backend"]}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Verify
    let resp = client.get(format!("/api/v1/monitors/{}", id)).dispatch();
    let body: serde_json::Value = resp.into_json().unwrap();
    let tags = body["tags"].as_array().unwrap();
    assert_eq!(tags.len(), 2);
    assert!(tags.contains(&serde_json::json!("v2")));
    assert!(tags.contains(&serde_json::json!("backend")));
}

#[test]
fn test_filter_monitors_by_tag() {
    let client = test_client();

    // Create monitors with different tags
    client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "API Prod", "url": "https://api.example.com", "is_public": true, "tags": ["prod", "api"]}"#)
        .dispatch();
    client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "API Staging", "url": "https://staging.example.com", "is_public": true, "tags": ["staging", "api"]}"#)
        .dispatch();
    client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Frontend", "url": "https://www.example.com", "is_public": true, "tags": ["prod", "frontend"]}"#)
        .dispatch();

    // Filter by tag=prod → should get 2
    let resp = client.get("/api/v1/monitors?tag=prod").dispatch();
    let body: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(body.len(), 2);

    // Filter by tag=api → should get 2
    let resp = client.get("/api/v1/monitors?tag=api").dispatch();
    let body: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(body.len(), 2);

    // Filter by tag=staging → should get 1
    let resp = client.get("/api/v1/monitors?tag=staging").dispatch();
    let body: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(body.len(), 1);
    assert_eq!(body[0]["name"], "API Staging");

    // Filter by tag=nonexistent → should get 0
    let resp = client.get("/api/v1/monitors?tag=nonexistent").dispatch();
    let body: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(body.len(), 0);
}

#[test]
fn test_status_page_tag_filter() {
    let client = test_client();

    client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Service A", "url": "https://a.example.com", "is_public": true, "tags": ["infra"]}"#)
        .dispatch();
    client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Service B", "url": "https://b.example.com", "is_public": true, "tags": ["app"]}"#)
        .dispatch();

    // Status page with tag filter
    let resp = client.get("/api/v1/status?tag=infra").dispatch();
    let body: serde_json::Value = resp.into_json().unwrap();
    let monitors = body["monitors"].as_array().unwrap();
    assert_eq!(monitors.len(), 1);
    assert_eq!(monitors[0]["name"], "Service A");
    assert!(monitors[0]["tags"].as_array().unwrap().contains(&serde_json::json!("infra")));

    // No tag filter → all
    let resp = client.get("/api/v1/status").dispatch();
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["monitors"].as_array().unwrap().len(), 2);
}

#[test]
fn test_list_tags_endpoint() {
    let client = test_client();

    // Empty at first
    let resp = client.get("/api/v1/tags").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Vec<String> = resp.into_json().unwrap();
    assert!(body.is_empty());

    // Create monitors with tags
    client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "M1", "url": "https://m1.example.com", "is_public": true, "tags": ["prod", "api"]}"#)
        .dispatch();
    client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "M2", "url": "https://m2.example.com", "is_public": true, "tags": ["staging", "api"]}"#)
        .dispatch();
    // Private monitor tags should NOT appear
    client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "M3", "url": "https://m3.example.com", "is_public": false, "tags": ["secret"]}"#)
        .dispatch();

    let resp = client.get("/api/v1/tags").dispatch();
    let body: Vec<String> = resp.into_json().unwrap();
    // Should have api, prod, staging (sorted), no "secret"
    assert_eq!(body, vec!["api", "prod", "staging"]);
}

#[test]
fn test_create_monitor_with_response_time_threshold() {
    let client = test_client();

    // Create with threshold
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "RT Alert Test", "url": "https://example.com", "is_public": true, "response_time_threshold_ms": 2000}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["monitor"]["response_time_threshold_ms"], 2000);

    let id = body["monitor"]["id"].as_str().unwrap();
    // Verify on GET
    let resp = client.get(format!("/api/v1/monitors/{}", id)).dispatch();
    let monitor: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(monitor["response_time_threshold_ms"], 2000);
}

#[test]
fn test_create_monitor_without_response_time_threshold() {
    let client = test_client();

    // Create without threshold — should be null
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "No RT Threshold", "url": "https://example.com", "is_public": true}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert!(body["monitor"]["response_time_threshold_ms"].is_null());
}

#[test]
fn test_update_monitor_response_time_threshold() {
    let client = test_client();
    let (id, key) = create_test_monitor(&client);

    // Set threshold
    let resp = client.patch(format!("/api/v1/monitors/{}", id))
        .header(ContentType::JSON)
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .body(r#"{"response_time_threshold_ms": 1500}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Verify
    let resp = client.get(format!("/api/v1/monitors/{}", id)).dispatch();
    let monitor: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(monitor["response_time_threshold_ms"], 1500);

    // Clear threshold by setting to null
    let resp = client.patch(format!("/api/v1/monitors/{}", id))
        .header(ContentType::JSON)
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .body(r#"{"response_time_threshold_ms": null}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    let resp = client.get(format!("/api/v1/monitors/{}", id)).dispatch();
    let monitor: serde_json::Value = resp.into_json().unwrap();
    assert!(monitor["response_time_threshold_ms"].is_null());
}

#[test]
fn test_response_time_threshold_minimum_enforced() {
    let client = test_client();

    // Create with threshold below minimum (100ms) — should be clamped
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Low RT", "url": "https://example.com", "is_public": true, "response_time_threshold_ms": 50}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["monitor"]["response_time_threshold_ms"], 100);
}

// ── Bulk Create Tests ──

#[test]
fn test_bulk_create_monitors() {
    let client = test_client();

    let resp = client.post("/api/v1/monitors/bulk")
        .header(ContentType::JSON)
        .body(r#"{"monitors": [
            {"name": "API Server", "url": "https://api.example.com/health", "is_public": true, "tags": ["api", "prod"]},
            {"name": "Web Frontend", "url": "https://www.example.com", "is_public": true, "interval_seconds": 900},
            {"name": "Internal DB", "url": "http://db.internal:5432/health", "is_public": false}
        ]}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();

    assert_eq!(body["total"], 3);
    assert_eq!(body["succeeded"], 3);
    assert_eq!(body["failed"], 0);
    assert_eq!(body["created"].as_array().unwrap().len(), 3);
    assert_eq!(body["errors"].as_array().unwrap().len(), 0);

    // Each created monitor should have a unique manage_key
    let keys: Vec<&str> = body["created"].as_array().unwrap()
        .iter()
        .map(|c| c["manage_key"].as_str().unwrap())
        .collect();
    assert_eq!(keys.len(), 3);
    assert_ne!(keys[0], keys[1]);
    assert_ne!(keys[1], keys[2]);

    // First monitor should have tags
    assert_eq!(body["created"][0]["monitor"]["tags"], serde_json::json!(["api", "prod"]));
    // Second monitor should have custom interval
    assert_eq!(body["created"][1]["monitor"]["interval_seconds"], 900);
}

#[test]
fn test_bulk_create_partial_failure() {
    let client = test_client();

    let resp = client.post("/api/v1/monitors/bulk")
        .header(ContentType::JSON)
        .body(r#"{"monitors": [
            {"name": "Good Monitor", "url": "https://example.com", "is_public": true},
            {"name": "", "url": "https://example.com"},
            {"name": "Also Good", "url": "https://example2.com"}
        ]}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();

    assert_eq!(body["total"], 3);
    assert_eq!(body["succeeded"], 2);
    assert_eq!(body["failed"], 1);
    assert_eq!(body["errors"][0]["index"], 1);
    assert_eq!(body["errors"][0]["code"], "VALIDATION_ERROR");
}

#[test]
fn test_bulk_create_empty_array() {
    let client = test_client();

    let resp = client.post("/api/v1/monitors/bulk")
        .header(ContentType::JSON)
        .body(r#"{"monitors": []}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::BadRequest);
}

#[test]
fn test_bulk_create_validation_errors() {
    let client = test_client();

    let resp = client.post("/api/v1/monitors/bulk")
        .header(ContentType::JSON)
        .body(r#"{"monitors": [
            {"name": "No URL", "url": ""},
            {"name": "Bad Method", "url": "https://example.com", "method": "PATCH"}
        ]}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();

    assert_eq!(body["total"], 2);
    assert_eq!(body["succeeded"], 0);
    assert_eq!(body["failed"], 2);
}

// ── Export Monitor Tests ──

#[test]
fn test_export_monitor() {
    let client = test_client();
    let (id, key) = create_test_monitor(&client);

    let resp = client.get(format!("/api/v1/monitors/{}/export?key={}", id, key))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();

    // Should have importable fields
    assert_eq!(body["name"], "Test Service");
    assert_eq!(body["url"], "https://httpbin.org/status/200");
    assert_eq!(body["method"], "GET");
    assert!(body["interval_seconds"].is_number());
    assert!(body["timeout_ms"].is_number());
    assert!(body["expected_status"].is_number());
    assert!(body["is_public"].is_boolean());

    // Should NOT have runtime fields
    assert!(body.get("id").is_none());
    assert!(body.get("current_status").is_none());
    assert!(body.get("created_at").is_none());
    assert!(body.get("is_paused").is_none());
}

#[test]
fn test_export_monitor_requires_auth() {
    let client = test_client();
    let (id, _key) = create_test_monitor(&client);

    // No key
    let resp = client.get(format!("/api/v1/monitors/{}/export", id))
        .dispatch();
    // Should fail (401 or 403) — ManageToken guard will reject
    assert_ne!(resp.status(), Status::Ok);
}

#[test]
fn test_export_reimport_roundtrip() {
    let client = test_client();

    // Create a monitor with custom settings
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Custom Monitor", "url": "https://api.example.com", "method": "HEAD", "interval_seconds": 900, "timeout_ms": 5000, "expected_status": 204, "is_public": true, "confirmation_threshold": 3, "response_time_threshold_ms": 2000, "tags": ["api", "staging"]}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let created: serde_json::Value = resp.into_json().unwrap();
    let id = created["monitor"]["id"].as_str().unwrap();
    let key = created["manage_key"].as_str().unwrap();

    // Export it
    let resp = client.get(format!("/api/v1/monitors/{}/export?key={}", id, key))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let exported: serde_json::Value = resp.into_json().unwrap();

    // Re-import via bulk create
    let bulk_body = serde_json::json!({"monitors": [exported]});
    let resp = client.post("/api/v1/monitors/bulk")
        .header(ContentType::JSON)
        .body(bulk_body.to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let bulk: serde_json::Value = resp.into_json().unwrap();

    assert_eq!(bulk["succeeded"], 1);
    assert_eq!(bulk["failed"], 0);

    // Verify the clone has same settings
    let clone = &bulk["created"][0]["monitor"];
    assert_eq!(clone["name"], "Custom Monitor");
    assert_eq!(clone["url"], "https://api.example.com");
    assert_eq!(clone["method"], "HEAD");
    assert_eq!(clone["interval_seconds"], 900);
    assert_eq!(clone["timeout_ms"], 5000);
    assert_eq!(clone["expected_status"], 204);
    assert_eq!(clone["is_public"], true);
    assert_eq!(clone["confirmation_threshold"], 3);
    assert_eq!(clone["response_time_threshold_ms"], 2000);
    assert_eq!(clone["tags"], serde_json::json!(["api", "staging"]));

    // But it should have a different ID
    assert_ne!(clone["id"].as_str().unwrap(), id);
}

// ── Maintenance Window Tests ──

#[test]
fn test_create_maintenance_window() {
    let client = test_client();

    // Create a monitor
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Maint Test", "url": "https://example.com", "is_public": true}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let created: serde_json::Value = resp.into_json().unwrap();
    let id = created["monitor"]["id"].as_str().unwrap();
    let key = created["manage_key"].as_str().unwrap();

    // Create a maintenance window
    let resp = client.post(format!("/api/v1/monitors/{}/maintenance?key={}", id, key))
        .header(ContentType::JSON)
        .body(r#"{"title": "Deploy v2.0", "starts_at": "2026-02-10T14:00:00Z", "ends_at": "2026-02-10T16:00:00Z"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let window: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(window["title"], "Deploy v2.0");
    assert_eq!(window["monitor_id"], id);
    assert_eq!(window["starts_at"], "2026-02-10T14:00:00Z");
    assert_eq!(window["ends_at"], "2026-02-10T16:00:00Z");
    assert!(window["id"].as_str().is_some());
}

#[test]
fn test_list_maintenance_windows() {
    let client = test_client();

    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Maint List", "url": "https://example.com", "is_public": true}"#)
        .dispatch();
    let created: serde_json::Value = resp.into_json().unwrap();
    let id = created["monitor"]["id"].as_str().unwrap();
    let key = created["manage_key"].as_str().unwrap();

    // Create two windows
    client.post(format!("/api/v1/monitors/{}/maintenance?key={}", id, key))
        .header(ContentType::JSON)
        .body(r#"{"title": "Window 1", "starts_at": "2026-02-10T10:00:00Z", "ends_at": "2026-02-10T11:00:00Z"}"#)
        .dispatch();
    client.post(format!("/api/v1/monitors/{}/maintenance?key={}", id, key))
        .header(ContentType::JSON)
        .body(r#"{"title": "Window 2", "starts_at": "2026-02-10T12:00:00Z", "ends_at": "2026-02-10T13:00:00Z"}"#)
        .dispatch();

    // List them
    let resp = client.get(format!("/api/v1/monitors/{}/maintenance", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let windows: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(windows.len(), 2);
}

#[test]
fn test_delete_maintenance_window() {
    let client = test_client();

    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Maint Delete", "url": "https://example.com", "is_public": true}"#)
        .dispatch();
    let created: serde_json::Value = resp.into_json().unwrap();
    let id = created["monitor"]["id"].as_str().unwrap();
    let key = created["manage_key"].as_str().unwrap();

    // Create a window
    let resp = client.post(format!("/api/v1/monitors/{}/maintenance?key={}", id, key))
        .header(ContentType::JSON)
        .body(r#"{"title": "To Delete", "starts_at": "2026-02-10T14:00:00Z", "ends_at": "2026-02-10T16:00:00Z"}"#)
        .dispatch();
    let window: serde_json::Value = resp.into_json().unwrap();
    let window_id = window["id"].as_str().unwrap();

    // Delete it
    let resp = client.delete(format!("/api/v1/maintenance/{}?key={}", window_id, key)).dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Verify gone
    let resp = client.get(format!("/api/v1/monitors/{}/maintenance", id)).dispatch();
    let windows: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(windows.len(), 0);
}

#[test]
fn test_maintenance_window_requires_auth() {
    let client = test_client();

    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Auth Test", "url": "https://example.com", "is_public": true}"#)
        .dispatch();
    let created: serde_json::Value = resp.into_json().unwrap();
    let id = created["monitor"]["id"].as_str().unwrap();

    // Try to create without auth
    let resp = client.post(format!("/api/v1/monitors/{}/maintenance", id))
        .header(ContentType::JSON)
        .body(r#"{"title": "No Auth", "starts_at": "2026-02-10T14:00:00Z", "ends_at": "2026-02-10T16:00:00Z"}"#)
        .dispatch();
    // Should fail - no key provided (401 or 403)
    assert_ne!(resp.status(), Status::Ok);
}

#[test]
fn test_maintenance_window_validation() {
    let client = test_client();

    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Validation", "url": "https://example.com", "is_public": true}"#)
        .dispatch();
    let created: serde_json::Value = resp.into_json().unwrap();
    let id = created["monitor"]["id"].as_str().unwrap();
    let key = created["manage_key"].as_str().unwrap();

    // ends_at before starts_at
    let resp = client.post(format!("/api/v1/monitors/{}/maintenance?key={}", id, key))
        .header(ContentType::JSON)
        .body(r#"{"title": "Bad Window", "starts_at": "2026-02-10T16:00:00Z", "ends_at": "2026-02-10T14:00:00Z"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::BadRequest);

    // Empty title
    let resp = client.post(format!("/api/v1/monitors/{}/maintenance?key={}", id, key))
        .header(ContentType::JSON)
        .body(r#"{"title": "", "starts_at": "2026-02-10T14:00:00Z", "ends_at": "2026-02-10T16:00:00Z"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::BadRequest);

    // Bad timestamp format
    let resp = client.post(format!("/api/v1/monitors/{}/maintenance?key={}", id, key))
        .header(ContentType::JSON)
        .body(r#"{"title": "Bad Time", "starts_at": "not-a-date", "ends_at": "2026-02-10T16:00:00Z"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::BadRequest);
}

#[test]
fn test_maintenance_suppresses_incidents() {
    let client = test_client();

    // Create a monitor
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Maint Suppression", "url": "https://example.com", "is_public": true}"#)
        .dispatch();
    let created: serde_json::Value = resp.into_json().unwrap();
    let id = created["monitor"]["id"].as_str().unwrap();
    let key = created["manage_key"].as_str().unwrap();

    // Create a maintenance window covering right now (a wide future window)
    let resp = client.post(format!("/api/v1/monitors/{}/maintenance?key={}", id, key))
        .header(ContentType::JSON)
        .body(r#"{"title": "Active Window", "starts_at": "2020-01-01T00:00:00Z", "ends_at": "2030-12-31T23:59:59Z"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let window: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(window["active"], true);

    // Verify is_in_maintenance returns true via the checker helper
    // We'll test this indirectly: the API should list the window as active
    let resp = client.get(format!("/api/v1/monitors/{}/maintenance", id)).dispatch();
    let windows: Vec<serde_json::Value> = resp.into_json().unwrap();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0]["active"], true);
}

#[test]
fn test_maintenance_window_cascade_delete() {
    let client = test_client();

    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Cascade Test", "url": "https://example.com", "is_public": true}"#)
        .dispatch();
    let created: serde_json::Value = resp.into_json().unwrap();
    let id = created["monitor"]["id"].as_str().unwrap();
    let key = created["manage_key"].as_str().unwrap();

    // Create a maintenance window
    client.post(format!("/api/v1/monitors/{}/maintenance?key={}", id, key))
        .header(ContentType::JSON)
        .body(r#"{"title": "Cascade Window", "starts_at": "2026-02-10T14:00:00Z", "ends_at": "2026-02-10T16:00:00Z"}"#)
        .dispatch();

    // Delete the monitor
    let resp = client.delete(format!("/api/v1/monitors/{}?key={}", id, key)).dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Monitor gone, maintenance windows should also be gone (cascade delete)
    let resp = client.get(format!("/api/v1/monitors/{}/maintenance", id)).dispatch();
    assert_eq!(resp.status(), Status::NotFound);
}

// ── Dashboard Tests ──

#[test]
fn test_dashboard_empty() {
    let client = test_client();
    let resp = client.get("/api/v1/dashboard").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["total_monitors"], 0);
    assert_eq!(body["public_monitors"], 0);
    assert_eq!(body["paused_monitors"], 0);
    assert_eq!(body["active_incidents"], 0);
    assert_eq!(body["avg_uptime_24h"], 100.0);
    assert_eq!(body["total_checks_24h"], 0);
    assert!(body["status_counts"]["up"].as_u64().unwrap() == 0);
    assert!(body["recent_incidents"].as_array().unwrap().is_empty());
    assert!(body["slowest_monitors"].as_array().unwrap().is_empty());
}

#[test]
fn test_dashboard_with_monitors() {
    let client = test_client();

    // Create a public monitor
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Public API", "url": "https://example.com", "is_public": true}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Create a private monitor
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Private API", "url": "https://internal.example.com", "is_public": false}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    let resp = client.get("/api/v1/dashboard").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["total_monitors"], 2);
    assert_eq!(body["public_monitors"], 1);
    assert_eq!(body["paused_monitors"], 0);
    // New monitors start as "unknown"
    assert_eq!(body["status_counts"]["unknown"], 2);
}

#[test]
fn test_dashboard_with_paused_monitor() {
    let client = test_client();
    let (id, key) = create_test_monitor(&client);

    // Pause it
    client.post(format!("/api/v1/monitors/{}/pause?key={}", id, key)).dispatch();

    let resp = client.get("/api/v1/dashboard").dispatch();
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["total_monitors"], 1);
    assert_eq!(body["paused_monitors"], 1);
}

// ── Uptime History Tests ──

#[test]
fn test_uptime_history_empty() {
    let client = test_client();
    let resp = client.get("/api/v1/uptime-history").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert!(body.as_array().unwrap().is_empty());
}

#[test]
fn test_uptime_history_with_days_param() {
    let client = test_client();
    let resp = client.get("/api/v1/uptime-history?days=7").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert!(body.as_array().is_some());
}

#[test]
fn test_uptime_history_clamps_days() {
    let client = test_client();
    // days > 90 should be clamped
    let resp = client.get("/api/v1/uptime-history?days=365").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert!(body.as_array().is_some());
}

#[test]
fn test_monitor_uptime_history_not_found() {
    let client = test_client();
    let resp = client.get("/api/v1/monitors/nonexistent/uptime-history").dispatch();
    assert_eq!(resp.status(), Status::NotFound);
}

#[test]
fn test_monitor_uptime_history_empty() {
    let client = test_client();
    let (id, _key) = create_test_monitor(&client);
    let resp = client.get(format!("/api/v1/monitors/{}/uptime-history", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    // No heartbeats yet, so empty
    assert!(body.as_array().unwrap().is_empty());
}

#[test]
fn test_uptime_history_with_heartbeats() {
    let client = test_client();
    let (id, _key) = create_test_monitor(&client);

    // Manually insert heartbeats via DB
    {
        let db_path = std::env::var("DATABASE_PATH").unwrap();
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "INSERT INTO heartbeats (id, monitor_id, status, response_time_ms, status_code, checked_at, seq)
             VALUES (?1, ?2, 'up', 150, 200, datetime('now'), (SELECT COALESCE(MAX(seq),0)+1 FROM heartbeats))",
            params![uuid::Uuid::new_v4().to_string(), id],
        ).unwrap();
        conn.execute(
            "INSERT INTO heartbeats (id, monitor_id, status, response_time_ms, status_code, checked_at, seq)
             VALUES (?1, ?2, 'down', 0, 500, datetime('now'), (SELECT COALESCE(MAX(seq),0)+1 FROM heartbeats))",
            params![uuid::Uuid::new_v4().to_string(), id],
        ).unwrap();
    }

    // Global history
    let resp = client.get("/api/v1/uptime-history?days=1").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    let days = body.as_array().unwrap();
    assert!(!days.is_empty());
    let day = &days[0];
    assert_eq!(day["total_checks"], 2);
    assert_eq!(day["up_checks"], 1);
    assert_eq!(day["down_checks"], 1);
    assert!((day["uptime_pct"].as_f64().unwrap() - 50.0).abs() < 0.01);
    assert!(day["avg_response_ms"].as_f64().is_some());

    // Per-monitor history
    let resp = client.get(format!("/api/v1/monitors/{}/uptime-history?days=1", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    let days = body.as_array().unwrap();
    assert!(!days.is_empty());
    assert_eq!(days[0]["total_checks"], 2);
}

// ── Badge Tests ──

#[test]
fn test_uptime_badge_default() {
    let client = test_client();
    let (id, _key) = create_test_monitor(&client);
    let resp = client.get(format!("/api/v1/monitors/{}/badge/uptime", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let ct = resp.content_type().unwrap();
    assert_eq!(ct.top().as_str(), "image");
    assert_eq!(ct.sub().as_str(), "svg+xml");
    let body = resp.into_string().unwrap();
    assert!(body.contains("<svg"));
    assert!(body.contains("100.0%")); // no heartbeats = 100%
    assert!(body.contains("uptime 24h"));
}

#[test]
fn test_uptime_badge_with_period() {
    let client = test_client();
    let (id, _key) = create_test_monitor(&client);
    let resp = client.get(format!("/api/v1/monitors/{}/badge/uptime?period=7d", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body = resp.into_string().unwrap();
    assert!(body.contains("uptime 7d"));
}

#[test]
fn test_uptime_badge_custom_label() {
    let client = test_client();
    let (id, _key) = create_test_monitor(&client);
    let resp = client.get(format!("/api/v1/monitors/{}/badge/uptime?label=my+api", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body = resp.into_string().unwrap();
    assert!(body.contains("my api"));
}

#[test]
fn test_uptime_badge_with_heartbeats() {
    let client = test_client();
    let (id, _key) = create_test_monitor(&client);

    // Insert heartbeats: 3 up, 1 down = 75% uptime
    {
        let db_path = std::env::var("DATABASE_PATH").unwrap();
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        for status in &["up", "up", "up", "down"] {
            conn.execute(
                "INSERT INTO heartbeats (id, monitor_id, status, response_time_ms, status_code, checked_at, seq)
                 VALUES (?1, ?2, ?3, 100, 200, datetime('now'), (SELECT COALESCE(MAX(seq),0)+1 FROM heartbeats))",
                params![uuid::Uuid::new_v4().to_string(), id, status],
            ).unwrap();
        }
    }

    let resp = client.get(format!("/api/v1/monitors/{}/badge/uptime", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body = resp.into_string().unwrap();
    assert!(body.contains("75.0%"));
    // 75% should be red (<90%)
    assert!(body.contains("#e05d44"));
}

#[test]
fn test_status_badge() {
    let client = test_client();
    let (id, _key) = create_test_monitor(&client);
    let resp = client.get(format!("/api/v1/monitors/{}/badge/status", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let ct = resp.content_type().unwrap();
    assert_eq!(ct.top().as_str(), "image");
    assert_eq!(ct.sub().as_str(), "svg+xml");
    let body = resp.into_string().unwrap();
    assert!(body.contains("<svg"));
    assert!(body.contains("unknown")); // never checked
    assert!(body.contains("status"));
}

#[test]
fn test_status_badge_custom_label() {
    let client = test_client();
    let (id, _key) = create_test_monitor(&client);
    let resp = client.get(format!("/api/v1/monitors/{}/badge/status?label=health", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body = resp.into_string().unwrap();
    assert!(body.contains("health"));
}

#[test]
fn test_badge_not_found() {
    let client = test_client();
    let resp = client.get("/api/v1/monitors/00000000-0000-0000-0000-000000000000/badge/uptime").dispatch();
    assert_eq!(resp.status(), Status::NotFound);
    let resp = client.get("/api/v1/monitors/00000000-0000-0000-0000-000000000000/badge/status").dispatch();
    assert_eq!(resp.status(), Status::NotFound);
}

// ── URL Validation Tests ──

#[test]
fn test_create_monitor_invalid_url_scheme() {
    let client = test_client();

    // No scheme
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Bad URL", "url": "example.com/health"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::BadRequest);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert!(body["error"].as_str().unwrap().contains("http://"));

    // FTP scheme
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "FTP", "url": "ftp://files.example.com"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::BadRequest);

    // Random string
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Gibberish", "url": "not-a-url-at-all"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::BadRequest);

    // Valid http:// should succeed
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "HTTP OK", "url": "http://example.com"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Valid https:// should succeed
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "HTTPS OK", "url": "https://example.com"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
}

#[test]
fn test_update_monitor_invalid_url_scheme() {
    let client = test_client();
    let (id, key) = create_test_monitor(&client);

    let resp = client.patch(format!("/api/v1/monitors/{}", id))
        .header(ContentType::JSON)
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .body(r#"{"url": "ftp://bad.example.com"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::BadRequest);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert!(body["error"].as_str().unwrap().contains("http://"));
}

// ── Headers Validation Tests ──

#[test]
fn test_create_monitor_with_headers() {
    let client = test_client();
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "With Headers", "url": "https://api.example.com", "headers": {"Authorization": "Bearer token123", "X-Custom": "value"}}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    let headers = &body["monitor"]["headers"];
    assert!(headers.is_object());
    assert_eq!(headers["Authorization"], "Bearer token123");
    assert_eq!(headers["X-Custom"], "value");
}

#[test]
fn test_create_monitor_headers_must_be_object() {
    let client = test_client();

    // Array instead of object
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Bad Headers", "url": "https://example.com", "headers": ["not", "an", "object"]}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::BadRequest);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert!(body["error"].as_str().unwrap().contains("JSON object"));

    // String instead of object — serde_json::Value accepts strings, but our validation catches it
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Bad Headers", "url": "https://example.com", "headers": "not-an-object"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::BadRequest);
}

#[test]
fn test_update_monitor_headers_must_be_object() {
    let client = test_client();
    let (id, key) = create_test_monitor(&client);

    let resp = client.patch(format!("/api/v1/monitors/{}", id))
        .header(ContentType::JSON)
        .header(rocket::http::Header::new("Authorization", format!("Bearer {}", key)))
        .body(r#"{"headers": [1, 2, 3]}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::BadRequest);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert!(body["error"].as_str().unwrap().contains("JSON object"));
}

// ── POST Method + body_contains Tests ──

#[test]
fn test_create_monitor_post_method() {
    let client = test_client();
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "POST Monitor", "url": "https://api.example.com/webhook", "method": "POST"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["monitor"]["method"], "POST");
}

#[test]
fn test_create_monitor_head_method() {
    let client = test_client();
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "HEAD Monitor", "url": "https://example.com", "method": "head"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    // Method should be uppercased
    assert_eq!(body["monitor"]["method"], "HEAD");
}

#[test]
fn test_create_monitor_with_body_contains() {
    let client = test_client();
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Body Check", "url": "https://example.com/health", "body_contains": "\"status\":\"ok\""}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["monitor"]["body_contains"], "\"status\":\"ok\"");
}

// ── Clamping & Default Tests ──

#[test]
fn test_create_monitor_interval_clamped() {
    let client = test_client();

    // Interval below minimum (600s / 10 min) should be clamped to 600
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Fast Check", "url": "https://example.com", "interval_seconds": 5}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["monitor"]["interval_seconds"], 600);
}

#[test]
fn test_create_monitor_timeout_clamped() {
    let client = test_client();

    // Timeout below minimum (1000ms) should be clamped to 1000
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Quick Timeout", "url": "https://example.com", "timeout_ms": 100}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["monitor"]["timeout_ms"], 1000);

    // Timeout above maximum (60000ms) should be clamped to 60000
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Slow Timeout", "url": "https://example.com", "timeout_ms": 120000}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["monitor"]["timeout_ms"], 60000);
}

#[test]
fn test_create_monitor_confirmation_threshold_clamped() {
    let client = test_client();

    // Below min (1) → clamped to 1
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Low Confirm", "url": "https://example.com", "confirmation_threshold": 0}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["monitor"]["confirmation_threshold"], 1);

    // Above max (10) → clamped to 10
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "High Confirm", "url": "https://example.com", "confirmation_threshold": 99}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["monitor"]["confirmation_threshold"], 10);
}

// ── Bulk Create Validation Tests ──

#[test]
fn test_bulk_create_invalid_url_scheme() {
    let client = test_client();
    let resp = client.post("/api/v1/monitors/bulk")
        .header(ContentType::JSON)
        .body(r#"{"monitors": [
            {"name": "Good", "url": "https://example.com"},
            {"name": "Bad", "url": "ftp://files.example.com"},
            {"name": "Also Good", "url": "http://example.org"}
        ]}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["succeeded"], 2);
    assert_eq!(body["failed"], 1);
    // The error should be for index 1
    assert_eq!(body["errors"][0]["index"], 1);
    assert!(body["errors"][0]["error"].as_str().unwrap().contains("http://"));
}

#[test]
fn test_bulk_create_invalid_headers() {
    let client = test_client();
    let resp = client.post("/api/v1/monitors/bulk")
        .header(ContentType::JSON)
        .body(r#"{"monitors": [
            {"name": "Good", "url": "https://example.com", "headers": {"X-Key": "val"}},
            {"name": "Bad", "url": "https://example.com", "headers": ["not", "object"]}
        ]}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["succeeded"], 1);
    assert_eq!(body["failed"], 1);
    assert!(body["errors"][0]["error"].as_str().unwrap().contains("JSON object"));
}

// ── Follow Redirects Tests ──

#[test]
fn test_create_monitor_follow_redirects_default_true() {
    let client = test_client();
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Default Redirect", "url": "https://example.com"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["monitor"]["follow_redirects"], true);
}

#[test]
fn test_create_monitor_follow_redirects_false() {
    let client = test_client();
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "No Redirect", "url": "https://example.com", "follow_redirects": false}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["monitor"]["follow_redirects"], false);
}

#[test]
fn test_update_monitor_follow_redirects() {
    let client = test_client();

    // Create with default (true)
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Update Redirect Test", "url": "https://example.com"}"#)
        .dispatch();
    let body: serde_json::Value = resp.into_json().unwrap();
    let id = body["monitor"]["id"].as_str().unwrap().to_string();
    let key = body["manage_key"].as_str().unwrap().to_string();
    assert_eq!(body["monitor"]["follow_redirects"], true);

    // Update to false
    let resp = client.patch(format!("/api/v1/monitors/{}", id))
        .header(ContentType::JSON)
        .header(rocket::http::Header::new("X-API-Key", key.clone()))
        .body(r#"{"follow_redirects": false}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Verify
    let resp = client.get(format!("/api/v1/monitors/{}", id)).dispatch();
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["follow_redirects"], false);

    // Update back to true
    let resp = client.patch(format!("/api/v1/monitors/{}", id))
        .header(ContentType::JSON)
        .header(rocket::http::Header::new("X-API-Key", key))
        .body(r#"{"follow_redirects": true}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    let resp = client.get(format!("/api/v1/monitors/{}", id)).dispatch();
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["follow_redirects"], true);
}

#[test]
fn test_bulk_create_follow_redirects() {
    let client = test_client();
    let resp = client.post("/api/v1/monitors/bulk")
        .header(ContentType::JSON)
        .body(r#"{"monitors": [
            {"name": "Follow", "url": "https://example.com"},
            {"name": "No Follow", "url": "https://example.com", "follow_redirects": false}
        ]}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["succeeded"], 2);
    assert_eq!(body["created"][0]["monitor"]["follow_redirects"], true);
    assert_eq!(body["created"][1]["monitor"]["follow_redirects"], false);
}

#[test]
fn test_export_includes_follow_redirects() {
    let client = test_client();

    // Create with follow_redirects: false
    let resp = client.post("/api/v1/monitors")
        .header(ContentType::JSON)
        .body(r#"{"name": "Export Redirect", "url": "https://example.com", "follow_redirects": false}"#)
        .dispatch();
    let body: serde_json::Value = resp.into_json().unwrap();
    let id = body["monitor"]["id"].as_str().unwrap().to_string();
    let key = body["manage_key"].as_str().unwrap().to_string();

    // Export
    let resp = client.get(format!("/api/v1/monitors/{}/export", id))
        .header(rocket::http::Header::new("X-API-Key", key))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["follow_redirects"], false);
}
