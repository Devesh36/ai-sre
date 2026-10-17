use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Open,
    Investigating,
    Triaged,
    Failed,
    Resolved,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub source: String,
    pub observed_at: DateTime<Utc>,
    pub summary: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub title: String,
    pub explanation: String,
    /// Qualitative evidence strength, never a calibrated probability.
    pub confidence: Confidence,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Report {
    pub summary: String,
    pub findings: Vec<Finding>,
    pub next_steps: Vec<String>,
    pub gaps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub at: DateTime<Utc>,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Usage {
    pub model_calls: u32,
    pub tool_calls: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incident {
    pub id: Uuid,
    pub title: String,
    pub service: String,
    pub status: Status,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub evidence: Vec<Evidence>,
    pub report: Option<Report>,
    pub events: Vec<Event>,
    pub usage: Usage,
    #[serde(default)]
    pub run_id: Option<Uuid>,
    #[serde(default)]
    pub cancel_requested: bool,
    #[serde(default)]
    pub schedule_id: Option<Uuid>,
    #[serde(default)]
    pub context_summary: Option<String>,
    #[serde(default)]
    pub trace: Vec<crate::harness::Trace>,
}

impl Incident {
    pub fn new(title: String, service: String) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !title.trim().is_empty() && title.len() <= 4000,
            "title must contain 1–4000 bytes"
        );
        anyhow::ensure!(
            !service.trim().is_empty() && service.len() <= 128,
            "service must contain 1–128 bytes"
        );
        let now = Utc::now();
        let mut incident = Self {
            id: Uuid::new_v4(),
            title,
            service,
            status: Status::Open,
            created_at: now,
            updated_at: now,
            evidence: vec![],
            report: None,
            events: vec![],
            usage: Usage::default(),
            run_id: None,
            cancel_requested: false,
            schedule_id: None,
            context_summary: None,
            trace: vec![],
        };
        incident.event("created", "Investigation opened");
        Ok(incident)
    }

    pub fn event(&mut self, kind: &str, message: impl Into<String>) {
        self.updated_at = Utc::now();
        self.events.push(Event {
            at: self.updated_at,
            kind: kind.into(),
            message: message.into(),
        });
    }
}
