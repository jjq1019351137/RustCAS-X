//! Quark-first resource search: PanSou (+ CloudSaver skeleton).
use crate::services::drive_util::detect_drive_type;
use crate::services::share_preview::{preview_share_batch, SharePreviewBatchIn};
use chrono::Utc;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Deserialize)]
pub struct SourcePatch {
    pub enabled: Option<bool>,
    pub server: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub token: Option<String>,
}

async fn ensure_defaults(db: &SqlitePool) -> anyhow::Result<()> {
    for key in ["pansou", "cloudsaver", "net"] {
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO resource_search_sources (key, enabled, config_json, updated_at)
            VALUES (?, 0, '{}', ?)
            "#,
        )
        .bind(key)
        .bind(Utc::now().to_rfc3339())
        .execute(db)
        .await?;
    }
    Ok(())
}

pub async fn list_sources(db: &SqlitePool) -> anyhow::Result<Value> {
    ensure_defaults(db).await?;
    let rows = sqlx::query_as::<_, (String, i64, String)>(
        "SELECT key, enabled, config_json FROM resource_search_sources ORDER BY key",
    )
    .fetch_all(db)
    .await?;
    let mut sources = Vec::new();
    for (key, enabled, config_json) in rows {
        let cfg: Value = serde_json::from_str(&config_json).unwrap_or(json!({}));
        sources.push(json!({
            "key": key,
            "enabled": enabled != 0,
            "server": cfg.get("server").and_then(|v| v.as_str()),
            "username": cfg.get("username").and_then(|v| v.as_str()),
            "password": Value::Null,
            "token": if key == "cloudsaver" {
                // never echo secrets
                if cfg.get("token").and_then(|v| v.as_str()).unwrap_or("").is_empty() {
                    Value::Null
                } else {
                    json!("***")
                }
            } else {
                Value::Null
            }
        }));
    }
    Ok(json!({ "sources": sources }))
}

pub async fn patch_source(db: &SqlitePool, key: &str, payload: SourcePatch) -> anyhow::Result<Value> {
    ensure_defaults(db).await?;
    let key = key.trim();
    if !matches!(key, "pansou" | "cloudsaver" | "net") {
        anyhow::bail!("不支持的搜索源");
    }
    let (enabled, config_json): (i64, String) = sqlx::query_as(
        "SELECT enabled, config_json FROM resource_search_sources WHERE key = ?",
    )
    .bind(key)
    .fetch_one(db)
    .await?;
    let mut cfg: Value = serde_json::from_str(&config_json).unwrap_or(json!({}));
    let obj = cfg.as_object_mut().unwrap();
    if let Some(v) = payload.server {
        obj.insert("server".into(), json!(v.trim()));
    }
    if let Some(v) = payload.username {
        obj.insert("username".into(), json!(v.trim()));
    }
    if let Some(v) = payload.password {
        if !v.trim().is_empty() {
            obj.insert("password".into(), json!(v));
        }
    }
    if let Some(v) = payload.token {
        if !v.trim().is_empty() {
            obj.insert("token".into(), json!(v));
        }
    }
    let enabled = payload.enabled.map(|b| if b { 1 } else { 0 }).unwrap_or(enabled);
    sqlx::query(
        "UPDATE resource_search_sources SET enabled=?, config_json=?, updated_at=? WHERE key=?",
    )
    .bind(enabled)
    .bind(cfg.to_string())
    .bind(Utc::now().to_rfc3339())
    .bind(key)
    .execute(db)
    .await?;
    list_sources(db).await
}

async fn load_source(db: &SqlitePool, key: &str) -> anyhow::Result<(bool, Value)> {
    ensure_defaults(db).await?;
    let row: Option<(i64, String)> = sqlx::query_as(
        "SELECT enabled, config_json FROM resource_search_sources WHERE key = ?",
    )
    .bind(key)
    .fetch_optional(db)
    .await?;
    match row {
        Some((en, raw)) => Ok((
            en != 0,
            serde_json::from_str(&raw).unwrap_or(json!({})),
        )),
        None => Ok((false, json!({}))),
    }
}

