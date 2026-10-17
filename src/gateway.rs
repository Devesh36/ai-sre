//! Signed/secret-authenticated message intake; incoming events never execute actions or send replies.
use crate::{
    api::{ApiError, ApiResult, App},
    config::check_env,
    domain::Incident,
    tools::Toolbox,
};
use anyhow::{Result, ensure};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    routing::post,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;
use subtle::ConstantTimeEq;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Gateways {
    pub slack: Option<Slack>,
    pub telegram: Option<Telegram>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slack {
    pub signing_secret_env: String,
    pub allowed_channels: Vec<String>,
    pub allowed_users: Vec<String>,
    pub service: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Telegram {
    pub secret_env: String,
    pub allowed_chats: Vec<i64>,
    pub allowed_users: Vec<i64>,
    pub service: String,
}
impl Gateways {
    pub fn validate(&self) -> Result<()> {
        if let Some(s) = &self.slack {
            check_env(&Some(s.signing_secret_env.clone()))?;
            ensure!(
                !s.allowed_channels.is_empty() && !s.allowed_users.is_empty(),
                "Slack requires channel and user allowlists"
            );
            Incident::new("gateway validation".into(), s.service.clone())?;
        }
        if let Some(s) = &self.telegram {
            check_env(&Some(s.secret_env.clone()))?;
            ensure!(
                !s.allowed_chats.is_empty() && !s.allowed_users.is_empty(),
                "Telegram requires chat and user allowlists"
            );
            Incident::new("gateway validation".into(), s.service.clone())?;
        }
        Ok(())
    }
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/webhooks/slack", post(slack))
        .route("/api/webhooks/telegram", post(telegram))
}

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
fn header<'a>(headers: &'a HeaderMap, name: &str) -> &'a str {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}
fn denied() -> ApiError {
    ApiError(
        StatusCode::UNAUTHORIZED,
        "webhook authentication failed".into(),
    )
}

async fn slack(State(app): State<App>, request: Request) -> ApiResult<Json<Value>> {
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

async fn telegram(
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
fn ensure_allowed(allowed: bool) -> ApiResult<()> {
    if allowed { Ok(()) } else { Err(denied()) }
}
fn receive(app: &App, key: &str, text: &str, service: &str) -> ApiResult<Json<Value>> {
    let tools = Toolbox::new(app.config.clone())?;
    let mut incident = Incident::new(tools.redact(text), service.into())
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    incident.event(
        "message_received",
        "Authenticated channel message received; awaiting operator investigation",
    );
    let (incident, created) = app.store.insert(&incident, Some(key))?;
    Ok(Json(json!({"incident_id":incident.id,"created":created})))
}
