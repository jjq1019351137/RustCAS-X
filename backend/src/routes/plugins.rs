//! Plugin registry stubs — Quark-first wave keeps runtime empty but API shape correct.
use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use axum::extract::Path;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

async fn list_plugins(user: CurrentUser) -> AppResult<Json<Vec<Value>>> {
    require_perm(&user, "plugin:read")?;
    Ok(Json(vec![]))
}

async fn refresh_plugins(user: CurrentUser) -> AppResult<Json<Vec<Value>>> {
    require_perm(&user, "plugin:write").or_else(|_| require_perm(&user, "plugin:read"))?;
    Ok(Json(vec![]))
}

#[derive(Deserialize, Default)]
struct PluginPatch {
    enabled: Option<bool>,
    priority: Option<i64>,
    config: Option<Value>,
}

async fn patch_plugin(
    user: CurrentUser,
    Path(plugin_key): Path<String>,
    Json(_payload): Json<PluginPatch>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "plugin:write")?;
    Err(AppError::bad_request(
        "PLUGIN_NOT_FOUND",
        format!("插件未移植或不存在: {plugin_key}"),
    ))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_plugins))
        .route("/refresh", post(refresh_plugins))
        .route("/{plugin_key}", patch(patch_plugin))
}

pub fn sync_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_plugins))
        .route("/refresh", post(refresh_plugins))
        .route("/{plugin_key}", patch(patch_plugin))
}
