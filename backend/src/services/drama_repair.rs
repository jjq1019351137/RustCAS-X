//! Quark-first banned-share repair: switch to a healthy backup link.
use crate::services::drive_util::detect_drive_type;
use crate::services::share_links::{load_share_links, mark_checked, save_share_links};
use crate::services::share_preview::{preview_share_batch, SharePreviewBatchIn};
use crate::state::AppState;
use chrono::Utc;
use serde_json::{json, Value};

fn rewrite_quark_shareurl(shareurl: &str, fid: Option<&str>) -> String {
    let url = shareurl.trim();
    let f = fid.map(|s| s.trim()).filter(|s| !s.is_empty() && *s != "0");
    let Some(f) = f else {
        return url.split('#').next().unwrap_or(url).trim().to_string();
    };
    if url.contains(f) {
        return url.to_string();
    }
    let base = url.split('#').next().unwrap_or(url).trim();
    format!("{base}#/list/share/{f}")
}

async fn repair_one(
    state: &AppState,
    id: i64,
    taskname: &str,
    active: &str,
    account_name: Option<&str>,
    list_json: &str,
) -> anyhow::Result<Option<Value>> {
    let links = load_share_links(list_json, Some(active));
    let backups: Vec<String> = links
        .iter()
        .filter_map(|l| {
            let url = l.get("url").and_then(|v| v.as_str())?.trim();
            if url.is_empty() || url == active {
                return None;
            }
            if l.get("status").and_then(|v| v.as_str()) == Some("invalid") {
                return None;
            }
            Some(url.to_string())
        })
        .collect();
    if backups.is_empty() {
        return Ok(None);
    }

    let batch = preview_share_batch(
        &state.db,
        SharePreviewBatchIn {
            shareurls: backups,
            account_name: account_name.map(|s| s.to_string()),
        },
    )
    .await?;
    let batch_items = batch
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut links_mut = links;
    let mut best: Option<(String, i64, i64, i64, Option<String>)> = None;
    for it in &batch_items {
        let url = it
            .get("shareurl")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let ok = it.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
        let message = it.get("message").and_then(|v| v.as_str());
        mark_checked(&mut links_mut, &url, ok, message);
        if !ok {
            continue;
        }
        let latest = it.get("latest_video");
        let season = latest
            .and_then(|v| v.get("season"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let episode = latest
            .and_then(|v| v.get("episode"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let size = latest
            .and_then(|v| v.get("size"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let resolved = it
            .get("resolved_pdir_fid")
            .or_else(|| it.get("pdir_fid"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let better = match &best {
            None => true,
            Some((_, s, e, sz, _)) => (season, episode, size) > (*s, *e, *sz),
        };
        if better {
            best = Some((url, season, episode, size, resolved));
        }
    }

    let Some((backup_url, season, episode, size, resolved)) = best else {
        let saved = save_share_links(&links_mut, active);
        sqlx::query("UPDATE tasks SET shareurl_list_json=?, updated_at=? WHERE id=?")
            .bind(&saved)
            .bind(Utc::now().to_rfc3339())
            .bind(id)
            .execute(&state.db)
            .await?;
        return Ok(None);
    };

    let new_shareurl = rewrite_quark_shareurl(&backup_url, resolved.as_deref());
    let saved = save_share_links(&links_mut, &new_shareurl);
    sqlx::query(
        r#"
        UPDATE tasks
        SET shareurl=?, shareurl_ban=NULL, shareurl_list_json=?, updated_at=?
        WHERE id=?
        "#,
    )
    .bind(&new_shareurl)
    .bind(&saved)
    .bind(Utc::now().to_rfc3339())
    .bind(id)
    .execute(&state.db)
    .await?;

    Ok(Some(json!({
        "task_id": id,
        "taskname": taskname,
        "drive_type": "quark",
        "old_shareurl": active,
        "new_shareurl": new_shareurl,
        "season": season,
        "episode": episode,
        "size": size
    })))
}

/// Mark share banned + record invalid + try backup switch for one task.
pub async fn ban_and_try_repair(
    state: &AppState,
    task_id: i64,
    reason: &str,
) -> anyhow::Result<Option<Value>> {
    let row = sqlx::query_as::<
        _,
        (
            String,
            Option<String>,
            Option<String>,
            String,
        ),
    >(
        r#"
        SELECT taskname, shareurl, account_name, COALESCE(shareurl_list_json, '[]')
        FROM tasks WHERE id = ?
        "#,
    )
    .bind(task_id)
    .fetch_optional(&state.db)
    .await?;
    let Some((taskname, shareurl, account_name, list_json)) = row else {
        return Ok(None);
    };
    let active = shareurl.unwrap_or_default();
    if !active.is_empty() {
        let _ = crate::services::share_cache::upsert_invalid(&state.db, &active, Some(reason)).await;
        sqlx::query("UPDATE tasks SET shareurl_ban=?, updated_at=? WHERE id=?")
            .bind(reason)
            .bind(Utc::now().to_rfc3339())
            .bind(task_id)
            .execute(&state.db)
            .await?;
    }
    if detect_drive_type(&active) != Some("quark") {
        return Ok(None);
    }
    repair_one(
        state,
        task_id,
        &taskname,
        &active,
        account_name.as_deref(),
        &list_json,
    )
    .await
}

pub async fn repair_banned_tasks(state: &AppState) -> anyhow::Result<Value> {
    let rows = sqlx::query_as::<
        _,
        (
            i64,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
        ),
    >(
        r#"
        SELECT id, taskname, shareurl, shareurl_ban, account_name,
               COALESCE(shareurl_list_json, '[]')
        FROM tasks
        WHERE task_type = 'drama'
          AND (
            (shareurl_ban IS NOT NULL AND TRIM(shareurl_ban) != '')
            OR enabled = 1
          )
        ORDER BY id DESC
        LIMIT 200
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    let mut checked = 0usize;
    let mut repaired = 0usize;
    let mut items = Vec::new();

    for (id, taskname, shareurl, shareurl_ban, account_name, list_json) in rows {
        let active = shareurl.clone().unwrap_or_default();
        let ban = shareurl_ban
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());
        let links = load_share_links(&list_json, Some(&active));
        let is_quark = detect_drive_type(&active) == Some("quark")
            || links.iter().any(|l| {
                detect_drive_type(l.get("url").and_then(|v| v.as_str()).unwrap_or(""))
                    == Some("quark")
            });
        if !is_quark {
            continue;
        }
        let active_invalid = links.iter().any(|l| {
            l.get("url").and_then(|v| v.as_str()) == Some(active.as_str())
                && l.get("status").and_then(|v| v.as_str()) == Some("invalid")
        });
        if ban.is_none() && !active_invalid {
            continue;
        }
        checked += 1;
        if let Some(item) = repair_one(
            state,
            id,
            &taskname,
            &active,
            account_name.as_deref(),
            &list_json,
        )
        .await?
        {
            repaired += 1;
            items.push(item);
        }
    }

    Ok(json!({
        "checked": checked,
        "repaired": repaired,
        "items": items
    }))
}
