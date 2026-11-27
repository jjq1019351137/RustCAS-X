use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::{AppError, AppResult};
use crate::services::proxy_image::{self, ProxyImageConfig};
use crate::services::share_cache;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize, Default)]
struct ListQuery {
    page: Option<i64>,
    page_size: Option<i64>,
    q: Option<String>,
    drive_type: Option<String>,
    ok: Option<bool>,
    expired_only: Option<bool>,
}

#[derive(Deserialize, Default)]
struct ItemQuery {
    shareurl: Option<String>,
}

#[derive(Deserialize, Default)]
struct PurgeIn {
    expired_only: Option<bool>,
    retention_seconds: Option<i64>,
}

#[derive(Deserialize, Default)]
struct ClearInvalidIn {
    drive_type: Option<String>,
}

async fn preview_list(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let out = share_cache::list_preview_cache(
        &state.db,
        q.page.unwrap_or(1),
        q.page_size.unwrap_or(20),
        q.q.as_deref(),
        q.drive_type.as_deref(),
        q.ok,
        q.expired_only.unwrap_or(false),
    )
    .await
    .map_err(|e| AppError::bad_request("CACHE_LIST", e.to_string()))?;
    Ok(Json(out))
}

async fn preview_delete(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<ItemQuery>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let url = q.shareurl.unwrap_or_default();
    let n = share_cache::delete_preview_cache(&state.db, &url)
        .await
        .map_err(|e| AppError::bad_request("CACHE_DELETE", e.to_string()))?;
    Ok(Json(json!({ "deleted": n })))
}

async fn preview_purge(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<PurgeIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let n = share_cache::purge_preview_cache(
        &state.db,
        payload.expired_only.unwrap_or(true),
        payload.retention_seconds.unwrap_or(7 * 24 * 3600),
    )
    .await
    .map_err(|e| AppError::bad_request("CACHE_PURGE", e.to_string()))?;
    Ok(Json(json!({ "deleted": n })))
}

async fn preview_clear_memory(user: CurrentUser) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    // No in-memory cache layer yet.
    Ok(Json(json!({ "cleared": true })))
}

async fn invalid_list(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let out = share_cache::list_invalid(
        &state.db,
        q.page.unwrap_or(1),
        q.page_size.unwrap_or(20),
        q.q.as_deref(),
        q.drive_type.as_deref(),
    )
    .await
    .map_err(|e| AppError::bad_request("INVALID_LIST", e.to_string()))?;
    Ok(Json(out))
}

async fn invalid_delete(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<ItemQuery>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let n = share_cache::delete_invalid(&state.db, &q.shareurl.unwrap_or_default())
        .await
        .map_err(|e| AppError::bad_request("INVALID_DELETE", e.to_string()))?;
    Ok(Json(json!({ "deleted": n })))
}

async fn invalid_clear(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<ClearInvalidIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let n = share_cache::clear_invalid(&state.db, payload.drive_type.as_deref())
        .await
        .map_err(|e| AppError::bad_request("INVALID_CLEAR", e.to_string()))?;
    Ok(Json(json!({ "deleted": n })))
}

async fn proxy_image_stats(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let cfg = ProxyImageConfig::from_data_dir(&state.settings.data_dir);
    Ok(Json(proxy_image::stats(&cfg)))
}

async fn proxy_image_purge(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let cfg = ProxyImageConfig::from_data_dir(&state.settings.data_dir);
    let (deleted_files, deleted_bytes) = proxy_image::purge_expired(&cfg);
    Ok(Json(json!({ "deleted_files": deleted_files, "deleted_bytes": deleted_bytes })))
}

async fn proxy_image_clear(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let cfg = ProxyImageConfig::from_data_dir(&state.settings.data_dir);
    let (deleted_files, deleted_bytes) = proxy_image::clear_all(&cfg);
    Ok(Json(json!({ "deleted_files": deleted_files, "deleted_bytes": deleted_bytes })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/share-preview-batch/list", get(preview_list))
        .route("/share-preview-batch/item", delete(preview_delete))
        .route("/share-preview-batch/purge", post(preview_purge))
        .route("/share-preview-batch/clear-memory", post(preview_clear_memory))
        .route("/invalid-share-links/list", get(invalid_list))
        .route("/invalid-share-links/item", delete(invalid_delete))
        .route("/invalid-share-links/clear", post(invalid_clear))
        .route("/proxy-image/stats", get(proxy_image_stats))
        .route("/proxy-image/purge", post(proxy_image_purge))
        .route("/proxy-image/clear", post(proxy_image_clear))
}
