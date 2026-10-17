use ai_sre::{
    config::Config,
    core::{
        domain::types::tools::{ToolRole, ToolSurface},
        tool::{
            contracts::{
                ExecutionResult, RuntimeTool, SideEffectLevel, ToolCall, ToolContext, ToolFuture,
                ToolMetadata,
            },
            execution::{
                BeforeDecision, ExecutionHooks, ExecutionPatch, HookChain, execute_tool_calls,
                execution_order, summarize_tool_failures,
            },
            live_catalog::LiveToolCatalog,
            metadata::{RegistryMetadata, normalize_surfaces, normalize_tags},
            output::{Content, OutputBudget, truncate_output_text},
            schema::validate_input,
        },
    },
    tools::Toolbox,
};
use anyhow::Result;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn differential_output_and_batch_cases_match_source_functions() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/tool-port.json")).unwrap();
    for c in fixture["text"].as_array().unwrap() {
        assert_eq!(
            truncate_output_text(
                c["text"].as_str().unwrap(),
                c["bytes"].as_u64().unwrap() as usize,
                c["tokens"].as_bool().unwrap()
            ),
            c["expected"].as_str().unwrap()
        );
    }
    for c in fixture["blocks"].as_array().unwrap() {
        let content: Content = serde_json::from_value(c["content"].clone()).unwrap();
        let budget = OutputBudget {
            tokens: c["tokens"].as_u64().unwrap() as usize,
        };
        assert_eq!(
            serde_json::to_value(budget.bound(&content)).unwrap(),
            c["expected"]
        );
    }
    for c in fixture["budgets"].as_array().unwrap() {
        let budget = OutputBudget {
            tokens: c["tokens"].as_u64().unwrap() as usize,
        };
        assert_eq!(json!(budget.bytes(false)), c["normal"]);
        assert_eq!(json!(budget.bytes(true)), c["history"]);
        assert_eq!(json!(budget.replay_bytes()), c["replay"]);
    }
    for c in fixture["batches"].as_array().unwrap() {
        let roles: Vec<ToolRole> = serde_json::from_value(c["roles"].clone()).unwrap();
        let calls: Vec<_> = roles
            .iter()
            .enumerate()
            .map(|(i, _)| call(&format!("tool{i}")))
            .collect();
        let map = calls
            .iter()
            .zip(roles)
            .map(|(c, r)| (c.name.clone(), r))
            .collect();
        let result = execution_order(&calls, &map);
        assert_eq!(result.is_ok(), c["valid"].as_bool().unwrap());
        if let Ok(order) = result {
            assert_eq!(json!(order), c["order"]);
        }
    }
}
fn call(name: &str) -> ToolCall {
    ToolCall {
        id: name.into(),
        name: name.into(),
        input: json!({}),
    }
}
struct RecordingTool {
    meta: ToolMetadata,
    calls: Arc<Mutex<Vec<String>>>,
    fail: bool,
}
impl RuntimeTool for RecordingTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.meta
    }
    fn execute<'a>(&'a self, arguments: Value, context: &'a ToolContext) -> ToolFuture<'a> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(self.meta.name.clone());
            context.emit_update(&json!({"status":"started"}));
            if self.fail {
                anyhow::bail!("secret internal transport detail")
            }
            let mut result = ExecutionResult::success(arguments);
            result.terminate = self.meta.registry.role == ToolRole::TurnEnding;
            Ok(result)
        })
    }
}
fn tool(name: &str, role: ToolRole, calls: &Arc<Mutex<Vec<String>>>) -> Arc<dyn RuntimeTool> {
    Arc::new(RecordingTool {
        meta: ToolMetadata {
            name: name.into(),
            description: "test tool".into(),
            source: "fixture".into(),
            input_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
            registry: RegistryMetadata {
                role,
                ..RegistryMetadata::default()
            },
            side_effect_level: SideEffectLevel::ReadOnly,
            evidence_type: None,
            injected_params: vec![],
        },
        calls: calls.clone(),
        fail: false,
    })
}
#[tokio::test]
async fn batch_rejection_order_cancellation_and_failure_summary() {
    let log = Arc::new(Mutex::new(vec![]));
    let tools = vec![
        tool("menu", ToolRole::TurnEnding, &log),
        tool("plan", ToolRole::Bookkeeping, &log),
        tool("read", ToolRole::Action, &log),
    ];
    let hooks = HookChain::default();
    let context = ToolContext::default();
    let calls = vec![call("menu"), call("read")];
    let results = execute_tool_calls(&calls, &tools, &context, &hooks, &|| false).await;
    assert!(log.lock().unwrap().is_empty());
    assert!(results.iter().all(|r| r.metadata["batch_rejected"] == true));
    let calls = vec![call("menu"), call("plan")];
    let results = execute_tool_calls(&calls, &tools, &context, &hooks, &|| false).await;
    assert_eq!(*log.lock().unwrap(), vec!["plan", "menu"]);
    assert!(results[0].terminate);
    assert!(!results[1].terminate);
    log.lock().unwrap().clear();
    let checks = AtomicUsize::new(0);
    let results = execute_tool_calls(&calls, &tools, &context, &hooks, &|| {
        checks.fetch_add(1, Ordering::SeqCst) > 0
    })
    .await;
    assert_eq!(*log.lock().unwrap(), vec!["plan"]);
    assert_eq!(results[0].metadata["skipped"], true);
    assert_eq!(
        summarize_tool_failures(&calls, &results).tool_error_count,
        0
    );
    let calls = vec![call("read"), call("read")];
    log.lock().unwrap().clear();
    let results = execute_tool_calls(&calls, &tools, &context, &hooks, &|| false).await;
    assert!(log.lock().unwrap().is_empty());
    assert!(results.iter().all(|r| r.is_error));
}
struct Block;
impl ExecutionHooks for Block {
    fn before_call(&self, _call: &ToolCall, _tool: &ToolMetadata) -> Result<BeforeDecision> {
        anyhow::bail!("sensitive authorization detail")
    }
}
struct Patch;
impl ExecutionHooks for Patch {
    fn after_call(
        &self,
        _call: &ToolCall,
        result: &ExecutionResult,
    ) -> Result<Option<ExecutionPatch>> {
        let mut details = result.details.clone();
        details["observed"] = json!(true);
        Ok(Some(ExecutionPatch {
            details: Some(details),
            ..ExecutionPatch::default()
        }))
    }
}
#[tokio::test]
async fn failed_authorization_never_dispatches_and_patch_chains_see_prior_results() {
    let log = Arc::new(Mutex::new(vec![]));
    let tools = vec![tool("read", ToolRole::Action, &log)];
    let calls = vec![call("read")];
    let context = ToolContext::default();
    let results = execute_tool_calls(&calls, &tools, &context, &Block, &|| false).await;
    assert!(log.lock().unwrap().is_empty());
    assert_eq!(results[0].metadata["blocked"], true);
    assert!(
        !serde_json::to_string(&results)
            .unwrap()
            .contains("sensitive")
    );
    assert_eq!(
        summarize_tool_failures(&calls, &results).blocked_tool_calls,
        1
    );
    let results = execute_tool_calls(
        &calls,
        &tools,
        &context,
        &HookChain(vec![Arc::new(Patch)]),
        &|| false,
    )
    .await;
    assert_eq!(results[0].details["observed"], true);
}
#[test]
fn catalog_replacement_is_atomic_and_old_snapshots_remain_valid() {
    let log = Arc::new(Mutex::new(vec![]));
    let catalog = LiveToolCatalog::default();
    catalog
        .replace(vec![tool("read", ToolRole::Action, &log)])
        .unwrap();
    let old = catalog.snapshot().unwrap();
    assert!(Arc::ptr_eq(&old, &catalog.snapshot().unwrap()));
    assert!(
        catalog
            .replace(vec![
                tool("same", ToolRole::Action, &log),
                tool("same", ToolRole::Action, &log)
            ])
            .is_err()
    );
    assert!(Arc::ptr_eq(&old, &catalog.snapshot().unwrap()));
    catalog
        .replace(vec![tool("new_read", ToolRole::Action, &log)])
        .unwrap();
    assert_eq!(old[0].metadata().name, "read");
    assert_eq!(
        catalog.for_surface(ToolSurface::Chat).unwrap()[0].name,
        "new_read"
    );
    assert_eq!(
        normalize_surfaces(&[" CHAT ".into(), "chat".into(), "ACTION".into()]).unwrap(),
        vec![ToolSurface::Chat, ToolSurface::Action]
    );
    assert!(normalize_surfaces(&["invalid".into()]).is_err());
    assert_eq!(
        normalize_tags(&[" a ".into(), "".into(), "a".into(), "B".into()]),
        vec!["a", "B"]
    );
}
#[test]
fn recursive_schema_validation_enforces_types_references_and_limits_without_leaking_values() {
    let schema = json!({"type":"object","properties":{"rows":{"type":"array","maxItems":2,"items":{"$ref":"#/$defs/row"}}},"required":["rows"],"additionalProperties":false,"$defs":{"row":{"type":"object","properties":{"count":{"type":"integer","minimum":0,"maximum":10}},"required":["count"],"additionalProperties":false}}});
    validate_input(&schema, &json!({"rows":[{"count":3}]})).unwrap();
    for bad in [
        json!({"rows":[{"count":true}]}),
        json!({"rows":[{"count":11}]}),
        json!({"rows":[{"count":"secret-value"}]}),
        json!({"rows":[],"secret-key":"hidden"}),
        json!({"rows":[{}, {}, {}]}),
    ] {
        let message = validate_input(&schema, &bad).unwrap_err().to_string();
        assert!(!message.contains("secret") && !message.contains("hidden"));
    }
    let cycle =
        json!({"type":"object","$ref":"#/$defs/loop","$defs":{"loop":{"$ref":"#/$defs/loop"}}});
    assert!(validate_input(&cycle, &json!({})).is_err());
    let union = json!({"type":"object","properties":{"n":{"oneOf":[{"type":"number"},{"type":"integer"}]}}});
    assert!(validate_input(&union, &json!({"n":1})).is_err());
}
#[tokio::test]
async fn production_toolbox_rejects_unexpected_arguments_before_io() {
    let toolbox = Toolbox::new(Config::default()).unwrap();
    let error = toolbox
        .call("memory_read", json!({"database":"/secret/path"}), "service")
        .await
        .unwrap_err();
    assert!(!error.to_string().contains("secret"));
    assert!(error.to_string().contains("unexpected"));
}

