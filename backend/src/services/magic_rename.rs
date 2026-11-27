//! Port of Python MagicRename: $TV_REGEX / magic variables / sub().
//! Patterns avoid regex lookbehind (unsupported by the `regex` crate).

use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::OnceLock;

pub fn builtin_magic_regex() -> HashMap<String, (String, String)> {
    let mut m = HashMap::new();
    m.insert(
        "$TV_REGEX".into(),
        (
            r".*?([Ss]\d{1,2})?(?:[第EePpXx.\-_( ]{1,2}|^)(\d{1,3})(?!\d).*?\.(mp4|mkv|mov|m4v|avi|mpeg|ts|zip|cas|iso)".into(),
            "{TASKNAME}.{SXX}E{E}.{EXT}".into(),
        ),
    );
    // Note: Rust `regex` has no lookaround — blacklist/show exclude run in code.
    m.insert(
        "$BLACK_WORD".into(),
        (
            r".*".into(),
            "".into(),
        ),
    );
    m.insert(
        "$TV_MAGIC".into(),
        (
            r".*\.(mp4|mkv|mov|m4v|avi|mpeg|ts|zip|cas|iso)$".into(),
            "{TASKNAME}.{SXX}E{E}.{EXT}".into(),
        ),
    );
    m.insert(
        "$SHOW_MAGIC".into(),
        (
            r".*?第\d+期.*".into(),
            "{TASKNAME}.{SXX}E{II}.第{E}期{PART}.{EXT}".into(),
        ),
    );
    m.insert(
        "$SHOW_PRO".into(),
        (
            r".*?第\d+期.*".into(),
            "{II}.{TASKNAME}.{DATE}.第{E}期{PART}.{EXT}".into(),
        ),
    );
    m
}

pub fn builtin_labels() -> HashMap<String, String> {
    HashMap::from([
        ("$TV_REGEX".into(), "剧集集数（SxxExx）".into()),
        ("$TV_MAGIC".into(), "通用视频命名".into()),
        ("$SHOW_MAGIC".into(), "综艺期数命名".into()),
        ("$SHOW_PRO".into(), "综艺期数命名（含日期）".into()),
        ("$BLACK_WORD".into(), "黑名单过滤（仅筛选不改名）".into()),
    ])
}

pub fn list_builtin_rules() -> Vec<Value> {
    let labels = builtin_labels();
    let order = ["$TV_REGEX", "$TV_MAGIC", "$SHOW_MAGIC", "$SHOW_PRO", "$BLACK_WORD"];
    let map = builtin_magic_regex();
    order
        .iter()
        .filter_map(|k| {
            let (pattern, replace) = map.get(*k)?;
            Some(json!({
                "key": k,
                "label": labels.get(*k).cloned().unwrap_or_default(),
                "enabled": true,
                "built_in": true,
                "overridden": false,
                "pattern": pattern,
                "replace": replace,
                "default_pattern": pattern,
                "default_replace": replace
            }))
        })
        .collect()
}

pub struct MagicRename {
    magic_regex: HashMap<String, (String, String)>,
    taskname: String,
}

impl MagicRename {
    pub fn new() -> Self {
        Self {
            magic_regex: builtin_magic_regex(),
            taskname: String::new(),
        }
    }

    pub fn with_extra(extra: HashMap<String, (String, String)>) -> Self {
        let mut s = Self::new();
        s.magic_regex.extend(extra);
        s
    }

    pub fn set_taskname(&mut self, name: &str) {
        self.taskname = name.to_string();
    }

    pub fn magic_regex_conv(&self, pattern: &str, replace: &str) -> (String, String) {
        if let Some((p, r)) = self.magic_regex.get(pattern.trim()) {
            let replace = if replace.trim().is_empty() {
                r.clone()
            } else {
                replace.to_string()
            };
            return (p.clone(), replace);
        }
        (pattern.to_string(), replace.to_string())
    }

