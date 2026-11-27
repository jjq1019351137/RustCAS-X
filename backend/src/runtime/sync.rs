use crate::runtime::deferred_delete;
use crate::services::openlist::{self, join_remote, OpenListClient};
use crate::state::AppState;
use chrono::Utc;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone)]
struct FileEvent {
    path: String,
    action: String,
    status: String,
    size: Option<i64>,
    message: Option<String>,
}

async fn flush_file_events(state: &AppState, exec_id: i64, events: &[FileEvent]) {
    let now = Utc::now().to_rfc3339();
    for ev in events.iter().take(2000) {
        let _ = sqlx::query(
            r#"
            INSERT INTO sync_execution_files
                (sync_execution_id, path, action, status, size, message, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(exec_id)
        .bind(&ev.path)
        .bind(&ev.action)
        .bind(&ev.status)
        .bind(ev.size)
        .bind(&ev.message)
        .bind(&now)
        .bind(&now)
        .execute(&state.db)
        .await;
    }
}

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
    let mut file_events: Vec<FileEvent> = Vec::new();
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

    let overwrite = target
        .get("overwrite")
        .or_else(|| source.get("overwrite"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let result_status;
    let result_msg;
    match (src_type, dst_type) {
        ("local", "local") => {
            let src_rel = source.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let dst_rel = target.get("path").and_then(|v| v.as_str()).unwrap_or("");
            match sync_local_to_local(state, src_rel, dst_rel, overwrite, &mut log, &mut file_events)
            {
                Ok((copied, skipped, dirs)) => {
                    result_status = "success";
                    result_msg = format!("local→local copied={copied} skipped={skipped} dirs={dirs}");
                    log.push_str(&format!("{result_msg}\n"));
                }
                Err(e) => {
                    result_status = "failed";
                    result_msg = e.to_string();
                    log.push_str(&format!("error: {result_msg}\n"));
                }
            }
        }
        ("local", "openlist") => {
            let src_rel = source.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let dst_rel = target.get("path").and_then(|v| v.as_str()).unwrap_or("/");
            match sync_local_to_openlist(
                state,
                src_rel,
                dst_rel,
                overwrite,
                &mut log,
                &mut file_events,
            )
            .await
            {
                Ok((copied, skipped)) => {
                    result_status = "success";
                    result_msg = format!("local→openlist uploaded={copied} skipped={skipped}");
                    log.push_str(&format!("{result_msg}\n"));
                }
                Err(e) => {
                    result_status = "failed";
                    result_msg = e.to_string();
                    log.push_str(&format!("error: {result_msg}\n"));
                }
            }
        }
        ("openlist", "local") => {
            let src_rel = source.get("path").and_then(|v| v.as_str()).unwrap_or("/");
            let dst_rel = target.get("path").and_then(|v| v.as_str()).unwrap_or("");
            match sync_openlist_to_local(
                state,
                src_rel,
                dst_rel,
                overwrite,
                &mut log,
                &mut file_events,
            )
            .await
            {
                Ok((copied, skipped)) => {
                    result_status = "success";
                    result_msg = format!("openlist→local downloaded={copied} skipped={skipped}");
                    log.push_str(&format!("{result_msg}\n"));
                }
                Err(e) => {
                    result_status = "failed";
                    result_msg = e.to_string();
                    log.push_str(&format!("error: {result_msg}\n"));
                }
            }
        }
        ("openlist", "openlist") => {
            let src_rel = source.get("path").and_then(|v| v.as_str()).unwrap_or("/");
            let dst_rel = target.get("path").and_then(|v| v.as_str()).unwrap_or("/");
            match sync_openlist_to_openlist(
                state,
                src_rel,
                dst_rel,
                overwrite,
                &mut log,
                &mut file_events,
            )
            .await
            {
                Ok((copied, skipped)) => {
                    result_status = "success";
                    result_msg = format!("openlist→openlist copied={copied} skipped={skipped}");
                    log.push_str(&format!("{result_msg}\n"));
                }
                Err(e) => {
                    result_status = "failed";
                    result_msg = e.to_string();
                    log.push_str(&format!("error: {result_msg}\n"));
                }
            }
        }
        _ => {
            result_status = "success";
            result_msg = format!("{src_type}→{dst_type} not supported yet");
            log.push_str(&format!("{result_msg}\n"));
            if dst_type == "local" {
                if let Some(base) = target.get("path").and_then(|v| v.as_str()) {
                    let id =
                        deferred_delete::enqueue(state, Some(&uid), base, ".rustcas-x-keep", 0)
                            .await?;
                    log.push_str(&format!(
                        "enqueued deferred noop marker id={id} (ignored if missing)\n"
                    ));
                }
            }
        }
    }

    log.push_str(&format!("[{}] done\n", Utc::now().to_rfc3339()));
    flush_file_events(state, exec_id, &file_events).await;

    sqlx::query(
        r#"
        UPDATE sync_executions
        SET status=?, message=?, run_log=?, finished_at=?
        WHERE id=?
        "#,
    )
    .bind(result_status)
    .bind(&result_msg)
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

    if result_status == "success" || result_status == "failed" {
        crate::services::notify::send_runtime(
            state,
            &format!("同步任务{result_status}"),
            &format!("任务: {name}\n执行ID: {exec_id}\n结果: {result_msg}"),
        )
        .await;
    }

    Ok((exec_id, log))
}

fn sync_root(state: &AppState) -> PathBuf {
    state.settings.data_dir.join("sync")
}

fn resolve_under(root: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    let rel = rel.trim().trim_start_matches(['/', '\\']);
    let full = root.join(rel).canonicalize().unwrap_or_else(|_| root.join(rel));
    let base = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if !full.starts_with(&base) && full != base {
        // allow not-yet-existing leaf under base without canonicalize
        let candidate = root.join(rel);
        let cleaned = normalize_components(&candidate);
        let base_norm = normalize_components(root);
        if !cleaned.starts_with(&base_norm) {
            anyhow::bail!("local path forbidden: {rel}");
        }
        return Ok(cleaned);
    }
    Ok(full)
}

fn normalize_components(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn sync_local_to_local(
    state: &AppState,
    src_rel: &str,
    dst_rel: &str,
    overwrite: bool,
    log: &mut String,
    events: &mut Vec<FileEvent>,
) -> anyhow::Result<(usize, usize, usize)> {
    let root = sync_root(state);
    fs::create_dir_all(&root)?;
    let src = resolve_under(&root, src_rel)?;
    let dst = resolve_under(&root, dst_rel)?;
    log.push_str(&format!(
        "local root={} src={} dst={} overwrite={overwrite}\n",
        root.display(),
        src.display(),
        dst.display()
    ));

    if !src.exists() {
        anyhow::bail!("source path not found: {}", src.display());
    }
    if src.is_file() {
        fs::create_dir_all(dst.parent().unwrap_or(Path::new(".")))?;
        let size = fs::metadata(&src).ok().map(|m| m.len() as i64);
        let outcome = copy_one_file(&src, &dst, overwrite)?;
        let (c, s) = match outcome {
            CopyOutcome::Copied => {
                events.push(FileEvent {
                    path: dst.display().to_string(),
                    action: "copy".into(),
                    status: "ok".into(),
                    size,
                    message: None,
                });
                (1, 0)
            }
            CopyOutcome::Skipped => {
                events.push(FileEvent {
                    path: dst.display().to_string(),
                    action: "copy".into(),
                    status: "skipped".into(),
                    size,
                    message: None,
                });
                (0, 1)
            }
        };
        return Ok((c, s, 0));
    }
    if !src.is_dir() {
        anyhow::bail!("source is neither file nor directory");
    }

    fs::create_dir_all(&dst)?;
    let mut copied = 0usize;
    let mut skipped = 0usize;
    let mut dirs = 0usize;

    walk_copy(
        &src,
        &dst,
        overwrite,
        &mut copied,
        &mut skipped,
        &mut dirs,
        log,
        events,
    )?;
    Ok((copied, skipped, dirs))
}

enum CopyOutcome {
    Copied,
    Skipped,
}

fn walk_copy(
    src_dir: &Path,
    dst_dir: &Path,
    overwrite: bool,
    copied: &mut usize,
    skipped: &mut usize,
    dirs: &mut usize,
    log: &mut String,
    events: &mut Vec<FileEvent>,
) -> anyhow::Result<()> {
    for entry in fs::read_dir(src_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        let sp = entry.path();
        let dp = dst_dir.join(&name);
        let ft = entry.file_type()?;
        if ft.is_dir() {
            fs::create_dir_all(&dp)?;
            *dirs += 1;
            walk_copy(&sp, &dp, overwrite, copied, skipped, dirs, log, events)?;
        } else if ft.is_file() {
            let size = fs::metadata(&sp).ok().map(|m| m.len() as i64);
            match copy_one_file(&sp, &dp, overwrite)? {
                CopyOutcome::Copied => {
                    *copied += 1;
                    log.push_str(&format!("copied {}\n", name_str));
                    events.push(FileEvent {
                        path: dp.display().to_string(),
                        action: "copy".into(),
                        status: "ok".into(),
                        size,
                        message: None,
                    });
                }
                CopyOutcome::Skipped => {
                    *skipped += 1;
                    events.push(FileEvent {
                        path: dp.display().to_string(),
                        action: "copy".into(),
                        status: "skipped".into(),
                        size,
                        message: None,
                    });
                }
            }
        }
    }
    Ok(())
}

fn copy_one_file(src: &Path, dst: &Path, overwrite: bool) -> anyhow::Result<CopyOutcome> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    if dst.exists() && !overwrite {
        // skip if same size
        let ss = fs::metadata(src)?.len();
        let ds = fs::metadata(dst)?.len();
        if ss == ds {
            return Ok(CopyOutcome::Skipped);
        }
        if !overwrite {
            return Ok(CopyOutcome::Skipped);
        }
    }
    if dst.exists() && overwrite {
        let tmp = dst.with_file_name(format!(
            ".{}.sync_tmp_{}",
            dst.file_name().and_then(|s| s.to_str()).unwrap_or("file"),
            Utc::now().timestamp_millis()
        ));
        fs::copy(src, &tmp)?;
        let _ = fs::remove_file(dst);
        fs::rename(&tmp, dst)?;
    } else {
        fs::copy(src, dst)?;
    }
    Ok(CopyOutcome::Copied)
}

async fn openlist_client(state: &AppState) -> anyhow::Result<OpenListClient> {
    let cfg = openlist::load_config(state).await?;
    OpenListClient::new(&cfg)
}

fn collect_local_files(base: &Path) -> anyhow::Result<Vec<(String, u64, PathBuf)>> {
    let mut out = Vec::new();
    if !base.exists() {
        return Ok(out);
    }
    if base.is_file() {
        let size = fs::metadata(base)?.len();
        let name = base
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file")
            .to_string();
        out.push((name, size, base.to_path_buf()));
        return Ok(out);
    }
    fn walk(cur: &Path, root: &Path, out: &mut Vec<(String, u64, PathBuf)>) -> anyhow::Result<()> {
        for entry in fs::read_dir(cur)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') {
                continue;
            }
            let p = entry.path();
            if p.is_dir() {
                walk(&p, root, out)?;
            } else if p.is_file() {
                let rel = p
                    .strip_prefix(root)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .replace('\\', "/");
                let size = fs::metadata(&p)?.len();
                out.push((rel, size, p));
            }
        }
        Ok(())
    }
    walk(base, base, &mut out)?;
    Ok(out)
}

async fn sync_local_to_openlist(
    state: &AppState,
    src_rel: &str,
    dst_rel: &str,
    overwrite: bool,
    log: &mut String,
    events: &mut Vec<FileEvent>,
) -> anyhow::Result<(usize, usize)> {
    let client = openlist_client(state).await?;
    let root = sync_root(state);
    fs::create_dir_all(&root)?;
    let src = resolve_under(&root, src_rel)?;
    log.push_str(&format!(
        "local→openlist src={} dst={}\n",
        src.display(),
        dst_rel
    ));
    if !src.exists() {
        anyhow::bail!("source path not found: {}", src.display());
    }
    client.ensure_dir(dst_rel).await?;
    let files = collect_local_files(&src)?;
    let remote_files = client.walk_files(dst_rel).await.unwrap_or_default();
    let remote_map: std::collections::HashMap<String, u64> = remote_files.into_iter().collect();

    let mut copied = 0usize;
    let mut skipped = 0usize;
    for (rel, size, local) in files {
        let remote = join_remote(dst_rel, &rel);
        if let Some(rs) = remote_map.get(&rel) {
            if (*rs == size || !overwrite) && !overwrite {
                skipped += 1;
                events.push(FileEvent {
                    path: remote.clone(),
                    action: "upload".into(),
                    status: "skipped".into(),
                    size: Some(size as i64),
                    message: None,
                });
                continue;
            }
        }
        if let Some(parent) = Path::new(&remote).parent() {
            let parent_s = parent.to_string_lossy().replace('\\', "/");
            if parent_s != "/" && !parent_s.is_empty() {
                let _ = client.ensure_dir(&parent_s).await;
            }
        }
        client.put_file(&remote, &local).await?;
        copied += 1;
        log.push_str(&format!("uploaded {rel}\n"));
        events.push(FileEvent {
            path: remote,
            action: "upload".into(),
            status: "ok".into(),
            size: Some(size as i64),
            message: None,
        });
    }
    Ok((copied, skipped))
}

async fn sync_openlist_to_local(
    state: &AppState,
    src_rel: &str,
    dst_rel: &str,
    overwrite: bool,
    log: &mut String,
    events: &mut Vec<FileEvent>,
) -> anyhow::Result<(usize, usize)> {
    let client = openlist_client(state).await?;
    let root = sync_root(state);
    fs::create_dir_all(&root)?;
    let dst = resolve_under(&root, dst_rel)?;
    fs::create_dir_all(&dst)?;
    log.push_str(&format!(
        "openlist→local src={src_rel} dst={}\n",
        dst.display()
    ));
    let files = client.walk_files(src_rel).await?;
    let mut copied = 0usize;
    let mut skipped = 0usize;
    for (rel, size) in files {
        let local = {
            let mut p = dst.to_path_buf();
            for part in rel.split('/') {
                if !part.is_empty() {
                    p.push(part);
                }
            }
            p
        };
        if local.exists() && !overwrite {
            let ls = fs::metadata(&local)?.len();
            skipped += 1;
            events.push(FileEvent {
                path: local.display().to_string(),
                action: "download".into(),
                status: "skipped".into(),
                size: Some(if ls == size { size as i64 } else { size as i64 }),
                message: None,
            });
            continue;
        }
        let remote = join_remote(src_rel, &rel);
        client.download_to_file(&remote, &local).await?;
        copied += 1;
        log.push_str(&format!("downloaded {rel}\n"));
        events.push(FileEvent {
            path: local.display().to_string(),
            action: "download".into(),
            status: "ok".into(),
            size: Some(size as i64),
            message: None,
        });
    }
    Ok((copied, skipped))
}

async fn sync_openlist_to_openlist(
    state: &AppState,
    src_rel: &str,
    dst_rel: &str,
    overwrite: bool,
    log: &mut String,
    events: &mut Vec<FileEvent>,
) -> anyhow::Result<(usize, usize)> {
    let client = openlist_client(state).await?;
    log.push_str(&format!("openlist→openlist src={src_rel} dst={dst_rel}\n"));
    client.ensure_dir(dst_rel).await?;
    let files = client.walk_files(src_rel).await?;
    let total = files.len();
    let remote_dst = client.walk_files(dst_rel).await.unwrap_or_default();
    let dst_map: std::collections::HashMap<String, u64> = remote_dst.into_iter().collect();

    let mut by_dir: std::collections::HashMap<String, Vec<(String, String)>> =
        std::collections::HashMap::new();
    let mut skipped = 0usize;
    for (rel, size) in files {
        let remote = join_remote(dst_rel, &rel);
        if dst_map.contains_key(&rel) && !overwrite {
            skipped += 1;
            events.push(FileEvent {
                path: remote,
                action: "copy".into(),
                status: "skipped".into(),
                size: Some(size as i64),
                message: None,
            });
            continue;
        }
        if let Some(rs) = dst_map.get(&rel) {
            if *rs == size && !overwrite {
                skipped += 1;
                events.push(FileEvent {
                    path: remote,
                    action: "copy".into(),
                    status: "skipped".into(),
                    size: Some(size as i64),
                    message: None,
                });
                continue;
            }
        }
        let name = Path::new(&rel)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&rel)
            .to_string();
        let parent = Path::new(&rel)
            .parent()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .filter(|p| !p.is_empty())
            .unwrap_or_default();
        by_dir.entry(parent).or_default().push((rel, name));
    }

    let mut copied = 0usize;
    for (parent, items) in &by_dir {
        let src_dir = if parent.is_empty() {
            openlist::normalize_abs(src_rel)
        } else {
            join_remote(src_rel, parent)
        };
        let dst_dir = if parent.is_empty() {
            openlist::normalize_abs(dst_rel)
        } else {
            join_remote(dst_rel, parent)
        };
        client.ensure_dir(&dst_dir).await?;
        let names: Vec<String> = items.iter().map(|(_, n)| n.clone()).collect();
        match client.fs_copy(&src_dir, &dst_dir, &names, overwrite).await {
            Ok(_) => {
                copied += names.len();
                for (rel, _) in items {
                    log.push_str(&format!("copied {rel}\n"));
                    events.push(FileEvent {
                        path: join_remote(dst_rel, rel),
                        action: "copy".into(),
                        status: "ok".into(),
                        size: None,
                        message: None,
                    });
                }
            }
            Err(e) => {
                log.push_str(&format!("fs_copy fail {src_dir} -> {dst_dir}: {e}\n"));
                for (rel, _) in items {
                    events.push(FileEvent {
                        path: join_remote(dst_rel, rel),
                        action: "copy".into(),
                        status: "failed".into(),
                        size: None,
                        message: Some(e.to_string()),
                    });
                }
            }
        }
    }
    Ok((copied, total.saturating_sub(copied).max(skipped)))
}
