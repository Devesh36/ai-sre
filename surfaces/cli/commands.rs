use crate::{config::Config, domain::Incident, operations, store::Store, tools::Toolbox};
use anyhow::{Result, ensure};
use clap::Subcommand;
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Subcommand)]
pub enum Commands {
    /// Audit cited evidence for repeated payloads, collection age, and missing support.
    Review {
        id: Uuid,
        #[arg(long)]
        json: bool,
    },
    /// Compare retained observations from the last two runs; absence is not recovery.
    Changes {
        id: Uuid,
        #[arg(long)]
        json: bool,
    },
    /// Print a Markdown handoff with findings, evidence references, and outstanding checks.
    Handoff { id: Uuid },
    /// Inspect the complete logical module inventory and partial implementation links.
    Architecture {
        #[arg(long)]
        prefix: Option<String>,
        #[arg(long)]
        pending: bool,
    },
    /// Publish, inspect, activate, roll back, or import versioned workflows.
    Skill {
        #[command(subcommand)]
        command: SkillCommand,
    },
    /// Propose, review, and execute fixed-destination actions with verification.
    Action {
        #[command(subcommand)]
        command: ActionCommand,
    },
    /// List the exact native connector operations available for configuration.
    Catalog,
    /// Diagnose local configuration without printing credentials.
    Doctor,
    /// Inspect local coding-agent process names without reading their conversations.
    Fleet,
    /// Request cooperative cancellation of a running investigation.
    Cancel { id: Uuid },
    /// Compact future model context while preserving retained evidence and history.
    Compact { id: Uuid },
    /// Show token usage and optional cost calculated from configured prices.
    Cost { id: Uuid },
    /// Export run-correlated harness metadata without prompt or tool argument payloads.
    Trace { id: Uuid },
    /// Explicitly deliver an incident report to a configured destination.
    Deliver { id: Uuid, destination: String },
    /// Execute one configured read-only tool with explicit JSON arguments.
    Tool {
        name: String,
        #[arg(default_value = "{}")]
        arguments: String,
        #[arg(long, default_value = "unknown")]
        service: String,
    },
    /// Persistent service knowledge, included in future investigations.
    Memory {
        #[command(subcommand)]
        command: MemoryCommand,
    },
    /// Track operator work items.
    Work {
        #[command(subcommand)]
        command: WorkCommand,
    },
    /// Manage recurring investigations; the serve process runs the scheduler.
    Schedule {
        #[command(subcommand)]
        command: ScheduleCommand,
    },
}