    pub fn sub(&self, pattern: &str, replace: &str, file_name: &str) -> String {
        if pattern.is_empty() && replace.is_empty() {
            return file_name.to_string();
        }
        let filled = self.fill_variables(replace, file_name);
        if pattern.is_empty() {
            return normalize_episode_filename(&filled, file_name);
        }
        let Ok(re) = Regex::new(pattern) else {
            return file_name.to_string();
        };
        // Support both magic templates and numeric backrefs like $1
        let result = if filled.contains('{') {
            // Already expanded magic vars — use as literal replacement string
            // Escape $ that aren't backrefs carefully: Regex::replace treats $N as groups
            match re.replace(file_name, filled.as_str()) {
                std::borrow::Cow::Borrowed(s) => s.to_string(),
                std::borrow::Cow::Owned(s) => s,
            }
        } else {
            re.replace(file_name, filled.as_str()).into_owned()
        };
        normalize_episode_filename(&result, file_name)
    }

    fn fill_variables(&self, replace_template: &str, file_name: &str) -> String {
        let mut replace = replace_template.to_string();
        if replace.contains("{TASKNAME}") {
            replace = replace.replace("{TASKNAME}", &self.taskname);
        }
        for (key, patterns) in magic_variable_patterns() {
            if !replace.contains(key) {
                continue;
            }
            let mut matched = None;
            for pat in patterns.iter() {
                let Ok(re) = Regex::new(pat) else { continue };
                if let Some(c) = re.captures(file_name) {
                    // Prefer first capture group if present, else whole match
                    let value = c
                        .get(1)
                        .or_else(|| c.get(0))
                        .map(|m| m.as_str().to_string())
                        .unwrap_or_default();
                    matched = Some(value);
                    break;
                }
            }
            let value = match matched {
                Some(v) => format_magic_value(key, &v),
                None => {
                    if *key == "{SXX}" {
                        "S01".into()
                    } else {
                        String::new()
                    }
                }
            };
            replace = replace.replace(key, &value);
        }
        // {II} index placeholder left for later sort pass — leave empty digits hint
        if replace.contains("{II}") {
            replace = replace.replace("{II}", "01");
        }
        if replace.contains("{I}") {
            replace = replace.replace("{I}", "1");
        }
        replace
    }
}

impl Default for MagicRename {
    fn default() -> Self {
        Self::new()
    }
}

const BLACK_WORDS: &[&str] = &["纯享", "加更", "超前企划", "训练室", "蒸蒸日上"];
const SHOW_EXCLUDE: &[&str] = &["纯享", "加更", "抢先", "预告"];

/// Extra keyword filters for magic keys that cannot use lookaround.
pub fn passes_magic_keyword_filter(magic_key: &str, file_name: &str) -> bool {
    match magic_key.trim() {
        "$BLACK_WORD" => !BLACK_WORDS.iter().any(|w| file_name.contains(w)),
        "$SHOW_MAGIC" | "$SHOW_PRO" => !SHOW_EXCLUDE.iter().any(|w| file_name.contains(w)),
        _ => true,
    }
}

