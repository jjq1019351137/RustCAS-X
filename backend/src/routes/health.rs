use crate::error::AppResult;
use crate::state::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use chrono::Local;
use serde::Serialize;

#[derive(Serialize)]
struct HealthOut {
    status: &'static str,
    db_connected: bool,
    time: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    build_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    build_tag: Option<String>,
    runtime: &'static str,
}

async fn health(State(state): State<AppState>) -> AppResult<Json<HealthOut>> {
    let db_connected = sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();

    Ok(Json(HealthOut {
        status: if db_connected { "ok" } else { "degraded" },
        db_connected,
        time: Local::now().to_rfc3339(),
        build_sha: state.settings.build_sha.clone(),
        build_tag: state.settings.build_tag.clone(),
        runtime: "rust",
    }))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/health", get(health))
}
