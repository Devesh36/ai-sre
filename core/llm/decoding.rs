use super::types::{Completion, Protocol};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
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
