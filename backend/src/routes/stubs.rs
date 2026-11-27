//! Remaining compatibility stubs while plugins/notifications keep migrating.
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

async fn metrics() -> AppResult<Json<Value>> {
    Ok(Json(json!({ "ok": true, "runtime": "rust", "wave": "quark-compat" })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/plugins", get(plugins_list))
        .route("/audit-logs", get(empty_list))
        .route("/metrics", get(metrics))
}
