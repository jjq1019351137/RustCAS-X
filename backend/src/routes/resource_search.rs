use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::{AppError, AppResult};
use crate::services::resource_search::{self, SourcePatch};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, patch};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, Default)]
struct SourcePatchIn {
    enabled: Option<bool>,
    server: Option<String>,
    username: Option<String>,
    password: Option<String>,
    token: Option<String>,
}

async fn list_sources(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let out = resource_search::list_sources(&state.db)
        .await
        .map_err(|e| AppError::bad_request("RESOURCE_SEARCH", e.to_string()))?;
    Ok(Json(out))
}

async fn patch_source(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
    Json(payload): Json<SourcePatchIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let out = resource_search::patch_source(
        &state.db,
        &key,
        SourcePatch {
            enabled: payload.enabled,
            server: payload.server,
            username: payload.username,
            password: payload.password,
            token: payload.token,
        },
    )
    .await
    .map_err(|e| AppError::bad_request("RESOURCE_SEARCH", e.to_string()))?;
    Ok(Json(out))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/sources", get(list_sources))
        .route("/sources/{key}", patch(patch_source))
}
