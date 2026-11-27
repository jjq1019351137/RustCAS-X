use crate::adapters::create_adapter;
use crate::auth_ctx::CurrentUser;
use crate::error::AppResult;
use crate::runtime::scheduler;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct DramaQuery {
    #[serde(default = "default_days")]
    days: i64,
}

fn default_days() -> i64 {
    30
}

async fn capacity_overview(
    State(state): State<AppState>,
    _user: CurrentUser,
) -> AppResult<Json<Value>> {
    let rows = sqlx::query_as::<_, (i64, String, String, String, Option<String>, i64)>(
        r#"
        SELECT id, name, drive_type, cookie, config_json, enabled
        FROM drive_accounts
        ORDER BY is_default DESC, id ASC
        "#,
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let account_count = rows.len() as i64;
    let enabled_account_count = rows.iter().filter(|r| r.5 != 0).count() as i64;
    let mut accounts = Vec::new();
    let mut warning_accounts = Vec::new();
    let mut unsupported = Vec::new();
    let mut total_used: i64 = 0;
    let mut total_space: i64 = 0;
    let mut capacity_count = 0i64;
    let mut warning_count = 0i64;

    for (id, name, drive_type, cookie, config_json, enabled) in rows {
        if drive_type != "quark" {
            unsupported.push(json!({
                "id": id,
                "name": name,
                "drive_type": drive_type,
                "reason": "capacity probe focuses on quark"
            }));
            continue;
        }
        if enabled == 0 {
            continue;
        }
        let config = config_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(json!({}));
        let mut adapter = match create_adapter(&drive_type, Some(&config), Some(&cookie)) {
            Ok(a) => a,
            Err(e) => {
                accounts.push(json!({
                    "id": id,
                    "name": name,
                    "drive_type": drive_type,
                    "ok": false,
                    "error": e.to_string()
                }));
                continue;
            }
        };
        match adapter.init().await {
            Ok(info) => {
                let used = info
                    .get("used_space")
                    .and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|u| u as i64)))
                    .unwrap_or(0);
                let total = info
                    .get("total_space")
                    .and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|u| u as i64)))
                    .unwrap_or(0);
                let ratio = if total > 0 {
                    Some((used as f64) / (total as f64))
                } else {
                    None
                };
                let warn = ratio.map(|r| r >= 0.85).unwrap_or(false);
                if total > 0 {
                    capacity_count += 1;
                    total_used += used;
                    total_space += total;
                }
                let row = json!({
                    "id": id,
                    "name": name,
                    "drive_type": drive_type,
                    "ok": info.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
                    "nickname": info.get("nickname"),
                    "used_space": used,
                    "total_space": total,
                    "usage_ratio": ratio,
                    "member_type": info.get("member_type"),
                    "warning": warn
                });
                if warn {
                    warning_count += 1;
                    warning_accounts.push(row.clone());
                }
                accounts.push(row);
            }
            Err(e) => {
                accounts.push(json!({
                    "id": id,
                    "name": name,
                    "drive_type": drive_type,
                    "ok": false,
                    "error": e.to_string()
                }));
            }
        }
    }

    let usage_ratio = if total_space > 0 {
        Some((total_used as f64) / (total_space as f64))
    } else {
        None
    };

    Ok(Json(json!({
        "summary": {
            "account_count": account_count,
            "enabled_account_count": enabled_account_count,
            "capacity_account_count": capacity_count,
            "warning_account_count": warning_count,
            "total_used_space": if capacity_count > 0 { Value::from(total_used) } else { Value::Null },
            "total_space": if capacity_count > 0 { Value::from(total_space) } else { Value::Null },
            "usage_ratio": usage_ratio
        },
        "accounts": accounts,
        "warning_accounts": warning_accounts,
        "unsupported_accounts": unsupported,
        "updated_at": Utc::now().to_rfc3339()
    })))
}

async fn drama_overview(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(q): Query<DramaQuery>,
) -> AppResult<Json<Value>> {
    let days = q.days.clamp(1, 365);
    let task_count: i64 = sqlx::query_scalar("SELECT COUNT(1) FROM tasks")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);
    let enabled_task_count: i64 =
        sqlx::query_scalar("SELECT COUNT(1) FROM tasks WHERE enabled = 1")
            .fetch_one(&state.db)
            .await
            .unwrap_or(0);
    let tmdb_bound_count: i64 =
        sqlx::query_scalar("SELECT COUNT(1) FROM tasks WHERE tmdb_id IS NOT NULL AND tmdb_id > 0")
            .fetch_one(&state.db)
            .await
            .unwrap_or(0);

    let exec_total: i64 = sqlx::query_scalar(
        "SELECT COUNT(1) FROM task_executions WHERE started_at >= datetime('now', ?)",
    )
    .bind(format!("-{days} days"))
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);
    let exec_success: i64 = sqlx::query_scalar(
        "SELECT COUNT(1) FROM task_executions WHERE status='success' AND started_at >= datetime('now', ?)",
    )
    .bind(format!("-{days} days"))
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);
    let exec_failed: i64 = sqlx::query_scalar(
        "SELECT COUNT(1) FROM task_executions WHERE status='failed' AND started_at >= datetime('now', ?)",
    )
    .bind(format!("-{days} days"))
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);
    let exec_skipped: i64 = sqlx::query_scalar(
        "SELECT COUNT(1) FROM task_executions WHERE status='skipped' AND started_at >= datetime('now', ?)",
    )
    .bind(format!("-{days} days"))
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let success_rate = if exec_total > 0 {
        Some((exec_success as f64) / (exec_total as f64))
    } else {
        None
    };

    let sched = scheduler::load(&state).await;
    Ok(Json(json!({
        "scheduler": {
            "enabled": sched.enabled,
            "crontab": format!("every {}m", sched.interval_minutes.max(1)),
            "interval_minutes": sched.interval_minutes,
            "timezone": sched.timezone,
            "hours": sched.hours
        },
        "summary": {
            "task_count": task_count,
            "enabled_task_count": enabled_task_count,
            "tmdb_bound_count": tmdb_bound_count,
            "banned_count": 0,
            "unknown_schedule_count": 0,
            "monthly_success_count": exec_success,
            "window_days": days,
            "execution_total": exec_total,
            "execution_success": exec_success,
            "execution_failed": exec_failed,
            "execution_skipped": exec_skipped,
            "success_rate": success_rate,
            "avg_duration_s": null
        },
        "trend": [],
        "recent_failures": [],
        "updated_at": Utc::now().to_rfc3339()
    })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/capacity-overview", get(capacity_overview))
        .route("/drama-overview", get(drama_overview))
}