fn magic_variable_patterns() -> &'static [(&'static str, &'static [&'static str])] {
    static PATS: OnceLock<Vec<(&'static str, &'static [&'static str])>> = OnceLock::new();
    PATS.get_or_init(|| {
        vec![
            ("{EXT}", &["\\.(\\w+)$"][..]),
            ("{CHINESE}", &[r"[\u{4e00}-\u{9fa5}]{2,}"][..]),
            (
                "{DATE}",
                &[
                    r"(?:18|19|20)?\d{2}[.\-/年]\d{1,2}[.\-/月]\d{1,2}",
                    r"(?:^|[^\d])([12]\d{3}[01]?\d[0123]?\d)(?:[^\d]|$)",
                    r"(?:^|[^\d])([01]?\d[.\-/月][0123]?\d)",
                ],
            ),
            ("{YEAR}", &[r"(?:^|[^\d])((?:18|19|20)\d{2})(?:[^\d|]|$)"][..]),
            (
                "{S}",
                &[r"[Ss](\d{1,2})(?=[EeXx])", r"[Ss](\d{1,2})"],
            ),
            (
                "{SXX}",
                &[r"[Ss]\d{1,2}(?=[EeXx])", r"[Ss]\d{1,2}"],
            ),
            (
                "{E}",
                &[
                    r"[Ss]\d{1,2}[Ee](\d{1,3})",
                    r"[Ee][Pp]?(\d{1,3})",
                    r"第(\d{1,3})[集期话部篇]",
                    r"(\d{1,3})[集期话部篇]",
                    r"(?:^|[._])(\d{1,3})(?:[._]|$)",
                    r"^(\d{1,3})\.\w+",
                    r"(?:^|[^\d])(\d{1,3})(?:[^\d]|$)",
                ],
            ),
            (
                "{PART}",
                &[
                    r"[集期话部篇第]([上中下一二三四五六七八九十])",
                    r"([上中下一二三四五六七八九十])",
                ],
            ),
            ("{VER}", &[r"[\u{4e00}-\u{9fa5}]+版"][..]),
        ]
    })
    .as_slice()
}

fn format_magic_value(key: &str, raw: &str) -> String {
    match key {
        "{EXT}" => raw.trim_start_matches('.').to_string(),
        "{E}" => {
            let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
            let n: i32 = digits.parse().unwrap_or(0);
            format!("{n:02}")
        }
        "{SXX}" => {
            let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = digits.parse::<i32>() {
                format!("S{n:02}")
            } else if raw.to_uppercase().starts_with('S') {
                raw.to_uppercase()
            } else {
                format!("S{raw}")
            }
        }
        "{S}" => raw.chars().filter(|c| c.is_ascii_digit()).collect(),
        "{DATE}" => {
            let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
            if digits.len() >= 8 {
                digits.chars().take(8).collect()
            } else {
                let year = chrono::Utc::now().format("%Y").to_string();
                let need = 8usize.saturating_sub(digits.len()).min(year.len());
                format!("{}{}", &year[..need], digits)
            }
        }
        _ => raw.to_string(),
    }
}

fn normalize_episode_filename(name: &str, original: &str) -> String {
    let result = name.to_string();
    if result.is_empty() || result == original {
        return result;
    }
    static BARE: OnceLock<Regex> = OnceLock::new();
    let bare = BARE.get_or_init(|| Regex::new(r"^E(\d{1,4})(\.[A-Za-z0-9]+)$").unwrap());
    if let Some(c) = bare.captures(&result) {
        let ep: i32 = c[1].parse().unwrap_or(0);
        return format!("S01E{ep:02}{}", &c[2]);
    }
    static SXX: OnceLock<Regex> = OnceLock::new();
    let sxx = SXX.get_or_init(|| {
        Regex::new(r"(^|.*?\.)([Ss])(\d{1,2})([Ee])(\d{1,4})(\.[A-Za-z0-9]+)?$").unwrap()
    });
    if let Some(c) = sxx.captures(&result) {
        let season: i32 = c[3].parse().unwrap_or(1);
        let ep: i32 = c[5].parse().unwrap_or(0);
        return format!(
            "{}{}{:02}{}{:02}{}",
            &c[1],
            c[2].to_uppercase(),
            season,
            c[4].to_uppercase(),
            ep,
            c.get(6).map(|m| m.as_str()).unwrap_or("")
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conv_tv_regex() {
        let mr = MagicRename::new();
        let (p, r) = mr.magic_regex_conv("$TV_REGEX", "");
        assert!(p.contains(r"\d{1,3}"));
        assert!(r.contains("{TASKNAME}"));
    }

    #[test]
    fn sub_black_word_filter() {
        assert!(passes_magic_keyword_filter("$BLACK_WORD", "正常剧集S01E01.mkv"));
        assert!(!passes_magic_keyword_filter("$BLACK_WORD", "纯享加更.mkv"));
        assert!(passes_magic_keyword_filter("$SHOW_MAGIC", "某某第3期.mkv"));
        assert!(!passes_magic_keyword_filter("$SHOW_MAGIC", "某某纯享第3期.mkv"));
    }
}
