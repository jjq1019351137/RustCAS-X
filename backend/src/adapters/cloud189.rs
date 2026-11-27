use super::traits::{
    deserialize_kv_cookie, normalize_config, serialize_kv_config, CloudDriveAdapter, DriveTypeMeta,
    ExtractedShare,
};
use async_trait::async_trait;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::OnceLock;
use url::Url;

pub fn meta() -> DriveTypeMeta {
    DriveTypeMeta {
        code: "cloud189".into(),
        drive_name: "天翼云盘".into(),
        class_name: "Cloud189Adapter".into(),
        config_format: "kv".into(),
        default_config: json!({
            "username": "",
            "password": "",
            "ssoncookie": "",
            "family_id": "",
            "lsdir_cache_path": "",
            "static_lsdir_cache_path": "",
            "strm_scan_path": "",
            "protocol": "pc",
            "name": "",
            "debug": false
        }),
        config_fields: vec![
            json!({
                "key": "username",
                "label": "用户名",
                "input_type": "text",
                "required": false,
                "secret": false
            }),
            json!({
                "key": "password",
                "label": "密码",
                "input_type": "password",
                "required": false,
                "secret": true
            }),
            json!({
                "key": "ssoncookie",
                "label": "SSO Cookie",
                "input_type": "textarea",
                "required": false,
                "secret": true
            }),
            json!({
                "key": "lsdir_cache_path",
                "label": "缓存路径",
                "input_type": "text",
                "required": true,
                "secret": false
            }),
        ],
    }
}

pub struct Cloud189Adapter {
    config: HashMap<String, Value>,
    cookie: String,
}

impl Cloud189Adapter {
    pub fn new(config: Option<&Value>, cookie: Option<&str>) -> Self {
        let m = meta();
        let cfg = if let Some(c) = config {
            normalize_config(&m.default_config, Some(c))
        } else if let Some(raw) = cookie {
            deserialize_kv_cookie(&m.default_config, raw)
        } else {
            normalize_config(&m.default_config, None)
        };
        let cookie = serialize_kv_config(&cfg);
        Self { config: cfg, cookie }
    }

    pub fn cookie(&self) -> &str {
        &self.cookie
    }
}

fn access_res() -> &'static [Regex] {
    static RES: OnceLock<Vec<Regex>> = OnceLock::new();
    RES.get_or_init(|| {
        [
            r"[（(]访问码[：:]\s*([a-zA-Z0-9]{4})[)）]",
            r"[（(]提取码[：:]\s*([a-zA-Z0-9]{4})[)）]",
            r"访问码[：:]\s*([a-zA-Z0-9]{4})",
            r"提取码[：:]\s*([a-zA-Z0-9]{4})",
            r"[（(]([a-zA-Z0-9]{4})[)）]",
        ]
        .into_iter()
        .map(|p| Regex::new(p).unwrap())
        .collect()
    })
}

fn url_res() -> &'static [Regex] {
    static RES: OnceLock<Vec<Regex>> = OnceLock::new();
    RES.get_or_init(|| {
        [
            r"(https?://cloud\.189\.cn/web/share\?[^\s]+)",
            r"(https?://cloud\.189\.cn/t/[a-zA-Z0-9]+[^\s]*)",
            r"(https?://h5\.cloud\.189\.cn/share\.html#/t/[a-zA-Z0-9]+[^\s]*)",
            r"(https?://[^/]+/web/share\?[^\s]+)",
            r"(https?://[^/]+/t/[a-zA-Z0-9]+[^\s]*)",
            r"(https?://[^/]+/share\.html[^\s]*)",
            r"(https?://content\.21cn\.com[^\s]+)",
        ]
        .into_iter()
        .map(|p| Regex::new(p).unwrap())
        .collect()
    })
}

#[async_trait]
impl CloudDriveAdapter for Cloud189Adapter {
    fn drive_type(&self) -> &'static str {
        "cloud189"
    }

    fn extract_url(&self, url: &str) -> ExtractedShare {
        if url.trim().is_empty() {
            return ExtractedShare {
                pwd_id: None,
                passcode: String::new(),
                pdir_fid: json!(0),
                paths: vec![],
            };
        }
        let normalized = url.trim().replace('？', "?").replace('＆', "&");
        let space_re = Regex::new(r"\s+").unwrap();
        let mut compact = space_re.replace_all(&normalized, "").to_string();
        if let Ok(decoded) = urlencoding::decode(&compact) {
            compact = decoded.into_owned();
        }

        let mut passcode = String::new();
        for re in access_res() {
            if let Some(m) = re.captures(&compact) {
                passcode = m.get(1).unwrap().as_str().trim().to_string();
                compact = compact.replacen(m.get(0).unwrap().as_str(), "", 1);
                break;
            }
        }

        let mut extracted = String::new();
        for re in url_res() {
            if let Some(m) = re.captures(&compact) {
                extracted = m.get(1).unwrap().as_str().to_string();
                break;
            }
        }
        if extracted.is_empty() {
            let any = Regex::new(r"(https?://[^\s]+)").unwrap();
            if let Some(m) = any.captures(&normalized) {
                extracted = m.get(1).unwrap().as_str().to_string();
            }
        }
        if extracted.is_empty() {
            extracted = url.trim().to_string();
        }

        let mut pwd_id = None;
        if let Ok(parsed) = Url::parse(&extracted) {
            for (k, v) in parsed.query_pairs() {
                if k == "code" || k == "shareCode" || k == "share_code" {
                    pwd_id = Some(v.to_string());
                }
                if (k == "accessCode" || k == "pwd" || k == "passcode") && passcode.is_empty() {
                    passcode = v.to_string();
                }
            }
            if pwd_id.is_none() {
                let path = parsed.path();
                if let Some(rest) = path.strip_prefix("/t/") {
                    pwd_id = Some(rest.split('/').next().unwrap_or(rest).to_string());
                }
            }
            if pwd_id.is_none() {
                if let Some(frag) = parsed.fragment() {
                    let re = Regex::new(r"/t/([a-zA-Z0-9]+)").unwrap();
                    if let Some(m) = re.captures(frag) {
                        pwd_id = Some(m.get(1).unwrap().as_str().to_string());
                    }
                }
            }
        }

        ExtractedShare {
            pwd_id,
            passcode,
            pdir_fid: json!(0),
            paths: vec![],
        }
    }

    fn config(&self) -> &HashMap<String, Value> {
        &self.config
    }
}
