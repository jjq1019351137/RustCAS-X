use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::{AppError, AppResult};
use crate::runtime::drama;
use crate::runtime::scheduler::{self, SchedulerConfig};
use crate::services::audit;
use crate::services::magic_rename;
use crate::services::share_links::{self, load_share_links, mark_checked, save_share_links, set_from_urls};
use crate::services::share_preview::{
    browse_drive, preview_share, preview_share_batch, DriveBrowseIn, SharePreviewBatchIn,
    SharePreviewIn,
};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
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
    #[serde(default)]
    shareurls: Option<Vec<String>>,
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
    shareurls: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
struct ShareUrlsCheckIn {
    shareurls: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct ShareUrlActivateIn {
    shareurl: String,
}

#[derive(Deserialize, Default)]
struct TaskSchedulerPatch {
    enabled: Option<bool>,
    crontab: Option<String>,
    timezone: Option<String>,
    openlist_cleanup_enabled: Option<bool>,
    openlist_cleanup_path: Option<String>,
    openlist_cleanup_crontab: Option<String>,
}

#[derive(Deserialize, Default)]
struct TaskImportIn {
    #[serde(default)]
    tasks: Vec<Value>,
    #[serde(default)]
    mode: Option<String>,
}

fn scheduler_to_frontend(cfg: &SchedulerConfig) -> Value {
    let mins = cfg.interval_minutes.max(1);
    json!({
        "enabled": cfg.enabled,
        "crontab": format!("*/{mins} * * * *"),
        "timezone": cfg.timezone,
        "openlist_cleanup_enabled": false,
        "openlist_cleanup_path": "",
        "openlist_cleanup_crontab": "0 4 * * *"
    })
}

fn parse_interval_from_crontab(crontab: &str) -> Option<u64> {
    // Support "*/N * * * *" only for Rust interval scheduler.
    let first = crontab.split_whitespace().next()?;
    if let Some(rest) = first.strip_prefix("*/") {
        return rest.parse().ok();
    }
    None
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
    shareurl_list_json: String,
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
    let shareurls = load_share_links(&r.shareurl_list_json, r.shareurl.as_deref());
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
        "shareurls": shareurls,
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
       addition_json, extra_json,
       COALESCE(shareurl_list_json, '[]') AS shareurl_list_json,
       created_at, updated_at
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

async fn attach_recent_executions(state: &AppState, tasks: &mut [Value]) -> AppResult<()> {
    for task in tasks.iter_mut() {
        let Some(id) = task.get("id").and_then(|v| v.as_i64()) else {
            continue;
        };
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
            LIMIT 5
            "#,
        )
        .bind(id)
        .fetch_all(&state.db)
        .await?;
        task["executions"] = json!(rows
            .into_iter()
            .map(|r| map_execution(r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7))
            .collect::<Vec<_>>());
    }
    Ok(())
}

async fn list_tasks(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let rows = sqlx::query_as::<_, TaskRow>(&format!(
        "{TASK_SELECT} ORDER BY sort_index ASC, id DESC"
    ))
    .fetch_all(&state.db)
    .await?;
    let mut tasks: Vec<Value> = rows.into_iter().map(map_task_full).collect();
    attach_recent_executions(&state, &mut tasks).await?;
    Ok(Json(json!(tasks)))
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
    let active = payload.shareurl.clone().unwrap_or_default();
    let share_list = match &payload.shareurls {
        Some(urls) => set_from_urls(urls, &active),
        None => set_from_urls(&[], &active),
    };
    let id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO tasks
            (task_uid, task_type, taskname, shareurl, savepath, account_name,
             pattern, "replace", startfid, update_subdir, enddate, shareurl_ban,
             tmdb_id, tmdb_media_type, ignore_extension, enabled,
             addition_json, extra_json, shareurl_list_json, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
    .bind(&share_list)
    .bind(&now)
    .bind(&now)
    .fetch_one(&state.db)
    .await?;

    audit::write_user(
        &state.db,
        user.id,
        "task.create",
        "task",
        id,
        Some(&payload.taskname),
    )
    .await;
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
    let active = shareurl.clone().unwrap_or_default();
    let share_list = if let Some(urls) = payload.shareurls {
        set_from_urls(&urls, &active)
    } else {
        let existing_links = mapped
            .get("shareurls")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        save_share_links(&existing_links, &active)
    };

    sqlx::query(
        r#"
        UPDATE tasks SET
            taskname=?, shareurl=?, savepath=?, account_name=?,
            pattern=?, "replace"=?, startfid=?, update_subdir=?,
            enddate=?, shareurl_ban=?,
            tmdb_id=?, tmdb_media_type=?, ignore_extension=?, enabled=?,
            addition_json=?, extra_json=?, shareurl_list_json=?, updated_at=?
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
    .bind(&share_list)
    .bind(&now)
    .bind(task_id)
    .execute(&state.db)
    .await?;

    audit::write_user(&state.db, user.id, "task.update", "task", task_id, None).await;
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
    audit::write_user(&state.db, user.id, "task.delete", "task", task_id, None).await;
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

async fn get_magic_regex(user: CurrentUser) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    Ok(Json(json!({ "rules": magic_rename::list_builtin_rules() })))
}

async fn get_task_scheduler(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let cfg = scheduler::load(&state).await;
    Ok(Json(scheduler_to_frontend(&cfg)))
}

async fn patch_task_scheduler(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<TaskSchedulerPatch>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let mut cfg = scheduler::load(&state).await;
    if let Some(v) = payload.enabled {
        cfg.enabled = v;
    }
    if let Some(tz) = payload.timezone.filter(|s| !s.trim().is_empty()) {
        cfg.timezone = tz;
    }
    if let Some(crontab) = payload.crontab {
        if let Some(mins) = parse_interval_from_crontab(&crontab) {
            cfg.interval_minutes = mins.max(1);
        }
    }
    let _ = (
        payload.openlist_cleanup_enabled,
        payload.openlist_cleanup_path,
        payload.openlist_cleanup_crontab,
    );
    scheduler::save(&state, &cfg)
        .await
        .map_err(|e| AppError::bad_request("SCHEDULER_SAVE", e.to_string()))?;
    Ok(Json(scheduler_to_frontend(&cfg)))
}

async fn export_tasks(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let rows = sqlx::query_as::<_, TaskRow>(&format!(
        "{TASK_SELECT} ORDER BY sort_index ASC, id ASC"
    ))
    .fetch_all(&state.db)
    .await?;
    let tasks: Vec<Value> = rows.into_iter().map(map_task_full).collect();
    Ok(Json(json!({
        "version": 1,
        "exported_at": Utc::now().to_rfc3339(),
        "tasks": tasks
    })))
}

async fn import_tasks(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<TaskImportIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let mode = payload.mode.unwrap_or_else(|| "skip".into());
    let mut created = 0usize;
    let mut updated = 0usize;
    let mut skipped = 0usize;
    let mut failed: Vec<Value> = Vec::new();
    for item in payload.tasks {
        let taskname = item
            .get("taskname")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if taskname.is_empty() {
            failed.push(json!({ "taskname": taskname, "error": "empty taskname" }));
            continue;
        }
        let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM tasks WHERE taskname = ? LIMIT 1")
            .bind(&taskname)
            .fetch_optional(&state.db)
            .await?;
        if let Some(id) = existing {
            if mode == "skip" {
                skipped += 1;
                continue;
            }
            let patch = TaskUpdateIn {
                taskname: Some(taskname.clone()),
                shareurl: item.get("shareurl").and_then(|v| v.as_str()).map(|s| s.to_string()),
                savepath: item.get("savepath").and_then(|v| v.as_str()).map(|s| s.to_string()),
                account_name: item
                    .get("account_name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                pattern: item.get("pattern").and_then(|v| v.as_str()).map(|s| s.to_string()),
                replace_rule: item
                    .get("replace")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                startfid: item.get("startfid").and_then(|v| v.as_str()).map(|s| s.to_string()),
                update_subdir: item
                    .get("update_subdir")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                enddate: item.get("enddate").and_then(|v| v.as_str()).map(|s| s.to_string()),
                shareurl_ban: item
                    .get("shareurl_ban")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                tmdb_id: item.get("tmdb_id").and_then(|v| v.as_i64()),
                tmdb_media_type: item
                    .get("tmdb_media_type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                ignore_extension: item.get("ignore_extension").and_then(|v| v.as_bool()),
                enabled: item.get("enabled").and_then(|v| v.as_bool()),
                extra: item.get("extra").cloned(),
                addition: item.get("addition").cloned(),
                shareurls: item.get("shareurls").and_then(|v| {
                    v.as_array().map(|arr| {
                        arr.iter()
                            .filter_map(|x| {
                                x.get("url")
                                    .and_then(|u| u.as_str())
                                    .or_else(|| x.as_str())
                                    .map(|s| s.to_string())
                            })
                            .collect()
                    })
                }),
            };
            match patch_task(
                State(state.clone()),
                user.clone(),
                Path(id),
                Json(patch),
            )
            .await
            {
                Ok(_) => updated += 1,
                Err(e) => failed.push(json!({ "taskname": taskname, "error": format!("{e:?}") })),
            }
        } else {
            let create = TaskCreateIn {
                taskname: taskname.clone(),
                task_type: item
                    .get("task_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("drama")
                    .to_string(),
                shareurl: item.get("shareurl").and_then(|v| v.as_str()).map(|s| s.to_string()),
                savepath: item.get("savepath").and_then(|v| v.as_str()).map(|s| s.to_string()),
                account_name: item
                    .get("account_name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                pattern: item.get("pattern").and_then(|v| v.as_str()).map(|s| s.to_string()),
                replace_rule: item
                    .get("replace")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                startfid: item.get("startfid").and_then(|v| v.as_str()).map(|s| s.to_string()),
                update_subdir: item
                    .get("update_subdir")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                enddate: item.get("enddate").and_then(|v| v.as_str()).map(|s| s.to_string()),
                shareurl_ban: item
                    .get("shareurl_ban")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                tmdb_id: item.get("tmdb_id").and_then(|v| v.as_i64()),
                tmdb_media_type: item
                    .get("tmdb_media_type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                ignore_extension: item
                    .get("ignore_extension")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                enabled: item.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
                extra: item.get("extra").cloned().unwrap_or(json!({})),
                addition: item.get("addition").cloned().unwrap_or(json!({})),
                shareurls: item.get("shareurls").and_then(|v| {
                    v.as_array().map(|arr| {
                        arr.iter()
                            .filter_map(|x| {
                                x.get("url")
                                    .and_then(|u| u.as_str())
                                    .or_else(|| x.as_str())
                                    .map(|s| s.to_string())
                            })
                            .collect()
                    })
                }),
            };
            match create_task(State(state.clone()), user.clone(), Json(create)).await {
                Ok(_) => created += 1,
                Err(e) => failed.push(json!({ "taskname": taskname, "error": format!("{e:?}") })),
            }
        }
    }
    Ok(Json(json!({
        "ok": true,
        "created": created,
        "updated": updated,
        "skipped": skipped,
        "failed": failed,
        "total": created + updated + skipped + failed.len()
    })))
}

async fn run_all_tasks(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:run")?;
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM tasks WHERE enabled = 1 AND task_type = 'drama' ORDER BY sort_index ASC, id ASC LIMIT 100",
    )
    .fetch_all(&state.db)
    .await?;
    let mut results = Vec::new();
    for id in ids {
        match drama::run_once(&state, id).await {
            Ok((exec_id, log)) => results.push(json!({
                "task_id": id,
                "execution_id": exec_id,
                "ok": true,
                "log_tail": log.chars().rev().take(200).collect::<String>().chars().rev().collect::<String>()
            })),
            Err(e) => results.push(json!({
                "task_id": id,
                "ok": false,
                "error": e.to_string()
            })),
        }
    }
    Ok(Json(json!({ "ok": true, "results": results })))
}

async fn run_all_tasks_stream(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    require_perm(&user, "task:run")?;
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM tasks WHERE enabled = 1 AND task_type = 'drama' ORDER BY sort_index ASC, id ASC LIMIT 100",
    )
    .fetch_all(&state.db)
    .await?;
    let mut events = vec![Event::default()
        .event("log")
        .data(format!("run-all count={}", ids.len()))];
    for id in ids {
        match drama::run_once(&state, id).await {
            Ok((exec_id, log)) => {
                events.push(
                    Event::default()
                        .event("log")
                        .data(format!("task={id} exec={exec_id}")),
                );
                for line in log.lines().take(40) {
                    events.push(Event::default().event("log").data(line.to_string()));
                }
            }
            Err(e) => {
                events.push(
                    Event::default()
                        .event("log")
                        .data(format!("task={id} error={e}")),
                );
            }
        }
    }
    events.push(Event::default().event("done").data("ok"));
    Ok(Sse::new(stream::iter(events.into_iter().map(Ok))).keep_alive(KeepAlive::default()))
}

#[derive(Deserialize)]
struct TaskStatusIn {
    enabled: bool,
}

async fn patch_task_status(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_id): Path<i64>,
    Json(payload): Json<TaskStatusIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let n = sqlx::query("UPDATE tasks SET enabled=?, updated_at=? WHERE id=?")
        .bind(if payload.enabled { 1 } else { 0 })
        .bind(Utc::now().to_rfc3339())
        .bind(task_id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::bad_request("TASK_NOT_FOUND", "任务不存在"));
    }
    audit::write_user(
        &state.db,
        user.id,
        "task.status",
        "task",
        task_id,
        Some(&format!("enabled={}", payload.enabled)),
    )
    .await;
    get_task(State(state), user, Path(task_id)).await
}

async fn savepath_snapshots_sync_stub(user: CurrentUser) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    Ok(Json(json!({
        "ok": true,
        "synced": 0,
        "note": "savepath snapshots not required for Quark-first wave"
    })))
}

async fn repair_banned_tasks(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let mut out = crate::services::drama_repair::repair_banned_tasks(&state)
        .await
        .map_err(|e| AppError::bad_request("REPAIR_BANNED", e.to_string()))?;

    let repaired_ids: Vec<i64> = out
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|it| it.get("task_id").and_then(|v| v.as_i64()))
        .collect();

    let mut reran = 0usize;
    let mut rerun_items = Vec::new();
    for task_id in repaired_ids {
        match drama::run_once(&state, task_id).await {
            Ok((exec_id, _)) => {
                reran += 1;
                rerun_items.push(json!({
                    "task_id": task_id,
                    "exec_id": exec_id,
                    "ok": true
                }));
            }
            Err(e) => {
                tracing::warn!(task_id, error = %e, "repair-banned re-run failed");
                rerun_items.push(json!({
                    "task_id": task_id,
                    "ok": false,
                    "error": e.to_string()
                }));
            }
        }
    }
    if let Some(obj) = out.as_object_mut() {
        obj.insert("reran".into(), json!(reran));
        obj.insert("rerun_items".into(), json!(rerun_items));
    }

    audit::write_user(
        &state.db,
        user.id,
        "task.repair_banned",
        "task",
        "*",
        Some(&out.to_string()),
    )
    .await;

    Ok(Json(out))
}

async fn stop_completed_drama_tasks(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let rows = sqlx::query_as::<
        _,
        (i64, Option<i64>, Option<String>, String, Option<String>),
    >(
        r#"
        SELECT id, tmdb_id, tmdb_media_type, extra_json, enddate
        FROM tasks
        WHERE enabled = 1 AND task_type = 'drama'
        ORDER BY id DESC
        "#,
    )
    .fetch_all(&state.db)
    .await?;
    let checked = rows.len();
    let today = chrono::Local::now().date_naive();
    let tmdb_cfg = crate::services::tmdb::load_config(&state)
        .await
        .unwrap_or_default();
    let client = crate::services::tmdb::TmdbClient::new(&tmdb_cfg);
    let mut matched_ids = Vec::new();

    for (id, tmdb_id, media_type, extra_json, enddate) in rows {
        let extra: Value = serde_json::from_str(&extra_json).unwrap_or(json!({}));
        if extra.get("ended").and_then(|v| v.as_bool()).unwrap_or(false) {
            matched_ids.push(id);
            continue;
        }
        if let Some(ed) = enddate.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            if let Ok(end_d) =
                chrono::NaiveDate::parse_from_str(&ed[..ed.len().min(10)], "%Y-%m-%d")
            {
                if today > end_d {
                    matched_ids.push(id);
                    continue;
                }
            }
        }
        if media_type.as_deref() == Some("tv") {
            if let Some(tid) = tmdb_id.filter(|x| *x > 0) {
                if client.is_configured() {
                    if let Ok(Some(detail)) = client.get_tv_details(tid).await {
                        let status = detail
                            .get("status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if status == "Ended" || status == "Canceled" {
                            matched_ids.push(id);
                        }
                    }
                }
            }
        }
    }

    let mut stopped = 0usize;
    let now = Utc::now().to_rfc3339();
    for id in &matched_ids {
        let extra_raw: String = sqlx::query_scalar(
            "SELECT COALESCE(extra_json, '{}') FROM tasks WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&state.db)
        .await
        .unwrap_or_else(|_| "{}".into());
        let mut extra: Value = serde_json::from_str(&extra_raw).unwrap_or(json!({}));
        if let Some(obj) = extra.as_object_mut() {
            obj.insert("ended".into(), json!(true));
        }
        let n = sqlx::query(
            "UPDATE tasks SET enabled=0, extra_json=?, updated_at=? WHERE id=? AND enabled=1",
        )
        .bind(extra.to_string())
        .bind(&now)
        .bind(id)
        .execute(&state.db)
        .await?
        .rows_affected();
        stopped += n as usize;
    }

    Ok(Json(json!({
        "checked": checked,
        "matched": matched_ids.len(),
        "stopped": stopped,
        "task_ids": matched_ids.into_iter().take(50).collect::<Vec<_>>()
    })))
}

#[derive(Deserialize, Default)]
struct SuggestionsQuery {
    q: Option<String>,
    d: Option<i64>,
    drive_type: Option<String>,
    drive_types: Option<String>,
    sources: Option<String>,
}

#[derive(Deserialize, Default)]
struct SuggestionsVerifyIn {
    shareurls: Option<Vec<String>>,
    account_name: Option<String>,
}

async fn task_suggestions(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<SuggestionsQuery>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let keyword = q.q.unwrap_or_default();
    let deep = q.d.unwrap_or(0);
    let drive_type = q
        .drive_type
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            q.drive_types
                .as_deref()
                .and_then(|s| s.split(',').map(|x| x.trim()).find(|x| !x.is_empty()))
                .map(|s| s.to_string())
        });
    let sources: Option<Vec<String>> = q.sources.map(|s| {
        s.split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect()
    });
    let out = crate::services::resource_search::fetch_suggestions(
        &state.db,
        &keyword,
        deep,
        drive_type.as_deref(),
        sources.as_deref(),
    )
    .await
    .map_err(|e| AppError::bad_request("SUGGESTIONS", e.to_string()))?;
    Ok(Json(out))
}

