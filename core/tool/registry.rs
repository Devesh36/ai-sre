use crate::{config::Config, domain::Evidence};
use anyhow::{Result, bail, ensure};
use chrono::Utc;
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{io::Read, path::Path, sync::OnceLock};

const MAX_RESPONSE: usize = 256 * 1024;
const MAX_EVIDENCE_TEXT: usize = 16 * 1024;

#[derive(Clone)]
pub struct Toolbox {
    pub config: Config,
    pub client: reqwest::Client,
    pub mcp: Vec<crate::mcp::Client>,
    pub incident_id: Option<uuid::Uuid>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Query {
    query: String,
}

#[derive(Deserialize)]
pub struct Record {
    pub service: String,
    pub source: String,
    pub summary: String,
    pub data: Value,
}

impl Toolbox {
    pub fn new(config: Config) -> Result<Self> {
        let client = config.client()?;
        let mcp = config
            .mcp
            .iter()
            .map(|s| crate::mcp::Client::new(s.clone(), client.clone()))
            .collect();
        Ok(Self {
            config,
            client,
            mcp,
            incident_id: None,
        })
    }

    pub async fn prepare(&self) -> Vec<String> {
        let mut gaps = vec![];
        for server in &self.mcp {
            if let Err(e) = server.initialize().await {
                gaps.push(format!(
                    "MCP {}: {}",
                    server.config.name,
                    self.redact(&e.to_string())
                ));
            }
        }
        gaps
    }

