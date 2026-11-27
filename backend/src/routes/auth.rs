use crate::auth_ctx::{load_user_roles_permissions, CurrentUser};
use crate::error::{AppError, AppResult};
use crate::security::{
    create_access_token, generate_refresh_token, hash_refresh_token, verify_password,
};
use crate::state::AppState;
use axum::extract::State;
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::CookieJar;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct MeOut {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
}

#[derive(Serialize)]
pub struct TokenOut {
    pub access_token: String,
    pub token_type: &'static str,
    pub expires_in: i64,
}

#[derive(Serialize)]
pub struct LoginOut {
    pub access_token: String,
    pub token_type: &'static str,
    pub expires_in: i64,
    pub user: MeOut,
}

fn set_refresh_cookie(state: &AppState, jar: CookieJar, token: String) -> CookieJar {
    let mut cookie = Cookie::build((state.settings.refresh_token_cookie_name.clone(), token))
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
    jar.add(cookie)
}

fn clear_refresh_cookie(state: &AppState, jar: CookieJar) -> CookieJar {
    let cookie = Cookie::build((state.settings.refresh_token_cookie_name.clone(), ""))
        .path("/")
        .http_only(true)
        .max_age(time::Duration::seconds(0))
        .build();
    jar.remove(cookie)
}

async fn write_audit(
    db: &sqlx::SqlitePool,
    actor_user_id: Option<i64>,
    action: &str,
    ip: Option<&str>,
    user_agent: Option<&str>,
) -> AppResult<()> {
    sqlx::query(
        r#"
        INSERT INTO audit_logs (actor_user_id, action, ip, user_agent, success)
        VALUES (?, ?, ?, ?, 1)
        "#,
    )
    .bind(actor_user_id)
    .bind(action)
    .bind(ip)
    .bind(user_agent)
    .execute(db)
    .await?;
    Ok(())
}

async fn issue_tokens(
    state: &AppState,
    jar: CookieJar,
    user_id: i64,
    permissions: &[String],
    ip: Option<&str>,
    user_agent: Option<&str>,
) -> AppResult<(String, CookieJar)> {
    let refresh_plain = generate_refresh_token();
    let refresh_hash = hash_refresh_token(&state.settings, &refresh_plain);
    let now = Utc::now();
    let expires_at = now + Duration::seconds(state.settings.refresh_token_expires_seconds);

    sqlx::query(
        r#"
        INSERT INTO refresh_tokens (user_id, token_hash, expires_at, ip, user_agent, last_used_at)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(user_id)
    .bind(&refresh_hash)
    .bind(expires_at.to_rfc3339())
    .bind(ip)
    .bind(user_agent)
    .bind(now.to_rfc3339())
    .execute(&state.db)
    .await?;

    let access = create_access_token(&state.settings, &user_id.to_string(), permissions)?;
    Ok((access, set_refresh_cookie(state, jar, refresh_plain)))
}

fn client_meta(headers: &HeaderMap) -> (Option<String>, Option<String>) {
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string());
    (ip, ua)
}

async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> AppResult<Response> {
    let row = sqlx::query_as::<_, (i64, String, String, String, i64)>(
        "SELECT id, username, email, hashed_password, is_active FROM users WHERE username = ?",
    )
    .bind(&form.username)
    .fetch_optional(&state.db)
    .await?;

    let Some((id, username, email, hashed, is_active)) = row else {
        return Err(AppError::unauthorized(
            "AUTH_BAD_CREDENTIALS",
            "用户名或密码错误",
        ));
    };
    if is_active == 0 || !verify_password(&form.password, &hashed) {
        return Err(AppError::unauthorized(
            "AUTH_BAD_CREDENTIALS",
            "用户名或密码错误",
        ));
    }

    let (roles, permissions) = load_user_roles_permissions(&state.db, id).await?;
    let (ip, ua) = client_meta(&headers);
    let (access, jar) = issue_tokens(
        &state,
        jar,
        id,
        &permissions,
        ip.as_deref(),
        ua.as_deref(),
    )
    .await?;
    write_audit(
        &state.db,
        Some(id),
        "auth.login",
        ip.as_deref(),
        ua.as_deref(),
    )
    .await?;

    let body = LoginOut {
        access_token: access,
        token_type: "bearer",
        expires_in: state.settings.access_token_expires_seconds,
        user: MeOut {
            id,
            username,
            email,
            roles,
            permissions,
        },
    };
    Ok((jar, Json(body)).into_response())
}

