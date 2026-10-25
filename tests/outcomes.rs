use ai_sre::{
    api::{self, App},
    config::Config,
    domain::{
        Incident, Status,
        outcome::{Input, Verdict},
    },
    store::Store,
};
use chrono::Duration;
use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
    sync::{Arc, Barrier},
};
use tower::ServiceExt;
use uuid::Uuid;

fn fixture() -> Incident {
    let mut cases: Vec<Incident> =
        serde_json::from_str(include_str!("../docs/examples/live-incidents.json")).unwrap();
    cases.remove(0)
}
fn input(i: &Incident) -> Input {
    Input {id:Uuid::new_v4(),run_id:i.run_id.unwrap(),actor:"oncall-alex".into(),claim:"The observed container exhausted its memory limit".into(),verdict:Verdict::Supported,
        rationale:"Reviewed the termination event and its configured memory limit; this does not establish a memory leak".into(),evidence_ids:vec!["E5".into()],supersedes:None}
}
fn setup() -> (tempfile::TempDir, Store, Incident) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("state.db")).unwrap();
    let i = fixture();
    store.insert(&i, None).unwrap();
    (dir, store, i)
}

#[test]
fn outcomes_preserve_provenance_are_immutable_and_never_mutate_the_incident() {
    let (dir, store, i) = setup();
    let request = input(&i);
    let before = serde_json::to_value(&i).unwrap();
    let (record, created) = store.record_outcome(i.id, request).unwrap();
    assert!(created);
    assert_eq!(record.run_id, i.run_id.unwrap());
    assert_eq!(record.incident_updated_at, i.updated_at);
    assert_eq!(record.evidence[0].observed_at, i.evidence[4].observed_at);
    assert_eq!(record.evidence[0].payload_sha256.len(), 64);
    let raw = serde_json::to_string(&record).unwrap();
    assert!(!raw.contains("exit_code"));
    assert_eq!(
        serde_json::to_value(store.get(i.id).unwrap().unwrap()).unwrap(),
        before
    );
    drop(store);
    let reopened = Store::open(&dir.path().join("state.db")).unwrap();
    assert_eq!(
        serde_json::to_value(&reopened.outcomes(i.id).unwrap().records[0]).unwrap(),
        serde_json::to_value(record).unwrap()
    );
    let sql = rusqlite::Connection::open(dir.path().join("state.db")).unwrap();
    assert!(
        sql.execute("UPDATE outcomes SET document='{}'", [])
            .is_err()
    );
    assert!(sql.execute("DELETE FROM outcomes", []).is_err());
    assert_eq!(reopened.outcomes(i.id).unwrap().records.len(), 1);
}

#[test]
fn invalid_or_stale_assessments_leave_no_records() {
    for variant in [
        "stale_run",
        "running",
        "failed",
        "missing_start",
        "failed_collection",
        "missing_evidence",
        "historical_citation",
        "context",
        "truncated",
        "empty_actor",
        "empty_rationale",
        "long_claim",
        "duplicate_ids",
        "nil_id",
        "self_correction",
    ] {
        let (_dir, store, mut i) = setup();
        let mut request = input(&i);
        match variant {
            "stale_run" => request.run_id = Uuid::new_v4(),
            "running" => i.status = Status::Investigating,
            "failed" => i.status = Status::Failed,
            "missing_start" => i
                .trace
                .retain(|t| t.run_id != i.run_id || t.phase != "run_started"),
            "failed_collection" => {
                let mut t = i.trace.last().unwrap().clone();
                t.phase = "tool_failed".into();
                i.trace.push(t);
            }
            "missing_evidence" => i.evidence.retain(|e| e.id != "E5"),
            "historical_citation" => request.evidence_ids = vec!["E1".into()],
            "context" => request.evidence_ids = vec!["E8".into()],
            "truncated" => i.evidence[4].data["truncated"] = json!(true),
            "empty_actor" => request.actor = " \n".into(),
            "empty_rationale" => request.rationale.clear(),
            "long_claim" => request.claim = "a".repeat(1001),
            "duplicate_ids" => request.evidence_ids.push("E5".into()),
            "nil_id" => request.id = Uuid::nil(),
            "self_correction" => request.supersedes = Some(request.id),
            _ => unreachable!(),
        }
        store
            .mutate(i.id, |saved| {
                *saved = i.clone();
                Ok(())
            })
            .unwrap();
        assert!(store.record_outcome(i.id, request).is_err(), "{variant}");
        assert!(
            store.outcomes(i.id).unwrap().records.is_empty(),
            "{variant}"
        );
    }
}

