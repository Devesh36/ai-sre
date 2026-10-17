use crate::{
    api::{ApiResult, App},
    config::Workflow,
    core::agent_harness::skills::Bundle,
    infrastructure::skills_registry::Registry,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/skills", get(list).post(publish))
        .route("/api/skills/{name}/release", get(release))
        .route("/api/skills/{name}/history", get(history))
        .route("/api/skills/{name}/activate", post(activate))
        .route("/api/skills/{name}/rollback", post(rollback))
}
async fn list(State(app): State<App>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(Registry::new(app.store)?.list()?)))
}
async fn publish(State(app): State<App>, Json(workflow): Json<Workflow>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(Registry::new(app.store)?.publish(workflow)?)))
}
async fn history(State(app): State<App>, Path(name): Path<String>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(Registry::new(app.store)?.history(&name)?)))
}
async fn rollback(State(app): State<App>, Path(name): Path<String>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(Registry::new(app.store)?.rollback(&name)?)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Activation {
    digest: String,
}
async fn activate(
    State(app): State<App>,
    Path(name): Path<String>,
    Json(input): Json<Activation>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!(
        Registry::new(app.store)?.activate(&name, &input.digest)?
    )))
}
async fn release(
    State(app): State<App>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let release = Registry::new(app.store)?.active(&name)?;
    let etag = format!("\"{}\"", release.digest);
    if headers.get("if-none-match").and_then(|h| h.to_str().ok()) == Some(etag.as_str()) {
        return Ok((StatusCode::NOT_MODIFIED, [("etag", etag)]).into_response());
    }
    Ok(([("etag", etag)], Json(Bundle::from(release))).into_response())
}