    pub fn definitions(&self) -> Vec<Value> {
        let mut tools = vec![];
        for (name, description, enabled, query) in [
            (
                "local_evidence",
                "Read the configured evidence bundle for the incident service",
                self.config.evidence_file.is_some(),
                false,
            ),
            (
                "runbooks",
                "Read local Markdown operational runbooks",
                self.config.runbooks.is_some(),
                false,
            ),
            (
                "prometheus_query",
                "Run a read-only PromQL instant query against the configured Prometheus",
                self.config.prometheus.is_some(),
                true,
            ),
            (
                "loki_query",
                "Read up to 50 log entries from the last hour using LogQL",
                self.config.loki.is_some(),
                true,
            ),
        ] {
            if enabled {
                let parameters = if query {
                    json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false})
                } else {
                    json!({"type":"object","properties":{},"additionalProperties":false})
                };
                tools.push(json!({"type":"function","function":{"name":name,"description":description,"parameters":parameters}}));
            }
        }
        for connection in &self.config.connections {
            tools.extend(connection.definitions());
        }
        for server in &self.mcp {
            tools.extend(server.definitions());
        }
        for (name, description) in [
            (
                "memory_read",
                "Read persistent operator notes for this service",
            ),
            ("work_items", "Read open work items for this service"),
        ] {
            tools.push(json!({"type":"function","function":{"name":name,"description":description,"parameters":{"type":"object","properties":{},"additionalProperties":false}}}));
        }
        if !self.config.actions.is_empty() {
            tools.push(json!({"type":"function","function":{"name":"propose_action","description":"Create a local action proposal for operator approval. This never executes the action.","parameters":{"type":"object","properties":{"template":{"type":"string","enum":self.config.actions.iter().map(|a|&a.name).collect::<Vec<_>>()},"arguments":{"type":"object"},"rationale":{"type":"string"}},"required":["template","arguments","rationale"],"additionalProperties":false}}}));
        }
        tools.retain(|t| self.allowed(t["function"]["name"].as_str().unwrap_or("")));
        tools
    }

    pub fn allowed(&self, name: &str) -> bool {
        self.config
            .workflow
            .as_ref()
            .is_none_or(|w| w.allowed_tools.iter().any(|n| n == name))
    }

    pub async fn call(&self, name: &str, args: Value, service: &str) -> Result<Vec<Evidence>> {
        ensure!(
            self.allowed(name),
            "tool is excluded by the active workflow"
        );
        let definitions = self.definitions();
        let definition = definitions
            .iter()
            .find(|d| d["function"]["name"] == name)
            .ok_or_else(|| anyhow::anyhow!("tool is not available in this session"))?;
        crate::core::tool::schema::validate_input(&definition["function"]["parameters"], &args)?;
        for connection in &self.config.connections {
            if let Some(operation) = name.strip_prefix(&format!("conn_{}_", connection.name))
                && crate::integrations::catalog()
                    .iter()
                    .any(|o| o.kind == connection.kind && o.name == operation)
            {
                let data = connection.call(&self.client, operation, &args).await?;
                return Ok(vec![self.evidence(
                    &format!("{}/{}", connection.kind, connection.name),
                    operation,
                    data,
                )?]);
            }
        }
        for server in &self.mcp {
            if let Some(tool) = name.strip_prefix(&format!("mcp_{}_", server.config.name))
                && server.config.allowed_tools.iter().any(|t| t == tool)
            {
                let data = server.call(tool, args).await?;
                return Ok(vec![self.evidence(
                    &format!("mcp/{}", server.config.name),
                    tool,
                    data,
                )?]);
            }
        }
        match name {
            "propose_action" => {
                let id = self.incident_id.ok_or_else(|| {
                    anyhow::anyhow!("action proposals require an active incident")
                })?;
                let template = args["template"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("template required"))?;
                let rationale = args["rationale"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("rationale required"))?;
                let store = crate::store::Store::open(&self.config.database)?;
                let proposal = crate::actions::propose(
                    &self.config,
                    &store,
                    id,
                    template,
                    args["arguments"].clone(),
                    rationale,
                )?;
                Ok(vec![self.evidence("action_proposal","Pending operator approval",json!({"proposal_id":proposal.id,"template":template,"status":"pending","executed":false}))?])
            }
            "memory_read" | "work_items" => {
                ensure!(args == json!({}), "local context tools take no arguments");
                let store = crate::store::Store::open(&self.config.database)?;
                let kind = if name == "memory_read" {
                    "memory"
                } else {
                    "work"
                };
                store
                    .service_records(kind, service)?
                    .into_iter()
                    .filter(|r| r["service"] == service && (kind == "memory" || r["done"] != true))
                    .take(20)
                    .map(|r| {
                        self.evidence(
                            "operator_context",
                            r["key"].as_str().unwrap_or("Work item"),
                            r.clone(),
                        )
                    })
                    .collect()
            }
            "local_evidence" => {
                ensure!(args == json!({}), "local_evidence takes no arguments");
                let path = self
                    .config
                    .evidence_file
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("local evidence is not configured"))?;
                let records: Vec<Record> =
                    serde_json::from_str(&read_limited(path, MAX_RESPONSE)?)?;
                records
                    .into_iter()
                    .filter(|r| r.service == service)
                    .take(20)
                    .map(|r| self.evidence(&r.source, &r.summary, r.data))
                    .collect()
            }
            "runbooks" => {
                ensure!(args == json!({}), "runbooks takes no arguments");
                let dir = self
                    .config
                    .runbooks
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("runbooks are not configured"))?;
                let mut paths = std::fs::read_dir(dir)?
                    .map(|p| p.map(|e| e.path()))
                    .collect::<std::io::Result<Vec<_>>>()?;
                paths.sort();
                let mut evidence = vec![];
                for path in paths
                    .into_iter()
                    .filter(|p| p.extension().is_some_and(|e| e == "md"))
                    .take(8)
                {
                    ensure!(
                        !path.symlink_metadata()?.file_type().is_symlink(),
                        "runbook symlinks are not allowed"
                    );
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    evidence.push(self.evidence(
                        "runbook",
                        &name,
                        json!({"text":read_limited(&path, MAX_EVIDENCE_TEXT)?}),
                    )?);
                }
                Ok(evidence)
            }
            "prometheus_query" | "loki_query" => {
                let query: Query = serde_json::from_value(args)?;
                ensure!(
                    !query.query.is_empty() && query.query.len() <= 2000,
                    "query must contain 1–2000 bytes"
                );
                let endpoint = if name == "prometheus_query" {
                    &self.config.prometheus
                } else {
                    &self.config.loki
                }
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("connector not configured"))?;
                let path = if name == "prometheus_query" {
                    "/api/v1/query"
                } else {
                    "/loki/api/v1/query_range"
                };
                let mut request = self
                    .client
                    .get(format!("{}{}", endpoint.url.trim_end_matches('/'), path))
                    .query(&[("query", query.query.as_str())]);
                if name == "loki_query" {
                    let end = Utc::now();
                    request = request.query(&[
                        ("start", (end - chrono::Duration::hours(1)).to_rfc3339()),
                        ("end", end.to_rfc3339()),
                        ("limit", "50".into()),
                    ]);
                }
                if let Some(var) = &endpoint.token_env {
                    request = request.bearer_auth(std::env::var(var)?);
                }
                let response = bounded_json(
                    request
                        .send()
                        .await
                        .map_err(|_| crate::harness::TransportFault)?,
                )
                .await?;
                ensure!(
                    response.get("status").and_then(Value::as_str) == Some("success"),
                    "connector returned an unsuccessful query result"
                );
                ensure!(
                    response
                        .pointer("/data/result")
                        .and_then(Value::as_array)
                        .is_some_and(|r| !r.is_empty()),
                    "query returned no matching series or log entries"
                );
                Ok(vec![self.evidence(name, &query.query, response)?])
            }
            _ => bail!("unknown or disallowed tool: {name}"),
        }
    }

    pub fn redact(&self, text: &str) -> String {
        let mut output = redact(text);
        let vars = [
            self.config
                .prometheus
                .as_ref()
                .and_then(|e| e.token_env.as_ref()),
            self.config.loki.as_ref().and_then(|e| e.token_env.as_ref()),
            self.config.model.as_ref().and_then(|e| e.key_env.as_ref()),
        ];
        for var in vars
            .into_iter()
            .flatten()
            .map(String::as_str)
            .chain(["SRE_API_TOKEN"])
        {
            if let Ok(value) = std::env::var(var)
                && !value.is_empty()
            {
                output = output.replace(&value, "[REDACTED]");
            }
        }
        for var in self
            .config
            .connections
            .iter()
            .flat_map(|c| c.token_env.iter().chain(c.header_env.values()))
            .chain(
                self.config
                    .mcp
                    .iter()
                    .flat_map(|s| s.token_env.iter().chain(s.env.values())),
            )
            .chain(
                self.config
                    .deliveries
                    .iter()
                    .flat_map(|d| d.token_env.iter().chain(d.url_env.iter())),
            )
            .chain(
                self.config
                    .actions
                    .iter()
                    .filter_map(|a| a.token_env.as_ref()),
            )
            .chain(
                self.config
                    .gateways
                    .slack
                    .iter()
                    .map(|s| &s.signing_secret_env),
            )
            .chain(self.config.gateways.telegram.iter().map(|s| &s.secret_env))
        {
            if let Ok(value) = std::env::var(var)
                && !value.is_empty()
            {
                output = output.replace(&value, "[REDACTED]");
            }
        }
        output
    }

    pub fn evidence(&self, source: &str, summary: &str, data: Value) -> Result<Evidence> {
        let mut data = data;
        self.redact_value(&mut data);
        let encoded = serde_json::to_string(&data)?;
        if encoded.len() > MAX_EVIDENCE_TEXT {
            data = json!({"truncated":true,"text":truncate(&encoded, MAX_EVIDENCE_TEXT)});
        }
        Ok(Evidence {
            id: String::new(),
            source: self.redact(&truncate(source, 128)),
            summary: self.redact(&truncate(summary, 2000)),
            observed_at: Utc::now(),
            data,
        })
    }

    pub fn redact_value(&self, value: &mut Value) {
        match value {
            Value::String(s) => *s = self.redact(s),
            Value::Array(values) => values.iter_mut().for_each(|v| self.redact_value(v)),
            Value::Object(values) => {
                for (key, value) in values.iter_mut() {
                    if [
                        "password",
                        "token",
                        "secret",
                        "authorization",
                        "api_key",
                        "apikey",
                        "access_token",
                    ]
                    .contains(&key.to_lowercase().as_str())
                    {
                        *value = json!("[REDACTED]");
                    } else {
                        self.redact_value(value);
                    }
                }
            }
            _ => {}
        }
    }
}

