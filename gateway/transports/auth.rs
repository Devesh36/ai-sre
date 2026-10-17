use crate::api::{ApiError, ApiResult};
use axum::http::{HeaderMap, StatusCode};
pub(crate) fn header<'a>(headers: &'a HeaderMap, name: &str) -> &'a str {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}
pub(crate) fn denied() -> ApiError {
    ApiError(
        StatusCode::UNAUTHORIZED,
        "webhook authentication failed".into(),
    )
}

pub(crate) fn ensure_allowed(allowed: bool) -> ApiResult<()> {
    if allowed { Ok(()) } else { Err(denied()) }
}
