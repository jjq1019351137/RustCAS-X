//! Quark-first share preview / browse for CreateTaskSheet.
use crate::services::drive_util::{
    detect_drive_type, extract_stoken, item_fid, item_fid_token, item_is_dir, item_name, item_size,
    item_updated_at, load_adapter, value_as_fid,
};
use crate::services::episode::{
    format_episode_anchor, is_before_start_episode, name_exists_in_dest, parse_episode_anchor,
    resolve_start_episode_key,
};
use crate::services::magic_rename::passes_magic_keyword_filter;
use crate::services::rename::{
    is_filter_only, is_video_name, matches_pattern, rename_preview_with_taskname,
};
use crate::services::tmdb::{self, TmdbClient};
use crate::state::AppState;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize, Default)]
pub struct SharePreviewIn {
    pub shareurl: String,
    pub account_name: Option<String>,
    pub pdir_fid: Option<String>,
    pub max_items: Option<usize>,
    pub taskname: Option<String>,
    pub pattern: Option<String>,
    #[serde(rename = "replace")]
    pub replace_rule: Option<String>,
    pub sort_index: Option<i64>,
    pub savepath: Option<String>,
    pub ignore_extension: Option<bool>,
    pub update_subdir: Option<String>,
    pub startfid: Option<String>,
    pub tmdb_id: Option<i64>,
    pub tmdb_media_type: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct SharePreviewBatchIn {
    pub shareurls: Vec<String>,
    pub account_name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct DriveBrowseIn {
    pub dir_path: String,
    pub account_name: Option<String>,
    pub shareurl: Option<String>,
    pub max_items: Option<i64>,
}

pub async fn preview_share(state: &AppState, payload: SharePreviewIn) -> anyhow::Result<Value> {
    let url = payload.shareurl.trim().to_string();
    let drive_type =
        detect_drive_type(&url).ok_or_else(|| anyhow::anyhow!("无法识别的网盘分享链接"))?;
    if drive_type != "quark" {
        anyhow::bail!("RustCAS-X 当前仅支持夸克分享预览");
    }
    let (account_name, adapter) =
        load_adapter(&state.db, drive_type, payload.account_name.as_deref()).await?;

    let extracted = adapter.extract_url(&url);
    let pwd_id = extracted
        .pwd_id
        .clone()
        .ok_or_else(|| anyhow::anyhow!("无法解析分享链接"))?;
    let stoken_resp = adapter.get_stoken(&pwd_id, &extracted.passcode).await?;
    let stoken = extract_stoken(&stoken_resp).ok_or_else(|| {
        anyhow::anyhow!(
            "{}",
            stoken_resp
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("获取分享 token 失败")
        )
    })?;

    let mut pdir_fid = payload
        .pdir_fid
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| value_as_fid(&extracted.pdir_fid));
    let detail = adapter.get_detail(&pwd_id, &stoken, &pdir_fid).await?;
    if let Some(resolved) = detail
        .pointer("/data/resolved_pdir_fid")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        pdir_fid = resolved;
    }
    let raw_items = detail
        .pointer("/data/list")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let taskname = payload.taskname.unwrap_or_default();
    let pattern = payload.pattern.unwrap_or_default();
    let replace = payload.replace_rule.unwrap_or_default();
    let update_subdir = payload.update_subdir.unwrap_or_default();
    let startfid = payload.startfid.unwrap_or_default();
    let ignore_ext = payload.ignore_extension.unwrap_or(false);
    let max_items = payload.max_items.unwrap_or(500).clamp(1, 2000);
    let _ = payload.sort_index;

