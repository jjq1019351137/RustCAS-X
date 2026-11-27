use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::{AppError, AppResult};
use crate::runtime::drama;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use futures_util::stream::{self, Stream};
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct TaskCreateIn {
    taskname: String,
    #[serde(default = "drama_type")]
    task_type: String,
    shareurl: Option<String>,
    savepath: Option<String>,
    account_name: Option<String>,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    extra: Value,
    #[serde(default)]
    addition: Value,
}

fn drama_type() -> String {
    "drama".into()
}
fn default_true() -> bool {
    true
}

fn map_task_row(
    id: i64,
    task_uid: String,
    task_type: String,
    taskname: String,
    shareurl: Option<String>,
    savepath: Option<String>,
    enabled: i64,
    tmdb_id: Option<i64>,
    tmdb_media_type: Option<String>,
    addition_json: String,
    extra_json: String,
    created_at: String,
    updated_at: String,
) -> Value {
    json!({
        "id": id,
        "task_uid": task_uid,
        "task_type": task_type,
        "taskname": taskname,
        "shareurl": shareurl,
        "savepath": savepath,
        "enabled": enabled != 0,
        "tmdb_id": tmdb_id,
        "tmdb_media_type": tmdb_media_type,
        "would_run_today": false,
        "tmdb_episode_weekdays": [],
        "tmdb_update_weekdays": [],
        "sync_task_uids": [],
        "shareurls": [],
        "addition": serde_json::from_str::<Value>(&addition_json).unwrap_or(json!({})),
        "extra": serde_json::from_str::<Value>(&extra_json).unwrap_or(json!({})),
        "created_at": created_at,
        "updated_at": updated_at
    })
}

async fn list_tasks(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Value>> {
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

    let out: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            map_task_row(
                r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9, r.10, r.11, r.12,
            )
        })
        .collect();
    Ok(Json(json!(out)))
}

async fn create_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<TaskCreateIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let uid = format!("task_{}", Uuid::new_v4().simple());
    let now = Utc::now().to_rfc3339();
    let id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO tasks
            (task_uid, task_type, taskname, shareurl, savepath, account_name, enabled, addition_json, extra_json, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        RETURNING id
        "#,
    )
    .bind(&uid)
    .bind(&payload.task_type)
    .bind(&payload.taskname)
    .bind(&payload.shareurl)
    .bind(&payload.savepath)
    .bind(&payload.account_name)
    .bind(if payload.enabled { 1 } else { 0 })
    .bind(payload.addition.to_string())
    .bind(payload.extra.to_string())
    .bind(&now)
    .bind(&now)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(map_task_row(
        id,
        uid,
        payload.task_type,
        payload.taskname,
        payload.shareurl,
        payload.savepath,
        if payload.enabled { 1 } else { 0 },
        None,
        None,
        payload.addition.to_string(),
        payload.extra.to_string(),
        now.clone(),
        now,
    )))
}

async fn run_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_id): Path<i64>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:run")?;
    let (exec_id, log) = drama::run_once(&state, task_id)
        .await
        .map_err(|e| AppError::bad_request("TASK_RUN", e.to_string()))?;
    Ok(Json(json!({
        "id": exec_id,
        "task_id": task_id,
        "status": "success",
        "message": "skeleton success",
        "run_log": log
    })))
}

async fn run_task_stream(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_id): Path<i64>,
) -> AppResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    require_perm(&user, "task:run")?;
    let (exec_id, log) = drama::run_once(&state, task_id)
        .await
        .map_err(|e| AppError::bad_request("TASK_RUN", e.to_string()))?;

    let mut events = vec![
        Event::default()
            .event("log")
            .data(format!("execution_id={exec_id}")),
    ];
    for line in log.lines() {
        events.push(Event::default().event("log").data(line.to_string()));
    }
    events.push(Event::default().event("done").data("ok"));

    Ok(Sse::new(stream::iter(events.into_iter().map(Ok))).keep_alive(KeepAlive::default()))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_tasks).post(create_task))
        .route("/{task_id}/run", post(run_task))
        .route("/{task_id}/run/stream", post(run_task_stream))
}
