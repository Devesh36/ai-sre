use ai_sre::{api, config::Config, domain::Status, store::Store, tools::Toolbox};
use ai_sre::{
    bootstrap::{files::write_if_missing, lifecycle::shutdown},
    surfaces::{cli::headless::ask, shared::output::print_incident},
};
use anyhow::{Result, ensure};
use clap::{CommandFactory, Parser, Subcommand};
use std::{
    io::{self, IsTerminal, Write},
    net::SocketAddr,
    path::PathBuf,
};
use uuid::Uuid;

#[derive(Parser)]
#[command(version, about = "AI SRE — evidence-first incident investigations")]
struct Cli {
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[arg(long, global = true)]
    workflow: Option<PathBuf>,
    #[arg(long, global = true, conflicts_with = "workflow")]
    skill: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run synthetic Prometheus, Loki, and Kubernetes endpoints for local incident exercises.
    Lab {
        #[arg(long, default_value = "127.0.0.1:9099")]
        bind: SocketAddr,
        #[arg(long, value_enum, default_value = "memory")]
        scenario: ai_sre::lab::Scenario,
    },
    #[command(flatten)]
    Operations(ai_sre::cli_ops::Commands),
    /// Investigate a question; use '-' to read the question from stdin.
    Ask {
        question: String,
        #[arg(long, default_value = "unknown")]
        service: String,
        #[arg(long)]
        incident: Option<Uuid>,
        #[arg(long)]
        json: bool,
    },
    /// Start the interactive REPL. Use /help for commands and /exit to quit.
    #[command(name = "repl", visible_aliases = ["cli", "shell", "chat"])]
    Shell {
        #[arg(long, default_value = "unknown")]
        service: String,
    },
    /// Serve the local dashboard and authenticated API.
    Serve {
        #[arg(long, default_value = "127.0.0.1:8080")]
        bind: SocketAddr,
    },
    /// List the 100 most recently updated investigations.
    Sessions,
    /// Export a complete investigation, including evidence and audit events.
    Show { id: Uuid },
    /// Mark an incident resolved after operator verification.
    Resolve { id: Uuid },
    /// Recover a stuck incident only after its worker has stopped.
    Recover {
        id: Uuid,
        #[arg(long)]
        worker_stopped: bool,
    },
    /// List configured tools; --verify performs read-only requests.
    Integrations {
        #[arg(long)]
        verify: bool,
        #[arg(long, default_value = "unknown")]
        service: String,
    },
    /// Run the bundled, credential-free demonstration.
    Demo,
    /// Write a starter configuration without overwriting an existing file.
    Init {
        #[arg(default_value = "sre.toml")]
        path: PathBuf,
    },
    /// Evaluate offline symptom rules or the production agent harness against scripted faults.
    Eval {
        #[arg(long, value_enum, default_value = "offline")]
        suite: EvalSuite,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, default_value_t = 1)]
        repeat: usize,
        #[arg(long)]
        baseline: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum EvalSuite {
    Offline,
    Harness,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let command = match cli.command {
        Some(command) => command,
        None if io::stdin().is_terminal() => Command::Shell {
            service: "unknown".into(),
        },
        None => {
            Cli::command().print_help()?;
            println!(
                "\n\nRun ai-sre in a terminal to chat, or use ai-sre ask - --json for piped input."
            );
            return Ok(());
        }
    };
    if let Command::Init { path } = &command {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/sre.toml"
        )))?;
        println!(
            "Created {}. Paths resolve relative to this file.",
            path.display()
        );
        return Ok(());
    }
    if let Command::Eval {
        suite,
        json,
        output,
        repeat,
        baseline,
    } = &command
    {
        match suite {
            EvalSuite::Offline => {
                ensure!(
                    !json && output.is_none() && baseline.is_none() && *repeat == 1,
                    "artifact, baseline, JSON, and repeat options require --suite harness"
                );
                return ai_sre::infrastructure::evaluation::offline::evaluate();
            }
            EvalSuite::Harness => {
                // Read the baseline before replacing a possibly identical output path.
                let baseline = baseline
                    .as_ref()
                    .map(|p| -> Result<ai_sre::evaluation::Evaluation> {
                        use std::io::Read;
                        let mut data = String::new();
                        std::fs::File::open(p)?
                            .take(8 * 1024 * 1024 + 1)
                            .read_to_string(&mut data)?;
                        ensure!(data.len() <= 8 * 1024 * 1024, "baseline exceeds 8 MiB");
                        Ok(serde_json::from_str(&data)?)
                    })
                    .transpose()?;
                let mut report = ai_sre::evaluation::harness(*repeat).await?;
                if let Some(baseline) = baseline {
                    report.baseline_regressions = report.compare(&baseline)?;
                    report.passed &= report.baseline_regressions.is_empty();
                }
                if let Some(path) = output {
                    report.write(path)?;
                }
                if *json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    for case in &report.cases {
                        println!(
                            "{} {} (run {}, {} ms)",
                            if case.passed { "PASS" } else { "FAIL" },
                            case.name,
                            case.repetition,
                            case.elapsed_ms
                        );
                        for check in case.checks.iter().filter(|c| !c.passed) {
                            println!("  {}: {}", check.name, check.detail);
                        }
                    }
                    for regression in &report.baseline_regressions {
                        println!("REGRESSION {regression}");
                    }
                    println!(
                        "{}/{} harness scenarios passed; p50={} ms, p95={} ms (includes injected delays)",
                        report.cases.iter().filter(|c| c.passed).count(),
                        report.cases.len(),
                        report.latency_p50_ms,
                        report.latency_p95_ms
                    );
                }
                ensure!(report.passed, "harness regression gate failed");
                return Ok(());
            }
        }
    }
    if let Command::Lab { bind, scenario } = command {
        ensure!(
            bind.ip().is_loopback(),
            "the synthetic lab must bind to loopback"
        );
        let listener = tokio::net::TcpListener::bind(bind).await?;
        println!("Synthetic incident lab listening on {bind}; scenario {scenario:?}");
        axum::serve(
            listener,
            ai_sre::lab::router(ai_sre::lab::Lab::new(scenario)),
        )
        .with_graceful_shutdown(shutdown())
        .await?;
        return Ok(());
    }
    let mut config = match cli.config {
        Some(path) => Config::load(&path)?,
        None if std::path::Path::new("sre.toml").exists() => {
            Config::load(std::path::Path::new("sre.toml"))?
        }
        None => Config::default(),
    };
    if matches!(command, Command::Demo) {
        let dir = PathBuf::from(".ai-sre/demo");
        std::fs::create_dir_all(dir.join("runbooks"))?;
        write_if_missing(
            &dir.join("evidence.json"),
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/examples/evidence.json"
            )),
        )?;
        write_if_missing(
            &dir.join("runbooks/checkout.md"),
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/examples/runbooks/checkout.md"
            )),
        )?;
        config = Config {
            evidence_file: Some(dir.join("evidence.json")),
            runbooks: Some(dir.join("runbooks")),
            ..Config::default()
        };
    }
    if let Some(path) = cli.workflow {
        config.workflow = Some(toml::from_str(&std::fs::read_to_string(path)?)?);
    }
    config.validate()?;
    let store = Store::open(&config.database)?;
    if let Some(name) = cli.skill {
        config.workflow = Some(
            ai_sre::infrastructure::skills_registry::Registry::new(store.clone())?
                .active(&name)?
                .workflow,
        );
        config.validate()?;
    }
    match command {
        Command::Operations(command) => command.run(&config, &store).await?,
        Command::Ask {
            question,
            service,
            incident,
            json,
        } => {
            let question = if question == "-" {
                use std::io::Read;
                let mut input = String::new();
                io::stdin().take(4001).read_to_string(&mut input)?;
                input
            } else {
                question
            };
            let result = ai_sre::surfaces::shared::terminal::progress(
                !json && ai_sre::surfaces::shared::terminal::interactive(),
                ask(&config, &store, &question, &service, incident),
            )
            .await?;
            print_incident(&result, json)?;
            ensure!(
                result.status != Status::Failed,
                "investigation failed; see saved event history"
            );
        }
        Command::Demo => {
            print_incident(
                &ask(
                    &config,
                    &store,
                    "Why is checkout-api failing after its latest release?",
                    "checkout-api",
                    None,
                )
                .await?,
                false,
            )?;
        }
        Command::Serve { bind } => {
            let token = api::token_from_environment()?;
            ensure!(
                bind.ip().is_loopback() || token.is_some(),
                "set SRE_API_TOKEN before binding a non-loopback interface"
            );
            let listener = tokio::net::TcpListener::bind(bind).await?;
            println!(
                "AI SRE listening on {bind}; {}",
                if config.model.is_some() {
                    "model-assisted investigations"
                } else {
                    "offline triage"
                }
            );
            let app = api::App::new(config.clone(), store, token);
            let capacity = app.capacity.clone();
            let (stop, receiver) = tokio::sync::watch::channel(false);
            let scheduler = tokio::spawn(ai_sre::operations::scheduler(app.clone(), receiver));
            axum::serve(listener, api::router(app))
                .with_graceful_shutdown(shutdown())
                .await?;
            let _ = stop.send(true);
            scheduler.await?;
            // Background investigations outlive their HTTP requests. Drain them before exiting.
            let _drain = tokio::time::timeout(
                std::time::Duration::from_secs(310),
                capacity.acquire_many(config.max_concurrent as u32),
            )
            .await?;
        }
        Command::Sessions => {
            println!("{}", serde_json::to_string_pretty(&store.list()?)?);
        }
        Command::Show { id } => {
            print_incident(
                &store
                    .get(id)?
                    .ok_or_else(|| anyhow::anyhow!("incident not found"))?,
                true,
            )?;
        }
        Command::Resolve { id } => {
            let incident = store.mutate(id, |i| {
                ensure!(
                    i.status != Status::Investigating,
                    "wait for investigation to finish"
                );
                i.status = Status::Resolved;
                i.event("resolved", "Operator marked incident resolved");
                Ok(())
            })?;
            print_incident(&incident, false)?;
        }
        Command::Recover { id, worker_stopped } => {
            ensure!(
                worker_stopped,
                "confirm the worker process has stopped with --worker-stopped"
            );
            store.mutate(id, |i| {
                ensure!(i.status == Status::Investigating, "incident is not running");
                i.status = Status::Failed;
                i.event(
                    "recovered",
                    "Operator confirmed worker stopped; retry is available",
                );
                Ok(())
            })?;
            println!("Recovered {id}. Resume using ask --incident {id}.");
        }
        Command::Integrations { verify, service } => {
            let tools = Toolbox::new(config)?;
            for gap in tools.prepare().await {
                eprintln!("{gap}");
            }
            let mut failed = false;
            for tool in tools.definitions() {
                let name = tool["function"]["name"].as_str().expect("tool name");
                if verify {
                    if tool["function"]["parameters"]["required"]
                        .as_array()
                        .is_some_and(|r| !r.is_empty())
                        && !matches!(name, "prometheus_query" | "loki_query")
                    {
                        println!("{name}: requires arguments; verify with the tool command");
                        continue;
                    }
                    let args = match name {
                        "prometheus_query" => serde_json::json!({"query":"up"}),
                        "loki_query" => {
                            serde_json::json!({"query":format!("{{service_name={}}}", serde_json::to_string(&service)?)})
                        }
                        _ => serde_json::json!({}),
                    };
                    match tools.call(name, args, &service).await {
                        Ok(items) => println!("{name}: verified ({} evidence items)", items.len()),
                        Err(e) => {
                            println!("{name}: failed — {}", tools.redact(&e.to_string()));
                            failed = true;
                        }
                    }
                } else {
                    println!("{name}: configured");
                }
            }
            if tools.definitions().is_empty() {
                println!("No evidence tools configured. Use demo or init.");
            }
            ensure!(!failed, "one or more integrations failed verification");
        }
        Command::Shell { service } => {
            ai_sre::surfaces::interactive_shell::run(&config, &store, &service).await?
        }
        Command::Init { .. } | Command::Eval { .. } | Command::Lab { .. } => unreachable!(),
    }
    Ok(())
}