    let mut dir_filename_list: Vec<String> = Vec::new();
    if let Some(savepath) = payload
        .savepath
        .as_ref()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
    {
        let normalized = compress_slashes(&savepath);
        if let Ok(fids) = adapter.get_fids(&[normalized.clone()]).await {
            let dest_fid = fids
                .iter()
                .find(|it| {
                    it.get("file_path")
                        .or_else(|| it.get("path"))
                        .and_then(|v| v.as_str())
                        == Some(normalized.as_str())
                })
                .or_else(|| fids.first())
                .and_then(item_fid);
            if let Some(fid) = dest_fid {
                if let Ok(listing) = adapter.ls_dir(&fid, 2000).await {
                    if let Some(list) = listing.pointer("/data/list").and_then(|v| v.as_array()) {
                        for raw in list {
                            if item_is_dir(raw) {
                                continue;
                            }
                            if let Some(name) = item_name(raw) {
                                dir_filename_list.push(name);
                            }
                        }
                    }
                }
            }
        }
    }

    let tmdb_cfg = tmdb::load_config(state).await.unwrap_or_default();
    let mut series_title: Option<String> = None;
    if tmdb_cfg.configured() {
        if let (Some(id), Some(mt)) = (
            payload.tmdb_id.filter(|x| *x > 0),
            payload
                .tmdb_media_type
                .as_deref()
                .map(|s| s.trim().to_lowercase())
                .filter(|s| s == "tv" || s == "movie"),
        ) {
            let client = TmdbClient::new(&tmdb_cfg);
            let detail = if mt == "movie" {
                client.get_movie_details(id).await
            } else {
                client.get_tv_details(id).await
            };
            if let Ok(Some(d)) = detail {
                series_title = tmdb::media_title(&d);
            }
        }
    }

    let share_fid_names: Vec<(String, String)> = raw_items
        .iter()
        .filter_map(|it| Some((item_fid(it)?, item_name(it)?)))
        .collect();
    let start_key = resolve_start_episode_key(&startfid, &share_fid_names);
    let start_skip_label = start_key
        .map(|(s, e)| format!("起始集数之前(< {})", format_episode_anchor(s, e)))
        .unwrap_or_else(|| "起始集数之前".into());

    let filter_only = is_filter_only(&pattern, &replace);
    let default_all = pattern.trim().is_empty()
        && replace.trim().is_empty()
        && series_title.as_deref().unwrap_or("").is_empty();
    let compiled_subdir = if update_subdir.trim().is_empty() {
        None
    } else {
        regex::Regex::new(&update_subdir).ok()
    };

    let mut preview_list = Vec::new();
    for raw in raw_items.into_iter().take(max_items) {
        let Some(fid) = item_fid(&raw) else {
            continue;
        };
        let Some(file_name) = item_name(&raw) else {
            continue;
        };
        let is_dir = item_is_dir(&raw);
        let mut item = json!({
            "fid": fid,
            "fid_token": item_fid_token(&raw),
            "name": file_name,
            "file_name": file_name,
            "is_dir": is_dir,
            "dir": is_dir,
            "updated_at": item_updated_at(&raw),
            "size": item_size(&raw),
            "include_items": Value::Null,
            "children_count": Value::Null,
            "name_re": Value::Null,
            "file_name_re": Value::Null,
            "file_name_saved": Value::Null,
        });

        if is_dir {
            if let Some(re) = &compiled_subdir {
                if re.is_match(&file_name) {
                    item["name_re"] = json!(file_name);
                    item["file_name_re"] = json!(file_name);
                } else {
                    item["file_name_saved"] = json!("目录未命中 update_subdir");
                }
            } else {
                item["file_name_saved"] = json!("未启用目录转存");
            }
            preview_list.push(item);
            continue;
        }

        if !pattern.trim().is_empty()
            && (!passes_magic_keyword_filter(&pattern, &file_name)
                || !matches_pattern(&file_name, &pattern))
        {
            item["file_name_saved"] = json!("未命中过滤规则");
            preview_list.push(item);
            continue;
        }

        if filter_only || default_all {
            if start_key.is_some() {
                let file_key = parse_episode_anchor(&file_name);
                if is_before_start_episode(file_key, start_key) {
                    item["file_name_saved"] = json!(start_skip_label);
                    preview_list.push(item);
                    continue;
                }
            }
            if name_exists_in_dest(&file_name, &dir_filename_list, ignore_ext) {
                item["file_name_saved"] = json!(file_name);
            } else {
                item["name_re"] = json!(file_name);
                item["file_name_re"] = json!(file_name);
            }
            preview_list.push(item);
            continue;
        }

        if !is_video_name(&file_name) && pattern.trim().is_empty() {
            preview_list.push(item);
            continue;
        }

        let renamed = rename_preview_with_taskname(
            &file_name,
            &pattern,
            &replace,
            &taskname,
            series_title.as_deref(),
            Some(tmdb_cfg.guessit_tmdb_tv_rename_template.as_str()),
            tmdb_cfg.disable_guessit_tmdb_fallback_rename,
        );
        if start_key.is_some() {
            let file_key =
                parse_episode_anchor(&renamed).or_else(|| parse_episode_anchor(&file_name));
            if is_before_start_episode(file_key, start_key) {
                item["file_name_saved"] = json!(start_skip_label);
                preview_list.push(item);
                continue;
            }
        }
        if name_exists_in_dest(&renamed, &dir_filename_list, ignore_ext) {
            item["file_name_saved"] = json!(renamed);
        } else {
            item["name_re"] = json!(renamed);
            item["file_name_re"] = json!(renamed);
        }
        preview_list.push(item);
    }

