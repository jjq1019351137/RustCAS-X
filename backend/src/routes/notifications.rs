use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::AppResult;
use crate::services::audit;
use crate::services::notify::{self, channel_enabled, cfg_str};
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

/// Frontend (CAS Vue) uses flat WxPusher keys + `__channel_enabled`.
/// Nested `channels.*` is kept for webhook / telegram / runtime dual-read.
fn default_config() -> Value {
    json!({
        "enabled": true,
        "HITOKOTO": false,
        "WXPUSHER_APP_TOKEN": "",
        "WXPUSHER_TOPIC_IDS": "",
        "WXPUSHER_UIDS": "",
        "__channel_enabled": {
            "wxpusher": false,
            "webhook": false,
            "telegram": false
        },
        "channels": {
            "webhook": {
                "enabled": false,
                "url": "",
                "method": "POST",
                "headers": {},
                "bearer_token": ""
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

/// Mirror flat ↔ nested so either UI / runtime shape works after save.
fn sync_channel_shapes(cfg: &mut Value) {
    if !cfg.is_object() {
        return;
    }

    if cfg.get("__channel_enabled").is_none() {
        cfg.as_object_mut()
            .unwrap()
            .insert("__channel_enabled".into(), json!({}));
    }
    if cfg.get("channels").is_none() {
        cfg.as_object_mut().unwrap().insert(
            "channels".into(),
            default_config()
                .get("channels")
                .cloned()
                .unwrap_or(json!({})),
        );
    }

    let nested_token = cfg
        .pointer("/channels/wxpusher/app_token")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let flat_token = cfg
        .get("WXPUSHER_APP_TOKEN")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let flat_uids = cfg
        .get("WXPUSHER_UIDS")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let flat_topics = cfg
        .get("WXPUSHER_TOPIC_IDS")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let wx_enabled = cfg
        .pointer("/__channel_enabled/wxpusher")
        .and_then(|v| v.as_bool())
        .or_else(|| {
            cfg.pointer("/channels/wxpusher/enabled")
                .and_then(|v| v.as_bool())
        })
        .unwrap_or(false);
    let enabled_map = cfg
        .get("__channel_enabled")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    let obj = cfg.as_object_mut().unwrap();
    if flat_token.is_empty() && !nested_token.is_empty() {
        obj.insert("WXPUSHER_APP_TOKEN".into(), json!(nested_token));
    }
    if let Some(ch) = obj
        .get_mut("channels")
        .and_then(|c| c.get_mut("wxpusher"))
        .and_then(|c| c.as_object_mut())
    {
        if !flat_token.is_empty() {
            ch.insert("app_token".into(), json!(flat_token));
        }
        if !flat_uids.is_empty() {
            ch.insert("uids".into(), json!(flat_uids));
        }
        if !flat_topics.is_empty() {
            ch.insert("topic_ids".into(), json!(flat_topics));
        }
        ch.insert("enabled".into(), json!(wx_enabled));
    }
    if let Some(map) = obj
        .get_mut("__channel_enabled")
        .and_then(|v| v.as_object_mut())
    {
        map.insert("wxpusher".into(), json!(wx_enabled));
    }
    for (key, val) in enabled_map {
        if let Some(ch) = obj
            .get_mut("channels")
            .and_then(|c| c.get_mut(&key))
            .and_then(|c| c.as_object_mut())
        {
            if let Some(b) = val.as_bool() {
                ch.insert("enabled".into(), json!(b));
            }
        }
    }
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
    let mut setting: NotificationSetting = get_or_default(&state.db, "notification_config")
        .await
        .unwrap_or_default();
    sync_channel_shapes(&mut setting.config);
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
    if let Some(mut cfg) = payload.config {
        sync_channel_shapes(&mut cfg);
        setting.config = cfg;
    }
    setting.updated_at.replace(Utc::now().to_rfc3339());
    put_typed(&state.db, "notification_config", &setting).await?;
    audit::write_user(
        &state.db,
        user.id,
        "notification.config.update",
        "notification",
        "config",
        None,
    )
    .await;
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
    let mut setting: NotificationSetting = get_or_default(&state.db, "notification_config")
        .await
        .unwrap_or_default();
    sync_channel_shapes(&mut setting.config);
    let cfg = &setting.config;
    let channels = payload.channels.unwrap_or_else(|| {
        let mut out = Vec::new();
        for key in ["webhook", "telegram", "wxpusher"] {
            if channel_enabled(cfg, key) {
                out.push(key.to_string());
            }
        }
        if out.is_empty() {
            out.push("wxpusher".into());
        }
        out
    });

    let mut results = Vec::new();
    for ch in channels {
        match ch.as_str() {
            "webhook" => {
                if !channel_enabled(cfg, "webhook") {
                    results.push(json!({
                        "channel": "webhook",
                        "ok": false,
                        "error": "webhook 未启用"
                    }));
                    continue;
                }
                let ch = cfg
                    .pointer("/channels/webhook")
                    .cloned()
                    .unwrap_or(json!({}));
                let (ok, error) = notify::send_webhook_channel(
                    &ch,
                    &payload.title,
                    &payload.content,
                    "rustcas-x-test",
                )
                .await;
                results.push(json!({
                    "channel": "webhook",
                    "ok": ok,
                    "error": error
                }));
            }
            "telegram" => {
                let token = cfg_str(cfg, "/channels/telegram/bot_token", "TELEGRAM_BOT_TOKEN");
                let chat_id = cfg_str(cfg, "/channels/telegram/chat_id", "TELEGRAM_CHAT_ID");
                if !channel_enabled(cfg, "telegram") {
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
                let token = cfg_str(cfg, "/channels/wxpusher/app_token", "WXPUSHER_APP_TOKEN");
                let uids = cfg_str(cfg, "/channels/wxpusher/uids", "WXPUSHER_UIDS");
                let topics = cfg_str(cfg, "/channels/wxpusher/topic_ids", "WXPUSHER_TOPIC_IDS");
                if !channel_enabled(cfg, "wxpusher") {
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
