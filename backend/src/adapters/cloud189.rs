use super::traits::{
    deserialize_kv_cookie, normalize_config, serialize_kv_config, CloudDriveAdapter, DriveTypeMeta,
    ExtractedShare,
};
use async_trait::async_trait;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, COOKIE};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::OnceLock;
use url::Url;

const HOST_URL: &str = "https://cloud.189.cn";

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
    client: reqwest::Client,
    active: bool,
    nickname: String,
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
        let client = reqwest::Client::builder()
            .cookie_store(true)
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            config: cfg,
            cookie,
            client,
            active: false,
            nickname: String::new(),
        }
    }

    pub fn cookie(&self) -> &str {
        &self.cookie
    }

    fn cfg_str(&self, key: &str) -> String {
        self.config
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string()
    }

    fn cookie_header(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let mut parts = Vec::new();
        let sson = self.cfg_str("ssoncookie");
        if !sson.is_empty() {
            if sson.contains('=') {
                parts.push(sson);
            } else {
                parts.push(format!("SSON={sson}"));
            }
        }
        for key in ["SSON", "COOKIE_LOGIN_USER", "COOKIE_LOGIN_USER_NEW", "DEVICEID", "OPENINFO"] {
            let v = self.cfg_str(key);
            if !v.is_empty() {
                parts.push(format!("{key}={v}"));
            }
        }
        // Also accept raw cookie field fragments from kv serialization
        if !self.cookie.is_empty() {
            for chunk in self.cookie.split(';') {
                let c = chunk.trim();
                if !c.is_empty() {
                    parts.push(c.to_string());
                }
            }
        }
        if let Ok(hv) = HeaderValue::from_str(&parts.join("; ")) {
            headers.insert(COOKIE, hv);
        }
        headers
    }

    async fn get_logined_infos(&self) -> anyhow::Result<Option<Value>> {
        let resp = self
            .client
            .get(format!("{HOST_URL}/v2/getLoginedInfos.action"))
            .headers(self.cookie_header())
            .send()
            .await?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        let j: Value = resp.json().await.unwrap_or(json!({}));
        if j.get("userId").is_some() {
            Ok(Some(j))
        } else {
            Ok(None)
        }
    }

    async fn get_share_info_by_code_v2(&self, share_code: &str) -> anyhow::Result<Option<Value>> {
        let resp = self
            .client
            .get(format!("{HOST_URL}/api/open/share/getShareInfoByCodeV2.action"))
            .headers(self.cookie_header())
            .query(&[
                ("noCache", format!("{}", chrono::Utc::now().timestamp_millis())),
                ("shareCode", share_code.to_string()),
            ])
            .send()
            .await?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        let j: Value = resp.json().await.unwrap_or(json!({}));
        let res_code = j
            .get("res_code")
            .or_else(|| j.get("resCode"))
            .map(|v| v.to_string().trim_matches('"').to_string())
            .unwrap_or_default();
        let file_id = j.get("fileId").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let share_id = j.get("shareId").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let share_mode = j
            .get("shareMode")
            .map(|v| v.to_string().trim_matches('"').to_string())
            .unwrap_or_else(|| "3".into());
        let ok = res_code == "0" || res_code.is_empty() || j.get("res_code") == Some(&json!(0));
        if (ok || !file_id.is_empty()) && !file_id.is_empty() {
            return Ok(Some(json!({
                "shareId": share_id,
                "fileId": file_id,
                "shareMode": share_mode,
                "isFolder": j.get("isFolder").and_then(|v| v.as_bool()).unwrap_or(false),
                "fileName": j.get("fileName").and_then(|v| v.as_str()).unwrap_or("")
            })));
        }
        Ok(None)
    }

    async fn check_access_code(&self, share_code: &str, access_code: &str) -> anyhow::Result<Option<String>> {
        let resp = self
            .client
            .get(format!("{HOST_URL}/api/open/share/checkAccessCode.action"))
            .headers(self.cookie_header())
            .query(&[
                ("shareCode", share_code),
                ("accessCode", access_code),
            ])
            .send()
            .await?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        let j: Value = resp.json().await.unwrap_or(json!({}));
        let share_id = j
            .get("shareId")
            .or_else(|| j.pointer("/data/shareId"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        Ok(share_id.filter(|s| !s.is_empty()))
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

    async fn init(&mut self) -> anyhow::Result<Value> {
        if let Some(info) = self.get_logined_infos().await? {
            self.active = true;
            let nickname = info
                .get("nickname")
                .and_then(|v| v.as_str())
                .unwrap_or("天翼用户")
                .to_string();
            self.nickname = nickname.clone();
            return Ok(json!({
                "ok": true,
                "drive_type": "cloud189",
                "drive_name": "天翼云盘",
                "nickname": nickname,
                "username": nickname,
                "raw": { "account_info": info }
            }));
        }
        Ok(json!({
            "ok": false,
            "message": "cookie/sson 无效；用户名密码登录与验证码流程仍在迁移"
        }))
    }

    async fn ls_dir(&self, pdir_fid: &str, max_items: i64) -> anyhow::Result<Value> {
        if !self.active {
            // best-effort: try once
        }
        let folder_id = if pdir_fid.is_empty() || pdir_fid == "0" || pdir_fid == "-11" {
            "-11"
        } else {
            pdir_fid
        };

        let mut merged = Vec::new();
        let mut page = 1i64;
        loop {
            let url = if folder_id == "-11" {
                format!("{HOST_URL}/api/portal/listFiles.action")
            } else {
                format!("{HOST_URL}/api/open/file/listFiles.action")
            };
            let mut req = self.client.get(url).headers(self.cookie_header());
            req = if folder_id == "-11" {
                req.query(&[("fileId", folder_id), ("noCache", "1")])
            } else {
                req.query(&[
                    ("folderId", folder_id),
                    ("orderBy", "lastOpTime"),
                    ("descending", "true"),
                    ("pageNum", &page.to_string()),
                    ("pageSize", "60"),
                    ("iconOption", "5"),
                    ("mediaType", "0"),
                    ("noCache", "1"),
                ])
            };
            let resp = req.send().await?;
            let j: Value = resp.json().await.unwrap_or(json!({}));
            if j.get("errorCode").is_some() {
                return Ok(json!({ "code": 1, "message": "获取目录列表失败", "data": { "list": [] }, "raw": j }));
            }

            let batch: Vec<Value> = if folder_id == "-11" {
                j.get("data")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|it| {
                        json!({
                            "fid": it.get("fileId").map(|v| v.to_string().trim_matches('"').to_string()).unwrap_or_default(),
                            "file_name": it.get("fileName").and_then(|v| v.as_str()).unwrap_or(""),
                            "dir": it.get("isFolder").and_then(|v| v.as_bool()).unwrap_or(false),
                            "size": it.get("fileSize").cloned().unwrap_or(json!(0)),
                            "share_fid_token": ""
                        })
                    })
                    .collect()
            } else {
                let ao = j.get("fileListAO").cloned().unwrap_or(json!({}));
                let mut items = Vec::new();
                if let Some(folders) = ao.get("folderList").and_then(|v| v.as_array()) {
                    for it in folders {
                        items.push(json!({
                            "fid": it.get("id").map(|v| v.to_string().trim_matches('"').to_string()).unwrap_or_default(),
                            "file_name": it.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                            "dir": true,
                            "size": 0,
                            "share_fid_token": ""
                        }));
                    }
                }
                if let Some(files) = ao.get("fileList").and_then(|v| v.as_array()) {
                    for it in files {
                        items.push(json!({
                            "fid": it.get("id").map(|v| v.to_string().trim_matches('"').to_string()).unwrap_or_default(),
                            "file_name": it.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                            "dir": false,
                            "size": it.get("size").cloned().unwrap_or(json!(0)),
                            "share_fid_token": ""
                        }));
                    }
                }
                items
            };

            if batch.is_empty() {
                break;
            }
            merged.extend(batch);
            if max_items > 0 && merged.len() as i64 >= max_items {
                merged.truncate(max_items as usize);
                break;
            }
            if folder_id == "-11" {
                break;
            }
            page += 1;
            if page > 100 {
                break;
            }
        }

        Ok(json!({ "code": 0, "message": "success", "data": { "list": merged } }))
    }

    async fn get_stoken(&self, pwd_id: &str, passcode: &str) -> anyhow::Result<Value> {
        if pwd_id.is_empty() {
            return Ok(json!({ "status": 400, "message": "分享链接无效", "data": {} }));
        }
        let Some(meta2) = self.get_share_info_by_code_v2(pwd_id).await? else {
            return Ok(json!({ "status": 404, "message": "解析分享失败", "data": {} }));
        };
        let share_mode = meta2
            .get("shareMode")
            .and_then(|v| v.as_str())
            .unwrap_or("3")
            .to_string();
        let mut share_id = meta2
            .get("shareId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if share_mode == "1" {
            if passcode.is_empty() {
                return Ok(json!({ "status": 400, "message": "提取码不能为空", "data": {} }));
            }
            match self.check_access_code(pwd_id, passcode).await? {
                Some(id) => share_id = id,
                None => {
                    return Ok(json!({ "status": 403, "message": "提取码错误", "data": {} }));
                }
            }
        }
        let stoken_obj = json!({
            "shareId": share_id,
            "shareMode": share_mode,
            "rootFileId": meta2.get("fileId"),
            "isFolder": meta2.get("isFolder"),
            "accessCode": passcode
        });
        Ok(json!({
            "status": 200,
            "data": { "stoken": stoken_obj.to_string() }
        }))
    }

    fn config(&self) -> &HashMap<String, Value> {
        &self.config
    }
}