    Ok(json!({
        "drive_type": drive_type,
        "suggested_account_name": account_name,
        "pwd_id": pwd_id,
        "pdir_fid": pdir_fid,
        "items": preview_list
    }))
}

pub async fn preview_share_batch(
    db: &sqlx::SqlitePool,
    payload: SharePreviewBatchIn,
) -> anyhow::Result<Value> {
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();
    for raw in payload.shareurls {
        let url = raw.trim().to_string();
        if url.is_empty() || !seen.insert(url.clone()) {
            continue;
        }
        let drive_type = detect_drive_type(&url);
        if drive_type != Some("quark") {
            items.push(json!({
                "shareurl": url,
                "drive_type": drive_type,
                "ok": false,
                "message": "仅支持夸克分享链接或无法识别"
            }));
            continue;
        }
        match probe_one(db, &url, payload.account_name.as_deref()).await {
            Ok(v) => items.push(v),
            Err(e) => items.push(json!({
                "shareurl": url,
                "drive_type": "quark",
                "ok": false,
                "message": e.to_string()
            })),
        }
    }
    Ok(json!({ "items": items }))
}

async fn probe_one(
    db: &sqlx::SqlitePool,
    url: &str,
    account_name: Option<&str>,
) -> anyhow::Result<Value> {
    let (suggested, adapter) = load_adapter(db, "quark", account_name).await?;
    let extracted = adapter.extract_url(url);
    let pwd_id = extracted
        .pwd_id
        .clone()
        .ok_or_else(|| anyhow::anyhow!("无法解析分享链接"))?;
    let stoken_resp = adapter.get_stoken(&pwd_id, &extracted.passcode).await?;
    let stoken = extract_stoken(&stoken_resp).ok_or_else(|| {
        anyhow::anyhow!(
            "{}",
            stoken_resp
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("获取分享 token 失败")
        )
    })?;
    let mut pdir = value_as_fid(&extracted.pdir_fid);
    let detail = adapter.get_detail(&pwd_id, &stoken, &pdir).await?;
    if let Some(resolved) = detail
        .pointer("/data/resolved_pdir_fid")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        pdir = resolved;
    }
    let list = detail
        .pointer("/data/list")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let latest_video = list
        .iter()
        .filter(|it| !item_is_dir(it))
        .filter_map(|it| {
            let name = item_name(it)?;
            if !is_video_name(&name) {
                return None;
            }
            let (season, episode) = parse_episode_anchor(&name).unwrap_or((None, 0));
            Some(json!({
                "fid": item_fid(it),
                "name": name,
                "updated_at": item_updated_at(it),
                "size": item_size(it),
                "season": season,
                "episode": episode
            }))
        })
        .max_by(|a, b| {
            let ea = a.get("episode").and_then(|v| v.as_i64()).unwrap_or(0);
            let eb = b.get("episode").and_then(|v| v.as_i64()).unwrap_or(0);
            let sa = a.get("season").and_then(|v| v.as_i64()).unwrap_or(0);
            let sb = b.get("season").and_then(|v| v.as_i64()).unwrap_or(0);
            (sa, ea).cmp(&(sb, eb))
        });

    Ok(json!({
        "shareurl": url,
        "drive_type": "quark",
        "ok": true,
        "message": null,
        "suggested_account_name": suggested,
        "pdir_fid": pdir,
        "resolved_pdir_fid": pdir,
        "latest_video": latest_video
    }))
}