#[test]
fn result_normalization_patch_rebudgeting_and_failure_envelopes() {
    for error in [
        Value::Null,
        json!(false),
        json!(0),
        json!(""),
        json!([]),
        json!({}),
    ] {
        assert!(!ExecutionResult::normalize(json!({"error":error,"rows":[1]})).is_error);
    }
    let mut result = ExecutionResult::normalize(
        json!({"error":"network failed","failure_state":"transport_failure"}),
    );
    assert!(result.is_error);
    let Content::Text(content) = result.provider_content(OutputBudget::default()) else {
        panic!("expected text")
    };
    assert!(content.contains("transport_failure"));
    result = ExecutionResult::success(json!("old"));
    result.provider_output_bounded = true;
    ExecutionPatch {
        content: Some(Content::Text("x".repeat(1000))),
        ..ExecutionPatch::default()
    }
    .apply(&mut result);
    let Content::Text(content) = result.provider_content(OutputBudget { tokens: 16 }) else {
        panic!("expected text")
    };
    assert!(content.contains("truncated"));
    assert_eq!(result.details, json!("old"));
}

#[tokio::test]
async fn configured_arguments_are_scoped_and_cannot_be_overridden() {
    let log = Arc::new(Mutex::new(vec![]));
    let base = tool("read", ToolRole::Action, &log);
    let mut metadata = base.metadata().clone();
    metadata.input_schema = json!({"type":"object","properties":{"endpoint":{"type":"string"},"query":{"type":"string"}},"required":["endpoint"],"additionalProperties":false});
    metadata.injected_params = vec!["endpoint".into()];
    let tools: Vec<Arc<dyn RuntimeTool>> = vec![Arc::new(RecordingTool {
        meta: metadata,
        calls: log.clone(),
        fail: false,
    })];
    let context = ToolContext {
        configured_arguments: [
            (
                "read".into(),
                json!({"endpoint":"configured"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
            (
                "other".into(),
                json!({"token":"private"}).as_object().unwrap().clone(),
            ),
        ]
        .into_iter()
        .collect(),
        ..ToolContext::default()
    };
    let mut request = call("read");
    request.input = json!({"query":"health"});
    let result = execute_tool_calls(
        &[request.clone()],
        &tools,
        &context,
        &HookChain::default(),
        &|| false,
    )
    .await;
    assert_eq!(
        result[0].details,
        json!({"endpoint":"configured","query":"health"})
    );
    request.input = json!({"endpoint":"attacker"});
    let result = execute_tool_calls(&[request], &tools, &context, &HookChain::default(), &|| {
        false
    })
    .await;
    assert!(result[0].is_error);
    assert_eq!(log.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn embedding_catalog_dispatches_the_real_service_scoped_toolbox() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        database: dir.path().join("state.db"),
        ..Config::default()
    };
    let store = ai_sre::store::Store::open(&config.database).unwrap();
    store.record_put("memory","note",&json!({"id":"note","service":"checkout","key":"dependency","text":"Check database saturation"})).unwrap();
    let toolbox = Toolbox::new(config).unwrap();
    let catalog = toolbox.runtime_catalog("checkout").unwrap();
    let snapshot = catalog.snapshot().unwrap();
    let results = execute_tool_calls(
        &[call("memory_read")],
        &snapshot,
        &ToolContext::default(),
        &HookChain::default(),
        &|| false,
    )
    .await;
    assert!(!results[0].is_error);
    assert!(
        results[0]
            .details
            .to_string()
            .contains("Check database saturation")
    );
}

#[tokio::test]
async fn production_agent_bounds_provider_output_but_retains_evidence() {
    use axum::{Json, Router, routing::post};
    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured = requests.clone();
    let app=Router::new().route("/model",post(move |Json(body):Json<Value>| {
        let captured=captured.clone(); async move {
            let mut requests=captured.lock().unwrap();requests.push(body);
            let message=if requests.len()==1 {json!({"role":"assistant","content":null,"tool_calls":[{"id":"read-1","type":"function","function":{"name":"memory_read","arguments":"{}"}}]})}
            else {json!({"role":"assistant","content":json!({"summary":"Evidence collected for review","findings":[],"next_steps":[],"gaps":[]}).to_string()})};
            Json(json!({"choices":[{"message":message}],"usage":{"prompt_tokens":1,"completion_tokens":1}}))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/model", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        database: dir.path().join("state.db"),
        model: Some(ai_sre::config::Model {
            url,
            name: "fixture".into(),
            protocol: ai_sre::providers::Protocol::OpenAi,
            key_env: None,
            input_price_per_million: 0.0,
            output_price_per_million: 0.0,
        }),
        harness: ai_sre::harness::Policy {
            tool_output_tokens: 64,
            ..Default::default()
        },
        ..Default::default()
    };
    let store = ai_sre::store::Store::open(&config.database).unwrap();
    let note = format!("HEAD{}TAIL", "中文🚀".repeat(400));
    store
        .record_put(
            "memory",
            "note",
            &json!({"id":"note","service":"checkout","text":note}),
        )
        .unwrap();
    let incident = store
        .insert(
            &ai_sre::domain::Incident::new("Inspect checkout".into(), "checkout".into()).unwrap(),
            None,
        )
        .unwrap()
        .0;
    let result =
        ai_sre::agent::investigate(&config, &store, store.claim(incident.id, None).unwrap())
            .await
            .unwrap();
    assert_eq!(result.status, ai_sre::domain::Status::Triaged);
    assert!(
        result
            .trace
            .iter()
            .any(|t| t.phase == "tool_output_bounded")
    );
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let response = requests[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "tool")
        .unwrap()["content"]
        .as_str()
        .unwrap();
    assert!(response.contains("truncated"));
    assert!(response.len() < 350);
    assert!(
        serde_json::to_string(&store.get(incident.id).unwrap().unwrap().evidence)
            .unwrap()
            .contains(&note)
    );
    server.abort();
}
