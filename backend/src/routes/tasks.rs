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
use sqlx::FromRow;
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
    pattern: Option<String>,
    #[serde(rename = "replace")]
    replace_rule: Option<String>,
    startfid: Option<String>,
    update_subdir: Option<String>,
    enddate: Option<String>,
    shareurl_ban: Option<String>,
    tmdb_id: Option<i64>,
    tmdb_media_type: Option<String>,
    #[serde(default)]
    ignore_extension: bool,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    extra: Value,
    #[serde(default)]
    addition: Value,
}

#[derive(Deserialize, Default)]
pub struct TaskUpdateIn {
    taskname: Option<String>,
    shareurl: Option<String>,
    savepath: Option<String>,
    account_name: Option<String>,
    pattern: Option<String>,
    #[serde(rename = "replace")]
    replace_rule: Option<String>,
    startfid: Option<String>,
    update_subdir: Option<String>,
    enddate: Option<String>,
    shareurl_ban: Option<String>,
    tmdb_id: Option<i64>,
    tmdb_media_type: Option<String>,
    ignore_extension: Option<bool>,
    enabled: Option<bool>,
    extra: Option<Value>,
    addition: Option<Value>,
}

fn drama_type() -> String {
    "drama".into()
}
fn default_true() -> bool {
    true
}

#[derive(Debug, FromRow)]
struct TaskRow {
    id: i64,
    task_uid: String,
    task_type: String,
    taskname: String,
    shareurl: Option<String>,
    savepath: Option<String>,
    enabled: i64,
    tmdb_id: Option<i64>,
    tmdb_media_type: Option<String>,
    account_name: Option<String>,
    pattern: Option<String>,
    #[sqlx(rename = "replace")]
    replace_rule: Option<String>,
    startfid: Option<String>,
    update_subdir: Option<String>,
    enddate: Option<String>,
    shareurl_ban: Option<String>,
    ignore_extension: Option<i64>,
    addition_json: String,
    extra_json: String,
    created_at: String,
    updated_at: String,
}

fn map_task_full(r: TaskRow) -> Value {
    let addition: Value = serde_json::from_str(&r.addition_json).unwrap_or(json!({}));
    let extra: Value = serde_json::from_str(&r.extra_json).unwrap_or(json!({}));
    let episode_weekdays = weekdays_from(&addition, "tmdb_episode_weekdays")
        .or_else(|| weekdays_from(&extra, "tmdb_episode_weekdays"))
        .unwrap_or_default();
    let update_weekdays = weekdays_from(&addition, "tmdb_update_weekdays")
        .or_else(|| weekdays_from(&extra, "tmdb_update_weekdays"))
        .unwrap_or_default();
    let would = compute_would_run_today(
        r.enabled != 0,
        r.tmdb_id,
        r.tmdb_media_type.as_deref(),
        &extra,
        &episode_weekdays,
        &update_weekdays,
        r.enddate.as_deref(),
        r.shareurl_ban.as_deref(),
    );
    json!({
        "id": r.id,
        "task_uid": r.task_uid,
        "task_type": r.task_type,
        "taskname": r.taskname,
        "shareurl": r.shareurl,
        "savepath": r.savepath,
        "account_name": r.account_name,
        "pattern": r.pattern.unwrap_or_default(),
        "replace": r.replace_rule.unwrap_or_default(),
        "startfid": r.startfid.unwrap_or_default(),
        "update_subdir": r.update_subdir.unwrap_or_default(),
        "enddate": r.enddate.clone(),
        "shareurl_ban": r.shareurl_ban.clone(),
        "ignore_extension": r.ignore_extension.unwrap_or(0) != 0,
        "enabled": r.enabled != 0,
        "tmdb_id": r.tmdb_id,
        "tmdb_media_type": r.tmdb_media_type,
        "would_run_today": would,
        "tmdb_episode_weekdays": episode_weekdays,
        "tmdb_update_weekdays": update_weekdays,
        "sync_task_uids": [],
        "shareurls": [],
        "addition": addition,
        "extra": extra,
        "created_at": r.created_at,
        "updated_at": r.updated_at
    })
}

const TASK_SELECT: &str = r#"
SELECT id, task_uid, task_type, taskname, shareurl, savepath, enabled,
       tmdb_id, tmdb_media_type, account_name, pattern, "replace", startfid,
       update_subdir, enddate, shareurl_ban, ignore_extension,
       addition_json, extra_json, created_at, updated_at
FROM tasks
"#;

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

