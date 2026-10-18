use ai_sre::{
    actions, agent,
    api::{self, App},
    config::{Config, Model},
    domain::{Incident, Status},
    integrations::{self, Connection},
    mcp,
    operations::{self, Schedule},
    providers::Protocol,
    store::Store,
    tools::Toolbox,
};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    routing::{get, patch, post},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, AtomicUsize, Ordering},
    },
};
use tempfile::TempDir;
use tower::ServiceExt;

fn setup() -> (TempDir, Config, Store) {
    let dir = TempDir::new().unwrap();
    let config = Config {
        database: dir.path().join("state.db"),
        ..Config::default()
    };
    let store = Store::open(&config.database).unwrap();
    (dir, config, store)
}
fn item() -> Incident {
    Incident::new("Investigate checkout-api".into(), "checkout-api".into()).unwrap()
}
async fn serve(router: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (url, handle)
}
fn model(url: String, protocol: Protocol) -> Model {
    Model {
        url,
        name: "mock".into(),
        key_env: None,
        protocol,
        input_price_per_million: 2.0,
        output_price_per_million: 4.0,
    }
}

#[derive(Clone)]
struct ProviderState {
    protocol: Protocol,
    bodies: Arc<Mutex<Vec<Value>>>,
}
async fn provider(State(state): State<ProviderState>, Json(body): Json<Value>) -> Json<Value> {
    let mut bodies = state.bodies.lock().unwrap();
    bodies.push(body);
    let first = bodies.len() == 1;
    let report=json!({"summary":"Review __SRE_IDENTIFIER_0__","findings":[{"title":"Observed symptom","explanation":"An observed symptom needs verification","confidence":"low","evidence_ids":["E1"]}],"next_steps":["Check the service"],"gaps":[]}).to_string();
    Json(match state.protocol {
        Protocol::OpenAi => {
            json!({"choices":[{"message":if first {json!({"role":"assistant","content":null,"tool_calls":[{"id":"c1","type":"function","function":{"name":"memory_read","arguments":"{}"}}]})}else{json!({"role":"assistant","content":report})}}],"usage":{"prompt_tokens":100,"completion_tokens":50}})
        }
        Protocol::Anthropic => {
            json!({"content":if first {json!([{"type":"tool_use","id":"c1","name":"memory_read","input":{}}])}else{json!([{"type":"text","text":report}])},"usage":{"input_tokens":100,"output_tokens":50}})
        }
        Protocol::Gemini => {
            json!({"candidates":[{"content":{"parts":if first {json!([{"functionCall":{"name":"memory_read","args":{}},"thoughtSignature":"signature"}])}else{json!([{"text":report}])}}}],"usageMetadata":{"promptTokenCount":100,"candidatesTokenCount":50}})
        }
        Protocol::Responses => {
            json!({"output":if first {json!([{"type":"function_call","call_id":"c1","name":"memory_read","arguments":"{}"}])}else{json!([{"type":"message","content":[{"type":"output_text","text":report}]}])},"usage":{"input_tokens":100,"output_tokens":50}})
        }
    })
}

#[tokio::test]
async fn all_four_model_protocols_execute_tool_roundtrips_with_masking_and_costs() {
    for protocol in [
        Protocol::OpenAi,
        Protocol::Anthropic,
        Protocol::Gemini,
        Protocol::Responses,
    ] {
        let (dir, mut config, store) = setup();
        let path = dir.path().join("evidence.json");
        std::fs::write(&path, include_str!("../examples/evidence.json")).unwrap();
        config.evidence_file = Some(path);
        config.mask_identifiers = vec!["checkout-api".into()];
        let bodies = Arc::new(Mutex::new(vec![]));
        let (url, handle) = serve(Router::new().route("/model", post(provider)).with_state(
            ProviderState {
                protocol,
                bodies: bodies.clone(),
            },
        ))
        .await;
        config.model = Some(model(format!("{url}/model"), protocol));
        let incident = store.insert(&item(), None).unwrap().0;
        let result = agent::investigate(&config, &store, store.claim(incident.id, None).unwrap())
            .await
            .unwrap();
        assert_eq!(
            result.status,
            Status::Triaged,
            "{:?}: {:?}",
            protocol,
            result.events
        );
        assert_eq!(result.usage.model_calls, 2);
        assert_eq!(result.usage.input_tokens, 200);
        assert!((result.usage.estimated_cost_usd.unwrap() - 0.0008).abs() < 1e-9);
        assert_eq!(result.report.unwrap().summary, "Review checkout-api");
        let requests = bodies.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(
            !serde_json::to_string(&*requests)
                .unwrap()
                .contains("checkout-api")
        );
        match protocol {
            Protocol::Anthropic => {
                assert!(requests[1]["messages"].to_string().contains("tool_result"))
            }
            Protocol::Gemini => {
                assert!(
                    requests[1]["contents"]
                        .to_string()
                        .contains("functionResponse")
                );
                assert!(
                    requests[1]["contents"]
                        .to_string()
                        .contains("thoughtSignature")
                );
            }
            Protocol::Responses => assert!(
                requests[1]["input"]
                    .to_string()
                    .contains("function_call_output")
            ),
            Protocol::OpenAi => {
                assert!(requests[1]["messages"].to_string().contains("tool_call_id"))
            }
        }
        handle.abort();
    }
}

