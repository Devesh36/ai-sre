// Copyright 2026 Tracer Cloud. SPDX-License-Identifier: Apache-2.0
// Modified: asynchronous Rust execution, explicit hooks, bounded batches; telemetry omitted.
use super::{
    contracts::{ExecutionResult, RuntimeTool, ToolCall, ToolContext, ToolMetadata},
    output::{Content, OutputBudget},
    schema::validate_input,
};
use crate::core::domain::types::tools::ToolRole;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

impl ExecutionResult {
    pub fn success(payload: Value) -> Self {
        Self {
            content: match &payload {
                Value::String(s) => Content::Text(s.clone()),
                Value::Array(a) if a.iter().all(Value::is_object) => Content::Blocks(
                    a.iter()
                        .map(|v| v.as_object().expect("checked object").clone())
                        .collect(),
                ),
                _ => Content::Text(payload.to_string()),
            },
            details: payload,
            is_error: false,
            terminate: false,
            metadata: Map::new(),
            provider_output_bounded: false,
        }
    }
    pub fn normalize(payload: Value) -> Self {
        let error = payload
            .get("error")
            .filter(|v| match v {
                Value::Null => false,
                Value::Bool(b) => *b,
                Value::String(s) => !s.is_empty(),
                Value::Number(n) => n.as_f64() != Some(0.0),
                Value::Array(a) => !a.is_empty(),
                Value::Object(o) => !o.is_empty(),
            })
            .cloned();
        let mut result = Self::success(payload);
        if let Some(error) = error {
            result.is_error = true;
            result.content = Content::Text(
                error
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| error.to_string()),
            );
        }
        result
    }
    pub fn model_only(&self) -> bool {
        self.metadata.get("model_only") == Some(&Value::Bool(true))
    }
    pub fn error(message: &str) -> Self {
        Self {
            content: Content::Text(message.into()),
            details: json!({"error":message}),
            is_error: true,
            terminate: false,
            metadata: Map::new(),
            provider_output_bounded: false,
        }
    }
    pub fn provider_content(&self, budget: OutputBudget) -> Content {
        if self.is_error
            && self
                .details
                .get("failure_state")
                .and_then(Value::as_str)
                .is_some_and(|s| {
                    matches!(
                        s,
                        "no_match"
                            | "missing_config"
                            | "denied"
                            | "transport_failure"
                            | "missing_data"
                    )
                })
        {
            return budget.bound(&Content::Text(self.details.to_string()));
        }
        if self.provider_output_bounded {
            self.content.clone()
        } else {
            budget.bound(&self.content)
        }
    }
    pub fn compat_payload(&self) -> Value {
        if self.is_error {
            json!({"error": self.content})
        } else if !self.details.is_null() {
            self.details.clone()
        } else {
            serde_json::to_value(&self.content).expect("serializable content")
        }
    }
}
#[derive(Clone, Default)]
pub struct ExecutionPatch {
    pub content: Option<Content>,
    pub details: Option<Value>,
    pub is_error: Option<bool>,
    pub terminate: Option<bool>,
    pub metadata: Map<String, Value>,
}
impl ExecutionPatch {
    pub fn apply(self, result: &mut ExecutionResult) {
        if let Some(content) = self.content {
            result.content = content;
            result.provider_output_bounded = false;
        }
        if let Some(details) = self.details {
            result.details = details;
        }
        if let Some(error) = self.is_error {
            result.is_error = error;
        }
        if let Some(terminate) = self.terminate {
            result.terminate = terminate;
        }
        result.metadata.extend(self.metadata);
    }
}
#[derive(Clone, Default)]
pub struct BeforeDecision {
    pub blocked: bool,
    pub reason: String,
    pub terminate: bool,
    pub metadata: Map<String, Value>,
}
pub trait ExecutionHooks: Send + Sync {
    fn before_batch(&self, _calls: &[ToolCall]) -> Result<()> {
        Ok(())
    }
    fn before_call(&self, _call: &ToolCall, _tool: &ToolMetadata) -> Result<BeforeDecision> {
        Ok(BeforeDecision::default())
    }
    fn after_call(
        &self,
        _call: &ToolCall,
        _result: &ExecutionResult,
    ) -> Result<Option<ExecutionPatch>> {
        Ok(None)
    }
    fn on_start(&self, _call: &ToolCall) -> Result<()> {
        Ok(())
    }
}
#[derive(Default)]
pub struct HookChain(pub Vec<Arc<dyn ExecutionHooks>>);
impl ExecutionHooks for HookChain {
    fn before_batch(&self, calls: &[ToolCall]) -> Result<()> {
        for h in &self.0 {
            h.before_batch(calls)?;
        }
        Ok(())
    }
    fn before_call(&self, call: &ToolCall, tool: &ToolMetadata) -> Result<BeforeDecision> {
        let mut last = BeforeDecision::default();
        for h in &self.0 {
            last = h.before_call(call, tool)?;
            if last.blocked {
                break;
            }
        }
        Ok(last)
    }
    fn after_call(
        &self,
        call: &ToolCall,
        result: &ExecutionResult,
    ) -> Result<Option<ExecutionPatch>> {
        let mut next = result.clone();
        let mut changed = false;
        for h in &self.0 {
            if let Ok(Some(patch)) = h.after_call(call, &next) {
                patch.apply(&mut next);
                changed = true;
            }
        }
        Ok(changed.then_some(ExecutionPatch {
            content: Some(next.content),
            details: Some(next.details),
            is_error: Some(next.is_error),
            terminate: Some(next.terminate),
            metadata: next.metadata,
        }))
    }
    fn on_start(&self, call: &ToolCall) -> Result<()> {
        for h in &self.0 {
            h.on_start(call)?;
        }
        Ok(())
    }
}
/// Reject illegal turn-ending batches before any dispatch, then move bookkeeping first.
pub fn execution_order(
    calls: &[ToolCall],
    roles: &BTreeMap<String, ToolRole>,
) -> Result<Vec<usize>> {
    ensure!(calls.len() <= 256, "tool batch exceeds 256 calls");
    let endings: Vec<_> = calls
        .iter()
        .enumerate()
        .filter(|(_, c)| roles.get(&c.name) == Some(&ToolRole::TurnEnding))
        .map(|(i, _)| i)
        .collect();
    ensure!(
        endings.len() <= 1,
        "a response may contain only one turn-ending tool"
    );
    if !endings.is_empty() {
        ensure!(
            calls.iter().all(|c| matches!(
                roles.get(&c.name),
                Some(ToolRole::TurnEnding | ToolRole::Bookkeeping)
            )),
            "turn-ending tools may only share a response with bookkeeping tools"
        );
    }
    Ok((0..calls.len())
        .filter(|i| !endings.contains(i))
        .chain(endings.iter().copied())
        .collect())
}
/// Config-only keys can never be overridden by model arguments.
pub fn model_arguments(
    tool: &ToolMetadata,
    configured: &Map<String, Value>,
    input: &Value,
) -> Value {
    let mut merged = configured.clone();
    if let Some(input) = input.as_object() {
        for (k, v) in input {
            if !tool.injected_params.contains(k) {
                merged.insert(k.clone(), v.clone());
            }
        }
    }
    Value::Object(merged)
}
fn marked_error(message: &str, key: &str) -> ExecutionResult {
    let mut result = ExecutionResult::error(message);
    result.metadata.insert(key.into(), json!(true));
    result
}
pub async fn execute_tool_calls(
    calls: &[ToolCall],
    tools: &[Arc<dyn RuntimeTool>],
    context: &ToolContext,
    hooks: &dyn ExecutionHooks,
    should_stop: &(dyn Fn() -> bool + Send + Sync),
) -> Vec<ExecutionResult> {
    let map: BTreeMap<_, _> = tools
        .iter()
        .map(|t| (t.metadata().name.clone(), t))
        .collect();
    let roles = map
        .iter()
        .map(|(name, t)| (name.clone(), t.metadata().registry.role))
        .collect();
    let mut ids = BTreeSet::new();
    let order = execution_order(calls, &roles).and_then(|order| {
        ensure!(map.len() == tools.len(), "duplicate tool names in runtime");
        ensure!(
            calls
                .iter()
                .all(|c| !c.id.is_empty() && c.id.len() <= 256 && ids.insert(&c.id)),
            "missing or duplicate tool call ID"
        );
        hooks.before_batch(calls)?;
        Ok(order)
    });
    let order = match order {
        Ok(order) => order,
        Err(_) => {
            return calls
                .iter()
                .map(|_| marked_error("Tool batch rejected before dispatch", "batch_rejected"))
                .collect();
        }
    };
    let mut results: Vec<_> = calls
        .iter()
        .map(|_| {
            marked_error(
                "Not run: the turn ended or was cancelled; re-issue next turn if needed",
                "skipped",
            )
        })
        .collect();
    let mut stopped = false;
    for i in order {
        if stopped || should_stop() {
            stopped = true;
            continue;
        }
        let call = &calls[i];
        let Some(tool) = map.get(&call.name) else {
            results[i] = ExecutionResult::error("Tool is not available in this session");
            continue;
        };
        let metadata = tool.metadata();
        if validate_input(&metadata.public_schema(), &call.input).is_err() {
            results[i] = ExecutionResult::error("Tool input does not match the public schema");
            continue;
        }
        let decision = match hooks.before_call(call, metadata) {
            Ok(d) => d,
            Err(_) => BeforeDecision {
                blocked: true,
                reason: "Tool authorization hook failed".into(),
                ..BeforeDecision::default()
            },
        };
        if decision.blocked {
            let mut result = marked_error(&decision.reason, "blocked");
            result.terminate = decision.terminate;
            result.metadata.extend(decision.metadata);
            result.metadata.insert("blocked".into(), json!(true));
            stopped = result.terminate;
            results[i] = result;
            continue;
        }
        if hooks.on_start(call).is_err() {
            results[i] = marked_error("Tool intent could not be recorded", "blocked");
            continue;
        }
        let configured = context
            .configured_arguments
            .get(&metadata.name)
            .cloned()
            .unwrap_or_default();
        let input = model_arguments(metadata, &configured, &call.input);
        let mut result = match tool.execute(input, context).await {
            Ok(r) => r,
            Err(_) => ExecutionResult::error("Tool execution failed"),
        };
        if let Ok(Some(patch)) = hooks.after_call(call, &result) {
            patch.apply(&mut result);
        }
        stopped = result.terminate;
        result.metadata.insert("tool_name".into(), json!(call.name));
        results[i] = result;
    }
    results
}
#[derive(Default, Debug, Serialize, Deserialize)]
pub struct ToolFailureSummary {
    pub tool_error_count: usize,
    pub blocked_tool_calls: usize,
    pub last_failed_tool: String,
    pub last_tool_error: String,
}
pub fn summarize_tool_failures(
    calls: &[ToolCall],
    results: &[ExecutionResult],
) -> ToolFailureSummary {
    let mut summary = ToolFailureSummary::default();
    for (call, result) in calls.iter().zip(results) {
        if !result.is_error || result.metadata.get("skipped") == Some(&Value::Bool(true)) {
            continue;
        }
        summary.tool_error_count += 1;
        if result.metadata.get("blocked") == Some(&Value::Bool(true)) {
            summary.blocked_tool_calls += 1;
        }
        summary.last_failed_tool = call.name.clone();
        summary.last_tool_error = result
            .details
            .get("error")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| match &result.content {
                Content::Text(s) => s.clone(),
                _ => String::new(),
            });
    }
    summary
}