#[test]
fn duplicate_submissions_are_idempotent_even_after_a_new_run_is_claimed() {
    let (_dir, store, i) = setup();
    let request = input(&i);
    let (first, _) = store.record_outcome(i.id, request.clone()).unwrap();
    store.claim(i.id, Some("A new investigation run")).unwrap();
    let (retried, created) = store.record_outcome(i.id, request.clone()).unwrap();
    assert!(!created);
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(retried).unwrap()
    );
    let mut conflicting = request;
    conflicting.verdict = Verdict::Refuted;
    assert!(
        store
            .record_outcome(i.id, conflicting)
            .unwrap_err()
            .to_string()
            .contains("different content")
    );
    assert_eq!(store.outcomes(i.id).unwrap().records.len(), 1);
}

#[test]
fn concurrent_retries_and_corrections_serialize_across_connections() {
    let (dir, store, i) = setup();
    let request = input(&i);
    let barrier = Arc::new(Barrier::new(4));
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let store = Store::open(&dir.path().join("state.db")).unwrap();
            let b = barrier.clone();
            let request = request.clone();
            let id = i.id;
            std::thread::spawn(move || {
                b.wait();
                store.record_outcome(id, request).unwrap().1
            })
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .map(|t| usize::from(t.join().unwrap()))
            .sum::<usize>(),
        1
    );
    assert_eq!(store.outcomes(i.id).unwrap().records.len(), 1);
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let store = Store::open(&dir.path().join("state.db")).unwrap();
            let b = barrier.clone();
            let mut corrected = input(&i);
            let id = i.id;
            corrected.supersedes = Some(request.id);
            corrected.verdict = Verdict::Inconclusive;
            std::thread::spawn(move || {
                b.wait();
                store.record_outcome(id, corrected).is_ok()
            })
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .map(|t| usize::from(t.join().unwrap()))
            .sum::<usize>(),
        1
    );
    let history = store.outcomes(i.id).unwrap();
    assert_eq!(history.records.len(), 2);
    assert_eq!(history.records[1].verdict, Verdict::Supported);
    assert_eq!(history.records[0].supersedes, Some(request.id));
}

#[test]
fn corrections_are_incident_scoped_and_history_is_bounded() {
    let (_dir, store, i) = setup();
    let first = input(&i);
    store.record_outcome(i.id, first.clone()).unwrap();
    let mut other = i.clone();
    other.id = Uuid::new_v4();
    store.insert(&other, None).unwrap();
    let mut cross = input(&other);
    cross.supersedes = Some(first.id);
    assert!(store.record_outcome(other.id, cross).is_err());
    for _ in 1..100 {
        store.record_outcome(i.id, input(&i)).unwrap();
    }
    assert!(
        store
            .record_outcome(i.id, input(&i))
            .unwrap_err()
            .to_string()
            .contains("100-assessment")
    );
    assert!(!store.record_outcome(i.id, first).unwrap().1);
    assert_eq!(store.outcomes(i.id).unwrap().records.len(), 100);
}

