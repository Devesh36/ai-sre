//! Offline replay of explicit fixture expectations. Assessment text is never detector input.
use crate::{
    agent,
    domain::{
        Evidence, Incident, Report,
        outcome::{self, Outcome, Verdict},
        review,
    },
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::Path,
};
use uuid::Uuid;

pub const MAX_BYTES: usize = 2 * 1024 * 1024;
pub const SCOPE: &str = "Offline symptom-detector replay against declared fixture expectations. Expected operator judgments are separate from observed output; inconclusive judgments are unscored. Matching a label does not prove a cause, fix, or recovery. No model, connector, database, or network is used. Fingerprints identify content, not authentic authorship.";
pub const PRESENTATION: &str = "Presentation strings use credential-pattern redaction. Fingerprints and scoring cover original decoded content, before redaction. Unrecognized sensitive text may remain; prepare fixtures before sharing. No environment secrets are read.";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Synthetic,
    LocalFixture,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dataset {
    pub schema_version: u32,
    pub name: String,
    pub origin: Origin,
    pub cases: Vec<Case>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    pub incident: Incident,
    pub assessments: Vec<Outcome>,
    pub expected: Vec<Expectation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expectation {
    pub finding: String,
    pub assessment_id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Passed,
    Failed,
    Unscored,
}

#[derive(Debug, Serialize)]
pub struct Check {
    pub finding: String,
    pub assessment_id: Uuid,
    pub expected_verdict: Verdict,
    pub expected_evidence_ids: Vec<String>,
    pub observed_evidence_ids: Vec<String>,
    pub state: State,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct CaseResult {
    pub name: String,
    pub repetition: usize,
    pub input_sha256: String,
    pub expected_sha256: String,
    pub evidence: Vec<outcome::Reference>,
    pub expected: Vec<Expectation>,
    pub operator_assessments: Vec<Outcome>,
    pub observed: Report,
    pub observed_sha256: String,
    pub checks: Vec<Check>,
    pub unexpected_findings: Vec<String>,
    pub state: State,
}

#[derive(Debug, Serialize)]
pub struct Evaluation {
    pub schema_version: u32,
    pub suite: &'static str,
    pub dataset_name: String,
    pub origin: Origin,
    pub dataset_sha256: String,
    pub implementation_sha256: &'static str,
    pub product_version: &'static str,
    pub repetitions: usize,
    pub passed: bool,
    pub passed_cases: usize,
    pub failed_cases: usize,
    pub unscored_cases: usize,
    pub unscored_judgments: usize,
    pub cases: Vec<CaseResult>,
    pub scope: &'static str,
    pub presentation: &'static str,
}

fn presentation<T: Serialize + serde::de::DeserializeOwned>(value: &T) -> Result<T> {
    fn redact(value: &mut Value) {
        match value {
            Value::String(s) => *s = crate::core::tool::registry::redact(s),
            Value::Array(a) => a.iter_mut().for_each(redact),
            Value::Object(o) => o.values_mut().for_each(redact),
            _ => {}
        }
    }
    let mut value = serde_json::to_value(value)?;
    redact(&mut value);
    Ok(serde_json::from_value(value)?)
}

fn digest(value: &impl Serialize) -> Result<String> {
    // Round-trip through Value so nested object keys are canonical; array order is retained.
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&serde_json::to_value(value)?)?)
    ))
}

pub fn read(path: &Path) -> Result<Dataset> {
    ensure!(
        std::fs::metadata(path)?.is_file(),
        "replay dataset must be a regular local JSON file"
    );
    let mut data = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut data)?;
    ensure!(data.len() <= MAX_BYTES, "replay dataset exceeds 2 MiB");
    serde_json::from_slice(&data).map_err(|e| {
        anyhow::anyhow!(
            "invalid replay dataset JSON/schema at line {} column {}",
            e.line(),
            e.column()
        )
    })
}

fn valid_name(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 128
}
fn known_finding(value: &str) -> bool {
    matches!(
        value,
        "Memory pressure detected"
            | "Dependency connectivity failure"
            | "Rate limiting observed"
            | "Storage capacity exhausted"
    )
}

