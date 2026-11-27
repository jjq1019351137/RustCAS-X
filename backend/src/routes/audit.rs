//! Audit log list/detail/actions — maps lean SQLite rows to frontend shape.
use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize, Default)]
struct ListQuery {
    page: Option<i64>,
    page_size: Option<i64>,
    q: Option<String>,
    action: Option<String>,
    category: Option<String>,
    source: Option<String>,
    success: Option<bool>,
}

fn category_of(action: &str) -> &str {
    action.split('.').next().unwrap_or("system")
}

fn category_label(cat: &str) -> &str {
    match cat {
        "task" => "任务",
        "sync" => "同步",
        "drive_account" => "网盘账号",
        "user" => "用户",
        "role" => "角色",
        "plugin" => "插件",
        "system" => "系统",
        "auth" => "认证",
        "notification" => "通知",
        "tmdb" => "TMDB",
        "dl302" => "DL302",
        _ => cat,
    }
}

fn action_label(action: &str) -> String {
    let map = [
        ("task.create", "创建任务"),
        ("task.update", "更新任务"),
        ("task.delete", "删除任务"),
        ("task.status", "启停任务"),
        ("task.run", "手动执行"),
        ("task.repair_banned", "修复失效分享"),
        ("plugin.refresh", "刷新插件"),
        ("plugin.update", "更新插件"),
        ("auth.login", "登录"),
        ("auth.logout", "退出登录"),
    ];
    map.iter()
        .find(|(k, _)| *k == action)
        .map(|(_, v)| (*v).to_string())
        .unwrap_or_else(|| action.to_string())
}

fn source_label(source: Option<&str>) -> Option<String> {
    match source {
        Some("manual") => Some("手动".into()),
        Some("scheduler") => Some("调度器".into()),
        Some("linked") => Some("联动触发".into()),
        Some("auto") => Some("自动流程".into()),
        Some(s) => Some(s.to_string()),
        None => None,
    }
}

fn row_to_item(
    id: i64,
    created_at: String,
    actor_user_id: Option<i64>,
    actor_username: Option<String>,
    action: String,
    target_type: Option<String>,
    target_id: Option<String>,
    ip: Option<String>,
    success: i64,
    detail: Option<String>,
) -> Value {
    let cat = category_of(&action).to_string();
    let summary = detail
        .as_deref()
        .map(|s| {
            let t = s.trim();
            if t.is_empty() {
                action_label(&action)
            } else if t.chars().count() > 120 {
                format!("{}…", t.chars().take(120).collect::<String>())
            } else {
                t.to_string()
            }
        })
        .unwrap_or_else(|| action_label(&action));
    let has_detail = detail.as_ref().map(|s| !s.trim().is_empty()).unwrap_or(false);
    json!({
        "id": id,
        "created_at": created_at,
        "actor_type": if actor_user_id.is_some() { "user" } else { "system" },
        "actor_user_id": actor_user_id,
        "actor_username": actor_username,
        "source": "manual",
        "source_label": source_label(Some("manual")),
        "category": cat,
        "category_label": category_label(&cat),
        "action": action,
        "action_label": action_label(&action),
        "target_type": target_type,
        "target_id": target_id,
        "target_name": Value::Null,
        "summary": summary,
        "ip": ip,
        "success": success != 0,
        "error_message": Value::Null,
        "has_detail": has_detail
    })
}

