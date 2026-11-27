use crate::error::{AppError, AppResult};
use crate::security::decode_access_token;
use crate::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum_extra::headers::authorization::Bearer;
use axum_extra::headers::Authorization;
use axum_extra::TypedHeader;

#[derive(Clone, Debug)]
pub struct CurrentUser {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub permissions: Vec<String>,
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let TypedHeader(Authorization(bearer)) =
            TypedHeader::<Authorization<Bearer>>::from_request_parts(parts, state)
                .await
                .map_err(|_| AppError::unauthorized("AUTH_REQUIRED", "未登录"))?;

        let claims = decode_access_token(&state.settings, bearer.token())?;
        let user_id: i64 = claims
            .sub
            .parse()
            .map_err(|_| AppError::unauthorized("AUTH_INVALID_TOKEN", "无效令牌"))?;

        let row = sqlx::query_as::<_, (i64, String, String, i64)>(
            "SELECT id, username, email, is_active FROM users WHERE id = ?",
        )
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?;

        let Some((id, username, email, is_active)) = row else {
            return Err(AppError::unauthorized("AUTH_INVALID_USER", "账号不可用"));
        };
        if is_active == 0 {
            return Err(AppError::unauthorized("AUTH_INVALID_USER", "账号不可用"));
        }

        Ok(CurrentUser {
            id,
            username,
            email,
            permissions: claims.perms,
        })
    }
}

pub async fn load_user_roles_permissions(
    db: &sqlx::SqlitePool,
    user_id: i64,
) -> AppResult<(Vec<String>, Vec<String>)> {
    let roles: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT r.name
        FROM roles r
        JOIN user_roles ur ON ur.role_id = r.id
        WHERE ur.user_id = ?
        ORDER BY r.name
        "#,
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;

    let permissions: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT DISTINCT p.code
        FROM permissions p
        JOIN role_permissions rp ON rp.permission_id = p.id
        JOIN user_roles ur ON ur.role_id = rp.role_id
        WHERE ur.user_id = ?
        ORDER BY p.code
        "#,
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;

    Ok((roles, permissions))
}

pub fn require_perm(user: &CurrentUser, code: &str) -> AppResult<()> {
    if user.permissions.iter().any(|p| p == code) {
        Ok(())
    } else {
        Err(AppError::forbidden("FORBIDDEN", format!("缺少权限: {code}")))
    }
}
