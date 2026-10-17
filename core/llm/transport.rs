use super::{
    decoding::decode,
    encoding::encode,
    types::{Completion, Protocol},
};
use crate::{config::Model, tools::bounded_json};
use anyhow::{Result, ensure};
use serde_json::Value;
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
