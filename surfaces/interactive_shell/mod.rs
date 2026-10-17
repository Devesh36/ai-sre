pub mod editor;
use crate::{
    config::Config,
    infrastructure::terminal::history::History,
    store::Store,
    surfaces::{
        cli::headless::ask,
        shared::{
            output::print_incident,
            terminal::{self, Ui},
        },
    },
    tools::Toolbox,
};
use anyhow::{Result, ensure};
use editor::{Input, Prompt};
use uuid::Uuid;

fn welcome(ui: Ui, config: &Config, service: &str) {
    println!();
    ui.rule();
    println!("  {}", ui.style("AI SRE", "1;36"));
    ui.note("TERMINAL WORKSPACE");
    ui.text(&format!(
        "v{} · {}",
        env!("CARGO_PKG_VERSION"),
        config
            .model
            .as_ref()
            .map(|m| format!("Model: {}", m.name))
            .unwrap_or_else(|| "Offline triage · no model configured".into())
    ));
    ui.text(&format!("Service: {service} · local history"));
    ui.rule();
    ui.text("Ask an incident question. Follow the evidence. Keep the context.");
    if config.model.is_none() {
        ui.note("Add a [model] block to sre.toml for model-assisted investigations.");
    }
    if config.evidence_file.is_none()
        && config.connections.is_empty()
        && config.mcp.is_empty()
        && config.prometheus.is_none()
        && config.loki.is_none()
    {
        ui.note("No evidence sources configured. Use ai-sre init to create sre.toml, or ai-sre demo to try synthetic data.");
    }
    ui.note("/help commands · /service NAME choose a service · /exit quit");
    if terminal::interactive() {
        ui.note("Tab complete · ↑ ↓ history · Ctrl+R search · Ctrl+C cancel · Ctrl+D quit");
    }
    println!();
}
fn load_history(prompt: &mut Prompt, history: &History, service: &str) -> Result<()> {
    prompt.replace_history(
        history
            .list(service, 200)?
            .into_iter()
            .rev()
            .map(|entry| entry.text),
    )
}
fn valid_service(service: &str) -> Result<()> {
    ensure!(
        !service.is_empty() && service.len() <= 128 && !service.chars().any(char::is_control),
        "service must contain 1–128 bytes and no control characters"
    );
    Ok(())
}

