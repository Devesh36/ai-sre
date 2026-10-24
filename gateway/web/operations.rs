use crate::{
    api::{ApiError, ApiResult, App},
    domain::{Incident, Status},
    tools::Toolbox,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/overview", get(overview))
        .route("/api/incidents/{id}/cancel", post(cancel))
        .route("/api/incidents/{id}/compact", post(compact))
        .route("/api/incidents/{id}/review", get(review))
        .route("/api/incidents/{id}/handoff", get(handoff))
        .route("/api/incidents/{id}/events", get(events))
        .route("/api/incidents/{id}/deliver/{destination}", post(deliver))
        .route(
            "/api/catalog",
            get(|| async { Json(crate::integrations::catalog()) }),
        )
        .route("/api/work", get(work_list).post(work_add))
        .route("/api/work/{id}/complete", post(work_complete))
        .route("/api/memory", get(memory_list).post(memory_add))
        .route("/api/memory/{id}", delete(memory_remove))
        .route("/api/schedules", get(schedule_list).post(schedule_add))
        .route("/api/schedules/{id}", delete(schedule_remove))
        .route("/api/actions", get(action_list).post(action_propose))
        .route("/api/actions/{id}/review", post(action_review))
        .route("/api/actions/{id}/execute", post(action_execute))
        .route(
            "/api/fleet",
            get(|| async { Json(crate::operations::scan_fleet()) }),
        )
}

async fn review(
    State(app): State<App>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<crate::domain::review::Review>> {
    let incident = app
        .store
        .get(id)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "incident not found".into()))?;
    Ok(Json(crate::domain::review::analyze(
        &incident,
        chrono::Utc::now(),
    )))
}

async fn handoff(State(app): State<App>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let incident = app
        .store
        .get(id)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "incident not found".into()))?;
    let review = crate::domain::review::analyze(&incident, chrono::Utc::now());
    Ok(Json(
        json!({"markdown":crate::domain::review::handoff(&incident, &review)}),
    ))
}

#[derive(Deserialize)]
struct Page {
    #[serde(default = "page_limit")]
    limit: u32,
    #[serde(default)]
    offset: u32,
}

