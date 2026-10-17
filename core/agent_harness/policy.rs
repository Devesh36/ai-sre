use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
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
