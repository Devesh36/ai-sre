//! Operator-approved, fixed-destination HTTP actions. The agent can propose but cannot execute.
use crate::{
    config::{Config, check_env, validate_url},
    store::Store,
    tools::{Toolbox, bounded_json},
};
use anyhow::{Result, ensure};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    pub name: String,
    pub description: String,
    pub url: String,
    pub method: Method,
    pub token_env: Option<String>,
    pub body: Value,
    #[serde(default)]
    pub arguments: BTreeMap<String, Argument>,
    pub precondition: Check,
    pub verification: Check,
    pub rollback: String,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    Post,
    Patch,
    Put,
    Delete,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Argument {
    pub kind: ArgumentKind,
    pub min: Option<i64>,
    pub max: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArgumentKind {
    String,
    Integer,
    Boolean,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub url: String,
    pub pointer: String,
    pub equals: Value,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Pending,
    Approved,
    Rejected,
    Executing,
    Verified,
    Failed,
    Unknown,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: Uuid,
    pub incident_id: Uuid,
    pub template: Template,
    pub arguments: Value,
    pub rendered_body: Value,
    pub rationale: String,
    pub status: ActionStatus,
    pub created_at: DateTime<Utc>,
    pub actor: Option<String>,
    pub approved_at: Option<DateTime<Utc>>,
    pub result: Option<String>,
}
impl Template {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            crate::integrations::valid_name(&self.name),
            "invalid action name"
        );
        validate_url(&self.url)?;
        check_env(&self.token_env)?;
        for check in [&self.precondition, &self.verification] {
            validate_url(&check.url)?;
            ensure!(
                reqwest::Url::parse(&check.url)?.origin()
                    == reqwest::Url::parse(&self.url)?.origin(),
                "action checks must share the action origin"
            );
            ensure!(
                check.pointer.is_empty() || check.pointer.starts_with('/'),
                "check pointer must be a JSON pointer"
            );
        }
        ensure!(
            !self.rollback.trim().is_empty(),
            "action requires an operator rollback procedure"
        );
        ensure!(
            self.body.to_string().len() <= 16384 && self.arguments.len() <= 16,
            "action template exceeds limits"
        );
        Ok(())
    }
    pub fn render(&self, args: &Value) -> Result<Value> {
        let args = args
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("arguments must be an object"))?;
        ensure!(
            args.len() == self.arguments.len(),
            "provide exactly the configured action arguments"
        );
        for (name, spec) in &self.arguments {
            let value = args
                .get(name)
                .ok_or_else(|| anyhow::anyhow!("missing action argument {name}"))?;
            match spec.kind {
                ArgumentKind::String => ensure!(
                    value.as_str().is_some_and(|s| s.len() <= 1024),
                    "argument {name} must be a bounded string"
                ),
                ArgumentKind::Boolean => {
                    ensure!(value.is_boolean(), "argument {name} must be boolean")
                }
                ArgumentKind::Integer => {
                    let v = value
                        .as_i64()
                        .ok_or_else(|| anyhow::anyhow!("argument {name} must be integer"))?;
                    ensure!(
                        spec.min.is_none_or(|m| v >= m) && spec.max.is_none_or(|m| v <= m),
                        "argument {name} is outside configured bounds"
                    );
                }
            }
        }
        fn substitute(value: &mut Value, args: &serde_json::Map<String, Value>) -> Result<()> {
            match value {
                Value::String(s) => {
                    if let Some(key) = s.strip_prefix("${").and_then(|s| s.strip_suffix('}')) {
                        *value = args
                            .get(key)
                            .cloned()
                            .ok_or_else(|| anyhow::anyhow!("unknown template argument"))?;
                    }
                }
                Value::Array(a) => {
                    for v in a {
                        substitute(v, args)?;
                    }
                }
                Value::Object(o) => {
                    for v in o.values_mut() {
                        substitute(v, args)?;
                    }
                }
                _ => {}
            }
            Ok(())
        }
        let mut body = self.body.clone();
        substitute(&mut body, args)?;
        Ok(body)
    }
}

pub fn propose(
    config: &Config,
    store: &Store,
    incident_id: Uuid,
    name: &str,
    arguments: Value,
    rationale: &str,
) -> Result<Proposal> {
    ensure!(store.get(incident_id)?.is_some(), "incident not found");
    let template = config
        .actions
        .iter()
        .find(|t| t.name == name)
        .ok_or_else(|| anyhow::anyhow!("unknown action template"))?
        .clone();
    template.validate()?;
    let rendered_body = template.render(&arguments)?;
    ensure!(
        !rationale.is_empty() && rationale.len() <= 4000,
        "rationale must contain 1–4000 bytes"
    );
    let tools = Toolbox::new(config.clone())?;
    let p = Proposal {
        id: Uuid::new_v4(),
        incident_id,
        template,
        arguments,
        rendered_body,
        rationale: tools.redact(rationale),
        status: ActionStatus::Pending,
        created_at: Utc::now(),
        actor: None,
        approved_at: None,
        result: None,
    };
    // Credentials must be environment references; reject secret-shaped proposal content instead of changing approved bytes.
    let mut clean = json!(p);
    tools.redact_value(&mut clean);
    ensure!(
        clean == json!(p),
        "action proposal contains sensitive data; use environment credential references"
    );
    store.record_put("action", &p.id.to_string(), &json!(p))?;
    Ok(p)
}

