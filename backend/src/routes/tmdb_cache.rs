//! Minimal TMDB cache management APIs for settings UI compatibility.
use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::{AppError, AppResult};
use crate::services::tmdb::{self, TmdbClient};
use crate::settings_store::{get_or_default, put_typed};
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::FromRow;

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
struct CacheScheduler {
    #[serde(default)]
    enabled: bool,
    #[serde(default = "default_crontab")]
    crontab: String,
    #[serde(default = "default_tz")]
    timezone: String,
    #[serde(default = "default_max")]
    max_items_per_run: i64,
    #[serde(default = "default_true")]
    only_refresh_linked_tasks: bool,
    #[serde(default = "default_retention")]
    retention_days: i64,
}

fn default_crontab() -> String {
    "0 */6 * * *".into()
}
fn default_tz() -> String {
    "Asia/Shanghai".into()
}
fn default_max() -> i64 {
    50
}
fn default_true() -> bool {
    true
}
fn default_retention() -> i64 {
    30
}

impl Default for CacheScheduler {
    fn default() -> Self {
        Self {
            enabled: false,
            crontab: default_crontab(),
            timezone: default_tz(),
            max_items_per_run: default_max(),
            only_refresh_linked_tasks: true,
            retention_days: default_retention(),
        }
    }
}

#[derive(Deserialize, Default)]
struct SchedulerPatch {
    enabled: Option<bool>,
    crontab: Option<String>,
    timezone: Option<String>,
    max_items_per_run: Option<i64>,
    only_refresh_linked_tasks: Option<bool>,
    retention_days: Option<i64>,
}

#[derive(Deserialize, Default)]
struct StatusQuery {
    media_type: Option<String>,
    tmdb_id: Option<i64>,
}

#[derive(Deserialize, Default)]
struct ListQuery {
    page: Option<i64>,
    page_size: Option<i64>,
    q: Option<String>,
}

#[derive(Deserialize, Default)]
struct RefreshIn {
    media_type: String,
    tmdb_id: i64,
}

#[derive(Deserialize, Default)]
struct PurgeIn {
    older_than_days: Option<i64>,
}

#[derive(Deserialize, Default)]
struct SetTtlIn {
    media_type: String,
    tmdb_id: i64,
    ttl_hours: Option<i64>,
}

#[derive(Debug, FromRow)]
struct CacheRow {
    media_type: String,
    tmdb_id: i64,
    language: String,
    poster_language: String,
    display_title: Option<String>,
    original_title: Option<String>,
    year: Option<String>,
    status: Option<String>,
    payload_json: String,
    update_weekdays_json: String,
    fetched_at: Option<String>,
    expires_at: Option<String>,
    last_accessed_at: Option<String>,
    refresh_in_progress: i64,
    fail_count: i64,
    last_error: Option<String>,
}

fn map_item(r: &CacheRow, configured: bool, with_payload: bool) -> Value {
    let mut v = json!({
        "configured": configured,
        "media_type": r.media_type,
        "tmdb_id": r.tmdb_id,
        "exists": true,
        "language": r.language,
        "poster_language": r.poster_language,
        "display_title": r.display_title,
        "original_title": r.original_title,
        "year": r.year,
        "status": r.status,
        "fetched_at": r.fetched_at,
        "expires_at": r.expires_at,
        "last_accessed_at": r.last_accessed_at,
        "refresh_in_progress": r.refresh_in_progress != 0,
        "fail_count": r.fail_count,
        "last_error": r.last_error,
        "update_weekdays": serde_json::from_str::<Value>(&r.update_weekdays_json).unwrap_or(json!([]))
    });
    if with_payload {
        v["payload_json"] = json!(r.payload_json);
    }
    v
}

async fn get_scheduler(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let cfg: CacheScheduler = get_or_default(&state.db, "tmdb_cache_scheduler")
        .await
        .unwrap_or_default();
    Ok(Json(serde_json::to_value(cfg).unwrap_or(json!({}))))
}

