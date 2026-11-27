use crate::adapters::create_adapter;
use crate::state::AppState;
use chrono::Utc;
use serde_json::json;

/// Drama run: load account cookie → extract_url → get_stoken → get_detail (list share).
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

    let account_row = if let Some(name) = account_name.as_ref().filter(|s| !s.trim().is_empty()) {
        sqlx::query_as::<_, (String, String, Option<String>)>(
            "SELECT drive_type, cookie, config_json FROM drive_accounts WHERE name = ? LIMIT 1",
        )
        .bind(name)
        .fetch_optional(&state.db)
        .await?
    } else {
        sqlx::query_as::<_, (String, String, Option<String>)>(
            "SELECT drive_type, cookie, config_json FROM drive_accounts WHERE enabled = 1 ORDER BY is_default DESC, id ASC LIMIT 1",
        )
        .fetch_optional(&state.db)
        .await?
    };

    let Some((drive_type, cookie, config_json)) = account_row else {
        let msg = "no drive account configured";
        finish_exec(state, exec_id, "failed", msg, &format!("{log}error: {msg}\n")).await?;
        return Ok((exec_id, format!("{log}error: {msg}\n")));
    };

    let config = config_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| json!({}));
    let mut adapter = create_adapter(&drive_type, Some(&config), Some(&cookie))?;
    log.push_str(&format!("drive={drive_type}\n"));

    match adapter.init().await {
        Ok(info) => {
            let ok = info.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            log.push_str(&format!(
                "init ok={ok} nickname={}\n",
                info.get("nickname").and_then(|v| v.as_str()).unwrap_or("-")
            ));
            if !ok {
                let msg = info
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("init failed");
                finish_exec(state, exec_id, "failed", msg, &format!("{log}error: {msg}\n")).await?;
                return Ok((exec_id, format!("{log}error: {msg}\n")));
            }
        }
        Err(err) => {
            finish_exec(
                state,
                exec_id,
                "failed",
                &err.to_string(),
                &format!("{log}error: {err}\n"),
            )
            .await?;
            return Ok((exec_id, format!("{log}error: {err}\n")));
        }
    }

    let Some(url) = shareurl.as_ref().filter(|s| !s.trim().is_empty()) else {
        let msg = "empty shareurl";
        finish_exec(state, exec_id, "skipped", msg, &format!("{log}skip: {msg}\n")).await?;
        return Ok((exec_id, format!("{log}skip: {msg}\n")));
    };

    let extracted = adapter.extract_url(url);
    log.push_str(&format!(
        "extract_url pwd_id={:?} passcode_len={} pdir_fid={}\n",
        extracted.pwd_id,
        extracted.passcode.len(),
        extracted.pdir_fid
    ));
    let Some(pwd_id) = extracted.pwd_id.clone() else {
        let msg = "share pwd_id not parsed";
        finish_exec(state, exec_id, "failed", msg, &format!("{log}error: {msg}\n")).await?;
        return Ok((exec_id, format!("{log}error: {msg}\n")));
    };

    let stoken_resp = adapter.get_stoken(&pwd_id, &extracted.passcode).await?;
    let stoken = stoken_resp
        .pointer("/data/stoken")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            // quark sometimes nests differently
            stoken_resp
                .pointer("/data/stoken")
                .map(|v| v.to_string().trim_matches('"').to_string())
        });

    let status = stoken_resp
        .get("status")
        .or_else(|| stoken_resp.get("code"))
        .cloned()
        .unwrap_or(json!(null));
    log.push_str(&format!("get_stoken status={status}\n"));

    let Some(stoken) = stoken.filter(|s| !s.is_empty()) else {
        let msg = stoken_resp
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("get_stoken failed");
        finish_exec(state, exec_id, "failed", msg, &format!("{log}error: {msg}\n")).await?;
        return Ok((exec_id, format!("{log}error: {msg}\n")));
    };

    let pdir = match &extracted.pdir_fid {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => "0".into(),
    };
    let detail = adapter.get_detail(&pwd_id, &stoken, &pdir).await?;
    let count = detail
        .pointer("/data/list")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    log.push_str(&format!("share detail items={count}\n"));
    if let Some(list) = detail.pointer("/data/list").and_then(|v| v.as_array()) {
        for item in list.iter().take(20) {
            let name = item
                .get("file_name")
                .or_else(|| item.get("fileName"))
                .or_else(|| item.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let fid = item
                .get("fid")
                .or_else(|| item.get("file_id"))
                .map(|v| v.to_string())
                .unwrap_or_default();
            log.push_str(&format!("  - {name} ({fid})\n"));
        }
        if list.len() > 20 {
            log.push_str(&format!("  ... {} more\n", list.len() - 20));
        }
    }

    log.push_str("note: save_file/transfer + TMDB rename still migrating\n");
    log.push_str(&format!("[{}] done\n", Utc::now().to_rfc3339()));
    finish_exec(state, exec_id, "success", "share listed", &log).await?;
    Ok((exec_id, log))
}

async fn finish_exec(
    state: &AppState,
    exec_id: i64,
    status: &str,
    message: &str,
    log: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE task_executions
        SET status=?, stage='done', message=?, run_log=?, finished_at=?
        WHERE id=?
        "#,
    )
    .bind(status)
    .bind(message)
    .bind(log)
    .bind(Utc::now().to_rfc3339())
    .bind(exec_id)
    .execute(&state.db)
    .await?;
    Ok(())
}