fn split_note(note: &str) -> (String, String) {
    let re = Regex::new(r"(?s)^(.*?)(?:[【\[]?(?:简介|介绍|描述)[】\]]?[:：]?)?(.*)$").ok();
    if let Some(re) = re {
        if let Some(c) = re.captures(note) {
            return (
                c.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_default(),
                c.get(2).map(|m| m.as_str().trim().to_string()).unwrap_or_default(),
            );
        }
    }
    (note.trim().to_string(), String::new())
}

async fn search_pansou(
    server: &str,
    keyword: &str,
    drive_type: Option<&str>,
) -> anyhow::Result<Vec<Value>> {
    let server = server.trim().trim_end_matches('/');
    if server.is_empty() {
        return Ok(vec![]);
    }
    let cloud_types = match drive_type {
        Some("quark") | None => "quark",
        Some(other) => other,
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(45))
        .build()?;
    let resp = client
        .get(format!("{server}/api/search"))
        .query(&[
            ("kw", keyword),
            ("cloud_types", cloud_types),
            ("res", "merge"),
            ("refresh", "false"),
        ])
        .send()
        .await?;
    if !resp.status().is_success() {
        return Ok(vec![]);
    }
    let data: Value = resp.json().await.unwrap_or(Value::Null);
    if data.get("code").and_then(|v| v.as_i64()) != Some(0) {
        return Ok(vec![]);
    }
    let merged = data
        .pointer("/data/merged_by_type")
        .cloned()
        .unwrap_or(json!({}));
    let mut raw_items = Vec::new();
    for key in ["quark", "uc"] {
        if let Some(arr) = merged.get(key).and_then(|v| v.as_array()) {
            raw_items.extend(arr.iter().cloned());
            break;
        }
    }
    // fallback: flatten all lists if quark key missing
    if raw_items.is_empty() {
        if let Some(obj) = merged.as_object() {
            for (_k, v) in obj {
                if let Some(arr) = v.as_array() {
                    raw_items.extend(arr.iter().cloned());
                }
            }
        }
    }

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in raw_items {
        let url = item
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if url.is_empty() || !seen.insert(url.clone()) {
            continue;
        }
        if detect_drive_type(&url) != Some("quark") && drive_type == Some("quark") {
            continue;
        }
        let note = item.get("note").and_then(|v| v.as_str()).unwrap_or("");
        let (title, content) = split_note(note);
        out.push(json!({
            "shareurl": url,
            "taskname": title,
            "content": content,
            "datetime": item.get("datetime"),
            "channel": item.get("source"),
            "source": "PanSou",
            "source_key": "pansou"
        }));
    }
    Ok(out)
}

fn strip_html(text: &str) -> String {
    let t = text
        .replace("<mark class=\"highlight\">", "")
        .replace("</mark>", "");
    Regex::new(r"<[^>]+>")
        .ok()
        .map(|re| re.replace_all(&t, "").to_string())
        .unwrap_or(t)
}

