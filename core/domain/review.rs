//! Deterministic review of retained evidence. Never treats missing data as recovery.
use super::{Evidence, Incident, Status};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct Review {
    pub incident_id: Uuid,
    pub generated_at: DateTime<Utc>,
    pub incident_updated_at: DateTime<Utc>,
    pub freshness_window_minutes: u32,
    pub observations: usize,
    pub distinct_payloads: usize,
    pub repeated_payloads: usize,
    pub context_items: usize,
    pub runs: Vec<RunReview>,
    pub changes: Changes,
    pub findings: Vec<FindingReview>,
    pub checks: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct RunReview {
    pub id: Uuid,
    pub outcome: String,
    pub started: bool,
    pub evidence_ids: Vec<String>,
    pub failed_tools: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct Changes {
    pub baseline_run: Option<Uuid>,
    pub current_run: Option<Uuid>,
    pub available: bool,
    pub reason: String,
    pub added: Vec<String>,
    pub not_repeated: Vec<String>,
    pub repeated: Vec<Repeated>,
}

#[derive(Debug, Serialize)]
pub struct Repeated {
    pub before: Vec<String>,
    pub after: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct FindingReview {
    pub title: String,
    pub citations: Vec<String>,
    pub current_observations: Vec<String>,
    pub older_or_unlinked_observations: Vec<String>,
    pub context_citations: Vec<String>,
    pub missing_citations: Vec<String>,
    pub aged_citations: Vec<String>,
    pub future_dated_citations: Vec<String>,
    pub distinct_payloads: usize,
    pub source_count: usize,
    pub concerns: Vec<String>,
}

pub fn is_observation(e: &Evidence) -> bool {
    !matches!(
        e.source.as_str(),
        "runbook" | "operator_context" | "action_proposal"
    )
}

pub(crate) fn fingerprint(e: &Evidence) -> String {
    // serde_json's ordered maps canonicalize nested object keys. Array order is significant.
    format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&(&e.source, &e.summary, &e.data)).expect("serializable evidence")
        )
    )
}

fn groups<'a>(evidence: impl Iterator<Item = &'a Evidence>) -> BTreeMap<String, Vec<String>> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for e in evidence.filter(|e| is_observation(e)) {
        groups.entry(fingerprint(e)).or_default().push(e.id.clone());
    }
    groups
}

