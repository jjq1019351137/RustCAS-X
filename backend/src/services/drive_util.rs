//! Shared helpers for Quark-first drive account + listing helpers.
use crate::adapters::create_adapter;
use crate::adapters::traits::CloudDriveAdapter;
use serde_json::{json, Value};
use sqlx::SqlitePool;

pub fn detect_drive_type(url: &str) -> Option<&'static str> {
    let u = url.to_lowercase();
    if u.contains("pan.quark.cn") || u.contains("quark.cn") {
        Some("quark")
    } else if u.contains("cloud.189.cn") || u.contains("189.cn") {
        Some("cloud189")
    } else {
        None
    }
}

pub fn item_fid(v: &Value) -> Option<String> {
    v.get("fid")
        .or_else(|| v.get("fs_id"))
        .or_else(|| v.get("file_id"))
        .or_else(|| v.get("fileId"))
        .or_else(|| v.get("id"))
        .map(|x| match x {
            Value::String(s) => s.clone(),
            other => other.to_string().trim_matches('"').to_string(),
        })
        .filter(|s| !s.is_empty())
}

pub fn item_name(v: &Value) -> Option<String> {
    v.get("file_name")
        .or_else(|| v.get("server_filename"))
        .or_else(|| v.get("fileName"))
        .or_else(|| v.get("name"))
        .or_else(|| v.get("title"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

pub fn item_is_dir(v: &Value) -> bool {
    if let Some(b) = v.get("is_dir").and_then(|x| x.as_bool()) {
        return b;
    }
    if let Some(b) = v.get("dir").and_then(|x| x.as_bool()) {
        return b;
    }
    if let Some(s) = v.get("isdir").and_then(|x| x.as_str()) {
        return s == "1" || s.eq_ignore_ascii_case("true");
    }
    matches!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("folder" | "dir" | "directory")
    ) || matches!(
        v.get("type").and_then(|x| x.as_str()),
        Some("folder" | "dir")
    ) || matches!(
        v.get("file_type").and_then(|x| x.as_str()),
        Some("0" | "dir" | "folder")
    )
}

pub fn item_size(v: &Value) -> Option<i64> {
    v.get("size").and_then(|x| {
        x.as_i64()
            .or_else(|| x.as_u64().map(|n| n as i64))
            .or_else(|| x.as_f64().map(|n| n as i64))
            .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
    })
}

pub fn item_updated_at(v: &Value) -> Value {
    v.get("updated_at")
        .or_else(|| v.get("update_time"))
        .or_else(|| v.get("mtime"))
        .or_else(|| v.get("modified_at"))
        .cloned()
        .unwrap_or(Value::Null)
}

pub fn item_fid_token(v: &Value) -> Option<String> {
    v.get("share_fid_token")
        .or_else(|| v.get("fid_token"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

pub fn extract_stoken(resp: &Value) -> Option<String> {
    resp.pointer("/data/stoken")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            resp.get("stoken")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
        })
}

pub fn value_as_fid(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => "0".into(),
    }
}

pub async fn pick_account(
    db: &SqlitePool,
    drive_type: &str,
    account_name: Option<&str>,
) -> anyhow::Result<(String, String, String, String)> {
    // returns (name, drive_type, cookie, config_json)
    if let Some(name) = account_name.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        let row = sqlx::query_as::<_, (String, String, String, String)>(
            r#"
            SELECT name, drive_type, cookie, config_json
            FROM drive_accounts
            WHERE name = ? AND enabled = 1
            LIMIT 1
            "#,
        )
        .bind(name)
        .fetch_optional(db)
        .await?;
        return row.ok_or_else(|| anyhow::anyhow!("DRIVE_ACCOUNT_NOT_FOUND"));
    }
    let row = sqlx::query_as::<_, (String, String, String, String)>(
        r#"
        SELECT name, drive_type, cookie, config_json
        FROM drive_accounts
        WHERE enabled = 1 AND drive_type = ?
        ORDER BY is_default DESC, id ASC
        LIMIT 1
        "#,
    )
    .bind(drive_type)
    .fetch_optional(db)
    .await?;
    row.ok_or_else(|| anyhow::anyhow!("DRIVE_ACCOUNT_NOT_FOUND"))
}

pub async fn load_adapter(
    db: &SqlitePool,
    drive_type: &str,
    account_name: Option<&str>,
) -> anyhow::Result<(String, Box<dyn CloudDriveAdapter>)> {
    let (name, dt, cookie, config_json) = pick_account(db, drive_type, account_name).await?;
    let config: Value = serde_json::from_str(&config_json).unwrap_or(json!({}));
    let mut adapter = create_adapter(&dt, Some(&config), Some(&cookie))?;
    let info = adapter.init().await?;
    let ok = info.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    if !ok {
        let msg = info
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("init failed");
        anyhow::bail!("{msg}");
    }
    Ok((name, adapter))
}
