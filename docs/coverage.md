# Capability coverage

This inventory separates implemented behavior from remaining gaps. The full tracked-tree audit covers 4,385 files in 755 directories: 2,208 source modules, 1,538 test modules, and 32,284 declarations. The earlier six-subsystem audit was narrower. These counts establish migration scope; they do not mean every behavior has been rebuilt. Selected shared tool modules now contain direct Rust adaptations under Apache-2.0; provenance and retained notices are in [third-party notices](../THIRD_PARTY_NOTICES.md). The [module inventory](module-inventory.json) records every audited path and declaration, with related implementation pointers and pending targets. See [repository layout](repository-layout.md) for how to use it.

| Area | Implemented in Rust | Remaining |
| --- | --- | --- |
| Agent loop | Bounded tools, recursive argument checks, provider output budgets, cited findings, explicit gaps, accounted retries, token/cost accounting, source circuits, duplicate suppression, report repair, bounded context projection, durable traces | Delegation, multimodal input, token streaming, richer reasoning policies |
| Model transports | Chat Completions, Responses, Anthropic Messages, Gemini generateContent | Bedrock Converse, Vertex identity, CLI model transports, hosted credits; live provider certification |
| Privacy | Credential redaction; reversible configured identifier masking | Automatic entity detection, comprehensive secret recognition, encrypted storage |
| Session state | SQLite WAL, atomic claims, tool checkpoints, cancellation, run ownership IDs, resumable SSE activity, follow-ups, compaction | Exact mid-tool continuation after process death; remote synchronization |
| Terminal | Headless and interactive questions, stdin/JSON, sessions/resume/status/cost/compact, tool listing, doctor, Ctrl+C cancellation, redacted persistent history and traces | Rich terminal UI, full slash-command coverage, attachments |
| Web workspace | Investigations, evidence, activity, search, cancellation, compaction, export, work/memory/schedule/action screens | Multi-user collaboration, SSO/RBAC, model streaming, cloud administration |
| Native integrations | 59 operations across 27 connector types; Prometheus and Loki built-ins | Complete vendor API coverage; native SDK/driver gaps listed below |
| MCP | Streamable HTTP, sessions, paginated discovery, JSON/SSE results, allowlists and read-only declarations | stdio, OAuth, resources/prompts, subscriptions, server-initiated requests |
| Workflows | Local TOML instructions, enforced tool scope, immutable releases, activation/rollback, authenticated digest-validated downloads with ETag caching | Continuous synchronization, dynamic skill routing, artifact signing, multi-file bundles |
| Memory/work | Service-scoped notes, work retrieval/completion | Semantic retrieval, knowledge graphs, hierarchical planning, repository preferences |
| Scheduling | Cron/timezones, atomic enqueue, persisted queue, bounded workers | Distributed leases, replay of every missed tick, notification schedules |
| Actions | Fixed destinations, typed arguments, concrete proposals, explicit review, expiry, preconditions, single dispatch, postconditions, rollback guidance | Verified identity/RBAC, automatic rollback, cloud-specific playbooks, reconciliation service |
| Intake | Alertmanager deduplication; authenticated Slack/Telegram webhooks with sender/channel allowlists | Interactive replies, incident-provider lifecycle synchronization |
| Delivery | Explicit Slack, Discord webhook, Telegram, JSON webhook summaries | Durable delivery/retry queues, message updates, email/additional channels |
| Fleet | Process-name/PID discovery without command-line/conversation inspection | Vendor-specific session/token readers, remote fleets, control/registration |
| Evaluation | Six offline scenarios; 28 runtime fault scenarios with JSON artifacts and regression gates; HTTP incident lab; reviewed synthetic recovery | Cloud failure injection, production datasets, training/RL environment, benchmarks |
| Distribution | Pinned builds, embedded assets, Linux cloud setup, CI, release executable | Signed installers, update/uninstall, Windows/macOS validation, cloud deployment |
| Hosted platform | None | Accounts, organizations, managed gateways, billing/credits, shared identity/credential services |

## Native integration gaps

No native clients currently exist for AWS services (EC2, EKS, ELB, S3, Lambda, CloudWatch, CloudTrail, SQS, RDS, Bedrock); Azure/Azure SQL; Yandex Cloud; Helm; MongoDB/Atlas; PostgreSQL; MySQL/MariaDB; ClickHouse; Redis; Snowflake; Kafka; Prefect; Dagster; Temporal; Splunk; Victoria Logs; SignOz; OpenObserve; New Relic; Coralogix; groundcover; Railway; Rocket.Chat; WhatsApp; Twilio; SMTP; Trello; and provider-specific MCP services. An operator can connect a trusted MCP server for some systems, but that does not constitute native client coverage.

See [native connector operations](integration-catalog.md) for exact routes. Filters, pagination modes, authentication schemes, and writes beyond that list remain unimplemented.

## Verification boundaries

Tests exercise local HTTP requests, all four model protocol tool round-trips, MCP sessions/SSE, authentication/signatures, cancellation/checkpoints, schedules, action concurrency, and synthetic incident scenarios. Browser tests cover investigation and operations workflows. The lab recovery changes only synthetic local state.

No real vendor credentials, cloud accounts, messaging channels, or model endpoints were available for certification. Mock protocol tests do not establish production vendor compatibility. This remains a single-tenant alpha; full behavioral coverage and production readiness are outstanding.

See [agent harness](harness.md) for exact runtime policies, fault scenarios, and measurement limits.

See [tool-layer port status](tool-layer-port.md) for exact translated files, source-comparison tests, and deliberate differences.
