# Agent harness

The harness governs the production investigation loop used by the CLI, API, and scheduler. The evaluator drives that same loop against scripted HTTP model responses and faulting evidence endpoints, with a separate temporary SQLite database for each scenario. Neither credentials nor external services are needed.

## Runtime controls

Add an optional table to the investigation configuration:

```toml
[harness]
deadline_seconds = 300
max_model_attempts = 18
max_context_bytes = 131072
max_reported_tokens = 100000
report_repairs = 1
source_failure_threshold = 2
```

These defaults apply to each run. Existing `max_turns`, `max_tool_calls`, and HTTP timeout limits also apply; retries and repairs never extend them.

| Control | Enforced behavior |
| --- | --- |
| Model attempts | Persist a started trace and increment usage before each HTTP dispatch, including retries and failed responses. Stop before an attempt beyond the budget. |
| Retry policy | Retry only 429/502/503/504, at most twice per logical completion. Honor integer Retry-After values up to two seconds; longer or unsupported header values stop automatic retry instead of sending early. Use bounded backoff when the header is absent. Authentication failures and transport errors are not automatically retried. |
| Reported tokens | Stop further model/tool work once the run's reported usage reaches the threshold. A final valid report can still be accepted. This is an observed-usage threshold, not a prepaid limit: the call crossing it can overshoot, and omitted or inaccurate provider usage cannot enforce a spend cap. |
| Context | Measure the complete encoded provider request, including tool schemas. If too large, restart the protocol conversation with pinned system/workflow instructions, the active question, explicit evidence excerpts, previous summary, and recent gaps. Fail before dispatch if this projection still exceeds the limit. |
| Duplicate requests | Reuse successful identical tool results within one run, including empty results and local action proposals. Preserve original evidence IDs and observation times, mark results cached, and count requests against the tool-request budget. A new run obtains fresh observations. |
| Source circuit | Typed transport failures count against the configured source. After the threshold, skip that source's sibling operations for the rest of the run. Success resets the count; application/argument errors do not increment it. The next run probes again. |
| Tool batches | Validate call IDs and remaining request budget before any batch dispatch. Reject duplicate IDs across the run. Unknown tools and malformed arguments produce bounded correction feedback; they do not dispatch arbitrary capabilities. |
| Report repair | Allow at most the configured number of correction turns for malformed reports or invalid citations. Findings need nonempty text, existing citations, and at least one observation; notes, runbooks, and proposals alone cannot support a finding. |
| Cancellation | Preserve completed evidence and pending request traces. Stop awaiting in-flight work and commit a terminal cancellation. Cancellation at final commit takes precedence over completion. |

Context projection is lossy and explicitly labeled. It preserves stored data, but cannot guarantee the model sees every relevant detail. Request bytes are exact UTF-8 wire-body bytes; they are not a tokenizer estimate. Cache entries and circuit state are run-local and are not an exact continuation mechanism after process death. A read-only MCP declaration remains a trust assertion by the configured server.

An existing evidence ID proves an observation was collected, not that a model's conclusion is correct. Prompt-injection fixtures test enforced tool boundaries and pinned instructions; they do not establish immunity to adversarial model behavior. Reviewed infrastructure actions use a separate executor and retain their own single-dispatch safeguards.

## Inspect execution

```sh
ai-sre --config examples/demo.toml trace INCIDENT_UUID
```

The incident JSON also includes `trace`, and the dashboard has a Trace tab. Each entry records the run ID, phase, operation, elapsed milliseconds from run start, encoded request size, and evidence IDs. Traces omit prompts, argument values, raw tool output, and credential values. Ordinary incident evidence remains in the database under the existing access policy. Trace retention is bounded to approximately the latest 4,096 entries.

Started traces are checkpointed before network requests. A started event without a completed counterpart indicates an interrupted/failed attempt, not success; use the terminal run status and retry events to interpret it. Elapsed values are cumulative within a run, not independent per-operation latency metrics. Usage includes attempted HTTP calls even when the upstream never receives a request.

## Reproducible conformance suite

```sh
cargo run --locked -- eval --suite harness --repeat 3 --output .ai-sre/harness.json
cargo run --locked -- eval --suite harness --json
cargo run --locked -- eval --suite harness \
  --baseline .ai-sre/harness.json --output .ai-sre/harness-next.json
```

The suite has 28 checked-in scenarios covering normal grounding, deduplicated reads, rejected tools/arguments, malformed reports, fabricated citations, context-only claims, retries, authentication failure, attempt/token/tool budgets, repeated call IDs, context pressure, transport circuits, cancellation, and deadlines. Each case checks expected terminal status and counts, actual upstream requests, durable checkpoints before dispatch, context ceilings, task preservation, run identity, trace payload exclusion, and database reopening. Independent Rust tests also cover sibling-source suppression, circuit/cache reset between runs, Unicode truncation, baseline rejection, and cancellation at commit.

Artifacts contain individual checks, repetition number, raw case latency, model attempts, tool dispatches, request sizes, a scenario digest, product version, and a digest of the runtime/evaluator sources and dependency lockfile. The build-time implementation digest covers all Rust files under the nine source roots, Cargo.toml, Cargo.lock, build.rs, and the module inventory. It does not cover web assets or integration-test files. Writes atomically replace the destination after serialization. A failed scenario still produces a report and exits nonzero. Test setup errors/timeouts are recorded as failures, not skipped.

Baseline comparison requires the same schema, suite, scenario digest, and case-name coverage. It rejects formerly passing cases that fail or increased model/tool dispatch counts. A current failed case always fails the gate, even if the baseline also failed. The baseline is an operator-selected artifact, not an authenticated benchmark record. Timing is reported as p50/p95 across all scenario executions, including deliberate fault delays; it is not a production latency benchmark or a hard CI performance gate.

CI runs the Rust tests and three conformance repetitions and uploads the JSON artifact even on failure. To run the browser checks, start the demo server and execute `node scripts/browser-smoke.cjs`; this also checks the trace interface. The existing `eval` command without `--suite` retains the six offline symptom checks.

## Measuring quality beyond conformance

No shared head-to-head production benchmark has been run. More tests or a different implementation language alone do not establish superiority.

A valid comparative diagnosis study must hold incident inputs, tool permissions, model/provider/version, resource limits, and scoring constant. Use held-out incidents with independent root-cause labels, include healthy/ambiguous/adversarial cases, and measure supported-diagnosis precision/recall, unsupported claims, abstention, recovery verification, cost, and latency. Report repeated-run distributions and failures, separate synthetic from real incidents, and publish both sides' raw results. The current scripted suite measures runtime conformance, not those diagnosis metrics. Native vendor certification, semantic claim verification, exact interrupted-tool continuation, and distributed worker recovery remain outstanding.

## Provider-visible tool output

`harness.tool_output_tokens` defaults to 10,000 approximate tokens (four UTF-8 bytes each). Tool responses preserve their beginning and end with an explicit omission marker; the marker is outside the retained-body budget. Evidence persistence and initial context retain their existing independent bounds. Truncation emits `tool_output_bounded`. See [tool-layer translation](tool-layer-port.md) for the source comparison tests and embedding contracts.
