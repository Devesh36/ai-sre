use crate::{
    agent,
    config::Config,
    domain::{Incident, Status},
    store::Store,
    tools::Toolbox,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;
use uuid::Uuid;

#[derive(Clone)]
pub struct App {
    pub config: Config,
    pub store: Store,
    pub token: Option<String>,
    pub capacity: Arc<Semaphore>,
    pub(crate) slack_secret: Option<String>,
    pub(crate) telegram_secret: Option<String>,
}

impl App {
    pub fn new(config: Config, store: Store, token: Option<String>) -> Self {
        let capacity = Arc::new(Semaphore::new(config.max_concurrent));
        let slack_secret = config
            .gateways
            .slack
            .as_ref()
            .and_then(|s| std::env::var(&s.signing_secret_env).ok());
        let telegram_secret = config
            .gateways
            .telegram
            .as_ref()
            .and_then(|s| std::env::var(&s.secret_env).ok());
        Self {
            config,
            store,
            token,
            capacity,
            slack_secret,
            telegram_secret,
        }
    }

    /// Allows embedded hosts to supply webhook secrets from their own secure store.
    pub fn with_gateway_secrets(mut self, slack: Option<String>, telegram: Option<String>) -> Self {
        self.slack_secret = slack;
        self.telegram_secret = telegram;
        self
    }
}

pub fn router(app: App) -> Router {
    let api = Router::new()
        .route("/api/incidents", get(list).post(create))
        .route("/api/incidents/{id}", get(detail))
        .route("/api/incidents/{id}/investigate", post(investigate))
        .route("/api/incidents/{id}/resolve", post(resolve))
        .route("/api/integrations", get(integrations))
        .route("/api/webhooks/alertmanager", post(alertmanager))
        .merge(crate::ops_api::routes())
        .merge(super::skills::routes())
        .route_layer(middleware::from_fn_with_state(app.clone(), authorize));
    Router::new()
        .merge(api)
        .merge(crate::gateway::routes())
        .route(
            "/healthz",
            get(|| async { Json(json!({"status":"ok","version":env!("CARGO_PKG_VERSION")})) }),
        )
        .route(
            "/",
            get(|| async {
                Html(include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/web/index.html"
                )))
            }),
        )
        .route(
            "/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/web/app.js")),
                )
            }),
        )
        .route(
            "/style.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/web/style.css")),
                )
            }),
        )
        .route(
            "/ops.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/web/ops.js")),
                )
            }),
        )
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn(security_headers))
        .with_state(app)
}

async fn security_headers(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    for (key, value) in [
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
        ),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "no-referrer"),
        ("cache-control", "no-store"),
    ] {
        response.headers_mut().insert(
            axum::http::HeaderName::from_static(key),
            value.parse().expect("static header"),
        );
    }
    response
}

async fn authorize(
    State(app): State<App>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    if app.token.is_none() {
        let local_host = request
            .headers()
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .and_then(|h| reqwest::Url::parse(&format!("http://{h}")).ok())
            .is_some_and(|u| matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")));
        if !local_host {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"error":"unauthenticated API requires a loopback Host header"})),
            )
                .into_response();
        }
    }
    // Reject cross-origin browser calls, including requests to an unauthenticated loopback server.
    if let Some(origin) = request.headers().get(header::ORIGIN) {
        let same_origin = origin
            .to_str()
            .ok()
            .and_then(|o| reqwest::Url::parse(o).ok())
            .is_some_and(|o| {
                let authority = match o.port() {
                    Some(p) => format!("{}:{p}", o.host_str().unwrap_or("")),
                    None => o.host_str().unwrap_or("").to_string(),
                };
                request
                    .headers()
                    .get(header::HOST)
                    .and_then(|v| v.to_str().ok())
                    == Some(authority.as_str())
            });
        if !same_origin {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"error":"cross-origin requests are disabled"})),
            )
                .into_response();
        }
    }
    if let Some(expected) = &app.token {
        let supplied = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        if !bool::from(supplied.as_bytes().ct_eq(expected.as_bytes())) {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error":"a valid bearer token is required"})),
            )
                .into_response();
        }
    }
    next.run(request).await
}

pub struct ApiError(pub(crate) StatusCode, pub(crate) String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}
impl From<anyhow::Error> for ApiError {
    fn from(_: anyhow::Error) -> Self {
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "operation failed; inspect server configuration and local storage".into(),
        )
    }
}
pub(crate) type ApiResult<T> = Result<T, ApiError>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    pub title: String,
    pub service: String,
}

async fn create(
    State(app): State<App>,
    Json(input): Json<Create>,
) -> ApiResult<(StatusCode, Json<Incident>)> {
    let tools = Toolbox::new(app.config.clone())?;
    let incident = Incident::new(tools.redact(&input.title), tools.redact(&input.service))
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    let (incident, _) = app.store.insert(&incident, None)?;
    Ok((StatusCode::CREATED, Json(incident)))
}

