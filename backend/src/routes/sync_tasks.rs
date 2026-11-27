use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::{AppError, AppResult};
use crate::runtime::sync as sync_runtime;
use crate::services::audit;
use crate::services::share_preview::{browse_drive, DriveBrowseIn};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use futures_util::stream::{self, Stream};
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::path::PathBuf;
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

#[derive(Deserialize, Default)]
pub struct SyncUpdateIn {
    name: Option<String>,
    mode: Option<String>,
    source: Option<Value>,
    target: Option<Value>,
    strategy: Option<Value>,
    enabled: Option<bool>,
}

#[derive(Deserialize, Default)]
struct SyncImportIn {
    #[serde(default)]
    tasks: Vec<Value>,
    #[serde(default)]
    mode: Option<String>,
}

#[derive(Deserialize, Default)]
struct LocalBrowseIn {
    path: String,
    max_items: Option<usize>,
}

#[derive(Deserialize, Default)]
struct NetdiskBrowseIn {
    dir_path: String,
    account_name: Option<String>,
    max_items: Option<i64>,
}

#[derive(Deserialize, Default)]
struct ExecFilesQuery {
    offset: Option<i64>,
    limit: Option<i64>,
}

#[derive(Deserialize, Default)]
struct CancelIn {
    message: Option<String>,
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
        "uid": sync_task_uid,
        "sync_task_uid": sync_task_uid,
        "name": name,
        "enabled": enabled != 0,
        "mode": mode,
        "source": serde_json::from_str::<Value>(&source_json).unwrap_or(json!({})),
        "target": serde_json::from_str::<Value>(&target_json).unwrap_or(json!({})),
        "strategy": serde_json::from_str::<Value>(&strategy_json).unwrap_or(json!({"concurrency": 1})),
        "is_running": is_running != 0,
        "drama_task_uids": [],
        "created_at": created_at,
        "updated_at": updated_at
    })
}

async fn fetch_one(state: &AppState, id: i64) -> AppResult<Value> {
    let row = sqlx::query_as::<
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
        FROM sync_tasks WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::bad_request("SYNC_NOT_FOUND", "同步任务不存在"))?;
    Ok(map_row(
        row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8, row.9, row.10,
    ))
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

async fn get_sync_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(sync_task_id): Path<i64>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "sync:read")?;
    Ok(Json(fetch_one(&state, sync_task_id).await?))
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

    audit::write_user(
        &state.db,
        user.id,
        "sync.create",
        "sync_task",
        id,
        Some(&payload.name),
    )
    .await;
    Ok(Json(fetch_one(&state, id).await?))
}

async fn patch_sync_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(sync_task_id): Path<i64>,
    Json(payload): Json<SyncUpdateIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "sync:write")?;
    let current = fetch_one(&state, sync_task_id).await?;
    let name = payload
        .name
        .unwrap_or_else(|| current["name"].as_str().unwrap_or("").to_string());
    let mode = payload
        .mode
        .unwrap_or_else(|| current["mode"].as_str().unwrap_or("one_way").to_string());
    let source = payload
        .source
        .unwrap_or_else(|| current["source"].clone());
    let target = payload
        .target
        .unwrap_or_else(|| current["target"].clone());
    let strategy = payload
        .strategy
        .unwrap_or_else(|| current["strategy"].clone());
    let enabled = payload
        .enabled
        .unwrap_or_else(|| current["enabled"].as_bool().unwrap_or(true));
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        UPDATE sync_tasks SET
            name=?, mode=?, source_json=?, target_json=?, strategy_json=?, enabled=?, updated_at=?
        WHERE id=?
        "#,
    )
    .bind(&name)
    .bind(&mode)
    .bind(source.to_string())
    .bind(target.to_string())
    .bind(strategy.to_string())
    .bind(if enabled { 1 } else { 0 })
    .bind(&now)
    .bind(sync_task_id)
    .execute(&state.db)
    .await?;
    audit::write_user(
        &state.db,
        user.id,
        "sync.update",
        "sync_task",
        sync_task_id,
        None,
    )
    .await;
    Ok(Json(fetch_one(&state, sync_task_id).await?))
}

async fn delete_sync_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(sync_task_id): Path<i64>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "sync:write")?;
    let n = sqlx::query("DELETE FROM sync_tasks WHERE id = ?")
        .bind(sync_task_id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::bad_request("SYNC_NOT_FOUND", "同步任务不存在"));
    }
    audit::write_user(
        &state.db,
        user.id,
        "sync.delete",
        "sync_task",
        sync_task_id,
        None,
    )
    .await;
    Ok(Json(json!({ "ok": true, "id": sync_task_id })))
}

