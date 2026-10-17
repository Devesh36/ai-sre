# API and integrations

Start with `ai-sre --config sre.toml serve`. Except for provider-authenticated Slack/Telegram webhook routes, all `/api/` routes require `Authorization: Bearer …` when `SRE_API_TOKEN` is configured. In loopback development without a token, use a loopback Host header. Non-browser clients do not need an Origin header; cross-origin browser requests are rejected.

| Method | Path | Result |
| --- | --- | --- |
| GET | `/healthz` | Version and liveness; does not probe external dependencies |
| GET | `/api/incidents` | Latest 100 full incident documents |
| GET | `/api/overview?limit=50&offset=0` | Compact incident summaries; limit 1–100 |
| POST | `/api/incidents` | Create with `{"title":"…","service":"…"}`; returns 201 |
| GET | `/api/incidents/{id}` | Full evidence, report, usage, audit history, and structured run traces |
| POST | `/api/incidents/{id}/investigate` | Body `{}` or `{"question":"…"}`; returns 202 |
| POST | `/api/incidents/{id}/resolve` | Operator-confirmed resolution |
| POST | `/api/incidents/{id}/cancel` | Request cancellation; retain checkpoints |
| POST | `/api/incidents/{id}/compact` | Compact future prompts; preserve original data |
| GET | `/api/incidents/{id}/events` | Activity SSE; accepts `Last-Event-ID` |
| POST | `/api/incidents/{id}/deliver/{destination}` | Explicitly deliver a report |
| GET | `/api/integrations` | Configuration presence, mode, and limits; no credential values |
| POST | `/api/webhooks/alertmanager` | Deduplicated firing-alert intake |

An in-flight or resolved investigation returns 409 when a new run is requested. Capacity exhaustion returns 429; clients may retry later. Unknown incident IDs return 404. The dashboard polls every three seconds. Export a single incident by saving the detail response.

## Alertmanager

Configure an Alertmanager webhook receiver pointed at `/api/webhooks/alertmanager`, with bearer authorization configured through its `http_config.authorization` mechanism. Send the standard `alerts` array. Each alert needs `status`, `fingerprint`, `startsAt`, and `labels`; `annotations` is optional. At most 50 alerts are accepted per request.

The title comes from `annotations.summary`, then `labels.alertname`. The service comes from `labels.service`, then `labels.job`. A fingerprint and normalized `startsAt` identify one occurrence; retries return the existing incident. A later start time creates a new incident. Resolved notifications are ignored, since alert clearance does not prove incident recovery. Ingestion does not automatically start a model run: start the investigation via the dashboard or API. Payloads are validated before writes, but multi-alert writes are not an atomic batch in the event of a database error; retries are idempotent.

## Prometheus

```toml
[prometheus]
url = "https://prometheus.example.com"
token_env = "PROMETHEUS_TOKEN"
```

The base URL may include a routing prefix. Initial collection queries `up{job="SERVICE"}`. A configured model can issue additional read-only instant PromQL queries. Metric names and labels vary by deployment; provide that context in your question/runbooks. Use a read-only credential and upstream query limits. Large responses fail rather than being silently sampled.

## Loki

```toml
[loki]
url = "https://loki.example.com"
token_env = "LOKI_TOKEN"
```

Initial collection queries `{service_name="SERVICE"}` over the last hour with a 50-entry limit. A model can supply a different LogQL selector for your labels. Custom tenant headers are not yet supported; use an authenticated gateway when your installation requires them.

## Local context

Set `evidence_file` to a JSON array of objects with `service`, `source`, `summary`, and `data`. Only matching services are collected, up to 20 records per tool call. This provides an input path for deployment events, traces, and other exported context without claiming a native integration.

Set `runbooks` to a directory of Markdown files. Up to eight files, sorted by name, are included; nested directories are not traversed and symlinks are rejected. Keep each file below 16 KiB. All included runbooks are shared context for this workspace; do not mix unrelated tenants. See the checked-in examples.

## Model providers

The model transport uses the chat completions `messages`, `tools`, `tool_calls`, and `usage` fields. Responses must include a final JSON report. Local models and hosted services exposing this protocol can be configured; individual provider behavior must be verified. Responses, Anthropic, Gemini, and MCP adapters are also implemented; see [operations](operations.md). Token usage is recorded when supplied by the provider; zero can also mean usage was omitted. Estimated cost is available when nonzero model prices are configured.

## Operations routes

All these routes use workspace bearer authorization when configured.

| Route | Methods and request |
| --- | --- |
| `/api/catalog` | GET: native operation registry |
| `/api/work` | GET list; POST `{service,text}` |
| `/api/work/{id}/complete` | POST: mark complete |
| `/api/memory` | GET list; POST `{service,key,text}` |
| `/api/memory/{id}` | DELETE: remove note |
| `/api/schedules` | GET list; POST `{expression,timezone,question,service}` |
| `/api/schedules/{id}` | DELETE: stop future occurrences |
| `/api/actions` | GET list; POST `{incident_id,template,arguments,rationale}` |
| `/api/actions/{id}/review` | POST `{actor,approve}` |
| `/api/actions/{id}/execute` | POST: dispatch an approved action after checks |
| `/api/fleet` | GET: local agent process names/PIDs |

Work, memory, schedules, and actions list the latest/next 100 records. Action outcomes must be read from the returned `status`; HTTP success does not mean verification succeeded. A reviewer name is an audit label, not verified identity. See [operations](operations.md) for expiry, mutation uncertainty, and rollback semantics.
