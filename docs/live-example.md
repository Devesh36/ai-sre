# Live example: investigate checkout with Codex

This walkthrough was executed on 2026-10-18 using the actual Rust executable.
The inputs are the repository's synthetic checkout evidence and runbook. There
is no configured model or production connection. “Live” means the commands run
the product now; the incident data is a fixture.

Codex works in the shared checkout: it invokes the CLI, reads evidence and
execution records, and explains the findings. AI SRE stores the investigation
and runs the tools. AI SRE's optional internal model loop is configured separately
through a `[model]` block; this walkthrough uses offline triage.

## Run the complete example

From the repository root:

```sh
cargo build --locked
bash scripts/live-example.sh
```

In the managed cloud workspace, first run `source scripts/cloud-env.sh` to select
its Rust toolchain. For an already installed executable, use
`AI_SRE_BIN="$(command -v ai-sre)" bash scripts/live-example.sh`.

The script creates a fresh `.ai-sre/live-example.XXXXXX/` directory, copies only
the demo evidence and runbook, and runs the real REPL with a prepared input file.
It retains `commands.txt`, `session.txt`, `incidents.json`, `config.toml`, and
`incidents.db`. It prints the command for reopening that exact database. No model
credentials or external services are used. This example requires Bash, standard
Unix utilities, and the Rust executable.

## 1. Give Codex an investigation task

Example request:

> Use AI SRE to investigate checkout failures after the latest deployment. Show
> the evidence, recheck it, and explain what we should verify before a rollback.

The script submits this first question to the REPL:

```text
Why is checkout failing after the latest deploy?
```

The actual first report contains these excerpts:

```text
Summary
  2 evidence-backed symptom hypothesis(es); root cause remains unverified.

Findings
  1. Memory pressure detected · Low support
  An out-of-memory signal is present. Check container limits, working-set growth, and
  recent releases before attributing a cause.
  Evidence: E1

  2. Dependency connectivity failure · Low support
  A dependency connection failed. This may reflect availability, routing, or
  saturation; it does not establish which one.
  Evidence: E2
```

The first run collected three incident observations and one runbook using two
local tool calls. `/evidence` lists their sources and summaries. The JSON export
also retains the underlying fields:

| Evidence | Observed fixture data | What it supports |
| --- | --- | --- |
| E1 | `reason: OOMKilled`, exit code `137`, memory limit `512Mi` | A container termination associated with memory exhaustion |
| E2 | `connection refused to inventory:8080`, error rate `18%`, window `5m` | A dependency connectivity symptom |
| E3 | Deployment `v2.4.1`; inventory response caching enabled | A change worth investigating; timing alone does not establish a cause |
| E4 | Checkout incident response runbook | Suggested investigation steps; guidance is not an observation of failure |

## 2. Inspect the evidence together

My assessment after reading the actual saved fields:

> There are two symptoms to investigate: an OOM kill and a refused inventory
> connection. Compare memory growth and limits around the deployment, then
> check inventory readiness, endpoint discovery, and network policy. The
> deployment record makes caching changes relevant to inspect, but does not
> establish that the release caused the outage. Gather those measurements
> before recommending a rollback.

This paragraph is Codex's interpretation of the exported evidence. It is not
text emitted by the offline rules engine. No restart, rollback, or production
change was executed.

## 3. Follow up in the same session

The next submitted question was:

```text
Recheck memory and dependency evidence before deciding on a rollback.
```

The same incident remained active. AI SRE read the two configured sources again
and retained E5–E8 alongside E1–E4. The resulting findings cite E1/E5 for memory
and E2/E6 for connectivity. The synthetic inputs did not change, so the report
still has two low-support hypotheses. These repeated readings are not independent
confirmation, and the follow-up did not establish recovery.

Actual final counters:

```text
8 evidence items · 4 tool calls · 0 model calls
0 input / 0 output tokens
```

Offline triage does not reason conversationally about arbitrary questions. It
recollects configured evidence and applies the same symptom rules. Configure a
model for iterative model-led investigation; see [model configuration](../README.md#configure-a-model).

## 4. Inspect the trace and resume

The script then runs `/status`, `/cost`, `/trace`, and `/sessions`. Each of the
two recorded runs contains:

```text
run started · investigation
tool started · local_evidence
tool completed · local_evidence
tool started · runbooks
tool completed · runbooks
run completed · investigation
```

The full trace includes elapsed milliseconds. Those timings describe a small
local fixture run, not production latency. The saved status is **Triaged**;
the example does not mark the incident resolved.

Run the reopening command printed by the script, then:

```text
/sessions
/resume PASTE_THE_FULL_ID_FROM_SESSIONS
/evidence
/trace
/exit
```

Use the ID from your own run. The captured example's ID is
`4489b9b5-1d4e-44e6-9cc8-9acf71afbfbe`; your database has its own IDs and timestamps.

## Try it interactively or from the CLI

To type the conversation yourself:

```sh
ai-sre --config examples/demo.toml repl --service checkout-api
```

Then enter:

```text
Why is checkout failing after the latest deploy?
/evidence
Recheck memory and dependency evidence before deciding on a rollback.
/cost
/trace
/sessions
/exit
```

Codex can also use the noninteractive CLI and parse JSON:

```sh
ai-sre --config examples/demo.toml ask \
  'Why is checkout failing after the latest deploy?' --service checkout-api --json
```

Copy the returned `id` into `INCIDENT_ID`, then use the same configuration for
follow-ups and inspection:

```sh
INCIDENT_ID='paste-the-returned-uuid-here'
ai-sre --config examples/demo.toml show "$INCIDENT_ID"
ai-sre --config examples/demo.toml ask \
  'Recheck memory and dependency evidence before deciding on a rollback.' \
  --incident "$INCIDENT_ID" --json
ai-sre --config examples/demo.toml trace "$INCIDENT_ID"
```

The interactive demo above uses `examples/demo.toml`; the automated script uses
its own fresh configuration. Use the script's printed config path to inspect
its session from the CLI instead.

## Captured evidence

- [Captured REPL output](examples/live-session.txt), with trailing spaces trimmed. Piped input is not echoed;
  the submitted sequence is in [the runnable script](../scripts/live-example.sh).
- [Actual JSON export](examples/live-incidents.json): one incident, eight evidence
  items, two run IDs, four tool calls, and the saved questions and reports.
- [Demo evidence](../examples/evidence.json) and [runbook](../examples/runbooks/checkout.md).

The captures were checked for one persisted incident, eight sequential evidence
IDs, two distinct run IDs, four tool calls, zero model calls, and the retained
follow-up question. UUIDs, timestamps, local paths, and elapsed times vary when
you rerun the example. Capture timestamps in the JSON are UTC.
