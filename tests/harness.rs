use ai_sre::{
    agent,
    config::Config,
    domain::{Incident, Status},
    evaluation,
    harness::{Policy, Run},
    store::Store,
    tools::Toolbox,
};
use serde_json::json;

#[tokio::test]
async fn fault_matrix_exercises_the_production_loop_and_rejects_regressing_baselines() {
    let report = evaluation::harness(1).await.unwrap();
    let failures: Vec<_> = report.cases.iter().filter(|c| !c.passed).collect();
    assert!(report.passed, "{failures:#?}");
    assert_eq!(report.cases.len(), 28);
    assert!(report.cases.iter().all(|c| c.checks.len() >= 12));
    let mut regressed: evaluation::Evaluation =
        serde_json::from_value(serde_json::to_value(&report).unwrap()).unwrap();
    regressed.cases[0].passed = false;
    regressed.cases[1].model_attempts += 1;
    regressed.cases[2].tool_dispatches += 1;
    assert_eq!(regressed.compare(&report).unwrap().len(), 3);
    regressed.cases.pop();
    assert!(
        regressed.compare(&report).is_err(),
        "a missing scenario cannot count as improvement"
    );
    regressed.scenario_sha256 = "changed workload".into();
    assert!(regressed.compare(&report).is_err());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reports/result.json");
    report.write(&path).unwrap();
    let reopened: evaluation::Evaluation =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(report.compare(&reopened).unwrap().is_empty());
    // Exercise the actual CLI exit gate, including a baseline/output path collision.
    let mut stricter = reopened;
    stricter.cases[0].model_attempts = 0;
    let path = dir.path().join("baseline.json");
    stricter.write(&path).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ai-sre"))
        .args(["eval", "--suite", "harness", "--baseline"])
        .arg(&path)
        .arg("--output")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!output.status.success(), "regressions must fail CI");
    let failed: evaluation::Evaluation =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!failed.passed);
    assert!(!failed.baseline_regressions.is_empty());
}

#[tokio::test]
async fn cached_observations_are_scoped_to_a_run_and_new_runs_collect_fresh_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("evidence.json");
    let config = Config {
        database: dir.path().join("state.db"),
        evidence_file: Some(path.clone()),
        ..Config::default()
    };
    let write = |value| {
        std::fs::write(&path,json!([{"service":"checkout","source":"logs","summary":"observation","data":{"value":value}}]).to_string()).unwrap()
    };
    write(1);
    let store = Store::open(&config.database).unwrap();
    let incident = store
        .insert(
            &Incident::new("Inspect checkout".into(), "checkout".into()).unwrap(),
            None,
        )
        .unwrap()
        .0;
    let tools = Toolbox::new(config.clone()).unwrap();
    let mut run = store.claim(incident.id, None).unwrap();
    let mut guard = Run::new(&run);
    let mut gaps = vec![];
    let first = guard
        .collect(
            &tools,
            &store,
            &mut run,
            "local_evidence",
            json!({}),
            &mut gaps,
        )
        .await
        .unwrap();
    write(2);
    let repeated = guard
        .collect(
            &tools,
            &store,
            &mut run,
            "local_evidence",
            json!({}),
            &mut gaps,
        )
        .await
        .unwrap();
    assert_eq!(first["evidence"], repeated["evidence"]);
    assert_eq!(repeated["cached"], true);
    assert_eq!(run.usage.tool_calls, 1);
    run.status = Status::Triaged;
    store.finish(&run).unwrap();
    let next = agent::investigate(&config, &store, store.claim(incident.id, None).unwrap())
        .await
        .unwrap();
    assert_eq!(next.evidence.len(), 2);
    assert_eq!(next.evidence[1].data["value"], 2);
    assert_ne!(next.run_id, run.run_id);
}

