use super::auth::{denied, ensure_allowed, header};
use super::receive;
use crate::api::{ApiError, ApiResult, App};
use anyhow::{Result, ensure};
use axum::{
    Json,
    body::to_bytes,
    extract::{Request, State},
    http::StatusCode,
};
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
pub fn verify_slack(
    secret: &str,
    timestamp: &str,
    signature: &str,
    body: &[u8],
    now: i64,
) -> Result<()> {
    let stamp = timestamp.parse::<i64>()?;
    ensure!(
        (now as i128 - stamp as i128).abs() <= 300,
        "expired webhook timestamp"
    );
    let hex = signature
        .strip_prefix("v0=")
        .ok_or_else(|| anyhow::anyhow!("invalid signature version"))?;
    ensure!(
        hex.len() == 64 && hex.is_ascii(),
        "invalid signature encoding"
    );
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())?;
    mac.update(format!("v0:{timestamp}:").as_bytes());
    mac.update(body);
    mac.verify_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("invalid webhook signature"))?;
    Ok(())
}
pub(crate) async fn slack(State(app): State<App>, request: Request) -> ApiResult<Json<Value>> {
    let config = app.config.gateways.slack.as_ref().ok_or_else(denied)?;
    let timestamp = header(request.headers(), "x-slack-request-timestamp").to_owned();
    let signature = header(request.headers(), "x-slack-signature").to_owned();
    let body = to_bytes(request.into_body(), 65536).await.map_err(|_| {
        ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "webhook body too large".into(),
        )
    })?;
    let secret = app
        .slack_secret
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(denied)?;
    verify_slack(
        secret,
        &timestamp,
        &signature,
        &body,
        chrono::Utc::now().timestamp(),
    )
    .map_err(|_| denied())?;
    let value: Value = serde_json::from_slice(&body)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid JSON".into()))?;
    if value["type"] == "url_verification" {
        return Ok(Json(json!({"challenge":value["challenge"]})));
    }
    let event = &value["event"];
    if !matches!(event["type"].as_str(), Some("app_mention" | "message"))
        || event.get("bot_id").is_some()
        || event.get("subtype").is_some()
    {
        return Ok(Json(json!({"ignored":true})));
    }
    ensure_allowed(
        config
            .allowed_channels
            .iter()
            .any(|s| Some(s.as_str()) == event["channel"].as_str())
            && config
                .allowed_users
                .iter()
                .any(|s| Some(s.as_str()) == event["user"].as_str()),
    )?;
    let id = value["event_id"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, "event_id required".into()))?;
    receive(
        &app,
        &format!("slack:{id}"),
        event["text"].as_str().unwrap_or(""),
        &config.service,
    )
}
