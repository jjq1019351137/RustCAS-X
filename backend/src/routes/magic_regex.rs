use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::AppResult;
use crate::services::magic_rename;
use crate::state::AppState;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

async fn list_rules(user: CurrentUser) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    Ok(Json(json!({
        "rules": magic_rename::list_builtin_rules()
    })))
}

async fn picker(user: CurrentUser) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let rules: Vec<Value> = magic_rename::list_builtin_rules()
        .into_iter()
        .map(|r| {
            json!({
                "key": r.get("key"),
                "label": r.get("label"),
                "pattern": r.get("pattern"),
                "replace": r.get("replace")
            })
        })
        .collect();
    Ok(Json(json!(rules)))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_rules))
        .route("/picker", get(picker))
}
