use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::{AppError, AppResult};
use crate::services::tmdb::{self, TmdbClient, TmdbConfig};
use crate::services::audit;
use crate::settings_store::{get_or_default, put_typed};
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

fn default_lang() -> String {
    "zh-CN".into()
}

#[derive(Deserialize)]
struct TmdbUpdate {
    api_key: Option<String>,
    language: Option<String>,
    poster_language: Option<String>,
    disable_guessit_tmdb_fallback_rename: Option<bool>,
    guessit_tmdb_tv_rename_template: Option<String>,
    guessit_tmdb_movie_rename_template: Option<String>,
}

fn to_out(cfg: &TmdbConfig) -> Value {
    json!({
        "has_api_key": !cfg.api_key.trim().is_empty(),
        "language": cfg.language,
        "poster_language": cfg.poster_language,
        "disable_guessit_tmdb_fallback_rename": cfg.disable_guessit_tmdb_fallback_rename,
        "guessit_tmdb_tv_rename_template": cfg.guessit_tmdb_tv_rename_template,
        "guessit_tmdb_movie_rename_template": cfg.guessit_tmdb_movie_rename_template
    })
}

async fn get_config(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let cfg: TmdbConfig = get_or_default(&state.db, "tmdb_config").await?;
    Ok(Json(to_out(&cfg)))
}

async fn patch_config(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<TmdbUpdate>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let mut cfg: TmdbConfig = get_or_default(&state.db, "tmdb_config").await?;
    if let Some(v) = payload.api_key {
        cfg.api_key = v;
    }
    if let Some(v) = payload.language {
        cfg.language = if v.trim().is_empty() {
            default_lang()
        } else {
            v
        };
    }
    if let Some(v) = payload.poster_language {
        cfg.poster_language = if v.trim().is_empty() {
            default_lang()
        } else {
            v
        };
    }
    if let Some(v) = payload.disable_guessit_tmdb_fallback_rename {
        cfg.disable_guessit_tmdb_fallback_rename = v;
    }
    if let Some(v) = payload.guessit_tmdb_tv_rename_template {
        cfg.guessit_tmdb_tv_rename_template = v;
    }
    if let Some(v) = payload.guessit_tmdb_movie_rename_template {
        cfg.guessit_tmdb_movie_rename_template = v;
    }
    put_typed(&state.db, "tmdb_config", &cfg).await?;
    audit::write_user(
        &state.db,
        user.id,
        "tmdb.config.update",
        "tmdb",
        "config",
        None,
    )
    .await;
    Ok(Json(to_out(&cfg)))
}

#[derive(Deserialize)]
struct SearchQuery {
    q: Option<String>,
    query: Option<String>,
    year: Option<String>,
    #[serde(default = "default_page")]
    page: u32,
    #[serde(default = "default_media")]
    media_type: String,
}

fn default_page() -> u32 {
    1
}
fn default_media() -> String {
    "tv".into()
}

async fn search(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<SearchQuery>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let query = q
        .q
        .or(q.query)
        .unwrap_or_default()
        .trim()
        .to_string();
    if query.is_empty() {
        return Err(AppError::bad_request("TMDB_QUERY_REQUIRED", "缺少搜索关键词"));
    }
    let cfg = tmdb::load_config(&state).await.map_err(|e| {
        AppError::api(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "TMDB_CONFIG",
            e.to_string(),
        )
    })?;
    if !cfg.configured() {
        return Err(AppError::bad_request("TMDB_NOT_CONFIGURED", "请先配置 TMDB API Key"));
    }
    let client = TmdbClient::new(&cfg);
    let mt = q.media_type.to_lowercase();
    let raw = match mt.as_str() {
        "movie" => client
            .search_movie(&query, q.year.as_deref(), q.page)
            .await,
        "multi" => client.search_multi(&query, q.page).await,
        _ => client.search_tv(&query, q.year.as_deref(), q.page).await,
    }
    .map_err(|e| {
        AppError::api(
            axum::http::StatusCode::BAD_GATEWAY,
            "TMDB_HTTP",
            e.to_string(),
        )
    })?
    .unwrap_or(json!({ "results": [] }));

    let results = raw
        .get("results")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|item| {
            let media = item
                .get("media_type")
                .and_then(|v| v.as_str())
                .unwrap_or(mt.as_str());
            json!({
                "id": item.get("id"),
                "media_type": media,
                "title": item.get("name").or_else(|| item.get("title")).cloned().unwrap_or(json!("")),
                "original_title": item.get("original_name").or_else(|| item.get("original_title")).cloned(),
                "overview": item.get("overview").cloned(),
                "poster_path": item.get("poster_path").cloned(),
                "backdrop_path": item.get("backdrop_path").cloned(),
                "year": tmdb::media_year(&item),
                "first_air_date": item.get("first_air_date").cloned(),
                "release_date": item.get("release_date").cloned(),
                "popularity": item.get("popularity").cloned(),
                "vote_average": item.get("vote_average").cloned()
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(json!({
        "query": query,
        "media_type": mt,
        "page": raw.get("page").cloned().unwrap_or(json!(q.page)),
        "total_pages": raw.get("total_pages").cloned().unwrap_or(json!(1)),
        "total_results": raw.get("total_results").cloned().unwrap_or(json!(results.len())),
        "results": results
    })))
}

#[derive(Deserialize)]
struct DetailQuery {
    media_type: Option<String>,
}

async fn detail(
    State(state): State<AppState>,
    user: CurrentUser,
    axum::extract::Path(id): axum::extract::Path<i64>,
    Query(q): Query<DetailQuery>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let cfg = tmdb::load_config(&state).await.map_err(|e| {
        AppError::api(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "TMDB_CONFIG",
            e.to_string(),
        )
    })?;
    if !cfg.configured() {
        return Err(AppError::bad_request("TMDB_NOT_CONFIGURED", "请先配置 TMDB API Key"));
    }
    let client = TmdbClient::new(&cfg);
    let mt = q.media_type.unwrap_or_else(|| "tv".into()).to_lowercase();
    let details = if mt == "movie" {
        client.get_movie_details(id).await
    } else {
        client.get_tv_details(id).await
    }
    .map_err(|e| {
        AppError::api(
            axum::http::StatusCode::BAD_GATEWAY,
            "TMDB_HTTP",
            e.to_string(),
        )
    })?
    .ok_or_else(|| AppError::bad_request("TMDB_NOT_FOUND", "未找到该条目"))?;

    let weekdays = tmdb::infer_weekdays_from_details(&details);
    let iso_days: Vec<i64> = weekdays
        .iter()
        .map(|d| if *d == 6 { 7 } else { (*d + 1) as i64 })
        .collect();

    Ok(Json(json!({
        "id": id,
        "media_type": mt,
        "title": tmdb::media_title(&details),
        "year": tmdb::media_year(&details),
        "tmdb_episode_weekdays": iso_days,
        "details": details
    })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/config", get(get_config).patch(patch_config))
        .route("/search", get(search).post(search_post))
        .route("/detail/{id}", get(detail))
}

#[derive(Deserialize)]
struct SearchBody {
    q: Option<String>,
    query: Option<String>,
    year: Option<String>,
    page: Option<u32>,
    media_type: Option<String>,
}

async fn search_post(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<SearchBody>,
) -> AppResult<Json<Value>> {
    search(
        State(state),
        user,
        Query(SearchQuery {
            q: body.q,
            query: body.query,
            year: body.year,
            page: body.page.unwrap_or(1),
            media_type: body.media_type.unwrap_or_else(|| "tv".into()),
        }),
    )
    .await
}
