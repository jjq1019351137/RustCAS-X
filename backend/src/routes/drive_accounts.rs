use crate::adapters::{create_adapter, drive_types, serialize_cookie};
use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct DriveAccountCreateIn {
    name: String,
    drive_type: String,
    cookie: Option<String>,
    config: Option<Value>,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    is_default: bool,
    #[serde(default = "default_threshold")]
    capacity_warning_threshold: i64,
}

fn default_true() -> bool {
    true
}
fn default_threshold() -> i64 {
    85
}

#[derive(Deserialize)]
pub struct DriveAccountUpdateIn {
    name: Option<String>,
    cookie: Option<String>,
    config: Option<Value>,
    enabled: Option<bool>,
    is_default: Option<bool>,
    capacity_warning_threshold: Option<i64>,
}

#[derive(Deserialize)]
pub struct StatusIn {
    enabled: bool,
}

fn account_out(
    id: i64,
    name: String,
    drive_type: String,
    config_json: Option<String>,
    profile_json: Option<String>,
    enabled: i64,
    is_default: i64,
    capacity_warning_threshold: i64,
    runtime_status: Option<String>,
    probe_fail_count: i64,
    last_checked_at: Option<String>,
    last_error: Option<String>,
    created_at: String,
    updated_at: String,
) -> Value {
    let config: Value = config_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| json!({}));
    let profile: Value = profile_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| json!({}));
    let used = profile
        .get("used_space")
        .and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|u| u as i64)));
    let total = profile
        .get("total_space")
        .and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|u| u as i64)));
    let usage_ratio = match (used, total) {
        (Some(u), Some(t)) if t > 0 => Some((u as f64) / (t as f64)),
        _ => None,
    };
    json!({
        "id": id,
        "name": name,
        "drive_type": drive_type,
        "config": config,
        "profile": profile,
        "enabled": enabled != 0,
        "is_default": is_default != 0,
        "capacity_warning_threshold": capacity_warning_threshold,
        "used_space": used,
        "total_space": total,
        "usage_ratio": usage_ratio,
        "capacity_warning": usage_ratio
            .map(|r| r * 100.0 >= capacity_warning_threshold as f64)
            .unwrap_or(false),
        "runtime_status": runtime_status,
        "probe_fail_count": probe_fail_count,
        "last_checked_at": last_checked_at,
        "profile_updated_at": last_checked_at,
        "last_error": last_error,
        "nickname": profile.get("nickname"),
        "member_type": profile.get("member_type"),
        "has_302_path": false,
        "lsdir_cache_base_path": null,
        "lsdir_cache_file_total": 0,
        "lsdir_cache_updated_at": null,
        "lsdir_cache_refresh": null,
        "created_at": created_at,
        "updated_at": updated_at
    })
}

async fn list_types(_user: CurrentUser) -> AppResult<Json<Value>> {
    let items: Vec<Value> = drive_types()
        .into_iter()
        .map(|t| {
            json!({
                "code": t.code,
                "drive_name": t.drive_name,
                "class_name": t.class_name,
                "config_format": t.config_format,
                "default_config": t.default_config,
                "config_fields": t.config_fields
            })
        })
        .collect();
    Ok(Json(json!(items)))
}

async fn list_accounts(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Value>> {
    require_perm(&user, "drive_account:read")?;
    let rows = sqlx::query_as::<
        _,
        (
            i64,
            String,
            String,
            Option<String>,
            Option<String>,
            i64,
            i64,
            i64,
            Option<String>,
            i64,
            Option<String>,
            Option<String>,
            String,
            String,
        ),
    >(
        r#"
        SELECT id, name, drive_type, config_json, profile_json, enabled, is_default,
               capacity_warning_threshold, runtime_status, probe_fail_count,
               last_checked_at, last_error, created_at, updated_at
        FROM drive_accounts
        ORDER BY id ASC
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    let out: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            account_out(
                r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9, r.10, r.11, r.12, r.13,
            )
        })
        .collect();
    Ok(Json(json!(out)))
}

