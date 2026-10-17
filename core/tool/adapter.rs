//! Bridge built-in/native/MCP tools to the typed embedding runtime.
use super::{
    contracts::{
        ExecutionResult, RuntimeTool, SideEffectLevel, ToolContext, ToolFuture, ToolMetadata,
    },
    live_catalog::LiveToolCatalog,
    metadata::RegistryMetadata,
    registry::Toolbox,
};
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
struct Adapter {
    toolbox: Toolbox,
    service: String,
    metadata: ToolMetadata,
}
impl RuntimeTool for Adapter {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }
    fn execute<'a>(&'a self, arguments: Value, _context: &'a ToolContext) -> ToolFuture<'a> {
        Box::pin(async move {
            let items = self
                .toolbox
                .call(&self.metadata.name, arguments, &self.service)
                .await?;
            Ok(ExecutionResult::success(
                serde_json::json!({"evidence": items}),
            ))
        })
    }
}
impl Toolbox {
    /// Call prepare() first to discover allowlisted MCP tools. Capture once per request.
    /// This embedding API collects evidence but does not assign incident IDs or checkpoint it.
    pub fn runtime_catalog(&self, service: &str) -> Result<LiveToolCatalog> {
        let tools = self
            .definitions()
            .into_iter()
            .map(|definition| {
                let f = &definition["function"];
                Arc::new(Adapter {
                    toolbox: self.clone(),
                    service: service.into(),
                    metadata: ToolMetadata {
                        name: f["name"].as_str().unwrap_or_default().into(),
                        description: f["description"].as_str().unwrap_or_default().into(),
                        source: "configured_toolbox".into(),
                        input_schema: f["parameters"].clone(),
                        registry: RegistryMetadata::default(),
                        side_effect_level: if f["name"] == "propose_action" {
                            SideEffectLevel::Mutating
                        } else {
                            SideEffectLevel::ReadOnly
                        },
                        evidence_type: None,
                        injected_params: vec![],
                    },
                }) as Arc<dyn RuntimeTool>
            })
            .collect();
        let catalog = LiveToolCatalog::default();
        catalog.replace(tools)?;
        Ok(catalog)
    }
}
