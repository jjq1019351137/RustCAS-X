use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::{AppError, AppResult};
use crate::runtime::sync as sync_runtime;
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
pub struct SyncCreateIn {
    name: String,
    #[serde(default = "one_way")]
    mode: String,
    #[serde(default)]
    source: Value,
    #[serde(default)]
    target: Value,
    #[serde(default)]
    strategy: Value,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn one_way() -> String {
    "one_way".into()
}
fn default_true() -> bool {
    true
}

fn map_row(
    id: i64,
    sync_task_uid: String,
    name: String,
    enabled: i64,
    mode: String,
    source_json: String,
    target_json: String,
    strategy_json: String,
    is_running: i64,
    created_at: String,
    updated_at: String,
) -> Value {
    json!({
        "id": id,
        "sync_task_uid": sync_task_uid,
        "name": name,
        "enabled": enabled != 0,
        "mode": mode,
        "source": serde_json::from_str::<Value>(&source_json).unwrap_or(json!({})),
        "target": serde_json::from_str::<Value>(&target_json).unwrap_or(json!({})),
        "strategy": serde_json::from_str::<Value>(&strategy_json).unwrap_or(json!({"concurrency": 1})),
        "is_running": is_running != 0,
        "created_at": created_at,
        "updated_at": updated_at
    })
}

async fn list_sync_tasks(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Value>> {
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

    let out: Vec<Value> = rows
        .into_iter()
        .map(|r| map_row(r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9, r.10))
        .collect();
    Ok(Json(json!(out)))
}

async fn create_sync_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<SyncCreateIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "sync:write")?;
    let uid = format!("sync_{}", Uuid::new_v4().simple());
    let now = Utc::now().to_rfc3339();
    let strategy = if payload.strategy.is_null() {
        json!({ "concurrency": 1 })
    } else {
        payload.strategy
    };
    let id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO sync_tasks
            (sync_task_uid, name, enabled, mode, source_json, target_json, strategy_json, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        RETURNING id
        "#,
    )
    .bind(&uid)
    .bind(&payload.name)
    .bind(if payload.enabled { 1 } else { 0 })
    .bind(&payload.mode)
    .bind(payload.source.to_string())
    .bind(payload.target.to_string())
    .bind(strategy.to_string())
    .bind(&now)
    .bind(&now)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(map_row(
        id,
        uid,
        payload.name,
        if payload.enabled { 1 } else { 0 },
        payload.mode,
        payload.source.to_string(),
        payload.target.to_string(),
        strategy.to_string(),
        0,
        now.clone(),
        now,
    )))
}

async fn run_sync(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(sync_task_id): Path<i64>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "sync:run")?;
    let (exec_id, log) = sync_runtime::run_once(&state, sync_task_id)
        .await
        .map_err(|e| AppError::bad_request("SYNC_RUN", e.to_string()))?;
    Ok(Json(json!({
        "id": exec_id,
        "sync_task_id": sync_task_id,
        "status": "success",
        "message": "skeleton success",
        "run_log": log
    })))
}

async fn run_sync_stream(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(sync_task_id): Path<i64>,
) -> AppResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    require_perm(&user, "sync:run")?;
    let (exec_id, log) = sync_runtime::run_once(&state, sync_task_id)
        .await
        .map_err(|e| AppError::bad_request("SYNC_RUN", e.to_string()))?;

    let mut events = vec![Event::default()
        .event("log")
        .data(format!("execution_id={exec_id}"))];
    for line in log.lines() {
        events.push(Event::default().event("log").data(line.to_string()));
    }
    events.push(Event::default().event("done").data("ok"));
    Ok(Sse::new(stream::iter(events.into_iter().map(Ok))).keep_alive(KeepAlive::default()))
}

fn map_sync_execution(
    id: i64,
    sync_task_id: i64,
    status: String,
    message: Option<String>,
    run_log: String,
    started_at: String,
    finished_at: Option<String>,
) -> Value {
    json!({
        "id": id,
        "sync_task_id": sync_task_id,
        "status": status,
        "message": message,
        "run_log": run_log,
        "started_at": started_at,
        "finished_at": finished_at
    })
}

async fn list_executions(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(sync_task_id): Path<i64>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "sync:read")?;
    let rows = sqlx::query_as::<
        _,
        (
            i64,
            i64,
            String,
            Option<String>,
            String,
            String,
            Option<String>,
        ),
    >(
        r#"
        SELECT id, sync_task_id, status, message, run_log, started_at, finished_at
        FROM sync_executions
        WHERE sync_task_id = ?
        ORDER BY id DESC
        LIMIT 50
        "#,
    )
    .bind(sync_task_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(json!(rows
        .into_iter()
        .map(|r| map_sync_execution(r.0, r.1, r.2, r.3, r.4, r.5, r.6))
        .collect::<Vec<_>>())))
}

async fn latest_execution(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(sync_task_id): Path<i64>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "sync:read")?;
    let row = sqlx::query_as::<
        _,
        (
            i64,
            i64,
            String,
            Option<String>,
            String,
            String,
            Option<String>,
        ),
    >(
        r#"
        SELECT id, sync_task_id, status, message, run_log, started_at, finished_at
        FROM sync_executions
        WHERE sync_task_id = ?
        ORDER BY id DESC
        LIMIT 1
        "#,
    )
    .bind(sync_task_id)
    .fetch_optional(&state.db)
    .await?;
    Ok(Json(match row {
        Some(r) => map_sync_execution(r.0, r.1, r.2, r.3, r.4, r.5, r.6),
        None => Value::Null,
    }))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_sync_tasks).post(create_sync_task))
        .route("/{sync_task_id}/run", post(run_sync))
        .route("/{sync_task_id}/run/stream", post(run_sync_stream))
        .route("/{sync_task_id}/executions", get(list_executions))
        .route("/{sync_task_id}/executions/latest", get(latest_execution))
}