fn inputs(case: &Case) -> Result<Vec<Evidence>> {
    ensure!(valid_name(&case.name), "case name must contain 1–128 bytes");
    ensure!(
        (1..=100).contains(&case.assessments.len()),
        "case requires 1–100 assessment records"
    );
    ensure!(
        (1..=4).contains(&case.expected.len()),
        "case requires 1–4 explicit finding expectations"
    );
    let i = &case.incident;
    ensure!(
        i.run_id
            .is_some_and(|id| review::successful_lifecycle(i, id)),
        "latest run requires one ordered start and final completion without failure or cancellation"
    );
    ensure!(
        !i.id.is_nil()
            && !i.service.trim().is_empty()
            && i.service.len() <= 128
            && !i.title.trim().is_empty()
            && i.title.len() <= 4000,
        "invalid incident identity or title"
    );
    ensure!(
        (1..128).contains(&i.evidence.len()) && i.trace.len() <= 4096,
        "incident evidence or trace exceeds replay bounds"
    );
    let mut evidence_ids = BTreeSet::new();
    for e in &i.evidence {
        ensure!(
            !e.id.is_empty() && e.id.len() <= 32 && evidence_ids.insert(&e.id),
            "evidence IDs must be nonempty, unique, and at most 32 bytes"
        );
        ensure!(
            e.source.len() <= 4096
                && e.summary.len() <= 4096
                && serde_json::to_vec(&e.data)?.len() <= 16384,
            "evidence exceeds replay field bounds"
        );
    }
    let records: BTreeMap<_, _> = case.assessments.iter().map(|o| (o.id, o)).collect();
    ensure!(
        records.len() == case.assessments.len(),
        "duplicate assessment ID"
    );
    let mut superseded = BTreeSet::new();
    for o in &case.assessments {
        ensure!(
            o.schema_version == 1
                && o.incident_id == i.id
                && o.service == i.service
                && Some(o.run_id) == i.run_id,
            "assessment must identify this incident and its latest run"
        );
        let mut input = outcome::Input {
            id: o.id,
            run_id: o.run_id,
            actor: o.actor.clone(),
            claim: o.claim.clone(),
            verdict: o.verdict,
            rationale: o.rationale.clone(),
            evidence_ids: o.evidence.iter().map(|e| e.id.clone()).collect(),
            supersedes: o.supersedes,
        };
        input.normalize()?;
        // Reuse recording-time eligibility and citation validation. No record is written.
        let checked = outcome::create(i, input)?;
        let declared: BTreeMap<_, _> = o.evidence.iter().map(|e| (&e.id, e)).collect();
        for e in &checked.evidence {
            let original = declared[&e.id];
            ensure!(
                original.source == e.source
                    && original.observed_at == e.observed_at
                    && original.payload_sha256 == e.payload_sha256,
                "assessment evidence fingerprint or collection metadata mismatch"
            );
        }
        if let Some(parent) = o.supersedes {
            ensure!(
                records.contains_key(&parent) && superseded.insert(parent),
                "correction parent is missing or has multiple successors"
            );
            let mut seen = BTreeSet::from([o.id]);
            let mut cursor = Some(parent);
            while let Some(id) = cursor {
                ensure!(seen.insert(id), "assessment correction cycle");
                cursor = records
                    .get(&id)
                    .ok_or_else(|| anyhow::anyhow!("correction parent missing"))?
                    .supersedes;
            }
        }
    }
    let mut targets = BTreeSet::new();
    for expectation in &case.expected {
        ensure!(
            known_finding(&expectation.finding) && targets.insert(&expectation.finding),
            "expectation must name a distinct supported offline finding title"
        );
        ensure!(
            records.contains_key(&expectation.assessment_id)
                && !superseded.contains(&expectation.assessment_id),
            "expectation must reference a retained non-superseded assessment"
        );
    }
    let run = review::analyze(i, i.updated_at)
        .runs
        .pop()
        .ok_or_else(|| anyhow::anyhow!("run trace missing"))?;
    let selected: Vec<_> = i
        .evidence
        .iter()
        .filter(|e| run.evidence_ids.contains(&e.id))
        .cloned()
        .collect();
    ensure!(
        selected
            .iter()
            .all(|e| e.data.get("truncated").and_then(Value::as_bool) != Some(true)),
        "replay requires complete, untruncated run inputs"
    );
    Ok(selected)
}

