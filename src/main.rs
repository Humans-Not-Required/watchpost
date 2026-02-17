#[macro_use] extern crate rocket;

mod db;
mod models;
mod auth;
mod routes;
mod checker;
mod consensus;
mod notifications;
mod sse;
mod catchers;

use std::path::PathBuf;
use std::sync::Arc;
use db::Db;
use rocket::fs::{FileServer, Options};
use rocket_cors::{AllowedOrigins, CorsOptions};

#[launch]
fn rocket() -> _ {
    dotenvy::dotenv().ok();

    let db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "watchpost.db".into());
    let database = Arc::new(Db::new(&db_path).expect("Failed to initialize database"));

    let rate_limit = std::env::var("MONITOR_RATE_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10u32);

    let rate_limiter = routes::RateLimiter::new(rate_limit, 3600);
    let broadcaster = Arc::new(sse::EventBroadcaster::new(256));

    let checker_db = database.clone();
    let checker_broadcaster = broadcaster.clone();

    let cors = CorsOptions::default()
        .allowed_origins(AllowedOrigins::all())
        .to_cors()
        .expect("CORS configuration failed");

    let static_dir: PathBuf = std::env::var("STATIC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("../frontend/dist"));

    let mut build = rocket::build()
        .attach(cors)
        .manage(database)
        .manage(rate_limiter)
        .manage(broadcaster)
        .mount("/api/v1", routes![
            routes::health,
            routes::create_monitor,
            routes::bulk_create_monitors,
            routes::export_monitor,
            routes::list_monitors,
            routes::get_monitor,
            routes::update_monitor,
            routes::delete_monitor,
            routes::pause_monitor,
            routes::resume_monitor,
            routes::get_heartbeats,
            routes::get_uptime,
            routes::get_incidents,
            routes::get_incident,
            routes::acknowledge_incident,
            routes::create_incident_note,
            routes::list_incident_notes,
            routes::dashboard,
            routes::admin_verify,
            routes::uptime_history,
            routes::monitor_uptime_history,
            routes::status_page,
            routes::create_notification,
            routes::list_notifications,
            routes::delete_notification,
            routes::update_notification,
            routes::list_tags,
            routes::list_groups,
            routes::get_settings,
            routes::update_settings,
            routes::create_maintenance_window,
            routes::list_maintenance_windows,
            routes::delete_maintenance_window,
            routes::llms_txt,
            routes::openapi_spec,
            routes::monitor_uptime_badge,
            routes::monitor_status_badge,
            routes::monitor_sla,
            routes::global_events,
            routes::monitor_events,
            routes::create_location,
            routes::list_locations,
            routes::get_location,
            routes::delete_location,
            routes::submit_probe,
            routes::monitor_location_status,
            routes::monitor_consensus,
            routes::create_status_page,
            routes::list_status_pages,
            routes::get_status_page_detail,
            routes::update_status_page,
            routes::delete_status_page,
            routes::add_page_monitors,
            routes::remove_page_monitor,
            routes::list_page_monitors,
            routes::set_alert_rules,
            routes::get_alert_rules,
            routes::delete_alert_rules,
            routes::get_alert_log,
            routes::list_webhook_deliveries,
            routes::add_dependency,
            routes::list_dependencies,
            routes::remove_dependency,
            routes::list_dependents,
        ])
        .register("/", catchers![
            catchers::bad_request,
            catchers::unauthorized,
            catchers::forbidden,
            catchers::not_found,
            catchers::unprocessable_entity,
            catchers::too_many_requests,
            catchers::internal_error,
        ])
        .attach(rocket::fairing::AdHoc::on_liftoff("Checker", move |rocket| {
            Box::pin(async move {
                let shutdown = rocket.shutdown();
                println!("🚀 Spawning checker task...");
                let handle = tokio::spawn(checker::run_checker(checker_db, checker_broadcaster, shutdown));
                // Monitor the checker task for unexpected exits
                tokio::spawn(async move {
                    match handle.await {
                        Ok(()) => eprintln!("⚠️  Checker task exited normally (unexpected)"),
                        Err(e) => eprintln!("❌ Checker task failed: {e}"),
                    }
                });
            })
        }));

    // Well-known skills discovery (mounted at root, outside /api/v1)
    build = build.mount("/", routes![
        routes::root_llms_txt,
        routes::skills_index,
        routes::skills_skill_md,
    ]);

    // Serve frontend static files if the directory exists
    if static_dir.is_dir() {
        println!("📦 Serving frontend from: {}", static_dir.display());
        build = build
            .mount("/", FileServer::new(&static_dir, Options::Index))
            .mount("/", routes![routes::spa_fallback]);
    } else {
        println!(
            "⚠️  Frontend directory not found: {} (API-only mode)",
            static_dir.display()
        );
    }

    build
}
