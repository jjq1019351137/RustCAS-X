//! OpenList / AList HTTP client (list / mkdir / get / put / copy).

use crate::settings_store::get_or_default;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenListConfig {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub token: String,
}

impl OpenListConfig {
    pub fn configured(&self) -> bool {
        !self.url.trim().is_empty() && !self.token.trim().is_empty()
    }

    pub fn base(&self) -> String {
        self.url.trim().trim_end_matches('/').to_string()
    }
}

pub async fn load_config(state: &AppState) -> anyhow::Result<OpenListConfig> {
    Ok(get_or_default(&state.db, "openlist_config").await?)
}

pub struct OpenListClient {
    base: String,
    token: String,
    http: reqwest::Client,
}

impl OpenListClient {
    pub fn new(cfg: &OpenListConfig) -> anyhow::Result<Self> {
        if !cfg.configured() {
            anyhow::bail!("OpenList not configured");
        }
        Ok(Self {
            base: cfg.base(),
            token: cfg.token.trim().to_string(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        })
    }

    async fn post_json(&self, path: &str, body: Value) -> anyhow::Result<Value> {
        let resp = self
            .http
            .post(format!("{}{path}", self.base))
            .header("Authorization", &self.token)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let v: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let msg = v
                .get("message")
                .and_then(|x| x.as_str())
                .unwrap_or("openlist http error");
            anyhow::bail!("{msg} (http {status})");
        }
        let code = v.get("code").and_then(|c| c.as_i64()).unwrap_or(200);
        if code != 200 {
            let msg = v
                .get("message")
                .and_then(|x| x.as_str())
                .unwrap_or("openlist api error");
            anyhow::bail!("{msg} (code {code})");
        }
        Ok(v)
    }

    pub async fn fs_list(&self, path: &str, refresh: bool) -> anyhow::Result<Vec<Value>> {
        let body = json!({
            "path": path,
            "password": "",
            "page": 1,
            "per_page": 0,
            "refresh": refresh
        });
        let v = match self.post_json("/api/fs/listGet", body.clone()).await {
            Ok(v) => v,
            Err(_) => self.post_json("/api/fs/list", body).await?,
        };
        let content = v
            .pointer("/data/content")
            .or_else(|| v.pointer("/data/items"))
            .cloned()
            .unwrap_or_else(|| json!([]));
        Ok(content.as_array().cloned().unwrap_or_default())
    }

    pub async fn fs_mkdir(&self, path: &str) -> anyhow::Result<Value> {
        self.post_json("/api/fs/mkdir", json!({ "path": path })).await
    }

    pub async fn fs_get(&self, path: &str) -> anyhow::Result<Value> {
        self.post_json("/api/fs/get", json!({ "path": path, "password": "" }))
            .await
    }

    pub async fn fs_copy(
        &self,
        src_dir: &str,
        dst_dir: &str,
        names: &[String],
        overwrite: bool,
    ) -> anyhow::Result<Value> {
        self.post_json(
            "/api/fs/copy",
            json!({
                "src_dir": src_dir,
                "dst_dir": dst_dir,
                "names": names,
                "overwrite": overwrite,
                "skip_existing": !overwrite,
                "merge": false
            }),
        )
        .await
    }

    pub async fn ensure_dir(&self, path: &str) -> anyhow::Result<()> {
        let path = normalize_abs(path);
        if path == "/" {
            return Ok(());
        }
        let mut cur = String::new();
        for part in path.trim_start_matches('/').split('/') {
            if part.is_empty() {
                continue;
            }
            cur.push('/');
            cur.push_str(part);
            let _ = self.fs_mkdir(&cur).await;
        }
        Ok(())
    }

    pub async fn download_to_file(&self, remote_path: &str, local: &Path) -> anyhow::Result<u64> {
        let meta = self.fs_get(remote_path).await?;
        let raw = meta
            .pointer("/data/raw_url")
            .or_else(|| meta.pointer("/data/rawURL"))
            .or_else(|| meta.pointer("/data/rawUrl"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("openlist missing raw_url for {remote_path}"))?;
        if let Some(parent) = local.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let resp = self.http.get(raw).send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("download failed http {}", resp.status());
        }
        let bytes = resp.bytes().await?;
        let n = bytes.len() as u64;
        std::fs::write(local, &bytes)?;
        Ok(n)
    }

    pub async fn put_file(&self, remote_path: &str, local: &Path) -> anyhow::Result<()> {
        let data = std::fs::read(local)?;
        let encoded = urlencoding::encode(remote_path);
        let resp = self
            .http
            .put(format!("{}/api/fs/put", self.base))
            .header("Authorization", &self.token)
            .header("File-Path", encoded.as_ref())
            .header("Content-Length", data.len().to_string())
            .body(data)
            .send()
            .await?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("fs_put failed: {body}");
        }
        Ok(())
    }

    /// Recursively list relative file paths under `base` (files only).
    pub async fn walk_files(&self, base: &str) -> anyhow::Result<Vec<(String, u64)>> {
        let base = normalize_abs(base);
        let mut out = Vec::new();
        let mut stack = vec![base.clone()];
        while let Some(dir) = stack.pop() {
            let items = self.fs_list(&dir, false).await?;
            for item in items {
                let name = item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if name.is_empty() || name.starts_with('.') {
                    continue;
                }
                let is_dir = item
                    .get("is_dir")
                    .or_else(|| item.get("isDir"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let abs = join_abs(&dir, &name);
                if is_dir {
                    stack.push(abs);
                } else {
                    let size = item.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                    let rel = rel_from(&base, &abs);
                    out.push((rel, size));
                }
            }
        }
        Ok(out)
    }
}

pub fn normalize_abs(path: &str) -> String {
    let mut s = path.trim().replace('\\', "/");
    if s.is_empty() {
        return "/".into();
    }
    if !s.starts_with('/') {
        s = format!("/{s}");
    }
    while s.contains("//") {
        s = s.replace("//", "/");
    }
    if s.len() > 1 {
        s = s.trim_end_matches('/').to_string();
    }
    s
}

fn join_abs(dir: &str, name: &str) -> String {
    if dir == "/" {
        format!("/{name}")
    } else {
        format!("{dir}/{name}")
    }
}

fn rel_from(base: &str, abs: &str) -> String {
    let base = if base == "/" {
        ""
    } else {
        base.trim_end_matches('/')
    };
    let rest = abs.strip_prefix(base).unwrap_or(abs).trim_start_matches('/');
    rest.to_string()
}

pub fn join_remote(base: &str, rel: &str) -> String {
    let base = normalize_abs(base);
    let rel = rel.trim().trim_start_matches('/');
    if rel.is_empty() {
        return base;
    }
    if base == "/" {
        format!("/{rel}")
    } else {
        format!("{base}/{rel}")
    }
}
