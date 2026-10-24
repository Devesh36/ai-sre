use ai_sre::{
    api::{self, App},
    config::Config,
    domain::{Incident, Status, review},
    harness::Trace,
    store::Store,
};
use chrono::{Duration, Utc};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

fn fixture() -> Incident {
    let mut items: Vec<Incident> =
        serde_json::from_str(include_str!("../docs/examples/live-incidents.json")).unwrap();
    items.remove(0)
}

#[test]
fn repeated_collection_is_not_independent_support_and_context_is_excluded() {
    let mut incident = fixture();
    let now = incident.evidence.last().unwrap().observed_at + Duration::seconds(5);
    // A cached tool event refers to the same evidence; it is not another observation.
    incident.trace.push(Trace {
        run_id: incident.run_id,
        phase: "tool_cached".into(),
        operation: "local_evidence".into(),
        elapsed_ms: 0,
        request_bytes: 0,
        evidence_ids: vec!["E5".into()],
    });
    let r = review::analyze(&incident, now);
    assert_eq!(
        (
            r.observations,
            r.distinct_payloads,
            r.repeated_payloads,
            r.context_items
        ),
        (6, 3, 3, 2)
    );
    assert!(r.changes.available);
    assert!(r.changes.added.is_empty() && r.changes.not_repeated.is_empty());
    assert_eq!(r.changes.repeated.len(), 3);
    let memory = &r.findings[0];
    assert_eq!(memory.distinct_payloads, 1);
    assert_eq!(memory.source_count, 1);
    assert_eq!(memory.current_observations, ["E5"]);
    assert_eq!(memory.older_or_unlinked_observations, ["E1"]);
    assert!(
        memory
            .concerns
            .iter()
            .any(|s| s.contains("Repeated payloads"))
    );
    assert!(memory.aged_citations.is_empty());
}

#[test]
fn changed_payloads_are_reported_as_added_and_not_repeated_never_as_recovery() {
    let mut incident = fixture();
    incident.evidence[4].data = json!({"status":"running"});
    let r = review::analyze(&incident, Utc::now());
    assert!(r.changes.available);
    assert_eq!(r.changes.added, ["E5"]);
    assert_eq!(r.changes.not_repeated, ["E1"]);
    assert_eq!(r.changes.repeated.len(), 2);
    assert!(
        r.limitations
            .iter()
            .any(|s| s.contains("does not establish recovery"))
    );
    // Object key order is not a change, while array order and value types are retained.
    incident.evidence[4].data =
        serde_json::from_str(&serde_json::to_string(&incident.evidence[0].data).unwrap()).unwrap();
    assert!(
        review::analyze(&incident, Utc::now())
            .changes
            .added
            .is_empty()
    );
}

#[test]
fn incomplete_failed_missing_and_capped_runs_withhold_comparison() {
    for variant in [
        "missing_start",
        "missing_evidence",
        "failed_tool",
        "failed_run",
        "cancelled",
        "in_progress",
        "capped",
    ] {
        let mut i = fixture();
        match variant {
            "missing_start" => {
                i.trace
                    .retain(|t| t.run_id != i.run_id || t.phase != "run_started");
            }
            "missing_evidence" => {
                i.evidence.retain(|e| e.id != "E5");
            }
            "failed_tool" => {
                let mut t = i.trace.last().unwrap().clone();
                t.phase = "tool_failed".into();
                t.operation = "loki_query".into();
                i.trace.push(t);
            }
            "failed_run" => i.trace.last_mut().unwrap().phase = "run_failed".into(),
            "cancelled" => i.trace.last_mut().unwrap().phase = "run_cancelled".into(),
            "in_progress" => {
                i.status = Status::Investigating;
                i.run_id = Some(Uuid::new_v4());
            }
            "capped" => {
                while i.evidence.len() < 128 {
                    let mut e = i.evidence[0].clone();
                    e.id = format!("E{}", i.evidence.len() + 1);
                    i.evidence.push(e);
                }
            }
            _ => unreachable!(),
        }
        let r = review::analyze(&i, Utc::now());
        assert!(!r.changes.available, "{variant}");
        assert!(r.changes.not_repeated.is_empty(), "{variant}");
    }
}

#[test]
fn review_exposes_historical_context_missing_and_bad_clock_citations() {
    let mut i = fixture();
    let now = i.evidence[0].observed_at + Duration::hours(1);
    i.report.as_mut().unwrap().findings[0].evidence_ids =
        vec!["E1".into(), "E4".into(), "E99".into()];
    i.evidence[5].observed_at = now + Duration::minutes(5);
    let r = review::analyze(&i, now);
    let f = &r.findings[0];
    assert_eq!(f.missing_citations, ["E99"]);
    assert_eq!(f.context_citations, ["E4"]);
    assert_eq!(f.aged_citations, ["E1"]);
    assert!(f.current_observations.is_empty());
    assert_eq!(r.findings[1].future_dated_citations, ["E6"]);
    assert!(f.concerns.iter().any(|s| s.contains("latest run")));
}

#[test]
fn legacy_empty_and_truncated_evidence_remain_explicitly_uncertain() {
    let empty = Incident::new("Check service".into(), "checkout".into()).unwrap();
    let r = review::analyze(&empty, Utc::now());
    assert!(!r.changes.available && r.findings.is_empty());
    assert!(
        r.checks
            .iter()
            .any(|c| c.contains("Collect service observations"))
    );
    let mut legacy = fixture();
    legacy.trace.clear();
    legacy.run_id = None;
    legacy.evidence[0].data = json!({"truncated":true,"text":"partial"});
    let r = review::analyze(&legacy, Utc::now());
    assert!(!r.changes.available);
    assert!(r.findings.iter().all(|f| f.current_observations.is_empty()));
    assert!(r.limitations.iter().any(|c| c.contains("truncated")));
}