async fn task_suggestions_verify(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<SuggestionsVerifyIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let out = crate::services::resource_search::verify_suggestions(
        &state.db,
        payload.shareurls.unwrap_or_default(),
        payload.account_name,
    )
    .await
    .map_err(|e| AppError::bad_request("SUGGESTIONS_VERIFY", e.to_string()))?;
    Ok(Json(out))
}

async fn post_share_preview(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<SharePreviewIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let out = preview_share(&state, payload)
        .await
        .map_err(|e| AppError::bad_request("SHARE_PREVIEW", e.to_string()))?;
    Ok(Json(out))
}

async fn post_share_preview_batch(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<SharePreviewBatchIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let out = preview_share_batch(&state.db, payload)
        .await
        .map_err(|e| AppError::bad_request("SHARE_PREVIEW_BATCH", e.to_string()))?;
    Ok(Json(out))
}

async fn post_drive_browse(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<DriveBrowseIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let out = browse_drive(&state.db, payload)
        .await
        .map_err(|e| AppError::bad_request("DRIVE_BROWSE", e.to_string()))?;
    Ok(Json(out))
}

async fn post_drive_mkdir(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<DriveBrowseIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let drive_type = payload
        .shareurl
        .as_deref()
        .and_then(crate::services::drive_util::detect_drive_type)
        .unwrap_or("quark");
    if drive_type != "quark" {
        return Err(AppError::bad_request(
            "DRIVE_MKDIR",
            "RustCAS-X 当前仅支持夸克 mkdir",
        ));
    }
    let (account_name, adapter) =
        crate::services::drive_util::load_adapter(&state.db, drive_type, payload.account_name.as_deref())
            .await
            .map_err(|e| AppError::bad_request("DRIVE_ACCOUNT_NOT_FOUND", e.to_string()))?;
    let dir_path = payload.dir_path.trim();
    let resp = adapter
        .mkdir(dir_path)
        .await
        .map_err(|e| AppError::bad_request("DRIVE_MKDIR", e.to_string()))?;
    Ok(Json(json!({
        "account_name": account_name,
        "dir_path": dir_path,
        "response": resp
    })))
}

