use super::traits::{
    normalize_config, serialize_raw_config, CloudDriveAdapter, DriveTypeMeta, ExtractedShare,
};
use async_trait::async_trait;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::OnceLock;

const BASE_URL: &str = "https://drive-pc.quark.cn";
const BASE_URL_APP: &str = "https://drive-m.quark.cn";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) quark-cloud-drive/3.14.2 Chrome/112.0.5615.165 Electron/24.1.3.8 Safari/537.36 Channel/pckk_other_ch";

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
fn re_kps() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|[;\s])kps=([a-zA-Z0-9%+/=]+)").unwrap())
}
fn re_sign() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|[;\s])sign=([a-zA-Z0-9%+/=]+)").unwrap())
}
fn re_vcode() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|[;\s])vcode=([a-zA-Z0-9%+/=]+)").unwrap())
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

#[derive(Clone, Default)]
struct MParam {
    kps: String,
    sign: String,
    vcode: String,
}

pub struct QuarkAdapter {
    config: HashMap<String, Value>,
    cookie: String,
    mparam: Option<MParam>,
    client: reqwest::Client,
    nickname: String,
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
        let mparam = match_mparam(&cookie);
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            config: cfg,
            cookie,
            mparam,
            client,
            nickname: String::new(),
        }
    }

    pub fn cookie(&self) -> &str {
        &self.cookie
    }

    async fn send_json(
        &self,
        method: reqwest::Method,
        mut url: String,
        mut params: Vec<(String, String)>,
        body: Option<Value>,
    ) -> anyhow::Result<Value> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            "application/json".parse().unwrap(),
        );

        let use_mparam = self.mparam.is_some() && url.contains("share") && url.contains(BASE_URL);
        if use_mparam {
            url = url.replace(BASE_URL, BASE_URL_APP);
            if let Some(mp) = &self.mparam {
                params.extend([
                    ("device_model".into(), "M2011K2C".into()),
                    ("entry".into(), "default_clouddrive".into()),
                    ("fr".into(), "android".into()),
                    ("pr".into(), "ucpro".into()),
                    ("kps".into(), mp.kps.clone()),
                    ("sign".into(), mp.sign.clone()),
                    ("vcode".into(), mp.vcode.clone()),
                    ("app".into(), "clouddrive".into()),
                ]);
            }
        } else if let Ok(cookie_hv) = reqwest::header::HeaderValue::from_str(&self.cookie) {
            headers.insert(reqwest::header::COOKIE, cookie_hv);
        }

        let mut req = self.client.request(method, &url).headers(headers).query(&params);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let json: Value = serde_json::from_str(&text).unwrap_or_else(|_| {
            json!({ "status": status.as_u16(), "code": 1, "message": text })
        });
        Ok(json)
    }

    async fn get_account_info(&self) -> anyhow::Result<Option<Value>> {
        let url = "https://pan.quark.cn/account/info";
        let params = vec![("fr".into(), "pc".into()), ("platform".into(), "pc".into())];
        let response = self
            .send_json(reqwest::Method::GET, url.into(), params, None)
            .await?;
        Ok(response.get("data").cloned().filter(|v| !v.is_null()))
    }

    async fn get_member_info(&self) -> anyhow::Result<Value> {
        let url = format!("{BASE_URL}/1/clouddrive/member");
        let params = vec![
            ("pr".into(), "ucpro".into()),
            ("fr".into(), "pc".into()),
            ("uc_param_str".into(), "".into()),
            ("fetch_subscribe".into(), "true".into()),
            ("_ch".into(), "home".into()),
            ("fetch_identity".into(), "true".into()),
        ];
        let response = self
            .send_json(reqwest::Method::GET, url, params, None)
            .await?;
        if response.get("code").and_then(|v| v.as_i64()) == Some(0) {
            Ok(response)
        } else {
            Ok(json!({}))
        }
    }

    async fn get_detail_inner(
        &self,
        pwd_id: &str,
        stoken: &str,
        pdir_fid: &str,
    ) -> anyhow::Result<Value> {
        let mut list_merge = Vec::new();
        let mut page = 1i64;
        let mut last = json!({});
        loop {
            let url = format!("{BASE_URL}/1/clouddrive/share/sharepage/detail");
            let params = vec![
                ("pr".into(), "ucpro".into()),
                ("fr".into(), "pc".into()),
                ("pwd_id".into(), pwd_id.into()),
                ("stoken".into(), stoken.into()),
                ("pdir_fid".into(), pdir_fid.into()),
                ("force".into(), "0".into()),
                ("_page".into(), page.to_string()),
                ("_size".into(), "50".into()),
                ("_fetch_banner".into(), "0".into()),
                ("_fetch_share".into(), "0".into()),
                ("_fetch_total".into(), "1".into()),
                ("_sort".into(), "file_type:asc,updated_at:desc".into()),
                ("ver".into(), "2".into()),
                ("fetch_share_full_path".into(), "0".into()),
            ];
            let response = self
                .send_json(reqwest::Method::GET, url, params, None)
                .await?;
            last = response.clone();
            if response.get("code").and_then(|v| v.as_i64()) != Some(0) {
                return Ok(response);
            }
            let batch = response
                .pointer("/data/list")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if batch.is_empty() {
                break;
            }
            list_merge.extend(batch);
            page += 1;
            let total = response
                .pointer("/metadata/_total")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            if total > 0 && list_merge.len() as i64 >= total {
                break;
            }
            if page > 200 {
                break;
            }
        }
        if let Some(obj) = last.get_mut("data").and_then(|d| d.as_object_mut()) {
            obj.insert("list".into(), Value::Array(list_merge));
        }
        Ok(last)
    }
}

