use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Settings {
    pub app_name: String,
    pub api_prefix: String,
    pub listen_addr: String,
    pub data_dir: PathBuf,
    pub database_url: String,
    pub jwt_secret_key: String,
    pub jwt_algorithm: String,
    pub access_token_expires_seconds: i64,
    pub refresh_token_expires_seconds: i64,
    pub refresh_token_cookie_name: String,
    pub refresh_token_cookie_secure: bool,
    pub refresh_token_pepper: String,
    pub build_sha: Option<String>,
    pub build_tag: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct SecretsFile {
    jwt_secret_key: Option<String>,
    refresh_token_pepper: Option<String>,
    internal_runtime_notify_token: Option<String>,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn random_secret() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 48];
    rand::thread_rng().fill_bytes(&mut buf);
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, buf)
}

fn load_or_init_secrets(data_dir: &Path) -> Result<SecretsFile> {
    fs::create_dir_all(data_dir).ok();
    let path = data_dir.join("secrets.json");
    let mut data = if path.exists() {
        let raw = fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str::<SecretsFile>(&raw).unwrap_or_default()
    } else {
        SecretsFile::default()
    };

    let mut changed = false;
    if data
        .jwt_secret_key
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        data.jwt_secret_key = Some(random_secret());
        changed = true;
    }
    if data
        .refresh_token_pepper
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        data.refresh_token_pepper = Some(random_secret());
        changed = true;
    }
    if data
        .internal_runtime_notify_token
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        data.internal_runtime_notify_token = Some(random_secret());
        changed = true;
    }

    if changed {
        let pretty = serde_json::to_string_pretty(&data)?;
        fs::write(&path, pretty).with_context(|| format!("write secrets {}", path.display()))?;
    }
    Ok(data)
}

impl Settings {
    pub fn load() -> Result<Self> {
        let _ = dotenvy::dotenv();
        let data_dir = PathBuf::from(env_or("APP_DATA_DIR", "./data"));
        fs::create_dir_all(&data_dir).ok();

        let secrets = load_or_init_secrets(&data_dir)?;
        let jwt_secret_key = std::env::var("JWT_SECRET_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or(secrets.jwt_secret_key)
            .unwrap_or_else(random_secret);
        let refresh_token_pepper = std::env::var("REFRESH_TOKEN_PEPPER")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or(secrets.refresh_token_pepper)
            .unwrap_or_else(random_secret);

        let sqlite_path = std::env::var("SQLITE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data_dir.join("app.db"));
        if let Some(parent) = sqlite_path.parent() {
            fs::create_dir_all(parent).ok();
        }

        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            let abs = if sqlite_path.is_absolute() {
                sqlite_path.clone()
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(&sqlite_path)
            };
            format!("sqlite://{}?mode=rwc", abs.to_string_lossy().replace('\\', "/"))
        });

        Ok(Self {
            app_name: env_or("APP_NAME", "rustcas-x-api"),
            api_prefix: env_or("API_PREFIX", "/api"),
            listen_addr: env_or("LISTEN_ADDR", "0.0.0.0:8000"),
            data_dir,
            database_url,
            jwt_secret_key,
            jwt_algorithm: env_or("JWT_ALGORITHM", "HS256"),
            access_token_expires_seconds: env_or("ACCESS_TOKEN_EXPIRES_SECONDS", "7200")
                .parse()
                .unwrap_or(7200),
            refresh_token_expires_seconds: env_or("REFRESH_TOKEN_EXPIRES_SECONDS", "1209600")
                .parse()
                .unwrap_or(1_209_600),
            refresh_token_cookie_name: env_or("REFRESH_TOKEN_COOKIE_NAME", "refresh_token"),
            refresh_token_cookie_secure: env_or("REFRESH_TOKEN_COOKIE_SECURE", "false")
                .eq_ignore_ascii_case("true"),
            refresh_token_pepper,
            build_sha: std::env::var("BUILD_SHA").ok().filter(|s| !s.is_empty()),
            build_tag: std::env::var("BUILD_TAG").ok().filter(|s| !s.is_empty()),
        })
    }
}
