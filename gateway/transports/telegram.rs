use super::auth::{denied, ensure_allowed, header};
use super::receive;
use crate::api::{ApiError, ApiResult, App};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use serde_json::Value;
use subtle::ConstantTimeEq;
pub(crate) async fn telegram(
    State(app): State<App>,
    headers: HeaderMap,
    Json(value): Json<Value>,
) -> ApiResult<Json<Value>> {
    let config = app.config.gateways.telegram.as_ref().ok_or_else(denied)?;
    let secret = app
        .telegram_secret
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(denied)?;
    ensure_allowed(bool::from(
        header(&headers, "x-telegram-bot-api-secret-token")
            .as_bytes()
            .ct_eq(secret.as_bytes()),
    ))?;
    let message = &value["message"];
    ensure_allowed(
        message["from"]["is_bot"] != true
            && message["chat"]["id"]
                .as_i64()
                .is_some_and(|id| config.allowed_chats.contains(&id))
            && message["from"]["id"]
                .as_i64()
                .is_some_and(|id| config.allowed_users.contains(&id)),
    )?;
    let id = value["update_id"]
        .as_i64()
        .ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, "update_id required".into()))?;
    receive(
        &app,
        &format!("telegram:{id}"),
        message["text"].as_str().unwrap_or(""),
        &config.service,
    )
}