async fn list(State(app): State<App>) -> ApiResult<Json<Vec<Incident>>> {
    Ok(Json(app.store.list()?))
}
async fn detail(State(app): State<App>, Path(id): Path<Uuid>) -> ApiResult<Json<Incident>> {
    Ok(Json(app.store.get(id)?.ok_or_else(|| {
        ApiError(StatusCode::NOT_FOUND, "incident not found".into())
    })?))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Followup {
    pub question: Option<String>,
}

async fn investigate(
    State(app): State<App>,
    Path(id): Path<Uuid>,
    Json(input): Json<Followup>,
) -> ApiResult<(StatusCode, Json<Incident>)> {
    if input
        .question
        .as_ref()
        .is_some_and(|q| q.trim().is_empty() || q.len() > 4000)
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "question must contain 1–4000 bytes".into(),
        ));
    }
    if app.store.get(id)?.is_none() {
        return Err(ApiError(StatusCode::NOT_FOUND, "incident not found".into()));
    }
    let permit = app.capacity.clone().try_acquire_owned().map_err(|_| {
        ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "all investigation slots are busy; retry later".into(),
        )
    })?;
    let tools = Toolbox::new(app.config.clone())?;
    let question = input.question.map(|q| tools.redact(&q));
    let incident = app
        .store
        .claim(id, question.as_deref())
        .map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?;
    let response = incident.clone();
    tokio::spawn(async move {
        let _permit = permit;
        if let Err(error) = agent::investigate(&app.config, &app.store, incident).await {
            eprintln!("Could not persist investigation {id}: {error}");
        }
    });
    Ok((StatusCode::ACCEPTED, Json(response)))
}

async fn resolve(State(app): State<App>, Path(id): Path<Uuid>) -> ApiResult<Json<Incident>> {
    if app.store.get(id)?.is_none() {
        return Err(ApiError(StatusCode::NOT_FOUND, "incident not found".into()));
    }
    let incident = app
        .store
        .mutate(id, |i| {
            anyhow::ensure!(
                i.status != Status::Investigating,
                "wait for the active investigation before resolving"
            );
            i.status = Status::Resolved;
            i.event("resolved", "Operator marked incident resolved");
            Ok(())
        })
        .map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(incident))
}

async fn integrations(State(app): State<App>) -> Json<Value> {
    Json(
        json!({"mode":if app.config.model.is_some() {"model"} else {"offline"},
        "connectors":[
            {"name":"Evidence bundle","configured":app.config.evidence_file.is_some()},
            {"name":"Runbooks","configured":app.config.runbooks.is_some()},
            {"name":"Prometheus","configured":app.config.prometheus.is_some()},
            {"name":"Loki","configured":app.config.loki.is_some()},
            {"name":"Chat completions model","configured":app.config.model.is_some()}
        ],"connections":app.config.connections.iter().map(|c|json!({"name":c.name,"kind":c.kind,"operations":c.definitions().len()})).collect::<Vec<_>>(),
        "mcp_servers":app.config.mcp.iter().map(|m|json!({"name":m.name,"allowed_tools":m.allowed_tools})).collect::<Vec<_>>(),
        "model_protocol":app.config.model.as_ref().map(|m|m.protocol),
        "max_turns":app.config.max_turns,"max_tool_calls":app.config.max_tool_calls}),
    )
}

#[derive(Deserialize)]
struct AlertEnvelope {
    alerts: Vec<Alert>,
}
#[derive(Deserialize)]
struct Alert {
    status: String,
    labels: BTreeMap<String, String>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
    fingerprint: String,
    #[serde(rename = "startsAt")]
    starts_at: String,
}

async fn alertmanager(
    State(app): State<App>,
    Json(input): Json<AlertEnvelope>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    if input.alerts.len() > 50 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "maximum 50 alerts per request".into(),
        ));
    }
    let tools = Toolbox::new(app.config.clone())?;
    let mut pending = vec![];
    // Validate the entire envelope before any writes. Resolved notifications are recorded as ignored:
    // an external alert clearing does not prove the incident has been resolved.
    let mut ignored = 0;
    for alert in input.alerts {
        if alert.status == "resolved" {
            ignored += 1;
            continue;
        }
        if alert.status != "firing" || alert.fingerprint.is_empty() || alert.fingerprint.len() > 256
        {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "invalid alert status or fingerprint".into(),
            ));
        }
        let started = chrono::DateTime::parse_from_rfc3339(&alert.starts_at).map_err(|_| {
            ApiError(
                StatusCode::BAD_REQUEST,
                "startsAt must be an RFC3339 timestamp".into(),
            )
        })?;
        let title = alert
            .annotations
            .get("summary")
            .or_else(|| alert.labels.get("alertname"))
            .cloned()
            .unwrap_or_else(|| "Incoming alert".into());
        let service = alert
            .labels
            .get("service")
            .or_else(|| alert.labels.get("job"))
            .cloned()
            .unwrap_or_else(|| "unknown".into());
        let mut incident = Incident::new(tools.redact(&title), tools.redact(&service))
            .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
        incident.event(
            "alert_received",
            "Firing alert received; investigation awaits operator request",
        );
        pending.push((
            incident,
            format!(
                "alertmanager:{}:{}",
                alert.fingerprint,
                started.timestamp_nanos_opt().unwrap_or(started.timestamp())
            ),
        ));
    }
    let mut results = vec![];
    for (incident, key) in pending {
        let (incident, created) = app.store.insert(&incident, Some(&key))?;
        results.push(json!({"id":incident.id,"created":created}));
    }
    Ok((
        StatusCode::OK,
        Json(json!({"incidents":results,"ignored_resolved":ignored})),
    ))
}

pub fn token_from_environment() -> anyhow::Result<Option<String>> {
    match std::env::var("SRE_API_TOKEN") {
        Ok(token) => {
            anyhow::ensure!(
                token.len() >= 24,
                "SRE_API_TOKEN must contain at least 24 bytes"
            );
            Ok(Some(token))
        }
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn bearer_headers(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Ok(value) = format!("Bearer {token}").parse() {
        headers.insert(header::AUTHORIZATION, value);
    }
    headers
}
