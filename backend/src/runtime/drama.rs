use crate::adapters::create_adapter;
use crate::adapters::traits::CloudDriveAdapter;
use crate::services::episode::{
    format_episode_anchor, is_before_start_episode, name_exists_in_dest, parse_episode_anchor,
    resolve_start_episode_key,
};
use crate::services::magic_rename::{passes_magic_keyword_filter, MagicRename};
use crate::services::rename::{matches_pattern, rename_preview_with_taskname};
use crate::services::tmdb::{self, TmdbClient};
use crate::settings_store::get_or_default;
use crate::state::AppState;
use chrono::Utc;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;

/// Drama run: init → extract → stoken → detail → filter → mkdir → save → query_task.
pub async fn run_once(state: &AppState, task_id: i64) -> anyhow::Result<(i64, String)> {
    let row = sqlx::query_as::<
        _,
        (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<i64>,
            Option<String>,
            String,
            Option<String>,
            Option<String>,
            i64,
        ),
    >(
        r#"
        SELECT taskname, shareurl, savepath, account_name, tmdb_media_type,
               pattern, "replace", startfid, tmdb_id, ignore_extension,
               update_subdir, extra_json, enddate, shareurl_ban, enabled
        FROM tasks WHERE id = ?
        "#,
    )
    .bind(task_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| anyhow::anyhow!("task not found"))?;

    let (
        taskname,
        shareurl,
        savepath,
        account_name,
        media_type,
        pattern,
        replace,
        startfid,
        tmdb_id,
        ignore_extension,
        update_subdir,
        extra_json,
        enddate,
        shareurl_ban,
        enabled,
    ) = row;
    let pattern = pattern.unwrap_or_default();
    let replace = replace.unwrap_or_default();
    let startfid = startfid.unwrap_or_default();
    let update_subdir = update_subdir.unwrap_or_default();
    let ignore_ext = ignore_extension.unwrap_or(0) != 0;
    let extra: Value = serde_json::from_str(&extra_json).unwrap_or(json!({}));
    let resave_mode = extra
        .get("update_subdir_resave_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("none")
        .to_string();

    #[derive(Deserialize, serde::Serialize, Default)]
    struct SaveRuleConfig {
        #[serde(default)]
        enable_skip_transferred_history: bool,
    }
    let save_rules: SaveRuleConfig = get_or_default(&state.db, "save_rules")
        .await
        .unwrap_or_default();
    let skip_history = save_rules.enable_skip_transferred_history;
    let history_raw: String = sqlx::query_scalar(
        "SELECT COALESCE(transferred_history_json, '[]') FROM tasks WHERE id = ?",
    )
    .bind(task_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .unwrap_or_else(|| "[]".into());
    let history_set = build_history_name_set(&history_raw);

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

    if enabled == 0 {
        return finish(state, exec_id, "skipped", "task disabled", &mut log).await;
    }
    if extra.get("ended").and_then(|v| v.as_bool()).unwrap_or(false) {
        return finish(state, exec_id, "skipped", "task marked ended", &mut log).await;
    }
    if let Some(ban) = shareurl_ban.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        log.push_str(&format!("shareurl_ban={ban}\n"));
        return finish(state, exec_id, "skipped", "shareurl banned", &mut log).await;
    }
    if let Some(ed) = enddate.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if let Ok(end_d) = chrono::NaiveDate::parse_from_str(&ed[..ed.len().min(10)], "%Y-%m-%d") {
            let today = chrono::Local::now().date_naive();
            if today > end_d {
                return finish(
                    state,
                    exec_id,
                    "skipped",
                    &format!("past enddate {ed}"),
                    &mut log,
                )
                .await;
            }
        }
    }

    let tmdb_cfg = tmdb::load_config(state).await.unwrap_or_default();
    let mut series_title: Option<String> = None;
    if tmdb_cfg.configured() {
        if let Some(id) = tmdb_id.filter(|v| *v > 0) {
            let client = TmdbClient::new(&tmdb_cfg);
            let mt = media_type.as_deref().unwrap_or("tv").to_lowercase();
            let details = if mt == "movie" {
                client.get_movie_details(id).await.ok().flatten()
            } else {
                client.get_tv_details(id).await.ok().flatten()
            };
            if let Some(d) = details.as_ref() {
                series_title = tmdb::media_title(d);
                let weekdays = tmdb::infer_weekdays_from_details(d);
                // TMDB weekdays are Mon=0..Sun=6; store as ISO Mon=1..Sun=7 for would_run_today
                let iso_days: Vec<i64> = weekdays
                    .iter()
                    .map(|d| if *d == 6 { 7 } else { (*d + 1) as i64 })
                    .collect();
                log.push_str(&format!(
                    "tmdb id={id} title={:?} year={:?} weekdays_iso={:?}\n",
                    series_title,
                    tmdb::media_year(d),
                    iso_days
                ));
                if !iso_days.is_empty() {
                    let _ = persist_tmdb_weekdays(state, task_id, &iso_days).await;
                }
            } else {
                log.push_str(&format!("tmdb fetch miss id={id}\n"));
            }
        }
    } else {
        log.push_str("tmdb: api_key not configured\n");
    }

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

    // Dest file names / fids for skip-existing or replace-delete
    let dest_list = adapter.ls_dir(&dest_fid, 500).await.unwrap_or(json!({}));
    let dest_items: Vec<Value> = dest_list
        .pointer("/data/list")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut dest_names: Vec<String> = Vec::new();
    let mut dest_name_to_fid: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for it in &dest_items {
        let is_dir = it
            .get("dir")
            .or_else(|| it.get("is_dir"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if is_dir {
            continue;
        }
        if let (Some(fid), Some(name)) = (item_fid(it), item_name(it)) {
            dest_names.push(name.clone());
            dest_name_to_fid.insert(name, fid);
        }
    }
    log.push_str(&format!("dest existing files={}\n", dest_names.len()));

    let share_fid_names: Vec<(String, String)> = items
        .iter()
        .filter_map(|it| {
            let fid = item_fid(it)?;
            let name = item_name(it)?;
            Some((fid, name))
        })
        .collect();

    let start_key = resolve_start_episode_key(&startfid, &share_fid_names);
    if !startfid.trim().is_empty() && start_key.is_none() {
        log.push_str(&format!("提示: 起始锚点无法解析为集数，已忽略 start={startfid}\n"));
    } else if let Some(sk) = start_key {
        log.push_str(&format!(
            "起始集数过滤: >= {} (原始={startfid})\n",
            format_episode_anchor(sk.0, sk.1)
        ));
    }

    let mut mr = MagicRename::new();
    mr.set_taskname(&taskname);
    let magic_key = pattern.clone();
    let (pattern, replace) = mr.magic_regex_conv(&pattern, &replace);
    if !pattern.trim().is_empty() {
        log.push_str(&format!(
            "pattern(after magic) key={magic_key} len={} replace_len={}\n",
            pattern.len(),
            replace.len()
        ));
    }

    let tv_tpl = tmdb_cfg.guessit_tmdb_tv_rename_template.clone();
    let disable_fb = tmdb_cfg.disable_guessit_tmdb_fallback_rename;
    // Quark: replace same-name dest before save; other drives keep skip
    let replace_existing = drive_type == "quark";

    let mut fid_list = Vec::new();
    let mut token_list = Vec::new();
    let mut rename_plan: Vec<(String, String)> = Vec::new(); // origin -> target (diff only)
    let mut saved_pairs: Vec<(String, String)> = Vec::new(); // all saved origin/target
    let mut pending_delete: Vec<String> = Vec::new();
    let mut skipped_pattern = 0usize;
    let mut skipped_start = 0usize;
    let mut skipped_exist = 0usize;

    for item in &items {
        let is_dir = item
            .get("dir")
            .or_else(|| item.get("is_dir"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if is_dir {
            continue;
        }
        let Some(fid) = item_fid(item) else { continue };
        let Some(name) = item_name(item) else { continue };

        if !passes_magic_keyword_filter(&magic_key, &name) {
            skipped_pattern += 1;
            continue;
        }
        if !matches_pattern(&name, &pattern) {
            skipped_pattern += 1;
            continue;
        }

        let renamed = rename_preview_with_taskname(
            &name,
            &pattern,
            &replace,
            &taskname,
            series_title.as_deref(),
            Some(tv_tpl.as_str()),
            disable_fb,
        );

        if start_key.is_some() {
            let file_key = parse_episode_anchor(&renamed).or_else(|| parse_episode_anchor(&name));
            if is_before_start_episode(file_key, start_key) {
                skipped_start += 1;
                continue;
            }
        }

        let exists = name_exists_in_dest(&renamed, &dest_names, ignore_ext)
            || name_exists_in_dest(&name, &dest_names, ignore_ext);
        if exists {
            if replace_existing {
                // Queue dest fids that collide with origin/target name for delete
                for (dn, dfid) in &dest_name_to_fid {
                    let hit = if ignore_ext {
                        let a = dn.rsplit_once('.').map(|(s, _)| s).unwrap_or(dn);
                        let b = renamed.rsplit_once('.').map(|(s, _)| s).unwrap_or(&renamed);
                        let c = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(&name);
                        a.eq_ignore_ascii_case(b) || a.eq_ignore_ascii_case(c)
                    } else {
                        dn == &renamed || dn == &name
                    };
                    if hit {
                        pending_delete.push(dfid.clone());
                    }
                }
                log.push_str(&format!("将覆盖已存在: {name} -> {renamed}\n"));
            } else {
                skipped_exist += 1;
                log.push_str(&format!("跳过已存在: {name} -> {renamed}\n"));
                continue;
            }
        }

        if skip_history && is_history_hit(&history_set, &name, &renamed) {
            skipped_exist += 1;
            log.push_str(&format!("跳过历史已转存: {name} -> {renamed}\n"));
            continue;
        }

        let token = item
            .get("share_fid_token")
            .or_else(|| item.get("shareFidToken"))
            .map(|v| v.as_str().unwrap_or("").to_string())
            .unwrap_or_default();
        let token = if token.is_empty() && drive_type == "cloud189" {
            json!({ "pid": "0", "dir": 0, "name": name }).to_string()
        } else {
            token
        };

        log.push_str(&format!("save candidate: {name} -> {renamed} ({fid})\n"));
        fid_list.push(fid);
        token_list.push(token);
        saved_pairs.push((name.clone(), renamed.clone()));
        if renamed != name {
            rename_plan.push((name, renamed));
        }
    }

    log.push_str(&format!(
        "filter: keep={} skip_pattern={skipped_pattern} skip_start={skipped_start} skip_exist={skipped_exist}\n",
        fid_list.len()
    ));

    if fid_list.is_empty() {
        return finish(state, exec_id, "skipped", "no new files to transfer", &mut log).await;
    }

    if !pending_delete.is_empty() {
        pending_delete.sort();
        pending_delete.dedup();
        log.push_str(&format!("delete conflicting dest count={}\n", pending_delete.len()));
        match adapter.delete(&pending_delete).await {
            Ok(ret) => log.push_str(&format!(
                "delete code/status={:?}\n",
                ret.get("code").or_else(|| ret.get("status"))
            )),
            Err(e) => log.push_str(&format!("delete err: {e}\n")),
        }
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
        let ok_save = matches!(
            save_ret.get("status").and_then(|v| v.as_i64()),
            Some(200)
        ) || save_ret.get("code").and_then(|v| v.as_i64()) == Some(0);
        if !ok_save {
            return finish(state, exec_id, "failed", msg, &mut log).await;
        }
    }

    if !rename_plan.is_empty() {
        // Brief settle so listing sees newly saved files
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let after = adapter.ls_dir(&dest_fid, 500).await.unwrap_or(json!({}));
        let dest_items = after
            .pointer("/data/list")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut by_name: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for it in &dest_items {
            if let (Some(fid), Some(name)) = (item_fid(it), item_name(it)) {
                by_name.insert(name, fid);
            }
        }
        let mut renamed_ok = 0usize;
        for (origin, target) in &rename_plan {
            let Some(fid) = by_name.get(origin) else {
                log.push_str(&format!("rename skip (fid not found): {origin} → {target}\n"));
                continue;
            };
            match adapter.rename(fid, target).await {
                Ok(ret) => {
                    let ok = ret.get("code").and_then(|v| v.as_i64()) == Some(0)
                        || ret.get("status").and_then(|v| v.as_i64()) == Some(200);
                    if ok {
                        renamed_ok += 1;
                        log.push_str(&format!("renamed {origin} → {target}\n"));
                    } else {
                        log.push_str(&format!(
                            "rename fail {origin} → {target}: {:?}\n",
                            ret.get("message")
                        ));
                    }
                }
                Err(e) => log.push_str(&format!("rename err {origin} → {target}: {e}\n")),
            }
        }
        log.push_str(&format!(
            "rename done ok={renamed_ok}/{}\n",
            rename_plan.len()
        ));
    }

    if !saved_pairs.is_empty() {
        let mut entries: Vec<Value> =
            serde_json::from_str(&history_raw).unwrap_or_else(|_| vec![]);
        for (origin, target) in &saved_pairs {
            entries.push(json!({
                "origin": origin,
                "target": target,
                "at": Utc::now().to_rfc3339()
            }));
        }
        if entries.len() > 500 {
            entries = entries.split_off(entries.len() - 500);
        }
        let _ = sqlx::query(
            "UPDATE tasks SET transferred_history_json = ?, updated_at = ? WHERE id = ?",
        )
        .bind(Value::Array(entries).to_string())
        .bind(Utc::now().to_rfc3339())
        .bind(task_id)
        .execute(&state.db)
        .await;
    }

    // Subdirectory sync (update_subdir)
    if !update_subdir.trim().is_empty() {
        let Ok(subdir_re) = Regex::new(&update_subdir) else {
            log.push_str(&format!("update_subdir regex invalid: {update_subdir}\n"));
            log.push_str(&format!("[{}] done\n", Utc::now().to_rfc3339()));
            return finish(state, exec_id, "success", "transferred", &mut log).await;
        };
        let file_pat = if pattern.trim().is_empty() {
            None
        } else {
            Regex::new(&pattern).ok()
        };
        log.push_str(&format!(
            "subdir sync update_subdir={update_subdir} resave_mode={resave_mode}\n"
        ));
        // Refresh dest dir map
        let dest_list2 = adapter.ls_dir(&dest_fid, 500).await.unwrap_or(json!({}));
        let mut dest_dir_map: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for it in dest_list2
            .pointer("/data/list")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
        {
            let is_dir = it
                .get("dir")
                .or_else(|| it.get("is_dir"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !is_dir {
                continue;
            }
            if let (Some(fid), Some(name)) = (item_fid(it), item_name(it)) {
                dest_dir_map.insert(name, fid);
            }
        }
        let mut sub_saved = 0usize;
        for raw in &items {
            let is_dir = raw
                .get("dir")
                .or_else(|| raw.get("is_dir"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !is_dir {
                continue;
            }
            let Some(name) = item_name(raw) else { continue };
            let Some(sfid) = item_fid(raw) else { continue };
            if !subdir_re.is_match(&name) {
                continue;
            }
            if resave_mode == "delete_then_resave" {
                if let Some(old) = dest_dir_map.get(&name) {
                    let _ = adapter.delete(&[old.clone()]).await;
                    log.push_str(&format!("deleted dest subdir for resave: {name}\n"));
                }
            }
            let child_path = join_path(&dest_path, &name);
            let child_dest = ensure_dir_fid(adapter.as_ref(), &child_path, &mut log).await;
            let Some(child_dest) = child_dest else {
                log.push_str(&format!("subdir mkdir failed: {child_path}\n"));
                continue;
            };
            match sync_share_dir(
                adapter.as_ref(),
                &pwd_id,
                &stoken,
                &sfid,
                &child_dest,
                &child_path,
                file_pat.as_ref(),
                Some(&subdir_re),
                0,
                &mut log,
            )
            .await
            {
                Ok(n) => {
                    sub_saved += n;
                    log.push_str(&format!("subdir synced {name} files={n}\n"));
                }
                Err(e) => log.push_str(&format!("subdir sync err {name}: {e}\n")),
            }
        }
        log.push_str(&format!("subdir sync total_files={sub_saved}\n"));
    }

    log.push_str(&format!("[{}] done\n", Utc::now().to_rfc3339()));
    finish(state, exec_id, "success", "transferred", &mut log).await
}

fn join_path(base: &str, name: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.is_empty() || base == "/" {
        format!("/{name}")
    } else {
        format!("{base}/{name}")
    }
}

async fn ensure_dir_fid(
    adapter: &dyn CloudDriveAdapter,
    path: &str,
    log: &mut String,
) -> Option<String> {
    let mkdir_ret = adapter.mkdir(path).await.ok()?;
    let mut fid = mkdir_ret
        .pointer("/data/fid")
        .map(|v| v.to_string().trim_matches('"').to_string())
        .filter(|s| !s.is_empty());
    if fid.is_none() {
        let fids = adapter.get_fids(&[path.to_string()]).await.ok()?;
        fid = fids
            .first()
            .and_then(|v| v.get("fid"))
            .map(|v| v.to_string().trim_matches('"').to_string())
            .filter(|s| !s.is_empty());
    }
    if fid.is_none() {
        log.push_str(&format!("ensure_dir miss path={path} ret={mkdir_ret}\n"));
    }
    fid
}

async fn sync_share_dir(
    adapter: &dyn CloudDriveAdapter,
    pwd_id: &str,
    stoken: &str,
    share_dir_fid: &str,
    dest_dir_fid: &str,
    dest_abs_path: &str,
    file_pattern: Option<&Regex>,
    subdir_pattern: Option<&Regex>,
    depth: usize,
    log: &mut String,
) -> anyhow::Result<usize> {
    if depth > 3 {
        return Ok(0);
    }
    let detail = adapter
        .get_detail(pwd_id, stoken, share_dir_fid)
        .await?;
    let share_items = detail
        .pointer("/data/list")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let dest_list = adapter.ls_dir(dest_dir_fid, 500).await.unwrap_or(json!({}));
    let mut dest_names = Vec::new();
    let mut dest_dir_map = std::collections::HashMap::new();
    for it in dest_list
        .pointer("/data/list")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
    {
        let is_dir = it
            .get("dir")
            .or_else(|| it.get("is_dir"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let Some(name) = item_name(it) else { continue };
        if is_dir {
            if let Some(fid) = item_fid(it) {
                dest_dir_map.insert(name, fid);
            }
        } else {
            dest_names.push(name);
        }
    }

    let mut transferred = 0usize;
    let mut file_fids = Vec::new();
    let mut file_tokens = Vec::new();

    for raw in &share_items {
        let is_dir = raw
            .get("dir")
            .or_else(|| raw.get("is_dir"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let Some(name) = item_name(raw) else { continue };
        let Some(fid) = item_fid(raw) else { continue };
        if is_dir {
            if let Some(re) = subdir_pattern {
                if !re.is_match(&name) {
                    continue;
                }
            }
            let child_dest = if let Some(existing) = dest_dir_map.get(&name) {
                existing.clone()
            } else {
                let child_path = join_path(dest_abs_path, &name);
                match ensure_dir_fid(adapter, &child_path, log).await {
                    Some(f) => f,
                    None => continue,
                }
            };
            let child_path = join_path(dest_abs_path, &name);
            transferred += Box::pin(sync_share_dir(
                adapter,
                pwd_id,
                stoken,
                &fid,
                &child_dest,
                &child_path,
                file_pattern,
                subdir_pattern,
                depth + 1,
                log,
            ))
            .await?;
            continue;
        }
        if let Some(re) = file_pattern {
            if !re.is_match(&name) {
                continue;
            }
        }
        if name_exists_in_dest(&name, &dest_names, false) {
            continue;
        }
        let token = raw
            .get("share_fid_token")
            .or_else(|| raw.get("shareFidToken"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        file_fids.push(fid);
        file_tokens.push(token);
        log.push_str(&format!("subdir save: {dest_abs_path}/{name}\n"));
    }

    if !file_fids.is_empty() {
        let save_ret = adapter
            .save_file(&file_fids, &file_tokens, dest_dir_fid, pwd_id, stoken)
            .await?;
        if let Some(tid) = save_ret
            .pointer("/data/task_id")
            .or_else(|| save_ret.get("task_id"))
            .map(|v| v.to_string().trim_matches('"').to_string())
            .filter(|s| !s.is_empty())
        {
            if !tid.starts_with("quark_sync_") {
                let _ = adapter.query_task(&tid).await;
            }
        }
        transferred += file_fids.len();
    }
    Ok(transferred)
}

fn history_name_key(name: &str) -> String {
    let stem = name
        .rsplit_once('.')
        .filter(|(_, ext)| !ext.is_empty() && ext.len() <= 8 && !ext.contains(['/', '\\']))
        .map(|(s, _)| s)
        .unwrap_or(name);
    stem.trim().to_lowercase()
}

fn build_history_name_set(raw: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    let arr: Vec<Value> = serde_json::from_str(raw).unwrap_or_default();
    for item in arr {
        for key in ["origin", "target", "file_name", "file_name_re"] {
            if let Some(s) = item.get(key).and_then(|v| v.as_str()) {
                let k = history_name_key(s);
                if !k.is_empty() {
                    set.insert(k);
                }
            }
        }
    }
    set
}

fn is_history_hit(set: &HashSet<String>, origin: &str, target: &str) -> bool {
    let o = history_name_key(origin);
    let t = history_name_key(target);
    (!o.is_empty() && set.contains(&o)) || (!t.is_empty() && set.contains(&t))
}

fn item_fid(item: &Value) -> Option<String> {
    item.get("fid")
        .or_else(|| item.get("file_id"))
        .or_else(|| item.get("fileId"))
        .map(|v| v.to_string().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
}

fn item_name(item: &Value) -> Option<String> {
    item.get("file_name")
        .or_else(|| item.get("fileName"))
        .or_else(|| item.get("title"))
        .or_else(|| item.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

fn extract_stoken(resp: &Value) -> Option<String> {
    if let Some(s) = resp.pointer("/data/stoken").and_then(|v| v.as_str()) {
        if !s.is_empty() {
            return Some(s.to_string());
        }
    }
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

async fn persist_tmdb_weekdays(state: &AppState, task_id: i64, days: &[i64]) -> anyhow::Result<()> {
    let raw: Option<String> =
        sqlx::query_scalar("SELECT addition_json FROM tasks WHERE id = ?")
            .bind(task_id)
            .fetch_optional(&state.db)
            .await?;
    let mut addition: Value = raw
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| json!({}));
    if let Some(obj) = addition.as_object_mut() {
        obj.insert("tmdb_episode_weekdays".into(), json!(days));
        obj.insert("tmdb_update_weekdays".into(), json!(days));
    }
    sqlx::query("UPDATE tasks SET addition_json = ?, updated_at = ? WHERE id = ?")
        .bind(addition.to_string())
        .bind(Utc::now().to_rfc3339())
        .bind(task_id)
        .execute(&state.db)
        .await?;
    Ok(())
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

    if status == "success" || status == "failed" {
        let taskname: String = sqlx::query_scalar(
            r#"
            SELECT COALESCE(t.taskname, '')
            FROM task_executions e
            JOIN tasks t ON t.id = e.task_id
            WHERE e.id = ?
            "#,
        )
        .bind(exec_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
        let title = format!("追剧任务{status}");
        let content = format!("任务: {taskname}\n执行ID: {exec_id}\n结果: {message}");
        crate::services::notify::send_runtime(state, &title, &content).await;
    }

    Ok((exec_id, log.clone()))
}