async fn check_task_shareurls(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_id): Path<i64>,
    Json(payload): Json<ShareUrlsCheckIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let row = sqlx::query_as::<_, TaskRow>(&format!("{TASK_SELECT} WHERE id = ?"))
        .bind(task_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::bad_request("TASK_NOT_FOUND", "任务不存在"))?;
    let mut links = load_share_links(&row.shareurl_list_json, row.shareurl.as_deref());
    let urls: Vec<String> = if let Some(list) = payload.shareurls.filter(|v| !v.is_empty()) {
        list
    } else {
        links
            .iter()
            .filter_map(|x| x.get("url").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .collect()
    };
    let batch = preview_share_batch(
        &state.db,
        SharePreviewBatchIn {
            shareurls: urls.clone(),
            account_name: row.account_name.clone(),
        },
    )
    .await
    .map_err(|e| AppError::bad_request("SHAREURLS_CHECK", e.to_string()))?;
    let batch_items = batch
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out_items = Vec::new();
    for it in &batch_items {
        let url = it.get("shareurl").and_then(|v| v.as_str()).unwrap_or("");
        let ok = it.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
        let message = it.get("message").and_then(|v| v.as_str());
        mark_checked(&mut links, url, ok, message);
        out_items.push(json!({
            "url": url,
            "ok": ok,
            "message": message,
            "status": if ok { "ok" } else { "invalid" },
            "latest_video": it.get("latest_video").cloned().unwrap_or(Value::Null)
        }));
    }
    let active = row.shareurl.clone().unwrap_or_default();
    let saved = save_share_links(&links, &active);
    sqlx::query("UPDATE tasks SET shareurl_list_json=?, updated_at=? WHERE id=?")
        .bind(&saved)
        .bind(Utc::now().to_rfc3339())
        .bind(task_id)
        .execute(&state.db)
        .await?;
    Ok(Json(json!({ "items": out_items })))
}

