use rocket::{get, serde::json::Json};
use rocket::http::ContentType;

// ── Health ──

#[get("/health")]
pub fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "service": "watchpost",
        "status": "ok",
        "version": "0.1.0"
    }))
}

// ── SKILL.md / llms.txt ──

/// GET /SKILL.md — canonical AI-readable service guide
#[get("/SKILL.md")]
pub fn skill_md() -> (rocket::http::ContentType, &'static str) {
    (rocket::http::ContentType::Plain, include_str!("../../SKILL.md"))
}

#[get("/llms.txt")]
pub fn llms_txt() -> (rocket::http::ContentType, &'static str) {
    (rocket::http::ContentType::Plain, include_str!("../../SKILL.md"))
}

/// Root-level /llms.txt for standard discoverability (outside /api/v1 mount)
#[get("/llms.txt")]
pub fn root_llms_txt() -> (rocket::http::ContentType, &'static str) {
    (rocket::http::ContentType::Plain, include_str!("../../SKILL.md"))
}

// ── OpenAPI Spec ──

#[get("/openapi.json")]
pub fn openapi_spec() -> (rocket::http::ContentType, &'static str) {
    (rocket::http::ContentType::JSON, include_str!("../../static/openapi.json"))
}

// ── Well-Known Skills Discovery ──

#[get("/.well-known/skills/index.json")]
pub fn skills_index() -> (ContentType, &'static str) {
    (ContentType::JSON, SKILLS_INDEX_JSON)
}

#[get("/.well-known/skills/watchpost/SKILL.md")]
pub fn skills_skill_md() -> (ContentType, &'static str) {
    (ContentType::Plain, include_str!("../../SKILL.md"))
}

/// GET /skills/SKILL.md — alternate path for agent discoverability
#[get("/skills/SKILL.md")]
pub fn api_skills_skill_md() -> (ContentType, &'static str) {
    (ContentType::Plain, include_str!("../../SKILL.md"))
}

const SKILLS_INDEX_JSON: &str = r#"{
  "skills": [
    {
      "name": "watchpost",
      "description": "Integrate with Watchpost — an agent-native uptime monitoring service. Create HTTP/TCP/DNS monitors, track incidents, configure alerts, stream events via SSE, and build monitoring automation on a private network.",
      "url": "/SKILL.md",
      "files": [
        "SKILL.md"
      ]
    }
  ]
}"#;

// ── SPA Fallback ──

#[get("/<_path..>", rank = 100)]
pub fn spa_fallback(_path: std::path::PathBuf) -> Option<(ContentType, Vec<u8>)> {
    let static_dir: std::path::PathBuf = std::env::var("STATIC_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("../frontend/dist"));
    let index_path = static_dir.join("index.html");
    std::fs::read(&index_path)
        .ok()
        .map(|bytes| (ContentType::HTML, bytes))
}
