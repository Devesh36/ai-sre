//! Owned local MCP process. No shell interpolation or model-selected commands.
use super::{Server, rpc_result};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::process::Stdio;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

const MAX_FRAME: usize = 256 * 1024;
const MAX_EXCHANGE: usize = 1024 * 1024;
const MAX_MESSAGES: usize = 64;

pub(super) struct Process {
    // Tokio kills and arranges reaping when dropped, including aborted futures.
    _child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Process {
    pub(super) fn spawn(config: &Server) -> Result<Self> {
        let mut command = Command::new(config.command.as_deref().context("MCP command missing")?);
        command.args(&config.args).env_clear();
        // Preserve executable lookup, platform runtime, and managed network/CA settings.
        // All application credentials require explicit operator-configured mappings.
        for key in [
            "PATH",
            "HOME",
            "USER",
            "LOGNAME",
            "TMPDIR",
            "TMP",
            "TEMP",
            "SYSTEMROOT",
            "WINDIR",
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
            "no_proxy",
            "SSL_CERT_FILE",
            "SSL_CERT_DIR",
            "REQUESTS_CA_BUNDLE",
            "NODE_EXTRA_CA_CERTS",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        for (key, variable) in &config.env {
            command.env(
                key,
                std::env::var(variable).context("MCP environment variable unavailable")?,
            );
        }
        command.env("NO_COLOR", "1").env("TERM", "dumb");
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Discard diagnostics rather than retaining secrets or blocking on a full pipe.
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| anyhow::anyhow!("MCP executable could not be started"))?;
        let input = child.stdin.take().context("MCP stdin unavailable")?;
        let output = BufReader::new(child.stdout.take().context("MCP stdout unavailable")?);
        Ok(Self {
            _child: child,
            input,
            output,
        })
    }

    async fn write(&mut self, body: &Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(body)?;
        ensure!(bytes.len() < MAX_FRAME, "MCP request exceeds 256 KiB");
        bytes.push(b'\n');
        self.input
            .write_all(&bytes)
            .await
            .context("MCP stdin closed")?;
        self.input.flush().await.context("MCP stdin flush failed")
    }

    async fn read(&mut self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        loop {
            let available = self
                .output
                .fill_buf()
                .await
                .context("MCP stdout read failed")?;
            ensure!(
                !available.is_empty(),
                "MCP process closed stdout before a complete response"
            );
            let newline = available.iter().position(|b| *b == b'\n');
            let count = newline.map_or(available.len(), |i| i + 1);
            ensure!(
                bytes.len() + count <= MAX_FRAME,
                "MCP response exceeds 256 KiB"
            );
            bytes.extend_from_slice(&available[..count]);
            self.output.consume(count);
            if newline.is_some() {
                return Ok(bytes);
            }
        }
    }

    pub(super) async fn exchange(
        &mut self,
        body: &Value,
        id: u64,
        notification: bool,
    ) -> Result<Value> {
        self.write(body).await?;
        if notification {
            return Ok(Value::Null);
        }
        let mut total = 0;
        for _ in 0..MAX_MESSAGES {
            let bytes = self.read().await?;
            total += bytes.len();
            ensure!(total <= MAX_EXCHANGE, "MCP exchange exceeds 1 MiB");
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|_| anyhow::anyhow!("MCP stdout contains invalid JSON"))?;
            ensure!(value["jsonrpc"] == "2.0", "MCP response version mismatch");
            if let Some(method) = value.get("method") {
                ensure!(
                    method.is_string()
                        && value.get("result").is_none()
                        && value.get("error").is_none(),
                    "invalid MCP server message"
                );
                if let Some(request_id) = value.get("id") {
                    ensure!(
                        request_id.is_string() || request_id.is_number(),
                        "invalid MCP server request ID"
                    );
                    let response = if method == "ping" {
                        json!({"jsonrpc":"2.0","id":request_id,"result":{}})
                    } else {
                        // No sampling, elicitation or filesystem-root authority is granted.
                        json!({"jsonrpc":"2.0","id":request_id,"error":{"code":-32601,"message":"Method not supported"}})
                    };
                    self.write(&response).await?;
                }
                // Notifications (including progress) carry no authority and are bounded above.
                continue;
            }
            return rpc_result(value, id);
        }
        anyhow::bail!("MCP exchange exceeded 64 messages")
    }
}
