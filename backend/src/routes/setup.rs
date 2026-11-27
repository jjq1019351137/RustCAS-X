use crate::auth_ctx::load_user_roles_permissions;
use crate::db::ensure_permissions_and_roles;
use crate::error::{AppError, AppResult};
use crate::routes::auth::{LoginOut, MeOut};
use crate::security::{create_access_token, generate_refresh_token, hash_password, hash_refresh_token, validate_password_strength};
use crate::state::AppState;
use axum::extract::State;
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::CookieJar;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct SetupStatusOut {
    initialized: bool,
}

#[derive(Deserialize)]
struct SetupAdminIn {
    username: String,
    password: String,
    email: Option<String>,
}

async fn status(State(state): State<AppState>) -> AppResult<Json<SetupStatusOut>> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(1) FROM users")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);
    Ok(Json(SetupStatusOut {
        initialized: count > 0,
    }))
}

async fn create_admin(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(payload): Json<SetupAdminIn>,
) -> AppResult<Response> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(1) FROM users")
        .fetch_one(&state.db)
        .await?;
    if count > 0 {
        return Err(AppError::conflict(
            "SETUP_ALREADY_INITIALIZED",
            "系统已初始化",
        ));
    }

    validate_password_strength(&payload.password)?;
    ensure_permissions_and_roles(&state.db)
        .await
        .map_err(AppError::from)?;

    let email = payload
        .email
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("{}@local", payload.username));
    let hashed = hash_password(&payload.password)?;

    let user_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO users (username, email, hashed_password, is_active)
        VALUES (?, ?, ?, 1)
        RETURNING id
        "#,
    )
    .bind(&payload.username)
    .bind(&email)
    .bind(&hashed)
    .fetch_one(&state.db)
    .await?;

    let role_id: i64 = sqlx::query_scalar("SELECT id FROM roles WHERE name = 'admin'")
        .fetch_one(&state.db)
        .await?;
    sqlx::query("INSERT OR IGNORE INTO user_roles (user_id, role_id) VALUES (?, ?)")
        .bind(user_id)
        .bind(role_id)
        .execute(&state.db)
        .await?;

    let (roles, permissions) = load_user_roles_permissions(&state.db, user_id).await?;

    let refresh_plain = generate_refresh_token();
    let refresh_hash = hash_refresh_token(&state.settings, &refresh_plain);
    let now = Utc::now();
    let expires_at = now + Duration::seconds(state.settings.refresh_token_expires_seconds);
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok());

    sqlx::query(
        r#"
        INSERT INTO refresh_tokens (user_id, token_hash, expires_at, user_agent, last_used_at)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(user_id)
    .bind(&refresh_hash)
    .bind(expires_at.to_rfc3339())
    .bind(ua)
    .bind(now.to_rfc3339())
    .execute(&state.db)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO audit_logs (actor_user_id, action, target_type, target_id, user_agent, success)
        VALUES (NULL, 'setup.init_admin', 'user', ?, ?, 1)
        "#,
    )
    .bind(user_id.to_string())
    .bind(ua)
    .execute(&state.db)
    .await?;

    let access = create_access_token(&state.settings, &user_id.to_string(), &permissions)?;
    let mut cookie = Cookie::build((
        state.settings.refresh_token_cookie_name.clone(),
        refresh_plain,
    ))
    .path("/")
    .http_only(true)
    .same_site(SameSite::Lax)
    .max_age(time::Duration::seconds(
        state.settings.refresh_token_expires_seconds,
    ))
    .build();
    if state.settings.refresh_token_cookie_secure {
        cookie.set_secure(true);
    }

    let body = LoginOut {
        access_token: access,
        token_type: "bearer",
        expires_in: state.settings.access_token_expires_seconds,
        user: MeOut {
            id: user_id,
            username: payload.username,
            email,
            roles,
            permissions,
        },
    };
    Ok((jar.add(cookie), Json(body)).into_response())
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/status", get(status))
        .route("/admin", post(create_admin))
}
