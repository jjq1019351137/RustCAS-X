use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{message}")]
    Api {
        code: &'static str,
        message: String,
        status: StatusCode,
        detail: Option<String>,
    },
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
}

#[derive(Serialize)]
struct ErrorBody {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

impl AppError {
    pub fn api(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self::Api {
            code,
            message: message.into(),
            status,
            detail: None,
        }
    }

    pub fn unauthorized(code: &'static str, message: impl Into<String>) -> Self {
        Self::api(StatusCode::UNAUTHORIZED, code, message)
    }

    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::api(StatusCode::BAD_REQUEST, code, message)
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::api(StatusCode::CONFLICT, code, message)
    }

    pub fn not_found(code: &'static str, message: impl Into<String>) -> Self {
        Self::api(StatusCode::NOT_FOUND, code, message)
    }

    pub fn forbidden(code: &'static str, message: impl Into<String>) -> Self {
        Self::api(StatusCode::FORBIDDEN, code, message)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message, detail) = match &self {
            AppError::Api {
                code,
                message,
                status,
                detail,
            } => (*status, (*code).to_string(), message.clone(), detail.clone()),
            AppError::Anyhow(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL".into(),
                err.to_string(),
                None,
            ),
            AppError::Sqlx(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "DB_ERROR".into(),
                err.to_string(),
                None,
            ),
        };

        (
            status,
            Json(ErrorBody {
                code,
                message,
                detail,
            }),
        )
            .into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