fn compute_would_run_today(
    enabled: bool,
    tmdb_id: Option<i64>,
    media_type: Option<&str>,
    extra: &Value,
    episode_weekdays: &[i64],
    update_weekdays: &[i64],
    enddate: Option<&str>,
    shareurl_ban: Option<&str>,
) -> bool {
    if !enabled {
        return false;
    }
    if extra.get("ended").and_then(|v| v.as_bool()).unwrap_or(false) {
        return false;
    }
    if shareurl_ban.map(|s| !s.trim().is_empty()).unwrap_or(false) {
        return false;
    }
    if let Some(ed) = enddate.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if let Ok(end_d) = chrono::NaiveDate::parse_from_str(&ed[..ed.len().min(10)], "%Y-%m-%d") {
            if chrono::Local::now().date_naive() > end_d {
                return false;
            }
        }
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
    let rows = sqlx::query_as::<_, TaskRow>(&format!(
        "{TASK_SELECT} ORDER BY sort_index ASC, id DESC"
    ))
    .fetch_all(&state.db)
    .await?;
    Ok(Json(json!(rows.into_iter().map(map_task_full).collect::<Vec<_>>())))
}

async fn get_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_id): Path<i64>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let row = sqlx::query_as::<_, TaskRow>(&format!("{TASK_SELECT} WHERE id = ?"))
        .bind(task_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::bad_request("TASK_NOT_FOUND", "任务不存在"))?;
    Ok(Json(map_task_full(row)))
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
            (task_uid, task_type, taskname, shareurl, savepath, account_name,
             pattern, "replace", startfid, update_subdir, enddate, shareurl_ban,
             tmdb_id, tmdb_media_type, ignore_extension, enabled,
             addition_json, extra_json, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        RETURNING id
        "#,
    )
    .bind(&uid)
    .bind(&payload.task_type)
    .bind(&payload.taskname)
    .bind(&payload.shareurl)
    .bind(&payload.savepath)
    .bind(&payload.account_name)
    .bind(payload.pattern.as_deref().unwrap_or(""))
    .bind(payload.replace_rule.as_deref().unwrap_or(""))
    .bind(payload.startfid.as_deref().unwrap_or(""))
    .bind(payload.update_subdir.as_deref().unwrap_or(""))
    .bind(&payload.enddate)
    .bind(&payload.shareurl_ban)
    .bind(payload.tmdb_id)
    .bind(&payload.tmdb_media_type)
    .bind(if payload.ignore_extension { 1 } else { 0 })
    .bind(if payload.enabled { 1 } else { 0 })
    .bind(payload.addition.to_string())
    .bind(payload.extra.to_string())
    .bind(&now)
    .bind(&now)
    .fetch_one(&state.db)
    .await?;

    get_task(State(state), user, Path(id)).await
}

async fn patch_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_id): Path<i64>,
    Json(payload): Json<TaskUpdateIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let existing = sqlx::query_as::<_, TaskRow>(&format!("{TASK_SELECT} WHERE id = ?"))
        .bind(task_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::bad_request("TASK_NOT_FOUND", "任务不存在"))?;

    let mapped = map_task_full(existing);
    let taskname = payload
        .taskname
        .unwrap_or_else(|| mapped["taskname"].as_str().unwrap_or("").to_string());
    let shareurl = payload
        .shareurl
        .or_else(|| mapped["shareurl"].as_str().map(|s| s.to_string()));
    let savepath = payload
        .savepath
        .or_else(|| mapped["savepath"].as_str().map(|s| s.to_string()));
    let account_name = payload
        .account_name
        .or_else(|| mapped["account_name"].as_str().map(|s| s.to_string()));
    let pattern = payload
        .pattern
        .unwrap_or_else(|| mapped["pattern"].as_str().unwrap_or("").to_string());
    let replace_rule = payload
        .replace_rule
        .unwrap_or_else(|| mapped["replace"].as_str().unwrap_or("").to_string());
    let startfid = payload
        .startfid
        .unwrap_or_else(|| mapped["startfid"].as_str().unwrap_or("").to_string());
    let update_subdir = payload
        .update_subdir
        .unwrap_or_else(|| mapped["update_subdir"].as_str().unwrap_or("").to_string());
    let enddate = payload
        .enddate
        .or_else(|| mapped["enddate"].as_str().map(|s| s.to_string()));
    let shareurl_ban = payload
        .shareurl_ban
        .or_else(|| mapped["shareurl_ban"].as_str().map(|s| s.to_string()));
    let tmdb_id = payload
        .tmdb_id
        .or_else(|| mapped["tmdb_id"].as_i64());
    let tmdb_media_type = payload
        .tmdb_media_type
        .or_else(|| mapped["tmdb_media_type"].as_str().map(|s| s.to_string()));
    let ignore_extension = payload
        .ignore_extension
        .unwrap_or_else(|| mapped["ignore_extension"].as_bool().unwrap_or(false));
    let enabled = payload
        .enabled
        .unwrap_or_else(|| mapped["enabled"].as_bool().unwrap_or(true));
    let addition = payload
        .addition
        .unwrap_or_else(|| mapped["addition"].clone());
    let extra = payload.extra.unwrap_or_else(|| mapped["extra"].clone());
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        r#"
        UPDATE tasks SET
            taskname=?, shareurl=?, savepath=?, account_name=?,
            pattern=?, "replace"=?, startfid=?, update_subdir=?,
            enddate=?, shareurl_ban=?,
            tmdb_id=?, tmdb_media_type=?, ignore_extension=?, enabled=?,
            addition_json=?, extra_json=?, updated_at=?
        WHERE id=?
        "#,
    )
    .bind(&taskname)
    .bind(&shareurl)
    .bind(&savepath)
    .bind(&account_name)
    .bind(&pattern)
    .bind(&replace_rule)
    .bind(&startfid)
    .bind(&update_subdir)
    .bind(&enddate)
    .bind(&shareurl_ban)
    .bind(tmdb_id)
    .bind(&tmdb_media_type)
    .bind(if ignore_extension { 1 } else { 0 })
    .bind(if enabled { 1 } else { 0 })
    .bind(addition.to_string())
    .bind(extra.to_string())
    .bind(&now)
    .bind(task_id)
    .execute(&state.db)
    .await?;

    get_task(State(state), user, Path(task_id)).await
}

