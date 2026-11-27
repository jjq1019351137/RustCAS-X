//! Runtime notification sender (webhook / telegram / wxpusher).
use crate::settings_store::get_or_default;
use crate::state::AppState;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, serde::Serialize, Deserialize, Default)]
struct NotificationSetting {
    #[serde(default)]
    config: Value,
}

fn channel_enabled(cfg: &Value, key: &str) -> bool {
    cfg.pointer(&format!("/channels/{key}/enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || cfg
            .get("__channel_enabled")
            .and_then(|v| v.get(key))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
}

async fn send_webhook(url: &str, title: &str, content: &str) -> bool {
    let body = json!({ "title": title, "content": content, "source": "rustcas-x" });
    match reqwest::Client::new()
        .post(url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
    {
        Ok(r) => r.status().is_success(),
        Err(_) => false,
    }
}

async fn send_telegram(token: &str, chat_id: &str, title: &str, content: &str) -> bool {
    if token.trim().is_empty() || chat_id.trim().is_empty() {
        return false;
    }
    let url = format!("https://api.telegram.org/bot{}/sendMessage", token.trim());
    let body = json!({
        "chat_id": chat_id.trim(),
        "text": format!("*{title}*\n\n{content}"),
        "parse_mode": "Markdown"
    });
    match reqwest::Client::new()
        .post(url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(12))
        .send()
        .await
    {
        Ok(r) => {
            let v: Value = r.json().await.unwrap_or(Value::Null);
            v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false)
        }
        Err(_) => false,
    }
}

async fn send_wxpusher(app_token: &str, uids: &str, topics: &str, title: &str, content: &str) -> bool {
    if app_token.trim().is_empty() {
        return false;
    }
    let mut payload = json!({
        "appToken": app_token.trim(),
        "content": format!("{title}\n\n{content}"),
        "contentType": 1
    });
    let uid_list: Vec<String> = uids
        .split([';', ','])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let topic_list: Vec<i64> = topics
        .split([';', ','])
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    if uid_list.is_empty() && topic_list.is_empty() {
        return false;
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
            v.get("code").and_then(|x| x.as_i64()) == Some(1000)
        }
        Err(_) => false,
    }
}

/// Fire-and-forget friendly: errors are logged, never fatal to task runtime.
pub async fn send_runtime(state: &AppState, title: &str, content: &str) {
    let setting: NotificationSetting = get_or_default(&state.db, "notification_config")
        .await
        .unwrap_or_default();
    let cfg = &setting.config;
    let globally_enabled = cfg
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    if !globally_enabled {
        return;
    }

    if channel_enabled(cfg, "webhook") {
        if let Some(url) = cfg
            .pointer("/channels/webhook/url")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            let ok = send_webhook(url, title, content).await;
            tracing::debug!(ok, "notify webhook");
        }
    }
    if channel_enabled(cfg, "telegram") {
        let token = cfg
            .pointer("/channels/telegram/bot_token")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let chat = cfg
            .pointer("/channels/telegram/chat_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let ok = send_telegram(token, chat, title, content).await;
        tracing::debug!(ok, "notify telegram");
    }
    if channel_enabled(cfg, "wxpusher") {
        let token = cfg
            .pointer("/channels/wxpusher/app_token")
            .or_else(|| cfg.get("WXPUSHER_APP_TOKEN"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let uids = cfg
            .pointer("/channels/wxpusher/uids")
            .or_else(|| cfg.get("WXPUSHER_UIDS"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let topics = cfg
            .pointer("/channels/wxpusher/topic_ids")
            .or_else(|| cfg.get("WXPUSHER_TOPIC_IDS"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let ok = send_wxpusher(token, uids, topics, title, content).await;
        tracing::debug!(ok, "notify wxpusher");
    }
}
