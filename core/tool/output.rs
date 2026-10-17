// Copyright 2026 Tracer Cloud. SPDX-License-Identifier: Apache-2.0
// Modified: UTF-8-safe Rust translation with saturating arithmetic and instance budgets.
use crate::config::constants::tool_output::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Blocks(Vec<Map<String, Value>>),
}
impl From<String> for Content {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct OutputBudget {
    pub tokens: usize,
}
impl Default for OutputBudget {
    fn default() -> Self {
        Self {
            tokens: DEFAULT_TOOL_OUTPUT_TOKENS,
        }
    }
}
impl OutputBudget {
    pub fn bytes(self, history: bool) -> usize {
        let tokens = if history {
            self.tokens
                .saturating_mul(TOOL_OUTPUT_HISTORY_NUMERATOR)
                .saturating_add(TOOL_OUTPUT_HISTORY_DENOMINATOR - 1)
                / TOOL_OUTPUT_HISTORY_DENOMINATOR
        } else {
            self.tokens
        };
        tokens.saturating_mul(TOOL_OUTPUT_BYTES_PER_TOKEN)
    }
    pub fn replay_bytes(self) -> usize {
        self.bytes(true).max(self.bytes(false).saturating_add(256))
    }
    pub fn bound(self, content: &Content) -> Content {
        let budget = self.bytes(false);
        match content {
            Content::Text(text) => truncate_output_text(text, budget, true).into(),
            Content::Blocks(blocks) => {
                let parts: Vec<_> = blocks
                    .iter()
                    .filter_map(|b| b.get("text").and_then(Value::as_str))
                    .collect();
                let joined = parts.join("\n");
                if joined.len() <= budget {
                    return content.clone();
                }
                let truncated = truncate_output_text(&joined, budget, true);
                let mut placed = false;
                Content::Blocks(
                    blocks
                        .iter()
                        .filter_map(|b| {
                            if b.get("text").is_some_and(Value::is_string) {
                                if placed {
                                    return None;
                                }
                                placed = true;
                                let mut first = b.clone();
                                first.insert("text".into(), Value::String(truncated.clone()));
                                Some(first)
                            } else {
                                Some(b.clone())
                            }
                        })
                        .collect(),
                )
            }
        }
    }
}
/// The marker is outside the retained body budget. Non-text blocks are not reduced.
pub fn truncate_output_text(text: &str, max_bytes: usize, tokens: bool) -> String {
    if text.len() <= max_bytes {
        return text.into();
    }
    let mut left = max_bytes / 2;
    let mut right = text.len() - (max_bytes - left);
    while !text.is_char_boundary(left) {
        left -= 1;
    }
    while !text.is_char_boundary(right) {
        right += 1;
    }
    let omitted = if tokens {
        (text.len() - max_bytes).div_ceil(TOOL_OUTPUT_BYTES_PER_TOKEN)
    } else {
        text[left..right].chars().count()
    };
    let unit = if tokens { "tokens" } else { "chars" };
    format!(
        "{}…{omitted} {unit} truncated…{}",
        &text[..left],
        &text[right..]
    )
}
