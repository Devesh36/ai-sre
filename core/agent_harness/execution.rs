use super::{
    policy::Policy,
    trace::{Trace, TransportFault},
};
use crate::{
    domain::{Incident, Usage},
    store::Store,
    tools::Toolbox,
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, time::Instant};
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
        let definitions = tools.definitions();
        let definition = definitions.iter().find(|d| d["function"]["name"] == name);
        let known = definition.is_some();
        // Unknown tool names are untrusted model output: do not record their text.
        let label = if known { name } else { "unavailable_tool" };
        if !known || !args.is_object() {
            let error = "tool unavailable under current policy, or arguments are not an object";
            self.trace(incident, "tool_rejected", label, 0, vec![]);
            store.checkpoint(incident)?;
            gaps.push(error.into());
            return Ok(json!({"evidence":[],"errors":[error]}));
        }
        let definition = definition.expect("known tool in the captured snapshot");
        if crate::core::tool::schema::validate_input(&definition["function"]["parameters"], &args)
            .is_err()
        {
            let error = "tool input does not match its public schema";
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
