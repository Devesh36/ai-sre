//! Explainable, service-scoped recall. Historical cases are leads, not current evidence.
use super::{Evidence, Incident, Status, review};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::BTreeMap;
use uuid::Uuid;

pub const SEARCH_LIMIT: usize = 100;
pub const RESULT_LIMIT: usize = 5;

#[derive(Debug, Serialize)]
pub struct Recall {
    pub incident_id: Uuid,
    pub incident_updated_at: DateTime<Utc>,
    pub service: String,
    pub current_run: Option<Uuid>,
    pub searched: usize,
    pub search_limit: usize,
    pub search_truncated: bool,
    pub matched_cases: usize,
    pub reason: String,
    pub cases: Vec<Case>,
    pub limitations: Vec<String>,
    pub checks: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Case {
    pub incident_id: Uuid,
    pub title: String,
    pub status: Status,
    pub updated_at: DateTime<Utc>,
    pub matched_run: Uuid,
    pub exact_matches: Vec<Match>,
    pub shared_symptoms: Vec<Match>,
    pub current_only_symptoms: Vec<String>,
    pub past_only_symptoms: Vec<String>,
    pub current_distinct_payloads: usize,
    pub past_distinct_payloads: usize,
}

#[derive(Debug, Serialize)]
pub struct Match {
    pub label: String,
    pub current_ids: Vec<String>,
    pub past_ids: Vec<String>,
}

fn snapshot<'a>(incident: &'a Incident, run: &review::RunReview) -> Option<Vec<&'a Evidence>> {
    if !run.started
        || run.outcome != "completed"
        || !run.failed_tools.is_empty()
        || incident.evidence.len() >= 128
        || run
            .evidence_ids
            .iter()
            .any(|id| !incident.evidence.iter().any(|e| &e.id == id))
    {
        return None;
    }
    Some(
        incident
            .evidence
            .iter()
            .filter(|e| {
                run.evidence_ids.contains(&e.id)
                    && review::is_observation(e)
                    && e.data.get("truncated").and_then(serde_json::Value::as_bool) != Some(true)
            })
            .collect(),
    )
}

fn payloads(evidence: &[&Evidence]) -> BTreeMap<String, (String, Vec<String>)> {
    let mut groups: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    for e in evidence {
        groups
            .entry(review::fingerprint(e))
            .or_insert_with(|| (e.source.clone(), vec![]))
            .1
            .push(e.id.clone());
    }
    groups
}

fn symptoms(evidence: &[&Evidence]) -> BTreeMap<String, Vec<String>> {
    // Use the same conservative, explicitly named rules as offline triage; never match report titles.
    crate::agent::offline_report(&evidence.iter().map(|e| (*e).clone()).collect::<Vec<_>>())
        .findings
        .into_iter()
        .map(|f| (f.title, f.evidence_ids))
        .collect()
}

