use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::AppResult;
use crate::settings_store::{get_or_default, put_typed};
use crate::state::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TmdbConfig {
    #[serde(default)]
    api_key: String,
    #[serde(default = "default_lang")]
    language: String,
    #[serde(default = "default_lang")]
    poster_language: String,
    #[serde(default)]
    disable_guessit_tmdb_fallback_rename: bool,
    #[serde(default)]
    guessit_tmdb_tv_rename_template: String,
    #[serde(default)]
    guessit_tmdb_movie_rename_template: String,
}

fn default_lang() -> String {
    "zh-CN".into()
}

impl Default for TmdbConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            language: default_lang(),
            poster_language: default_lang(),
            disable_guessit_tmdb_fallback_rename: false,
            guessit_tmdb_tv_rename_template: String::new(),
            guessit_tmdb_movie_rename_template: String::new(),
        }
    }
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
        cfg.language = v;
    }
    if let Some(v) = payload.poster_language {
        cfg.poster_language = v;
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
    Ok(Json(to_out(&cfg)))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/config", get(get_config).patch(patch_config))
}
