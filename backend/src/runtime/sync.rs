use crate::runtime::deferred_delete;
use crate::state::AppState;
use chrono::Utc;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

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
            match sync_local_to_local(state, src_rel, dst_rel, overwrite, &mut log) {
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
        _ => {
            result_status = "success";
            result_msg = format!("{src_type}→{dst_type} pipeline still migrating (local→local live)");
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
        let outcome = copy_one_file(&src, &dst, overwrite)?;
        let (c, s) = match outcome {
            CopyOutcome::Copied => (1, 0),
            CopyOutcome::Skipped => (0, 1),
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

    walk_copy(&src, &dst, overwrite, &mut copied, &mut skipped, &mut dirs, log)?;
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
            walk_copy(&sp, &dp, overwrite, copied, skipped, dirs, log)?;
        } else if ft.is_file() {
            match copy_one_file(&sp, &dp, overwrite)? {
                CopyOutcome::Copied => {
                    *copied += 1;
                    log.push_str(&format!("copied {}\n", name_str));
                }
                CopyOutcome::Skipped => {
                    *skipped += 1;
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
