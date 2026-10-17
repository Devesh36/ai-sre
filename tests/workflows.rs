use ai_sre::{
    agent,
    api::{self, App},
    config::{Config, Endpoint, Model},
    domain::{Evidence, Incident, Status},
    store::Store,
    tools::{Toolbox, redact},
};
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::State,
    http::{Request, StatusCode},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use tower::ServiceExt;

fn setup() -> (TempDir, Config, Store) {
    let dir = TempDir::new().unwrap();
    let config = Config {
        database: dir.path().join("incidents.db"),
        ..Config::default()
    };
    let store = Store::open(&config.database).unwrap();
    (dir, config, store)
}
fn incident() -> Incident {
    Incident::new("Investigate checkout failure".into(), "checkout-api".into()).unwrap()
}
fn request(method: &str, path: &str, body: Value, token: Option<&str>) -> Request<Body> {
    let mut r = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost")
        .header("content-type", "application/json");
    if let Some(token) = token {
        r = r.header("authorization", format!("Bearer {token}"));
    }
    r.body(Body::from(body.to_string())).unwrap()
}
async fn json_body(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap()
}

#[test]
fn sqlite_dedup_claim_and_restart_preserve_history() {
    let (_dir, config, store) = setup();
    let item = incident();
    assert!(store.insert(&item, Some("alert:1")).unwrap().1);
    let (duplicate, created) = store.insert(&incident(), Some("alert:1")).unwrap();
    assert!(!created);
    assert_eq!(duplicate.id, item.id);
    let another_connection = Store::open(&config.database).unwrap();
    store.claim(item.id, Some("first question")).unwrap();
    assert!(
        another_connection
            .claim(item.id, Some("racing question"))
            .is_err()
    );
    let mut claimed = another_connection.get(item.id).unwrap().unwrap();
    claimed.status = Status::Triaged;
    claimed.event("completed", "saved");
    store.finish(&claimed).unwrap();
    drop(store);
    let reopened = Store::open(&config.database).unwrap();
    assert_eq!(
        reopened
            .get(item.id)
            .unwrap()
            .unwrap()
            .events
            .last()
            .unwrap()
            .message,
        "saved"
    );
    assert!(
        reopened
            .mutate(item.id, |i| {
                i.title = "bad update".into();
                anyhow::bail!("rollback")
            })
            .is_err()
    );
    assert_eq!(reopened.get(item.id).unwrap().unwrap().title, item.title);
}