#[test]
fn historical_resolution_and_operator_support_never_become_current_evidence_or_change_rank() {
    let (_dir, store, past) = setup();
    let mut current = past.clone();
    current.id = Uuid::new_v4();
    current.created_at += Duration::days(1);
    current.updated_at = current.created_at;
    store.insert(&current, None).unwrap();
    store
        .mutate(past.id, |i| {
            i.status = Status::Resolved;
            Ok(())
        })
        .unwrap();
    assert!(
        store.outcomes(past.id).unwrap().records.is_empty(),
        "resolution alone must not create an outcome"
    );
    let before = store.recall(&current).unwrap();
    let original_current = serde_json::to_value(&current).unwrap();
    let request = input(&past);
    store.record_outcome(past.id, request.clone()).unwrap();
    let after = store.recall(&current).unwrap();
    assert_eq!(after.cases[0].operator_outcome_count, 1);
    assert_eq!(after.cases[0].operator_outcomes[0].incident_id, past.id);
    assert_eq!(before.cases[0].incident_id, after.cases[0].incident_id);
    assert_eq!(
        before.cases[0].exact_matches.len(),
        after.cases[0].exact_matches.len()
    );
    assert_eq!(
        before.cases[0].shared_symptoms.len(),
        after.cases[0].shared_symptoms.len()
    );
    assert!(
        after
            .limitations
            .iter()
            .any(|s| s.contains("never establish facts about another incident"))
    );
    let mut correction = input(&past);
    correction.supersedes = Some(request.id);
    correction.verdict = Verdict::Refuted;
    store.record_outcome(past.id, correction).unwrap();
    let after = store.recall(&current).unwrap();
    assert_eq!(after.cases[0].operator_outcome_count, 1);
    assert_eq!(
        after.cases[0].operator_outcomes[0].verdict,
        Verdict::Refuted
    );
    // Separate, conflicting judgments remain visible; there is no automatic consensus label.
    store.record_outcome(past.id, input(&past)).unwrap();
    let disputed = store.recall(&current).unwrap();
    assert_eq!(disputed.cases[0].operator_outcome_count, 2);
    assert!(
        disputed.cases[0]
            .operator_outcomes
            .iter()
            .any(|o| o.verdict == Verdict::Supported)
    );
    assert!(
        disputed.cases[0]
            .operator_outcomes
            .iter()
            .any(|o| o.verdict == Verdict::Refuted)
    );
    assert_eq!(
        serde_json::to_value(store.get(current.id).unwrap().unwrap()).unwrap(),
        original_current
    );
    assert!(store.outcomes(current.id).unwrap().records.is_empty());
}

async fn call(
    app: axum::Router,
    id: Uuid,
    method: &str,
    body: Option<Value>,
    auth: bool,
) -> (u16, Value) {
    let mut req = axum::http::Request::builder()
        .method(method)
        .uri(format!("/api/incidents/{id}/outcomes"))
        .header("Content-Type", "application/json");
    if auth {
        req = req.header("Authorization", "Bearer test-token");
    }
    let response = app
        .oneshot(
            req.body(axum::body::Body::from(
                body.map(|v| v.to_string()).unwrap_or_default(),
            ))
            .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"text":String::from_utf8_lossy(&bytes)})),
    )
}
#[tokio::test]
async fn api_checks_auth_status_codes_idempotency_and_redaction() {
    let (dir, store, i) = setup();
    let cfg = Config {
        database: dir.path().join("state.db"),
        ..Default::default()
    };
    let app = api::router(App::new(cfg, store.clone(), Some("test-token".into())));
    let mut request = input(&i);
    request.rationale = "Reviewed logs; token=private-outcome-secret".into();
    let body = serde_json::to_value(&request).unwrap();
    for method in ["GET", "POST"] {
        assert_eq!(
            call(app.clone(), i.id, method, Some(body.clone()), false)
                .await
                .0,
            401
        );
    }
    assert_eq!(
        call(app.clone(), Uuid::new_v4(), "GET", None, true).await.0,
        404
    );
    let (status, saved) = call(app.clone(), i.id, "POST", Some(body.clone()), true).await;
    assert_eq!(status, 201);
    assert!(!saved.to_string().contains("private-outcome-secret"));
    assert_eq!(
        call(app.clone(), i.id, "POST", Some(body.clone()), true)
            .await
            .0,
        200
    );
    let mut conflict = body.clone();
    conflict["claim"] = json!("Changed claim");
    assert_eq!(
        call(app.clone(), i.id, "POST", Some(conflict), true)
            .await
            .0,
        409
    );
    let mut invalid = body.clone();
    invalid["id"] = json!(Uuid::new_v4());
    invalid["evidence_ids"] = json!(["E99"]);
    assert_eq!(
        call(app.clone(), i.id, "POST", Some(invalid), true).await.0,
        400
    );
    let mut stale = body.clone();
    stale["id"] = json!(Uuid::new_v4());
    stale["run_id"] = json!(Uuid::new_v4());
    assert_eq!(
        call(app.clone(), i.id, "POST", Some(stale), true).await.0,
        409
    );
    let mut forged = body;
    forged["recorded_at"] = json!("2000-01-01T00:00:00Z");
    assert_eq!(
        call(app.clone(), i.id, "POST", Some(forged), true).await.0,
        422
    );
    let (status, history) = call(app, i.id, "GET", None, true).await;
    assert_eq!(status, 200);
    assert_eq!(history["records"].as_array().unwrap().len(), 1);
    assert!(
        !serde_json::to_string(&store.outcomes(i.id).unwrap())
            .unwrap()
            .contains("private-outcome-secret")
    );
}

