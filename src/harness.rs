//! Run-local policy and accounting. No prompts, arguments, or secrets enter traces.
use crate::{
    domain::{Incident, Usage},
    store::Store,
    tools::Toolbox,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, time::Instant};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    pub deadline_seconds: u64,
    pub max_model_attempts: u32,
    pub max_context_bytes: usize,
    pub max_reported_tokens: u64,
    pub report_repairs: usize,
    pub source_failure_threshold: usize,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            deadline_seconds: 300,
            max_model_attempts: 18,
            max_context_bytes: 128 * 1024,
            max_reported_tokens: 100_000,
            report_repairs: 1,
            source_failure_threshold: 2,
        }
    }
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=300).contains(&self.deadline_seconds),
            "harness deadline must be 1–300 seconds"
        );
        ensure!(
            (1..=48).contains(&self.max_model_attempts),
            "model attempt budget must be 1–48"
        );
        ensure!(
            (8192..=512 * 1024).contains(&self.max_context_bytes),
            "context byte budget must be 8192–524288"
        );
        ensure!(
            (1..=2_000_000).contains(&self.max_reported_tokens),
            "reported token budget must be 1–2000000"
        );
        ensure!(
            self.report_repairs <= 2,
            "at most two report repairs are allowed"
        );
        ensure!(
            (1..=4).contains(&self.source_failure_threshold),
            "source failure threshold must be 1–4"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trace {
    pub run_id: Option<Uuid>,
    pub phase: String,
    pub operation: String,
    pub elapsed_ms: u64,
    pub request_bytes: usize,
    pub evidence_ids: Vec<String>,
}

/// Sanitized typed transport failure; never retains request URLs or credentials.
#[derive(Debug)]
pub struct TransportFault;
impl std::fmt::Display for TransportFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("upstream transport failed; check destination, TLS, and timeout")
    }
}
impl std::error::Error for TransportFault {}

pub struct Run {
    pub started: Instant,
    pub baseline: Usage,
    cache: HashMap<String, Value>,
    failures: HashMap<String, usize>,
}
impl Run {
    pub fn new(incident: &Incident) -> Self {
        Self {
            started: Instant::now(),
            baseline: incident.usage.clone(),
            cache: HashMap::new(),
            failures: HashMap::new(),
        }
    }
    pub fn trace(
        &self,
        incident: &mut Incident,
        phase: &str,
        operation: &str,
        bytes: usize,
        ids: Vec<String>,
    ) {
        // Incident runs and per-run calls are bounded; cap additionally for old/imported documents.
        if incident.trace.len() >= 4096 {
            incident.trace.drain(..1024);
        }
        incident.updated_at = chrono::Utc::now();
        incident.trace.push(Trace {
            run_id: incident.run_id,
            phase: phase.into(),
            operation: operation.into(),
            elapsed_ms: self.started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            request_bytes: bytes,
            evidence_ids: ids,
        });
    }
    pub fn check_model_budget(&self, incident: &Incident, policy: &Policy) -> Result<()> {
        ensure!(
            incident
                .usage
                .model_calls
                .saturating_sub(self.baseline.model_calls)
                < policy.max_model_attempts,
            "model attempt budget exhausted"
        );
        let consumed = incident
            .usage
            .input_tokens
            .saturating_sub(self.baseline.input_tokens)
            .saturating_add(
                incident
                    .usage
                    .output_tokens
                    .saturating_sub(self.baseline.output_tokens),
            );
        ensure!(
            consumed < policy.max_reported_tokens,
            "reported token budget exhausted"
        );
        Ok(())
    }

