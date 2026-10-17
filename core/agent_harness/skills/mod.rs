//! Versioned workflow contracts shared by the registry, CLI and gateway.
use crate::config::{Config, Workflow};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub digest: String,
    pub revision: u64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub workflow: Workflow,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    pub digest: String,
    pub workflow: Workflow,
}

pub fn validate(workflow: &Workflow) -> Result<()> {
    ensure!(
        !workflow.name.is_empty()
            && workflow.name.len() <= 64
            && workflow
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
        "workflow name must be 1–64 ASCII letters, digits, hyphens or underscores"
    );
    ensure!(
        workflow.allowed_tools.len() <= 256
            && workflow.allowed_tools.iter().all(|t| !t.is_empty()
                && t.len() <= 160
                && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')),
        "invalid workflow tool allowlist"
    );
    Config {
        workflow: Some(workflow.clone()),
        ..Config::default()
    }
    .validate()
}
pub fn digest(workflow: &Workflow) -> Result<String> {
    validate(workflow)?;
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(workflow)?)
    ))
}
impl Bundle {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.digest == digest(&self.workflow)?,
            "workflow digest mismatch"
        );
        Ok(())
    }
}
impl From<Release> for Bundle {
    fn from(release: Release) -> Self {
        Self {
            digest: release.digest,
            workflow: release.workflow,
        }
    }
}
