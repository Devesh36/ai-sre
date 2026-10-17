use serde::{Deserialize, Serialize};
use serde_json::Value;
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