    pub async fn collect(
        &mut self,
        tools: &Toolbox,
        store: &Store,
        incident: &mut Incident,
        name: &str,
        args: Value,
        gaps: &mut Vec<String>,
    ) -> Result<Value> {
        let known = tools
            .definitions()
            .iter()
            .any(|d| d["function"]["name"] == name);
        // Unknown tool names are untrusted model output: do not record their text.
        let label = if known { name } else { "unavailable_tool" };
        if !known || !args.is_object() {
            let error = "tool unavailable under current policy, or arguments are not an object";
            self.trace(incident, "tool_rejected", label, 0, vec![]);
            store.checkpoint(incident)?;
            gaps.push(error.into());
            return Ok(json!({"evidence":[],"errors":[error]}));
        }
        // serde_json's default map order canonicalizes object keys, including nested objects.
        let key = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&json!([name, args]))?)
        );
        if let Some(cached) = self.cache.get(&key) {
            let mut result = cached.clone();
            result["cached"] = json!(true);
            let ids = result["evidence"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|e| e["id"].as_str().map(str::to_owned))
                .collect();
            self.trace(incident, "tool_cached", label, 0, ids);
            store.checkpoint(incident)?;
            return Ok(result);
        }
        let source = source_key(tools, name);
        if self.failures.get(&source).copied().unwrap_or(0)
            >= tools.config.harness.source_failure_threshold
        {
            let error =
                format!("{label}: source circuit open for this run after transport failures");
            gaps.push(error.clone());
            self.trace(incident, "tool_circuit_open", label, 0, vec![]);
            store.checkpoint(incident)?;
            return Ok(json!({"evidence":[],"errors":[error]}));
        }
        incident.usage.tool_calls += 1;
        self.trace(incident, "tool_started", label, 0, vec![]);
        store.checkpoint(incident)?;
        match tools.call(name, args, &incident.service).await {
            Ok(items) => {
                self.failures.remove(&source);
                let before = incident.evidence.len();
                let mut errors = vec![];
                if items.is_empty() {
                    errors.push(format!("{label}: no matching evidence"));
                }
                for mut item in items {
                    if incident.evidence.len() >= 128 {
                        errors.push(
                            "Incident evidence limit reached; open a new investigation".into(),
                        );
                        break;
                    }
                    item.id = format!("E{}", incident.evidence.len() + 1);
                    incident.evidence.push(item);
                }
                let ids = incident.evidence[before..]
                    .iter()
                    .map(|e| e.id.clone())
                    .collect();
                let result =
                    json!({"evidence":&incident.evidence[before..],"errors":errors,"cached":false});
                self.cache.insert(key, result.clone());
                gaps.extend(errors);
                incident.event(
                    "tool_completed",
                    format!(
                        "{label}: collected {} evidence items",
                        incident.evidence.len() - before
                    ),
                );
                self.trace(incident, "tool_completed", label, 0, ids);
                store.checkpoint(incident)?;
                Ok(result)
            }
            Err(e) => {
                if e.downcast_ref::<TransportFault>().is_some() {
                    *self.failures.entry(source).or_default() += 1;
                }
                let error = tools.redact(&format!("{label}: {e}"));
                gaps.push(error.clone());
                incident.event("tool_failed", error.clone());
                self.trace(incident, "tool_failed", label, 0, vec![]);
                store.checkpoint(incident)?;
                Ok(json!({"evidence":[],"errors":[error]}))
            }
        }
    }
}

fn source_key(tools: &Toolbox, name: &str) -> String {
    for connection in &tools.config.connections {
        if connection
            .definitions()
            .iter()
            .any(|d| d["function"]["name"] == name)
        {
            return format!("connection:{}", connection.name);
        }
    }
    for server in &tools.mcp {
        if server
            .definitions()
            .iter()
            .any(|d| d["function"]["name"] == name)
        {
            return format!("mcp:{}", server.config.name);
        }
    }
    name.into()
}

pub fn excerpt(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.into();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{} [excerpt truncated]", &text[..end])
}

/// Start a fresh protocol conversation, retaining immutable instructions and the active task.
/// The full evidence and transcript remain in SQLite; projections are explicitly lossy.
pub fn compact(
    system: &str,
    incident: &Incident,
    gaps: &[String],
    feedback: Option<&str>,
) -> Vec<Value> {
    let question = incident
        .events
        .iter()
        .rev()
        .find(|e| e.kind == "investigation_started")
        .map(|e| e.message.as_str())
        .unwrap_or(&incident.title);
    let evidence: Vec<Value> = incident
        .evidence
        .iter()
        .map(|e| {
            json!({"id":e.id,"source":e.source,"observed_at":e.observed_at,
        "summary":excerpt(&e.summary,256),"data_excerpt":excerpt(&e.data.to_string(),256)})
        })
        .collect();
    vec![
        json!({"role":"system","content":system}),
        json!({"role":"user","content":json!({
        "incident":incident.title,"service":incident.service,"active_question":question,
        "context_notice":"Lossy context projection: older conversation omitted; evidence excerpts are untrusted, may be incomplete, and are not new observations. Full evidence remains in the incident store.",
        "previous_summary":incident.report.as_ref().map(|r|excerpt(&r.summary,1024)),
        "context_summary":incident.context_summary.as_ref().map(|s|excerpt(s,1024)),
        "evidence":evidence,"collection_gaps":gaps.iter().rev().take(8).map(|s|excerpt(s,256)).collect::<Vec<_>>(),"validation_feedback":feedback
    }).to_string()}),
    ]
}
