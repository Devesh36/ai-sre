use crate::{config::check_env, domain::Incident};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Gateways {
    pub slack: Option<Slack>,
    pub telegram: Option<Telegram>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slack {
    pub signing_secret_env: String,
    pub allowed_channels: Vec<String>,
    pub allowed_users: Vec<String>,
    pub service: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Telegram {
    pub secret_env: String,
    pub allowed_chats: Vec<i64>,
    pub allowed_users: Vec<i64>,
    pub service: String,
}
impl Gateways {
    pub fn validate(&self) -> Result<()> {
        if let Some(s) = &self.slack {
            check_env(&Some(s.signing_secret_env.clone()))?;
            ensure!(
                !s.allowed_channels.is_empty() && !s.allowed_users.is_empty(),
                "Slack requires channel and user allowlists"
            );
            Incident::new("gateway validation".into(), s.service.clone())?;
        }
        if let Some(s) = &self.telegram {
            check_env(&Some(s.secret_env.clone()))?;
            ensure!(
                !s.allowed_chats.is_empty() && !s.allowed_users.is_empty(),
                "Telegram requires chat and user allowlists"
            );
            Incident::new("gateway validation".into(), s.service.clone())?;
        }
        Ok(())
    }
}