/// The caller supplies candidate incidents. Filtering is repeated here so other callers cannot
/// accidentally mix services, the current incident, or cases opened later than this incident.
pub fn analyze(current: &Incident, candidates: &[Incident], now: DateTime<Utc>) -> Recall {
    let mut recall = Recall {
        incident_id: current.id, incident_updated_at: current.updated_at, service: current.service.clone(),
        current_run: current.run_id, searched: 0, search_limit: SEARCH_LIMIT, search_truncated: false,
        checks: vec![],
        matched_cases: 0, reason: String::new(), cases: vec![], limitations: vec![
            "Historical matches are investigation leads, not current evidence or proof of the same root cause. A resolved status does not verify a particular fix.".into(),
            "Exact matches compare source, summary, and retained data, excluding collection time and evidence ID. Repeated payloads count once; source labels do not establish independence.".into(),
            "Symptom matches use four explicit offline keyword rules, not semantic similarity or a probability. Review negation, time windows, query coverage, and conflicting observations.".into(),
            "Only earlier same-service cases are searched. Each match names a completed run; that run may precede the case's latest report. Context, truncated payloads, incomplete runs, and failed collection are excluded.".into(),
        ],
    };
    let current_review = review::analyze(current, now);
    let observations = current_review.runs.last().and_then(|run| {
        (Some(run.id) == current.run_id
            && matches!(current.status, Status::Triaged | Status::Resolved))
        .then(|| snapshot(current, run))
        .flatten()
    });
    let Some(observations) = observations.filter(|v| !v.is_empty()) else {
        recall.reason = "Complete a successful investigation with linked, untruncated observations before recalling past cases.".into();
        return recall;
    };
    if observations
        .iter()
        .any(|e| now.signed_duration_since(e.observed_at).num_minutes() >= 30)
    {
        recall.checks.push("Some current observations were collected at least 30 minutes ago. Refresh evidence before acting on historical matches.".into());
    }
    if observations.iter().any(|e| e.observed_at > now) {
        recall.checks.push(
            "Some current collection timestamps are in the future. Check clock accuracy.".into(),
        );
    }
    let current_payloads = payloads(&observations);
    let current_symptoms = symptoms(&observations);
    let mut candidates: Vec<_> = candidates
        .iter()
        .filter(|i| {
            i.id != current.id
                && i.service == current.service
                && i.created_at < current.created_at
                && matches!(i.status, Status::Triaged | Status::Resolved)
        })
        .collect();
    candidates.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    recall.search_truncated = candidates.len() > SEARCH_LIMIT;
    for past in candidates.into_iter().take(SEARCH_LIMIT) {
        recall.searched += 1;
        let past_review = review::analyze(past, now);
        let mut best: Option<Case> = None;
        // Newest first so equally strong snapshots prefer the most recent run.
        for run in past_review.runs.iter().rev() {
            let Some(evidence) = snapshot(past, run) else {
                continue;
            };
            let past_payloads = payloads(&evidence);
            let past_symptoms = symptoms(&evidence);
            let exact_matches: Vec<_> = current_payloads
                .iter()
                .filter_map(|(key, (source, ids))| {
                    past_payloads.get(key).map(|(_, old)| Match {
                        label: source.clone(),
                        current_ids: ids.clone(),
                        past_ids: old.clone(),
                    })
                })
                .collect();
            let shared_symptoms: Vec<_> = current_symptoms
                .iter()
                .filter_map(|(name, ids)| {
                    past_symptoms.get(name).map(|old| Match {
                        label: name.clone(),
                        current_ids: ids.clone(),
                        past_ids: old.clone(),
                    })
                })
                .collect();
            if exact_matches.is_empty() && shared_symptoms.is_empty() {
                continue;
            }
            let case = Case {
                incident_id: past.id,
                title: past.title.clone(),
                status: past.status.clone(),
                updated_at: past.updated_at,
                matched_run: run.id,
                exact_matches,
                shared_symptoms,
                current_only_symptoms: current_symptoms
                    .keys()
                    .filter(|s| !past_symptoms.contains_key(*s))
                    .cloned()
                    .collect(),
                past_only_symptoms: past_symptoms
                    .keys()
                    .filter(|s| !current_symptoms.contains_key(*s))
                    .cloned()
                    .collect(),
                current_distinct_payloads: current_payloads.len(),
                past_distinct_payloads: past_payloads.len(),
            };
            if best.as_ref().is_none_or(|b| rank(&case) > rank(b)) {
                best = Some(case);
            }
        }
        if let Some(case) = best {
            recall.cases.push(case);
        }
    }
    recall.cases.sort_by(|a, b| {
        rank(b)
            .cmp(&rank(a))
            .then_with(|| b.updated_at.cmp(&a.updated_at))
            .then_with(|| a.incident_id.cmp(&b.incident_id))
    });
    recall.matched_cases = recall.cases.len();
    recall.cases.truncate(RESULT_LIMIT);
    recall.reason = format!(
        "{} matching cases in {} earlier same-service cases searched; showing up to {RESULT_LIMIT}. Ranked by distinct exact payloads, then shared symptom rules, then update time.",
        recall.matched_cases, recall.searched
    );
    recall
}

fn rank(case: &Case) -> (usize, usize) {
    (case.exact_matches.len(), case.shared_symptoms.len())
}