async fn create_account(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<DriveAccountCreateIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "drive_account:write")?;
    let drive_type = payload.drive_type.trim().to_string();
    // Focus: Quark only for this migration wave (cloud189 retained but not promoted).
    if drive_type != "quark" && drive_type != "cloud189" {
        return Err(AppError::bad_request("DRIVE_TYPE", "仅支持 quark（推荐）/ cloud189（未继续迁移）"));
    }
    let config = payload.config.unwrap_or_else(|| json!({}));
    let cookie = if let Some(c) = payload.cookie.filter(|s| !s.trim().is_empty()) {
        c
    } else {
        serialize_cookie(&drive_type, &config)
    };
    let now = Utc::now().to_rfc3339();
    if payload.is_default {
        sqlx::query("UPDATE drive_accounts SET is_default = 0")
            .execute(&state.db)
            .await?;
    }
    let id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO drive_accounts
            (name, drive_type, cookie, config_json, enabled, is_default, capacity_warning_threshold, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        RETURNING id
        "#,
    )
    .bind(&payload.name)
    .bind(&drive_type)
    .bind(&cookie)
    .bind(config.to_string())
    .bind(if payload.enabled { 1 } else { 0 })
    .bind(if payload.is_default { 1 } else { 0 })
    .bind(payload.capacity_warning_threshold.clamp(1, 100))
    .bind(&now)
    .bind(&now)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(account_out(
        id,
        payload.name,
        drive_type,
        Some(config.to_string()),
        None,
        if payload.enabled { 1 } else { 0 },
        if payload.is_default { 1 } else { 0 },
        payload.capacity_warning_threshold,
        None,
        0,
        None,
        None,
        now.clone(),
        now,
    )))
}

async fn patch_account(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(account_id): Path<i64>,
    Json(payload): Json<DriveAccountUpdateIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "drive_account:write")?;
    let row = sqlx::query_as::<_, (String, String, String, Option<String>, i64, i64, i64)>(
        "SELECT name, drive_type, cookie, config_json, enabled, is_default, capacity_warning_threshold FROM drive_accounts WHERE id = ?",
    )
    .bind(account_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("DRIVE_NOT_FOUND", "账号不存在"))?;

    let mut name = row.0;
    let drive_type = row.1;
    let mut cookie = row.2;
    let mut config_json = row.3.unwrap_or_else(|| "{}".into());
    let mut enabled = row.4;
    let mut is_default = row.5;
    let mut threshold = row.6;

    if let Some(n) = payload.name {
        name = n;
    }
    if let Some(c) = payload.config {
        config_json = c.to_string();
        cookie = serialize_cookie(&drive_type, &c);
    }
    if let Some(c) = payload.cookie {
        cookie = c;
    }
    if let Some(e) = payload.enabled {
        enabled = if e { 1 } else { 0 };
    }
    if let Some(d) = payload.is_default {
        is_default = if d { 1 } else { 0 };
        if d {
            sqlx::query("UPDATE drive_accounts SET is_default = 0")
                .execute(&state.db)
                .await?;
        }
    }
    if let Some(t) = payload.capacity_warning_threshold {
        threshold = t.clamp(1, 100);
    }
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        UPDATE drive_accounts
        SET name=?, cookie=?, config_json=?, enabled=?, is_default=?, capacity_warning_threshold=?, updated_at=?
        WHERE id=?
        "#,
    )
    .bind(&name)
    .bind(&cookie)
    .bind(&config_json)
    .bind(enabled)
    .bind(is_default)
    .bind(threshold)
    .bind(&now)
    .bind(account_id)
    .execute(&state.db)
    .await?;

    list_one(&state, account_id).await
}

async fn list_one(state: &AppState, account_id: i64) -> AppResult<Json<Value>> {
    let r = sqlx::query_as::<
        _,
        (
            i64,
            String,
            String,
            Option<String>,
            Option<String>,
            i64,
            i64,
            i64,
            Option<String>,
            i64,
            Option<String>,
            Option<String>,
            String,
            String,
        ),
    >(
        r#"
        SELECT id, name, drive_type, config_json, profile_json, enabled, is_default,
               capacity_warning_threshold, runtime_status, probe_fail_count,
               last_checked_at, last_error, created_at, updated_at
        FROM drive_accounts WHERE id = ?
        "#,
    )
    .bind(account_id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(account_out(
        r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9, r.10, r.11, r.12, r.13,
    )))
}

async fn patch_status(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(account_id): Path<i64>,
    Json(payload): Json<StatusIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "drive_account:write")?;
    sqlx::query("UPDATE drive_accounts SET enabled=?, updated_at=? WHERE id=?")
        .bind(if payload.enabled { 1 } else { 0 })
        .bind(Utc::now().to_rfc3339())
        .bind(account_id)
        .execute(&state.db)
        .await?;
    list_one(&state, account_id).await
}

async fn set_default(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(account_id): Path<i64>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "drive_account:write")?;
    sqlx::query("UPDATE drive_accounts SET is_default = 0")
        .execute(&state.db)
        .await?;
    sqlx::query("UPDATE drive_accounts SET is_default = 1, updated_at=? WHERE id=?")
        .bind(Utc::now().to_rfc3339())
        .bind(account_id)
        .execute(&state.db)
        .await?;
    list_one(&state, account_id).await
}