async fn export_sync_tasks(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<Value>> {
    require_perm(&user, "sync:read")?;
    let list = list_sync_tasks(State(state), user).await?.0;
    Ok(Json(json!({
        "version": 1,
        "exported_at": Utc::now().to_rfc3339(),
        "tasks": list
    })))
}

async fn import_sync_tasks(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<SyncImportIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "sync:write")?;
    let mode = payload.mode.unwrap_or_else(|| "skip".into());
    let mut created = 0usize;
    let mut updated = 0usize;
    let mut skipped = 0usize;
    let mut failed: Vec<Value> = Vec::new();
    for item in payload.tasks {
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            failed.push(json!({ "name": name, "error": "empty name" }));
            continue;
        }
        let existing: Option<i64> =
            sqlx::query_scalar("SELECT id FROM sync_tasks WHERE name = ? LIMIT 1")
                .bind(&name)
                .fetch_optional(&state.db)
                .await?;
        if let Some(id) = existing {
            if mode == "skip" {
                skipped += 1;
                continue;
            }
            let patch = SyncUpdateIn {
                name: Some(name.clone()),
                mode: item.get("mode").and_then(|v| v.as_str()).map(|s| s.to_string()),
                source: item.get("source").cloned(),
                target: item.get("target").cloned(),
                strategy: item.get("strategy").cloned(),
                enabled: item.get("enabled").and_then(|v| v.as_bool()),
            };
            match patch_sync_task(State(state.clone()), user.clone(), Path(id), Json(patch)).await {
                Ok(_) => updated += 1,
                Err(e) => failed.push(json!({ "name": name, "error": format!("{e:?}") })),
            }
        } else {
            let create = SyncCreateIn {
                name: name.clone(),
                mode: item
                    .get("mode")
                    .and_then(|v| v.as_str())
                    .unwrap_or("one_way")
                    .to_string(),
                source: item.get("source").cloned().unwrap_or(json!({})),
                target: item.get("target").cloned().unwrap_or(json!({})),
                strategy: item.get("strategy").cloned().unwrap_or(json!({"concurrency": 1})),
                enabled: item.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
            };
            match create_sync_task(State(state.clone()), user.clone(), Json(create)).await {
                Ok(_) => created += 1,
                Err(e) => failed.push(json!({ "name": name, "error": format!("{e:?}") })),
            }
        }
    }
    audit::write_user(
        &state.db,
        user.id,
        "sync.import",
        "sync_task",
        "*",
        Some(&format!(
            "created={created} updated={updated} skipped={skipped} failed={}",
            failed.len()
        )),
    )
    .await;
    Ok(Json(json!({
        "ok": true,
        "created": created,
        "updated": updated,
        "skipped": skipped,
        "failed": failed,
        "total": created + updated + skipped + failed.len()
    })))
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
    audit::write_user(
        &state.db,
        user.id,
        "sync.run",
        "sync_task",
        sync_task_id,
        Some(&format!("exec_id={exec_id}")),
    )
    .await;
    Ok(Json(json!({
        "id": exec_id,
        "sync_task_id": sync_task_id,
        "status": "success",
        "message": "ok",
        "run_log": log,
        "stats": {}
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
        "finished_at": finished_at,
        "stats": {},
        "stage": Value::Null
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

async fn execution_files(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((sync_task_id, execution_id)): Path<(i64, i64)>,
    Query(q): Query<ExecFilesQuery>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "sync:read")?;
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM sync_executions WHERE sync_task_id=? AND id=?",
    )
    .bind(sync_task_id)
    .bind(execution_id)
    .fetch_optional(&state.db)
    .await?;
    if exists.is_none() {
        return Err(AppError::bad_request("EXEC_NOT_FOUND", "执行记录不存在"));
    }
    let offset = q.offset.unwrap_or(0).max(0);
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(1) FROM sync_execution_files WHERE sync_execution_id=?",
    )
    .bind(execution_id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);
    let rows = sqlx::query_as::<
        _,
        (
            i64,
            i64,
            String,
            String,
            String,
            Option<i64>,
            Option<String>,
            String,
            String,
        ),
    >(
        r#"
        SELECT id, sync_execution_id, path, action, status, size, message, created_at, updated_at
        FROM sync_execution_files
        WHERE sync_execution_id = ?
        ORDER BY id ASC
        LIMIT ? OFFSET ?
        "#,
    )
    .bind(execution_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    let items: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            json!({
                "id": r.0,
                "sync_execution_id": r.1,
                "path": r.2,
                "action": r.3,
                "status": r.4,
                "size": r.5,
                "message": r.6,
                "created_at": r.7,
                "updated_at": r.8
            })
        })
        .collect();
    Ok(Json(json!({
        "items": items,
        "total": total,
        "offset": offset,
        "limit": limit
    })))
}