async fn list_logs(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "audit:read")?;
    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;

    let mut where_sql = String::from("WHERE 1=1");
    let mut binds: Vec<String> = Vec::new();
    if let Some(raw) = q.q.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        where_sql.push_str(" AND (action LIKE ? OR IFNULL(detail,'') LIKE ? OR IFNULL(target_id,'') LIKE ?)");
        let like = format!("%{raw}%");
        binds.push(like.clone());
        binds.push(like.clone());
        binds.push(like);
    }
    if let Some(action) = q.action.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        where_sql.push_str(" AND action = ?");
        binds.push(action);
    }
    if let Some(cat) = q.category.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        where_sql.push_str(" AND action LIKE ?");
        binds.push(format!("{cat}.%"));
    }
    if let Some(success) = q.success {
        where_sql.push_str(" AND success = ?");
        binds.push(if success { "1".into() } else { "0".into() });
    }
    let _ = q.source; // lean schema has no source column yet

    let count_sql = format!("SELECT COUNT(*) FROM audit_logs {where_sql}");
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        count_q = count_q.bind(b);
    }
    let total = count_q.fetch_one(&state.db).await.unwrap_or(0);

    let list_sql = format!(
        r#"
        SELECT a.id, a.created_at, a.actor_user_id, u.username, a.action,
               a.target_type, a.target_id, a.ip, a.success, a.detail
        FROM audit_logs a
        LEFT JOIN users u ON u.id = a.actor_user_id
        {where_sql}
        ORDER BY a.id DESC
        LIMIT ? OFFSET ?
        "#
    );
    let mut list_q = sqlx::query_as::<
        _,
        (
            i64,
            String,
            Option<i64>,
            Option<String>,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            i64,
            Option<String>,
        ),
    >(&list_sql);
    for b in &binds {
        list_q = list_q.bind(b);
    }
    list_q = list_q.bind(page_size).bind(offset);
    let rows = list_q.fetch_all(&state.db).await?;

    let items: Vec<Value> = rows
        .into_iter()
        .map(|(id, created_at, uid, uname, action, tt, tid, ip, success, detail)| {
            row_to_item(id, created_at, uid, uname, action, tt, tid, ip, success, detail)
        })
        .collect();

    Ok(Json(json!({
        "page": page,
        "page_size": page_size,
        "total": total,
        "items": items
    })))
}

async fn actions_registry(user: CurrentUser) -> AppResult<Json<Value>> {
    require_perm(&user, "audit:read")?;
    Ok(Json(json!({
        "actions": [
            {"code": "task.create", "label": "创建任务", "category": "task", "category_label": "任务"},
            {"code": "task.update", "label": "更新任务", "category": "task", "category_label": "任务"},
            {"code": "task.delete", "label": "删除任务", "category": "task", "category_label": "任务"},
            {"code": "task.status", "label": "启停任务", "category": "task", "category_label": "任务"},
            {"code": "task.run", "label": "手动执行", "category": "task", "category_label": "任务"},
            {"code": "task.repair_banned", "label": "修复失效分享", "category": "task", "category_label": "任务"},
            {"code": "plugin.refresh", "label": "刷新插件", "category": "plugin", "category_label": "插件"},
            {"code": "plugin.update", "label": "更新插件", "category": "plugin", "category_label": "插件"},
            {"code": "auth.login", "label": "登录", "category": "auth", "category_label": "认证"}
        ],
        "categories": {
            "task": "任务",
            "sync": "同步",
            "drive_account": "网盘账号",
            "user": "用户",
            "plugin": "插件",
            "auth": "认证",
            "system": "系统"
        },
        "sources": {
            "manual": "手动",
            "scheduler": "调度器",
            "linked": "联动触发",
            "auto": "自动流程"
        }
    })))
}

async fn get_detail(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "audit:read")?;
    let row = sqlx::query_as::<
        _,
        (
            i64,
            String,
            Option<i64>,
            Option<String>,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            i64,
            Option<String>,
        ),
    >(
        r#"
        SELECT a.id, a.created_at, a.actor_user_id, u.username, a.action,
               a.target_type, a.target_id, a.ip, a.success, a.detail
        FROM audit_logs a
        LEFT JOIN users u ON u.id = a.actor_user_id
        WHERE a.id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::bad_request("AUDIT_NOT_FOUND", "审计日志不存在"))?;

    let (id, created_at, uid, uname, action, tt, tid, ip, success, detail) = row;
    let mut item = row_to_item(id, created_at, uid, uname, action, tt, tid, ip, success, detail.clone());
    if let Some(obj) = item.as_object_mut() {
        obj.insert("detail".into(), json!(detail));
    }
    Ok(Json(item))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_logs))
        .route("/actions", get(actions_registry))
        .route("/{id}", get(get_detail))
}
