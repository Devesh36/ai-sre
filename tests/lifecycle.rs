use ai_sre::{
    domain::{Incident, Status, outcome::Input, recall},
    infrastructure::evaluation::replay::{self, Dataset},
    store::Store,
};
use chrono::Duration;
use serde_json::to_value;
use uuid::Uuid;

fn dataset() -> Dataset {
    let mut d: Dataset =
        serde_json::from_str(include_str!("../examples/replay-cases.json")).unwrap();
    d.cases.truncate(1);
    d
}

fn request(d: &Dataset) -> Input {
    let o = &d.cases[0].assessments[0];
    Input {
        id: Uuid::new_v4(),
        run_id: o.run_id,
        actor: o.actor.clone(),
        claim: o.claim.clone(),
        verdict: o.verdict,
        rationale: o.rationale.clone(),
        evidence_ids: o.evidence.iter().map(|e| e.id.clone()).collect(),
        supersedes: None,
    }
}

fn corrupt(i: &mut Incident, variant: &str) {
    let trace = &mut i.trace;
    match variant {
        "reversed" => trace.reverse(),
        "failed" | "cancelled" => {
            let mut event = trace[2].clone();
            event.phase = format!("run_{variant}");
            trace.insert(2, event);
        }
        "late_collection" => trace.swap(1, 2),
        "duplicate_start" => trace.insert(1, trace[0].clone()),
        "duplicate_end" => trace.push(trace[2].clone()),
        _ => unreachable!(),
    }
}

#[test]
fn malformed_lifecycles_are_rejected_consistently_without_changing_history() {
    for variant in [
        "reversed",
        "failed",
        "cancelled",
        "late_collection",
        "duplicate_start",
        "duplicate_end",
    ] {
        let mut d = dataset();
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.db")).unwrap();
        let i = &d.cases[0].incident;
        store.insert(i, None).unwrap();
        let original_request = request(&d);
        let (original, _) = store
            .record_outcome(i.id, original_request.clone())
            .unwrap();
        let history = to_value(store.outcomes(i.id).unwrap()).unwrap();
        let mut next = request(&d);
        next.supersedes = Some(original.id);
        corrupt(&mut d.cases[0].incident, variant);
        let invalid = &d.cases[0].incident;
        store
            .mutate(invalid.id, |saved| {
                *saved = invalid.clone();
                Ok(())
            })
            .unwrap();
        let before = to_value(store.get(invalid.id).unwrap()).unwrap();
        assert!(store.record_outcome(invalid.id, next).is_err(), "{variant}");
        assert_eq!(
            to_value(store.outcomes(invalid.id).unwrap()).unwrap(),
            history,
            "{variant}"
        );
        assert_eq!(
            to_value(store.get(invalid.id).unwrap()).unwrap(),
            before,
            "{variant}"
        );
        // An existing receipt remains an idempotent receipt, not a fresh validation.
        let (receipt, created) = store.record_outcome(invalid.id, original_request).unwrap();
        assert!(!created);
        assert_eq!(to_value(receipt).unwrap(), to_value(&original).unwrap());
        assert!(replay::evaluate(&d, 1).is_err(), "{variant}");

        let mut valid = dataset().cases.remove(0).incident;
        valid.id = Uuid::new_v4();
        valid.created_at -= Duration::days(1);
        assert!(
            recall::analyze(invalid, &[valid.clone()], invalid.updated_at)
                .cases
                .is_empty(),
            "invalid current {variant}"
        );
        valid.created_at += Duration::days(2);
        assert!(
            recall::analyze(&valid, std::slice::from_ref(invalid), valid.updated_at)
                .cases
                .is_empty(),
            "invalid past {variant}"
        );
    }
}

#[test]
fn earlier_successful_runs_remain_recallable_after_failed_or_cancelled_attempts() {
    for status in [Status::Failed, Status::Cancelled] {
        let d = dataset();
        let mut past = d.cases[0].incident.clone();
        let mut current = past.clone();
        current.id = Uuid::new_v4();
        current.created_at += Duration::days(1);
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.db")).unwrap();
        store.insert(&past, None).unwrap();
        let (assessment, _) = store.record_outcome(past.id, request(&d)).unwrap();
        let earlier = past.run_id.unwrap();
        let later = Uuid::new_v4();
        let mut events = past.trace.clone();
        for event in &mut events {
            event.run_id = Some(later);
        }
        events[2].phase = if status == Status::Failed {
            "run_failed"
        } else {
            "run_cancelled"
        }
        .into();
        past.trace.extend(events);
        past.run_id = Some(later);
        past.status = status.clone();
        let mut misordered = past.clone();
        misordered.trace.rotate_left(3);
        assert!(
            recall::analyze(&current, &[misordered], current.updated_at)
                .cases
                .is_empty()
        );
        store
            .mutate(past.id, |saved| {
                *saved = past.clone();
                Ok(())
            })
            .unwrap();
        let before = to_value(&current).unwrap();
        let pure = recall::analyze(&current, std::slice::from_ref(&past), current.updated_at);
        assert_eq!(pure.cases.len(), 1);
        assert_eq!(pure.cases[0].matched_run, earlier);
        let persisted = store.recall(&current).unwrap();
        assert_eq!(persisted.cases.len(), 1);
        assert_eq!(persisted.cases[0].matched_run, earlier);
        assert_eq!(persisted.cases[0].operator_outcomes[0].id, assessment.id);
        assert_eq!(to_value(&current).unwrap(), before);
        // A failed current run must never fall back to its earlier successful run.
        let mut older = current.clone();
        older.created_at = past.created_at - Duration::days(1);
        assert!(
            recall::analyze(&past, &[older], past.updated_at)
                .cases
                .is_empty()
        );
        // With the sole successful run removed, failed-run observations cannot match.
        past.trace.retain(|t| t.run_id == Some(later));
        assert!(
            recall::analyze(&current, &[past], current.updated_at)
                .cases
                .is_empty()
        );
        // A successful-looking trace cannot override a failed/cancelled current status.
        let mut contradictory = d.cases[0].incident.clone();
        contradictory.status = status;
        for run_id in [
            contradictory.run_id,
            None,
            Some(Uuid::nil()),
            Some(Uuid::new_v4()),
        ] {
            contradictory.run_id = run_id;
            assert!(
                recall::analyze(
                    &current,
                    std::slice::from_ref(&contradictory),
                    current.updated_at
                )
                .cases
                .is_empty()
            );
        }
    }
}

#[test]
fn invalid_historical_runs_do_not_invalidate_a_valid_current_run() {
    let mut d = dataset();
    let case = &mut d.cases[0];
    let mut historical = case.incident.clone();
    corrupt(&mut historical, "reversed");
    let old_id = Uuid::new_v4();
    for event in &mut historical.trace {
        event.run_id = Some(old_id);
    }
    historical.trace.extend(case.incident.trace.clone());
    case.incident.trace = historical.trace;
    assert!(replay::evaluate(&d, 1).unwrap().passed);
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("state.db")).unwrap();
    store.insert(&d.cases[0].incident, None).unwrap();
    assert!(
        store
            .record_outcome(d.cases[0].incident.id, request(&d))
            .unwrap()
            .1
    );
}
