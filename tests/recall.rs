use ai_sre::{
    api::{self, App},
    config::Config,
    domain::{Incident, Status, recall},
    store::Store,
};
use chrono::{Duration, Utc};
use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};
use tower::ServiceExt;
use uuid::Uuid;

fn pair() -> (Incident, Incident) {
    let mut cases: Vec<Incident> =
        serde_json::from_str(include_str!("../docs/examples/live-incidents.json")).unwrap();
    let past = cases.remove(0);
    let mut current = past.clone();
    current.id = Uuid::new_v4();
    current.created_at += Duration::days(1);
    current.updated_at = current.created_at;
    (current, past)
}

#[test]
fn exact_matches_use_run_scoped_ids_and_do_not_count_repeated_collection_twice() {
    let (current, mut past) = pair();
    // Same content and different collection timestamps must still match.
    for e in &mut past.evidence {
        e.observed_at -= Duration::days(3);
    }
    let result = recall::analyze(&current, std::slice::from_ref(&past), Utc::now());
    let case = &result.cases[0];
    assert_eq!(case.exact_matches.len(), 3);
    assert_eq!(case.shared_symptoms.len(), 2);
    assert_eq!(case.current_distinct_payloads, 3);
    assert_eq!(case.past_distinct_payloads, 3);
    assert_eq!(case.matched_run, past.run_id.unwrap());
    assert!(
        case.exact_matches
            .iter()
            .all(|m| m.current_ids.len() == 1 && m.past_ids.len() == 1)
    );
    assert!(
        case.exact_matches
            .iter()
            .any(|m| m.current_ids == ["E5"] && m.past_ids == ["E5"])
    );
    assert!(
        result
            .limitations
            .iter()
            .any(|s| s.contains("does not verify"))
    );
}

#[test]
fn changed_payloads_match_only_explicit_symptoms_and_show_differences() {
    let (current, mut past) = pair();
    for e in &mut past.evidence {
        if e.source == "runbook" {
            continue;
        }
        e.summary = "Previous incident signal".into();
        e.data = if e.id == "E1" || e.id == "E5" {
            json!({"message":"OOMKilled", "pod":"another-pod"})
        } else {
            json!({"message":"disk full"})
        };
    }
    let result = recall::analyze(&current, &[past], Utc::now());
    let case = &result.cases[0];
    assert!(case.exact_matches.is_empty());
    assert_eq!(case.shared_symptoms.len(), 1);
    assert_eq!(case.shared_symptoms[0].label, "Memory pressure detected");
    assert_eq!(
        case.current_only_symptoms,
        ["Dependency connectivity failure"]
    );
    assert_eq!(case.past_only_symptoms, ["Storage capacity exhausted"]);
}

#[test]
fn recovery_run_does_not_hide_an_earlier_matching_failure_run() {
    let (current, mut past) = pair();
    past.status = Status::Resolved;
    for e in past.evidence.iter_mut().skip(4) {
        e.summary = "Healthy check".into();
        e.data = json!({"healthy":true});
    }
    let result = recall::analyze(&current, std::slice::from_ref(&past), Utc::now());
    let case = &result.cases[0];
    assert_ne!(Some(case.matched_run), past.run_id);
    assert_eq!(case.matched_run, past.trace[0].run_id.unwrap());
    assert!(
        case.exact_matches
            .iter()
            .any(|m| m.current_ids == ["E5"] && m.past_ids == ["E1"])
    );
}

#[test]
fn incomplete_current_runs_and_stale_only_matches_are_withheld() {
    let (current, past) = pair();
    for variant in [
        "failed",
        "new_claim",
        "missing_evidence",
        "failed_tool",
        "capped",
        "legacy",
    ] {
        let mut i = current.clone();
        match variant {
            "failed" => i.status = Status::Failed,
            "new_claim" => {
                i.status = Status::Investigating;
                i.run_id = Some(Uuid::new_v4());
            }
            "missing_evidence" => i.evidence.retain(|e| e.id != "E5"),
            "failed_tool" => {
                let mut t = i.trace.last().unwrap().clone();
                t.phase = "tool_failed".into();
                i.trace.push(t);
            }
            "capped" => {
                while i.evidence.len() < 128 {
                    i.evidence.push(i.evidence[0].clone());
                }
            }
            "legacy" => i.trace.clear(),
            _ => unreachable!(),
        }
        let result = recall::analyze(&i, std::slice::from_ref(&past), Utc::now());
        assert!(result.cases.is_empty(), "{variant}");
        assert!(result.reason.contains("Complete a successful"), "{variant}");
    }
    let mut i = current;
    for e in i.evidence.iter_mut().skip(4) {
        e.summary = "Different observation".into();
        e.data = json!({"ok":true});
    }
    // The previous run still contains matching evidence; only the latest current run may be used.
    assert!(recall::analyze(&i, &[past], Utc::now()).cases.is_empty());
}

#[test]
fn context_truncation_report_titles_and_other_services_cannot_create_matches() {
    let (current, past) = pair();
    for variant in [
        "context",
        "truncated",
        "report_only",
        "different_service",
        "future_case",
        "self",
        "failed_collection",
    ] {
        let mut p = past.clone();
        match variant {
            "context" => {
                for e in &mut p.evidence {
                    e.source = "operator_context".into();
                }
            }
            "truncated" => {
                for e in &mut p.evidence {
                    e.data["truncated"] = json!(true);
                }
            }
            "report_only" => {
                for e in &mut p.evidence {
                    if e.source != "runbook" {
                        e.summary = "Unrelated observation".into();
                        e.data = json!({"status":"running"});
                    }
                }
            }
            "different_service" => p.service = "billing".into(),
            "future_case" => p.created_at = current.created_at + Duration::days(1),
            "self" => p.id = current.id,
            "failed_collection" => {
                for t in &mut p.trace {
                    if t.phase == "run_completed" {
                        t.phase = "run_failed".into();
                    }
                }
            }
            _ => unreachable!(),
        }
        assert!(
            recall::analyze(&current, &[p], Utc::now()).cases.is_empty(),
            "{variant}"
        );
    }
}