pub fn review(store: &Store, id: Uuid, actor: &str, approve: bool) -> Result<Proposal> {
    ensure!(
        !actor.trim().is_empty() && actor.len() <= 128,
        "reviewer name is required"
    );
    let value = store.record_mutate("action", &id.to_string(), |v| {
        let mut p: Proposal = serde_json::from_value(v.clone())?;
        ensure!(
            matches!(p.status, ActionStatus::Pending),
            "proposal was already reviewed"
        );
        ensure!(
            (Utc::now() - p.created_at).num_minutes() < 60,
            "proposal expired; create a new one"
        );
        p.status = if approve {
            ActionStatus::Approved
        } else {
            ActionStatus::Rejected
        };
        p.actor = Some(actor.into());
        p.approved_at = Some(Utc::now());
        *v = json!(p);
        Ok(())
    })?;
    Ok(serde_json::from_value(value)?)
}

pub async fn execute(config: &Config, store: &Store, id: Uuid) -> Result<Proposal> {
    let value = store
        .record_get("action", &id.to_string())?
        .ok_or_else(|| anyhow::anyhow!("proposal not found"))?;
    let proposal: Proposal = serde_json::from_value(value)?;
    ensure!(
        matches!(proposal.status, ActionStatus::Approved),
        "action requires explicit approval"
    );
    ensure!(
        proposal
            .approved_at
            .is_some_and(|t| (Utc::now() - t).num_minutes() < 15),
        "approval expired"
    );
    let template = config
        .actions
        .iter()
        .find(|t| t.name == proposal.template.name)
        .ok_or_else(|| anyhow::anyhow!("action template was removed"))?;
    ensure!(
        json!(template) == json!(proposal.template),
        "action configuration changed after proposal; re-propose and review"
    );
    template.validate()?;
    ensure!(
        template.render(&proposal.arguments)? == proposal.rendered_body,
        "proposal body integrity check failed"
    );
    let tools = Toolbox::new(config.clone())?;
    check(&tools.client, template, &template.precondition).await?;
    store.record_mutate("action", &id.to_string(), |v| {
        ensure!(v["status"] == "approved", "action was already claimed");
        v["status"] = json!("executing");
        Ok(())
    })?;
    let method = match template.method {
        Method::Post => reqwest::Method::POST,
        Method::Patch => reqwest::Method::PATCH,
        Method::Put => reqwest::Method::PUT,
        Method::Delete => reqwest::Method::DELETE,
    };
    let mut request = tools
        .client
        .request(method, &template.url)
        .json(&proposal.rendered_body);
    if matches!(template.method, Method::Patch) {
        request = request.header("Content-Type", "application/merge-patch+json");
    }
    if let Some(var) = &template.token_env {
        request = request.bearer_auth(std::env::var(var)?);
    }
    // Never retry mutations: a transport failure may occur after the remote side effect.
    let (status,result)=match request.send().await {
        Err(_)=>("unknown","Transport failed after dispatch; inspect remote state before any retry".to_string()),
        Ok(response) if !response.status().is_success()=>("failed",format!("Remote action returned HTTP {}; inspect remote state",response.status().as_u16())),
        Ok(_)=>match check(&tools.client,template,&template.verification).await {
            Ok(())=>("verified","Action request succeeded and verification matched".into()),
            Err(_)=>("unknown","Request succeeded but verification did not match; inspect state and the rollback procedure".into()),
        }
    };
    let value = store.record_mutate("action", &id.to_string(), |v| {
        v["status"] = json!(status);
        v["result"] = json!(result);
        Ok(())
    })?;
    Ok(serde_json::from_value(value)?)
}

async fn check(client: &reqwest::Client, template: &Template, check: &Check) -> Result<()> {
    let mut request = client.get(&check.url);
    if let Some(var) = &template.token_env {
        request = request.bearer_auth(std::env::var(var)?);
    }
    let value = bounded_json(
        request
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("action check transport failed"))?,
    )
    .await?;
    ensure!(
        value.pointer(&check.pointer) == Some(&check.equals),
        "action condition did not match"
    );
    Ok(())
}