pub fn analyze(incident: &Incident, now: DateTime<Utc>) -> Review {
    let mut runs: Vec<RunReview> = vec![];
    for trace in &incident.trace {
        let Some(id) = trace.run_id else {
            continue;
        };
        let index = runs.iter().position(|r| r.id == id).unwrap_or_else(|| {
            runs.push(RunReview {
                id,
                outcome: "incomplete".into(),
                started: false,
                evidence_ids: vec![],
                failed_tools: vec![],
            });
            runs.len() - 1
        });
        let run = &mut runs[index];
        match trace.phase.as_str() {
            "run_started" => run.started = true,
            "run_completed" => run.outcome = "completed".into(),
            "run_failed" => run.outcome = "failed".into(),
            "run_cancelled" => run.outcome = "cancelled".into(),
            "tool_completed" => {
                for id in &trace.evidence_ids {
                    if !run.evidence_ids.contains(id) {
                        run.evidence_ids.push(id.clone());
                    }
                }
            }
            "tool_failed" | "tool_rejected" | "tool_circuit_open" => {
                if !run.failed_tools.contains(&trace.operation) {
                    run.failed_tools.push(trace.operation.clone());
                }
            }
            _ => {}
        }
    }
    // A just-claimed run may not yet have a start trace. Never compare the previous run
    // as if it were current during that window.
    if let Some(id) = incident.run_id
        && !runs.iter().any(|r| r.id == id)
    {
        runs.push(RunReview {
            id,
            outcome: "incomplete".into(),
            started: false,
            evidence_ids: vec![],
            failed_tools: vec![],
        });
    }
    let current = runs.last();
    let current_ids: BTreeSet<&str> = current
        .into_iter()
        .flat_map(|r| &r.evidence_ids)
        .map(String::as_str)
        .collect();
    let all_groups = groups(incident.evidence.iter());
    let observations = incident
        .evidence
        .iter()
        .filter(|e| is_observation(e))
        .count();
    let mut review = Review {
        incident_id: incident.id, generated_at: now, incident_updated_at: incident.updated_at, freshness_window_minutes: 30,
        observations, distinct_payloads: all_groups.len(), repeated_payloads: observations.saturating_sub(all_groups.len()),
        context_items: incident.evidence.len()-observations,
        changes: Changes::default(), findings: vec![], checks: vec![], limitations: vec![
            "Exact retained payloads are compared using source, summary, and data; collection times and evidence IDs are excluded. Repetition does not establish independent corroboration.".into(),
            "Not repeated means absent from this run's retained observations. Queries, windows, and source coverage may differ; it does not establish recovery or deletion.".into(),
            "Freshness measures local collection age (30-minute review window), not the age of events inside a provider response. Distinct source labels do not prove source independence.".into(),
        ], runs: vec![],
    };
    if let Some(current) = current {
        review.changes.current_run = Some(current.id);
        if !current.failed_tools.is_empty() {
            review.checks.push(format!(
                "Review failed, rejected, or circuit-blocked tool attempts before relying on collection: {}.",
                current.failed_tools.join(", ")
            ));
        }
    }
    if runs.len() < 2 {
        review.changes.reason = "Two recorded runs are required for comparison.".into();
    } else {
        let before = &runs[runs.len() - 2];
        let after = &runs[runs.len() - 1];
        review.changes.baseline_run = Some(before.id);
        let complete = |run: &RunReview| {
            run.started
                && run.outcome == "completed"
                && run.failed_tools.is_empty()
                && run
                    .evidence_ids
                    .iter()
                    .all(|id| incident.evidence.iter().any(|e| &e.id == id))
        };
        if !complete(before)
            || !complete(after)
            || incident.status == Status::Investigating
            || incident.evidence.len() >= 128
        {
            review.changes.reason = "Comparison withheld: a run is incomplete, collection failed, trace evidence is missing, or the evidence retention limit was reached.".into();
        } else {
            let previous = groups(
                incident
                    .evidence
                    .iter()
                    .filter(|e| before.evidence_ids.contains(&e.id)),
            );
            let latest = groups(
                incident
                    .evidence
                    .iter()
                    .filter(|e| after.evidence_ids.contains(&e.id)),
            );
            for (key, ids) in &latest {
                if let Some(old) = previous.get(key) {
                    review.changes.repeated.push(Repeated {
                        before: old.clone(),
                        after: ids.clone(),
                    });
                } else {
                    review.changes.added.extend(ids.clone());
                }
            }
            for (key, ids) in &previous {
                if !latest.contains_key(key) {
                    review.changes.not_repeated.extend(ids.clone());
                }
            }
            review.changes.available = true;
            review.changes.reason =
                "Comparison of the last two recorded runs; context and proposals are excluded."
                    .into();
        }
    }
    if incident
        .evidence
        .iter()
        .any(|e| e.data.get("truncated").and_then(serde_json::Value::as_bool) == Some(true))
    {
        review.limitations.push("Some evidence payloads were truncated before storage. Equal retained excerpts cannot establish equality of the original responses.".into());
    }
    if current.is_none() || current.is_some_and(|r| !r.started || r.outcome != "completed") {
        review.checks.push("Complete a new investigation before relying on the retained report; the latest run has no complete success trace.".into());
    }
    for finding in incident.report.iter().flat_map(|r| &r.findings) {
        let ids: BTreeSet<&str> = finding.evidence_ids.iter().map(String::as_str).collect();
        let evidence: Vec<&Evidence> = incident
            .evidence
            .iter()
            .filter(|e| ids.contains(e.id.as_str()))
            .collect();
        let mut f = FindingReview {
            title: finding.title.clone(),
            citations: ids.iter().map(|s| s.to_string()).collect(),
            current_observations: vec![],
            older_or_unlinked_observations: vec![],
            context_citations: vec![],
            missing_citations: vec![],
            aged_citations: vec![],
            future_dated_citations: vec![],
            distinct_payloads: groups(evidence.iter().copied()).len(),
            source_count: evidence
                .iter()
                .filter(|e| is_observation(e))
                .map(|e| &e.source)
                .collect::<BTreeSet<_>>()
                .len(),
            concerns: vec![],
        };
        for id in &ids {
            if !evidence.iter().any(|e| e.id == *id) {
                f.missing_citations.push((*id).into());
            }
        }
        for e in evidence {
            if !is_observation(e) {
                f.context_citations.push(e.id.clone());
                continue;
            }
            if current_ids.contains(e.id.as_str()) {
                f.current_observations.push(e.id.clone());
            } else {
                f.older_or_unlinked_observations.push(e.id.clone());
            }
            let age = now.signed_duration_since(e.observed_at).num_seconds();
            if age < 0 {
                f.future_dated_citations.push(e.id.clone());
            } else if age > i64::from(review.freshness_window_minutes) * 60 {
                f.aged_citations.push(e.id.clone());
            }
        }
        if f.current_observations.is_empty() {
            f.concerns.push("No cited observation is linked to the latest run. Recollect the supporting sources.".into());
        }
        if !f.missing_citations.is_empty() {
            f.concerns
                .push("Some citations cannot be found in retained evidence.".into());
        }
        if f.current_observations.len() + f.older_or_unlinked_observations.len()
            > f.distinct_payloads
        {
            f.concerns.push("Repeated payloads support this finding; citation count overstates the number of distinct observations.".into());
        }
        if f.source_count < 2 {
            f.concerns.push("Seek a corroborating or contradicting observation from another source before treating this as a cause.".into());
        }
        if !f.aged_citations.is_empty() {
            f.concerns.push("Some supporting observations were collected more than 30 minutes ago. Refresh them before acting.".into());
        }
        if !f.future_dated_citations.is_empty() {
            f.concerns
                .push("Some collection timestamps are in the future; check clock accuracy.".into());
        }
        review.findings.push(f);
    }
    if review.repeated_payloads > 0 {
        review.checks.push("Seek new corroborating or contradicting evidence; collecting identical payloads again does not strengthen a causal claim.".into());
    }
    if observations == 0 {
        review.checks.push("Collect service observations. Runbooks, notes, and action proposals alone do not establish an incident symptom.".into());
    }
    if review.findings.is_empty() && observations > 0 {
        review.checks.push("Review the retained observations and complete an investigation report; no findings are available to audit.".into());
    }
    review.runs = runs;
    review
}

