//! Frontend `/media/*` — TMDB search/detail, Douban discover, poster proxy.
use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::{AppError, AppResult};
use crate::services::douban;
use crate::services::proxy_image::{self, ProxyImageConfig};
use crate::services::tmdb::{self, TmdbClient};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize, Default)]
struct SearchQuery {
    q: Option<String>,
    #[serde(rename = "type")]
    media_type: Option<String>,
    page: Option<u32>,
    year: Option<String>,
}

#[derive(Deserialize, Default)]
struct DoubanListQuery {
    main_category: Option<String>,
    sub_category: Option<String>,
    start: Option<i64>,
    limit: Option<i64>,
}

#[derive(Deserialize)]
struct ProxyImageQuery {
    url: String,
}

async fn search(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<SearchQuery>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let query = q.q.unwrap_or_default().trim().to_string();
    let cfg = tmdb::load_config(&state).await.unwrap_or_default();
    if query.is_empty() {
        return Ok(Json(json!({
            "configured": cfg.configured(),
            "page": 1,
            "total_pages": 0,
            "total_results": 0,
            "items": []
        })));
    }
    if !cfg.configured() {
        return Ok(Json(json!({
            "configured": false,
            "page": 1,
            "total_pages": 0,
            "total_results": 0,
            "items": []
        })));
    }
    let client = TmdbClient::new(&cfg);
    let mt = q
        .media_type
        .unwrap_or_else(|| "multi".into())
        .to_lowercase();
    let page = q.page.unwrap_or(1).max(1);
    let raw = match mt.as_str() {
        "movie" => client.search_movie(&query, q.year.as_deref(), page).await,
        "tv" => client.search_tv(&query, q.year.as_deref(), page).await,
        _ => client.search_multi(&query, page).await,
    }
    .map_err(|e| AppError::bad_request("TMDB_HTTP", e.to_string()))?
    .unwrap_or(json!({ "results": [] }));

    let items: Vec<Value> = raw
        .get("results")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|item| {
            let media = item
                .get("media_type")
                .and_then(|v| v.as_str())
                .unwrap_or(if mt == "multi" { "tv" } else { mt.as_str() });
            json!({
                "id": item.get("id"),
                "media_type": media,
                "title": item.get("title").cloned(),
                "original_title": item.get("original_title").cloned(),
                "name": item.get("name").cloned(),
                "original_name": item.get("original_name").cloned(),
                "overview": item.get("overview").cloned(),
                "poster_path": item.get("poster_path").cloned(),
                "release_date": item.get("release_date").cloned(),
                "first_air_date": item.get("first_air_date").cloned(),
                "vote_average": item.get("vote_average").cloned()
            })
        })
        .collect();

    Ok(Json(json!({
        "configured": true,
        "page": raw.get("page").cloned().unwrap_or(json!(page)),
        "total_pages": raw.get("total_pages").cloned().unwrap_or(json!(1)),
        "total_results": raw.get("total_results").cloned().unwrap_or(json!(items.len())),
        "items": items
    })))
}

async fn detail(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((media_type, id)): Path<(String, i64)>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let cfg = tmdb::load_config(&state).await.unwrap_or_default();
    if !cfg.configured() {
        return Err(AppError::bad_request("TMDB_NOT_CONFIGURED", "请先配置 TMDB API Key"));
    }
    let client = TmdbClient::new(&cfg);
    let mt = media_type.to_lowercase();
    let details = if mt == "movie" {
        client.get_movie_details(id).await
    } else {
        client.get_tv_details(id).await
    }
    .map_err(|e| AppError::bad_request("TMDB_HTTP", e.to_string()))?
    .ok_or_else(|| AppError::bad_request("TMDB_NOT_FOUND", "未找到该条目"))?;

    let weekdays = tmdb::infer_weekdays_from_details(&details);
    let iso_days: Vec<i64> = weekdays
        .iter()
        .map(|d| if *d == 6 { 7 } else { (*d + 1) as i64 })
        .collect();

    Ok(Json(json!({
        "media_type": mt,
        "data": details,
        "update_weekdays": iso_days,
        "episode_weekdays": iso_days
    })))
}

async fn douban_categories(user: CurrentUser) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    Ok(Json(douban::categories()))
}

async fn douban_list(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<DoubanListQuery>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let main = q.main_category.unwrap_or_else(|| "tv_drama".into());
    let sub = q.sub_category.unwrap_or_default();
    let mut out = douban::list(&main, &sub, q.start.unwrap_or(0), q.limit.unwrap_or(20)).await;
    let cfg = tmdb::load_config(&state).await.unwrap_or_default();
    if let Some(obj) = out.as_object_mut() {
        obj.insert("tmdb_configured".into(), json!(cfg.configured()));
        obj.insert("notice".into(), Value::Null);
    }
    Ok(Json(out))
}

async fn proxy_image(
    State(state): State<AppState>,
    Query(q): Query<ProxyImageQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let url = q.url.trim();
    if url.len() < 8 || url.len() > 2048 || !proxy_image::host_allowed(url) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let cfg = ProxyImageConfig::from_data_dir(&state.settings.data_dir);
    let cache_control = format!("public, max-age={}", cfg.ttl_seconds);

    if let Some(cached) = proxy_image::try_read_cache(&cfg, url) {
        if let Some(inm) = headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) {
            if inm.split(',').any(|p| {
                let p = p.trim();
                p == "*" || p == cached.etag || p.trim_matches('"') == cached.etag.trim_matches('"')
            }) {
                return (
                    StatusCode::NOT_MODIFIED,
                    [
                        (header::ETAG, cached.etag),
                        (header::CACHE_CONTROL, cache_control),
                    ],
                )
                    .into_response();
            }
        }
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, cached.content_type),
                (header::ETAG, cached.etag),
                (header::CACHE_CONTROL, cached.cache_control),
            ],
            cached.bytes,
        )
            .into_response();
    }

    match proxy_image::fetch_image(url).await {
        Ok((bytes, ct)) => {
            proxy_image::store_cache(&cfg, url, &ct, &bytes);
            let etag = format!("\"{}\"", {
                use sha2::{Digest, Sha256};
                let mut h = Sha256::new();
                h.update(url.as_bytes());
                hex::encode(h.finalize())
            });
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, ct),
                    (header::ETAG, etag),
                    (header::CACHE_CONTROL, cache_control),
                ],
                bytes,
            )
                .into_response()
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/search", get(search))
        .route("/douban/categories", get(douban_categories))
        .route("/douban/list", get(douban_list))
        .route("/proxy-image", get(proxy_image))
        .route("/{media_type}/{id}", get(detail))
}
