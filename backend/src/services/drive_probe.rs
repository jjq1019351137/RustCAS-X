//! Drive account probe + scheduled refresh.
use crate::adapters::create_adapter;
use crate::settings_store::{get_or_default, put_typed};
use crate::state::AppState;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeSchedulerConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_crontab")]
    pub crontab: String,
    #[serde(default = "default_tz")]
    pub timezone: String,
    #[serde(default = "default_enabled_only")]
    pub enabled_only: bool,
}

fn default_crontab() -> String {
    "0 */6 * * *".into()
}
fn default_tz() -> String {
    "Asia/Shanghai".into()
}
fn default_enabled_only() -> bool {
    true
}

impl Default for ProbeSchedulerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            crontab: default_crontab(),
            timezone: default_tz(),
            enabled_only: true,
        }
    }
}

pub fn interval_minutes(crontab: &str) -> u64 {
    let parts: Vec<&str> = crontab.split_whitespace().collect();
    if parts.len() >= 2 {
        let hour = parts[1];
        if let Some(n) = hour.strip_prefix("*/").and_then(|s| s.parse::<u64>().ok()) {
            return n.max(1) * 60;
        }
    }
    360
}

pub async fn probe_account(state: &AppState, account_id: i64) -> anyhow::Result<bool> {
    let row = sqlx::query_as::<_, (String, String, Option<String>)>(
        "SELECT drive_type, cookie, config_json FROM drive_accounts WHERE id = ?",
    )
    .bind(account_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| anyhow::anyhow!("account not found"))?;

    let config: Value = row
        .2
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| json!({}));
    let mut adapter = create_adapter(&row.0, Some(&config), Some(&row.1))?;
    let init = adapter
        .init()
        .await
        .unwrap_or_else(|e| json!({ "ok": false, "message": e.to_string() }));
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
    Ok(ok)
}

pub async fn run_due_probes(state: &AppState) -> anyhow::Result<usize> {
    let cfg: ProbeSchedulerConfig = get_or_default(&state.db, "drive_probe_scheduler")
        .await
        .unwrap_or_default();
    if !cfg.enabled {
        return Ok(0);
    }
    let now = Utc::now();
    let last: Option<String> = sqlx::query_scalar(
        "SELECT value_json FROM app_settings WHERE key = 'drive_probe_last_tick'",
    )
    .fetch_optional(&state.db)
    .await?;
    let interval = interval_minutes(&cfg.crontab);
    if let Some(raw) = last.as_deref() {
        let ts = raw.trim().trim_matches('"');
        if let Ok(prev) = chrono::DateTime::parse_from_rfc3339(ts) {
            let elapsed = now.signed_duration_since(prev.with_timezone(&Utc));
            if elapsed.num_minutes() < interval as i64 {
                return Ok(0);
            }
        }
    }
    put_typed(&state.db, "drive_probe_last_tick", &now.to_rfc3339()).await?;

    let sql = if cfg.enabled_only {
        "SELECT id FROM drive_accounts WHERE enabled = 1 ORDER BY id ASC"
    } else {
        "SELECT id FROM drive_accounts ORDER BY id ASC"
    };
    let ids: Vec<i64> = sqlx::query_scalar(sql).fetch_all(&state.db).await?;
    let mut n = 0usize;
    for id in ids {
        match probe_account(state, id).await {
            Ok(_) => n += 1,
            Err(e) => tracing::warn!(account_id = id, error = %e, "scheduled probe failed"),
        }
    }
    Ok(n)
}
