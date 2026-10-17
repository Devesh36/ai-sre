//! Local deterministic incident laboratory. All telemetry is synthetic and explicitly labeled.
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Scenario {
    Healthy,
    Memory,
    Dependency,
    RateLimit,
    Disk,
}
#[derive(Clone)]
pub struct Lab(Arc<RwLock<Scenario>>);
impl Lab {
    pub fn new(scenario: Scenario) -> Self {
        Self(Arc::new(RwLock::new(scenario)))
    }
}
fn signal(s: Scenario) -> &'static str {
    match s {
        Scenario::Healthy => "Request completed successfully",
        Scenario::Memory => "Container OOMKilled after memory limit exceeded",
        Scenario::Dependency => "Inventory connection refused",
        Scenario::RateLimit => "Gateway: too many requests",
        Scenario::Disk => "Writer: no space left on device",
    }
}

pub fn router(state: Lab) -> Router {
    Router::new()
        .route("/lab/state",get(|State(lab):State<Lab>|async move {let s=*lab.0.read().expect("lab lock");Json(json!({"synthetic":true,"scenario":s,"status":if matches!(s,Scenario::Healthy){"healthy"}else{"degraded"}}))}))
        .route("/lab/scenario",post(|State(lab):State<Lab>,Json(input):Json<Change>|async move{*lab.0.write().expect("lab lock")=input.scenario;StatusCode::NO_CONTENT}))
        .route("/api/v1/query",get(|State(lab):State<Lab>,Query(query):Query<HashMap<String,String>>|async move{
            let s=*lab.0.read().expect("lab lock");let value=if query.get("query").is_some_and(|q|q.contains("error"))&&!matches!(s,Scenario::Healthy){"0.18"}else{"1"};
            Json(json!({"status":"success","data":{"resultType":"vector","result":[{"metric":{"job":"checkout-api","synthetic":"true"},"value":[chrono::Utc::now().timestamp(),value]}]}}))
        }))
        .route("/loki/api/v1/query_range",get(|State(lab):State<Lab>|async move{let s=*lab.0.read().expect("lab lock");Json(json!({"status":"success","data":{"resultType":"streams","result":[{"stream":{"service_name":"checkout-api","synthetic":"true"},"values":[[chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0).to_string(),signal(s)]]}]}}))}))
        .route("/api/v1/namespaces/default/pods",get(|State(lab):State<Lab>|async move{let s=*lab.0.read().expect("lab lock");Json(json!({"kind":"PodList","metadata":{"synthetic":true},"items":[{"metadata":{"name":"checkout-api-lab"},"status":{"phase":if matches!(s,Scenario::Memory){"Failed"}else{"Running"},"message":signal(s)}}]}))}))
        .route("/api/v1/namespaces/default/events",get(|State(lab):State<Lab>|async move{let s=*lab.0.read().expect("lab lock");Json(json!({"kind":"EventList","metadata":{"synthetic":true},"items":[{"reason":"LabObservation","message":signal(s)}]}))}))
        .with_state(state)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    scenario: Scenario,
}
