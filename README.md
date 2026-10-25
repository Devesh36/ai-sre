# AI SRE

An incident investigation workspace written in Rust. Ask a production question, collect read-only evidence, and keep the findings, citations, follow-ups, and activity history together.

![AI SRE terminal answering a checkout incident question with memory and dependency findings, evidence citations, and suggested next steps](docs/assets/terminal-demo.png)

*Real REPL output using the bundled synthetic evidence. This offline demo shows findings, citations, suggested checks, and what remains unknown—no model key required.*

**Status: early alpha.** The Rust engine, CLI, dashboard, operational workflows, four model protocols, MCP client, and reviewed-action executor are implemented. The native registry contains 59 bounded operations across 27 connector types, alongside Prometheus and Loki. Offline triage and the synthetic incident lab need no account or model key. Full behavioral coverage and production readiness remain incomplete; see the [coverage inventory](docs/coverage.md).

## Open the terminal assistant

```sh
cargo install --path . --locked
ai-sre repl
```

Start the interactive assistant with `ai-sre repl` or `ai-sre cli`. Running `ai-sre` alone also opens it when used in a terminal. The REPL includes editable prompts, Tab command completion, searchable history, service/session context, formatted reports, and cancellable progress. Use `ai-sre repl --service checkout-api` to select a service immediately; type `/help` for commands or `/exit` to quit. The `shell` and `chat` aliases remain available. See [terminal usage](docs/terminal.md) for configuration, keyboard shortcuts, and scripting.

Try the session pictured above from this checkout:

```sh
ai-sre --config examples/demo.toml repl --service checkout-api
# Ask: Why is checkout failing after the latest deploy?
```

## Live example: investigate with Codex

![AI SRE live-example terminal showing a resumed investigation, follow-up evidence citations, eight retained evidence items, and the cost command](docs/assets/live-example-terminal.png)

*The actual saved demo session reopened in the amber REPL. Synthetic evidence, four local tool calls, and no model calls.*

Ask Codex: **“Use AI SRE to investigate checkout failures. Show the evidence, recheck it, and explain what we should verify before a rollback.”** Codex can run the CLI, inspect the saved results, and help interpret them while AI SRE keeps the investigation and evidence together.

Run the same example yourself from this checkout:

```sh
cargo build --locked
bash scripts/live-example.sh
```

This runs the real REPL with copied synthetic fixtures, asks an initial question and a follow-up, then prints the evidence, usage, trace, and saved session. Each run creates its own database and saves a transcript and JSON export. No model key or cloud connection is needed.

An abridged result from the recorded run:

```text
2 evidence-backed symptom hypothesis(es); root cause remains unverified.

Memory pressure detected           Evidence: E1 · E5
Dependency connectivity failure     Evidence: E2 · E6

8 evidence items · 4 tool calls · 0 model calls
```

These are two collections of the same demo inputs, not eight independent confirmations. My assessment of the saved evidence: the OOM kill and refused dependency connection warrant checking memory growth and inventory readiness; the nearby deployment alone does not prove causation. Offline triage repeats rule-based checks; Codex supplies the interpretation in this example.

Read the [complete walkthrough with Codex](docs/live-example.md), [captured terminal output](docs/examples/live-session.txt), and [actual JSON export](docs/examples/live-incidents.json).

## Run it

Install Rust using the official toolchain installer, then run from this checkout:

```sh
cargo build --locked
cargo run --locked -- demo
cargo run --locked -- --config examples/demo.toml serve
```

The server binds to loopback port 8080. The dashboard shows the demo investigation and lets you create investigations, inspect evidence, ask follow-ups, resolve incidents, and export JSON. Demo evidence is synthetic and explicitly labeled. The binary embeds the dashboard; Node and Python are not runtime dependencies.

For your own environment:

```sh
cargo run --locked -- init
# Edit sre.toml to configure your evidence sources and optional model.
cargo run --locked -- integrations --verify --service checkout-api
cargo run --locked -- ask 'Why is checkout failing?' --service checkout-api
cargo run --locked -- shell --service checkout-api
```

