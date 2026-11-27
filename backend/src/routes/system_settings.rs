use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::AppResult;
use crate::runtime::scheduler::{self, SchedulerConfig};
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

#[derive(Deserialize)]
struct SchedulerUpdate {
    enabled: Option<bool>,
    interval_minutes: Option<u64>,
    timezone: Option<String>,
    hours: Option<Vec<u32>>,
}

async fn get_scheduler(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<SchedulerConfig>> {
    require_perm(&user, "task:read")?;
    Ok(Json(scheduler::load(&state).await))
}

async fn patch_scheduler(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<SchedulerUpdate>,
) -> AppResult<Json<SchedulerConfig>> {
    require_perm(&user, "task:write")?;
    let mut cfg = scheduler::load(&state).await;
    if let Some(v) = payload.enabled {
        cfg.enabled = v;
    }
    if let Some(v) = payload.interval_minutes {
        cfg.interval_minutes = v.max(1);
    }
    if let Some(v) = payload.timezone {
        cfg.timezone = v;
    }
    if let Some(v) = payload.hours {
        cfg.hours = v;
    }
    scheduler::save(&state, &cfg).await?;
    Ok(Json(cfg))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/save-rules", get(get_save_rules).patch(patch_save_rules))
        .route("/scheduler", get(get_scheduler).patch(patch_scheduler))
}