#[test]
fn markdown_handoff_escapes_untrusted_text_omits_payloads_and_labels_old_reports() {
    let mut i = fixture();
    i.title = "<script>alert(1)</script> [click](javascript:bad) ```\x1b[2J".into();
    i.evidence[0].data = json!({"private":"NEVER_INCLUDE_RAW"});
    i.status = Status::Failed;
    let output = review::handoff(&i, &review::analyze(&i, Utc::now()));
    assert!(output.starts_with("# Incident handoff:"));
    assert!(output.contains("&lt;script&gt;") && output.contains("\\[click\\]"));
    assert!(!output.contains("<script>") && !output.contains("```") && !output.contains('\x1b'));
    assert!(!output.contains("NEVER_INCLUDE_RAW"));
    assert!(
        output.contains("retained report")
            && output.contains("Evidence index")
            && output.contains("E5")
    );
}

#[tokio::test]
async fn review_and_handoff_api_require_auth_and_do_not_modify_the_incident() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = Config {
        database: dir.path().join("state.db"),
        ..Default::default()
    };
    let store = Store::open(&cfg.database).unwrap();
    let i = fixture();
    store.insert(&i, None).unwrap();
    let app = api::router(App::new(cfg, store.clone(), Some("example-token".into())));
    for path in ["review", "handoff"] {
        let url = format!("/api/incidents/{}/{path}", i.id);
        let request = axum::http::Request::builder()
            .uri(&url)
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!(app.clone().oneshot(request).await.unwrap().status(), 401);
        let request = axum::http::Request::builder()
            .uri(&url)
            .header("Authorization", "Bearer example-token")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);
        let value: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        if path == "review" {
            assert_eq!(value["repeated_payloads"], 3);
            assert_eq!(value["changes"]["available"], true);
        } else {
            assert!(
                value["markdown"]
                    .as_str()
                    .unwrap()
                    .contains("Incident handoff")
            );
        }
    }
    let request = axum::http::Request::builder()
        .uri(format!("/api/incidents/{}/review", Uuid::new_v4()))
        .header("Authorization", "Bearer example-token")
        .body(axum::body::Body::empty())
        .unwrap();
    assert_eq!(app.oneshot(request).await.unwrap().status(), 404);
    assert_eq!(
        serde_json::to_value(store.get(i.id).unwrap().unwrap()).unwrap(),
        serde_json::to_value(i).unwrap()
    );
}

#[test]
fn cli_review_changes_and_handoff_work_against_a_reopened_database() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = Config {
        database: dir.path().join("state.db"),
        ..Default::default()
    };
    let i = fixture();
    Store::open(&cfg.database)
        .unwrap()
        .insert(&i, None)
        .unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
    for command in ["review", "changes", "handoff"] {
        let mut cli = std::process::Command::new(env!("CARGO_BIN_EXE_ai-sre"));
        cli.arg("--config")
            .arg(&path)
            .arg(command)
            .arg(i.id.to_string());
        if command != "handoff" {
            cli.arg("--json");
        }
        let result = cli.output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let output = String::from_utf8(result.stdout).unwrap();
        if command == "handoff" {
            assert!(output.starts_with("# Incident handoff:"));
        } else {
            let value: Value = serde_json::from_str(&output).unwrap();
            assert!(!output.contains('\x1b'));
            if command == "changes" {
                assert_eq!(value["repeated"].as_array().unwrap().len(), 3);
            }
        }
    }
}

#[tokio::test]
async fn real_followup_detects_changed_input_and_flags_a_retained_historical_finding() {
    let dir = tempfile::tempdir().unwrap();
    let evidence = dir.path().join("evidence.json");
    let cfg = Config {
        database: dir.path().join("state.db"),
        evidence_file: Some(evidence.clone()),
        ..Default::default()
    };
    let store = Store::open(&cfg.database).unwrap();
    let incident = Incident::new("Investigate checkout".into(), "checkout".into()).unwrap();
    store.insert(&incident, None).unwrap();
    std::fs::write(&evidence,json!([{"service":"checkout","source":"synthetic/pod","summary":"Container OOMKilled","data":{"reason":"OOMKilled"}}]).to_string()).unwrap();
    let initial = ai_sre::agent::investigate(&cfg, &store, store.claim(incident.id, None).unwrap())
        .await
        .unwrap();
    assert_eq!(initial.report.unwrap().findings.len(), 1);
    std::fs::write(&evidence,json!([{"service":"checkout","source":"synthetic/pod","summary":"Container currently running","data":{"reason":"Running"}}]).to_string()).unwrap();
    let followup = ai_sre::agent::investigate(
        &cfg,
        &store,
        store.claim(incident.id, Some("Recheck the pod")).unwrap(),
    )
    .await
    .unwrap();
    let r = review::analyze(&followup, Utc::now());
    assert_eq!(r.changes.added, ["E2"]);
    assert_eq!(r.changes.not_repeated, ["E1"]);
    assert!(r.findings[0].current_observations.is_empty());
    assert!(
        r.findings[0]
            .concerns
            .iter()
            .any(|c| c.contains("latest run"))
    );
    assert_eq!(
        store.get(incident.id).unwrap().unwrap().status,
        Status::Triaged
    );
}
