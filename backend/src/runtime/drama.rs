use crate::adapters::create_adapter;
use crate::state::AppState;
use chrono::Utc;
use serde_json::{json, Value};

/// Drama run: init → extract → stoken → detail → mkdir(savepath) → save_file → query_task.
pub async fn run_once(state: &AppState, task_id: i64) -> anyhow::Result<(i64, String)> {
    let row = sqlx::query_as::<
        _,
        (String, Option<String>, Option<String>, Option<String>, Option<String>),
    >(
        "SELECT taskname, shareurl, savepath, account_name, tmdb_media_type FROM tasks WHERE id = ?",
    )
    .bind(task_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| anyhow::anyhow!("task not found"))?;

    let (taskname, shareurl, savepath, account_name, _media) = row;
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
        return finish(state, exec_id, "failed", "no drive account", &mut log).await;
    };

    let config = config_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| json!({}));
    let mut adapter = create_adapter(&drive_type, Some(&config), Some(&cookie))?;
    log.push_str(&format!("drive={drive_type}\n"));

    let info = adapter.init().await?;
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
        return finish(state, exec_id, "failed", msg, &mut log).await;
    }

    let Some(url) = shareurl.as_ref().filter(|s| !s.trim().is_empty()) else {
        return finish(state, exec_id, "skipped", "empty shareurl", &mut log).await;
    };

    let extracted = adapter.extract_url(url);
    log.push_str(&format!(
        "extract_url pwd_id={:?} passcode_len={} pdir_fid={}\n",
        extracted.pwd_id,
        extracted.passcode.len(),
        extracted.pdir_fid
    ));
    let Some(pwd_id) = extracted.pwd_id.clone() else {
        return finish(state, exec_id, "failed", "share pwd_id not parsed", &mut log).await;
    };

    let stoken_resp = adapter.get_stoken(&pwd_id, &extracted.passcode).await?;
    let stoken = extract_stoken(&stoken_resp);
    log.push_str(&format!(
        "get_stoken status={:?}\n",
        stoken_resp.get("status").or_else(|| stoken_resp.get("code"))
    ));
    let Some(stoken) = stoken else {
        let msg = stoken_resp
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("get_stoken failed");
        return finish(state, exec_id, "failed", msg, &mut log).await;
    };

    let pdir = value_as_fid(&extracted.pdir_fid);
    let detail = adapter.get_detail(&pwd_id, &stoken, &pdir).await?;
    let items = detail
        .pointer("/data/list")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    log.push_str(&format!("share detail items={}\n", items.len()));
    if items.is_empty() {
        return finish(state, exec_id, "skipped", "share empty", &mut log).await;
    }

    let dest_path = savepath
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("/{}", taskname));
    log.push_str(&format!("mkdir savepath={dest_path}\n"));
    let mkdir_ret = adapter.mkdir(&dest_path).await?;
    let mut dest_fid = mkdir_ret
        .pointer("/data/fid")
        .map(|v| v.to_string().trim_matches('"').to_string())
        .filter(|s| !s.is_empty());
    if dest_fid.is_none() {
        let fids = adapter.get_fids(&[dest_path.clone()]).await.unwrap_or_default();
        dest_fid = fids
            .first()
            .and_then(|v| v.get("fid"))
            .map(|v| v.to_string().trim_matches('"').to_string())
            .filter(|s| !s.is_empty());
        log.push_str(&format!("get_fids fallback={:?}\n", dest_fid));
    }
    let Some(dest_fid) = dest_fid else {
        let msg = mkdir_ret
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("mkdir/get_fids failed");
        return finish(state, exec_id, "failed", msg, &mut log).await;
    };
    log.push_str(&format!("dest_fid={dest_fid}\n"));

    let mut fid_list = Vec::new();
    let mut token_list = Vec::new();
    for item in &items {
        let fid = item
            .get("fid")
            .or_else(|| item.get("file_id"))
            .or_else(|| item.get("fileId"))
            .map(|v| v.to_string().trim_matches('"').to_string())
            .filter(|s| !s.is_empty());
        let Some(fid) = fid else { continue };
        let token = item
            .get("share_fid_token")
            .or_else(|| item.get("shareFidToken"))
            .map(|v| v.as_str().unwrap_or("").to_string())
            .unwrap_or_default();
        // For cloud189, encode helper token JSON when missing
        let token = if token.is_empty() && drive_type == "cloud189" {
            let name = item
                .get("file_name")
                .or_else(|| item.get("fileName"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let dir = if item.get("dir").and_then(|v| v.as_bool()).unwrap_or(false) {
                1
            } else {
                0
            };
            json!({ "pid": "0", "dir": dir, "name": name }).to_string()
        } else {
            token
        };
        let name = item
            .get("file_name")
            .or_else(|| item.get("fileName"))
            .or_else(|| item.get("title"))
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        log.push_str(&format!("save candidate: {name} ({fid})\n"));
        fid_list.push(fid);
        token_list.push(token);
    }
    if fid_list.is_empty() {
        return finish(state, exec_id, "skipped", "no transferable fids", &mut log).await;
    }

    log.push_str(&format!("save_file count={}\n", fid_list.len()));
    let save_ret = adapter
        .save_file(&fid_list, &token_list, &dest_fid, &pwd_id, &stoken)
        .await?;
    let task_id_remote = save_ret
        .pointer("/data/task_id")
        .or_else(|| save_ret.get("task_id"))
        .map(|v| v.to_string().trim_matches('"').to_string())
        .filter(|s| !s.is_empty());
    log.push_str(&format!(
        "save_file code/status={:?} task_id={:?}\n",
        save_ret.get("code").or_else(|| save_ret.get("status")),
        task_id_remote
    ));

    if let Some(tid) = task_id_remote {
        if !tid.is_empty() && !tid.starts_with("quark_sync_") {
            log.push_str("query_task waiting...\n");
            let q = adapter.query_task(&tid).await?;
            log.push_str(&format!(
                "query_task data.status={:?}\n",
                q.pointer("/data/status")
            ));
        }
    } else {
        let msg = save_ret
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("save_file failed");
        // quark may return status 200 without nested code
        let ok_save = matches!(
            save_ret.get("status").and_then(|v| v.as_i64()),
            Some(200)
        ) || save_ret.get("code").and_then(|v| v.as_i64()) == Some(0);
        if !ok_save {
            return finish(state, exec_id, "failed", msg, &mut log).await;
        }
    }

    log.push_str("note: TMDB rename / episode filter still migrating\n");
    log.push_str(&format!("[{}] done\n", Utc::now().to_rfc3339()));
    finish(state, exec_id, "success", "transferred", &mut log).await
}

fn extract_stoken(resp: &Value) -> Option<String> {
    if let Some(s) = resp.pointer("/data/stoken").and_then(|v| v.as_str()) {
        if !s.is_empty() {
            return Some(s.to_string());
        }
    }
    // object stoken serialized
    if let Some(obj) = resp.pointer("/data/stoken") {
        if obj.is_object() {
            return Some(obj.to_string());
        }
        let s = obj.to_string().trim_matches('"').to_string();
        if !s.is_empty() && s != "null" {
            return Some(s);
        }
    }
    None
}

fn value_as_fid(v: &Value) -> String {
    match v {
        Value::String(s) => {
            if s.is_empty() {
                "0".into()
            } else {
                s.clone()
            }
        }
        Value::Number(n) => n.to_string(),
        _ => "0".into(),
    }
}

async fn finish(
    state: &AppState,
    exec_id: i64,
    status: &str,
    message: &str,
    log: &mut String,
) -> anyhow::Result<(i64, String)> {
    if status != "success" {
        log.push_str(&format!("{status}: {message}\n"));
    }
    sqlx::query(
        r#"
        UPDATE task_executions
        SET status=?, stage='done', message=?, run_log=?, finished_at=?
        WHERE id=?
        "#,
    )
    .bind(status)
    .bind(message)
    .bind(&*log)
    .bind(Utc::now().to_rfc3339())
    .bind(exec_id)
    .execute(&state.db)
    .await?;
    Ok((exec_id, log.clone()))
}
