use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
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
            watchpost::routes::status_page,
            watchpost::routes::create_notification,
            watchpost::routes::list_notifications,
            watchpost::routes::delete_notification,
            watchpost::routes::llms_txt,
            watchpost::routes::global_events,
            watchpost::routes::monitor_events,
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
        .body(r#"{"name": "My API", "url": "https://example.com/health", "interval_seconds": 60}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["monitor"]["name"], "My API");
    assert_eq!(body["monitor"]["url"], "https://example.com/health");
    assert_eq!(body["monitor"]["interval_seconds"], 60);
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
        .body(r#"{"name": "Updated Service", "interval_seconds": 120}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Verify update
    let resp = client.get(format!("/api/v1/monitors/{}", id)).dispatch();
    let body: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(body["name"], "Updated Service");
    assert_eq!(body["interval_seconds"], 120);
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
    assert_eq!(m["interval_seconds"], 300);
    assert_eq!(m["timeout_ms"], 10000);
    assert_eq!(m["expected_status"], 200);
    assert_eq!(m["is_public"], false);
    assert_eq!(m["is_paused"], false);
    assert_eq!(m["confirmation_threshold"], 2);
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
