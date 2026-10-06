use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Upstream(String),
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub fn unauthorized() -> Self {
        Self::Unauthorized("Unauthorized".into())
    }
}

#[derive(Serialize)]
struct ErrorBody {
    ok: bool,
    error: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            AppError::Forbidden(_) => StatusCode::FORBIDDEN,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::Upstream(_) => StatusCode::BAD_GATEWAY,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (
            status,
            Json(ErrorBody {
                ok: false,
                error: self.to_string(),
            }),
        )
            .into_response()
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        AppError::Internal(crate::google::sheets::sanitize_google_user_error(&err.to_string()))
    }
}

impl From<reqwest::Error> for AppError {
    fn from(err: reqwest::Error) -> Self {
        AppError::Upstream(crate::google::sheets::sanitize_google_user_error(&err.to_string()))
    }
}

pub fn booking_error(err: anyhow::Error) -> AppError {
    // The sanitized text drops the cause, and any message mentioning
    // sheets.googleapis.com becomes a generic "busy" notice — keep the original
    // server-side or a real failure is indistinguishable from a quota blip.
    tracing::warn!("[booking] raw error: {err:?}");
    let message = crate::google::sheets::sanitize_google_user_error(&err.to_string());
    if message == "Unauthorized" {
        return AppError::unauthorized();
    }
    let conflict = regex::Regex::new(r"(?i)already being booked|no longer (?:available|reserved)|hold expired|just taken").unwrap();
    if conflict.is_match(&message) {
        return AppError::Conflict(message);
    }
    let re = regex::Regex::new(r"(?i)required|too large|Select a valid|POC is missing").unwrap();
    if re.is_match(&message) {
        AppError::BadRequest(message)
    } else {
        AppError::Upstream(message)
    }
}
