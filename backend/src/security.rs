use crate::config::Settings;
use crate::error::{AppError, AppResult};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub perms: Vec<String>,
    pub iat: i64,
    pub exp: i64,
}

pub fn hash_password(plain: &str) -> AppResult<String> {
    bcrypt::hash(plain, bcrypt::DEFAULT_COST)
        .map_err(|e| AppError::api(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "HASH_FAIL", e.to_string()))
}

pub fn verify_password(plain: &str, hashed: &str) -> bool {
    bcrypt::verify(plain, hashed).unwrap_or(false)
}

pub fn validate_password_strength(password: &str) -> AppResult<()> {
    if password.len() < 8 {
        return Err(AppError::bad_request("PASSWORD_WEAK", "密码至少8位"));
    }
    if !password.chars().any(|c| !c.is_alphanumeric()) {
        return Err(AppError::bad_request(
            "PASSWORD_WEAK",
            "密码强度不足: 至少8位且包含特殊字符",
        ));
    }
    Ok(())
}

pub fn create_access_token(settings: &Settings, subject: &str, permissions: &[String]) -> AppResult<String> {
    let now = Utc::now();
    let claims = Claims {
        sub: subject.to_string(),
        perms: permissions.to_vec(),
        iat: now.timestamp(),
        exp: (now + Duration::seconds(settings.access_token_expires_seconds)).timestamp(),
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(settings.jwt_secret_key.as_bytes()),
    )
    .map_err(|e| AppError::api(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "JWT_ENCODE", e.to_string()))
}

pub fn decode_access_token(settings: &Settings, token: &str) -> AppResult<Claims> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(settings.jwt_secret_key.as_bytes()),
        &validation,
    )
    .map(|d| d.claims)
    .map_err(|_| AppError::unauthorized("AUTH_INVALID_TOKEN", "无效令牌"))
}

pub fn generate_refresh_token() -> String {
    let mut buf = [0u8; 48];
    rand::thread_rng().fill_bytes(&mut buf);
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, buf)
}

pub fn hash_refresh_token(settings: &Settings, token: &str) -> String {
    let raw = format!("{}:{}", token, settings.refresh_token_pepper);
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    hex::encode(hasher.finalize())
}