async fn patch_scheduler(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<SchedulerPatch>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let mut cfg: CacheScheduler = get_or_default(&state.db, "tmdb_cache_scheduler")
        .await
        .unwrap_or_default();
    if let Some(v) = payload.enabled {
        cfg.enabled = v;
    }
    if let Some(v) = payload.crontab {
        cfg.crontab = v;
    }
    if let Some(v) = payload.timezone {
        cfg.timezone = v;
    }
    if let Some(v) = payload.max_items_per_run {
        cfg.max_items_per_run = v.max(1);
    }
    if let Some(v) = payload.only_refresh_linked_tasks {
        cfg.only_refresh_linked_tasks = v;
    }
    if let Some(v) = payload.retention_days {
        cfg.retention_days = v.max(1);
    }
    put_typed(&state.db, "tmdb_cache_scheduler", &cfg).await?;
    Ok(Json(serde_json::to_value(cfg).unwrap_or(json!({}))))
}

async fn cache_status(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<StatusQuery>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let cfg = tmdb::load_config(&state).await.unwrap_or_default();
    let configured = cfg.configured();
    let mt = q.media_type.unwrap_or_else(|| "tv".into());
    let tid = q.tmdb_id.unwrap_or(0);
    let row = sqlx::query_as::<_, CacheRow>(
        r#"
        SELECT media_type, tmdb_id, language, poster_language, display_title, original_title,
               year, status, payload_json, update_weekdays_json, fetched_at, expires_at,
               last_accessed_at, refresh_in_progress, fail_count, last_error
        FROM tmdb_media_cache
        WHERE media_type=? AND tmdb_id=?
        LIMIT 1
        "#,
    )
    .bind(&mt)
    .bind(tid)
    .fetch_optional(&state.db)
    .await?;
    Ok(Json(match row {
        Some(r) => map_item(&r, configured, false),
        None => json!({
            "configured": configured,
            "media_type": mt,
            "tmdb_id": tid,
            "exists": false
        }),
    }))
}

async fn cache_item(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<StatusQuery>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let cfg = tmdb::load_config(&state).await.unwrap_or_default();
    let configured = cfg.configured();
    let mt = q.media_type.unwrap_or_else(|| "tv".into());
    let tid = q
        .tmdb_id
        .ok_or_else(|| AppError::bad_request("TMDB_ID_REQUIRED", "缺少 tmdb_id"))?;
    let row = sqlx::query_as::<_, CacheRow>(
        r#"
        SELECT media_type, tmdb_id, language, poster_language, display_title, original_title,
               year, status, payload_json, update_weekdays_json, fetched_at, expires_at,
               last_accessed_at, refresh_in_progress, fail_count, last_error
        FROM tmdb_media_cache
        WHERE media_type=? AND tmdb_id=?
        LIMIT 1
        "#,
    )
    .bind(&mt)
    .bind(tid)
    .fetch_optional(&state.db)
    .await?;
    Ok(Json(match row {
        Some(r) => map_item(&r, configured, true),
        None => json!({
            "configured": configured,
            "media_type": mt,
            "tmdb_id": tid,
            "exists": false
        }),
    }))
}

