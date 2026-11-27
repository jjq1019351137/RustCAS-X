//! Compatibility stubs so the Vue frontend can boot while adapters/runtime are ported.
use crate::auth_ctx::CurrentUser;
use crate::error::AppResult;
use crate::state::AppState;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

async fn empty_list(_user: CurrentUser) -> AppResult<Json<Vec<Value>>> {
    Ok(Json(vec![]))
}

async fn empty_object(_user: CurrentUser) -> AppResult<Json<Value>> {
    Ok(Json(json!({})))
}

async fn plugins_list(_user: CurrentUser) -> AppResult<Json<Value>> {
    Ok(Json(json!({
        "items": [],
        "note": "RustCAS-X: plugin runtime not ported yet"
    })))
}

async fn drive_accounts(_user: CurrentUser) -> AppResult<Json<Vec<Value>>> {
    Ok(Json(vec![]))
}

async fn system_settings(_user: CurrentUser) -> AppResult<Json<Value>> {
    Ok(Json(json!({
        "runtime": "rust",
        "migration": "in_progress"
    })))
}

async fn tmdb_settings(_user: CurrentUser) -> AppResult<Json<Value>> {
    Ok(Json(json!({
        "enabled": false,
        "api_key": null
    })))
}

async fn openlist_settings(_user: CurrentUser) -> AppResult<Json<Value>> {
    Ok(Json(json!({
        "enabled": false
    })))
}

async fn notifications(_user: CurrentUser) -> AppResult<Json<Value>> {
    Ok(Json(json!({
        "channels": []
    })))
}

async fn users_list(_user: CurrentUser) -> AppResult<Json<Vec<Value>>> {
    Ok(Json(vec![]))
}

async fn roles_list(_user: CurrentUser) -> AppResult<Json<Vec<Value>>> {
    Ok(Json(vec![]))
}

async fn permissions_list(_user: CurrentUser) -> AppResult<Json<Vec<Value>>> {
    Ok(Json(vec![]))
}

async fn metrics() -> AppResult<Json<Value>> {
    Ok(Json(json!({ "ok": true, "runtime": "rust" })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/drive-accounts", get(drive_accounts))
        .route("/plugins", get(plugins_list))
        .route("/users", get(users_list))
        .route("/roles", get(roles_list))
        .route("/permissions", get(permissions_list))
        .route("/audit-logs", get(empty_list))
        .route("/system-settings", get(system_settings))
        .route("/tmdb/settings", get(tmdb_settings))
        .route("/openlist/settings", get(openlist_settings))
        .route("/notifications", get(notifications))
        .route("/magic-regex", get(empty_object))
        .route("/metrics", get(metrics))
}