pub async fn run(config: &Config, store: &Store, initial_service: &str) -> Result<()> {
    valid_service(initial_service)?;
    let history = History::new(store.clone())?;
    let tools = Toolbox::new(config.clone())?;
    let mut service = tools.redact(initial_service);
    let mut active: Option<Uuid> = None;
    let mut prompt = Prompt::new(Ui::detect().color)?;
    load_history(&mut prompt, &history, &service)?;
    welcome(Ui::detect(), config, &service);
    loop {
        let ui = Ui::detect();
        let label = terminal::clean(&service)
            .chars()
            .take(24)
            .collect::<String>();
        let session = active
            .map(|id| format!(" · {}", &id.to_string()[..8]))
            .unwrap_or_default();
        let line = match prompt.read(format!("{label}{session} › ")).await? {
            Input::Eof => break,
            Input::Interrupted => {
                ui.note("Input cancelled. Ctrl+D or /exit to quit.");
                continue;
            }
            Input::Line(line) => line,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "/exit" || line == "/quit" {
            break;
        }
        // Command failures remain local to this prompt instead of closing the workspace.
        let result:Result<()>=async {
            match line {
                "/help"=>{
                    ui.heading("Commands");
                    for (command,description) in editor::COMMANDS {ui.text(&format!("{command}  —  {description}"));}
                    ui.heading("Keyboard");ui.text("Tab completes slash commands. Up/Down recall questions. Ctrl+R searches history. Ctrl+C clears input or cancels an investigation. Ctrl+D exits from an empty prompt.");
                    ui.heading("Try a question");ui.text("Why is checkout failing after its latest release?");
                },
                "/new"=>{active=None;ui.note("Started a new conversation. Previous investigations remain saved.");},
                "/sessions"=>{
                    ui.heading("Saved investigations");let items=store.list()?;
                    if items.is_empty(){ui.text("No investigations yet. Ask a question to start one.");}
                    for i in items {ui.text(&format!("{} · {:?} · {}\n{}",i.id,i.status,i.service,i.title));}
                    ui.note("Continue with /resume FULL_ID");
                },
                "/status"|"/cost"|"/trace"|"/evidence"|"/compact"=>{
                    let Some(id)=active else {ui.note("No active investigation. Ask a question or use /resume ID.");return Ok(());};
                    let i=store.get(id)?.ok_or_else(||anyhow::anyhow!("Investigation no longer exists"))?;
                    match line {
                        "/status"=>{ui.heading("Current investigation");ui.text(&format!("{}\n{} · {:?} · {}",i.title,i.id,i.status,i.service));},
                        "/cost"=>{ui.heading("Usage");ui.text(&format!("{} model calls · {} tool calls\n{} input tokens · {} output tokens",i.usage.model_calls,i.usage.tool_calls,i.usage.input_tokens,i.usage.output_tokens));ui.text(&i.usage.estimated_cost_usd.map(|cost|format!("Estimated cost: ${cost:.4}")).unwrap_or_else(||"Cost unavailable; configure model token prices to estimate it.".into()));},
                        "/trace"=>{ui.heading("Execution trace");for t in &i.trace{ui.text(&format!("{:>6} ms  {} · {}",t.elapsed_ms,t.phase.replace('_'," "),t.operation));}},
                        "/evidence"=>{ui.heading("Retained evidence");if i.evidence.is_empty(){ui.text("No evidence collected yet.");}for e in &i.evidence{ui.text(&format!("[{}] {} · {}\n{}",e.id,e.source,e.observed_at,e.summary));}},
                        _=>{store.compact(id)?;ui.note("Context compacted; original evidence retained.");}
                    }
                },
                "/history"=>{ui.heading(&format!("Recent questions · {service}"));let entries=history.list(&service,20)?;if entries.is_empty(){ui.text("No saved questions for this service.");}for e in entries {ui.text(&format!("{}  {}",e.created_at,e.text));}},
                "/clear-history"=>{let count=history.clear(&service)?;load_history(&mut prompt,&history,&service)?;ui.note(&format!("Removed {count} history entries."));},
                "/integrations"=>{
                    ui.heading("Available tools");for gap in tools.prepare().await{ui.error(&gap);}
                    for tool in tools.definitions(){ui.text(tool["function"]["name"].as_str().unwrap_or("unknown"));}
                },
                _ if line.starts_with("/service ")=>{let next=line.trim_start_matches("/service ").trim();valid_service(next)?;service=tools.redact(next);active=None;load_history(&mut prompt,&history,&service)?;ui.note(&format!("Service: {service}. Started a new conversation."));},
                _ if line.starts_with("/resume ")=>{
                    let id=Uuid::parse_str(line.trim_start_matches("/resume ").trim()).map_err(|_|anyhow::anyhow!("Use /resume followed by a complete investigation ID from /sessions"))?;
                    let i=store.get(id)?.ok_or_else(||anyhow::anyhow!("Unknown investigation ID"))?;
                    service=i.service.clone();active=Some(id);load_history(&mut prompt,&history,&service)?;
                    ui.note(&format!("Resumed {id}"));print_incident(&i,false)?;
                },
                _ if line.starts_with('/')=>{ui.error("Unknown command. Type /help or press Tab after / to see available commands.");},
                _=>{
                    history.append(&tools,&service,line)?;prompt.remember(&tools.redact(line))?;
                    let result=terminal::progress(terminal::interactive(),ask(config,store,line,&service,active)).await?;
                    active=Some(result.id);print_incident(&result,false)?;
                }
            }
            Ok(())
        }.await;
        if let Err(error) = result {
            ui.error(&tools.redact(&error.to_string()));
        }
        println!();
    }
    Ui::detect().note("Session saved. Run ai-sre again whenever you need it.");
    Ok(())
}
