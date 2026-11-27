use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct DriveTypeMeta {
    pub code: String,
    pub drive_name: String,
    pub class_name: String,
    pub config_format: String,
    pub default_config: Value,
    pub config_fields: Vec<Value>,
}

#[derive(Clone, Debug)]
pub struct ExtractedShare {
    pub pwd_id: Option<String>,
    pub passcode: String,
    pub pdir_fid: Value,
    pub paths: Vec<Value>,
}

#[async_trait]
pub trait CloudDriveAdapter: Send + Sync {
    fn drive_type(&self) -> &'static str;
    fn extract_url(&self, url: &str) -> ExtractedShare;

    async fn init(&mut self) -> anyhow::Result<Value> {
        Ok(json!({ "ok": false, "message": "adapter init not fully ported" }))
    }

    async fn ls_dir(&self, _pdir_fid: &str, _max_items: i64) -> anyhow::Result<Value> {
        Ok(json!({
            "code": -1,
            "message": format!("{} ls_dir pending full port", self.drive_type()),
            "data": { "list": [] }
        }))
    }

    async fn get_stoken(&self, _pwd_id: &str, _passcode: &str) -> anyhow::Result<Value> {
        Ok(json!({ "status": 0, "message": "get_stoken pending full port" }))
    }

    fn config(&self) -> &HashMap<String, Value>;
}

/// Normalize adapter config like Python BaseCloudDriveAdapter.normalize_config.
pub fn normalize_config(defaults: &Value, input: Option<&Value>) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    if let Some(obj) = defaults.as_object() {
        for (k, v) in obj {
            out.insert(k.clone(), v.clone());
        }
    }
    if let Some(Value::Object(map)) = input {
        for (k, v) in map {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

pub fn serialize_raw_config(config: &HashMap<String, Value>, primary: &str) -> String {
    config
        .get(primary)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

pub fn serialize_kv_config(config: &HashMap<String, Value>) -> String {
    let mut parts = Vec::new();
    for (k, v) in config {
        let s = match v {
            Value::Bool(b) => {
                if *b {
                    "true".into()
                } else {
                    continue;
                }
            }
            Value::String(s) if s.trim().is_empty() => continue,
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => continue,
        };
        if s.trim().is_empty() && !matches!(v, Value::Bool(true)) {
            continue;
        }
        parts.push(format!("{k}={s}"));
    }
    parts.join(";")
}

pub fn deserialize_kv_cookie(defaults: &Value, cookie: &str) -> HashMap<String, Value> {
    let mut config = normalize_config(defaults, None);
    for chunk in cookie.split(';') {
        let chunk = chunk.trim();
        if chunk.is_empty() || !chunk.contains('=') {
            continue;
        }
        let (k, v) = chunk.split_once('=').unwrap();
        let key = k.trim().to_string();
        let val = v.trim().to_string();
        if let Some(default) = config.get(&key) {
            if default.as_bool().is_some() {
                config.insert(
                    key,
                    Value::Bool(matches!(val.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")),
                );
                continue;
            }
            if default.as_i64().is_some() {
                if let Ok(n) = val.parse::<i64>() {
                    config.insert(key, json!(n));
                    continue;
                }
            }
        }
        config.insert(key, Value::String(val));
    }
    config
}
