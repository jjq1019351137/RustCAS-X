use crate::runtime::deferred_delete;
use crate::state::AppState;
use chrono::Utc;
use serde_json::Value;

pub async fn run_once(state: &AppState, sync_task_id: i64) -> anyhow::Result<(i64, String)> {
    let row = sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT sync_task_uid, name, mode, source_json, target_json FROM sync_tasks WHERE id = ?",
    )
    .bind(sync_task_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| anyhow::anyhow!("sync task not found"))?;

    let (uid, name, mode, source_json, target_json) = row;
    sqlx::query("UPDATE sync_tasks SET is_running = 1, updated_at = ? WHERE id = ?")
        .bind(Utc::now().to_rfc3339())
        .bind(sync_task_id)
        .execute(&state.db)
        .await?;

    let exec_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO sync_executions (sync_task_id, status, message, run_log, started_at)
        VALUES (?, 'running', ?, '', ?)
        RETURNING id
        "#,
    )
    .bind(sync_task_id)
    .bind(format!("运行同步任务: {name}"))
    .bind(Utc::now().to_rfc3339())
    .fetch_one(&state.db)
    .await?;

    let mut log = String::new();
    log.push_str(&format!(
        "[{}] sync start name={name} mode={mode} uid={uid}\n",
        Utc::now().to_rfc3339()
    ));
    log.push_str(&format!("source={source_json}\n"));
    log.push_str(&format!("target={target_json}\n"));

    let source: Value = serde_json::from_str(&source_json).unwrap_or_default();
    let target: Value = serde_json::from_str(&target_json).unwrap_or_default();
    let src_type = source.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let dst_type = target.get("type").and_then(|v| v.as_str()).unwrap_or("");
    log.push_str(&format!("plan: {src_type} -> {dst_type}\n"));
    log.push_str("note: file walk/copy/delete pipeline migrating; deferred-delete queue is live\n");

    // Demonstrate deferred delete queue wiring for local targets.
    if dst_type == "local" {
        if let Some(base) = target.get("path").and_then(|v| v.as_str()) {
            let id = deferred_delete::enqueue(state, Some(&uid), base, ".rustcas-x-keep", 0).await?;
            log.push_str(&format!("enqueued deferred noop marker id={id} (ignored if missing)\n"));
        }
    }

    log.push_str(&format!("[{}] done (skeleton)\n", Utc::now().to_rfc3339()));

    sqlx::query(
        r#"
        UPDATE sync_executions
        SET status='success', message=?, run_log=?, finished_at=?
        WHERE id=?
        "#,
    )
    .bind("skeleton success")
    .bind(&log)
    .bind(Utc::now().to_rfc3339())
    .bind(exec_id)
    .execute(&state.db)
    .await?;

    sqlx::query("UPDATE sync_tasks SET is_running = 0, updated_at = ? WHERE id = ?")
        .bind(Utc::now().to_rfc3339())
        .bind(sync_task_id)
        .execute(&state.db)
        .await?;

    Ok((exec_id, log))
}
