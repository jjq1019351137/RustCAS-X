use crate::state::AppState;
use chrono::Utc;

pub async fn process_due(state: &AppState) -> anyhow::Result<()> {
    let now = Utc::now().to_rfc3339();
    let rows = sqlx::query_as::<_, (i64, String, String)>(
        r#"
        SELECT id, base_path, dst_rel
        FROM deferred_sync_deletes
        WHERE status = 'pending' AND due_at <= ?
        ORDER BY due_at ASC
        LIMIT 20
        "#,
    )
    .bind(&now)
    .fetch_all(&state.db)
    .await?;

    for (id, base_path, dst_rel) in rows {
        // Wave-4: filesystem delete when target is local; remote delete via adapters later.
        let path = std::path::Path::new(&base_path).join(&dst_rel);
        let result = if path.exists() {
            if path.is_dir() {
                tokio::fs::remove_dir_all(&path).await
            } else {
                tokio::fs::remove_file(&path).await
            }
        } else {
            Ok(())
        };

        match result {
            Ok(()) => {
                sqlx::query(
                    "UPDATE deferred_sync_deletes SET status='done', updated_at=?, last_error=NULL WHERE id=?",
                )
                .bind(Utc::now().to_rfc3339())
                .bind(id)
                .execute(&state.db)
                .await?;
                tracing::info!(id, path = %path.display(), "deferred delete done");
            }
            Err(err) => {
                sqlx::query(
                    "UPDATE deferred_sync_deletes SET attempts=attempts+1, last_error=?, updated_at=? WHERE id=?",
                )
                .bind(err.to_string())
                .bind(Utc::now().to_rfc3339())
                .bind(id)
                .execute(&state.db)
                .await?;
                tracing::warn!(id, error = %err, "deferred delete failed");
            }
        }
    }
    Ok(())
}

pub async fn enqueue(
    state: &AppState,
    sync_task_uid: Option<&str>,
    base_path: &str,
    dst_rel: &str,
    delay_secs: i64,
) -> anyhow::Result<i64> {
    let due = Utc::now() + chrono::Duration::seconds(delay_secs.max(0));
    let id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO deferred_sync_deletes (sync_task_uid, base_path, dst_rel, due_at, status)
        VALUES (?, ?, ?, ?, 'pending')
        RETURNING id
        "#,
    )
    .bind(sync_task_uid)
    .bind(base_path)
    .bind(dst_rel)
    .bind(due.to_rfc3339())
    .fetch_one(&state.db)
    .await?;
    Ok(id)
}
