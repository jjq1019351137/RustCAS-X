use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::AppResult;
use crate::state::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Serialize)]
struct SyncTaskOut {
    id: i64,
    sync_task_uid: String,
    name: String,
    enabled: bool,
    mode: String,
    source: Value,
    target: Value,
    strategy: Value,
    is_running: bool,
    created_at: String,
    updated_at: String,
}

async fn list_sync_tasks(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<Vec<SyncTaskOut>>> {
    require_perm(&user, "sync:read")?;

    let rows = sqlx::query_as::<
        _,
        (
            i64,
            String,
            String,
            i64,
            String,
            String,
            String,
            String,
            i64,
            String,
            String,
        ),
    >(
        r#"
        SELECT id, sync_task_uid, name, enabled, mode,
               source_json, target_json, strategy_json, is_running, created_at, updated_at
        FROM sync_tasks
        ORDER BY id DESC
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    let out = rows
        .into_iter()
        .map(
            |(
                id,
                sync_task_uid,
                name,
                enabled,
                mode,
                source_json,
                target_json,
                strategy_json,
                is_running,
                created_at,
                updated_at,
            )| SyncTaskOut {
                id,
                sync_task_uid,
                name,
                enabled: enabled != 0,
                mode,
                source: serde_json::from_str(&source_json).unwrap_or_else(|_| json!({})),
                target: serde_json::from_str(&target_json).unwrap_or_else(|_| json!({})),
                strategy: serde_json::from_str(&strategy_json)
                    .unwrap_or_else(|_| json!({"concurrency": 1})),
                is_running: is_running != 0,
                created_at,
                updated_at,
            },
        )
        .collect();

    Ok(Json(out))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list_sync_tasks))
}
