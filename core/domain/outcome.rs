//! Human-recorded assessments; these are not autonomous verification or current-incident proof.
use super::{Incident, Status, review};
use anyhow::{Result, ensure};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fmt};
use uuid::Uuid;

pub const LIMIT: usize = 100;
pub const NOTICE: &str = "Operator assessments are human assertions tied to a specific incident and run. Actor names are self-reported, not authenticated identities. These records do not independently verify a cause, fix, or recovery, and never establish facts about another incident.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Supported,
    Refuted,
    Inconclusive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub id: Uuid,
    pub run_id: Uuid,
    pub actor: String,
    pub claim: String,
    pub verdict: Verdict,
    pub rationale: String,
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub supersedes: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    pub id: String,
    pub source: String,
    pub observed_at: DateTime<Utc>,
    pub payload_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Outcome {
    pub schema_version: u32,
    pub id: Uuid,
    pub incident_id: Uuid,
    pub service: String,
    pub run_id: Uuid,
    pub incident_updated_at: DateTime<Utc>,
    pub recorded_at: DateTime<Utc>,
    pub actor: String,
    pub claim: String,
    pub verdict: Verdict,
    pub rationale: String,
    pub evidence: Vec<Reference>,
    pub supersedes: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct History {
    pub incident_id: Uuid,
    pub limit: usize,
    pub records: Vec<Outcome>,
    pub notice: &'static str,
}

#[derive(Debug)]
pub enum Error {
    NotFound,
    Invalid(&'static str),
    Conflict(&'static str),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotFound => "incident not found",
            Self::Invalid(s) | Self::Conflict(s) => s,
        })
    }
}
impl std::error::Error for Error {}

impl Input {
    pub fn redact(&mut self, redact: impl Fn(&str) -> String) {
        self.actor = redact(&self.actor);
        self.claim = redact(&self.claim);
        self.rationale = redact(&self.rationale);
    }

    pub fn normalize(&mut self) -> Result<()> {
        ensure!(
            !self.id.is_nil() && !self.run_id.is_nil(),
            Error::Invalid("record and run IDs must not be nil")
        );
        for (value, limit) in [
            (&mut self.actor, 128),
            (&mut self.claim, 1000),
            (&mut self.rationale, 4000),
        ] {
            *value = value.trim().to_owned();
            ensure!(
                !value.is_empty() && value.len() <= limit,
                Error::Invalid(
                    "actor, claim, and rationale are required; byte limits are 128, 1000, and 4000"
                )
            );
        }
        ensure!(
            (1..=16).contains(&self.evidence_ids.len()),
            Error::Invalid("cite 1–16 observation IDs")
        );
        let ids: BTreeSet<_> = self.evidence_ids.iter().collect();
        ensure!(
            ids.len() == self.evidence_ids.len()
                && ids.iter().all(|id| !id.is_empty() && id.len() <= 32),
            Error::Invalid("evidence IDs must be distinct and contain 1–32 bytes")
        );
        ensure!(
            self.supersedes != Some(self.id),
            Error::Invalid("an assessment cannot supersede itself")
        );
        self.evidence_ids.sort();
        Ok(())
    }
}

impl Outcome {
    pub fn same_request(&self, incident_id: Uuid, input: &Input) -> bool {
        self.incident_id == incident_id
            && self.id == input.id
            && self.run_id == input.run_id
            && self.actor == input.actor
            && self.claim == input.claim
            && self.verdict == input.verdict
            && self.rationale == input.rationale
            && self.supersedes == input.supersedes
            && self
                .evidence
                .iter()
                .map(|e| &e.id)
                .eq(input.evidence_ids.iter())
    }
}

pub(crate) fn create(incident: &Incident, input: Input) -> Result<Outcome> {
    ensure!(
        matches!(incident.status, Status::Triaged | Status::Resolved)
            && incident.run_id == Some(input.run_id),
        Error::Conflict(
            "assessment requires the latest completed run; reload the incident and review its evidence"
        )
    );
    let review = review::analyze(incident, Utc::now());
    let run = review
        .runs
        .last()
        .filter(|r| {
            r.id == input.run_id
                && r.started
                && r.outcome == "completed"
                && r.failed_tools.is_empty()
        })
        .ok_or(Error::Conflict(
            "assessment requires a complete successful trace without failed collection",
        ))?;
    ensure!(
        incident.evidence.len() < 128
            && run
                .evidence_ids
                .iter()
                .all(|id| incident.evidence.iter().any(|e| &e.id == id)),
        Error::Conflict("assessment withheld because retained evidence is incomplete or capped")
    );
    let mut evidence = vec![];
    for id in &input.evidence_ids {
        let e = incident
            .evidence
            .iter()
            .find(|e| {
                &e.id == id
                    && run.evidence_ids.contains(id)
                    && review::is_observation(e)
                    && e.data.get("truncated").and_then(serde_json::Value::as_bool) != Some(true)
            })
            .ok_or(Error::Invalid(
                "citations must be retained, untruncated observations from the specified run",
            ))?;
        evidence.push(Reference {
            id: e.id.clone(),
            source: e.source.clone(),
            observed_at: e.observed_at,
            payload_sha256: review::fingerprint(e),
        });
    }
    Ok(Outcome {
        schema_version: 1,
        id: input.id,
        incident_id: incident.id,
        service: incident.service.clone(),
        run_id: input.run_id,
        incident_updated_at: incident.updated_at,
        recorded_at: Utc::now(),
        actor: input.actor,
        claim: input.claim,
        verdict: input.verdict,
        rationale: input.rationale,
        evidence,
        supersedes: input.supersedes,
    })
}
