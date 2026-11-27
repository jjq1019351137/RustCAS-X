use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::AppResult;
use crate::settings_store::{get_or_default, put_typed};
use crate::state::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SaveRuleConfig {
    #[serde(default)]
    enable_skip_transferred_history: bool,
}

#[derive(Deserialize)]
struct SaveRuleUpdate {
    enable_skip_transferred_history: Option<bool>,
}

async fn get_save_rules(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<SaveRuleConfig>> {
    require_perm(&user, "task:read")?;
    let cfg: SaveRuleConfig = get_or_default(&state.db, "save_rules").await?;
    Ok(Json(cfg))
}

async fn patch_save_rules(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<SaveRuleUpdate>,
) -> AppResult<Json<SaveRuleConfig>> {
    require_perm(&user, "task:write")?;
    let mut cfg: SaveRuleConfig = get_or_default(&state.db, "save_rules").await?;
    if let Some(v) = payload.enable_skip_transferred_history {
        cfg.enable_skip_transferred_history = v;
    }
    put_typed(&state.db, "save_rules", &cfg).await?;
    Ok(Json(cfg))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/save-rules", get(get_save_rules).patch(patch_save_rules))
}
