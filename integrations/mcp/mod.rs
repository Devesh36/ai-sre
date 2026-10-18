//! MCP client with bounded HTTP/stdio transports and an explicit tool allowlist.
//! Transport lifecycle adapted from the Apache-2.0 reference; see THIRD_PARTY_NOTICES.md.
mod stdio;
use crate::config::{check_env, validate_url};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::Mutex;

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Transport {
    #[default]
    StreamableHttp,
    Stdio,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Server {
    pub name: String,
    #[serde(default)]
    pub mode: Transport,
    #[serde(default)]
    pub url: String,
    pub token_env: Option<String>,
    pub allowed_tools: Vec<String>,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    /// Child variable name -> parent variable name. Values never enter configuration.
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}
fn default_timeout() -> u64 {
    30
}
impl Default for Server {
    fn default() -> Self {
        Self {
            name: String::new(),
            mode: Transport::default(),
            url: String::new(),
            token_env: None,
            allowed_tools: vec![],
            command: None,
            args: vec![],
            env: Default::default(),
            timeout_seconds: default_timeout(),
        }
    }
}
impl Server {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            crate::integrations::valid_name(&self.name),
            "invalid MCP server name"
        );
        ensure!(
            (1..=300).contains(&self.timeout_seconds),
            "MCP timeout must be 1–300 seconds"
        );
        match self.mode {
            Transport::StreamableHttp => {
                validate_url(&self.url)?;
                check_env(&self.token_env)?;
                ensure!(
                    self.command.is_none() && self.args.is_empty() && self.env.is_empty(),
                    "HTTP MCP cannot configure a local process"
                );
            }
            Transport::Stdio => {
                ensure!(
                    self.url.is_empty() && self.token_env.is_none(),
                    "stdio MCP cannot configure HTTP credentials or a URL"
                );
                ensure!(
                    self.command.as_ref().is_some_and(|c| !c.is_empty()
                        && c.len() <= 4096
                        && !c.chars().any(char::is_control)),
                    "stdio MCP requires a valid executable command"
                );
                ensure!(
                    self.args.len() <= 64
                        && self
                            .args
                            .iter()
                            .all(|a| a.len() <= 8192 && !a.contains('\0')),
                    "MCP command arguments exceed limits"
                );
                ensure!(
                    self.env.len() <= 32,
                    "MCP permits at most 32 environment mappings"
                );
                for (key, variable) in &self.env {
                    ensure!(
                        valid_env_name(key) && valid_env_name(variable),
                        "invalid MCP environment variable name"
                    );
                    check_env(&Some(variable.clone()))?;
                }
            }
        }
        ensure!(
            !self.allowed_tools.is_empty() && self.allowed_tools.len() <= 32,
            "MCP requires an explicit allowlist of 1–32 tools"
        );
        ensure!(
            self.allowed_tools.iter().all(|t| !t.is_empty()
                && t.len() <= 32
                && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')),
            "MCP tool names must be alphanumeric or underscores, maximum 32 bytes"
        );
        Ok(())
    }
}
fn valid_env_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .enumerate()
            .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit()))
}

#[derive(Clone)]
pub struct Client {
    pub config: Server,
    http: reqwest::Client,
    session: Arc<Mutex<Option<String>>>,
    tools: Arc<std::sync::RwLock<Vec<Value>>>,
    next_id: Arc<AtomicU64>,
    local: Arc<Mutex<Option<stdio::Process>>>,
    operation: Arc<Mutex<()>>,
}

impl Client {
    pub fn new(config: Server, http: reqwest::Client) -> Self {
        Self {
            config,
            http,
            session: Arc::new(Mutex::new(None)),
            tools: Arc::new(std::sync::RwLock::new(vec![])),
            next_id: Arc::new(AtomicU64::new(1)),
            local: Arc::new(Mutex::new(None)),
            operation: Arc::new(Mutex::new(())),
        }
    }

    pub async fn initialize(&self) -> Result<()> {
        let _operation = self.operation.lock().await;
        self.config.validate()?;
        self.tools
            .write()
            .map_err(|_| anyhow::anyhow!("MCP registry poisoned"))?
            .clear();
        self.local.lock().await.take();
        *self.session.lock().await = None;
        let result = self.discover().await;
        if result.is_err() {
            self.local.lock().await.take();
        }
        result
    }

