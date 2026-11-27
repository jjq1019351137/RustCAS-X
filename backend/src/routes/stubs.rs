//! Remaining thin compatibility stubs.
use crate::error::AppResult;
use crate::state::AppState;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

async fn metrics() -> AppResult<Json<Value>> {
    Ok(Json(json!({ "ok": true, "runtime": "rust", "wave": "quark-compat" })))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/metrics", get(metrics))
}
