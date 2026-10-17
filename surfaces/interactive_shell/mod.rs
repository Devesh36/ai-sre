use crate::{
    config::Config,
    store::Store,
    surfaces::{cli::headless::ask, shared::output::print_incident},
    tools::Toolbox,
};
use anyhow::Result;
use std::io::{self, BufRead, Write};
use uuid::Uuid;
pub async fn run(config: &Config, store: &Store, service: &str) -> Result<()> {
    let history = crate::infrastructure::terminal::history::History::new(store.clone())?;
    let history_tools = Toolbox::new(config.clone())?;
    let mut active = None;
    println!("AI SRE • /new /sessions /resume ID /exit");
    loop {
        print!("sre> ");
        io::stdout().flush()?;
        let mut line = String::new();
        if io::stdin().lock().read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim();
        match line {
            "" => continue,
            "/exit" => break,
            "/help" => println!(
                "/help /status /cost /compact /integrations /sessions /resume ID /history /clear-history /trace /new /exit"
            ),
            "/status" | "/cost" => {
                if let Some(id) = active
                    && let Some(i) = store.get(id)?
                {
                    println!(
                        "{} {:?} {}",
                        i.id,
                        i.status,
                        serde_json::to_string(&i.usage)?
                    );
                } else {
                    println!("No active incident");
                }
            }
            "/history" => {
                for entry in history.list(service, 20)? {
                    println!("{} {}", entry.created_at, entry.text);
                }
            }
            "/clear-history" => {
                println!("Removed {} history entries", history.clear(service)?);
            }
            "/trace" => {
                if let Some(id) = active {
                    if let Some(i) = store.get(id)? {
                        println!("{}", serde_json::to_string_pretty(&i.trace)?);
                    }
                } else {
                    println!("No active incident");
                }
            }
            "/compact" => {
                if let Some(id) = active {
                    store.compact(id)?;
                    println!("Context compacted; original evidence retained");
                } else {
                    println!("No active incident");
                }
            }
            "/integrations" => {
                let tools = Toolbox::new(config.clone())?;
                for gap in tools.prepare().await {
                    eprintln!("{gap}");
                }
                for tool in tools.definitions() {
                    println!("{}", tool["function"]["name"]);
                }
            }
            "/new" => {
                active = None;
                println!("Started a new conversation");
            }
            "/sessions" => {
                for i in store.list()? {
                    println!("{} {:?} {}", i.id, i.status, i.title);
                }
            }
            _ if line.starts_with("/resume ") => {
                match Uuid::parse_str(line.trim_start_matches("/resume ")) {
                    Ok(id) if store.get(id)?.is_some() => {
                        active = Some(id);
                        println!("Resumed {id}");
                    }
                    _ => println!("Unknown incident ID"),
                }
            }
            _ if line.starts_with('/') => println!("Unknown command; use /help"),
            _ => {
                if let Err(error) = history.append(&history_tools, service, line) {
                    eprintln!("{error}");
                    continue;
                }
                match ask(config, store, line, service, active).await {
                    Ok(i) => {
                        active = Some(i.id);
                        print_incident(&i, false)?;
                    }
                    Err(e) => eprintln!("{e}"),
                }
            }
        }
    }
    Ok(())
}
