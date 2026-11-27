//! Douban mobile rexxar charts for MediaDiscover.
use serde_json::{json, Value};
use std::time::Duration;

const BASE: &str = "https://m.douban.com/rexxar/api/v2";
const UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 14_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0 Mobile/15E148 Safari/604.1";

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(UA)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

pub fn categories() -> Value {
    json!({
        "categories": [
            {
                "key": "movie_hot",
                "label": "热门电影",
                "media_type": "movie",
                "subs": [
                    {"key": "全部", "label": "全部"},
                    {"key": "华语", "label": "华语"},
                    {"key": "欧美", "label": "欧美"},
                    {"key": "韩国", "label": "韩国"},
                    {"key": "日本", "label": "日本"}
                ]
            },
            {
                "key": "movie_latest",
                "label": "最新电影",
                "media_type": "movie",
                "subs": [
                    {"key": "全部", "label": "全部"},
                    {"key": "华语", "label": "华语"},
                    {"key": "欧美", "label": "欧美"},
                    {"key": "韩国", "label": "韩国"},
                    {"key": "日本", "label": "日本"}
                ]
            },
            {
                "key": "movie_top",
                "label": "豆瓣高分",
                "media_type": "movie",
                "subs": [
                    {"key": "全部", "label": "全部"},
                    {"key": "华语", "label": "华语"},
                    {"key": "欧美", "label": "欧美"},
                    {"key": "韩国", "label": "韩国"},
                    {"key": "日本", "label": "日本"}
                ]
            },
            {
                "key": "movie_underrated",
                "label": "冷门佳片",
                "media_type": "movie",
                "subs": [
                    {"key": "全部", "label": "全部"},
                    {"key": "华语", "label": "华语"},
                    {"key": "欧美", "label": "欧美"},
                    {"key": "韩国", "label": "韩国"},
                    {"key": "日本", "label": "日本"}
                ]
            },
            {
                "key": "tv_drama",
                "label": "最近热门剧集",
                "media_type": "tv",
                "subs": [
                    {"key": "综合", "label": "综合"},
                    {"key": "国产剧", "label": "国产剧"},
                    {"key": "欧美剧", "label": "欧美剧"},
                    {"key": "日剧", "label": "日剧"},
                    {"key": "韩剧", "label": "韩剧"},
                    {"key": "动画", "label": "动画"},
                    {"key": "纪录片", "label": "纪录片"}
                ]
            },
            {
                "key": "tv_variety",
                "label": "最近热门综艺",
                "media_type": "tv",
                "subs": [
                    {"key": "综合", "label": "综合"},
                    {"key": "国内", "label": "国内"},
                    {"key": "国外", "label": "国外"}
                ]
            }
        ]
    })
}