pub async fn browse_drive(db: &sqlx::SqlitePool, payload: DriveBrowseIn) -> anyhow::Result<Value> {
    let drive_type = if let Some(url) = payload.shareurl.as_deref().filter(|s| !s.trim().is_empty())
    {
        detect_drive_type(url).unwrap_or("quark")
    } else {
        "quark"
    };
    if drive_type != "quark" {
        anyhow::bail!("RustCAS-X 当前仅支持夸克目录浏览");
    }
    let (account_name, adapter) =
        load_adapter(db, drive_type, payload.account_name.as_deref()).await?;
    let max_items = payload.max_items.unwrap_or(500).clamp(1, 2000);
    let dir_path = payload.dir_path.trim();
    let is_fid_mode = !dir_path.contains('/') && dir_path != "/" && dir_path != "0";
    let pdir_fid = if dir_path.is_empty() || dir_path == "/" || dir_path == "0" {
        "0".to_string()
    } else if is_fid_mode {
        dir_path.to_string()
    } else {
        let normalized = compress_slashes(dir_path);
        let fids = adapter
            .get_fids(&[normalized.clone()])
            .await
            .unwrap_or_default();
        fids.iter()
            .find(|it| {
                it.get("file_path")
                    .or_else(|| it.get("path"))
                    .and_then(|v| v.as_str())
                    == Some(normalized.as_str())
            })
            .or_else(|| fids.first())
            .and_then(item_fid)
            .ok_or_else(|| anyhow::anyhow!("目录不存在: {normalized}"))?
    };

    let listing = adapter.ls_dir(&pdir_fid, max_items).await?;
    let list = listing
        .pointer("/data/list")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let items: Vec<Value> = list
        .iter()
        .filter_map(|raw| {
            let fid = item_fid(raw)?;
            let name = item_name(raw)?;
            let is_dir = item_is_dir(raw);
            Some(json!({
                "fid": fid,
                "name": name,
                "file_name": name,
                "is_dir": is_dir,
                "updated_at": item_updated_at(raw),
                "size": item_size(raw),
                "include_items": Value::Null
            }))
        })
        .collect();

    let paths = if is_fid_mode || dir_path == "/" || dir_path == "0" || dir_path.is_empty() {
        vec![json!({ "fid": pdir_fid, "name": "/", "path": "/" })]
    } else {
        let normalized = compress_slashes(dir_path);
        let mut acc = String::new();
        let mut out = vec![json!({ "fid": "0", "name": "/", "path": "/" })];
        for seg in normalized.split('/').filter(|s| !s.is_empty()) {
            acc.push('/');
            acc.push_str(seg);
            out.push(json!({ "fid": Value::Null, "name": seg, "path": acc }));
        }
        out
    };

    Ok(json!({
        "drive_type": drive_type,
        "account_name": account_name,
        "pdir_fid": pdir_fid,
        "paths": paths,
        "items": items
    }))
}

fn compress_slashes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_slash = false;
    for ch in s.chars() {
        if ch == '/' {
            if !prev_slash {
                out.push(ch);
            }
            prev_slash = true;
        } else {
            out.push(ch);
            prev_slash = false;
        }
    }
    if out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    if out.is_empty() {
        "/".into()
    } else if !out.starts_with('/') {
        format!("/{out}")
    } else {
        out
    }
}
