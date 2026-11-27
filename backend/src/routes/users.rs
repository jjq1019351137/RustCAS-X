use crate::auth_ctx::{load_user_roles_permissions, require_perm, CurrentUser};
use crate::error::{AppError, AppResult};
use crate::security::{hash_password, validate_password_strength};
use crate::services::audit;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default = "default_page_size")]
    page_size: i64,
    q: Option<String>,
}
fn default_page() -> i64 {
    1
}
fn default_page_size() -> i64 {
    20
}

#[derive(Deserialize)]
pub struct CreateUserIn {
    username: String,
    email: String,
    password: String,
}

#[derive(Deserialize)]
pub struct StatusIn {
    is_active: bool,
}

#[derive(Deserialize)]
pub struct RolesIn {
    role_ids: Vec<i64>,
}

async fn list_users(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "user:read")?;
    let page = q.page.max(1);
    let page_size = q.page_size.clamp(1, 100);
    let offset = (page - 1) * page_size;
    let like = q.q.as_ref().map(|s| format!("%{s}%"));

    let total: i64 = if let Some(ref pat) = like {
        sqlx::query_scalar("SELECT COUNT(1) FROM users WHERE username LIKE ? OR email LIKE ?")
            .bind(pat)
            .bind(pat)
            .fetch_one(&state.db)
            .await?
    } else {
        sqlx::query_scalar("SELECT COUNT(1) FROM users")
            .fetch_one(&state.db)
            .await?
    };

    let rows = if let Some(ref pat) = like {
        sqlx::query_as::<_, (i64, String, String, i64, String, String)>(
            r#"
            SELECT id, username, email, is_active, created_at, updated_at
            FROM users WHERE username LIKE ? OR email LIKE ?
            ORDER BY id ASC LIMIT ? OFFSET ?
            "#,
        )
        .bind(pat)
        .bind(pat)
        .bind(page_size)
        .bind(offset)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as::<_, (i64, String, String, i64, String, String)>(
            r#"
            SELECT id, username, email, is_active, created_at, updated_at
            FROM users ORDER BY id ASC LIMIT ? OFFSET ?
            "#,
        )
        .bind(page_size)
        .bind(offset)
        .fetch_all(&state.db)
        .await?
    };

    let mut items = Vec::new();
    for (id, username, email, is_active, created_at, updated_at) in rows {
        let (roles, permissions) = load_user_roles_permissions(&state.db, id).await?;
        items.push(json!({
            "id": id,
            "username": username,
            "email": email,
            "is_active": is_active != 0,
            "roles": roles,
            "permissions": permissions,
            "created_at": created_at,
            "updated_at": updated_at
        }));
    }

    Ok(Json(json!({
        "items": items,
        "total": total,
        "page": page,
        "page_size": page_size
    })))
}

async fn create_user(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<CreateUserIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "user:write")?;
    validate_password_strength(&payload.password)?;
    let hashed = hash_password(&payload.password)?;
    let id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO users (username, email, hashed_password, is_active)
        VALUES (?, ?, ?, 1)
        RETURNING id
        "#,
    )
    .bind(&payload.username)
    .bind(&payload.email)
    .bind(&hashed)
    .fetch_one(&state.db)
    .await
    .map_err(|e| AppError::bad_request("USER_CREATE", e.to_string()))?;

    audit::write_user(
        &state.db,
        user.id,
        "user.create",
        "user",
        id,
        Some(&payload.username),
    )
    .await;

    Ok(Json(json!({
        "id": id,
        "username": payload.username,
        "email": payload.email,
        "is_active": true
    })))
}

async fn set_status(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(user_id): Path<i64>,
    Json(payload): Json<StatusIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "user:write")?;
    sqlx::query("UPDATE users SET is_active=? WHERE id=?")
        .bind(if payload.is_active { 1 } else { 0 })
        .bind(user_id)
        .execute(&state.db)
        .await?;
    audit::write_user(
        &state.db,
        user.id,
        "user.status",
        "user",
        user_id,
        Some(&format!("is_active={}", payload.is_active)),
    )
    .await;
    Ok(Json(json!({ "ok": true, "is_active": payload.is_active })))
}

async fn set_roles(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(user_id): Path<i64>,
    Json(payload): Json<RolesIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "user:write")?;
    sqlx::query("DELETE FROM user_roles WHERE user_id = ?")
        .bind(user_id)
        .execute(&state.db)
        .await?;
    for rid in payload.role_ids {
        sqlx::query("INSERT OR IGNORE INTO user_roles (user_id, role_id) VALUES (?, ?)")
            .bind(user_id)
            .bind(rid)
            .execute(&state.db)
            .await?;
    }
    audit::write_user(&state.db, user.id, "user.roles", "user", user_id, None).await;
    let (roles, permissions) = load_user_roles_permissions(&state.db, user_id).await?;
    Ok(Json(json!({ "roles": roles, "permissions": permissions })))
}

#[derive(Deserialize)]
struct BatchStatusIn {
    user_ids: Vec<i64>,
    is_active: bool,
}

#[derive(Deserialize)]
struct BatchRolesIn {
    user_ids: Vec<i64>,
    role_ids: Vec<i64>,
}

async fn batch_status(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<BatchStatusIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "user:write")?;
    let mut updated = 0i64;
    for uid in &payload.user_ids {
        let n = sqlx::query("UPDATE users SET is_active=? WHERE id=?")
            .bind(if payload.is_active { 1 } else { 0 })
            .bind(uid)
            .execute(&state.db)
            .await?
            .rows_affected();
        updated += n as i64;
    }
    audit::write_user(
        &state.db,
        user.id,
        "user.batch_status",
        "user",
        "*",
        Some(&format!(
            "count={} is_active={}",
            payload.user_ids.len(),
            payload.is_active
        )),
    )
    .await;
    Ok(Json(json!({ "ok": true, "updated": updated })))
}

async fn batch_roles(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<BatchRolesIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "user:write")?;
    for uid in &payload.user_ids {
        sqlx::query("DELETE FROM user_roles WHERE user_id = ?")
            .bind(uid)
            .execute(&state.db)
            .await?;
        for rid in &payload.role_ids {
            sqlx::query("INSERT OR IGNORE INTO user_roles (user_id, role_id) VALUES (?, ?)")
                .bind(uid)
                .bind(rid)
                .execute(&state.db)
                .await?;
        }
    }
    audit::write_user(
        &state.db,
        user.id,
        "user.batch_roles",
        "user",
        "*",
        Some(&format!("count={}", payload.user_ids.len())),
    )
    .await;
    Ok(Json(json!({ "ok": true, "updated": payload.user_ids.len() })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_users).post(create_user))
        .route("/batch/status", post(batch_status))
        .route("/batch/roles", post(batch_roles))
        .route("/{user_id}/status", patch(set_status))
        .route("/{user_id}/roles", post(set_roles))
}
