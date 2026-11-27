use crate::adapters::create_adapter;
use crate::state::AppState;
use chrono::Utc;
use serde_json::json;

/// Minimal drama run: validate share URL via adapter extract_url, write execution log.
pub async fn run_once(state: &AppState, task_id: i64) -> anyhow::Result<(i64, String)> {
    let row = sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<String>, String)>(
        "SELECT taskname, shareurl, account_name, tmdb_media_type, extra_json FROM tasks WHERE id = ?",
    )
    .bind(task_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| anyhow::anyhow!("task not found"))?;

    let (taskname, shareurl, account_name, _media, _extra) = row;
    let exec_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO task_executions (task_id, status, stage, message, run_log, started_at)
        VALUES (?, 'running', 'prepare', ?, '', ?)
        RETURNING id
        "#,
    )
    .bind(task_id)
    .bind(format!("运行追剧任务: {taskname}"))
    .bind(Utc::now().to_rfc3339())
    .fetch_one(&state.db)
    .await?;

    let mut log = String::new();
    log.push_str(&format!("[{}] start task={taskname}\n", Utc::now().to_rfc3339()));

    let drive_type = if let Some(name) = account_name.as_ref() {
        sqlx::query_scalar::<_, String>("SELECT drive_type FROM drive_accounts WHERE name = ? LIMIT 1")
            .bind(name)
            .fetch_optional(&state.db)
            .await?
            .unwrap_or_else(|| "quark".into())
    } else {
        sqlx::query_scalar::<_, String>(
            "SELECT drive_type FROM drive_accounts WHERE enabled = 1 ORDER BY is_default DESC, id ASC LIMIT 1",
        )
        .fetch_optional(&state.db)
        .await?
        .unwrap_or_else(|| "quark".into())
    };

    let adapter = create_adapter(&drive_type, None, None)?;
    if let Some(url) = shareurl.as_ref().filter(|s| !s.trim().is_empty()) {
        let extracted = adapter.extract_url(url);
        log.push_str(&format!(
            "extract_url drive={drive_type} pwd_id={:?} passcode_len={} pdir_fid={}\n",
            extracted.pwd_id,
            extracted.passcode.len(),
            extracted.pdir_fid
        ));
        if extracted.pwd_id.is_none() {
            log.push_str("warn: share pwd_id not parsed\n");
        }
    } else {
        log.push_str("skip: empty shareurl\n");
    }

    log.push_str(
        "note: full transfer/lsdir/TMDB update pipeline still migrating (Wave 3+)\n",
    );
    log.push_str(&format!("[{}] done (skeleton)\n", Utc::now().to_rfc3339()));

    sqlx::query(
        r#"
        UPDATE task_executions
        SET status='success', stage='done', message=?, run_log=?, finished_at=?
        WHERE id=?
        "#,
    )
    .bind("skeleton success")
    .bind(&log)
    .bind(Utc::now().to_rfc3339())
    .bind(exec_id)
    .execute(&state.db)
    .await?;

    let _ = json!({ "exec_id": exec_id });
    Ok((exec_id, log))
}
