//! Multi share-link list helpers (shareurl_list_json).
use chrono::Utc;
use serde_json::{json, Value};

pub const MAX_SHARE_LINKS: usize = 10;

pub fn new_link(url: &str, source: &str) -> Value {
    json!({
        "url": url.trim(),
        "status": "unknown",
        "reason": Value::Null,
        "last_checked_at": Value::Null,
        "added_at": Utc::now().to_rfc3339(),
        "source": source,
        "is_active": false
    })
}

pub fn load_share_links(list_json: &str, active_shareurl: Option<&str>) -> Vec<Value> {
    let mut links: Vec<Value> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(list_json) {
        for item in arr {
            let Some(url) = item.get("url").and_then(|v| v.as_str()).map(|s| s.trim().to_string())
            else {
                continue;
            };
            if url.is_empty() || !seen.insert(url.clone()) {
                continue;
            }
            let status = item
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let status = match status {
                "ok" | "invalid" | "unknown" => status,
                _ => "unknown",
            };
            links.push(json!({
                "url": url,
                "status": status,
                "reason": item.get("reason").cloned().unwrap_or(Value::Null),
                "last_checked_at": item.get("last_checked_at").cloned().unwrap_or(Value::Null),
                "added_at": item.get("added_at").cloned().unwrap_or(Value::Null),
                "source": item.get("source").and_then(|v| v.as_str()).unwrap_or("manual"),
                "is_active": false
            }));
        }
    }
    let active = active_shareurl.map(|s| s.trim()).filter(|s| !s.is_empty());
    if let Some(a) = active {
        if !seen.contains(a) {
            let mut link = new_link(a, "legacy");
            link["is_active"] = json!(true);
            links.insert(0, link);
        }
    }
    for link in &mut links {
        let url = link.get("url").and_then(|v| v.as_str()).unwrap_or("");
        link["is_active"] = json!(active == Some(url));
    }
    links
}

pub fn save_share_links(links: &[Value], active_url: &str) -> String {
    let active = active_url.trim();
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in links {
        let Some(url) = item.get("url").and_then(|v| v.as_str()).map(|s| s.trim().to_string())
        else {
            continue;
        };
        if url.is_empty() || !seen.insert(url.clone()) {
            continue;
        }
        out.push(json!({
            "url": url,
            "status": item.get("status").and_then(|v| v.as_str()).unwrap_or("unknown"),
            "reason": item.get("reason").cloned().unwrap_or(Value::Null),
            "last_checked_at": item.get("last_checked_at").cloned().unwrap_or(Value::Null),
            "added_at": item.get("added_at").cloned().unwrap_or(Value::Null),
            "source": item.get("source").and_then(|v| v.as_str()).unwrap_or("manual"),
        }));
    }
    if !active.is_empty() && !seen.contains(active) {
        out.insert(0, new_link(active, "manual"));
    }
    // Evict over limit: drop oldest invalid first
    while out.len() > MAX_SHARE_LINKS {
        let drop_idx = out
            .iter()
            .enumerate()
            .rev()
            .find(|(_, x)| {
                let url = x.get("url").and_then(|v| v.as_str()).unwrap_or("");
                url != active && x.get("status").and_then(|v| v.as_str()) == Some("invalid")
            })
            .map(|(i, _)| i)
            .or_else(|| {
                out.iter()
                    .enumerate()
                    .rev()
                    .find(|(_, x)| x.get("url").and_then(|v| v.as_str()).unwrap_or("") != active)
                    .map(|(i, _)| i)
            });
        if let Some(i) = drop_idx {
            out.remove(i);
        } else {
            break;
        }
    }
    serde_json::to_string(&out).unwrap_or_else(|_| "[]".into())
}

pub fn set_from_urls(urls: &[String], active: &str) -> String {
    let mut links = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let active = active.trim();
    if !active.is_empty() {
        seen.insert(active.to_string());
        links.push(new_link(active, "manual"));
    }
    for u in urls {
        let u = u.trim();
        if u.is_empty() || !seen.insert(u.to_string()) {
            continue;
        }
        links.push(new_link(u, "manual"));
    }
    save_share_links(&links, active)
}

pub fn mark_checked(links: &mut [Value], url: &str, ok: bool, message: Option<&str>) {
    let now = Utc::now().to_rfc3339();
    for link in links.iter_mut() {
        if link.get("url").and_then(|v| v.as_str()) == Some(url) {
            link["status"] = json!(if ok { "ok" } else { "invalid" });
            link["reason"] = json!(message);
            link["last_checked_at"] = json!(now);
        }
    }
}
