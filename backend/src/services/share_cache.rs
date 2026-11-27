//! Invalid share links + share preview-batch DB cache.
use crate::services::drive_util::detect_drive_type;
use chrono::{Duration, Utc};
use serde_json::{json, Value};
use sqlx::SqlitePool;

pub async fn upsert_invalid(db: &SqlitePool, shareurl: &str, message: Option<&str>) -> anyhow::Result<()> {
    let url = shareurl.trim();
    if url.is_empty() {
        return Ok(());
    }
    let dt = detect_drive_type(url);
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO invalid_share_links (shareurl, drive_type, message, hit_count, created_at, updated_at)
        VALUES (?, ?, ?, 1, ?, ?)
        ON CONFLICT(shareurl) DO UPDATE SET
            drive_type=excluded.drive_type,
            message=COALESCE(excluded.message, invalid_share_links.message),
            hit_count=invalid_share_links.hit_count + 1,
            updated_at=excluded.updated_at
        "#,
    )
    .bind(url)
    .bind(dt)
    .bind(message)
    .bind(&now)
    .bind(&now)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn is_invalid(db: &SqlitePool, shareurl: &str) -> bool {
    let url = shareurl.trim();
    if url.is_empty() {
        return false;
    }
    sqlx::query_scalar::<_, i64>("SELECT 1 FROM invalid_share_links WHERE shareurl=? LIMIT 1")
        .bind(url)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .is_some()
}

pub async fn cache_get(db: &SqlitePool, shareurl: &str) -> Option<Value> {
    let now = Utc::now().to_rfc3339();
    let row: Option<(i64, String, Option<String>)> = sqlx::query_as(
        r#"
        SELECT ok, COALESCE(payload_json, '{}'), message
        FROM share_preview_batch_cache
        WHERE shareurl=? AND expires_at > ?
        LIMIT 1
        "#,
    )
    .bind(shareurl.trim())
    .bind(&now)
    .fetch_optional(db)
    .await
    .ok()
    .flatten();
    let Some((ok, payload, message)) = row else {
        return None;
    };
    let _ = sqlx::query(
        "UPDATE share_preview_batch_cache SET hit_count=hit_count+1, updated_at=? WHERE shareurl=?",
    )
    .bind(&now)
    .bind(shareurl.trim())
    .execute(db)
    .await;
    let mut item: Value = serde_json::from_str(&payload).unwrap_or(json!({}));
    if let Some(obj) = item.as_object_mut() {
        obj.insert("shareurl".into(), json!(shareurl.trim()));
        obj.insert("ok".into(), json!(ok != 0));
        if let Some(m) = message {
            obj.entry("message".to_string()).or_insert(json!(m));
        }
    }
    Some(item)
}

pub async fn cache_set(db: &SqlitePool, item: &Value, ttl_hours: i64) -> anyhow::Result<()> {
    let url = item
        .get("shareurl")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if url.is_empty() {
        return Ok(());
    }
    let ok = item.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let message = item.get("message").and_then(|v| v.as_str());
    let dt = item
        .get("drive_type")
        .and_then(|v| v.as_str())
        .or_else(|| detect_drive_type(url));
    let now = Utc::now();
    let expires = (now + Duration::hours(ttl_hours.max(1))).to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO share_preview_batch_cache
            (shareurl, drive_type, ok, message, payload_json, checked_at, expires_at, hit_count, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?)
        ON CONFLICT(shareurl) DO UPDATE SET
            drive_type=excluded.drive_type,
            ok=excluded.ok,
            message=excluded.message,
            payload_json=excluded.payload_json,
            checked_at=excluded.checked_at,
            expires_at=excluded.expires_at,
            updated_at=excluded.updated_at
        "#,
    )
    .bind(url)
    .bind(dt)
    .bind(if ok { 1 } else { 0 })
    .bind(message)
    .bind(item.to_string())
    .bind(now.to_rfc3339())
    .bind(&expires)
    .bind(now.to_rfc3339())
    .bind(now.to_rfc3339())
    .execute(db)
    .await?;
    if !ok {
        let _ = upsert_invalid(db, url, message).await;
    }
    Ok(())
}