#[tokio::test]
async fn configured_source_circuit_covers_sibling_operations_and_resets_next_run() {
    let dir = tempfile::tempdir().unwrap();
    // Reserve a loopback listener without accepting HTTP: deterministic client timeout.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let connection = ai_sre::integrations::Connection {
        name: "cluster".into(),
        kind: "kubernetes".into(),
        url: format!("http://{}", listener.local_addr().unwrap()),
        token_env: None,
        header_env: Default::default(),
        parameters: Default::default(),
    };
    let config = Config {
        database: dir.path().join("state.db"),
        timeout_seconds: 1,
        connections: vec![connection],
        harness: Policy {
            source_failure_threshold: 1,
            ..Policy::default()
        },
        ..Config::default()
    };
    let store = Store::open(&config.database).unwrap();
    let tools = Toolbox::new(config).unwrap();
    let incident = store
        .insert(
            &Incident::new("Inspect cluster".into(), "checkout".into()).unwrap(),
            None,
        )
        .unwrap()
        .0;
    let mut incident = store.claim(incident.id, None).unwrap();
    let mut runtime = Run::new(&incident);
    let mut gaps = vec![];
    runtime
        .collect(
            &tools,
            &store,
            &mut incident,
            "conn_cluster_pods",
            json!({"namespace":"default"}),
            &mut gaps,
        )
        .await
        .unwrap();
    let sibling = runtime
        .collect(
            &tools,
            &store,
            &mut incident,
            "conn_cluster_events",
            json!({"namespace":"default"}),
            &mut gaps,
        )
        .await
        .unwrap();
    assert!(sibling["errors"].to_string().contains("circuit open"));
    assert_eq!(incident.usage.tool_calls, 1);
    incident.status = Status::Triaged;
    store.finish(&incident).unwrap();
    let mut next = store.claim(incident.id, None).unwrap();
    let mut next_runtime = Run::new(&next);
    next_runtime
        .collect(
            &tools,
            &store,
            &mut next,
            "conn_cluster_events",
            json!({"namespace":"default"}),
            &mut gaps,
        )
        .await
        .unwrap();
    assert_eq!(
        next.usage.tool_calls, 2,
        "new run must probe the source again"
    );
}

#[tokio::test]
async fn cancellation_at_commit_keeps_trace_and_terminal_status_consistent() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("db")).unwrap();
    let item = store
        .insert(
            &Incident::new("Inspect".into(), "test".into()).unwrap(),
            None,
        )
        .unwrap()
        .0;
    let mut run = store.claim(item.id, None).unwrap();
    Run::new(&run).trace(&mut run, "run_completed", "investigation", 0, vec![]);
    run.status = Status::Triaged;
    store.cancel(item.id).unwrap();
    let saved = store.finish(&run).unwrap();
    assert_eq!(saved.status, Status::Cancelled);
    assert_eq!(saved.trace.last().unwrap().phase, "run_cancelled");
}

#[test]
fn policy_rejects_unbounded_configuration_and_compaction_preserves_unicode() {
    let mut config = Config::default();
    config.harness.max_context_bytes = 0;
    assert!(config.validate().is_err());
    config.harness = Policy::default();
    config.harness.report_repairs = 100;
    assert!(config.validate().is_err());
    config.harness = Policy::default();
    config.harness.max_model_attempts = 0;
    assert!(config.validate().is_err());
    assert_eq!(
        ai_sre::harness::excerpt("故障 evidence", 4),
        "故 [excerpt truncated]"
    );
}

#[tokio::test]
async fn model_budgets_reset_per_run_while_lifetime_usage_accumulates() {
    use axum::{Json, Router, routing::post};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/model", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener,Router::new().route("/model",post(||async {
            Json(json!({"choices":[{"message":{"role":"assistant","content":json!({"summary":"No observations available","findings":[],"next_steps":["Collect telemetry"],"gaps":["No configured source"]}).to_string()}}],"usage":{"prompt_tokens":10,"completion_tokens":5}}))
        }))).await.unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        database: dir.path().join("db"),
        model: Some(ai_sre::config::Model {
            url,
            name: "scripted".into(),
            key_env: None,
            protocol: ai_sre::providers::Protocol::OpenAi,
            input_price_per_million: 0.0,
            output_price_per_million: 0.0,
        }),
        harness: Policy {
            max_model_attempts: 1,
            max_reported_tokens: 1,
            ..Policy::default()
        },
        ..Config::default()
    };
    let store = Store::open(&config.database).unwrap();
    let item = store
        .insert(
            &Incident::new("Inspect".into(), "test".into()).unwrap(),
            None,
        )
        .unwrap()
        .0;
    let first = agent::investigate(&config, &store, store.claim(item.id, None).unwrap())
        .await
        .unwrap();
    let second = agent::investigate(&config, &store, store.claim(item.id, None).unwrap())
        .await
        .unwrap();
    assert_eq!(first.status, Status::Triaged);
    assert_eq!(second.status, Status::Triaged);
    assert_eq!(second.usage.model_calls, 2);
    assert_eq!(second.usage.input_tokens, 20);
    assert_ne!(first.run_id, second.run_id);
    assert!(second.trace.iter().any(|t| t.run_id == first.run_id));
    assert!(second.trace.iter().any(|t| t.run_id == second.run_id));
    server.abort();
}
