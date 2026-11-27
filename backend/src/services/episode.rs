//! Episode anchor parsing / start-episode filtering (port of start_episode_filter.py + guessit-ish CN).

use regex::Regex;
use std::sync::OnceLock;

fn re_se() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)[Ss](\d{1,2})\s*[.\-_ ]?\s*[Ee][Pp]?(\d{1,4})\b").unwrap())
}

fn re_e_only() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|[^A-Za-z0-9])[Ee][Pp]?(\d{1,4})\b").unwrap())
}

fn re_bare() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[Ee]?(\d{1,4})$").unwrap())
}

fn re_cn_ep() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"第\s*0*(\d{1,4})\s*[集期话部篇]").unwrap())
}

fn re_cn_ep_bare() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|[^\d第])0*(\d{1,4})\s*集").unwrap())
}

fn re_cn_season() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"第\s*([0-9一二两三四五六七八九十]{1,3})\s*季").unwrap())
}

fn re_leading_ep() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // 01.Show / 12 1080p / 05-title
    RE.get_or_init(|| {
        Regex::new(r"(?i)^\s*(\d{1,4})(?:\s*(?:\(\s*v\d+\s*\)|v\d+))?\s*(?:[-._\s]+|$)").unwrap()
    })
}

fn re_leading_ep_res() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^\s*(\d{1,4})(?:\s*(?:\(\s*v\d+\s*\)|v\d+))?\s*[\s._\-\\/／~]+\s*(?:4k|8k|2160p|1080p|720p)(?:[\s._\-\\/／].*)?$").unwrap()
    })
}

pub type EpisodeKey = (Option<i32>, i32);

pub fn format_episode_anchor(season: Option<i32>, episode: i32) -> String {
    match season {
        Some(s) if s > 0 => format!("S{s:02}E{episode:02}"),
        _ => format!("E{episode}"),
    }
}

fn cn_num(s: &str) -> Option<i32> {
    if let Ok(n) = s.parse::<i32>() {
        return Some(n);
    }
    let map = [
        ('一', 1),
        ('二', 2),
        ('两', 2),
        ('三', 3),
        ('四', 4),
        ('五', 5),
        ('六', 6),
        ('七', 7),
        ('八', 8),
        ('九', 9),
        ('十', 10),
    ];
    if s.chars().count() == 1 {
        return map.iter().find(|(c, _)| s.starts_with(*c)).map(|(_, n)| *n);
    }
    if s.starts_with('十') {
        let rest = s.chars().nth(1);
        if rest.is_none() {
            return Some(10);
        }
        let ones = map.iter().find(|(c, _)| rest == Some(*c)).map(|(_, n)| *n)?;
        return Some(10 + ones);
    }
    if s.ends_with('十') {
        let first = s.chars().next()?;
        let tens = map.iter().find(|(c, _)| *c == first).map(|(_, n)| *n)?;
        return Some(tens * 10);
    }
    None
}

