use crate::{
    config::{Config, check_env, validate_url},
    domain::Incident,
    tools::{Toolbox, bounded_json},
};
use anyhow::{Result, ensure};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
pub struct WorkItem {
    pub id: Uuid,
    pub service: String,
    pub text: String,
    pub done: bool,
    pub created_at: DateTime<Utc>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: Uuid,
    pub service: String,
    pub key: String,
    pub text: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub id: Uuid,
    pub expression: String,
    pub timezone: String,
    pub question: String,
    pub service: String,
    pub enabled: bool,
    pub next_at: DateTime<Utc>,
}
impl Schedule {
    pub fn new(
        expression: String,
        timezone: String,
        question: String,
        service: String,
    ) -> Result<Self> {
        Incident::new(question.clone(), service.clone())?;
        let next_at = next_occurrence(&expression, &timezone, Utc::now())?;
        Ok(Self {
            id: Uuid::new_v4(),
            expression,
            timezone,
            question,
            service,
            enabled: true,
            next_at,
        })
    }
}
pub fn next_occurrence(
    expression: &str,
    timezone: &str,
    after: DateTime<Utc>,
) -> Result<DateTime<Utc>> {
    let schedule = cron::Schedule::from_str(expression)?;
    let zone = chrono_tz::Tz::from_str(timezone)?;
    let next = schedule
        .after(&after.with_timezone(&zone))
        .next()
        .ok_or_else(|| anyhow::anyhow!("cron has no future occurrence"))?
        .with_timezone(&Utc);
    ensure!(
        (next - after).num_seconds() >= 1,
        "schedule must advance time"
    );
    Ok(next)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delivery {
    pub name: String,
    pub kind: DeliveryKind,
    /// Public API endpoint only; webhook URL credentials belong in url_env instead.
    pub url: Option<String>,
    pub url_env: Option<String>,
    pub token_env: Option<String>,
    pub destination: Option<String>,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryKind {
    Slack,
    DiscordWebhook,
    Telegram,
    JsonWebhook,
}
impl Delivery {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            crate::integrations::valid_name(&self.name),
            "invalid delivery name"
        );
        ensure!(
            self.url.is_some() ^ self.url_env.is_some(),
            "provide exactly one of url or url_env for delivery"
        );
        if let Some(url) = &self.url {
            validate_url(url)?;
        }
        if let Some(var) = &self.url_env {
            check_env(&Some(var.clone()))?;
            validate_url(&std::env::var(var)?)?;
        }
        check_env(&self.token_env)?;
        if matches!(self.kind, DeliveryKind::Slack | DeliveryKind::Telegram) {
            ensure!(
                self.destination.as_ref().is_some_and(|s| !s.is_empty()),
                "delivery destination is required"
            );
        }
        Ok(())
    }
}

/// Called only by an explicit operator command, never by the model or automatic scheduling.
pub async fn deliver(config: &Config, incident: &Incident, name: &str) -> Result<Value> {
    let delivery = config
        .deliveries
        .iter()
        .find(|d| d.name == name)
        .ok_or_else(|| anyhow::anyhow!("unknown delivery destination"))?;
    delivery.validate()?;
    let tools = Toolbox::new(config.clone())?;
    let report = incident
        .report
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("incident has no report to deliver"))?;
    let text = tools.redact(&format!(
        "{}\nService: {}\n{}\n\nNext steps:\n{}\n\nIncident: {}",
        incident.title,
        incident.service,
        report.summary,
        report
            .next_steps
            .iter()
            .map(|s| format!("• {s}"))
            .collect::<Vec<_>>()
            .join("\n"),
        incident.id
    ));
    let text = crate::tools::truncate(&text, 1800);
    let body = match delivery.kind {
        DeliveryKind::Slack => {
            json!({"channel":delivery.destination,"text":text,"unfurl_links":false,"unfurl_media":false})
        }
        DeliveryKind::DiscordWebhook => json!({"content":text,"allowed_mentions":{"parse":[]}}),
        DeliveryKind::Telegram => {
            json!({"chat_id":delivery.destination,"text":text,"disable_web_page_preview":true})
        }
        DeliveryKind::JsonWebhook => {
            json!({"incident_id":incident.id,"service":incident.service,"summary":text})
        }
    };
    let url = match &delivery.url {
        Some(u) => u.clone(),
        None => std::env::var(delivery.url_env.as_ref().expect("validated"))?,
    };
    let mut request = tools.client.post(url).json(&body);
    if let Some(var) = &delivery.token_env {
        request = request.bearer_auth(std::env::var(var)?);
    }
    let response = request.send().await.map_err(|_| {
        anyhow::anyhow!("delivery transport failed; inspect endpoint and authentication")
    })?;
    ensure!(
        response.status().is_success(),
        "delivery HTTP {}",
        response.status().as_u16()
    );
    if response.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(json!({"delivered":true}));
    }
    let result = bounded_json(response).await?;
    ensure!(
        result.get("ok") != Some(&Value::Bool(false)),
        "delivery API rejected the message"
    );
    Ok(json!({"delivered":true}))
}

/// Enumerates only process names and IDs. Never reads command lines or environments.
pub fn scan_fleet() -> Value {
    let mut agents = vec![];
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<u32>().ok())
            else {
                continue;
            };
            let Ok(name) = std::fs::read_to_string(entry.path().join("comm")) else {
                continue;
            };
            let name = name.trim();
            if ["codex", "claude", "cursor", "aider", "opencode"]
                .iter()
                .any(|a| name.to_lowercase().starts_with(a))
            {
                agents.push(json!({"pid":pid,"name":name,"observed_at":Utc::now()}));
            }
        }
    }
    json!({"agents":agents,"scope":"process names only; no token usage or conversation inspection"})
}

pub async fn scheduler(app: crate::api::App, mut stop: tokio::sync::watch::Receiver<bool>) {
    loop {
        if *stop.borrow() {
            break;
        }
        if let Err(e) = app.store.enqueue_due(chrono::Utc::now()) {
            eprintln!("Scheduler enqueue failed: {e}");
        }
        if let Ok(queued) = app.store.queued() {
            for incident in queued {
                let Ok(permit) = app.capacity.clone().try_acquire_owned() else {
                    break;
                };
                let Ok(claimed) = app.store.claim(incident.id, None) else {
                    continue;
                };
                let worker = app.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    if let Err(e) =
                        crate::agent::investigate(&worker.config, &worker.store, claimed).await
                    {
                        eprintln!("Scheduled investigation failed: {e}");
                    }
                });
            }
        }
        tokio::select! {_=tokio::time::sleep(std::time::Duration::from_secs(1))=>{},result=stop.changed()=>{if result.is_err() || *stop.borrow(){break;}}}
    }
}
