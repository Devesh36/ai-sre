use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub database: PathBuf,
    pub evidence_file: Option<PathBuf>,
    pub runbooks: Option<PathBuf>,
    pub max_turns: usize,
    pub max_tool_calls: usize,
    pub timeout_seconds: u64,
    pub max_concurrent: usize,
    pub prometheus: Option<Endpoint>,
    pub loki: Option<Endpoint>,
    pub model: Option<Model>,
    pub connections: Vec<crate::integrations::Connection>,
    pub mcp: Vec<crate::mcp::Server>,
    pub mask_identifiers: Vec<String>,
    pub deliveries: Vec<crate::operations::Delivery>,
    pub actions: Vec<crate::actions::Template>,
    pub workflow: Option<Workflow>,
    pub gateways: crate::gateway::Gateways,
    pub harness: crate::harness::Policy,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    pub name: String,
    pub instructions: Vec<String>,
    pub allowed_tools: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub url: String,
    pub token_env: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    /// Full chat completions endpoint; works with compatible hosted or local providers.
    pub url: String,
    pub name: String,
    pub key_env: Option<String>,
    #[serde(default)]
    pub protocol: crate::providers::Protocol,
    #[serde(default)]
    pub input_price_per_million: f64,
    #[serde(default)]
    pub output_price_per_million: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database: ".ai-sre/incidents.db".into(),
            evidence_file: None,
            runbooks: None,
            max_turns: 6,
            max_tool_calls: 12,
            timeout_seconds: 20,
            max_concurrent: 4,
            prometheus: None,
            loki: None,
            model: None,
            connections: vec![],
            mcp: vec![],
            mask_identifiers: vec![],
            deliveries: vec![],
            actions: vec![],
            workflow: None,
            gateways: Default::default(),
            harness: Default::default(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let mut cfg: Self = toml::from_str(
            &std::fs::read_to_string(path)
                .with_context(|| format!("read config {}", path.display()))?,
        )?;
        let base = path.parent().unwrap_or(Path::new("."));
        if cfg.database.is_relative() {
            cfg.database = base.join(&cfg.database);
        }
        for p in [&mut cfg.evidence_file, &mut cfg.runbooks]
            .into_iter()
            .flatten()
        {
            if p.is_relative() {
                *p = base.join(&*p);
            }
        }
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<()> {
        self.harness.validate()?;
        ensure!((1..=16).contains(&self.max_turns), "max_turns must be 1–16");
        ensure!(
            (1..=32).contains(&self.max_tool_calls),
            "max_tool_calls must be 1–32"
        );
        ensure!(
            (1..=60).contains(&self.timeout_seconds),
            "timeout_seconds must be 1–60"
        );
        ensure!(
            (1..=16).contains(&self.max_concurrent),
            "max_concurrent must be 1–16"
        );
        for endpoint in [&self.prometheus, &self.loki].into_iter().flatten() {
            validate_url(&endpoint.url)?;
            check_env(&endpoint.token_env)?;
        }
        if let Some(model) = &self.model {
            validate_url(&model.url)?;
            ensure!(!model.name.trim().is_empty(), "model name is required");
            check_env(&model.key_env)?;
            ensure!(
                [
                    model.input_price_per_million,
                    model.output_price_per_million
                ]
                .iter()
                .all(|p| p.is_finite() && *p >= 0.0),
                "model prices must be finite and nonnegative"
            );
        }
        ensure!(
            self.connections.len() <= 32 && self.mcp.len() <= 8,
            "maximum 32 connections and 8 MCP servers"
        );
        let mut names = std::collections::HashSet::new();
        for connection in &self.connections {
            connection.validate()?;
            ensure!(
                names.insert(connection.name.clone()),
                "duplicate connection name"
            );
        }
        for server in &self.mcp {
            server.validate()?;
            ensure!(
                names.insert(server.name.clone()),
                "duplicate connection/server name"
            );
        }
        for delivery in &self.deliveries {
            delivery.validate()?;
        }
        let mut action_names = std::collections::HashSet::new();
        self.gateways.validate()?;
        for action in &self.actions {
            action.validate()?;
            ensure!(action_names.insert(&action.name), "duplicate action name");
        }
        if let Some(workflow) = &self.workflow {
            ensure!(
                !workflow.name.is_empty()
                    && workflow.instructions.len() <= 20
                    && workflow.instructions.iter().all(|s| s.len() <= 4000),
                "invalid workflow instructions"
            );
            ensure!(
                !workflow.allowed_tools.is_empty(),
                "workflow needs an explicit tool allowlist"
            );
        }
        ensure!(
            self.mask_identifiers.len() <= 100
                && self
                    .mask_identifiers
                    .iter()
                    .all(|s| s.len() >= 3 && s.len() <= 256),
            "mask identifiers must be 3–256 bytes; maximum 100"
        );
        Ok(())
    }

    pub fn client(&self) -> Result<reqwest::Client> {
        Ok(reqwest::Client::builder()
            .timeout(Duration::from_secs(self.timeout_seconds))
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()?)
    }
}

pub fn check_env(name: &Option<String>) -> Result<()> {
    if let Some(name) = name {
        ensure!(
            !name.is_empty() && std::env::var(name).is_ok_and(|v| !v.is_empty()),
            "required credential variable {name} is missing; set it in your process environment"
        );
    }
    Ok(())
}

pub fn validate_url(value: &str) -> Result<()> {
    let url = reqwest::Url::parse(value)?;
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "URLs must not contain credentials, queries, or fragments"
    );
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    ensure!(
        url.scheme() == "https" || (url.scheme() == "http" && local),
        "use HTTPS for remote endpoints; HTTP is supported only on loopback"
    );
    Ok(())
}
