use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::{AppError, AppResult};
use crate::runtime::drama;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Datelike, Utc};
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
    let addition: Value = serde_json::from_str(&addition_json).unwrap_or(json!({}));
    let extra: Value = serde_json::from_str(&extra_json).unwrap_or(json!({}));
    let episode_weekdays = weekdays_from(&addition, "tmdb_episode_weekdays")
        .or_else(|| weekdays_from(&extra, "tmdb_episode_weekdays"))
        .unwrap_or_default();
    let update_weekdays = weekdays_from(&addition, "tmdb_update_weekdays")
        .or_else(|| weekdays_from(&extra, "tmdb_update_weekdays"))
        .unwrap_or_default();
    let would = compute_would_run_today(
        enabled != 0,
        tmdb_id,
        tmdb_media_type.as_deref(),
        &extra,
        &episode_weekdays,
        &update_weekdays,
    );
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
        "would_run_today": would,
        "tmdb_episode_weekdays": episode_weekdays,
        "tmdb_update_weekdays": update_weekdays,
        "sync_task_uids": [],
        "shareurls": [],
        "addition": addition,
        "extra": extra,
        "created_at": created_at,
        "updated_at": updated_at
    })
}

fn weekdays_from(obj: &Value, key: &str) -> Option<Vec<i64>> {
    let arr = obj.get(key)?.as_array()?;
    Some(
        arr.iter()
            .filter_map(|v| v.as_i64().or_else(|| v.as_u64().map(|u| u as i64)))
            .collect(),
    )
}

fn normalize_weekdays(v: &Value) -> Vec<i64> {
    match v {
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_i64().or_else(|| x.as_u64().map(|u| u as i64)))
            .filter(|d| (1..=7).contains(d))
            .collect(),
        Value::String(s) => s
            .split([',', ' ', '|'])
            .filter_map(|p| p.trim().parse::<i64>().ok())
            .filter(|d| (1..=7).contains(d))
            .collect(),
        _ => vec![],
    }
}

/// Align with Python: iso weekday Mon=1 … Sun=7.
fn compute_would_run_today(
    enabled: bool,
    tmdb_id: Option<i64>,
    media_type: Option<&str>,
    extra: &Value,
    episode_weekdays: &[i64],
    update_weekdays: &[i64],
) -> bool {
    if !enabled {
        return false;
    }
    if extra.get("ended").and_then(|v| v.as_bool()).unwrap_or(false) {
        return false;
    }
    let today = chrono::Local::now().date_naive().weekday().number_from_monday() as i64;
    let mode = extra
        .get("runweek_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("manual")
        .to_lowercase();
    if mode == "auto" {
        let tid = tmdb_id.unwrap_or(0);
        let mt = media_type.unwrap_or("").to_lowercase();
        if tid <= 0 || mt != "tv" {
            return false;
        }
        let week: std::collections::HashSet<i64> = if !episode_weekdays.is_empty() {
            episode_weekdays.iter().copied().collect()
        } else {
            update_weekdays.iter().copied().collect()
        };
        if week.is_empty() {
            return false;
        }
        return week.contains(&today);
    }
    let runweek = normalize_weekdays(extra.get("runweek").unwrap_or(&Value::Null));
    if runweek.is_empty() {
        return true;
    }
    runweek.contains(&today)
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