#[derive(Subcommand)]
pub enum SkillCommand {
    List,
    Publish {
        path: std::path::PathBuf,
    },
    Show {
        name: String,
    },
    History {
        name: String,
    },
    Activate {
        name: String,
        digest: String,
    },
    Rollback {
        name: String,
    },
    Pull {
        url: String,
        #[arg(long)]
        token_env: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum ActionCommand {
    List,
    Propose {
        incident: Uuid,
        template: String,
        arguments: String,
        rationale: String,
    },
    Approve {
        id: Uuid,
        #[arg(long)]
        actor: String,
    },
    Reject {
        id: Uuid,
        #[arg(long)]
        actor: String,
    },
    Execute {
        id: Uuid,
    },
}

#[derive(Subcommand)]
pub enum MemoryCommand {
    List,
    Add {
        service: String,
        key: String,
        text: String,
    },
    Remove {
        id: Uuid,
    },
}
#[derive(Subcommand)]
pub enum WorkCommand {
    List,
    Add { service: String, text: String },
    Complete { id: Uuid },
}
#[derive(Subcommand)]
pub enum ScheduleCommand {
    List,
    Add {
        expression: String,
        question: String,
        #[arg(long)]
        service: String,
        #[arg(long, default_value = "UTC")]
        timezone: String,
    },
    Remove {
        id: Uuid,
    },
}

impl Commands {
    pub async fn run(self, config: &Config, store: &Store) -> Result<()> {
        let tools = Toolbox::new(config.clone())?;
        let output: Value = match self {
            Self::Review { id, json } | Self::Changes { id, json } => {
                let incident = store
                    .get(id)?
                    .ok_or_else(|| anyhow::anyhow!("incident not found"))?;
                let review = crate::domain::review::analyze(&incident, chrono::Utc::now());
                let changes_only = matches!(self, Self::Changes { .. });
                if json {
                    if changes_only {
                        serde_json::to_value(&review.changes)?
                    } else {
                        serde_json::to_value(&review)?
                    }
                } else {
                    if changes_only {
                        crate::surfaces::shared::review::print_changes(&review.changes);
                    } else {
                        crate::surfaces::shared::review::print_review(&review);
                    }
                    return Ok(());
                }
            }
            Self::Handoff { id } => {
                let incident = store
                    .get(id)?
                    .ok_or_else(|| anyhow::anyhow!("incident not found"))?;
                let review = crate::domain::review::analyze(&incident, chrono::Utc::now());
                print!("{}", crate::domain::review::handoff(&incident, &review));
                return Ok(());
            }
            Self::Architecture { prefix, pending } => {
                let inventory = crate::bootstrap::architecture::Inventory::bundled()?;
                if prefix.is_none() && !pending {
                    inventory.summary()
                } else {
                    json!(inventory.select(prefix.as_deref().unwrap_or(""), pending)?)
                }
            }
            Self::Skill { command } => {
                let registry =
                    crate::infrastructure::skills_registry::Registry::new(store.clone())?;
                match command {
                    SkillCommand::List => json!(registry.list()?),
                    SkillCommand::Publish { path } => {
                        let workflow: crate::config::Workflow =
                            toml::from_str(&crate::tools::read_limited(&path, 128 * 1024)?)?;
                        json!(registry.publish(workflow)?)
                    }
                    SkillCommand::Show { name } => json!(registry.active(&name)?),
                    SkillCommand::History { name } => json!(registry.history(&name)?),
                    SkillCommand::Activate { name, digest } => {
                        json!(registry.activate(&name, &digest)?)
                    }
                    SkillCommand::Rollback { name } => json!(registry.rollback(&name)?),
                    SkillCommand::Pull { url, token_env } => {
                        crate::infrastructure::skills_registry::client::pull(
                            config, store, &url, token_env,
                        )
                        .await?
                    }
                }
            }
            Self::Action { command } => match command {
                ActionCommand::List => json!(store.records("action")?),
                ActionCommand::Propose {
                    incident,
                    template,
                    arguments,
                    rationale,
                } => json!(crate::actions::propose(
                    config,
                    store,
                    incident,
                    &template,
                    serde_json::from_str(&arguments)?,
                    &rationale
                )?),
                ActionCommand::Approve { id, actor } => json!(crate::actions::review(
                    store,
                    id,
                    &tools.redact(&actor),
                    true
                )?),
                ActionCommand::Reject { id, actor } => json!(crate::actions::review(
                    store,
                    id,
                    &tools.redact(&actor),
                    false
                )?),
                ActionCommand::Execute { id } => {
                    json!(crate::actions::execute(config, store, id).await?)
                }
            },
            Self::Catalog => json!(crate::integrations::catalog()),
            Self::Doctor => {
                config.validate()?;
                json!({"database":"opened","model_configured":config.model.is_some(),"protocol":config.model.as_ref().map(|m|m.protocol),"native_connections":config.connections.iter().map(|c|&c.name).collect::<Vec<_>>(),"mcp_servers":config.mcp.iter().map(|c|&c.name).collect::<Vec<_>>(),"schedules":store.schedules()?.len(),"note":"Configuration validation only; use tool or integrations --verify for live checks."})
            }
            Self::Fleet => operations::scan_fleet(),
            Self::Cancel { id } => json!(store.cancel(id)?),
            Self::Compact { id } => json!(store.compact(id)?),
            Self::Trace { id } => json!(
                store
                    .get(id)?
                    .ok_or_else(|| anyhow::anyhow!("incident not found"))?
                    .trace
            ),
            Self::Cost { id } => json!(
                store
                    .get(id)?
                    .ok_or_else(|| anyhow::anyhow!("incident not found"))?
                    .usage
            ),
            Self::Deliver { id, destination } => {
                let incident = store
                    .get(id)?
                    .ok_or_else(|| anyhow::anyhow!("incident not found"))?;
                ensure!(
                    incident.status != crate::domain::Status::Investigating,
                    "wait for investigation to finish"
                );
                let output = operations::deliver(config, &incident, &destination).await?;
                store.mutate(id, |i| {
                    i.event(
                        "report_delivered",
                        format!("Operator delivered report to {destination}"),
                    );
                    Ok(())
                })?;
                output
            }
            Self::Tool {
                name,
                arguments,
                service,
            } => {
                for gap in tools.prepare().await {
                    eprintln!("{gap}");
                }
                json!(
                    tools
                        .call(&name, serde_json::from_str(&arguments)?, &service)
                        .await?
                )
            }
            Self::Memory { command } => match command {
                MemoryCommand::List => json!(store.records("memory")?),
                MemoryCommand::Remove { id } => {
                    json!({"removed":store.record_delete("memory",&id.to_string())?})
                }
                MemoryCommand::Add { service, key, text } => {
                    Incident::new(text.clone(), service.clone())?;
                    ensure!(
                        !key.is_empty() && key.len() <= 128,
                        "key must contain 1–128 bytes"
                    );
                    let record = operations::Memory {
                        id: Uuid::new_v4(),
                        service: tools.redact(&service),
                        key: tools.redact(&key),
                        text: tools.redact(&text),
                        updated_at: chrono::Utc::now(),
                    };
                    let value = json!(record);
                    store.record_put("memory", &record.id.to_string(), &value)?;
                    value
                }
            },
            Self::Work { command } => match command {
                WorkCommand::List => json!(store.records("work")?),
                WorkCommand::Complete { id } => {
                    store.record_mutate("work", &id.to_string(), |v| {
                        v["done"] = json!(true);
                        Ok(())
                    })?
                }
                WorkCommand::Add { service, text } => {
                    Incident::new(text.clone(), service.clone())?;
                    let record = operations::WorkItem {
                        id: Uuid::new_v4(),
                        service: tools.redact(&service),
                        text: tools.redact(&text),
                        done: false,
                        created_at: chrono::Utc::now(),
                    };
                    let value = json!(record);
                    store.record_put("work", &record.id.to_string(), &value)?;
                    value
                }
            },
            Self::Schedule { command } => match command {
                ScheduleCommand::List => json!(store.schedules()?),
                ScheduleCommand::Remove { id } => json!({"removed":store.schedule_remove(id)?}),
                ScheduleCommand::Add {
                    expression,
                    question,
                    service,
                    timezone,
                } => {
                    let schedule = operations::Schedule::new(
                        expression,
                        timezone,
                        tools.redact(&question),
                        tools.redact(&service),
                    )?;
                    store.schedule_add(&schedule)?;
                    json!(schedule)
                }
            },
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        Ok(())
    }
}
