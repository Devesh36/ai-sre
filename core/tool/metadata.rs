// Copyright 2026 Tracer Cloud. SPDX-License-Identifier: Apache-2.0
// Modified: Rust metadata translation; validated construction replaces Pydantic validators.
use crate::core::domain::types::tools::{ToolRole, ToolSurface};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryMetadata {
    pub surfaces: Vec<ToolSurface>,
    pub tags: Vec<String>,
    pub role: ToolRole,
}
pub fn normalize_surfaces(values: &[String]) -> Result<Vec<ToolSurface>> {
    let mut output = Vec::new();
    for value in values {
        let surface = match value.trim().to_lowercase().as_str() {
            "chat" => ToolSurface::Chat,
            "action" => ToolSurface::Action,
            _ => bail!("unsupported tool surface; expected action or chat"),
        };
        if !output.contains(&surface) {
            output.push(surface);
        }
    }
    if output.is_empty() {
        output.push(ToolSurface::Chat);
    }
    Ok(output)
}
pub fn normalize_tags(values: &[String]) -> Vec<String> {
    let mut output = Vec::new();
    for value in values {
        let value = value.trim();
        if !value.is_empty() && !output.iter().any(|s| s == value) {
            output.push(value.to_owned());
        }
    }
    output
}

impl Default for RegistryMetadata {
    fn default() -> Self {
        Self {
            surfaces: vec![ToolSurface::Chat],
            tags: vec![],
            role: ToolRole::Action,
        }
    }
}
