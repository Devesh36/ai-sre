use super::catalog;
use crate::{
    config::{check_env, validate_url},
    tools::bounded_json,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub name: String,
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub token_env: Option<String>,
    #[serde(default)]
    pub header_env: BTreeMap<String, String>,
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 20
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

impl Connection {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            valid_name(&self.name),
            "connection names must contain 1–20 lowercase letters, digits, or underscores"
        );
        ensure!(
            catalog().iter().any(|o| o.kind == self.kind),
            "unsupported connector kind: {}",
            self.kind
        );
        validate_url(&self.url)?;
        check_env(&self.token_env)?;
        for (header, var) in &self.header_env {
            let parsed = reqwest::header::HeaderName::from_bytes(header.as_bytes())?;
            ensure!(
                !matches!(
                    parsed.as_str(),
                    "host" | "content-length" | "transfer-encoding" | "connection"
                ),
                "transport header cannot be overridden"
            );
            check_env(&Some(var.clone()))?;
        }
        Ok(())
    }

    pub fn definitions(&self) -> Vec<Value> {
        catalog().into_iter().filter(|o| o.kind == self.kind).map(|o| {
            let mut properties = serde_json::Map::new();
            let mut required = vec![];
            for name in placeholders(o.path) {
                if !self.parameters.contains_key(name) { properties.insert(name.into(), json!({"type":"string","minLength":1,"maxLength":1024})); required.push(name); }
            }
            for key in o.query { properties.insert((*key).into(), json!({"type":"string","maxLength":2000})); }
            if o.search_body { properties.insert("body".into(), json!({"type":"object","description":"Read-only search DSL; bounded response required"})); required.push("body"); }
            json!({"type":"function","function":{"name":format!("conn_{}_{}",self.name,o.name),"description":format!("{} ({})",o.description,self.name),"parameters":{"type":"object","properties":properties,"required":required,"additionalProperties":false}}})
        }).collect()
    }

    pub async fn call(
        &self,
        client: &reqwest::Client,
        operation: &str,
        args: &Value,
    ) -> Result<Value> {
        let op = catalog()
            .into_iter()
            .find(|o| o.kind == self.kind && o.name == operation)
            .ok_or_else(|| anyhow::anyhow!("operation is not allowed"))?;
        let args = args
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("tool arguments must be an object"))?;
        let path_keys = placeholders(op.path);
        for key in args.keys() {
            if key != "body" {
                ensure!(args[key].is_string(), "argument {key} must be a string");
            }
            ensure!(
                (path_keys.contains(&key.as_str()) && !self.parameters.contains_key(key))
                    || op.query.contains(&key.as_str())
                    || (op.search_body && key == "body"),
                "unexpected or fixed argument: {key}"
            );
        }
        let mut url = reqwest::Url::parse(&self.url)?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| anyhow::anyhow!("invalid base URL"))?;
            segments.pop_if_empty();
            for segment in op.path.trim_start_matches('/').split('/') {
                if let Some(key) = segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                    let value = self
                        .parameters
                        .get(key)
                        .map(String::as_str)
                        .or_else(|| args.get(key).and_then(Value::as_str))
                        .ok_or_else(|| anyhow::anyhow!("missing path argument: {key}"))?;
                    ensure!(
                        !value.is_empty() && value.len() <= 1024 && value != "." && value != "..",
                        "invalid path argument"
                    );
                    segments.push(value);
                } else {
                    segments.push(segment);
                }
            }
        }
        for key in op.query {
            if let Some(value) = args
                .get(*key)
                .and_then(Value::as_str)
                .or_else(|| self.parameters.get(*key).map(String::as_str))
            {
                ensure!(value.len() <= 2000, "query argument too long");
                url.query_pairs_mut().append_pair(key, value);
            } else if matches!(
                *key,
                "limit" | "per_page" | "page_size" | "pagelen" | "maxResults" | "sysparm_limit"
            ) {
                url.query_pairs_mut().append_pair(key, "50");
            }
        }
        let mut request = if op.search_body {
            let mut body = args
                .get("body")
                .and_then(Value::as_object)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("search body must be an object"))?;
            ensure!(
                !body.contains_key("script_fields") && !body.contains_key("runtime_mappings"),
                "scripted search fields are not allowed"
            );
            body.insert(
                "size".into(),
                json!(
                    body.get("size")
                        .and_then(Value::as_u64)
                        .unwrap_or(50)
                        .min(100)
                ),
            );
            client.post(url).json(&body)
        } else {
            client.get(url)
        };
        request = request
            .header("User-Agent", "ai-sre/0.1")
            .header("Accept", "application/json");
        if let Some(var) = &self.token_env {
            let token = std::env::var(var)?;
            request = match self.kind.as_str() {
                "pagerduty" => request.header("Authorization", format!("Token token={token}")),
                "opsgenie" => request.header("Authorization", format!("GenieKey {token}")),
                "discord" => request.header("Authorization", format!("Bot {token}")),
                "honeycomb" => request.header("X-Honeycomb-Team", token),
                _ => request.bearer_auth(token),
            };
        }
        for (header, var) in &self.header_env {
            request = request.header(header, std::env::var(var)?);
        }
        let response = bounded_json(
            request
                .send()
                .await
                .map_err(|_| crate::harness::TransportFault)?,
        )
        .await?;
        ensure!(
            response.get("ok") != Some(&Value::Bool(false)),
            "connector reported an API error"
        );
        ensure!(
            response.get("error").is_none(),
            "connector reported an error"
        );
        Ok(response)
    }
}

fn placeholders(path: &str) -> Vec<&str> {
    path.split('/')
        .filter_map(|s| s.strip_prefix('{').and_then(|s| s.strip_suffix('}')))
        .collect()
}
