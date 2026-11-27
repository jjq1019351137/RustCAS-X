use crate::error::{AppError, AppResult};
use chrono::Utc;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::SqlitePool;

pub async fn get_json(pool: &SqlitePool, key: &str) -> AppResult<Option<Value>> {
    let raw: Option<String> = sqlx::query_scalar("SELECT value_json FROM app_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(raw.and_then(|s| serde_json::from_str(&s).ok()))
}

pub async fn get_or_default<T: DeserializeOwned + Serialize + Default>(
    pool: &SqlitePool,
    key: &str,
) -> AppResult<T> {
    if let Some(v) = get_json(pool, key).await? {
        serde_json::from_value(v).map_err(|e| {
            AppError::api(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "SETTINGS_PARSE",
                e.to_string(),
            )
        })
    } else {
        Ok(T::default())
    }
}

pub async fn put_json(pool: &SqlitePool, key: &str, value: &Value) -> AppResult<()> {
    let raw = serde_json::to_string(value).unwrap_or_else(|_| "{}".into());
    sqlx::query(
        r#"
        INSERT INTO app_settings (key, value_json, updated_at)
        VALUES (?, ?, ?)
        ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at
        "#,
    )
    .bind(key)
    .bind(raw)
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn put_typed<T: Serialize>(pool: &SqlitePool, key: &str, value: &T) -> AppResult<()> {
    let v = serde_json::to_value(value).unwrap_or_else(|_| json!({}));
    put_json(pool, key, &v).await
}
