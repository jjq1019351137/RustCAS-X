//! Best-effort audit log writer (lean SQLite schema).
use chrono::Utc;
use sqlx::SqlitePool;

pub async fn write(
    db: &SqlitePool,
    actor_user_id: Option<i64>,
    action: &str,
    target_type: Option<&str>,
    target_id: Option<&str>,
    detail: Option<&str>,
    success: bool,
) {
    let _ = sqlx::query(
        r#"
        INSERT INTO audit_logs
            (actor_user_id, action, target_type, target_id, success, detail, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(actor_user_id)
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(if success { 1 } else { 0 })
    .bind(detail)
    .bind(Utc::now().to_rfc3339())
    .execute(db)
    .await;
}

pub async fn write_user(
    db: &SqlitePool,
    actor_user_id: i64,
    action: &str,
    target_type: &str,
    target_id: impl ToString,
    detail: Option<&str>,
) {
    write(
        db,
        Some(actor_user_id),
        action,
        Some(target_type),
        Some(&target_id.to_string()),
        detail,
        true,
    )
    .await;
}
