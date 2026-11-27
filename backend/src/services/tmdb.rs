//! Minimal TMDB HTTP client for drama rename / weekday hints.

use crate::settings_store::get_or_default;
use crate::state::AppState;
use chrono::Datelike;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_lang")]
    pub language: String,
    #[serde(default = "default_lang")]
    pub poster_language: String,
    #[serde(default)]
    pub disable_guessit_tmdb_fallback_rename: bool,
    #[serde(default)]
    pub guessit_tmdb_tv_rename_template: String,
    #[serde(default)]
    pub guessit_tmdb_movie_rename_template: String,
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

impl TmdbConfig {
    pub fn configured(&self) -> bool {
        !self.api_key.trim().is_empty()
    }
}

pub async fn load_config(state: &AppState) -> anyhow::Result<TmdbConfig> {
    Ok(get_or_default(&state.db, "tmdb_config").await?)
}

pub struct TmdbClient {
    api_key: String,
    language: String,
    http: reqwest::Client,
}

impl TmdbClient {
    pub fn new(cfg: &TmdbConfig) -> Self {
        Self {
            api_key: cfg.api_key.trim().to_string(),
            language: if cfg.language.trim().is_empty() {
                default_lang()
            } else {
                cfg.language.trim().to_string()
            },
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(12))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    pub fn is_configured(&self) -> bool {
        !self.api_key.is_empty()
    }

    async fn get(&self, path: &str, extra: &[(&str, String)]) -> anyhow::Result<Option<Value>> {
        if !self.is_configured() {
            return Ok(None);
        }
        let bases = [
            "https://api.tmdb.org/3",
            "https://api.themoviedb.org/3",
        ];
        let mut last_err = None;
        for base in bases {
            let mut req = self
                .http
                .get(format!("{base}{path}"))
                .query(&[
                    ("api_key", self.api_key.as_str()),
                    ("language", self.language.as_str()),
                    ("include_adult", "false"),
                ]);
            for (k, v) in extra {
                req = req.query(&[(k, v.as_str())]);
            }
            match req.send().await {
                Ok(resp) if resp.status().is_success() => {
                    let v = resp.json::<Value>().await.unwrap_or(Value::Null);
                    return Ok(Some(v));
                }
                Ok(resp) => {
                    last_err = Some(anyhow::anyhow!("tmdb status {}", resp.status()));
                }
                Err(e) => last_err = Some(e.into()),
            }
        }
        if let Some(e) = last_err {
            tracing::debug!("tmdb request failed: {e}");
        }
        Ok(None)
    }

    pub async fn get_tv_details(&self, tv_id: i64) -> anyhow::Result<Option<Value>> {
        self.get(&format!("/tv/{tv_id}"), &[]).await
    }

    pub async fn get_movie_details(&self, movie_id: i64) -> anyhow::Result<Option<Value>> {
        self.get(&format!("/movie/{movie_id}"), &[]).await
    }

    pub async fn search_tv(&self, query: &str, year: Option<&str>, page: u32) -> anyhow::Result<Option<Value>> {
        let mut extra = vec![
            ("query", query.to_string()),
            ("page", page.max(1).to_string()),
        ];
        if let Some(y) = year.filter(|s| !s.is_empty()) {
            extra.push(("first_air_date_year", y.to_string()));
        }
        self.get("/search/tv", &extra).await
    }

    pub async fn search_movie(
        &self,
        query: &str,
        year: Option<&str>,
        page: u32,
    ) -> anyhow::Result<Option<Value>> {
        let mut extra = vec![
            ("query", query.to_string()),
            ("page", page.max(1).to_string()),
        ];
        if let Some(y) = year.filter(|s| !s.is_empty()) {
            extra.push(("year", y.to_string()));
        }
        self.get("/search/movie", &extra).await
    }

    pub async fn search_multi(&self, query: &str, page: u32) -> anyhow::Result<Option<Value>> {
        self.get(
            "/search/multi",
            &[
                ("query", query.to_string()),
                ("page", page.max(1).to_string()),
            ],
        )
        .await
    }
}

/// Prefer localized name, then original_name / title.
pub fn media_title(details: &Value) -> Option<String> {
    for key in ["name", "title", "original_name", "original_title"] {
        if let Some(s) = details.get(key).and_then(|v| v.as_str()) {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

pub fn media_year(details: &Value) -> Option<i32> {
    for key in ["first_air_date", "release_date"] {
        if let Some(s) = details.get(key).and_then(|v| v.as_str()) {
            if s.len() >= 4 {
                if let Ok(y) = s[..4].parse::<i32>() {
                    if y > 1900 {
                        return Some(y);
                    }
                }
            }
        }
    }
    None
}

/// Infer weekday list (0=Mon … 6=Sun) from recent episode air dates in season summaries.
pub fn infer_weekdays_from_details(details: &Value) -> Vec<i32> {
    let mut samples: Vec<i32> = Vec::new();
    if let Some(arr) = details.get("seasons").and_then(|v| v.as_array()) {
        for season in arr.iter().rev().take(3) {
            // season objects rarely include air_date only; still collect
            if let Some(d) = season.get("air_date").and_then(|v| v.as_str()) {
                if let Some(wd) = weekday_from_ymd(d) {
                    samples.push(wd);
                }
            }
        }
    }
    // last_episode_to_air / next_episode_to_air
    for key in ["last_episode_to_air", "next_episode_to_air"] {
        if let Some(ep) = details.get(key) {
            if let Some(d) = ep.get("air_date").and_then(|v| v.as_str()) {
                if let Some(wd) = weekday_from_ymd(d) {
                    samples.push(wd);
                }
            }
        }
    }
    if samples.is_empty() {
        return vec![];
    }
    // majority vote
    let mut counts = [0i32; 7];
    for s in &samples {
        if (0..7).contains(s) {
            counts[*s as usize] += 1;
        }
    }
    let max = *counts.iter().max().unwrap_or(&0);
    if max <= 0 {
        return vec![];
    }
    counts
        .iter()
        .enumerate()
        .filter(|(_, c)| **c == max)
        .map(|(i, _)| i as i32)
        .collect()
}

fn weekday_from_ymd(s: &str) -> Option<i32> {
    // YYYY-MM-DD → chrono weekday Mon=0
    let parts: Vec<_> = s.split('-').collect();
    if parts.len() < 3 {
        return None;
    }
    let y: i32 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    let d: u32 = parts[2].parse().ok()?;
    let date = chrono::NaiveDate::from_ymd_opt(y, m, d)?;
    // chrono: Mon=0 … Sun=6 via number_from_monday
    Some(date.weekday().num_days_from_monday() as i32)
}