Configuration paths resolve relative to the TOML file. The default database is `.ai-sre/incidents.db`. `init` preserves existing files. With no configured evidence sources, the investigator reports insufficient evidence instead of inventing a diagnosis.

## What works today

| Capability | Behavior |
| --- | --- |
| Headless investigations | Question argument or stdin, structured JSON output, nonzero exit on failed runs |
| Interactive terminal | Default interactive launch, editable prompts, Tab completion, searchable history, sessions/resume, wrapped reports, traces, and Ctrl+C cancellation |
| Model-assisted reasoning | Chat Completions, Responses, Anthropic, and Gemini protocol adapters with bounded tool loops |
| Local evidence | Service-filtered JSON bundles; Markdown runbooks from a configured directory |
| Observability | Read-only PromQL queries and one-hour Loki log queries |
| Alert intake | Alertmanager deduplication; authenticated Slack/Telegram intake with sender allowlists |
| Workspace | Dashboard, search, status filters, cited evidence, timeline, follow-up, export, manual resolution |
| Persistence | SQLite WAL, atomic claims, per-tool checkpoints, cancellation, resumable activity SSE |
| Evidence changes | Compare the last two runs: added, not repeated, and identical retained observations, with clickable citations |
| Investigation review | Flag repeated support, missing or historical-only citations, and aged collection timestamps without inflating confidence |
| Operator outcomes | Explicit supported/refuted/inconclusive assessments with actor labels, run-specific evidence fingerprints, immutable corrections, and historical isolation |
| Incident recall | Find earlier same-service cases, explain exact payload and symptom matches, and inspect the supporting historical run |
| On-call handoff | Export Markdown with findings, evidence references, run history, unknowns, and checks for the next operator |
| Execution controls | Per-run budgets, bounded retries/repair, duplicate suppression, source circuits, context projection, durable traces |
| Privacy | No telemetry; secret redaction and reversible configured identifier masking |
| Evaluation | Six offline scenarios, HTTP incident laboratory, and verified synthetic recovery |
| Native connectors | Explicit read operations for Kubernetes, source control, observability, incident tools, and more; [exact catalog](docs/integration-catalog.md) |
| MCP | Session-aware Streamable HTTP and local stdio, bounded messages, explicit tool allowlists |
| Workflow releases | Content-addressed publishing, explicit activation, rollback, and authenticated downloads with ETag caching |
| Operations | Service memory, work items, cron/timezones, concrete proposals, approvals, and verified execution |
| Delivery and fleet | Explicit report delivery; local coding-agent process discovery |

The offline engine recognizes explicit memory, dependency-connectivity, throttling, and disk-capacity symptoms. These are low-confidence hypotheses, not verified root causes. Runbooks, operator notes, and action proposals are excluded from symptom matching. A model can correlate additional evidence, but valid citations establish only that a referenced item exists—not that a claim is correct.

## Record what an operator verified

Use `ai-sre outcome record` to record an explicit claim, verdict, rationale, actor
label, and current-run evidence references. Inspect the history with
`ai-sre outcome list INCIDENT_ID`, `/outcomes`, or the dashboard's **Outcomes** tab.
Corrections retain the original record. Historical assessments appear beside
recalled cases without changing the current report or its confidence.

These are human-recorded assessments, not independent verification. See
[operator outcomes](docs/operator-outcomes.md) for commands, provenance, retry
behavior, and the distinction between a recorded assessment and a proven fix.

## Reuse past investigation evidence

Run `ai-sre recall INCIDENT_ID`, use `/recall` in the REPL, or open **Recall** in
the dashboard. Each result explains its current and past evidence references,
matching run, and symptom differences. Duplicate observations do not inflate
ranking, and an old resolution is not treated as a verified fix.

Try two real synthetic investigations with `bash scripts/recall-example.sh`
after building. See [incident recall](docs/incident-recall.md) for the matching
rules, bounded search, and limitations.

## Review evidence and hand off an incident

In an active REPL session, use `/review`, `/changes`, and `/handoff`. The dashboard
offers a **Review** tab and a **Handoff** download. The same features are available
from the CLI:

```sh
ai-sre --config examples/demo.toml review INCIDENT_UUID
ai-sre --config examples/demo.toml changes INCIDENT_UUID --json
ai-sre --config examples/demo.toml handoff INCIDENT_UUID > handoff.md
```

Use an ID from `sessions` and the configuration that owns the incident. These
checks run without a model. Repeated evidence is not independent corroboration,
and an observation missing from a later run does not prove recovery. See
[evidence review, comparisons, and handoffs](docs/investigation-review.md).

## Configure a model

Add a model block to `sre.toml`:

```toml
[model]
url = "http://127.0.0.1:11434/v1/chat/completions"
name = "your-tool-capable-model"
# key_env = "SRE_MODEL_KEY"
```

For other protocols, set `protocol` as described in [operational workflows](docs/operations.md). For a hosted compatible provider, use its HTTPS endpoint and set the named credential in the process environment. Never put credentials in the TOML file. The provider must support function tools and emit the structured JSON report described by the agent prompt. Hosted-provider compatibility has not been certified; the protocol is exercised against a local mock server in tests. No live model credentials are bundled.

Configuring a remote model authorizes sending the incident question, history, and redacted evidence to that provider. Redaction is best effort: inspect source data and your provider’s retention policy before using sensitive production information.

## Resume and automate

```sh
cargo run --locked -- sessions
cargo run --locked -- ask 'Check the dependency hypothesis' --incident INCIDENT_UUID --json
cargo run --locked -- show INCIDENT_UUID
cargo run --locked -- resolve INCIDENT_UUID
printf '%s' 'Why are requests timing out?' | cargo run --locked -- ask - --service checkout-api --json
```

The shell and CLI use the same library as the API. The Rust modules are public for embedding; the API is not yet stable.

## Validate

See the [verification record](docs/verification.md) for the checks performed and their limits.

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo run --locked -- eval
cargo run --locked -- eval --suite harness --repeat 3 --output .ai-sre/harness.json
```

Tests cover incident lifecycle, persistence, authentication, all four model tool round-trips, masking, connector boundaries, MCP, cancellation, schedule deduplication, signed webhooks, action approval races, expired approvals, and synthetic failure/recovery workflows. The evaluation command measures offline symptom detection only; it is not an infrastructure benchmark or proof of production diagnosis quality.

Browser checks run in CI and can be run locally after building the debug binary:

```sh
cargo build --locked
npm ci
npx playwright install --with-deps chromium
CHROMIUM_EXECUTABLE_PATH="$(node -p "require('playwright').chromium.executablePath()")" npm run test:browser
```

The runner creates and removes an isolated synthetic database, starts its own server on port 18080 (override with `AI_SRE_BROWSER_PORT`), and checks investigation creation, follow-ups, citations, reviews, handoff downloads, work, memory, schedules, incident recall, operator outcome history, and mobile layout. Deterministic delayed responses also check stale-request protection, incident selection, retained drafts, cursor position, and expanded evidence. Screenshots go into the ignored `.ai-sre` directory. Node, Playwright, and Chromium are development dependencies only; the product runs as a Rust binary.

Individual `scripts/*smoke.cjs` checks accept `SRE_TEST_URL` and `CHROMIUM_EXECUTABLE_PATH`. The operations check additionally exercises explicit action review and recovery against a configured local lab when `SRE_ACTION_PROPOSAL_ID` is set. See [operational workflows](docs/operations.md).

See [agent harness and evaluation](docs/harness.md), [API and integrations](docs/api.md), [architecture](docs/architecture.md), [deployment](docs/deployment.md), and [product roadmap](docs/roadmap.md).

The implementation is organized into eight functional layers with separate modules for native connectors. See [repository layout and module inventory](docs/repository-layout.md) and [workflow registry](docs/workflow-registry.md).

Selected shared tool modules have direct Rust adaptations with source-comparison tests. See the [file-level port status](docs/tool-layer-port.md) and [third-party notices](THIRD_PARTY_NOTICES.md).