fn parse_cloudsaver_data(data: &Value, keyword: &str) -> Vec<Value> {
    let arr = data
        .get("data")
        .and_then(|v| v.as_array())
        .cloned()
        .or_else(|| data.as_array().cloned())
        .unwrap_or_default();
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let kw = keyword.trim().to_lowercase();

    for ch in arr {
        // CloudSaver returns channels with nested list + cloudLinks
        if let Some(list) = ch.get("list").and_then(|v| v.as_array()) {
            let channel = ch
                .get("channel")
                .or_else(|| ch.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            for item in list {
                let title = strip_html(item.get("title").and_then(|v| v.as_str()).unwrap_or(""));
                if !kw.is_empty() {
                    let norm: String = title.chars().filter(|c| !c.is_whitespace()).collect();
                    let norm = norm.to_lowercase();
                    let tokens: Vec<&str> = kw.split_whitespace().filter(|t| !t.is_empty()).collect();
                    if !tokens.is_empty()
                        && !tokens.iter().all(|t| {
                            let tt: String = t.chars().filter(|c| !c.is_whitespace()).collect();
                            norm.contains(&tt.to_lowercase())
                        })
                    {
                        continue;
                    }
                }
                let desc = strip_html(
                    item.get("desc")
                        .or_else(|| item.get("content"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                );
                let tm = item
                    .get("datetime")
                    .or_else(|| item.get("time"))
                    .cloned()
                    .unwrap_or(Value::Null);
                let links = item
                    .get("cloudLinks")
                    .or_else(|| item.get("cloud_links"))
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                for cl in links {
                    let url = cl
                        .get("url")
                        .or_else(|| cl.get("link"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if url.is_empty() || !seen.insert(url.clone()) {
                        continue;
                    }
                    if detect_drive_type(&url) != Some("quark") {
                        continue;
                    }
                    out.push(json!({
                        "shareurl": url,
                        "taskname": title,
                        "content": desc,
                        "datetime": tm,
                        "channel": channel,
                        "source": "CloudSaver",
                        "source_key": "cloudsaver"
                    }));
                }
            }
            continue;
        }

        // flat fallback
        let url = ch
            .get("shareurl")
            .or_else(|| ch.get("url"))
            .or_else(|| ch.get("link"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if url.is_empty() || !seen.insert(url.clone()) {
            continue;
        }
        if detect_drive_type(&url) != Some("quark") {
            continue;
        }
        out.push(json!({
            "shareurl": url,
            "taskname": ch.get("title").or_else(|| ch.get("taskname")).or_else(|| ch.get("name")),
            "content": ch.get("content"),
            "datetime": ch.get("datetime"),
            "channel": ch.get("channel"),
            "source": "CloudSaver",
            "source_key": "cloudsaver"
        }));
    }
    out
}

async fn cloudsaver_login(
    client: &reqwest::Client,
    server: &str,
    username: &str,
    password: &str,
) -> anyhow::Result<Option<String>> {
    if username.trim().is_empty() || password.is_empty() {
        return Ok(None);
    }
    let resp = client
        .post(format!("{server}/api/user/login"))
        .json(&json!({ "username": username, "password": password }))
        .send()
        .await?;
    let data: Value = resp.json().await.unwrap_or(Value::Null);
    if data.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
        Ok(data
            .pointer("/data/token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()))
    } else {
        Ok(None)
    }
}

async fn cloudsaver_search_once(
    client: &reqwest::Client,
    server: &str,
    token: &str,
    keyword: &str,
) -> anyhow::Result<(bool, Value)> {
    let resp = client
        .get(format!("{server}/api/search"))
        .header("Authorization", format!("Bearer {token}"))
        .query(&[("keyword", keyword), ("lastMessageId", "")])
        .send()
        .await?;
    let data: Value = resp.json().await.unwrap_or(Value::Null);
    let ok = data.get("success").and_then(|v| v.as_bool()).unwrap_or(false)
        || data.get("data").is_some();
    Ok((ok, data))
}

/// Returns (items, optional refreshed token to persist).
async fn search_cloudsaver(
    server: &str,
    token: &str,
    username: &str,
    password: &str,
    keyword: &str,
) -> anyhow::Result<(Vec<Value>, Option<String>)> {
    let server = server.trim().trim_end_matches('/');
    if server.is_empty() {
        return Ok((vec![], None));
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(45))
        .build()?;

    let mut token = token.trim().to_string();
    let mut new_token: Option<String> = None;

    if token.is_empty() {
        if let Some(t) = cloudsaver_login(&client, server, username, password).await? {
            token = t.clone();
            new_token = Some(t);
        } else {
            return Ok((vec![], None));
        }
    }

    let (ok, data) = cloudsaver_search_once(&client, server, &token, keyword).await?;
    if ok {
        return Ok((parse_cloudsaver_data(&data, keyword), new_token));
    }

    // token expired → re-login
    if let Some(t) = cloudsaver_login(&client, server, username, password).await? {
        token = t.clone();
        new_token = Some(t);
        let (ok2, data2) = cloudsaver_search_once(&client, server, &token, keyword).await?;
        if ok2 {
            return Ok((parse_cloudsaver_data(&data2, keyword), new_token));
        }
    }
    Ok((vec![], new_token))
}

pub async fn fetch_suggestions(
    db: &SqlitePool,
    keyword: &str,
    deep: i64,
    drive_type: Option<&str>,
    sources: Option<&[String]>,
) -> anyhow::Result<Value> {
    let _ = deep;
    let keyword = keyword.trim();
    if keyword.is_empty() {
        return Ok(json!({ "success": true, "data": [], "message": "empty query" }));
    }
    let dt = drive_type
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    // Quark-first: default filter quark if unspecified
    let dt_filter = dt.as_deref().or(Some("quark"));

    let allow = sources.map(|s| {
        s.iter()
            .map(|x| x.trim().to_lowercase())
            .filter(|x| !x.is_empty())
            .collect::<std::collections::HashSet<_>>()
    });

    let mut data = Vec::new();
    let mut messages = Vec::new();

    let (pansou_on, pansou_cfg) = load_source(db, "pansou").await?;
    if pansou_on && allow.as_ref().map(|a| a.contains("pansou")).unwrap_or(true) {
        let server = pansou_cfg
            .get("server")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        match search_pansou(server, keyword, dt_filter).await {
            Ok(items) => data.extend(items),
            Err(e) => messages.push(format!("pansou: {e}")),
        }
    }

    let (cs_on, mut cs_cfg) = load_source(db, "cloudsaver").await?;
    if cs_on && allow.as_ref().map(|a| a.contains("cloudsaver")).unwrap_or(true) {
        let server = cs_cfg
            .get("server")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let token = cs_cfg
            .get("token")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let username = cs_cfg
            .get("username")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let password = cs_cfg
            .get("password")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        match search_cloudsaver(&server, &token, &username, &password, keyword).await {
            Ok((items, new_token)) => {
                data.extend(items);
                if let Some(t) = new_token {
                    if let Some(obj) = cs_cfg.as_object_mut() {
                        obj.insert("token".into(), json!(t));
                    }
                    let _ = sqlx::query(
                        "UPDATE resource_search_sources SET config_json=?, updated_at=? WHERE key='cloudsaver'",
                    )
                    .bind(cs_cfg.to_string())
                    .bind(Utc::now().to_rfc3339())
                    .execute(db)
                    .await;
                }
            }
            Err(e) => messages.push(format!("cloudsaver: {e}")),
        }
    }

    // de-dupe by shareurl keep first
    let mut seen = std::collections::HashSet::new();
    data.retain(|it| {
        let url = it
            .get("shareurl")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        !url.is_empty() && seen.insert(url)
    });

    Ok(json!({
        "success": true,
        "data": data,
        "message": if messages.is_empty() {
            Some(format!("已限定网盘类型：{}", dt_filter.unwrap_or("quark")))
        } else {
            Some(messages.join("; "))
        }
    }))
}

pub async fn verify_suggestions(
    db: &SqlitePool,
    shareurls: Vec<String>,
    account_name: Option<String>,
) -> anyhow::Result<Value> {
    let mut urls: Vec<String> = shareurls
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let mut seen = std::collections::HashSet::new();
    urls.retain(|u| seen.insert(u.clone()));
    if urls.is_empty() {
        return Ok(json!({ "items": [], "total": 0, "valid": 0, "invalid": 0 }));
    }

    let mut items = Vec::new();
    let mut valid = 0usize;
    let mut invalid = 0usize;
    for chunk in urls.chunks(50) {
        let batch = preview_share_batch(
            db,
            SharePreviewBatchIn {
                shareurls: chunk.to_vec(),
                account_name: account_name.clone(),
            },
        )
        .await?;
        for it in batch
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
        {
            let ok = it.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            if ok {
                valid += 1;
            } else {
                invalid += 1;
            }
            let size = it
                .pointer("/latest_video/size")
                .and_then(|v| v.as_i64());
            items.push(json!({
                "shareurl": it.get("shareurl"),
                "drive_type": it.get("drive_type"),
                "ok": ok,
                "message": it.get("message"),
                "pdir_fid": it.get("resolved_pdir_fid").or_else(|| it.get("pdir_fid")),
                "max_file_size": size,
                "latest_video": it.get("latest_video"),
                "is_largest": false
            }));
        }
    }
    if let Some(max_idx) = items
        .iter()
        .enumerate()
        .filter(|(_, x)| x.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
        .max_by_key(|(_, x)| x.get("max_file_size").and_then(|v| v.as_i64()).unwrap_or(0))
        .map(|(i, _)| i)
    {
        items[max_idx]["is_largest"] = json!(true);
    }
    let total = items.len();
    Ok(json!({ "items": items, "total": total, "valid": valid, "invalid": invalid }))
}