fn match_mparam(cookie: &str) -> Option<MParam> {
    let kps = re_kps().captures(cookie)?.get(1)?.as_str().replace("%25", "%");
    let sign = re_sign().captures(cookie)?.get(1)?.as_str().replace("%25", "%");
    let vcode = re_vcode().captures(cookie)?.get(1)?.as_str().replace("%25", "%");
    Some(MParam { kps, sign, vcode })
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

    async fn init(&mut self) -> anyhow::Result<Value> {
        if self.cookie.trim().is_empty() {
            return Ok(json!({ "ok": false, "message": "cookie empty" }));
        }
        let Some(info) = self.get_account_info().await? else {
            return Ok(json!({ "ok": false, "message": "cookie invalid or account info empty" }));
        };
        let nickname = info
            .get("nickname")
            .or_else(|| info.get("nick_name"))
            .and_then(|v| v.as_str())
            .unwrap_or("夸克用户")
            .to_string();
        self.nickname = nickname.clone();
        let member = self.get_member_info().await.unwrap_or_else(|_| json!({}));
        let member_data = member.get("data").cloned().unwrap_or(json!({}));
        Ok(json!({
            "ok": true,
            "drive_type": "quark",
            "drive_name": "夸克网盘",
            "nickname": nickname,
            "username": nickname,
            "used_space": member_data.get("use_capacity"),
            "total_space": member_data.get("total_capacity"),
            "member_type": member_data.get("member_type"),
            "raw": {
                "account_info": info,
                "member_info": member_data
            }
        }))
    }

    async fn ls_dir(&self, pdir_fid: &str, max_items: i64) -> anyhow::Result<Value> {
        let mut list_merge = Vec::new();
        let mut page = 1i64;
        let mut last = json!({});
        let fid = if pdir_fid.is_empty() { "0" } else { pdir_fid };
        loop {
            let url = format!("{BASE_URL}/1/clouddrive/file/sort");
            let params = vec![
                ("pr".into(), "ucpro".into()),
                ("fr".into(), "pc".into()),
                ("uc_param_str".into(), "".into()),
                ("pdir_fid".into(), fid.into()),
                ("_page".into(), page.to_string()),
                ("_size".into(), "1000".into()),
                ("_fetch_total".into(), "1".into()),
                ("_fetch_sub_dirs".into(), "0".into()),
                ("_sort".into(), "file_type:asc,updated_at:desc".into()),
                ("_fetch_full_path".into(), "0".into()),
                ("fetch_all_file".into(), "1".into()),
                ("fetch_risk_file_name".into(), "1".into()),
            ];
            let response = self
                .send_json(reqwest::Method::GET, url, params, None)
                .await?;
            last = response.clone();
            if response.get("code").and_then(|v| v.as_i64()) != Some(0) {
                return Ok(response);
            }
            let batch = response
                .pointer("/data/list")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if batch.is_empty() {
                break;
            }
            list_merge.extend(batch);
            if max_items > 0 && list_merge.len() as i64 >= max_items {
                list_merge.truncate(max_items as usize);
                break;
            }
            page += 1;
            let total = response
                .pointer("/metadata/_total")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            if total > 0 && list_merge.len() as i64 >= total {
                break;
            }
            if page > 200 {
                break;
            }
        }
        if let Some(obj) = last.get_mut("data").and_then(|d| d.as_object_mut()) {
            obj.insert("list".into(), Value::Array(list_merge));
        }
        Ok(last)
    }

    async fn get_stoken(&self, pwd_id: &str, passcode: &str) -> anyhow::Result<Value> {
        let url = format!("{BASE_URL}/1/clouddrive/share/sharepage/token");
        let params = vec![("pr".into(), "ucpro".into()), ("fr".into(), "pc".into())];
        let payload = json!({ "pwd_id": pwd_id, "passcode": passcode });
        self.send_json(reqwest::Method::POST, url, params, Some(payload))
            .await
    }

    async fn get_detail(
        &self,
        pwd_id: &str,
        stoken: &str,
        pdir_fid: &str,
    ) -> anyhow::Result<Value> {
        self.get_detail_inner(pwd_id, stoken, pdir_fid).await
    }

    async fn mkdir(&self, dir_path: &str) -> anyhow::Result<Value> {
        let url = format!("{BASE_URL}/1/clouddrive/file");
        let params = vec![
            ("pr".into(), "ucpro".into()),
            ("fr".into(), "pc".into()),
            ("uc_param_str".into(), "".into()),
        ];
        let payload = json!({
            "pdir_fid": "0",
            "file_name": "",
            "dir_path": dir_path,
            "dir_init_lock": false
        });
        self.send_json(reqwest::Method::POST, url, params, Some(payload))
            .await
    }

    async fn get_fids(&self, file_paths: &[String]) -> anyhow::Result<Vec<Value>> {
        let mut fids = Vec::new();
        let mut remaining: Vec<String> = file_paths.to_vec();
        while !remaining.is_empty() {
            let chunk: Vec<String> = remaining.drain(..remaining.len().min(50)).collect();
            let url = format!("{BASE_URL}/1/clouddrive/file/info/path_list");
            let params = vec![("pr".into(), "ucpro".into()), ("fr".into(), "pc".into())];
            let payload = json!({ "file_path": chunk, "namespace": "0" });
            let response = self
                .send_json(reqwest::Method::POST, url, params, Some(payload))
                .await?;
            if response.get("code").and_then(|v| v.as_i64()) != Some(0) {
                break;
            }
            if let Some(arr) = response.get("data").and_then(|v| v.as_array()) {
                fids.extend(arr.clone());
            }
        }
        Ok(fids)
    }

    async fn save_file(
        &self,
        fid_list: &[String],
        fid_token_list: &[String],
        to_pdir_fid: &str,
        pwd_id: &str,
        stoken: &str,
    ) -> anyhow::Result<Value> {
        let url = format!("{BASE_URL}/1/clouddrive/share/sharepage/save");
        let now_ms = chrono::Utc::now().timestamp_millis();
        let params = vec![
            ("pr".into(), "ucpro".into()),
            ("fr".into(), "pc".into()),
            ("uc_param_str".into(), "".into()),
            ("app".into(), "clouddrive".into()),
            ("__dt".into(), ((now_ms % 300_000) + 60_000).to_string()),
            ("__t".into(), (now_ms / 1000).to_string()),
        ];
        let payload = json!({
            "fid_list": fid_list,
            "fid_token_list": fid_token_list,
            "to_pdir_fid": to_pdir_fid,
            "pwd_id": pwd_id,
            "stoken": stoken,
            "pdir_fid": "0",
            "scene": "link"
        });
        self.send_json(reqwest::Method::POST, url, params, Some(payload))
            .await
    }

    async fn query_task(&self, task_id: &str) -> anyhow::Result<Value> {
        let mut retry_index = 0i64;
        loop {
            let url = format!("{BASE_URL}/1/clouddrive/task");
            let now_ms = chrono::Utc::now().timestamp_millis();
            let params = vec![
                ("pr".into(), "ucpro".into()),
                ("fr".into(), "pc".into()),
                ("uc_param_str".into(), "".into()),
                ("task_id".into(), task_id.into()),
                ("retry_index".into(), retry_index.to_string()),
                ("__dt".into(), ((now_ms % 300_000) + 60_000).to_string()),
                ("__t".into(), (now_ms / 1000).to_string()),
            ];
            let response = self
                .send_json(reqwest::Method::GET, url, params, None)
                .await?;
            let status = response.get("status").and_then(|v| v.as_i64()).unwrap_or(0);
            if status != 200 {
                return Ok(response);
            }
            let data_status = response
                .pointer("/data/status")
                .and_then(|v| v.as_i64())
                .unwrap_or(-1);
            if data_status == 2 {
                return Ok(response);
            }
            retry_index += 1;
            if retry_index > 120 {
                return Ok(response);
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    fn config(&self) -> &HashMap<String, Value> {
        &self.config
    }
}
