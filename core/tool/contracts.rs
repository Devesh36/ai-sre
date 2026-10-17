// Copyright 2026 Tracer Cloud. SPDX-License-Identifier: Apache-2.0
// Modified: typed Rust contracts replace Python introspection and class decorators.
use super::{metadata::RegistryMetadata, output::Content};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceType {
    Logs,
    Metrics,
    Traces,
    Events,
    Topology,
    DeploymentMetadata,
    QueryStats,
    Artifact,
    Other,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEffectLevel {
    None,
    #[default]
    ReadOnly,
    Mutating,
    External,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolMetadata {
    pub name: String,
    pub description: String,
    pub source: String,
    pub input_schema: Value,
    pub registry: RegistryMetadata,
    pub side_effect_level: SideEffectLevel,
    pub evidence_type: Option<EvidenceType>,
    #[serde(default)]
    pub injected_params: Vec<String>,
}
impl ToolMetadata {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.is_empty()
                && self.name.len() <= 160
                && self
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "invalid tool name"
        );
        ensure!(
            !self.description.trim().is_empty() && !self.source.trim().is_empty(),
            "tool description and source required"
        );
        ensure!(
            self.input_schema["type"] == "object",
            "tool input schema must describe an object"
        );
        ensure!(
            self.input_schema.to_string().len() <= 64 * 1024,
            "tool schema exceeds 64 KiB"
        );
        ensure!(
            !self.registry.surfaces.is_empty(),
            "tool must have an exposure surface"
        );
        Ok(())
    }
    pub fn public_schema(&self) -> Value {
        let mut schema = self.input_schema.clone();
        if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
            for key in &self.injected_params {
                properties.remove(key);
            }
        }
        if let Some(required) = schema.get_mut("required").and_then(Value::as_array_mut) {
            required.retain(|v| {
                !v.as_str()
                    .is_some_and(|s| self.injected_params.iter().any(|k| k == s))
            });
        }
        schema
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub content: Content,
    pub details: Value,
    pub is_error: bool,
    pub terminate: bool,
    pub metadata: Map<String, Value>,
    pub provider_output_bounded: bool,
}
pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<ExecutionResult>> + Send + 'a>>;
pub type UpdateObserver = Arc<dyn Fn(&Value) + Send + Sync>;
#[derive(Clone, Default)]
pub struct ToolContext {
    pub configured_arguments: BTreeMap<String, Map<String, Value>>,
    pub resources: Map<String, Value>,
    pub on_update: Option<UpdateObserver>,
}
impl ToolContext {
    pub fn emit_update(&self, update: &Value) {
        if let Some(callback) = &self.on_update {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(update)));
        }
    }
}
pub trait RuntimeTool: Send + Sync {
    fn metadata(&self) -> &ToolMetadata;
    fn execute<'a>(&'a self, arguments: Value, context: &'a ToolContext) -> ToolFuture<'a>;
}
