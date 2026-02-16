use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};
use sha2::{Sha256, Digest};

/// Extracts a manage key from Bearer header, X-API-Key header, or ?key= query param.
pub struct ManageToken(pub String);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ManageToken {
    type Error = &'static str;

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        // 1. Authorization: Bearer <token>
        if let Some(auth) = request.headers().get_one("Authorization") {
            if let Some(token) = auth.strip_prefix("Bearer ") {
                return Outcome::Success(ManageToken(token.to_string()));
            }
        }

        // 2. X-API-Key: <token>
        if let Some(key) = request.headers().get_one("X-API-Key") {
            return Outcome::Success(ManageToken(key.to_string()));
        }

        // 3. ?key=<token>
        if let Some(Ok(key)) = request.query_value::<String>("key") {
            return Outcome::Success(ManageToken(key));
        }

        Outcome::Error((Status::Unauthorized, "Missing manage key"))
    }
}

/// Like ManageToken but doesn't fail if no auth is provided — returns None.
pub struct OptionalManageToken(pub Option<String>);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for OptionalManageToken {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        // 1. Authorization: Bearer <token>
        if let Some(auth) = request.headers().get_one("Authorization") {
            if let Some(token) = auth.strip_prefix("Bearer ") {
                return Outcome::Success(OptionalManageToken(Some(token.to_string())));
            }
        }

        // 2. X-API-Key: <token>
        if let Some(key) = request.headers().get_one("X-API-Key") {
            return Outcome::Success(OptionalManageToken(Some(key.to_string())));
        }

        // 3. ?key=<token>
        if let Some(Ok(key)) = request.query_value::<String>("key") {
            return Outcome::Success(OptionalManageToken(Some(key)));
        }

        Outcome::Success(OptionalManageToken(None))
    }
}

pub fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn generate_key() -> String {
    format!("wp_{}", hex::encode(rand::random::<[u8; 16]>()))
}

/// Extract client IP for rate limiting.
pub struct ClientIp(pub String);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ClientIp {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        // Check forwarded headers first
        if let Some(xff) = request.headers().get_one("X-Forwarded-For") {
            if let Some(first) = xff.split(',').next() {
                return Outcome::Success(ClientIp(first.trim().to_string()));
            }
        }
        if let Some(real) = request.headers().get_one("X-Real-Ip") {
            return Outcome::Success(ClientIp(real.to_string()));
        }
        // Fall back to socket address
        let ip = request.client_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        Outcome::Success(ClientIp(ip))
    }
}
