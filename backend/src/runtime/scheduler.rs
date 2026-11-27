//! Periodic drama/sync runner driven by settings `scheduler_config`.

use crate::runtime::{drama, sync};
use crate::settings_store::{get_or_default, put_typed};
use crate::state::AppState;
use chrono::{Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static TICKING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Minutes between full scans (min 1).
    #[serde(default = "default_interval")]
    pub interval_minutes: u64,
    #[serde(default = "default_tz")]
    pub timezone: String,
    /// Optional hour filter (UTC+8 local hour). Empty = any hour.
    #[serde(default)]
    pub hours: Vec<u32>,
}

fn default_interval() -> u64 {
    120
}
fn default_tz() -> String {
    "Asia/Shanghai".into()
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_minutes: default_interval(),
            timezone: default_tz(),
            hours: vec![],
        }
    }
}

pub async fn load(state: &AppState) -> SchedulerConfig {
    get_or_default(&state.db, "scheduler_config")
        .await
        .unwrap_or_default()
}

pub async fn save(state: &AppState, cfg: &SchedulerConfig) -> anyhow::Result<()> {
    put_typed(&state.db, "scheduler_config", cfg).await?;
    Ok(())
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            if TICKING.swap(true, Ordering::SeqCst) {
                continue;
            }
            let result = run_tick(&state).await;
            TICKING.store(false, Ordering::SeqCst);
            if let Err(e) = result {
                tracing::warn!(error = %e, "scheduler tick failed");
            }
        }
    });
}

async fn run_tick(state: &AppState) -> anyhow::Result<()> {
    let cfg = load(state).await;
    if !cfg.enabled {
        return Ok(());
    }

    let now = Utc::now();
    // Approximate Asia/Shanghai as UTC+8 without chrono-tz dependency
    let local = now + chrono::Duration::hours(8);
    if !cfg.hours.is_empty() && !cfg.hours.contains(&local.hour()) {
        return Ok(());
    }

    let last: Option<String> = sqlx::query_scalar(
        "SELECT value_json FROM app_settings WHERE key = 'scheduler_last_tick'",
    )
    .fetch_optional(&state.db)
    .await?;
    let interval = cfg.interval_minutes.max(1);
    if let Some(raw) = last.as_deref() {
        let ts = raw.trim().trim_matches('"');
        if let Ok(prev) = chrono::DateTime::parse_from_rfc3339(ts) {
            let elapsed = now.signed_duration_since(prev.with_timezone(&Utc));
            if elapsed.num_minutes() < interval as i64 {
                return Ok(());
            }
        }
    }

    put_typed(&state.db, "scheduler_last_tick", &now.to_rfc3339()).await?;
    tracing::info!(
        hour = local.hour(),
        weekday = local.weekday().number_from_monday(),
        "scheduler tick"
    );

    let drama_ids = due_drama_ids(state, local.weekday().number_from_monday() as i64).await?;
    for id in drama_ids {
        tracing::info!(task_id = id, "scheduler run drama");
        match drama::run_once(state, id).await {
            Ok((exec_id, _)) => tracing::info!(task_id = id, exec_id, "drama finished"),
            Err(e) => tracing::warn!(task_id = id, error = %e, "drama failed"),
        }
    }

    let sync_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM sync_tasks WHERE enabled = 1 AND is_running = 0 ORDER BY id ASC LIMIT 20",
    )
    .fetch_all(&state.db)
    .await?;
    for id in sync_ids {
        tracing::info!(sync_task_id = id, "scheduler run sync");
        match sync::run_once(state, id).await {
            Ok((exec_id, _)) => tracing::info!(sync_task_id = id, exec_id, "sync finished"),
            Err(e) => tracing::warn!(sync_task_id = id, error = %e, "sync failed"),
        }
    }

    Ok(())
}

async fn due_drama_ids(state: &AppState, today_iso: i64) -> anyhow::Result<Vec<i64>> {
    let rows = sqlx::query_as::<_, (i64, Option<i64>, Option<String>, String, String)>(
        r#"
        SELECT id, tmdb_id, tmdb_media_type, addition_json, extra_json
        FROM tasks
        WHERE enabled = 1 AND task_type = 'drama'
        ORDER BY sort_index ASC, id ASC
        LIMIT 50
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    let mut out = Vec::new();
    for (id, tmdb_id, media, addition_json, extra_json) in rows {
        let addition: Value = serde_json::from_str(&addition_json).unwrap_or(json!({}));
        let extra: Value = serde_json::from_str(&extra_json).unwrap_or(json!({}));
        if would_run_today(tmdb_id, media.as_deref(), &addition, &extra, today_iso) {
            out.push(id);
        }
    }
    Ok(out)
}

fn weekdays(obj: &Value, key: &str) -> Vec<i64> {
    obj.get(key)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_i64().or_else(|| x.as_u64().map(|u| u as i64)))
                .collect()
        })
        .unwrap_or_default()
}

fn would_run_today(
    tmdb_id: Option<i64>,
    media_type: Option<&str>,
    addition: &Value,
    extra: &Value,
    today_iso: i64,
) -> bool {
    if extra.get("ended").and_then(|v| v.as_bool()).unwrap_or(false) {
        return false;
    }
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
        let mut days = weekdays(addition, "tmdb_episode_weekdays");
        if days.is_empty() {
            days = weekdays(addition, "tmdb_update_weekdays");
        }
        if days.is_empty() {
            days = weekdays(extra, "tmdb_episode_weekdays");
        }
        return !days.is_empty() && days.contains(&today_iso);
    }
    let runweek = match extra.get("runweek") {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|x| x.as_i64())
            .filter(|d| (1..=7).contains(d))
            .collect::<Vec<_>>(),
        Some(Value::String(s)) => s
            .split([',', ' ', '|'])
            .filter_map(|p| p.trim().parse().ok())
            .filter(|d: &i64| (1..=7).contains(d))
            .collect(),
        _ => vec![],
    };
    runweek.is_empty() || runweek.contains(&today_iso)
}