async fn refresh(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> AppResult<Response> {
    let plain = jar
        .get(&state.settings.refresh_token_cookie_name)
        .map(|c| c.value().to_string())
        .ok_or_else(|| AppError::unauthorized("AUTH_NO_REFRESH", "请重新登录"))?;

    let token_hash = hash_refresh_token(&state.settings, &plain);
    let row = sqlx::query_as::<_, (i64, i64, String, Option<String>)>(
        "SELECT id, user_id, expires_at, revoked_at FROM refresh_tokens WHERE token_hash = ?",
    )
    .bind(&token_hash)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::unauthorized("AUTH_REFRESH_INVALID", "请重新登录"))?;

    let (token_id, user_id, expires_at, revoked_at) = row;
    if revoked_at.is_some() {
        return Err(AppError::unauthorized("AUTH_REFRESH_EXPIRED", "请重新登录"));
    }
    let exp = chrono::DateTime::parse_from_rfc3339(&expires_at)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now() - Duration::seconds(1));
    if exp <= Utc::now() {
        return Err(AppError::unauthorized("AUTH_REFRESH_EXPIRED", "请重新登录"));
    }

    let active: i64 =
        sqlx::query_scalar("SELECT is_active FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?
            .unwrap_or(0);
    if active == 0 {
        return Err(AppError::unauthorized("AUTH_INVALID_USER", "账号不可用"));
    }

    sqlx::query("UPDATE refresh_tokens SET revoked_at = ?, last_used_at = ? WHERE id = ?")
        .bind(Utc::now().to_rfc3339())
        .bind(Utc::now().to_rfc3339())
        .bind(token_id)
        .execute(&state.db)
        .await?;

    let (_, permissions) = load_user_roles_permissions(&state.db, user_id).await?;
    let (ip, ua) = client_meta(&headers);
    let (access, jar) = issue_tokens(
        &state,
        jar,
        user_id,
        &permissions,
        ip.as_deref(),
        ua.as_deref(),
    )
    .await?;

    let body = TokenOut {
        access_token: access,
        token_type: "bearer",
        expires_in: state.settings.access_token_expires_seconds,
    };
    Ok((jar, Json(body)).into_response())
}

async fn logout(
    State(state): State<AppState>,
    current: CurrentUser,
    jar: CookieJar,
    headers: HeaderMap,
) -> AppResult<Response> {
    if let Some(plain) = jar
        .get(&state.settings.refresh_token_cookie_name)
        .map(|c| c.value().to_string())
    {
        let token_hash = hash_refresh_token(&state.settings, &plain);
        sqlx::query(
            "UPDATE refresh_tokens SET revoked_at = ? WHERE token_hash = ? AND revoked_at IS NULL",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(token_hash)
        .execute(&state.db)
        .await?;
    }

    let (ip, ua) = client_meta(&headers);
    write_audit(
        &state.db,
        Some(current.id),
        "auth.logout",
        ip.as_deref(),
        ua.as_deref(),
    )
    .await?;

    let jar = clear_refresh_cookie(&state, jar);
    Ok((jar, Json(serde_json::json!({ "ok": true }))).into_response())
}

async fn me(State(state): State<AppState>, current: CurrentUser) -> AppResult<Json<MeOut>> {
    let (roles, permissions) = load_user_roles_permissions(&state.db, current.id).await?;
    Ok(Json(MeOut {
        id: current.id,
        username: current.username,
        email: current.email,
        roles,
        permissions,
    }))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login", post(login))
        .route("/refresh", post(refresh))
        .route("/logout", post(logout))
        .route("/me", get(me))
}
