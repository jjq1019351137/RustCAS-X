//! Filename pattern replace + TMDB-style SxxExx rename fallback.

use crate::services::episode::parse_episode_anchor;
use regex::Regex;

pub fn apply_pattern_replace(origin: &str, pattern: &str, replace: &str) -> String {
    let pattern = pattern.trim();
    let replace = replace.trim();
    if pattern.is_empty() {
        return origin.to_string();
    }
    let Ok(re) = Regex::new(pattern) else {
        return origin.to_string();
    };
    if replace.is_empty() {
        // filter-only mode: keep original name
        return origin.to_string();
    }
    re.replace(origin, replace).to_string()
}

pub fn is_filter_only(pattern: &str, replace: &str) -> bool {
    !pattern.trim().is_empty() && replace.trim().is_empty()
}

pub fn matches_pattern(origin: &str, pattern: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return true;
    }
    match Regex::new(pattern) {
        Ok(re) => re.is_match(origin),
        Err(_) => true,
    }
}

/// When no pattern/replace: `{title}.SxxExx{ext}` if episode can be parsed.
pub fn tmdb_fallback_rename(
    origin: &str,
    series_title: &str,
    template: Option<&str>,
) -> Option<String> {
    let title = series_title.trim();
    if title.is_empty() {
        return None;
    }
    if !is_video_name(origin) {
        return None;
    }
    let (season, episode) = parse_episode_anchor(origin)?;
    let ext = extension(origin).unwrap_or_default();
    let ss = season.unwrap_or(1);
    let ee = episode;
    let tpl = template.unwrap_or("").trim();
    if !tpl.is_empty() {
        let mut out = tpl
            .replace("{title}", title)
            .replace("{name}", title)
            .replace("{season}", &format!("{ss:02}"))
            .replace("{episode}", &format!("{ee:02}"))
            .replace("{S}", &format!("S{ss:02}"))
            .replace("{E}", &format!("E{ee:02}"))
            .replace("{ext}", &ext);
        if !ext.is_empty() && !out.to_lowercase().ends_with(&ext.to_lowercase()) {
            out.push_str(&ext);
        }
        return Some(out);
    }
    Some(format!("{title}.S{ss:02}E{ee:02}{ext}"))
}

pub fn rename_preview(
    origin: &str,
    pattern: &str,
    replace: &str,
    series_title: Option<&str>,
    tv_template: Option<&str>,
    disable_tmdb_fallback: bool,
) -> String {
    let pattern = pattern.trim();
    let replace = replace.trim();
    if is_filter_only(pattern, replace) {
        return origin.to_string();
    }
    if !pattern.is_empty() {
        return apply_pattern_replace(origin, pattern, replace);
    }
    if !disable_tmdb_fallback {
        if let Some(title) = series_title.filter(|s| !s.trim().is_empty()) {
            if let Some(renamed) = tmdb_fallback_rename(origin, title, tv_template) {
                return renamed;
            }
        }
    }
    origin.to_string()
}

fn is_video_name(name: &str) -> bool {
    const EXTS: &[&str] = &[
        ".mkv", ".mp4", ".ts", ".m2ts", ".avi", ".mov", ".wmv", ".flv", ".webm", ".iso",
    ];
    let lower = name.to_lowercase();
    EXTS.iter().any(|e| lower.ends_with(e))
}

fn extension(name: &str) -> Option<String> {
    let (_, ext) = name.rsplit_once('.')?;
    if ext.is_empty() || ext.len() > 8 || ext.contains(['/', '\\']) {
        return None;
    }
    Some(format!(".{ext}"))
}