#[tokio::test]
async fn cancellation_retains_checkpoints_and_stale_workers_cannot_overwrite_new_runs() {
    let (dir, mut config, store) = setup();
    let path = dir.path().join("evidence.json");
    std::fs::write(&path, include_str!("../examples/evidence.json")).unwrap();
    config.evidence_file = Some(path);
    let started = Arc::new(tokio::sync::Notify::new());
    let marker = started.clone();
    let (url, handle) = serve(Router::new().route(
        "/model",
        post(move || {
            let marker = marker.clone();
            async move {
                marker.notify_one();
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                Json(json!({}))
            }
        }),
    ))
    .await;
    config.model = Some(model(format!("{url}/model"), Protocol::OpenAi));
    let incident = store.insert(&item(), None).unwrap().0;
    let claimed = store.claim(incident.id, None).unwrap();
    let stale = claimed.clone();
    let worker_store = store.clone();
    let worker = tokio::spawn(async move {
        agent::investigate(&config, &worker_store, claimed)
            .await
            .unwrap()
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), started.notified())
        .await
        .unwrap();
    assert_eq!(store.get(incident.id).unwrap().unwrap().evidence.len(), 3);
    store.cancel(incident.id).unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), worker)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.status, Status::Cancelled);
    let newer = store.claim(incident.id, Some("retry")).unwrap();
    assert_ne!(stale.run_id, newer.run_id);
    assert!(store.finish(&stale).is_err());
    assert!(store.checkpoint(&stale).is_err());
    handle.abort();
}

#[tokio::test]
async fn every_native_operation_stays_under_its_configured_prefix_and_enforces_arguments() {
    let requests = Arc::new(Mutex::new(vec![]));
    let captured = requests.clone();
    let (url, handle) = serve(Router::new().fallback(move |request: Request| {
        let captured = captured.clone();
        async move {
            let method = request.method().to_string();
            let path = request.uri().to_string();
            let bytes = axum::body::to_bytes(request.into_body(), 65536)
                .await
                .unwrap();
            captured.lock().unwrap().push((
                method,
                path,
                String::from_utf8(bytes.to_vec()).unwrap(),
            ));
            Json(json!({"items":[],"status":"ok"}))
        }
    }))
    .await;
    let client = Config::default().client().unwrap();
    for op in integrations::catalog() {
        let mut parameters = BTreeMap::new();
        for segment in op.path.split('/') {
            if let Some(key) = segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                parameters.insert(key.to_string(), "scoped/value".to_string());
            }
        }
        let connection = Connection {
            name: "test".into(),
            kind: op.kind.into(),
            url: format!("{url}/gateway"),
            token_env: None,
            header_env: BTreeMap::new(),
            parameters,
        };
        connection.validate().unwrap();
        let args = if op.search_body {
            json!({"body":{"query":{"match_all":{}},"size":10000}})
        } else {
            json!({})
        };
        connection.call(&client, op.name, &args).await.unwrap();
        assert!(
            connection
                .call(
                    &client,
                    op.name,
                    &json!({"url":"https://elsewhere.invalid"})
                )
                .await
                .is_err()
        );
    }
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), integrations::catalog().len());
    assert!(requests.iter().all(|(_, p, _)| p.starts_with("/gateway/")));
    assert!(
        requests
            .iter()
            .filter(|(m, _, _)| m == "POST")
            .all(|(_, _, body)| serde_json::from_str::<Value>(body).unwrap()["size"] == 100)
    );
    assert!(
        requests
            .iter()
            .any(|(_, p, _)| p.contains("scoped%2Fvalue"))
    );
    handle.abort();
}

