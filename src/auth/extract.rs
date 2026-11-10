use axum::{
    Json,
    extract::FromRequestParts,
    http::{StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use serde_json::json;

use super::AuthError;
use crate::AppState;

/// The authenticated calling app: add it to a handler's arguments to
/// protect the route.
///
/// ```ignore
/// async fn count(Caller(sub): Caller) -> impl IntoResponse { ... }
/// ```
#[derive(Debug, Clone)]
pub struct Caller(pub String);

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let status = match self {
            AuthError::InvalidToken(_) => StatusCode::UNAUTHORIZED,
            AuthError::Forbidden(_) => StatusCode::FORBIDDEN,
            AuthError::KeysUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        // Say what was wrong in the log, not to the caller.
        tracing::warn!(reason = %self, "request refused");
        let message = match status {
            StatusCode::UNAUTHORIZED => "unauthorized",
            StatusCode::FORBIDDEN => "forbidden",
            _ => "service unavailable",
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

/// No verifier configured: every protected route is closed.
struct NotConfigured;

impl IntoResponse for NotConfigured {
    fn into_response(self) -> Response {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "authentication is not configured" })),
        )
            .into_response()
    }
}

impl FromRequestParts<AppState> for Caller {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let Some(verifier) = &state.verifier else {
            return Err(NotConfigured.into_response());
        };
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| AuthError::InvalidToken("no bearer token".into()).into_response())?;
        verifier
            .verify(token.trim())
            .await
            .map(Caller)
            .map_err(IntoResponse::into_response)
    }
}
