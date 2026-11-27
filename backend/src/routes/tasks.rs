use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::AppResult;
use crate::state::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Serialize)]
struct TaskOut {
    id: i64,
    task_uid: String,
    task_type: String,
    taskname: String,
    shareurl: Option<String>,
    savepath: Option<String>,
    enabled: bool,
    tmdb_id: Option<i64>,
    tmdb_media_type: Option<String>,
    would_run_today: bool,
    tmdb_episode_weekdays: Vec<i64>,
    tmdb_update_weekdays: Vec<i64>,
    sync_task_uids: Vec<String>,
    shareurls: Vec<Value>,
    addition: Value,
    extra: Value,
    created_at: String,
    updated_at: String,
}

async fn list_tasks(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<Vec<TaskOut>>> {
    require_perm(&user, "task:read")?;

    let rows = sqlx::query_as::<
        _,
        (
            i64,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            i64,
            Option<i64>,
            Option<String>,
            String,
            String,
            String,
            String,
        ),
    >(
        r#"
        SELECT id, task_uid, task_type, taskname, shareurl, savepath, enabled,
               tmdb_id, tmdb_media_type, addition_json, extra_json, created_at, updated_at
        FROM tasks
        ORDER BY sort_index ASC, id DESC
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    let out = rows
        .into_iter()
        .map(
            |(
                id,
                task_uid,
                task_type,
                taskname,
                shareurl,
                savepath,
                enabled,
                tmdb_id,
                tmdb_media_type,
                addition_json,
                extra_json,
                created_at,
                updated_at,
            )| {
                let addition: Value =
                    serde_json::from_str(&addition_json).unwrap_or_else(|_| json!({}));
                let extra: Value = serde_json::from_str(&extra_json).unwrap_or_else(|_| json!({}));
                TaskOut {
                    id,
                    task_uid,
                    task_type,
                    taskname,
                    shareurl,
                    savepath,
                    enabled: enabled != 0,
                    tmdb_id,
                    tmdb_media_type,
                    would_run_today: false,
                    tmdb_episode_weekdays: vec![],
                    tmdb_update_weekdays: vec![],
                    sync_task_uids: vec![],
                    shareurls: vec![],
                    addition,
                    extra,
                    created_at,
                    updated_at,
                }
            },
        )
        .collect();

    Ok(Json(out))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list_tasks))
}