fn process_item(item: &Value) -> Option<Value> {
    let title = item
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if title.is_empty() {
        return None;
    }
    let pic = item.get("pic").cloned().unwrap_or(json!({}));
    let pic_url = pic
        .get("normal")
        .or_else(|| pic.get("large"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let rating = item.get("rating").and_then(|r| {
        r.get("value").map(|v| json!({ "value": v }))
    });
    let original_url = item
        .get("url")
        .or_else(|| item.get("uri"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let url = if original_url.starts_with("douban://douban.com/") {
        if let Some(id) = original_url.split("/movie/").nth(1) {
            format!("https://movie.douban.com/subject/{id}/")
        } else if let Some(id) = original_url.split("/tv/").nth(1) {
            format!("https://movie.douban.com/subject/{id}/")
        } else {
            original_url.to_string()
        }
    } else {
        original_url.to_string()
    };
    Some(json!({
        "id": item.get("id").map(|v| v.to_string().trim_matches('"').to_string()).unwrap_or_default(),
        "title": title,
        "year": item.get("year").and_then(|v| v.as_str()).unwrap_or(""),
        "url": url,
        "pic": { "normal": pic_url },
        "rating": rating,
        "card_subtitle": item.get("card_subtitle").and_then(|v| v.as_str()).unwrap_or(""),
        "tmdb": Value::Null
    }))
}

fn movie_category_param(main: &str) -> &'static str {
    match main {
        "movie_latest" => "最新",
        "movie_top" => "豆瓣高分",
        "movie_underrated" => "冷门佳片",
        _ => "热门",
    }
}

fn map_tv_sub(main: &str, sub: &str) -> String {
    if main == "tv_variety" {
        return match sub {
            "国内" => "show_domestic".into(),
            "国外" => "show_foreign".into(),
            _ => "show".into(),
        };
    }
    match sub {
        "国产剧" => "tv_domestic".into(),
        "欧美剧" => "tv_american".into(),
        "日剧" => "tv_japanese".into(),
        "韩剧" => "tv_korean".into(),
        "动画" => "tv_animation".into(),
        "纪录片" => "tv_documentary".into(),
        _ => "tv".into(),
    }
}

fn mock_movie(limit: i64) -> Vec<Value> {
    let base = [
        json!({"id":"1292052","title":"肖申克的救赎","rating":{"value":9.7},"year":"1994","url":"https://movie.douban.com/subject/1292052/","pic":{"normal":""},"card_subtitle":"1994 / 美国 / 剧情 犯罪","tmdb":null}),
        json!({"id":"1291546","title":"霸王别姬","rating":{"value":9.6},"year":"1993","url":"https://movie.douban.com/subject/1291546/","pic":{"normal":""},"card_subtitle":"1993 / 中国大陆 香港 / 剧情 爱情","tmdb":null}),
        json!({"id":"1295644","title":"阿甘正传","rating":{"value":9.5},"year":"1994","url":"https://movie.douban.com/subject/1295644/","pic":{"normal":""},"card_subtitle":"1994 / 美国 / 剧情 爱情","tmdb":null}),
    ];
    base.into_iter().take(limit.max(1).min(3) as usize).collect()
}

fn mock_tv(limit: i64) -> Vec<Value> {
    let base = [
        json!({"id":"26794435","title":"请回答1988","rating":{"value":9.7},"year":"2015","url":"https://movie.douban.com/subject/26794435/","pic":{"normal":""},"card_subtitle":"2015 / 韩国 / 剧情 喜剧 家庭","tmdb":null}),
        json!({"id":"1309163","title":"大明王朝1566","rating":{"value":9.7},"year":"2007","url":"https://movie.douban.com/subject/1309163/","pic":{"normal":""},"card_subtitle":"2007 / 中国大陆 / 剧情 历史","tmdb":null}),
        json!({"id":"1309169","title":"亮剑","rating":{"value":9.3},"year":"2005","url":"https://movie.douban.com/subject/1309169/","pic":{"normal":""},"card_subtitle":"2005 / 中国大陆 / 剧情 战争","tmdb":null}),
    ];
    base.into_iter().take(limit.max(1).min(3) as usize).collect()
}

async fn fetch_ranking(
    path: &str,
    params: &[(&str, String)],
    mock_fn: fn(i64) -> Vec<Value>,
    limit: i64,
) -> Value {
    let client = http();
    let url = format!("{BASE}{path}");
    let result = client
        .get(&url)
        .header("Referer", "https://m.douban.com/")
        .header("Accept", "application/json, text/plain, */*")
        .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
        .query(params)
        .send()
        .await;

    match result {
        Ok(resp) if resp.status().is_success() => {
            let data: Value = resp.json().await.unwrap_or(json!({}));
            let raw_items = data
                .get("items")
                .or_else(|| data.get("subjects"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let mut processed = Vec::new();
            for item in raw_items.iter().take(limit as usize) {
                if let Some(row) = process_item(item) {
                    processed.push(row);
                }
            }
            if processed.is_empty() {
                let items = mock_fn(limit);
                return json!({
                    "success": true,
                    "message": "获取成功（模拟数据）",
                    "is_mock_data": true,
                    "mock_reason": "API返回空数据",
                    "total": items.len(),
                    "items": items
                });
            }
            let total = data
                .get("total")
                .and_then(|v| v.as_i64())
                .unwrap_or(processed.len() as i64);
            json!({
                "success": true,
                "message": "获取成功",
                "is_mock_data": false,
                "total": total,
                "items": processed
            })
        }
        Ok(resp) => {
            tracing::warn!(status = %resp.status(), "douban ranking rejected");
            let items = mock_fn(limit);
            json!({
                "success": true,
                "message": "获取成功（模拟数据）",
                "is_mock_data": true,
                "mock_reason": format!("API status {}", resp.status()),
                "total": items.len(),
                "items": items
            })
        }
        Err(e) => {
            tracing::warn!(error = %e, "douban ranking failed");
            let items = mock_fn(limit);
            json!({
                "success": true,
                "message": "获取成功（模拟数据）",
                "is_mock_data": true,
                "mock_reason": e.to_string(),
                "total": items.len(),
                "items": items
            })
        }
    }
}

/// Attach TMDB briefs by title/year for discover cards (best-effort).
pub async fn enrich_tmdb(
    items: &mut [Value],
    media_type: &str,
    client: &crate::services::tmdb::TmdbClient,
) {
    if !client.is_configured() || items.is_empty() {
        return;
    }
    let mt = if media_type == "movie" { "movie" } else { "tv" };
    for item in items.iter_mut() {
        let title = item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if title.is_empty() {
            continue;
        }
        let year = item
            .get("year")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let raw = if mt == "movie" {
            client.search_movie(&title, year.as_deref(), 1).await
        } else {
            client.search_tv(&title, year.as_deref(), 1).await
        };
        let brief = raw.ok().flatten().and_then(|data| {
            let results = data.get("results")?.as_array()?;
            let best = pick_best_by_year(results, mt, year.as_deref())?;
            Some(to_tmdb_brief(best, mt))
        });
        if let Some(obj) = item.as_object_mut() {
            obj.insert("tmdb".into(), brief.unwrap_or(Value::Null));
        }
    }
}

fn pick_best_by_year<'a>(results: &'a [Value], media_type: &str, year: Option<&str>) -> Option<&'a Value> {
    if results.is_empty() {
        return None;
    }
    let Some(y) = year.filter(|s| !s.is_empty()) else {
        return results.first();
    };
    let date_key = if media_type == "movie" {
        "release_date"
    } else {
        "first_air_date"
    };
    results
        .iter()
        .find(|item| {
            item.get(date_key)
                .and_then(|v| v.as_str())
                .map(|d| d.starts_with(y))
                .unwrap_or(false)
        })
        .or_else(|| results.first())
}

fn to_tmdb_brief(item: &Value, media_type: &str) -> Value {
    if media_type == "movie" {
        json!({
            "id": item.get("id"),
            "media_type": "movie",
            "title": item.get("title"),
            "original_title": item.get("original_title"),
            "overview": item.get("overview"),
            "poster_path": item.get("poster_path"),
            "vote_average": item.get("vote_average"),
            "release_date": item.get("release_date")
        })
    } else {
        json!({
            "id": item.get("id"),
            "media_type": "tv",
            "name": item.get("name"),
            "original_name": item.get("original_name"),
            "overview": item.get("overview"),
            "poster_path": item.get("poster_path"),
            "vote_average": item.get("vote_average"),
            "first_air_date": item.get("first_air_date")
        })
    }
}

pub async fn list(
    main_category: &str,
    sub_category: &str,
    start: i64,
    limit: i64,
) -> Value {
    let main = main_category.trim();
    let sub = sub_category.trim();
    let start = start.max(0);
    let limit = limit.clamp(1, 50);

    if main.starts_with("movie_") {
        let mut params = vec![
            ("start", start.to_string()),
            ("limit", limit.to_string()),
        ];
        let cat = movie_category_param(main);
        if cat != "热门" {
            params.push(("category", cat.to_string()));
        }
        if !sub.is_empty() && sub != "全部" {
            params.push(("type", sub.to_string()));
        }
        return fetch_ranking("/subject/recent_hot/movie", &params, mock_movie, limit).await;
    }

    if main.starts_with("tv_") {
        let mut params = vec![
            ("start", start.to_string()),
            ("limit", limit.to_string()),
        ];
        if !sub.is_empty() {
            params.push(("type", map_tv_sub(main, sub)));
        }
        if main == "tv_variety" {
            params.push(("category", "show".into()));
        }
        return fetch_ranking("/subject/recent_hot/tv", &params, mock_tv, limit).await;
    }

    json!({
        "success": false,
        "message": format!("不支持的主分类: {main}"),
        "is_mock_data": false,
        "total": 0,
        "items": []
    })
}