pub async fn list_invalid(
    db: &SqlitePool,
    page: i64,
    page_size: i64,
    q: Option<&str>,
    drive_type: Option<&str>,
) -> anyhow::Result<Value> {
    let page = page.max(1);
    let page_size = page_size.clamp(1, 200);
    let offset = (page - 1) * page_size;
    let like = q.map(|s| format!("%{}%", s.trim())).filter(|s| s != "%%");
    let dt = drive_type.map(|s| s.trim()).filter(|s| !s.is_empty());

    let total: i64 = match (like.as_ref(), dt) {
        (Some(pat), Some(d)) => {
            sqlx::query_scalar(
                "SELECT COUNT(1) FROM invalid_share_links WHERE shareurl LIKE ? AND drive_type=?",
            )
            .bind(pat)
            .bind(d)
            .fetch_one(db)
            .await?
        }
        (Some(pat), None) => {
            sqlx::query_scalar("SELECT COUNT(1) FROM invalid_share_links WHERE shareurl LIKE ?")
                .bind(pat)
                .fetch_one(db)
                .await?
        }
        (None, Some(d)) => {
            sqlx::query_scalar("SELECT COUNT(1) FROM invalid_share_links WHERE drive_type=?")
                .bind(d)
                .fetch_one(db)
                .await?
        }
        (None, None) => {
            sqlx::query_scalar("SELECT COUNT(1) FROM invalid_share_links")
                .fetch_one(db)
                .await?
        }
    };

    let rows = match (like.as_ref(), dt) {
        (Some(pat), Some(d)) => {
            sqlx::query_as::<
                _,
                (
                    String,
                    Option<String>,
                    Option<String>,
                    i64,
                    Option<String>,
                    Option<String>,
                ),
            >(
                r#"
                SELECT shareurl, drive_type, message, hit_count, created_at, updated_at
                FROM invalid_share_links
                WHERE shareurl LIKE ? AND drive_type=?
                ORDER BY updated_at DESC LIMIT ? OFFSET ?
                "#,
            )
            .bind(pat)
            .bind(d)
            .bind(page_size)
            .bind(offset)
            .fetch_all(db)
            .await?
        }
        (Some(pat), None) => {
            sqlx::query_as(
                r#"
                SELECT shareurl, drive_type, message, hit_count, created_at, updated_at
                FROM invalid_share_links
                WHERE shareurl LIKE ?
                ORDER BY updated_at DESC LIMIT ? OFFSET ?
                "#,
            )
            .bind(pat)
            .bind(page_size)
            .bind(offset)
            .fetch_all(db)
            .await?
        }
        (None, Some(d)) => {
            sqlx::query_as(
                r#"
                SELECT shareurl, drive_type, message, hit_count, created_at, updated_at
                FROM invalid_share_links
                WHERE drive_type=?
                ORDER BY updated_at DESC LIMIT ? OFFSET ?
                "#,
            )
            .bind(d)
            .bind(page_size)
            .bind(offset)
            .fetch_all(db)
            .await?
        }
        (None, None) => {
            sqlx::query_as(
                r#"
                SELECT shareurl, drive_type, message, hit_count, created_at, updated_at
                FROM invalid_share_links
                ORDER BY updated_at DESC LIMIT ? OFFSET ?
                "#,
            )
            .bind(page_size)
            .bind(offset)
            .fetch_all(db)
            .await?
        }
    };

    Ok(json!({
        "page": page,
        "page_size": page_size,
        "total": total,
        "items": rows.into_iter().map(|r| json!({
            "shareurl": r.0,
            "drive_type": r.1,
            "message": r.2,
            "hit_count": r.3,
            "created_at": r.4,
            "updated_at": r.5
        })).collect::<Vec<_>>()
    }))
}

pub async fn delete_invalid(db: &SqlitePool, shareurl: &str) -> anyhow::Result<u64> {
    let n = sqlx::query("DELETE FROM invalid_share_links WHERE shareurl=?")
        .bind(shareurl.trim())
        .execute(db)
        .await?
        .rows_affected();
    Ok(n)
}

