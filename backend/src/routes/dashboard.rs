use crate::auth_ctx::CurrentUser;
use crate::error::AppResult;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct DramaQuery {
    #[serde(default = "default_days")]
    days: i64,
}

fn default_days() -> i64 {
    30
}

async fn capacity_overview(_user: CurrentUser) -> AppResult<Json<Value>> {
    Ok(Json(json!({
        "summary": {
            "account_count": 0,
            "enabled_account_count": 0,
            "capacity_account_count": 0,
            "warning_account_count": 0,
            "total_used_space": null,
            "total_space": null,
            "usage_ratio": null
        },
        "accounts": [],
        "warning_accounts": [],
        "unsupported_accounts": [],
        "updated_at": Utc::now().to_rfc3339()
    })))
}

async fn drama_overview(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(q): Query<DramaQuery>,
) -> AppResult<Json<Value>> {
    let days = q.days.clamp(1, 365);
    let task_count: i64 = sqlx::query_scalar("SELECT COUNT(1) FROM tasks")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);
    let enabled_task_count: i64 =
        sqlx::query_scalar("SELECT COUNT(1) FROM tasks WHERE enabled = 1")
            .fetch_one(&state.db)
            .await
            .unwrap_or(0);
    let tmdb_bound_count: i64 =
        sqlx::query_scalar("SELECT COUNT(1) FROM tasks WHERE tmdb_id IS NOT NULL AND tmdb_id > 0")
            .fetch_one(&state.db)
            .await
            .unwrap_or(0);

    Ok(Json(json!({
        "scheduler": {
            "enabled": false,
            "crontab": "0 */2 * * *",
            "timezone": "Asia/Shanghai"
        },
        "summary": {
            "task_count": task_count,
            "enabled_task_count": enabled_task_count,
            "tmdb_bound_count": tmdb_bound_count,
            "banned_count": 0,
            "unknown_schedule_count": 0,
            "monthly_success_count": 0,
            "window_days": days,
            "execution_total": 0,
            "execution_success": 0,
            "execution_failed": 0,
            "execution_skipped": 0,
            "success_rate": null,
            "avg_duration_s": null
        },
        "trend": [],
        "recent_failures": [],
        "updated_at": Utc::now().to_rfc3339()
    })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/capacity-overview", get(capacity_overview))
        .route("/drama-overview", get(drama_overview))
}