#[derive(Clone)]
struct McpState {
    deny: bool,
    requests: Arc<Mutex<Vec<Value>>>,
}
async fn mcp_handler(
    State(state): State<McpState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    state.requests.lock().unwrap().push(body.clone());
    let method = body["method"].as_str().unwrap();
    if method != "initialize" {
        assert_eq!(headers["mcp-session-id"], "session-one");
    }
    if method == "notifications/initialized" {
        return StatusCode::ACCEPTED.into_response();
    }
    let result = match method {
        "initialize" => json!({"protocolVersion":"2025-03-26","capabilities":{"tools":{}}}),
        "tools/list" => {
            json!({"tools":[{"name":"inspect","description":"Inspect context","inputSchema":{"type":"object","properties":{}},"annotations":{"readOnlyHint":!state.deny}},{"name":"danger","inputSchema":{"type":"object"},"annotations":{"readOnlyHint":false}}]})
        }
        "tools/call" => {
            assert_eq!(body["params"]["name"], "inspect");
            json!({"content":[{"type":"text","text":"A bounded observation"}],"isError":false})
        }
        _ => panic!("unexpected RPC"),
    };
    let payload = json!({"jsonrpc":"2.0","id":body["id"],"result":result});
    if method == "tools/call" {
        (
            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
            format!(": heartbeat\n\nevent: message\ndata: {payload}\n\n"),
        )
            .into_response()
    } else {
        ([("mcp-session-id", "session-one")], Json(payload)).into_response()
    }
}

#[tokio::test]
async fn mcp_negotiates_sessions_decodes_sse_and_blocks_unapproved_or_mutating_tools() {
    for deny in [false, true] {
        let requests = Arc::new(Mutex::new(vec![]));
        let (url, handle) = serve(Router::new().route("/mcp", post(mcp_handler)).with_state(
            McpState {
                deny,
                requests: requests.clone(),
            },
        ))
        .await;
        let client = mcp::Client::new(
            mcp::Server {
                name: "observability".into(),
                url: format!("{url}/mcp"),
                token_env: None,
                allowed_tools: vec!["inspect".into()],
                ..Default::default()
            },
            Config::default().client().unwrap(),
        );
        let result = client.initialize().await;
        if deny {
            assert!(result.is_err());
            assert!(client.definitions().is_empty());
        } else {
            result.unwrap();
            assert_eq!(client.definitions().len(), 1);
            assert!(client.call("inspect", json!({})).await.unwrap()["content"].is_array());
        }
        assert!(client.call("danger", json!({})).await.is_err());
        assert!(
            !requests
                .lock()
                .unwrap()
                .iter()
                .any(|r| r["params"]["name"] == "danger")
        );
        handle.abort();
    }
}

