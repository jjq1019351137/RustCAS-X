use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::AppResult;
use crate::settings_store::{get_or_default, put_typed};
use crate::state::AppState;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
struct NotificationSetting {
    #[serde(default = "default_config")]
    config: Value,
    #[serde(default)]
    updated_at: Option<String>,
}

fn default_config() -> Value {
    json!({
        "enabled": false,
        "channels": {
            "webhook": {
                "enabled": false,
                "url": ""
            },
            "telegram": {
                "enabled": false,
                "bot_token": "",
                "chat_id": ""
            }
        }
    })
}

impl Default for NotificationSetting {
    fn default() -> Self {
        Self {
            config: default_config(),
            updated_at: None,
        }
    }
}

#[derive(Deserialize, Default)]
struct ConfigPatchIn {
    config: Option<Value>,
}

#[derive(Deserialize)]
struct TestIn {
    title: String,
    content: String,
    channels: Option<Vec<String>>,
}

async fn get_config(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Value>> {
    require_perm(&user, "notify:read").or_else(|_| require_perm(&user, "task:read"))?;
    let setting: NotificationSetting = get_or_default(&state.db, "notification_config")
        .await
        .unwrap_or_default();
    Ok(Json(json!({
        "config": setting.config,
        "default_config": default_config(),
        "updated_at": setting.updated_at
    })))
}

async fn patch_config(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<ConfigPatchIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "notify:write").or_else(|_| require_perm(&user, "task:write"))?;
    let mut setting: NotificationSetting = get_or_default(&state.db, "notification_config")
        .await
        .unwrap_or_default();
    if let Some(cfg) = payload.config {
        setting.config = cfg;
    }
    setting.updated_at = Some(Utc::now().to_rfc3339());
    put_typed(&state.db, "notification_config", &setting).await?;
    Ok(Json(json!({
        "config": setting.config,
        "default_config": default_config(),
        "updated_at": setting.updated_at
    })))
}

async fn test_notify(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<TestIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "notify:write").or_else(|_| require_perm(&user, "task:write"))?;
    let setting: NotificationSetting = get_or_default(&state.db, "notification_config")
        .await
        .unwrap_or_default();
    let channels = payload.channels.unwrap_or_else(|| {
        let mut out = Vec::new();
        if setting
            .config
            .pointer("/channels/webhook/enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            out.push("webhook".into());
        }
        if setting
            .config
            .pointer("/channels/telegram/enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            out.push("telegram".into());
        }
        if out.is_empty() {
            out.push("webhook".into());
        }
        out
    });

    let mut results = Vec::new();
    for ch in channels {
        match ch.as_str() {
            "webhook" => {
                let url = setting
                    .config
                    .pointer("/channels/webhook/url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                let enabled = setting
                    .config
                    .pointer("/channels/webhook/enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if !enabled || url.is_empty() {
                    results.push(json!({
                        "channel": "webhook",
                        "ok": false,
                        "error": "webhook 未启用或 URL 为空"
                    }));
                    continue;
                }
                let body = json!({
                    "title": payload.title,
                    "content": payload.content,
                    "source": "rustcas-x-test"
                });
                let resp = reqwest::Client::new()
                    .post(url)
                    .json(&body)
                    .timeout(std::time::Duration::from_secs(10))
                    .send()
                    .await;
                match resp {
                    Ok(r) if r.status().is_success() => {
                        results.push(json!({ "channel": "webhook", "ok": true, "error": null }))
                    }
                    Ok(r) => results.push(json!({
                        "channel": "webhook",
                        "ok": false,
                        "error": format!("HTTP {}", r.status())
                    })),
                    Err(e) => results.push(json!({
                        "channel": "webhook",
                        "ok": false,
                        "error": e.to_string()
                    })),
                }
            }
            "telegram" => {
                results.push(json!({
                    "channel": "telegram",
                    "ok": false,
                    "error": "telegram sender 尚未完整移植，请先用 webhook"
                }));
            }
            other => results.push(json!({
                "channel": other,
                "ok": false,
                "error": "unsupported channel"
            })),
        }
    }
    Ok(Json(json!({ "results": results })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/config", get(get_config).patch(patch_config))
        .route("/test", post(test_notify))
}