pub fn parse_episode_anchor(value: &str) -> Option<EpisodeKey> {
    let text = value.trim();
    if text.is_empty() {
        return None;
    }
    let stem = text
        .rsplit_once('.')
        .filter(|(_, ext)| !ext.is_empty() && ext.len() <= 8 && !ext.contains(['/', '\\']))
        .map(|(s, _)| s)
        .unwrap_or(text)
        .trim();
    let candidate = if stem.is_empty() { text } else { stem };

    if let Some(c) = re_se().captures(candidate) {
        let season = c.get(1)?.as_str().parse::<i32>().ok()?;
        let episode = c.get(2)?.as_str().parse::<i32>().ok()?;
        if episode > 0 {
            return Some((Some(season), episode));
        }
    }

    let mut season_hint: Option<i32> = None;
    if let Some(c) = re_cn_season().captures(candidate) {
        season_hint = cn_num(c.get(1)?.as_str());
    }

    if let Some(c) = re_cn_ep().captures(candidate) {
        let episode = c.get(1)?.as_str().parse::<i32>().ok()?;
        if episode > 0 {
            return Some((season_hint, episode));
        }
    }
    if let Some(c) = re_cn_ep_bare().captures(candidate) {
        let episode = c.get(1)?.as_str().parse::<i32>().ok()?;
        if episode > 0 {
            return Some((season_hint, episode));
        }
    }
    if let Some(c) = re_e_only().captures(candidate) {
        let episode = c.get(1)?.as_str().parse::<i32>().ok()?;
        if episode > 0 {
            return Some((season_hint, episode));
        }
    }
    if let Some(c) = re_bare().captures(candidate) {
        let episode = c.get(1)?.as_str().parse::<i32>().ok()?;
        if episode > 0 && episode <= 9999 {
            return Some((season_hint, episode));
        }
    }
    // Strict known: "12 1080p" / leading episode
    if let Some(c) = re_leading_ep_res().captures(candidate) {
        let episode = c.get(1)?.as_str().parse::<i32>().ok()?;
        if (1..=9999).contains(&episode) {
            return Some((season_hint.or(Some(1)), episode));
        }
    }
    if let Some(c) = re_leading_ep().captures(candidate) {
        let episode = c.get(1)?.as_str().parse::<i32>().ok()?;
        // Avoid years like 2024 at start
        if (1..=999).contains(&episode) {
            return Some((season_hint.or(Some(1)), episode));
        }
    }
    None
}

pub fn resolve_start_episode_key(
    start_value: &str,
    share_names_by_fid: &[(String, String)],
) -> Option<EpisodeKey> {
    let raw = start_value.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(direct) = parse_episode_anchor(raw) {
        return Some(direct);
    }
    for (fid, name) in share_names_by_fid {
        if fid == raw {
            return parse_episode_anchor(name);
        }
    }
    None
}

/// True when file episode is strictly before start (start itself is kept: >= transfers).
pub fn is_before_start_episode(file_key: Option<EpisodeKey>, start_key: Option<EpisodeKey>) -> bool {
    let (Some(file), Some(start)) = (file_key, start_key) else {
        return false;
    };
    match (file.0, start.0) {
        (Some(fs), Some(ss)) => (fs, file.1) < (ss, start.1),
        _ => file.1 < start.1,
    }
}

pub fn name_exists_in_dest(target: &str, dest_names: &[String], ignore_extension: bool) -> bool {
    if target.is_empty() {
        return false;
    }
    let key = if ignore_extension {
        stem(target)
    } else {
        target.to_string()
    };
    dest_names.iter().any(|d| {
        let other = if ignore_extension { stem(d) } else { d.clone() };
        other.eq_ignore_ascii_case(&key)
    })
}

fn stem(name: &str) -> String {
    name.rsplit_once('.')
        .filter(|(_, ext)| !ext.is_empty() && ext.len() <= 8 && !ext.contains(['/', '\\']))
        .map(|(s, _)| s.to_string())
        .unwrap_or_else(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sxxexx() {
        assert_eq!(parse_episode_anchor("Show.S01E12.mkv"), Some((Some(1), 12)));
        assert_eq!(parse_episode_anchor("E08"), Some((None, 8)));
        assert_eq!(parse_episode_anchor("S01E92"), Some((Some(1), 92)));
    }

    #[test]
    fn parse_cn() {
        assert_eq!(parse_episode_anchor("剧名 第12集.mkv"), Some((None, 12)));
        assert_eq!(parse_episode_anchor("第2季第5集.mp4"), Some((Some(2), 5)));
        assert_eq!(parse_episode_anchor("12.1080p.mkv"), Some((Some(1), 12)));
    }

    #[test]
    fn before_start() {
        let start = Some((Some(1), 10));
        assert!(is_before_start_episode(Some((Some(1), 9)), start));
        assert!(!is_before_start_episode(Some((Some(1), 10)), start));
        assert!(!is_before_start_episode(Some((Some(1), 11)), start));
    }
}
