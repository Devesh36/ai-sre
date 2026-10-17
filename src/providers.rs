//! Provider protocol adapters. Normalized messages use the chat-completions shape.
use crate::{config::Model, tools::bounded_json};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    #[default]
    OpenAi,
    Anthropic,
    Gemini,
    Responses,
}

pub struct Completion {
    pub message: Value,
    pub input: u64,
    pub output: u64,
    pub attempts: u32,
}

pub async fn complete(
    client: &reqwest::Client,
    model: &Model,
    messages: &[Value],
    tools: &[Value],
) -> Result<Completion> {
    complete_observed(client, model, messages, tools, |_| Ok(())).await
}

/// Calls the observer before every dispatch, including retries; failed attempts are accountable.
#[derive(Clone, Copy)]
pub enum AttemptEvent {
    Started,
    Retry { status: u16, delay_ms: u64 },
}

pub async fn complete_observed(
    client: &reqwest::Client,
    model: &Model,
    messages: &[Value],
    tools: &[Value],
    mut before_attempt: impl FnMut(AttemptEvent) -> Result<()>,
) -> Result<Completion> {
    let body = encode(model, messages, tools)?;
    ensure!(
        body.to_string().len() <= 512 * 1024,
        "model request exceeds 512 KiB; compact context or narrow evidence"
    );
    for attempt in 1..=3 {
        before_attempt(AttemptEvent::Started)?;
        let mut request = client.post(&model.url).json(&body);
        if let Some(var) = &model.key_env {
            let key = std::env::var(var)?;
            request = match model.protocol {
                Protocol::Anthropic => request.header("x-api-key", key),
                Protocol::Gemini => request.header("x-goog-api-key", key),
                _ => request.bearer_auth(key),
            };
        }
        if matches!(model.protocol, Protocol::Anthropic) {
            request = request.header("anthropic-version", "2023-06-01");
        }
        let response = request.send().await.map_err(|_| {
            anyhow::anyhow!("model transport failed; check destination, TLS, and timeout")
        })?;
        if matches!(response.status().as_u16(), 429 | 502 | 503 | 504) && attempt < 3 {
            let delay = if let Some(header) = response.headers().get("retry-after") {
                let seconds = header
                    .to_str()
                    .ok()
                    .and_then(|h| h.parse::<u64>().ok())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "model Retry-After is not an integer delay; retry deferred to operator"
                        )
                    })?;
                ensure!(
                    seconds <= 2,
                    "model requested retry delay beyond local retry window; retry deferred to operator"
                );
                seconds * 1000
            } else {
                250 * (1 << attempt)
            };
            before_attempt(AttemptEvent::Retry {
                status: response.status().as_u16(),
                delay_ms: delay,
            })?;
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            continue;
        }
        let value = bounded_json(response).await?;
        let mut completion = decode(model.protocol, &value)?;
        completion.attempts = attempt;
        return Ok(completion);
    }
    unreachable!()
}

