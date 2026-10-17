use ai_sre::{
    agent, api,
    config::Config,
    domain::{Incident, Status},
    store::Store,
    tools::Toolbox,
};
use anyhow::{Result, ensure};
use clap::{Parser, Subcommand};
use std::{
    io::{self, BufRead, Write},
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
    #[command(subcommand)]
    command: Command,
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
    /// Start a resumable terminal conversation. Commands: /new, /sessions, /resume ID, /exit.
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
    if let Command::Init { path } = &cli.command {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(include_bytes!("../examples/sre.toml"))?;
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
    } = &cli.command
    {
        match suite {
            EvalSuite::Offline => {
                ensure!(
                    !json && output.is_none() && baseline.is_none() && *repeat == 1,
                    "artifact, baseline, JSON, and repeat options require --suite harness"
                );
                return evaluate();
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
    if let Command::Lab { bind, scenario } = cli.command {
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
    if matches!(cli.command, Command::Demo) {
        let dir = PathBuf::from(".ai-sre/demo");
        std::fs::create_dir_all(dir.join("runbooks"))?;
        write_if_missing(
            &dir.join("evidence.json"),
            include_bytes!("../examples/evidence.json"),
        )?;
        write_if_missing(
            &dir.join("runbooks/checkout.md"),
            include_bytes!("../examples/runbooks/checkout.md"),
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
    match cli.command {
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
            let result = ask(&config, &store, &question, &service, incident).await?;
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
                        "/help /status /cost /compact /integrations /sessions /resume ID /new /exit"
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
                    _ => match ask(&config, &store, line, &service, active).await {
                        Ok(i) => {
                            active = Some(i.id);
                            print_incident(&i, false)?;
                        }
                        Err(e) => eprintln!("{e}"),
                    },
                }
            }
        }
        Command::Init { .. } | Command::Eval { .. } | Command::Lab { .. } => unreachable!(),
    }
    Ok(())
}

async fn ask(
    config: &Config,
    store: &Store,
    question: &str,
    service: &str,
    id: Option<Uuid>,
) -> Result<Incident> {
    ensure!(
        !question.trim().is_empty() && question.len() <= 4000,
        "question must contain 1–4000 bytes"
    );
    let tools = Toolbox::new(config.clone())?;
    let question = tools.redact(question);
    let id = match id {
        Some(id) => id,
        None => {
            store
                .insert(
                    &Incident::new(question.clone(), tools.redact(service))?,
                    None,
                )?
                .0
                .id
        }
    };
    let incident = store.claim(id, Some(&question))?;
    let run = agent::investigate(config, store, incident);
    tokio::pin!(run);
    tokio::select! {result=&mut run=>result,_=tokio::signal::ctrl_c()=>{store.cancel(id)?;run.await}}
}

fn print_incident(i: &Incident, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(i)?);
        return Ok(());
    }
    println!("\n{}\n{} · {:?}\n", i.title, i.id, i.status);
    if let Some(report) = &i.report {
        println!("{}", report.summary);
        for f in &report.findings {
            println!(
                "\n• {} [{:?}] ({})\n  {}",
                f.title,
                f.confidence,
                f.evidence_ids.join(", "),
                f.explanation
            );
        }
        for step in &report.next_steps {
            println!("→ {step}");
        }
        for gap in &report.gaps {
            println!("Gap: {gap}");
        }
    }
    if i.status == Status::Failed
        && let Some(e) = i.events.last()
    {
        println!("Failure: {}", e.message);
    }
    println!(
        "\n{} evidence items · {} model calls · {} input / {} output tokens",
        i.evidence.len(),
        i.usage.model_calls,
        i.usage.input_tokens,
        i.usage.output_tokens
    );
    Ok(())
}

fn write_if_missing(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut f) => f.write_all(bytes)?,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

fn evaluate() -> Result<()> {
    #[derive(serde::Deserialize)]
    struct Scenario {
        name: String,
        signal: String,
        expected: Option<String>,
    }
    let scenarios: Vec<Scenario> =
        serde_json::from_str(include_str!("../examples/scenarios.json"))?;
    let total = scenarios.len();
    let mut passed = 0;
    for s in scenarios {
        let evidence = ai_sre::domain::Evidence {
            id: "E1".into(),
            source: "evaluation".into(),
            observed_at: chrono::Utc::now(),
            summary: s.signal,
            data: serde_json::json!({}),
        };
        let report = agent::offline_report(&[evidence]);
        let ok = match s.expected {
            Some(title) => report.findings.iter().any(|f| f.title == title),
            None => report.findings.is_empty(),
        };
        println!("{} {}", if ok { "PASS" } else { "FAIL" }, s.name);
        passed += usize::from(ok);
    }
    println!("{passed}/{total} scenarios passed (offline symptom detection only)");
    ensure!(passed == total, "evaluation failed");
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        if let Ok(mut term) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
