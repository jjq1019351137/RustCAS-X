use crate::auth_ctx::{require_perm, CurrentUser};
use crate::error::{AppError, AppResult};
use crate::services::magic_rename;
use crate::settings_store::{get_or_default, put_typed};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::{delete, get, patch};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Deserialize, Default)]
struct RulePatchIn {
    enabled: Option<bool>,
    pattern: Option<String>,
    replace: Option<String>,
    label: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize, Default)]
struct MagicOverrides {
    #[serde(default)]
    rules: Map<String, Value>,
}

async fn load_overrides(state: &AppState) -> MagicOverrides {
    get_or_default(&state.db, "magic_regex_overrides")
        .await
        .unwrap_or_default()
}

fn merge_rules(overrides: &MagicOverrides) -> Vec<Value> {
    let mut out = magic_rename::list_builtin_rules();
    for rule in &mut out {
        let key = rule
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if let Some(over) = overrides.rules.get(&key) {
            if let Some(obj) = rule.as_object_mut() {
                if let Some(v) = over.get("enabled") {
                    obj.insert("enabled".into(), v.clone());
                }
                if let Some(v) = over.get("pattern") {
                    obj.insert("pattern".into(), v.clone());
                    obj.insert("overridden".into(), json!(true));
                }
                if let Some(v) = over.get("replace") {
                    obj.insert("replace".into(), v.clone());
                    obj.insert("overridden".into(), json!(true));
                }
                if let Some(v) = over.get("label") {
                    obj.insert("label".into(), v.clone());
                }
            }
        }
    }
    out
}

async fn list_rules(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let overrides = load_overrides(&state).await;
    Ok(Json(json!({ "rules": merge_rules(&overrides) })))
}

async fn picker(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Value>> {
    require_perm(&user, "task:read")?;
    let overrides = load_overrides(&state).await;
    let rules: Vec<Value> = merge_rules(&overrides)
        .into_iter()
        .filter(|r| r.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true))
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

async fn patch_rule(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
    Json(payload): Json<RulePatchIn>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let builtins = magic_rename::list_builtin_rules();
    if !builtins
        .iter()
        .any(|r| r.get("key").and_then(|v| v.as_str()) == Some(key.as_str()))
    {
        return Err(AppError::bad_request("MAGIC_RULE_NOT_FOUND", "规则不存在"));
    }
    let mut overrides = load_overrides(&state).await;
    let entry = overrides.rules.entry(key).or_insert_with(|| json!({}));
    if let Some(obj) = entry.as_object_mut() {
        if let Some(v) = payload.enabled {
            obj.insert("enabled".into(), json!(v));
        }
        if let Some(v) = payload.pattern {
            obj.insert("pattern".into(), json!(v));
        }
        if let Some(v) = payload.replace {
            obj.insert("replace".into(), json!(v));
        }
        if let Some(v) = payload.label {
            obj.insert("label".into(), json!(v));
        }
    }
    put_typed(&state.db, "magic_regex_overrides", &overrides)
        .await
        .map_err(|e| AppError::bad_request("MAGIC_RULE_SAVE", e.to_string()))?;
    Ok(Json(json!({ "rules": merge_rules(&overrides) })))
}

async fn delete_rule(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
) -> AppResult<Json<Value>> {
    require_perm(&user, "task:write")?;
    let mut overrides = load_overrides(&state).await;
    overrides.rules.remove(&key);
    put_typed(&state.db, "magic_regex_overrides", &overrides)
        .await
        .map_err(|e| AppError::bad_request("MAGIC_RULE_SAVE", e.to_string()))?;
    Ok(Json(json!({ "rules": merge_rules(&overrides) })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_rules))
        .route("/rules", get(list_rules))
        .route("/picker", get(picker))
        .route("/rules/{key}", patch(patch_rule).delete(delete_rule))
}