pub async fn bounded_json(mut response: reqwest::Response) -> Result<Value> {
    ensure!(
        response.status().is_success(),
        "upstream HTTP {}",
        response.status().as_u16()
    );
    ensure!(
        response
            .content_length()
            .is_none_or(|n| n <= MAX_RESPONSE as u64),
        "upstream response exceeds 256 KiB"
    );
    let mut bytes = vec![];
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| crate::harness::TransportFault)?
    {
        ensure!(
            bytes.len() + chunk.len() <= MAX_RESPONSE,
            "upstream response exceeds 256 KiB"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn read_limited(path: &Path, limit: usize) -> Result<String> {
    let mut text = String::new();
    std::fs::File::open(path)?
        .take((limit + 1) as u64)
        .read_to_string(&mut text)?;
    ensure!(text.len() <= limit, "file exceeds configured size limit");
    Ok(text)
}

pub fn truncate(text: &str, bytes: usize) -> String {
    let mut end = text.len().min(bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

pub fn redact(text: &str) -> String {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        [
            r"(?i)\b(?:bearer\s+)[A-Za-z0-9._~+/=-]+",
            r"(?i)\b(?:password|token|api[_-]?key|secret)\s*[=:]\s*[^\s,;]+",
            r"\b(?:sk-[A-Za-z0-9_-]{12,}|AKIA[A-Z0-9]{16}|gh[pousr]_[A-Za-z0-9]{20,})\b",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("static regex"))
        .collect()
    });
    patterns.iter().fold(text.to_string(), |s, p| {
        p.replace_all(&s, "[REDACTED]").into_owned()
    })
}