async fn action_list(State(app): State<App>) -> ApiResult<Json<Vec<Value>>> {
    Ok(Json(app.store.records("action")?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalInput {
    incident_id: Uuid,
    template: String,
    arguments: Value,
    rationale: String,
}
async fn action_propose(
    State(app): State<App>,
    Json(input): Json<ProposalInput>,
) -> ApiResult<Json<Value>> {
    let proposal = crate::actions::propose(
        &app.config,
        &app.store,
        input.incident_id,
        &input.template,
        input.arguments,
        &input.rationale,
    )
    .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(json!(proposal)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewInput {
    actor: String,
    approve: bool,
}
async fn action_review(
    State(app): State<App>,
    Path(id): Path<Uuid>,
    Json(input): Json<ReviewInput>,
) -> ApiResult<Json<Value>> {
    let tools = Toolbox::new(app.config.clone())?;
    Ok(Json(json!(
        crate::actions::review(&app.store, id, &tools.redact(&input.actor), input.approve)
            .map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?
    )))
}
async fn action_execute(State(app): State<App>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(
        crate::actions::execute(&app.config, &app.store, id)
            .await
            .map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?
    )))
}
fn page_limit() -> u32 {
    50
}
async fn overview(State(app): State<App>, Query(page): Query<Page>) -> ApiResult<Json<Vec<Value>>> {
    if !(1..=100).contains(&page.limit) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "limit must be 1–100".into(),
        ));
    }
    Ok(Json(app.store.summaries(page.limit, page.offset)?))
}
async fn cancel(State(app): State<App>, Path(id): Path<Uuid>) -> ApiResult<Json<Incident>> {
    Ok(Json(app.store.cancel(id).map_err(|e| {
        ApiError(StatusCode::CONFLICT, e.to_string())
    })?))
}
async fn compact(State(app): State<App>, Path(id): Path<Uuid>) -> ApiResult<Json<Incident>> {
    Ok(Json(app.store.compact(id).map_err(|e| {
        ApiError(StatusCode::CONFLICT, e.to_string())
    })?))
}
async fn events(
    State(app): State<App>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    use axum::response::sse::{Event, KeepAlive, Sse};
    if app.store.get(id)?.is_none() {
        return Err(ApiError(StatusCode::NOT_FOUND, "incident not found".into()));
    }
    let mut cursor = headers
        .get("last-event-id")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok())
        .map_or(0, |v| v.saturating_add(1));
    let stream = async_stream::stream! {
        loop {
            match app.store.get(id) {
                Ok(Some(incident))=>{
                    while cursor<incident.events.len() {
                        let item=&incident.events[cursor];
                        yield Ok::<Event,std::convert::Infallible>(Event::default().id(cursor.to_string()).event("activity").data(serde_json::to_string(item).unwrap_or_default()));
                        cursor+=1;
                    }
                    if incident.status!=Status::Investigating {yield Ok(Event::default().event("complete").data(json!({"status":incident.status}).to_string()));break;}
                }
                _=>{yield Ok(Event::default().event("error").data("storage unavailable"));break;}
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    };
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response())
}
async fn deliver(
    State(app): State<App>,
    Path((id, destination)): Path<(Uuid, String)>,
) -> ApiResult<Json<Value>> {
    let incident = app
        .store
        .get(id)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "incident not found".into()))?;
    if incident.status == Status::Investigating {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "wait for investigation to finish".into(),
        ));
    }
    let result = crate::operations::deliver(&app.config, &incident, &destination)
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?;
    app.store.mutate(id, |i| {
        i.event(
            "report_delivered",
            format!("Operator delivered report to {destination}"),
        );
        Ok(())
    })?;
    Ok(Json(result))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TextInput {
    service: String,
    text: String,
    #[serde(default)]
    key: String,
}
async fn work_list(State(app): State<App>) -> ApiResult<Json<Vec<Value>>> {
    Ok(Json(app.store.records("work")?))
}
async fn work_add(State(app): State<App>, Json(input): Json<TextInput>) -> ApiResult<Json<Value>> {
    let tools = Toolbox::new(app.config.clone())?;
    Incident::new(input.text.clone(), input.service.clone())
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    let item = crate::operations::WorkItem {
        id: Uuid::new_v4(),
        service: tools.redact(&input.service),
        text: tools.redact(&input.text),
        done: false,
        created_at: chrono::Utc::now(),
    };
    let value = serde_json::to_value(&item).map_err(anyhow::Error::from)?;
    app.store.record_put("work", &item.id.to_string(), &value)?;
    Ok(Json(value))
}
async fn work_complete(State(app): State<App>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    if app.store.record_get("work", &id.to_string())?.is_none() {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            "work item not found".into(),
        ));
    }
    let item = app.store.record_mutate("work", &id.to_string(), |v| {
        v["done"] = json!(true);
        Ok(())
    })?;
    Ok(Json(item))
}
async fn memory_list(State(app): State<App>) -> ApiResult<Json<Vec<Value>>> {
    Ok(Json(app.store.records("memory")?))
}
async fn memory_add(
    State(app): State<App>,
    Json(input): Json<TextInput>,
) -> ApiResult<Json<Value>> {
    Incident::new(input.text.clone(), input.service.clone())
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    if input.key.is_empty() || input.key.len() > 128 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "memory key must contain 1–128 bytes".into(),
        ));
    }
    let tools = Toolbox::new(app.config.clone())?;
    let item = crate::operations::Memory {
        id: Uuid::new_v4(),
        service: tools.redact(&input.service),
        key: tools.redact(&input.key),
        text: tools.redact(&input.text),
        updated_at: chrono::Utc::now(),
    };
    let value = serde_json::to_value(&item).map_err(anyhow::Error::from)?;
    app.store
        .record_put("memory", &item.id.to_string(), &value)?;
    Ok(Json(value))
}
async fn memory_remove(State(app): State<App>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({"removed":app.store.record_delete("memory",&id.to_string())?}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScheduleInput {
    expression: String,
    timezone: String,
    question: String,
    service: String,
}
async fn schedule_list(
    State(app): State<App>,
) -> ApiResult<Json<Vec<crate::operations::Schedule>>> {
    Ok(Json(app.store.schedules()?))
}
async fn schedule_add(
    State(app): State<App>,
    Json(input): Json<ScheduleInput>,
) -> ApiResult<Json<crate::operations::Schedule>> {
    let tools = Toolbox::new(app.config.clone())?;
    let schedule = crate::operations::Schedule::new(
        input.expression,
        input.timezone,
        tools.redact(&input.question),
        tools.redact(&input.service),
    )
    .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    app.store.schedule_add(&schedule)?;
    Ok(Json(schedule))
}
async fn schedule_remove(State(app): State<App>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({"removed":app.store.schedule_remove(id)?})))
}