    async fn discover(&self) -> Result<()> {
        let result = self.rpc("initialize",json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"ai-sre","version":env!("CARGO_PKG_VERSION")}}), false).await?;
        ensure!(
            result["protocolVersion"] == "2025-03-26",
            "unsupported negotiated MCP protocol version"
        );
        self.rpc("notifications/initialized", json!({}), true)
            .await?;
        let mut cursor = None;
        let mut accepted = vec![];
        let mut complete = false;
        for _ in 0..8 {
            let params = cursor.map_or_else(|| json!({}), |c: Value| json!({"cursor":c}));
            let page = self.rpc("tools/list", params, false).await?;
            for tool in page["tools"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("MCP tools/list missing tools"))?
            {
                let name = tool["name"].as_str().unwrap_or("");
                if self.config.allowed_tools.iter().any(|n| n == name) {
                    ensure!(
                        tool.pointer("/annotations/readOnlyHint") == Some(&Value::Bool(true)),
                        "MCP allowlisted tool {name} is not declared read-only"
                    );
                    ensure!(
                        tool["inputSchema"]["type"] == "object",
                        "MCP tool schema must describe an object"
                    );
                    ensure!(
                        !accepted.iter().any(|t: &Value| t["name"] == name),
                        "duplicate MCP tool name"
                    );
                    accepted.push(tool.clone());
                }
            }
            cursor = page.get("nextCursor").cloned();
            if cursor.is_none() {
                complete = true;
                break;
            }
        }
        ensure!(complete, "MCP tool discovery exceeded eight pages");
        ensure!(
            self.config
                .allowed_tools
                .iter()
                .all(|n| accepted.iter().any(|t| t["name"] == n.as_str())),
            "one or more allowlisted MCP tools were not advertised"
        );
        *self
            .tools
            .write()
            .map_err(|_| anyhow::anyhow!("MCP registry poisoned"))? = accepted;
        Ok(())
    }

    pub fn definitions(&self) -> Vec<Value> {
        self.tools.read().map(|tools|tools.iter().map(|t|json!({"type":"function","function":{
            "name":format!("mcp_{}_{}",self.config.name,t["name"].as_str().unwrap_or("")),
            "description":t["description"].as_str().unwrap_or("Read-only remote tool"),"parameters":t["inputSchema"]
        }})).collect()).unwrap_or_default()
    }

    pub async fn call(&self, name: &str, arguments: Value) -> Result<Value> {
        let _operation = self.operation.lock().await;
        ensure!(arguments.is_object(), "MCP arguments must be an object");
        ensure!(
            self.tools
                .read()
                .map_err(|_| anyhow::anyhow!("MCP registry poisoned"))?
                .iter()
                .any(|t| t["name"] == name),
            "MCP tool is not authorized or initialized"
        );
        let result = self
            .rpc(
                "tools/call",
                json!({"name":name,"arguments":arguments}),
                false,
            )
            .await?;
        ensure!(result["isError"] != true, "MCP tool reported an error");
        Ok(result)
    }

    async fn rpc(&self, method: &str, params: Value, notification: bool) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut body = json!({"jsonrpc":"2.0","method":method,"params":params});
        if !notification {
            body["id"] = json!(id);
        }
        let operation = async {
            if self.config.mode == Transport::Stdio {
                // Move ownership into this future: cancellation or timeout drops and kills
                // the child, so a late reply can never satisfy a subsequent request.
                let mut process = match self.local.lock().await.take() {
                    Some(process) => process,
                    None => {
                        ensure!(
                            method == "initialize",
                            "MCP local session ended; initialize again before calling tools"
                        );
                        stdio::Process::spawn(&self.config)?
                    }
                };
                let result = process.exchange(&body, id, notification).await?;
                *self.local.lock().await = Some(process);
                Ok(result)
            } else {
                self.http_rpc(method, body, id, notification).await
            }
        };
        tokio::time::timeout(
            std::time::Duration::from_secs(self.config.timeout_seconds),
            operation,
        )
        .await
        .map_err(|_| anyhow::anyhow!("MCP request timed out"))?
    }

    async fn http_rpc(
        &self,
        method: &str,
        body: Value,
        id: u64,
        notification: bool,
    ) -> Result<Value> {
        let mut request = self
            .http
            .post(&self.config.url)
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", "2025-03-26")
            .json(&body);
        if let Some(session) = self.session.lock().await.clone() {
            request = request.header("Mcp-Session-Id", session);
        }
        if let Some(var) = &self.config.token_env {
            request = request.bearer_auth(std::env::var(var)?);
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| crate::harness::TransportFault)?;
        ensure!(
            response.status().is_success(),
            "MCP HTTP {}",
            response.status().as_u16()
        );
        if method == "initialize"
            && let Some(session) = response.headers().get("mcp-session-id")
        {
            let value = session.to_str()?;
            ensure!(value.len() <= 1024, "invalid MCP session id");
            *self.session.lock().await = Some(value.into());
        }
        if notification {
            return Ok(Value::Null);
        }
        let sse = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream"));
        let mut bytes = vec![];
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| crate::harness::TransportFault)?
        {
            ensure!(
                bytes.len() + chunk.len() <= 256 * 1024,
                "MCP response exceeds 256 KiB"
            );
            bytes.extend_from_slice(&chunk);
            if sse && let Some(value) = parse_sse(&bytes, id)? {
                return rpc_result(value, id);
            }
        }
        ensure!(!sse, "MCP stream ended without a matching result");
        rpc_result(serde_json::from_slice(&bytes)?, id)
    }
}

fn rpc_result(value: Value, id: u64) -> Result<Value> {
    ensure!(
        value["jsonrpc"] == "2.0" && value["id"] == id,
        "MCP response ID or version mismatch"
    );
    ensure!(
        value.get("error").is_none(),
        "MCP returned a protocol error"
    );
    value
        .get("result")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("MCP response has no result"))
}

fn parse_sse(bytes: &[u8], id: u64) -> Result<Option<Value>> {
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) if e.error_len().is_none() => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let normalized = text.replace("\r\n", "\n");
    for block in normalized.split_terminator("\n\n") {
        // A final partial block must not be parsed before the event delimiter arrives.
        if !normalized.ends_with("\n\n") && normalized.ends_with(block) {
            continue;
        }
        let data = block
            .lines()
            .filter_map(|l| l.strip_prefix("data:").map(str::trim_start))
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(&data)?;
        if value["id"] == id {
            return Ok(Some(value));
        }
    }
    Ok(None)
}