async fn delete_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_id): Path<i64>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let n = sqlx::query("DELETE FROM tasks WHERE id = ?")
        .bind(task_id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::bad_request("TASK_NOT_FOUND", "任务不存在"));
    }
    Ok(Json(json!({ "ok": true, "id": task_id })))
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
        "message": "ok",
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

fn map_execution(
    id: i64,
    task_id: i64,
    status: String,
    stage: Option<String>,
    message: Option<String>,
    run_log: String,
    started_at: String,
    finished_at: Option<String>,
) -> Value {
    json!({
        "id": id,
        "task_id": task_id,
        "status": status,
        "stage": stage,
        "message": message,
        "run_log": run_log,
        "started_at": started_at,
        "finished_at": finished_at
    })
}

async fn list_executions(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_id): Path<i64>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let rows = sqlx::query_as::<
        _,
        (
            i64,
            i64,
            String,
            Option<String>,
            Option<String>,
            String,
            String,
            Option<String>,
        ),
    >(
        r#"
        SELECT id, task_id, status, stage, message, run_log, started_at, finished_at
        FROM task_executions
        WHERE task_id = ?
        ORDER BY id DESC
        LIMIT 50
        "#,
    )
    .bind(task_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(json!(rows
        .into_iter()
        .map(|r| map_execution(r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7))
        .collect::<Vec<_>>())))
}

async fn get_execution(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((task_id, exec_id)): Path<(i64, i64)>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let row = sqlx::query_as::<
        _,
        (
            i64,
            i64,
            String,
            Option<String>,
            Option<String>,
            String,
            String,
            Option<String>,
        ),
    >(
        r#"
        SELECT id, task_id, status, stage, message, run_log, started_at, finished_at
        FROM task_executions
        WHERE task_id = ? AND id = ?
        "#,
    )
    .bind(task_id)
    .bind(exec_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::bad_request("EXEC_NOT_FOUND", "执行记录不存在"))?;
    Ok(Json(map_execution(
        row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7,
    )))
}

async fn latest_execution(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_id): Path<i64>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let row = sqlx::query_as::<
        _,
        (
            i64,
            i64,
            String,
            Option<String>,
            Option<String>,
            String,
            String,
            Option<String>,
        ),
    >(
        r#"
        SELECT id, task_id, status, stage, message, run_log, started_at, finished_at
        FROM task_executions
        WHERE task_id = ?
        ORDER BY id DESC
        LIMIT 1
        "#,
    )
    .bind(task_id)
    .fetch_optional(&state.db)
    .await?;
    Ok(Json(match row {
        Some(r) => map_execution(r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7),
        None => Value::Null,
    }))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_tasks).post(create_task))
        .route(
            "/{task_id}",
            get(get_task).patch(patch_task).delete(delete_task),
        )
        .route("/{task_id}/run", post(run_task))
        .route("/{task_id}/run/stream", post(run_task_stream))
        .route("/{task_id}/executions", get(list_executions))
        .route("/{task_id}/executions/latest", get(latest_execution))
        .route("/{task_id}/executions/{exec_id}", get(get_execution))
}
