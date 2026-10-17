use super::types::Protocol;
use crate::config::Model;
use anyhow::Result;
use serde_json::{Value, json};
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