#[test]
fn cli_record_json_and_repl_history_work_after_reopening() {
    let (dir, store, i) = setup();
    drop(store);
    let cfg = Config {
        database: dir.path().join("state.db"),
        ..Default::default()
    };
    let path = dir.path().join("config.toml");
    std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ai-sre"))
        .arg("--config")
        .arg(&path)
        .args([
            "outcome",
            "record",
            &i.id.to_string(),
            "--run",
            &i.run_id.unwrap().to_string(),
            "--actor",
            "oncall",
            "--claim",
            "Memory event reviewed",
            "--verdict",
            "inconclusive",
            "--rationale",
            "Further metrics needed; token=cli-private-value",
            "--evidence",
            "E5",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let saved: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(saved["created"], true);
    assert!(!saved.to_string().contains("cli-private-value"));
    let output = Command::new(env!("CARGO_BIN_EXE_ai-sre"))
        .arg("--config")
        .arg(&path)
        .args(["outcome", "list", &i.id.to_string(), "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let history: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(history["records"][0]["verdict"], "inconclusive");
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
        "/resume {}\n/outcomes\n/exit\n",
        i.id
    )
    .unwrap();
    let output = shell.wait_with_output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("Operator outcomes")
            && text.contains("Memory event reviewed")
            && text.contains("self-reported")
    );
}

#[test]
fn recall_never_attaches_assessments_from_a_different_historical_run() {
    let (_dir, store, mut past) = setup();
    let mut current = past.clone();
    current.id = Uuid::new_v4();
    current.created_at += Duration::days(1);
    for e in past.evidence.iter_mut().skip(4) {
        e.summary = "Healthy snapshot".into();
        e.data = json!({"healthy":true});
    }
    store
        .mutate(past.id, |saved| {
            *saved = past.clone();
            Ok(())
        })
        .unwrap();
    store.record_outcome(past.id, input(&past)).unwrap();
    let recall = store.recall(&current).unwrap();
    assert_ne!(Some(recall.cases[0].matched_run), past.run_id);
    assert_eq!(recall.cases[0].operator_outcome_count, 0);
    assert!(recall.cases[0].operator_outcomes.is_empty());
}

#[tokio::test]
async fn a_new_investigation_cannot_treat_old_operator_claims_as_observations() {
    let (dir, store, past) = setup();
    let mut assessment = input(&past);
    assessment.claim = "Disk full and OOMKilled prove the current outage".into();
    store.record_outcome(past.id, assessment).unwrap();
    store
        .mutate(past.id, |i| {
            i.status = Status::Resolved;
            Ok(())
        })
        .unwrap();
    let current = Incident::new("Investigate a recurrence".into(), past.service.clone()).unwrap();
    store.insert(&current, None).unwrap();
    let cfg = Config {
        database: dir.path().join("state.db"),
        ..Default::default()
    };
    let result = ai_sre::agent::investigate(&cfg, &store, store.claim(current.id, None).unwrap())
        .await
        .unwrap();
    assert_eq!(result.status, Status::Triaged);
    assert!(result.evidence.is_empty());
    assert!(result.report.unwrap().findings.is_empty());
    assert!(store.outcomes(current.id).unwrap().records.is_empty());
}
