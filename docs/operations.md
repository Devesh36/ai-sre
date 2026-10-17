# Operational workflows

Run commands from the repository root. Replace `ai-sre` with `cargo run --locked --` during development. Global `--config` and `--workflow` flags select configuration and an optional workflow file.

## Model protocols and masking

```toml
mask_identifiers = ["checkout-production", "account-identifier"]

[model]
protocol = "anthropic" # open_ai (default), responses, anthropic, gemini
url = "https://api.anthropic.com/v1/messages"
name = "YOUR_AVAILABLE_MODEL"
key_env = "SRE_MODEL_KEY"
input_price_per_million = 0.0
output_price_per_million = 0.0
```

Provide the full protocol endpoint. Responses uses `/v1/responses`; Gemini uses the selected model’s `:generateContent` URL and `x-goog-api-key`. Compatible chat-completions services use `open_ai`. Models must support tools and return the requested structured report. Protocols are tested locally; hosted models are not individually certified.

Configured identifier strings are replaced before model requests and restored in tool arguments/reports. This is exact-string masking, not automatic anonymization. Secrets remain redacted. Prices are operator-supplied USD per million tokens; cost is absent when prices are zero. Token counts depend on provider reporting. Transient 429/502/503/504 responses receive up to two retries. The harness defaults to a 128 KiB encoded context limit, with automatic lossy projection; 512 KiB is the maximum configurable ceiling. See [harness policies and evaluation](harness.md) for attempt/token budgets, retry accounting, and context limits.

## Workflows and context

```sh
ai-sre --config examples/demo.toml --workflow examples/workflow.toml ask 'Investigate checkout' --service checkout-api
ai-sre memory add checkout-api deployment-window 'Deployments normally happen at 09:00 UTC.'
ai-sre memory list
ai-sre memory remove NOTE_UUID
ai-sre work add checkout-api 'Compare dependency saturation before and after release.'
ai-sre work complete WORK_UUID
ai-sre compact INCIDENT_UUID
ai-sre cost INCIDENT_UUID
ai-sre cancel INCIDENT_UUID
```

Workflow allowlists are enforced on tool execution. Operator notes are service-scoped and do not themselves trigger offline failure findings. Compaction preserves original history/evidence, while future prompts use the report summary, recent events, and current-run evidence. It is deterministic context selection, not a semantic summary of every old log.

The shell supports `/help`, `/status`, `/cost`, `/compact`, `/integrations`, `/sessions`, `/resume ID`, `/new`, and `/exit`. Ctrl+C requests cancellation and retains completed checkpoints. The API provides `/api/incidents/ID/cancel` and resumable `/api/incidents/ID/events` activity SSE. Send `Last-Event-ID` to resume. Model tokens are not streamed.

## MCP

```toml
[[mcp]]
name = "observability"
url = "https://tools.example.com/mcp"
token_env = "MCP_READ_TOKEN"
allowed_tools = ["query_metrics", "search_logs"]
```

The client negotiates `2025-03-26`, preserves session IDs, supports JSON/SSE results, and discovers up to eight pages. Only allowlisted tools declaring `readOnlyHint: true` become available. Discovery failures become investigation gaps. Annotations are advisory: trust the server, constrain its credentials, and review the allowlist. The client cannot enforce remote server behavior. stdio, OAuth, resources, and subscriptions are not implemented.

## Scheduled checks

```sh
ai-sre schedule add '0 */15 * * * *' 'Check checkout health' --service checkout-api --timezone Asia/Kolkata
ai-sre schedule list
ai-sre schedule remove SCHEDULE_UUID
ai-sre serve
```

Cron includes seconds first and optional year. Due-event creation and schedule advancement are one SQLite transaction. Duplicate workers cannot enqueue an occurrence twice. Scheduled and manual runs share the concurrency limit. After downtime, one catch-up occurrence is queued and the next future time calculated; every missed tick is not replayed. Forced termination requires recovery of a stuck investigation after confirming its worker stopped.

## Reviewed actions

Actions are operator-defined HTTP templates with fixed destinations/methods, typed bounded arguments, a precondition, a verification check, and rollback instructions. See `examples/lab.toml`.

```sh
ai-sre action propose INCIDENT_UUID reset_lab '{}' 'Observed synthetic failure; restore healthy lab state.'
ai-sre action list
ai-sre action approve PROPOSAL_UUID --actor operator-name
ai-sre action execute PROPOSAL_UUID
```

The model can propose but cannot approve or execute. Review the rendered request before approval. Proposals expire after an hour; approvals expire after 15 minutes. Configuration drift invalidates execution. The executor checks the precondition, claims the proposal atomically, dispatches once, and checks the postcondition. No mutation is automatically retried. Transport failures or mismatched postconditions produce an unknown outcome; inspect remote state before proposing a replacement. Verification is immediate, so asynchronous infrastructure changes may require operator follow-up.

Reviewer names are audit labels, not verified identities. Every workspace token holder can review and execute configured actions; this is not RBAC or separation-of-duties enforcement. Rollback is manual. A crash during dispatch leaves an executing record whose remote outcome must be investigated.

## Channels

Slack intake uses `[gateways.slack]` with `signing_secret_env`, `allowed_channels`, `allowed_users`, and `service`. Telegram uses `[gateways.telegram]` with `secret_env`, `allowed_chats`, `allowed_users`, and `service`. Routes are `/api/webhooks/slack` and `/api/webhooks/telegram`. Their provider authentication replaces workspace bearer authentication on those routes. Slack signatures expire after five minutes. Both deduplicate event IDs and reject unlisted senders. They create incidents for operator investigation; no automatic replies or actions are sent.

Explicit outbound delivery uses `ai-sre deliver INCIDENT_UUID DESTINATION_NAME`. A `[[deliveries]]` entry has `name`, `kind` (`slack`, `discord_webhook`, `telegram`, `json_webhook`), exactly one of `url`/`url_env`, optional `token_env`, and `destination` for Slack/Telegram. Token-bearing webhook URLs belong in an environment variable named by `url_env`. Provide the full send-message endpoint. Delivery sends a bounded summary and records an audit event after success. It has no automatic retry queue; a timeout may occur after acceptance, so inspect the channel before retrying.

## Synthetic incident lab

```sh
ai-sre lab --scenario memory
# In another terminal:
ai-sre --config examples/lab.toml ask 'Investigate checkout' --service checkout-api --json
ai-sre --config examples/lab.toml tool conn_lab_cluster_pods '{}'
ai-sre --config examples/lab.toml serve --bind 127.0.0.1:8082
```

The lab serves synthetic Prometheus, Loki, and Kubernetes responses on loopback port 9099. Scenarios are `healthy`, `memory`, `dependency`, `rate-limit`, and `disk`. Change state through `/lab/scenario` with JSON `{"scenario":"memory"}` (`rate_limit` in JSON). The `reset_lab` action demonstrates proposal review and verified recovery without production changes. This is a deterministic exercise, not a real cloud failure simulator or training benchmark.
