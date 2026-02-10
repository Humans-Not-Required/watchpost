#[macro_use] extern crate rocket;

mod db;
mod models;
mod auth;
mod routes;
mod checker;

use std::sync::Arc;
use db::Db;

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

    let checker_db = database.clone();

    rocket::build()
        .manage(database)
        .manage(rate_limiter)
        .mount("/api/v1", routes![
            routes::health,
            routes::create_monitor,
            routes::list_monitors,
            routes::get_monitor,
            routes::update_monitor,
            routes::delete_monitor,
            routes::pause_monitor,
            routes::resume_monitor,
            routes::get_heartbeats,
            routes::get_uptime,
            routes::get_incidents,
            routes::acknowledge_incident,
            routes::status_page,
            routes::create_notification,
            routes::list_notifications,
            routes::delete_notification,
            routes::llms_txt,
        ])
        .attach(rocket::fairing::AdHoc::on_liftoff("Checker", move |rocket| {
            Box::pin(async move {
                let shutdown = rocket.shutdown();
                tokio::spawn(checker::run_checker(checker_db, shutdown));
            })
        }))
}