async fn delete_account(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(account_id): Path<i64>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "drive_account:write")?;
    let n = sqlx::query("DELETE FROM drive_accounts WHERE id = ?")
        .bind(account_id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::not_found("DRIVE_NOT_FOUND", "账号不存在"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn probe(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(account_id): Path<i64>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "drive_account:write")?;
    let row = sqlx::query_as::<_, (String, String, Option<String>)>(
        "SELECT drive_type, cookie, config_json FROM drive_accounts WHERE id = ?",
    )
    .bind(account_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("DRIVE_NOT_FOUND", "账号不存在"))?;

    let config: Value = row
        .2
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| json!({}));
    let mut adapter = create_adapter(&row.0, Some(&config), Some(&row.1))
        .map_err(|e| AppError::bad_request("ADAPTER", e.to_string()))?;
    let init = adapter.init().await.unwrap_or_else(|e| json!({ "ok": false, "message": e.to_string() }));
    let ok = init.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let now = Utc::now().to_rfc3339();
    let profile = if ok { Some(init.to_string()) } else { None };
    sqlx::query(
        r#"
        UPDATE drive_accounts
        SET runtime_status=?, last_checked_at=?, last_error=?, profile_json=COALESCE(?, profile_json),
            probe_fail_count=CASE WHEN ? THEN 0 ELSE probe_fail_count+1 END, updated_at=?
        WHERE id=?
        "#,
    )
    .bind(if ok { "ok" } else { "error" })
    .bind(&now)
    .bind(init.get("message").and_then(|v| v.as_str()))
    .bind(profile)
    .bind(ok)
    .bind(&now)
    .bind(account_id)
    .execute(&state.db)
    .await?;
    list_one(&state, account_id).await
}

async fn extract_share(
    State(_state): State<AppState>,
    user: CurrentUser,
    Path(account_id): Path<i64>,
    Json(payload): Json<Value>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "drive_account:read")?;
    let url = payload
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT drive_type FROM drive_accounts WHERE id = ?",
    )
    .bind(account_id)
    .fetch_optional(&_state.db)
    .await?
    .ok_or_else(|| AppError::not_found("DRIVE_NOT_FOUND", "账号不存在"))?;
    let adapter = create_adapter(&row.0, None, None)
        .map_err(|e| AppError::bad_request("ADAPTER", e.to_string()))?;
    let extracted = adapter.extract_url(&url);
    Ok(Json(json!({
        "pwd_id": extracted.pwd_id,
        "passcode": extracted.passcode,
        "pdir_fid": extracted.pdir_fid,
        "paths": extracted.paths
    })))
}

async fn lsdir_cache_status(_user: CurrentUser) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "items": [] })))
}

#[derive(Deserialize)]
struct LsDirIn {
    #[serde(default = "root_fid")]
    pdir_fid: String,
    #[serde(default)]
    max_items: i64,
}
fn root_fid() -> String {
    "0".into()
}

async fn ls_dir(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(account_id): Path<i64>,
    Json(payload): Json<LsDirIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "drive_account:read")?;
    let row = sqlx::query_as::<_, (String, String, Option<String>)>(
        "SELECT drive_type, cookie, config_json FROM drive_accounts WHERE id = ?",
    )
    .bind(account_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("DRIVE_NOT_FOUND", "账号不存在"))?;
    let config: Value = row
        .2
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| json!({}));
    let mut adapter = create_adapter(&row.0, Some(&config), Some(&row.1))
        .map_err(|e| AppError::bad_request("ADAPTER", e.to_string()))?;
    let _ = adapter.init().await;
    let out = adapter
        .ls_dir(&payload.pdir_fid, payload.max_items)
        .await
        .map_err(|e| AppError::api(axum::http::StatusCode::BAD_GATEWAY, "LSDIR", e.to_string()))?;
    Ok(Json(out))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/types", get(list_types))
        .route("/lsdir-cache/status", get(lsdir_cache_status))
        .route("/", get(list_accounts).post(create_account))
        .route("/{account_id}", patch(patch_account).delete(delete_account))
        .route("/{account_id}/status", patch(patch_status))
        .route("/{account_id}/default", post(set_default))
        .route("/{account_id}/probe", post(probe))
        .route("/{account_id}/extract-url", post(extract_share))
        .route("/{account_id}/lsdir", post(ls_dir))
}
