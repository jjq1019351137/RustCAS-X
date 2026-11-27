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
            },
            "wxpusher": {
                "enabled": false,
                "app_token": "",
                "uids": "",
                "topic_ids": ""
            }
        }
    })
}

async fn send_telegram(bot_token: &str, chat_id: &str, title: &str, content: &str) -> (bool, Option<String>) {
    let token = bot_token.trim();
    let chat = chat_id.trim();
    if token.is_empty() || chat.is_empty() {
        return (false, Some("telegram bot_token/chat_id 未配置".into()));
    }
    let text = format!("*{title}*\n\n{content}");
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let body = json!({
        "chat_id": chat,
        "text": text,
        "parse_mode": "Markdown"
    });
    match reqwest::Client::new()
        .post(&url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(12))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => {
            let v: Value = r.json().await.unwrap_or(Value::Null);
            if v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false) {
                (true, None)
            } else {
                (false, Some(v.to_string()))
            }
        }
        Ok(r) => (false, Some(format!("HTTP {}", r.status()))),
        Err(e) => (false, Some(e.to_string())),
    }
}

async fn send_wxpusher(app_token: &str, uids: &str, topic_ids: &str, title: &str, content: &str) -> (bool, Option<String>) {
    let token = app_token.trim();
    if token.is_empty() {
        return (false, Some("wxpusher app_token 未配置".into()));
    }
    let mut payload = json!({
        "appToken": token,
        "content": format!("{title}\n\n{content}"),
        "contentType": 1
    });
    let uid_list: Vec<String> = uids
        .split([';', ','])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let topic_list: Vec<i64> = topic_ids
        .split([';', ','])
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    if uid_list.is_empty() && topic_list.is_empty() {
        return (false, Some("wxpusher uids/topic_ids 为空".into()));
    }
    if !uid_list.is_empty() {
        payload["uids"] = json!(uid_list);
    }
    if !topic_list.is_empty() {
        payload["topicIds"] = json!(topic_list);
    }
    match reqwest::Client::new()
        .post("https://wxpusher.zjiecode.com/api/send/message")
        .json(&payload)
        .timeout(std::time::Duration::from_secs(12))
        .send()
        .await
    {
        Ok(r) => {
            let v: Value = r.json().await.unwrap_or(Value::Null);
            if v.get("code").and_then(|x| x.as_i64()) == Some(1000) {
                (true, None)
            } else {
                (false, Some(v.to_string()))
            }
        }
        Err(e) => (false, Some(e.to_string())),
    }
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
        for key in ["webhook", "telegram", "wxpusher"] {
            if setting
                .config
                .pointer(&format!("/channels/{key}/enabled"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                out.push(key.to_string());
            }
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
                let enabled = setting
                    .config
                    .pointer("/channels/telegram/enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let token = setting
                    .config
                    .pointer("/channels/telegram/bot_token")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let chat_id = setting
                    .config
                    .pointer("/channels/telegram/chat_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !enabled {
                    results.push(json!({
                        "channel": "telegram",
                        "ok": false,
                        "error": "telegram 未启用"
                    }));
                } else {
                    let (ok, err) =
                        send_telegram(token, chat_id, &payload.title, &payload.content).await;
                    results.push(json!({ "channel": "telegram", "ok": ok, "error": err }));
                }
            }
            "wxpusher" => {
                let enabled = setting
                    .config
                    .pointer("/channels/wxpusher/enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let token = setting
                    .config
                    .pointer("/channels/wxpusher/app_token")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let uids = setting
                    .config
                    .pointer("/channels/wxpusher/uids")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let topics = setting
                    .config
                    .pointer("/channels/wxpusher/topic_ids")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !enabled {
                    results.push(json!({
                        "channel": "wxpusher",
                        "ok": false,
                        "error": "wxpusher 未启用"
                    }));
                } else {
                    let (ok, err) =
                        send_wxpusher(token, uids, topics, &payload.title, &payload.content).await;
                    results.push(json!({ "channel": "wxpusher", "ok": ok, "error": err }));
                }
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