pub async fn clear_invalid(db: &SqlitePool, drive_type: Option<&str>) -> anyhow::Result<u64> {
    let n = if let Some(dt) = drive_type.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        sqlx::query("DELETE FROM invalid_share_links WHERE drive_type=?")
            .bind(dt)
            .execute(db)
            .await?
            .rows_affected()
    } else {
        sqlx::query("DELETE FROM invalid_share_links")
            .execute(db)
            .await?
            .rows_affected()
    };
    Ok(n)
}

pub async fn list_preview_cache(
    db: &SqlitePool,
    page: i64,
    page_size: i64,
    q: Option<&str>,
    drive_type: Option<&str>,
    ok: Option<bool>,
    expired_only: bool,
) -> anyhow::Result<Value> {
    let page = page.max(1);
    let page_size = page_size.clamp(1, 200);
    let offset = (page - 1) * page_size;
    let now = Utc::now().to_rfc3339();
    let like = q.map(|s| format!("%{}%", s.trim())).filter(|s| s != "%%");
    let dt = drive_type.map(|s| s.trim()).filter(|s| !s.is_empty());
    let ok_i = ok.map(|b| if b { 1 } else { 0 });

    // Pull a wider window then filter in Rust to keep SQL simple.
    let rows = sqlx::query_as::<
        _,
        (
            String,
            Option<String>,
            i64,
            Option<String>,
            Option<String>,
            Option<String>,
            i64,
            Option<String>,
        ),
    >(
        r#"
        SELECT shareurl, drive_type, ok, message, checked_at, expires_at, hit_count, updated_at
        FROM share_preview_batch_cache
        ORDER BY updated_at DESC
        LIMIT 2000
        "#,
    )
    .fetch_all(db)
    .await?;

    let filtered: Vec<_> = rows
        .into_iter()
        .filter(|r| {
            if let Some(pat) = like.as_ref() {
                let needle = pat.trim_matches('%');
                if !needle.is_empty() && !r.0.contains(needle) {
                    return false;
                }
            }
            if let Some(d) = dt {
                if r.1.as_deref() != Some(d) {
                    return false;
                }
            }
            if let Some(o) = ok_i {
                if r.2 != o {
                    return false;
                }
            }
            let exp = r.5.as_deref().unwrap_or("");
            if expired_only {
                exp <= now.as_str()
            } else {
                exp > now.as_str()
            }
        })
        .collect();
    let total = filtered.len() as i64;
    let items: Vec<Value> = filtered
        .into_iter()
        .skip(offset as usize)
        .take(page_size as usize)
        .map(|r| {
            json!({
                "shareurl": r.0,
                "drive_type": r.1,
                "ok": r.2 != 0,
                "message": r.3,
                "checked_at": r.4,
                "expires_at": r.5,
                "hit_count": r.6,
                "updated_at": r.7
            })
        })
        .collect();

    Ok(json!({
        "page": page,
        "page_size": page_size,
        "total": total,
        "items": items
    }))
}

pub async fn delete_preview_cache(db: &SqlitePool, shareurl: &str) -> anyhow::Result<u64> {
    Ok(sqlx::query("DELETE FROM share_preview_batch_cache WHERE shareurl=?")
        .bind(shareurl.trim())
        .execute(db)
        .await?
        .rows_affected())
}

pub async fn purge_preview_cache(
    db: &SqlitePool,
    expired_only: bool,
    retention_seconds: i64,
) -> anyhow::Result<u64> {
    let cutoff = (Utc::now() - Duration::seconds(retention_seconds.max(0))).to_rfc3339();
    let n = if expired_only {
        sqlx::query("DELETE FROM share_preview_batch_cache WHERE expires_at <= ?")
            .bind(Utc::now().to_rfc3339())
            .execute(db)
            .await?
            .rows_affected()
    } else {
        sqlx::query("DELETE FROM share_preview_batch_cache WHERE checked_at < ?")
            .bind(&cutoff)
            .execute(db)
            .await?
            .rows_affected()
    };
    Ok(n)
}