#[tokio::test]
async fn schedules_enqueue_atomically_survive_restart_and_are_executed_by_the_worker() {
    let (_dir, config, store) = setup();
    let now = chrono::Utc::now();
    let mut schedule = Schedule::new(
        "0 * * * * * *".into(),
        "Asia/Kolkata".into(),
        "Check checkout".into(),
        "checkout-api".into(),
    )
    .unwrap();
    schedule.next_at = now - chrono::Duration::seconds(1);
    store.schedule_add(&schedule).unwrap();
    assert_eq!(store.enqueue_due(now).unwrap(), 1);
    let reopened = Store::open(&config.database).unwrap();
    assert_eq!(reopened.enqueue_due(now).unwrap(), 0);
    assert_eq!(reopened.queued().unwrap().len(), 1);
    let (stop, receiver) = tokio::sync::watch::channel(false);
    let worker = tokio::spawn(operations::scheduler(
        App::new(config, reopened.clone(), None),
        receiver,
    ));
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if reopened.list().unwrap()[0].status == Status::Triaged {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    stop.send(true).unwrap();
    worker.await.unwrap();
    assert_eq!(reopened.list().unwrap().len(), 1);
    assert_eq!(reopened.list().unwrap()[0].schedule_id, Some(schedule.id));
    assert!(reopened.schedules().unwrap()[0].next_at > now);
}

#[tokio::test]
async fn memory_is_service_scoped_and_is_not_treated_as_an_observed_failure() {
    let (_dir, config, store) = setup();
    store.record_put("memory","one",&json!({"service":"checkout-api","key":"procedure","text":"If OOMKilled occurs, inspect memory"})).unwrap();
    store
        .record_put(
            "memory",
            "two",
            &json!({"service":"billing","key":"private","text":"different service"}),
        )
        .unwrap();
    let incident = store.insert(&item(), None).unwrap().0;
    let result = agent::investigate(&config, &store, store.claim(incident.id, None).unwrap())
        .await
        .unwrap();
    assert_eq!(result.evidence.len(), 1);
    assert!(result.report.unwrap().findings.is_empty());
    assert!(
        !serde_json::to_string(&result.evidence)
            .unwrap()
            .contains("different service")
    );
}

#[derive(Clone)]
struct Scale {
    replicas: Arc<AtomicI64>,
    calls: Arc<AtomicUsize>,
}
fn template(url: &str) -> actions::Template {
    actions::Template {
        name: "scale".into(),
        description: "Scale a fixed test service".into(),
        url: format!("{url}/scale"),
        method: actions::Method::Patch,
        token_env: None,
        body: json!({"replicas":"${replicas}"}),
        arguments: BTreeMap::from([(
            "replicas".into(),
            actions::Argument {
                kind: actions::ArgumentKind::Integer,
                min: Some(1),
                max: Some(5),
            },
        )]),
        precondition: actions::Check {
            url: format!("{url}/state"),
            pointer: "/replicas".into(),
            equals: json!(1),
        },
        verification: actions::Check {
            url: format!("{url}/state"),
            pointer: "/replicas".into(),
            equals: json!(3),
        },
        rollback: "Restore the previous replica count after checking service health".into(),
    }
}

#[tokio::test]
async fn actions_require_review_preserve_approved_bytes_and_execute_at_most_once() {
    let (_dir, mut config, store) = setup();
    let scale = Scale {
        replicas: Arc::new(AtomicI64::new(1)),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let (url, handle) = serve(
        Router::new()
            .route(
                "/state",
                get(|State(s): State<Scale>| async move {
                    Json(json!({"replicas":s.replicas.load(Ordering::SeqCst)}))
                }),
            )
            .route(
                "/scale",
                patch(
                    |State(s): State<Scale>, Json(body): Json<Value>| async move {
                        s.calls.fetch_add(1, Ordering::SeqCst);
                        s.replicas
                            .store(body["replicas"].as_i64().unwrap(), Ordering::SeqCst);
                        StatusCode::NO_CONTENT
                    },
                ),
            )
            .with_state(scale.clone()),
    )
    .await;
    config.actions = vec![template(&url)];
    let incident = store.insert(&item(), None).unwrap().0;
    assert!(
        actions::propose(
            &config,
            &store,
            incident.id,
            "scale",
            json!({"replicas":100}),
            "Investigated capacity"
        )
        .is_err()
    );
    let proposal = actions::propose(
        &config,
        &store,
        incident.id,
        "scale",
        json!({"replicas":3}),
        "Investigated capacity",
    )
    .unwrap();
    assert!(
        actions::execute(&config, &store, proposal.id)
            .await
            .is_err()
    );
    assert_eq!(scale.calls.load(Ordering::SeqCst), 0);
    actions::review(&store, proposal.id, "operator", true).unwrap();
    let mut drift = config.clone();
    drift.actions[0].body = json!({"replicas":5});
    assert!(actions::execute(&drift, &store, proposal.id).await.is_err());
    let (a, b) = tokio::join!(
        actions::execute(&config, &store, proposal.id),
        actions::execute(&config, &store, proposal.id)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert_eq!(scale.calls.load(Ordering::SeqCst), 1);
    let final_state: actions::Proposal = serde_json::from_value(
        store
            .record_get("action", &proposal.id.to_string())
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        final_state.status,
        actions::ActionStatus::Verified
    ));
    handle.abort();
}

#[test]
fn webhook_signatures_reject_tampering_expiry_and_malformed_unicode() {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let secret = "test signing key";
    let timestamp = "1700000000";
    let body = b"{\"event\":\"hello\"}";
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(format!("v0:{timestamp}:").as_bytes());
    mac.update(body);
    let signature = format!(
        "v0={}",
        mac.finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    ai_sre::gateway::verify_slack(secret, timestamp, &signature, body, 1700000001).unwrap();
    assert!(
        ai_sre::gateway::verify_slack(secret, timestamp, &signature, b"changed", 1700000001)
            .is_err()
    );
    assert!(
        ai_sre::gateway::verify_slack(secret, timestamp, &signature, body, 1700000400).is_err()
    );
    assert!(
        ai_sre::gateway::verify_slack(
            secret,
            timestamp,
            &format!("v0={}", "😀".repeat(16)),
            body,
            1700000001
        )
        .is_err()
    );
}

#[tokio::test]
async fn compact_context_preserves_data_and_sse_resumes_from_event_ids() {
    let (_dir, config, store) = setup();
    let incident = store.insert(&item(), None).unwrap().0;
    let result = agent::investigate(&config, &store, store.claim(incident.id, None).unwrap())
        .await
        .unwrap();
    let compacted = store.compact(incident.id).unwrap();
    assert!(compacted.context_summary.is_some());
    assert_eq!(compacted.evidence.len(), result.evidence.len());
    let app = api::router(App::new(config, store, None));
    let request = axum::http::Request::builder()
        .uri(format!("/api/incidents/{}/events", incident.id))
        .header("host", "localhost")
        .header("last-event-id", "0")
        .body(axum::body::Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(!text.contains("id: 0\n"));
    assert!(text.contains("context_compacted"));
    assert!(text.contains("event: complete"));
}

#[tokio::test]
async fn workflow_scope_blocks_tools_even_when_invoked_directly() {
    let (_dir, mut config, _store) = setup();
    config.workflow = Some(ai_sre::config::Workflow {
        name: "notes".into(),
        instructions: vec!["Read operator context only".into()],
        allowed_tools: vec!["memory_read".into()],
    });
    let tools = Toolbox::new(config).unwrap();
    assert_eq!(tools.definitions().len(), 1);
    assert!(
        tools
            .call("work_items", json!({}), "checkout-api")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn synthetic_lab_scenarios_are_investigated_over_real_http() {
    use ai_sre::lab::{Lab, Scenario};
    for (scenario, expected) in [
        (Scenario::Healthy, None),
        (Scenario::Memory, Some("Memory pressure detected")),
        (
            Scenario::Dependency,
            Some("Dependency connectivity failure"),
        ),
        (Scenario::RateLimit, Some("Rate limiting observed")),
        (Scenario::Disk, Some("Storage capacity exhausted")),
    ] {
        let (_dir, mut config, store) = setup();
        let (url, handle) = serve(ai_sre::lab::router(Lab::new(scenario))).await;
        config.prometheus = Some(ai_sre::config::Endpoint {
            url: url.clone(),
            token_env: None,
        });
        config.loki = Some(ai_sre::config::Endpoint {
            url,
            token_env: None,
        });
        let i = store.insert(&item(), None).unwrap().0;
        let result = agent::investigate(&config, &store, store.claim(i.id, None).unwrap())
            .await
            .unwrap();
        assert_eq!(result.status, Status::Triaged);
        assert_eq!(result.evidence.len(), 2);
        let findings = result.report.unwrap().findings;
        match expected {
            Some(title) => assert!(findings.iter().any(|f| f.title == title)),
            None => assert!(findings.is_empty()),
        }
        handle.abort();
    }
}

#[tokio::test]
async fn authenticated_gateway_events_are_idempotent_and_reject_unlisted_senders() {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let (_dir, mut config, store) = setup();
    config.gateways.slack = Some(ai_sre::gateway::Slack {
        signing_secret_env: "UNUSED_TEST_SECRET".into(),
        allowed_channels: vec!["C1".into()],
        allowed_users: vec!["U1".into()],
        service: "checkout-api".into(),
    });
    config.gateways.telegram = Some(ai_sre::gateway::Telegram {
        secret_env: "UNUSED_TEST_SECRET".into(),
        allowed_chats: vec![42],
        allowed_users: vec![7],
        service: "checkout-api".into(),
    });
    let app = api::router(
        App::new(config, store.clone(), Some("workspace-auth".into())).with_gateway_secrets(
            Some("signing-test-key".into()),
            Some("telegram-test-key".into()),
        ),
    );
    let timestamp = chrono::Utc::now().timestamp().to_string();
    let slack = |user: &str| {
        let body=json!({"type":"event_callback","event_id":"ev-one","event":{"type":"app_mention","user":user,"channel":"C1","text":"Investigate latency"}}).to_string();
        let mut mac = Hmac::<Sha256>::new_from_slice(b"signing-test-key").unwrap();
        mac.update(format!("v0:{timestamp}:").as_bytes());
        mac.update(body.as_bytes());
        let signature = format!(
            "v0={}",
            mac.finalize()
                .into_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        axum::http::Request::builder()
            .method("POST")
            .uri("/api/webhooks/slack")
            .header("content-type", "application/json")
            .header("x-slack-request-timestamp", &timestamp)
            .header("x-slack-signature", signature)
            .body(axum::body::Body::from(body))
            .unwrap()
    };
    assert_eq!(
        app.clone().oneshot(slack("U1")).await.unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(
        app.clone().oneshot(slack("U1")).await.unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(store.list().unwrap().len(), 1);
    assert_eq!(
        app.clone()
            .oneshot(slack("unlisted"))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let telegram = |secret: &str| {
        axum::http::Request::builder().method("POST").uri("/api/webhooks/telegram").header("content-type","application/json").header("x-telegram-bot-api-secret-token",secret).body(axum::body::Body::from(json!({"update_id":1,"message":{"from":{"id":7,"is_bot":false},"chat":{"id":42},"text":"Check checkout errors"}}).to_string())).unwrap()
    };
    assert_eq!(
        app.clone()
            .oneshot(telegram("wrong"))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.clone()
            .oneshot(telegram("telegram-test-key"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        app.oneshot(telegram("telegram-test-key"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(store.list().unwrap().len(), 2);
}

#[tokio::test]
async fn an_unverified_action_is_not_retried_and_expired_approvals_are_rejected() {
    let (_dir, mut config, store) = setup();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let (url, handle) = serve(
        Router::new()
            .route("/state", get(|| async { Json(json!({"replicas":1})) }))
            .route(
                "/scale",
                patch(move || {
                    let counter = counter.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        StatusCode::NO_CONTENT
                    }
                }),
            ),
    )
    .await;
    config.actions = vec![template(&url)];
    let incident = store.insert(&item(), None).unwrap().0;
    let p = actions::propose(
        &config,
        &store,
        incident.id,
        "scale",
        json!({"replicas":3}),
        "Test unknown verification",
    )
    .unwrap();
    actions::review(&store, p.id, "operator", true).unwrap();
    let result = actions::execute(&config, &store, p.id).await.unwrap();
    assert!(matches!(result.status, actions::ActionStatus::Unknown));
    assert!(actions::execute(&config, &store, p.id).await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let p = actions::propose(
        &config,
        &store,
        incident.id,
        "scale",
        json!({"replicas":3}),
        "Test expired approval",
    )
    .unwrap();
    actions::review(&store, p.id, "operator", true).unwrap();
    store
        .record_mutate("action", &p.id.to_string(), |v| {
            v["approved_at"] = json!(chrono::Utc::now() - chrono::Duration::minutes(20));
            Ok(())
        })
        .unwrap();
    assert!(actions::execute(&config, &store, p.id).await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    handle.abort();
}

#[test]
fn context_lookup_is_not_lost_when_other_services_have_more_than_one_page_of_notes() {
    let (_dir, _config, store) = setup();
    store
        .record_put(
            "memory",
            "important",
            &json!({"service":"checkout-api","text":"retained context"}),
        )
        .unwrap();
    for i in 0..110 {
        store
            .record_put(
                "memory",
                &i.to_string(),
                &json!({"service":"other","text":"unrelated"}),
            )
            .unwrap();
    }
    assert_eq!(
        store
            .service_records("memory", "checkout-api")
            .unwrap()
            .len(),
        1
    );
    assert!(store.record_get("memory", "important").unwrap().is_some());
}