async fn cancel_execution(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((sync_task_id, execution_id)): Path<(i64, i64)>,
    Json(payload): Json<CancelIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "sync:run")?;
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
        WHERE sync_task_id=? AND id=?
        "#,
    )
    .bind(sync_task_id)
    .bind(execution_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::bad_request("EXEC_NOT_FOUND", "执行记录不存在"))?;

    let msg = payload
        .message
        .unwrap_or_else(|| "cancelled by user".into());
    if row.2 == "running" || row.2 == "pending" {
        // Mark cancelled; running sync loop polls this and stops cooperatively.
        sqlx::query(
            r#"
            UPDATE sync_executions
            SET status='cancelled', message=?
            WHERE id=?
            "#,
        )
        .bind(&msg)
        .bind(execution_id)
        .execute(&state.db)
        .await?;
        audit::write_user(
            &state.db,
            user.id,
            "sync.cancel",
            "sync_execution",
            execution_id,
            Some(&msg),
        )
        .await;
    }
    let refreshed = sqlx::query_as::<
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
        FROM sync_executions WHERE id=?
        "#,
    )
    .bind(execution_id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(map_sync_execution(
        refreshed.0,
        refreshed.1,
        refreshed.2,
        refreshed.3,
        refreshed.4,
        refreshed.5,
        refreshed.6,
    )))
}

async fn browse_local(
    user: CurrentUser,
    Json(payload): Json<LocalBrowseIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "sync:read")?;
    let raw = payload.path.trim();
    let path = if raw.is_empty() {
        PathBuf::from("/")
    } else {
        PathBuf::from(raw)
    };
    if !path.exists() {
        return Err(AppError::bad_request(
            "LOCAL_PATH_NOT_FOUND",
            format!("路径不存在: {}", path.display()),
        ));
    }
    let max_items = payload.max_items.unwrap_or(500).clamp(1, 2000);
    let mut items = Vec::new();
    if path.is_dir() {
        let rd = std::fs::read_dir(&path)
            .map_err(|e| AppError::bad_request("LOCAL_BROWSE", e.to_string()))?;
        for entry in rd.take(max_items) {
            let entry =
                entry.map_err(|e| AppError::bad_request("LOCAL_BROWSE", e.to_string()))?;
            let meta = entry.metadata().ok();
            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let size = meta.as_ref().and_then(|m| {
                if m.is_file() {
                    Some(m.len() as i64)
                } else {
                    None
                }
            });
            items.push(json!({
                "name": entry.file_name().to_string_lossy(),
                "path": entry.path().to_string_lossy(),
                "is_dir": is_dir,
                "size": size
            }));
        }
    }
    Ok(Json(json!({
        "path": path.to_string_lossy(),
        "items": items
    })))
}

async fn browse_netdisk(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<NetdiskBrowseIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "sync:read")?;
    let out = browse_drive(
        &state.db,
        DriveBrowseIn {
            dir_path: payload.dir_path,
            account_name: payload.account_name,
            shareurl: None,
            max_items: payload.max_items,
        },
    )
    .await
    .map_err(|e| AppError::bad_request("NETDISK_BROWSE", e.to_string()))?;
    Ok(Json(out))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_sync_tasks).post(create_sync_task))
        .route("/export", get(export_sync_tasks))
        .route("/import", post(import_sync_tasks))
        .route("/local/browse", post(browse_local))
        .route("/netdisk/browse", post(browse_netdisk))
        .route(
            "/{sync_task_id}",
            get(get_sync_task)
                .patch(patch_sync_task)
                .delete(delete_sync_task),
        )
        .route("/{sync_task_id}/run", post(run_sync))
        .route("/{sync_task_id}/run/stream", post(run_sync_stream))
        .route("/{sync_task_id}/executions", get(list_executions))
        .route("/{sync_task_id}/executions/latest", get(latest_execution))
        .route(
            "/{sync_task_id}/executions/{execution_id}/files",
            get(execution_files),
        )
        .route(
            "/{sync_task_id}/executions/{execution_id}/cancel",
            post(cancel_execution),
        )
}