async fn activate_task_shareurl(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_id): Path<i64>,
    Json(payload): Json<ShareUrlActivateIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let row = sqlx::query_as::<_, TaskRow>(&format!("{TASK_SELECT} WHERE id = ?"))
        .bind(task_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::bad_request("TASK_NOT_FOUND", "任务不存在"))?;
    let url = payload.shareurl.trim().to_string();
    if url.is_empty() {
        return Err(AppError::bad_request("SHAREURL_EMPTY", "分享链接为空"));
    }
    let mut links = load_share_links(&row.shareurl_list_json, row.shareurl.as_deref());
    if !links
        .iter()
        .any(|x| x.get("url").and_then(|v| v.as_str()) == Some(url.as_str()))
    {
        links.push(share_links::new_link(&url, "manual"));
    }
    let saved = save_share_links(&links, &url);
    sqlx::query(
        "UPDATE tasks SET shareurl=?, shareurl_ban=NULL, shareurl_list_json=?, updated_at=? WHERE id=?",
    )
    .bind(&url)
    .bind(&saved)
    .bind(Utc::now().to_rfc3339())
    .bind(task_id)
    .execute(&state.db)
    .await?;
    get_task(State(state), user, Path(task_id)).await
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_tasks).post(create_task))
        .route("/magic-regex", get(get_magic_regex))
        .route(
            "/scheduler",
            get(get_task_scheduler).patch(patch_task_scheduler),
        )
        .route("/export", get(export_tasks))
        .route("/import", post(import_tasks))
        .route("/run-all", post(run_all_tasks))
        .route("/run-all/stream", post(run_all_tasks_stream))
        .route("/repair-banned", post(repair_banned_tasks))
        .route("/drama/stop-completed", post(stop_completed_drama_tasks))
        .route("/drama/savepath-snapshots/sync", post(savepath_snapshots_sync_stub))
        .route("/suggestions", get(task_suggestions))
        .route("/suggestions/verify", post(task_suggestions_verify))
        .route("/share/preview", post(post_share_preview))
        .route("/share/preview-batch", post(post_share_preview_batch))
        .route("/drive/browse", post(post_drive_browse))
        .route("/drive/mkdir", post(post_drive_mkdir))
        .route(
            "/{task_id}",
            get(get_task).patch(patch_task).delete(delete_task),
        )
        .route("/{task_id}/status", axum::routing::patch(patch_task_status))
        .route("/{task_id}/run", post(run_task))
        .route("/{task_id}/run/stream", post(run_task_stream))
        .route("/{task_id}/executions", get(list_executions))
        .route("/{task_id}/executions/latest", get(latest_execution))
        .route("/{task_id}/executions/{exec_id}", get(get_execution))
        .route("/{task_id}/shareurls/check", post(check_task_shareurls))
        .route("/{task_id}/shareurls/activate", post(activate_task_shareurl))
}
