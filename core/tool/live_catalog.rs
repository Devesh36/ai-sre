// Copyright 2026 Tracer Cloud. SPDX-License-Identifier: Apache-2.0
// Modified: immutable Arc snapshots and atomic validated replacement in Rust.
use super::contracts::{RuntimeTool, ToolMetadata};
use crate::core::domain::types::tools::ToolSurface;
use anyhow::{Result, ensure};
use std::{
    collections::BTreeSet,
    sync::{Arc, RwLock},
};
pub type Snapshot = Arc<Vec<Arc<dyn RuntimeTool>>>;
#[derive(Clone, Default)]
pub struct LiveToolCatalog {
    tools: Arc<RwLock<Snapshot>>,
}
impl LiveToolCatalog {
    pub fn replace(&self, tools: Vec<Arc<dyn RuntimeTool>>) -> Result<()> {
        let mut names = BTreeSet::new();
        ensure!(tools.len() <= 256, "tool catalog exceeds 256 tools");
        for tool in &tools {
            tool.metadata().validate()?;
            ensure!(names.insert(&tool.metadata().name), "duplicate tool name");
        }
        *self
            .tools
            .write()
            .map_err(|_| anyhow::anyhow!("tool catalog lock poisoned"))? = Arc::new(tools);
        Ok(())
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        Ok(self
            .tools
            .read()
            .map_err(|_| anyhow::anyhow!("tool catalog lock poisoned"))?
            .clone())
    }
    pub fn for_surface(&self, surface: ToolSurface) -> Result<Vec<ToolMetadata>> {
        Ok(self
            .snapshot()?
            .iter()
            .filter(|t| t.metadata().registry.surfaces.contains(&surface))
            .map(|t| t.metadata().clone())
            .collect())
    }
}
