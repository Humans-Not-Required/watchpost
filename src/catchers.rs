use rocket::catch;
use rocket::serde::json::Json;
use rocket::Request;

#[catch(400)]
pub fn bad_request(_req: &Request) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "error": "Bad request",
        "code": "BAD_REQUEST"
    }))
}

#[catch(401)]
pub fn unauthorized(_req: &Request) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "error": "Missing or invalid authentication. Provide manage_key via Authorization: Bearer, X-API-Key header, or ?key= query param.",
        "code": "UNAUTHORIZED"
    }))
}

#[catch(403)]
pub fn forbidden(_req: &Request) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "error": "Forbidden",
        "code": "FORBIDDEN"
    }))
}

#[catch(404)]
pub fn not_found(_req: &Request) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "error": "Not found",
        "code": "NOT_FOUND"
    }))
}

#[catch(422)]
pub fn unprocessable_entity(_req: &Request) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "error": "Unprocessable entity. Check that your JSON body is valid and matches the expected schema.",
        "code": "UNPROCESSABLE_ENTITY"
    }))
}

#[catch(429)]
pub fn too_many_requests(_req: &Request) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "error": "Rate limit exceeded",
        "code": "RATE_LIMIT_EXCEEDED"
    }))
}

#[catch(500)]
pub fn internal_error(_req: &Request) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "error": "Internal server error",
        "code": "INTERNAL_ERROR"
    }))
}
