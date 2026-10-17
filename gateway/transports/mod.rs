use crate::{
    api::{ApiError, ApiResult, App},
    domain::Incident,
    tools::Toolbox,
};
use axum::{Json, Router, http::StatusCode, routing::post};
use serde_json::{Value, json};
mod auth;
pub mod slack;
pub mod telegram;
pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/webhooks/slack", post(slack::slack))
        .route("/api/webhooks/telegram", post(telegram::telegram))
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