#[test]
fn bounded_search_is_service_scoped_before_limit_and_returns_explainable_top_five() {
    let (current, past) = pair();
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("db")).unwrap();
    store.insert(&past, None).unwrap();
    for n in 0..105 {
        let mut noise = past.clone();
        noise.id = Uuid::new_v4();
        noise.service = "other-service".into();
        noise.created_at += Duration::seconds(n + 1);
        noise.updated_at = noise.created_at;
        store.insert(&noise, None).unwrap();
    }
    assert_eq!(
        store.recall(&current).unwrap().cases[0].incident_id,
        past.id
    );
    for n in 0..101 {
        let mut p = past.clone();
        p.id = Uuid::new_v4();
        p.created_at += Duration::seconds(n + 1);
        p.updated_at = p.created_at;
        store.insert(&p, None).unwrap();
    }
    let result = store.recall(&current).unwrap();
    assert_eq!(result.searched, 100);
    assert!(result.search_truncated);
    assert_eq!(result.matched_cases, 100);
    assert_eq!(result.cases.len(), 5);
    assert!(
        result
            .cases
            .windows(2)
            .all(|w| w[0].updated_at >= w[1].updated_at)
    );
}

#[tokio::test]
async fn recall_api_enforces_auth_reports_missing_cases_and_does_not_modify_incidents() {
    let (current, past) = pair();
    let dir = tempfile::tempdir().unwrap();
    let cfg = Config {
        database: dir.path().join("db"),
        ..Default::default()
    };
    let store = Store::open(&cfg.database).unwrap();
    store.insert(&current, None).unwrap();
    store.insert(&past, None).unwrap();
    let app = api::router(App::new(cfg, store.clone(), Some("test-token".into())));
    for (auth, id, status) in [
        (false, current.id, 401),
        (true, current.id, 200),
        (true, Uuid::new_v4(), 404),
    ] {
        let mut request = axum::http::Request::builder().uri(format!("/api/incidents/{id}/recall"));
        if auth {
            request = request.header("Authorization", "Bearer test-token");
        }
        let response = app
            .clone()
            .oneshot(request.body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        if status == 200 {
            let body: Value = serde_json::from_slice(
                &axum::body::to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["cases"][0]["incident_id"], past.id.to_string());
        }
    }
    assert_eq!(
        serde_json::to_value(store.get(current.id).unwrap().unwrap()).unwrap(),
        serde_json::to_value(current).unwrap()
    );
}

#[test]
fn cli_json_and_repl_expose_recall_after_database_reopen() {
    let (current, past) = pair();
    let dir = tempfile::tempdir().unwrap();
    let cfg = Config {
        database: dir.path().join("db"),
        ..Default::default()
    };
    let store = Store::open(&cfg.database).unwrap();
    store.insert(&current, None).unwrap();
    store.insert(&past, None).unwrap();
    drop(store);
    let path = dir.path().join("config.toml");
    std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
    let cli = Command::new(env!("CARGO_BIN_EXE_ai-sre"))
        .arg("--config")
        .arg(&path)
        .args(["recall", &current.id.to_string(), "--json"])
        .output()
        .unwrap();
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let body: Value = serde_json::from_slice(&cli.stdout).unwrap();
    assert_eq!(
        body["cases"][0]["exact_matches"].as_array().unwrap().len(),
        3
    );
    let mut shell = Command::new(env!("CARGO_BIN_EXE_ai-sre"))
        .arg("--config")
        .arg(&path)
        .arg("repl")
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    write!(
        shell.stdin.take().unwrap(),
        "/resume {}\n/recall\n/exit\n",
        current.id
    )
    .unwrap();
    let output = shell.wait_with_output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Past incident recall") && text.contains(&past.id.to_string()));
    assert!(text.contains("Current-only symptoms") && text.contains("does not verify"));
}

#[test]
fn duplicate_ids_do_not_boost_ranking_and_exact_matches_outrank_shared_rules() {
    let (mut current, mut strong) = pair();
    let mut weak = strong.clone();
    weak.id = Uuid::new_v4();
    weak.updated_at += Duration::hours(1);
    for e in &mut weak.evidence {
        e.data["different_instance"] = json!(true);
    }
    for incident in [&mut current, &mut strong] {
        let mut duplicate = incident.evidence[4].clone();
        duplicate.id = "E9".into();
        incident.evidence.push(duplicate);
        let run_id = incident.run_id;
        incident
            .trace
            .iter_mut()
            .find(|t| t.run_id == run_id && t.phase == "tool_completed")
            .unwrap()
            .evidence_ids
            .push("E9".into());
    }
    let result = recall::analyze(&current, &[weak, strong.clone()], Utc::now());
    assert_eq!(result.cases.len(), 2);
    assert_eq!(result.cases[0].incident_id, strong.id);
    assert_eq!(result.cases[0].exact_matches.len(), 3);
    assert_eq!(result.cases[0].current_distinct_payloads, 3);
    assert!(
        result.cases[0]
            .exact_matches
            .iter()
            .any(|m| m.current_ids == ["E5", "E9"])
    );
    assert_eq!(result.cases[1].exact_matches.len(), 0);
    assert_eq!(result.cases[1].shared_symptoms.len(), 2);
}