async fn cache_list(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let cfg = tmdb::load_config(&state).await.unwrap_or_default();
    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;
    let like = q
        .q
        .as_deref()
        .map(|s| format!("%{}%", s.trim()))
        .filter(|s| s != "%%");

    let total: i64 = if let Some(ref pat) = like {
        sqlx::query_scalar(
            "SELECT COUNT(1) FROM tmdb_media_cache WHERE display_title LIKE ? OR original_title LIKE ?",
        )
        .bind(pat)
        .bind(pat)
        .fetch_one(&state.db)
        .await?
    } else {
        sqlx::query_scalar("SELECT COUNT(1) FROM tmdb_media_cache")
            .fetch_one(&state.db)
            .await?
    };

    let rows = if let Some(ref pat) = like {
        sqlx::query_as::<_, CacheRow>(
            r#"
            SELECT media_type, tmdb_id, language, poster_language, display_title, original_title,
                   year, status, payload_json, update_weekdays_json, fetched_at, expires_at,
                   last_accessed_at, refresh_in_progress, fail_count, last_error
            FROM tmdb_media_cache
            WHERE display_title LIKE ? OR original_title LIKE ?
            ORDER BY fetched_at DESC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(pat)
        .bind(pat)
        .bind(page_size)
        .bind(offset)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as::<_, CacheRow>(
            r#"
            SELECT media_type, tmdb_id, language, poster_language, display_title, original_title,
                   year, status, payload_json, update_weekdays_json, fetched_at, expires_at,
                   last_accessed_at, refresh_in_progress, fail_count, last_error
            FROM tmdb_media_cache
            ORDER BY fetched_at DESC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(page_size)
        .bind(offset)
        .fetch_all(&state.db)
        .await?
    };

    Ok(Json(json!({
        "configured": cfg.configured(),
        "page": page,
        "page_size": page_size,
        "total": total,
        "items": rows.iter().map(|r| map_item(r, cfg.configured(), false)).collect::<Vec<_>>()
    })))
}

async fn upsert_from_detail(
    state: &AppState,
    media_type: &str,
    tmdb_id: i64,
    detail: &Value,
) -> AppResult<()> {
    let title = tmdb::media_title(detail);
    let year = tmdb::media_year(detail).map(|y| y.to_string());
    let status = detail.get("status").and_then(|v| v.as_str());
    let original = detail
        .get("original_name")
        .or_else(|| detail.get("original_title"))
        .and_then(|v| v.as_str());
    let weekdays = tmdb::infer_weekdays_from_details(detail);
    let now = Utc::now();
    let expires = (now + Duration::hours(72)).to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO tmdb_media_cache
            (media_type, tmdb_id, language, poster_language, display_title, original_title,
             year, status, payload_json, update_weekdays_json, fetched_at, expires_at,
             last_accessed_at, refresh_in_progress, fail_count, last_error)
        VALUES (?, ?, 'zh-CN', 'zh-CN', ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, NULL)
        ON CONFLICT(media_type, tmdb_id, language, poster_language) DO UPDATE SET
            display_title=excluded.display_title,
            original_title=excluded.original_title,
            year=excluded.year,
            status=excluded.status,
            payload_json=excluded.payload_json,
            update_weekdays_json=excluded.update_weekdays_json,
            fetched_at=excluded.fetched_at,
            expires_at=excluded.expires_at,
            last_accessed_at=excluded.last_accessed_at,
            refresh_in_progress=0,
            fail_count=0,
            last_error=NULL
        "#,
    )
    .bind(media_type)
    .bind(tmdb_id)
    .bind(&title)
    .bind(original)
    .bind(&year)
    .bind(status)
    .bind(detail.to_string())
    .bind(serde_json::to_string(&weekdays).unwrap_or_else(|_| "[]".into()))
    .bind(now.to_rfc3339())
    .bind(&expires)
    .bind(now.to_rfc3339())
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn refresh_one(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<RefreshIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let cfg = tmdb::load_config(&state)
        .await
        .map_err(|e| AppError::bad_request("TMDB_CONFIG", e.to_string()))?;
    if !cfg.configured() {
        return Err(AppError::bad_request("TMDB_NOT_CONFIGURED", "TMDB 未配置"));
    }
    let client = TmdbClient::new(&cfg);
    let mt = payload.media_type.trim().to_lowercase();
    let detail = if mt == "movie" {
        client.get_movie_details(payload.tmdb_id).await
    } else {
        client.get_tv_details(payload.tmdb_id).await
    }
    .map_err(|e| AppError::bad_request("TMDB_FETCH", e.to_string()))?
    .ok_or_else(|| AppError::bad_request("TMDB_EMPTY", "TMDB 返回空"))?;

    upsert_from_detail(&state, &mt, payload.tmdb_id, &detail).await?;
    let status = cache_status(
        State(state),
        user,
        Query(StatusQuery {
            media_type: Some(mt),
            tmdb_id: Some(payload.tmdb_id),
        }),
    )
    .await?
    .0;
    Ok(Json(json!({ "queued": false, "status": status })))
}

async fn refresh_linked(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let cfg = tmdb::load_config(&state)
        .await
        .map_err(|e| AppError::bad_request("TMDB_CONFIG", e.to_string()))?;
    if !cfg.configured() {
        return Ok(Json(json!({ "configured": false, "targets": 0, "refreshed": 0 })));
    }
    let client = TmdbClient::new(&cfg);
    let rows: Vec<(String, i64)> = sqlx::query_as(
        r#"
        SELECT DISTINCT tmdb_media_type, tmdb_id
        FROM tasks
        WHERE tmdb_id IS NOT NULL AND tmdb_media_type IS NOT NULL
        LIMIT 100
        "#,
    )
    .fetch_all(&state.db)
    .await?;
    let targets = rows.len();
    let mut refreshed = 0usize;
    for (mt, tid) in rows {
        let mt = mt.to_lowercase();
        let detail = if mt == "movie" {
            client.get_movie_details(tid).await
        } else {
            client.get_tv_details(tid).await
        };
        if let Ok(Some(d)) = detail {
            if upsert_from_detail(&state, &mt, tid, &d).await.is_ok() {
                refreshed += 1;
            }
        }
    }
    let _ = user;
    Ok(Json(json!({
        "configured": true,
        "targets": targets,
        "refreshed": refreshed
    })))
}

async fn purge_cache(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<PurgeIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let days = payload.older_than_days.unwrap_or(30).max(1);
    let cutoff = (Utc::now() - Duration::days(days)).to_rfc3339();
    let n = sqlx::query("DELETE FROM tmdb_media_cache WHERE fetched_at IS NULL OR fetched_at < ?")
        .bind(&cutoff)
        .execute(&state.db)
        .await?
        .rows_affected();
    Ok(Json(json!({ "deleted": n })))
}

async fn set_ttl(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<SetTtlIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let hours = payload.ttl_hours.unwrap_or(72).max(1);
    let expires = (Utc::now() + Duration::hours(hours)).to_rfc3339();
    let n = sqlx::query(
        "UPDATE tmdb_media_cache SET expires_at=? WHERE media_type=? AND tmdb_id=?",
    )
    .bind(&expires)
    .bind(payload.media_type.trim())
    .bind(payload.tmdb_id)
    .execute(&state.db)
    .await?
    .rows_affected();
    let status = cache_status(
        State(state),
        user,
        Query(StatusQuery {
            media_type: Some(payload.media_type),
            tmdb_id: Some(payload.tmdb_id),
        }),
    )
    .await?
    .0;
    Ok(Json(json!({ "updated": n > 0, "status": status })))
}

async fn delete_item(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<StatusQuery>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let mt = q.media_type.unwrap_or_else(|| "tv".into());
    let tid = q
        .tmdb_id
        .ok_or_else(|| AppError::bad_request("TMDB_ID_REQUIRED", "缺少 tmdb_id"))?;
    let n = sqlx::query("DELETE FROM tmdb_media_cache WHERE media_type=? AND tmdb_id=?")
        .bind(&mt)
        .bind(tid)
        .execute(&state.db)
        .await?
        .rows_affected();
    Ok(Json(json!({ "deleted": n })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/cache/scheduler", get(get_scheduler).patch(patch_scheduler))
        .route("/cache/status", get(cache_status))
        .route("/cache/item", get(cache_item).delete(delete_item))
        .route("/cache/list", get(cache_list))
        .route("/cache/refresh", post(refresh_one))
        .route("/cache/refresh-linked-tasks", post(refresh_linked))
        .route("/cache/purge", post(purge_cache))
        .route("/cache/set-ttl", post(set_ttl))
}
