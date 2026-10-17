use super::Registry;
use crate::{
    config::{Config, check_env, validate_url},
    core::agent_harness::skills::Bundle,
    store::Store,
    tools::bounded_json,
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Pulling imports a validated immutable release; activation is always a separate operator action.
pub async fn pull(
    config: &Config,
    store: &Store,
    url: &str,
    token_env: Option<String>,
) -> Result<Value> {
    validate_url(url)?;
    check_env(&token_env)?;
    let key = format!("{:x}", Sha256::digest(url.as_bytes()));
    let prior = store.record_get("workflow_remote", &key)?;
    let mut request = config.client()?.get(url);
    if let Some(var) = token_env {
        request = request.bearer_auth(std::env::var(var)?);
    }
    if let Some(etag) = prior.as_ref().and_then(|v| v["etag"].as_str()) {
        request = request.header("if-none-match", etag);
    }
    let response = request
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("workflow registry transport failed"))?;
    let registry = Registry::new(store.clone())?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        let cached = prior
            .ok_or_else(|| anyhow::anyhow!("registry returned 304 without a cached release"))?;
        let name = cached["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("invalid cached release"))?;
        let digest = cached["digest"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("invalid cached digest"))?;
        return Ok(
            json!({"status":"unchanged","release":registry.get(name,digest)?,"activated":false}),
        );
    }
    let etag = response
        .headers()
        .get("etag")
        .and_then(|h| h.to_str().ok())
        .filter(|s| s.len() <= 256)
        .map(str::to_owned);
    let bundle: Bundle = serde_json::from_value(bounded_json(response).await?)?;
    bundle.validate()?;
    ensure!(
        serde_json::to_vec(&bundle)?.len() <= 128 * 1024,
        "workflow bundle too large"
    );
    let release = registry.publish(bundle.workflow)?;
    store.record_put(
        "workflow_remote",
        &key,
        &json!({"etag":etag,"name":release.workflow.name,"digest":release.digest}),
    )?;
    Ok(json!({"status":"imported","release":release,"activated":false}))
}