pub fn evaluate(dataset: &Dataset, repetitions: usize) -> Result<Evaluation> {
    ensure!(
        dataset.schema_version == 1,
        "unsupported replay schema version"
    );
    ensure!(
        valid_name(&dataset.name) && (1..=16).contains(&dataset.cases.len()),
        "dataset requires a name and 1–16 cases"
    );
    ensure!((1..=10).contains(&repetitions), "repeat must be 1–10");
    ensure!(
        serde_json::to_vec(dataset)?.len() <= MAX_BYTES,
        "replay dataset exceeds 2 MiB"
    );
    let mut names = BTreeSet::new();
    let mut record_ids = BTreeSet::new();
    let mut prepared = vec![];
    // Validate the whole dataset before running or writing any report.
    for (index, case) in dataset.cases.iter().enumerate() {
        ensure!(names.insert(&case.name), "duplicate replay case name");
        for o in &case.assessments {
            ensure!(
                record_ids.insert(o.id),
                "assessment IDs must be unique across cases"
            );
        }
        prepared.push(inputs(case).with_context(|| format!("invalid replay case {}", index + 1))?);
    }
    let mut cases = vec![];
    for repetition in 1..=repetitions {
        for (case, evidence) in dataset.cases.iter().zip(&prepared) {
            let observed = agent::offline_report(evidence);
            agent::validate_report(&observed, evidence)?;
            let mut checks = vec![];
            for expected in &case.expected {
                let o = case
                    .assessments
                    .iter()
                    .find(|o| o.id == expected.assessment_id)
                    .expect("validated assessment");
                let found = observed
                    .findings
                    .iter()
                    .find(|f| f.title == expected.finding);
                let mut ids: Vec<_> = o.evidence.iter().map(|e| e.id.clone()).collect();
                ids.sort();
                let (state, reason) = match o.verdict {
                    Verdict::Inconclusive => (
                        State::Unscored,
                        "Inconclusive operator judgment; no pass or fail inferred.",
                    ),
                    Verdict::Supported
                        if found
                            .is_some_and(|f| ids.iter().all(|id| f.evidence_ids.contains(id))) =>
                    {
                        (
                            State::Passed,
                            "Expected finding and cited observations were emitted.",
                        )
                    }
                    Verdict::Supported => (
                        State::Failed,
                        "Expected finding or its cited observations were absent.",
                    ),
                    Verdict::Refuted if found.is_none() => (
                        State::Passed,
                        "Fixture expected this finding to be absent; detector abstained on it.",
                    ),
                    Verdict::Refuted => (
                        State::Failed,
                        "Detector emitted a finding the fixture expected to be absent.",
                    ),
                };
                checks.push(Check {
                    finding: expected.finding.clone(),
                    assessment_id: o.id,
                    expected_verdict: o.verdict,
                    expected_evidence_ids: presentation(&ids)?,
                    observed_evidence_ids: presentation(
                        &found.map(|f| f.evidence_ids.clone()).unwrap_or_default(),
                    )?,
                    state,
                    reason: reason.into(),
                });
            }
            let unexpected_findings: Vec<_> = observed
                .findings
                .iter()
                .filter(|f| !case.expected.iter().any(|e| e.finding == f.title))
                .map(|f| f.title.clone())
                .collect();
            let state = if !unexpected_findings.is_empty()
                || checks.iter().any(|c| c.state == State::Failed)
            {
                State::Failed
            } else if checks.iter().any(|c| c.state == State::Passed) {
                State::Passed
            } else {
                State::Unscored
            };
            cases.push(CaseResult {
                name: crate::core::tool::registry::redact(&case.name),
                repetition,
                input_sha256: digest(evidence)?,
                expected_sha256: digest(
                    &json!({"assessments":case.assessments,"expectations":case.expected}),
                )?,
                evidence: presentation(
                    &evidence
                        .iter()
                        .map(|e| outcome::Reference {
                            id: e.id.clone(),
                            source: e.source.clone(),
                            observed_at: e.observed_at,
                            payload_sha256: review::fingerprint(e),
                        })
                        .collect::<Vec<_>>(),
                )?,
                expected: case.expected.clone(),
                operator_assessments: presentation(&case.assessments)?,
                observed_sha256: digest(&observed)?,
                observed: presentation(&observed)?,
                checks,
                unexpected_findings,
                state,
            });
        }
    }
    let passed_cases = cases.iter().filter(|c| c.state == State::Passed).count();
    let failed_cases = cases.iter().filter(|c| c.state == State::Failed).count();
    let unscored_cases = cases.iter().filter(|c| c.state == State::Unscored).count();
    let unscored_judgments = cases
        .iter()
        .flat_map(|c| &c.checks)
        .filter(|c| c.state == State::Unscored)
        .count();
    Ok(Evaluation {
        schema_version: 1,
        suite: "offline-replay-v1",
        dataset_name: crate::core::tool::registry::redact(&dataset.name),
        origin: dataset.origin,
        dataset_sha256: digest(dataset)?,
        implementation_sha256: env!("AI_SRE_IMPLEMENTATION_SHA256"),
        product_version: env!("CARGO_PKG_VERSION"),
        repetitions,
        passed: failed_cases == 0 && passed_cases > 0,
        passed_cases,
        failed_cases,
        unscored_cases,
        unscored_judgments,
        cases,
        scope: SCOPE,
        presentation: PRESENTATION,
    })
}