/// Escape untrusted fields for an ordinary Markdown renderer. No raw tool payloads.
fn md(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\\' | '`' | '*' | '_' | '[' | ']' | '(' | ')' | '#' | '|' | '!' => {
                out.push('\\');
                out.push(ch);
            }
            c if c.is_control()
                || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') =>
            {
                out.push(' ')
            }
            c => out.push(c),
        }
    }
    out
}

pub fn handoff(incident: &Incident, review: &Review) -> String {
    let mut out = format!(
        "# Incident handoff: {}\n\nService: **{}** · Status: **{:?}**\n\nIncident: `{}` · Generated: {}\n\n",
        md(&incident.title),
        md(&incident.service),
        incident.status,
        incident.id,
        review.generated_at.to_rfc3339()
    );
    out.push_str("## Situation\n\n");
    if let Some(report) = &incident.report {
        out.push_str(&md(&report.summary));
        out.push_str("\n\n");
        if (incident.status != Status::Triaged && incident.status != Status::Resolved)
            || review.runs.last().is_none_or(|r| r.outcome != "completed")
        {
            out.push_str("This is a retained report; the current investigation has not completed successfully.\n\n");
        }
        out.push_str("## Findings and evidence checks\n\n");
        for (finding, audit) in report.findings.iter().zip(&review.findings) {
            out.push_str(&format!(
                "### {}\n\n{}\n\nSupport: {:?} · Citations: {}\n\n",
                md(&finding.title),
                md(&finding.explanation),
                finding.confidence,
                md(&finding.evidence_ids.join(", "))
            ));
            for concern in &audit.concerns {
                out.push_str(&format!("- {}\n", md(concern)));
            }
            out.push('\n');
        }
        out.push_str("## Suggested next steps (operator review)\n\n");
        for step in &report.next_steps {
            out.push_str(&format!("- {}\n", md(step)));
        }
        out.push_str("\n## Reported unknowns\n\n");
        for gap in &report.gaps {
            out.push_str(&format!("- {}\n", md(gap)));
        }
    } else {
        out.push_str("No investigation report is available.\n");
    }
    out.push_str(&format!(
        "\n## Evidence changes\n\n{}\n\n",
        md(&review.changes.reason)
    ));
    if review.changes.available {
        out.push_str(&format!(
            "Added: {} · Not repeated: {} · Repeated payload groups: {}\n\n",
            md(&review.changes.added.join(", ")),
            md(&review.changes.not_repeated.join(", ")),
            review.changes.repeated.len()
        ));
    }
    out.push_str("## Checks for the next operator\n\n");
    for check in &review.checks {
        out.push_str(&format!("- {}\n", md(check)));
    }
    out.push_str("\n## Evidence index\n\n| ID | Source | Collected (UTC) | Summary |\n| --- | --- | --- | --- |\n");
    for e in &incident.evidence {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            md(&e.id),
            md(&e.source),
            e.observed_at.to_rfc3339(),
            md(&e.summary)
        ));
    }
    out.push_str("\n## Run history\n\n");
    for run in &review.runs {
        out.push_str(&format!(
            "- `{}`: {}; {} retained evidence items.\n",
            run.id,
            md(&run.outcome),
            run.evidence_ids.len()
        ));
    }
    out.push_str("\n## Interpretation limits\n\n");
    for limit in &review.limitations {
        out.push_str(&format!("- {}\n", md(limit)));
    }
    out.push_str("\nThis handoff summarizes retained evidence. It does not verify root cause, recovery, or authorize remediation. Raw tool payloads are omitted.\n");
    out
}
