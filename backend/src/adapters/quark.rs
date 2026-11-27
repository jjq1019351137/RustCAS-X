use super::traits::{
    normalize_config, serialize_raw_config, CloudDriveAdapter, DriveTypeMeta, ExtractedShare,
};
use async_trait::async_trait;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::OnceLock;

fn re_pwd_id() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"/s/(\w+)").unwrap())
}
fn re_passcode() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"pwd=(\w+)").unwrap())
}
fn re_query_fid() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:\?|&)fid=([^&#]+)").unwrap())
}
fn re_hash_fid() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"#/list/share/([a-zA-Z0-9]{6,64})").unwrap())
}
fn re_paths() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"/(\w{32})-?([^/]+)?").unwrap())
}

pub fn meta() -> DriveTypeMeta {
    DriveTypeMeta {
        code: "quark".into(),
        drive_name: "夸克网盘".into(),
        class_name: "QuarkAdapter".into(),
        config_format: "raw".into(),
        default_config: json!({
            "cookie": "",
            "refresh_token": "",
            "device_id": "",
            "query_token": "",
            "lsdir_cache_path": "",
            "static_lsdir_cache_path": "",
            "strm_scan_path": ""
        }),
        config_fields: vec![
            json!({
                "key": "cookie",
                "label": "Cookie",
                "description": "夸克网盘登录 Cookie 原文",
                "input_type": "textarea",
                "required": true,
                "secret": true
            }),
            json!({
                "key": "refresh_token",
                "label": "TV 刷新令牌",
                "input_type": "textarea",
                "required": false,
                "secret": true
            }),
            json!({
                "key": "lsdir_cache_path",
                "label": "缓存路径",
                "input_type": "text",
                "required": false,
                "secret": false
            }),
        ],
    }
}

pub struct QuarkAdapter {
    config: HashMap<String, Value>,
    cookie: String,
}

impl QuarkAdapter {
    pub fn new(config: Option<&Value>, cookie: Option<&str>) -> Self {
        let m = meta();
        let mut cfg = if let Some(c) = config {
            normalize_config(&m.default_config, Some(c))
        } else if let Some(raw) = cookie {
            let mut c = normalize_config(&m.default_config, None);
            c.insert("cookie".into(), Value::String(raw.to_string()));
            c
        } else {
            normalize_config(&m.default_config, None)
        };
        let cookie = serialize_raw_config(&cfg, "cookie");
        cfg.insert("cookie".into(), Value::String(cookie.clone()));
        Self { config: cfg, cookie }
    }

    pub fn cookie(&self) -> &str {
        &self.cookie
    }
}

#[async_trait]
impl CloudDriveAdapter for QuarkAdapter {
    fn drive_type(&self) -> &'static str {
        "quark"
    }

    fn extract_url(&self, url: &str) -> ExtractedShare {
        let pwd_id = re_pwd_id()
            .captures(url)
            .and_then(|c| c.get(1).map(|m| m.as_str().to_string()));
        let passcode = re_passcode()
            .captures(url)
            .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
            .unwrap_or_default();

        let mut pdir_fid: Value = json!(0);
        if let Some(cap) = re_query_fid().captures(url) {
            let query_fid = urlencoding::decode(cap.get(1).unwrap().as_str())
                .unwrap_or_default()
                .trim()
                .to_string();
            if !query_fid.is_empty() && query_fid != "0" && query_fid != "root" {
                pdir_fid = Value::String(query_fid);
            }
        }
        if matches!(pdir_fid, Value::Number(_))
            || pdir_fid.as_str() == Some("0")
            || pdir_fid.as_str() == Some("")
            || pdir_fid.is_null()
        {
            if let Some(cap) = re_hash_fid().captures(url) {
                let hash_fid = cap.get(1).unwrap().as_str().trim();
                if !hash_fid.is_empty() {
                    pdir_fid = Value::String(hash_fid.to_string());
                }
            }
        }

        let mut paths = Vec::new();
        for cap in re_paths().captures_iter(url) {
            let fid = cap.get(1).unwrap().as_str().to_string();
            let name = cap
                .get(2)
                .map(|m| {
                    urlencoding::decode(m.as_str())
                        .unwrap_or_default()
                        .replace("*101", "-")
                })
                .unwrap_or_default();
            paths.push(json!({ "fid": fid, "name": name }));
        }
        if (matches!(pdir_fid, Value::Number(_)) || pdir_fid.as_str() == Some("0")) && !paths.is_empty() {
            if let Some(last) = paths.last() {
                if let Some(fid) = last.get("fid") {
                    pdir_fid = fid.clone();
                }
            }
        }

        ExtractedShare {
            pwd_id,
            passcode,
            pdir_fid,
            paths,
        }
    }

    fn config(&self) -> &HashMap<String, Value> {
        &self.config
    }
}
