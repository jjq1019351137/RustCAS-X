//! Disk cache for Douban poster proxy (`/media/proxy-image`).
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_TTL: i64 = 7 * 24 * 3600;
const DEFAULT_MAX_FILE: u64 = 5 * 1024 * 1024;
const DEFAULT_MAX_TOTAL: u64 = 200 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ProxyImageConfig {
    pub enabled: bool,
    pub cache_dir: PathBuf,
    pub ttl_seconds: i64,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
}

impl ProxyImageConfig {
    pub fn from_data_dir(data_dir: &Path) -> Self {
        let ttl = std::env::var("MEDIA_PROXY_IMAGE_CACHE_TTL_SECONDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_TTL)
            .max(1);
        let max_file = std::env::var("MEDIA_PROXY_IMAGE_CACHE_MAX_FILE_BYTES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_MAX_FILE);
        let max_total = std::env::var("MEDIA_PROXY_IMAGE_CACHE_MAX_TOTAL_BYTES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_MAX_TOTAL);
        let enabled = std::env::var("MEDIA_PROXY_IMAGE_CACHE_ENABLED")
            .map(|s| !matches!(s.to_ascii_lowercase().as_str(), "0" | "false" | "off" | "no"))
            .unwrap_or(true);
        let cache_dir = std::env::var("MEDIA_PROXY_IMAGE_CACHE_DIR")
            .ok()
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| data_dir.join("cache").join("proxy_image"));
        Self {
            enabled,
            cache_dir,
            ttl_seconds: ttl,
            max_file_bytes: max_file,
            max_total_bytes: max_total,
        }
    }
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    hex::encode(h.finalize())
}

fn paths_for(cache_dir: &Path, url: &str) -> (String, PathBuf, PathBuf, PathBuf) {
    let key = sha256_hex(url);
    let shard = &key[..2];
    let shard_dir = cache_dir.join(shard);
    let meta = shard_dir.join(format!("{key}.json"));
    let bin = shard_dir.join(format!("{key}.bin"));
    (key, shard_dir, meta, bin)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Meta {
    url: String,
    content_type: String,
    etag: String,
    size: u64,
    created_at: i64,
    expires_at: i64,
}

pub fn host_allowed(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return false;
    }
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    host.ends_with("doubanio.com") || host.ends_with("douban.com")
}

pub struct CachedImage {
    pub bytes: Vec<u8>,
    pub content_type: String,
    pub etag: String,
    pub cache_control: String,
}

pub fn try_read_cache(cfg: &ProxyImageConfig, url: &str) -> Option<CachedImage> {
    if !cfg.enabled {
        return None;
    }
    let (key, _, meta_path, bin_path) = paths_for(&cfg.cache_dir, url);
    let meta_raw = fs::read_to_string(&meta_path).ok()?;
    let meta: Meta = serde_json::from_str(&meta_raw).ok()?;
    if meta.expires_at <= now_ts() {
        let _ = fs::remove_file(&meta_path);
        let _ = fs::remove_file(&bin_path);
        return None;
    }
    let bytes = fs::read(&bin_path).ok()?;
    if !meta.content_type.to_ascii_lowercase().starts_with("image/") {
        return None;
    }
    Some(CachedImage {
        bytes,
        content_type: meta.content_type,
        etag: format!("\"{}\"", if meta.etag.is_empty() { key } else { meta.etag }),
        cache_control: format!("public, max-age={}", cfg.ttl_seconds),
    })
}

pub fn store_cache(cfg: &ProxyImageConfig, url: &str, content_type: &str, bytes: &[u8]) {
    if !cfg.enabled {
        return;
    }
    if bytes.len() as u64 > cfg.max_file_bytes && cfg.max_file_bytes > 0 {
        return;
    }
    if !content_type.to_ascii_lowercase().starts_with("image/") {
        return;
    }
    let (key, shard_dir, meta_path, bin_path) = paths_for(&cfg.cache_dir, url);
    if fs::create_dir_all(&shard_dir).is_err() {
        return;
    }
    let tmp = shard_dir.join(format!("{key}.tmp"));
    if fs::write(&tmp, bytes).is_err() {
        return;
    }
    if fs::rename(&tmp, &bin_path).is_err() {
        let _ = fs::remove_file(&tmp);
        return;
    }
    let now = now_ts();
    let meta = Meta {
        url: url.to_string(),
        content_type: content_type.to_string(),
        etag: key.clone(),
        size: bytes.len() as u64,
        created_at: now,
        expires_at: now + cfg.ttl_seconds,
    };
    if let Ok(raw) = serde_json::to_string(&meta) {
        let _ = fs::write(meta_path, raw);
    }
    enforce_total_budget(cfg);
}

fn walk_entries(cache_dir: &Path) -> Vec<(PathBuf, PathBuf, i64, u64)> {
    let mut out = Vec::new();
    let Ok(shards) = fs::read_dir(cache_dir) else {
        return out;
    };
    for shard in shards.flatten() {
        let path = shard.path();
        if !path.is_dir() {
            continue;
        }
        let Ok(files) = fs::read_dir(&path) else {
            continue;
        };
        for f in files.flatten() {
            let p = f.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(raw) = fs::read_to_string(&p) else {
                continue;
            };
            let Ok(meta) = serde_json::from_str::<Meta>(&raw) else {
                continue;
            };
            let bin = p.with_extension("bin");
            out.push((p, bin, meta.expires_at, meta.size));
        }
    }
    out
}

fn enforce_total_budget(cfg: &ProxyImageConfig) {
    if cfg.max_total_bytes == 0 {
        return;
    }
    let mut entries = walk_entries(&cfg.cache_dir);
    let mut total: u64 = entries.iter().map(|e| e.3).sum();
    if total <= cfg.max_total_bytes {
        return;
    }
    entries.sort_by_key(|e| e.2); // oldest expiry first
    for (meta, bin, _, size) in entries {
        if total <= cfg.max_total_bytes {
            break;
        }
        let _ = fs::remove_file(meta);
        let _ = fs::remove_file(bin);
        total = total.saturating_sub(size);
    }
}

pub fn stats(cfg: &ProxyImageConfig) -> serde_json::Value {
    let now = now_ts();
    let entries = walk_entries(&cfg.cache_dir);
    let total_files = entries.len();
    let total_bytes: u64 = entries.iter().map(|e| e.3).sum();
    let stale_files = entries.iter().filter(|e| e.2 <= now).count();
    serde_json::json!({
        "enabled": cfg.enabled,
        "cache_dir": cfg.cache_dir.to_string_lossy(),
        "ttl_seconds": cfg.ttl_seconds,
        "max_file_bytes": cfg.max_file_bytes,
        "max_total_bytes": cfg.max_total_bytes,
        "total_files": total_files,
        "total_bytes": total_bytes,
        "stale_files": stale_files
    })
}

pub fn purge_expired(cfg: &ProxyImageConfig) -> (usize, u64) {
    let now = now_ts();
    let mut deleted_files = 0usize;
    let mut deleted_bytes = 0u64;
    for (meta, bin, exp, size) in walk_entries(&cfg.cache_dir) {
        if exp > now {
            continue;
        }
        let _ = fs::remove_file(meta);
        let _ = fs::remove_file(bin);
        deleted_files += 1;
        deleted_bytes += size;
    }
    (deleted_files, deleted_bytes)
}

pub fn clear_all(cfg: &ProxyImageConfig) -> (usize, u64) {
    let mut deleted_files = 0usize;
    let mut deleted_bytes = 0u64;
    for (meta, bin, _, size) in walk_entries(&cfg.cache_dir) {
        let _ = fs::remove_file(meta);
        let _ = fs::remove_file(bin);
        deleted_files += 1;
        deleted_bytes += size;
    }
    (deleted_files, deleted_bytes)
}

pub async fn fetch_image(url: &str) -> anyhow::Result<(Vec<u8>, String)> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let resp = client
        .get(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (iPhone; CPU iPhone OS 14_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0 Mobile/15E148 Safari/604.1",
        )
        .header("Referer", "https://movie.douban.com/")
        .header(
            "Accept",
            "image/avif,image/webp,image/apng,image/*,*/*;q=0.8",
        )
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("upstream {}", resp.status());
    }
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .split(';')
        .next()
        .unwrap_or("image/jpeg")
        .trim()
        .to_string();
    let bytes = resp.bytes().await?.to_vec();
    Ok((bytes, ct))
}
