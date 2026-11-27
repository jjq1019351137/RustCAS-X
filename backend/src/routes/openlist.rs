use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::{AppError, AppResult};
use crate::services::audit;
use crate::settings_store::{get_or_default, put_typed};
use crate::state::AppState;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct OpenListConfig {
    #[serde(default)]
    url: String,
    #[serde(default)]
    token: String,
}

#[derive(Deserialize)]
struct OpenListUpdate {
    url: Option<String>,
    token: Option<String>,
}

#[derive(Deserialize)]
struct BrowseIn {
    path: Option<String>,
}

fn to_out(cfg: &OpenListConfig) -> Value {
    json!({
        "url": if cfg.url.trim().is_empty() { Value::Null } else { Value::String(cfg.url.clone()) },
        "has_token": !cfg.token.trim().is_empty()
    })
}

async fn get_config(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let cfg: OpenListConfig = get_or_default(&state.db, "openlist_config").await?;
    Ok(Json(to_out(&cfg)))
}

async fn patch_config(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<OpenListUpdate>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let mut cfg: OpenListConfig = get_or_default(&state.db, "openlist_config").await?;
    if let Some(v) = payload.url {
        cfg.url = v;
    }
    if let Some(v) = payload.token {
        cfg.token = v;
    }
    put_typed(&state.db, "openlist_config", &cfg).await?;
    audit::write_user(
        &state.db,
        user.id,
        "openlist.config.update",
        "openlist",
        "config",
        None,
    )
    .await;
    Ok(Json(to_out(&cfg)))
}

async fn browse(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<BrowseIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "sync:read")?;
    let cfg: OpenListConfig = get_or_default(&state.db, "openlist_config").await?;
    if cfg.url.trim().is_empty() || cfg.token.trim().is_empty() {
        return Err(AppError::bad_request(
            "OPENLIST_NOT_CONFIGURED",
            "请先配置 OpenList URL 与 Token",
        ));
    }
    let path = payload.path.unwrap_or_else(|| "/".into());
    let base = cfg.url.trim_end_matches('/');
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/api/fs/list"))
        .header("Authorization", cfg.token.trim())
        .json(&json!({ "path": path, "page": 1, "per_page": 0, "refresh": false }))
        .send()
        .await
        .map_err(|e| AppError::api(axum::http::StatusCode::BAD_GATEWAY, "OPENLIST_HTTP", e.to_string()))?;

    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
    if !status.is_success() {
        return Err(AppError::api(
            axum::http::StatusCode::BAD_GATEWAY,
            "OPENLIST_HTTP",
            body.get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("OpenList 请求失败")
                .to_string(),
        ));
    }

    let content = body
        .pointer("/data/content")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let items: Vec<Value> = content
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|item| {
            json!({
                "name": item.get("name").cloned().unwrap_or(json!("")),
                "is_dir": item.get("is_dir").and_then(|v| v.as_bool()).unwrap_or(false),
                "size": item.get("size").cloned(),
                "updated_at": item.get("modified").cloned()
            })
        })
        .collect();

    Ok(Json(json!({
        "path": path,
        "items": items
    })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/config", get(get_config).patch(patch_config))
        .route("/browse", post(browse))
}