pub fn encode(model: &Model, messages: &[Value], tools: &[Value]) -> Result<Value> {
    let definitions: Vec<Value> = tools.iter().map(|t| t["function"].clone()).collect();
    let system = messages
        .iter()
        .filter(|m| m["role"] == "system")
        .filter_map(|m| m["content"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let mut body = match model.protocol {
        Protocol::OpenAi => json!({"model":model.name,"messages":messages,"max_tokens":2500}),
        Protocol::Anthropic => {
            let mut output = vec![];
            for m in messages.iter().filter(|m| m["role"] != "system") {
                let role = if m["role"] == "assistant" {
                    "assistant"
                } else {
                    "user"
                };
                let mut blocks = vec![];
                if m["role"] == "tool" {
                    blocks.push(json!({"type":"tool_result","tool_use_id":m["tool_call_id"],"content":m["content"]}));
                } else {
                    if let Some(text) = m["content"].as_str().filter(|s| !s.is_empty()) {
                        blocks.push(json!({"type":"text","text":text}));
                    }
                    if let Some(calls) = m["tool_calls"].as_array() {
                        for call in calls {
                            blocks.push(json!({"type":"tool_use","id":call["id"],"name":call["function"]["name"],"input":arguments(call)?}));
                        }
                    }
                }
                // Adjacent tool results belong to one user turn for this protocol.
                if let Some(last) = output.last_mut().filter(|v: &&mut Value| v["role"] == role) {
                    last["content"]
                        .as_array_mut()
                        .expect("blocks")
                        .extend(blocks);
                } else {
                    output.push(json!({"role":role,"content":blocks}));
                }
            }
            json!({"model":model.name,"system":system,"messages":output,"max_tokens":2500})
        }
        Protocol::Gemini => {
            let mut contents = vec![];
            let mut call_names = std::collections::HashMap::new();
            for m in messages.iter().filter(|m| m["role"] != "system") {
                let mut parts = vec![];
                if m["role"] == "tool" {
                    let id = m["tool_call_id"].as_str().unwrap_or("");
                    let name = call_names
                        .get(id)
                        .ok_or_else(|| anyhow::anyhow!("unmatched tool result"))?;
                    let result: Value =
                        serde_json::from_str(m["content"].as_str().unwrap_or("{}"))?;
                    parts.push(json!({"functionResponse":{"name":name,"response":result}}));
                } else {
                    if let Some(text) = m["content"].as_str().filter(|s| !s.is_empty()) {
                        parts.push(json!({"text":text}));
                    }
                    if let Some(calls) = m["tool_calls"].as_array() {
                        for call in calls {
                            call_names.insert(
                                call["id"].as_str().unwrap_or("").to_owned(),
                                call["function"]["name"].clone(),
                            );
                            let mut part = json!({"functionCall":{"name":call["function"]["name"],"args":arguments(call)?}});
                            if let Some(signature) = call.get("thought_signature") {
                                part["thoughtSignature"] = signature.clone();
                            }
                            parts.push(part);
                        }
                    }
                }
                let role = if m["role"] == "assistant" {
                    "model"
                } else {
                    "user"
                };
                contents.push(json!({"role":role,"parts":parts}));
            }
            json!({"systemInstruction":{"parts":[{"text":system}]},"contents":contents,"generationConfig":{"maxOutputTokens":2500}})
        }
        Protocol::Responses => {
            let mut input = vec![];
            for m in messages.iter().filter(|m| m["role"] != "system") {
                if m["role"] == "tool" {
                    input.push(json!({"type":"function_call_output","call_id":m["tool_call_id"],"output":m["content"]}));
                } else {
                    if let Some(text) = m["content"].as_str().filter(|s| !s.is_empty()) {
                        input.push(json!({"role":m["role"],"content":text}));
                    }
                    if let Some(calls) = m["tool_calls"].as_array() {
                        for call in calls {
                            input.push(json!({"type":"function_call","call_id":call["id"],"name":call["function"]["name"],"arguments":call["function"]["arguments"]}));
                        }
                    }
                }
            }
            json!({"model":model.name,"instructions":system,"input":input,"max_output_tokens":2500,"store":false})
        }
    };
    if !tools.is_empty() {
        body["tools"] = match model.protocol {
            Protocol::OpenAi => json!(tools),
            Protocol::Anthropic => json!(definitions.iter().map(|d| json!({"name":d["name"],"description":d["description"],"input_schema":d["parameters"]})).collect::<Vec<_>>()),
            Protocol::Gemini => json!([{"functionDeclarations":definitions}]),
            Protocol::Responses => json!(definitions.iter().map(|d| json!({"type":"function","name":d["name"],"description":d["description"],"parameters":d["parameters"]})).collect::<Vec<_>>()),
        };
    }
    Ok(body)
}

fn arguments(call: &Value) -> Result<Value> {
    Ok(serde_json::from_str(
        call["function"]["arguments"].as_str().unwrap_or("{}"),
    )?)
}

pub fn decode(protocol: Protocol, value: &Value) -> Result<Completion> {
    let mut text = vec![];
    let mut calls = vec![];
    let (input, output) = match protocol {
        Protocol::OpenAi => {
            let message = value
                .pointer("/choices/0/message")
                .ok_or_else(|| anyhow::anyhow!("model response has no message"))?;
            return Ok(Completion {
                message: message.clone(),
                input: value["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
                output: value["usage"]["completion_tokens"].as_u64().unwrap_or(0),
                attempts: 1,
            });
        }
        Protocol::Anthropic => {
            for block in value["content"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("model response has no content"))?
            {
                if block["type"] == "text"
                    && let Some(t) = block["text"].as_str()
                {
                    text.push(t.to_string());
                }
                if block["type"] == "tool_use" {
                    calls.push(json!({"id":block["id"],"type":"function","function":{"name":block["name"],"arguments":block["input"].to_string()}}));
                }
            }
            (
                value["usage"]["input_tokens"].as_u64().unwrap_or(0),
                value["usage"]["output_tokens"].as_u64().unwrap_or(0),
            )
        }
        Protocol::Gemini => {
            for (index, part) in value
                .pointer("/candidates/0/content/parts")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow::anyhow!("model response has no candidate"))?
                .iter()
                .enumerate()
            {
                if let Some(t) = part["text"].as_str() {
                    text.push(t.to_string());
                }
                if let Some(call) = part.get("functionCall") {
                    let mut normalized = json!({"id":format!("call_{index}"),"type":"function","function":{"name":call["name"],"arguments":call["args"].to_string()}});
                    if let Some(signature) = part.get("thoughtSignature") {
                        normalized["thought_signature"] = signature.clone();
                    }
                    calls.push(normalized);
                }
            }
            (
                value["usageMetadata"]["promptTokenCount"]
                    .as_u64()
                    .unwrap_or(0),
                value["usageMetadata"]["candidatesTokenCount"]
                    .as_u64()
                    .unwrap_or(0),
            )
        }
        Protocol::Responses => {
            for item in value["output"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("model response has no output"))?
            {
                if item["type"] == "function_call" {
                    calls.push(json!({"id":item["call_id"],"type":"function","function":{"name":item["name"],"arguments":item["arguments"]}}));
                }
                if let Some(content) = item["content"].as_array() {
                    for block in content {
                        if let Some(t) = block["text"].as_str() {
                            text.push(t.to_string());
                        }
                    }
                }
            }
            (
                value["usage"]["input_tokens"].as_u64().unwrap_or(0),
                value["usage"]["output_tokens"].as_u64().unwrap_or(0),
            )
        }
    };
    ensure!(
        !text.is_empty() || !calls.is_empty(),
        "model returned neither text nor tool calls"
    );
    let mut message = json!({"role":"assistant","content":text.join("\n")});
    if !calls.is_empty() {
        message["tool_calls"] = json!(calls);
    }
    Ok(Completion {
        message,
        input,
        output,
        attempts: 1,
    })
}

pub struct Masker(Vec<(String, String)>);
impl Masker {
    pub fn new(values: &[String]) -> Self {
        let mut values = values.to_vec();
        values.sort_by_key(|v| std::cmp::Reverse(v.len()));
        values.dedup();
        Self(
            values
                .into_iter()
                .enumerate()
                .map(|(i, v)| (v, format!("__SRE_IDENTIFIER_{i}__")))
                .collect(),
        )
    }
    pub fn mask(&self, text: &str) -> String {
        self.0.iter().fold(text.into(), |s, (a, b)| s.replace(a, b))
    }
    pub fn restore(&self, text: &str) -> String {
        self.0.iter().fold(text.into(), |s, (a, b)| s.replace(b, a))
    }
    pub fn restore_value(&self, value: &mut Value) {
        map_strings(value, &|s| self.restore(s));
    }
    pub fn messages(&self, messages: &[Value]) -> Vec<Value> {
        let mut result = messages.to_vec();
        for message in &mut result {
            if let Some(text) = message["content"].as_str() {
                message["content"] = json!(self.payload(text));
            }
            if let Some(calls) = message["tool_calls"].as_array_mut() {
                for call in calls {
                    if let Some(args) = call["function"]["arguments"].as_str() {
                        call["function"]["arguments"] = json!(self.payload(args));
                    }
                }
            }
        }
        result
    }
    fn payload(&self, text: &str) -> String {
        if let Ok(mut value) = serde_json::from_str::<Value>(text) {
            map_strings(&mut value, &|s| self.mask(s));
            value.to_string()
        } else {
            self.mask(text)
        }
    }
}

fn map_strings(value: &mut Value, f: &impl Fn(&str) -> String) {
    match value {
        Value::String(s) => *s = f(s),
        Value::Array(a) => a.iter_mut().for_each(|v| map_strings(v, f)),
        Value::Object(o) => o.values_mut().for_each(|v| map_strings(v, f)),
        _ => {}
    }
}