#[tokio::test]
async fn api_auth_origin_and_host_boundaries() {
    let (_dir, config, store) = setup();
    let app = api::router(App::new(
        config.clone(),
        store.clone(),
        Some("test-token".into()),
    ));
    assert_eq!(
        app.clone()
            .oneshot(request("GET", "/api/incidents", json!({}), None))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.clone()
            .oneshot(request("GET", "/api/incidents", json!({}), Some("wrong")))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let mut hostile = request(
        "POST",
        "/api/incidents",
        json!({"title":"x","service":"x"}),
        Some("test-token"),
    );
    hostile
        .headers_mut()
        .insert("origin", "https://attacker.example".parse().unwrap());
    assert_eq!(
        app.clone().oneshot(hostile).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.oneshot(request(
            "GET",
            "/api/incidents",
            json!({}),
            Some("test-token")
        ))
        .await
        .unwrap()
        .status(),
        StatusCode::OK
    );
    let local = api::router(App::new(config, store, None));
    let mut rebound = request("GET", "/api/incidents", json!({}), None);
    rebound
        .headers_mut()
        .insert("host", "attacker.example".parse().unwrap());
    assert_eq!(
        local.clone().oneshot(rebound).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let large = request(
        "POST",
        "/api/incidents",
        json!({"title":"x".repeat(70_000),"service":"x"}),
        None,
    );
    assert_eq!(
        local.oneshot(large).await.unwrap().status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[tokio::test]
async fn alert_replay_is_idempotent_but_new_occurrences_are_distinct() {
    let (_dir, config, store) = setup();
    let app = api::router(App::new(config, store.clone(), None));
    let alert = json!({"status":"firing","labels":{"service":"checkout-api","alertname":"HighErrorRate"},"annotations":{"summary":"Checkout errors"},"fingerprint":"abc123","startsAt":"2026-01-01T00:00:00Z"});
    let body = json!({"alerts":[alert.clone()]});
    let first = json_body(
        app.clone()
            .oneshot(request(
                "POST",
                "/api/webhooks/alertmanager",
                body.clone(),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    let second = json_body(
        app.clone()
            .oneshot(request("POST", "/api/webhooks/alertmanager", body, None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(first["incidents"][0]["id"], second["incidents"][0]["id"]);
    assert_eq!(second["incidents"][0]["created"], false);
    let mut new_alert = alert.clone();
    new_alert["startsAt"] = json!("2026-01-02T00:00:00Z");
    app.clone()
        .oneshot(request(
            "POST",
            "/api/webhooks/alertmanager",
            json!({"alerts":[new_alert]}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(store.list().unwrap().len(), 2);
    let mut resolved = alert.clone();
    resolved["status"] = json!("resolved");
    app.clone()
        .oneshot(request(
            "POST",
            "/api/webhooks/alertmanager",
            json!({"alerts":[resolved]}),
            None,
        ))
        .await
        .unwrap();
    assert!(
        store
            .list()
            .unwrap()
            .iter()
            .all(|i| i.status == Status::Open)
    );
    let mut valid = alert.clone();
    valid["startsAt"] = json!("2026-01-03T00:00:00Z");
    let mut invalid = alert;
    invalid["startsAt"] = json!("bad");
    let response = app
        .oneshot(request(
            "POST",
            "/api/webhooks/alertmanager",
            json!({"alerts":[valid,invalid]}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(store.list().unwrap().len(), 2);
}

#[tokio::test]
async fn offline_investigation_and_followup_have_cited_evidence() {
    let (dir, mut config, store) = setup();
    let file = dir.path().join("evidence.json");
    std::fs::write(&file, include_str!("../examples/evidence.json")).unwrap();
    config.evidence_file = Some(file);
    let item = store.insert(&incident(), None).unwrap().0;
    let first = agent::investigate(&config, &store, store.claim(item.id, None).unwrap())
        .await
        .unwrap();
    assert_eq!(first.status, Status::Triaged);
    assert_eq!(first.report.as_ref().unwrap().findings.len(), 2);
    agent::validate_report(first.report.as_ref().unwrap(), &first.evidence).unwrap();
    let second = agent::investigate(
        &config,
        &store,
        store.claim(item.id, Some("Check again")).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(second.evidence.len(), 6);
    assert_eq!(second.usage.tool_calls, 2);
    assert_eq!(second.usage.model_calls, 0);
    store
        .mutate(item.id, |i| {
            i.status = Status::Resolved;
            Ok(())
        })
        .unwrap();
    assert!(store.claim(item.id, None).is_err());
}

#[test]
fn offline_abstains_and_does_not_treat_runbook_instructions_as_observations() {
    let evidence = Evidence {
        id: "E1".into(),
        source: "runbook".into(),
        observed_at: chrono::Utc::now(),
        summary: "If OOMKilled occurs, inspect memory".into(),
        data: json!({}),
    };
    assert!(agent::offline_report(&[evidence]).findings.is_empty());
    assert!(agent::offline_report(&[]).findings.is_empty());
}

#[tokio::test]
async fn redaction_and_tool_allowlist_apply_before_storage() {
    let (dir, mut config, _) = setup();
    let file = dir.path().join("evidence.json");
    std::fs::write(&file, json!([{"service":"checkout-api","source":"logs","summary":"password=hunter2 failure","data":{"authorization":"Bearer verysecret","nested":[{"api_key":"hidden"}],"message":"token=abcdef"}}]).to_string()).unwrap();
    config.evidence_file = Some(file);
    let tools = Toolbox::new(config).unwrap();
    let items = tools
        .call("local_evidence", json!({}), "checkout-api")
        .await
        .unwrap();
    let text = serde_json::to_string(&items).unwrap();
    for secret in ["hunter2", "verysecret", "hidden", "abcdef"] {
        assert!(!text.contains(secret));
    }
    assert!(
        tools
            .call("shell", json!({"command":"id"}), "checkout-api")
            .await
            .is_err()
    );
    assert!(
        tools
            .call(
                "local_evidence",
                json!({"path":"/etc/passwd"}),
                "checkout-api"
            )
            .await
            .is_err()
    );
    assert_eq!(
        redact("Authorization: Bearer abcde"),
        "Authorization: [REDACTED]"
    );
}

#[test]
fn endpoint_policy_blocks_cleartext_remote_and_url_credentials() {
    for url in [
        "http://remote.example",
        "https://user:pass@example.com",
        "https://example.com?token=secret",
        "file:///etc/passwd",
    ] {
        assert!(ai_sre::config::validate_url(url).is_err());
    }
    for url in [
        "https://example.com/api",
        "http://127.0.0.1:11434/v1/chat/completions",
    ] {
        ai_sre::config::validate_url(url).unwrap();
    }
}

#[derive(Clone)]
struct Mock {
    requests: Arc<Mutex<Vec<Value>>>,
    mode: &'static str,
}
async fn model(State(mock): State<Mock>, Json(body): Json<Value>) -> Json<Value> {
    let mut requests = mock.requests.lock().unwrap();
    requests.push(body);
    let count = requests.len();
    let message = if mock.mode == "bad_citation" {
        json!({"role":"assistant","content":json!({"summary":"A finding","findings":[{"title":"Bad","explanation":"Invented","confidence":"high","evidence_ids":["E999"]}],"next_steps":[],"gaps":[]}).to_string()})
    } else if mock.mode == "loop" || count == 1 {
        json!({"role":"assistant","content":null,"tool_calls":[{"id":format!("call_{count}"),"type":"function","function":{"name":"prometheus_query","arguments":"{\"query\":\"vector(1)\"}"}}]})
    } else {
        json!({"role":"assistant","content":json!({"summary":"Reviewed the collected metric","findings":[{"title":"Metric observation","explanation":"The query returned one result; this does not establish root cause.","confidence":"low","evidence_ids":["E2"]}],"next_steps":["Inspect service logs"],"gaps":["No logs supplied"]}).to_string()})
    };
    Json(
        json!({"choices":[{"message":message}],"usage":{"prompt_tokens":10,"completion_tokens":5}}),
    )
}
async fn mock_server(
    mode: &'static str,
) -> (String, Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    let requests = Arc::new(Mutex::new(vec![]));
    let app = Router::new().route("/chat", post(model)).route("/api/v1/query", get(|| async { Json(json!({"status":"success","data":{"resultType":"vector","result":[{"metric":{},"value":[1,"1"]}]}})) }))
        .with_state(Mock { requests: requests.clone(), mode });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (base, requests, handle)
}

#[tokio::test]
async fn model_executes_read_only_tool_loop_and_accounts_for_usage() {
    let (_dir, mut config, store) = setup();
    let (base, requests, handle) = mock_server("success").await;
    config.prometheus = Some(Endpoint {
        url: base.clone(),
        token_env: None,
    });
    config.model = Some(Model {
        url: format!("{base}/chat"),
        name: "test".into(),
        key_env: None,
        protocol: Default::default(),
        input_price_per_million: 0.0,
        output_price_per_million: 0.0,
    });
    let item = store.insert(&incident(), None).unwrap().0;
    let result = agent::investigate(&config, &store, store.claim(item.id, None).unwrap())
        .await
        .unwrap();
    assert_eq!(result.status, Status::Triaged);
    assert_eq!(result.usage.model_calls, 2);
    assert_eq!(result.usage.input_tokens, 20);
    assert_eq!(result.evidence.len(), 2);
    assert_eq!(requests.lock().unwrap()[1]["messages"][3]["role"], "tool");
    handle.abort();
}

#[tokio::test]
async fn invalid_model_citations_and_infinite_tool_loops_fail_durably() {
    for mode in ["bad_citation", "loop"] {
        let (_dir, mut config, store) = setup();
        let (base, requests, handle) = mock_server(mode).await;
        config.max_turns = 2;
        config.prometheus = Some(Endpoint {
            url: base.clone(),
            token_env: None,
        });
        config.model = Some(Model {
            url: format!("{base}/chat"),
            name: "test".into(),
            key_env: None,
            protocol: Default::default(),
            input_price_per_million: 0.0,
            output_price_per_million: 0.0,
        });
        let item = store.insert(&incident(), None).unwrap().0;
        let result = agent::investigate(&config, &store, store.claim(item.id, None).unwrap())
            .await
            .unwrap();
        assert_eq!(result.status, Status::Failed);
        assert_eq!(store.get(item.id).unwrap().unwrap().status, Status::Failed);
        assert!(result.report.is_none());
        assert!(requests.lock().unwrap().len() <= 2);
        handle.abort();
    }
}

#[tokio::test]
async fn api_runs_a_complete_incident_lifecycle() {
    let (_dir, config, store) = setup();
    let app = api::router(App::new(config, store.clone(), None));
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/incidents",
            json!({"title":"Why is checkout slow?","service":"checkout-api"}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = json_body(response).await;
    let id = created["id"].as_str().unwrap();
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/api/incidents/{id}/investigate"),
            json!({}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let uuid = uuid::Uuid::parse_str(id).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while store.get(uuid).unwrap().unwrap().status == Status::Investigating {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let response = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/api/incidents/{id}"),
            json!({}),
            None,
        ))
        .await
        .unwrap();
    let result = json_body(response).await;
    assert_eq!(result["status"], "triaged");
    assert_eq!(result["report"]["findings"].as_array().unwrap().len(), 0);
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/api/incidents/{id}/resolve"),
            json!({}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(json_body(response).await["status"], "resolved");
    let response = app
        .oneshot(request(
            "POST",
            &format!("/api/incidents/{id}/investigate"),
            json!({}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn loki_uses_a_real_one_hour_window_and_reports_empty_queries() {
    use axum::extract::Query;
    use std::collections::HashMap;
    let upstream = Router::new().route("/loki/api/v1/query_range", get(|Query(q): Query<HashMap<String, String>>| async move {
        let start = chrono::DateTime::parse_from_rfc3339(&q["start"]).unwrap();
        let end = chrono::DateTime::parse_from_rfc3339(&q["end"]).unwrap();
        assert_eq!((end - start).num_seconds(), 3600);
        assert!((chrono::Utc::now() - end.with_timezone(&chrono::Utc)).num_seconds().abs() < 5);
        assert_eq!(q["limit"], "50");
        let result = if q["query"] == "{service_name=\"empty\"}" { json!([]) }
            else { json!([{"stream":{"service_name":"checkout-api"},"values":[["1","connection refused"]]}]) };
        Json(json!({"status":"success","data":{"resultType":"streams","result":result}}))
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        axum::serve(listener, upstream).await.unwrap();
    });
    let config = Config {
        loki: Some(Endpoint {
            url,
            token_env: None,
        }),
        ..Config::default()
    };
    let tools = Toolbox::new(config).unwrap();
    assert_eq!(
        tools
            .call(
                "loki_query",
                json!({"query":"{service_name=\"checkout-api\"}"}),
                "checkout-api"
            )
            .await
            .unwrap()
            .len(),
        1
    );
    let error = tools
        .call(
            "loki_query",
            json!({"query":"{service_name=\"empty\"}"}),
            "empty",
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no matching"));
    handle.abort();
}
