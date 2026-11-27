use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::AppResult;
use crate::state::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

async fn list_roles(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Value>> {
    require_perm(&user, "role:read")?;
    let roles = sqlx::query_as::<_, (i64, String, Option<String>)>(
        "SELECT id, name, description FROM roles ORDER BY id",
    )
    .fetch_all(&state.db)
    .await?;

    let mut out = Vec::new();
    for (id, name, description) in roles {
        let perms: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT p.code FROM permissions p
            JOIN role_permissions rp ON rp.permission_id = p.id
            WHERE rp.role_id = ?
            ORDER BY p.code
            "#,
        )
        .bind(id)
        .fetch_all(&state.db)
        .await?;
        out.push(json!({
            "id": id,
            "name": name,
            "description": description,
            "permissions": perms
        }));
    }
    Ok(Json(json!(out)))
}

async fn list_permissions(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Value>> {
    require_perm(&user, "permission:read")?;
    let rows = sqlx::query_as::<_, (i64, String, String)>(
        "SELECT id, code, name FROM permissions ORDER BY code",
    )
    .fetch_all(&state.db)
    .await?;
    let out: Vec<Value> = rows
        .into_iter()
        .map(|(id, code, name)| json!({ "id": id, "code": code, "name": name }))
        .collect();
    Ok(Json(json!(out)))
}

pub fn roles_router() -> Router<AppState> {
    Router::new().route("/", get(list_roles))
}

pub fn permissions_router() -> Router<AppState> {
    Router::new().route("/", get(list_permissions))
}
