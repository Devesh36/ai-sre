use crate::{
    config::{Config, check_env, validate_url},
    domain::Incident,
    tools::{Toolbox, bounded_json},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
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
