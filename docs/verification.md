# Verification record

Verified on 25 October 2026 in the Linux cloud workspace with Rust 1.90.0,
Playwright 1.62.1, and Chromium. These results establish the behaviors exercised
below; they do not guarantee an absence of defects.

| Check | Result |
| --- | --- |
| Formatting and Clippy, all targets with warnings denied | Passed |
| Rust unit and integration tests | 94 passed, zero failures |
| Offline symptom evaluation | 6/6 scenarios passed |
| Offline assessment replay | Seven synthetic cases × two repetitions: 12 passed case executions, zero failures, two unscored cases/judgments; eight replay regression tests passed |
| Agent harness | 28 scenarios × 3 repetitions: 84/84 passed |
| Real pseudo-terminal acceptance | Passed launch, completion, history, editing, search, cancellation, resize, error recovery, exit, and terminal restoration |
| Documented live example | Passed in a fresh database; transcript and JSON export produced |
| Dashboard workflow | Passed creation, follow-up, citations, trace, resolution, search, connections, and mobile layout |
| Evidence review | Passed two real offline runs, duplicate detection, run comparison, citation navigation, and Markdown download |
| Operations dashboard | Passed work completion, memory, schedule management, and mobile layout |
| Operator outcomes | Passed ten Rust tests for immutable provenance, stale-run rejection, idempotency, correction races, bounded history, redaction, CLI/API/REPL access, and historical isolation; browser workflow passed |
| Incident recall | Passed nine Rust regression tests, CLI/REPL and authenticated API access, two-case example, historical citation navigation, escaped titles, and mobile layout |
| Dashboard state regressions | Passed delayed responses, selection changes, queued refreshes, incident-specific drafts, cursor position, expanded evidence, and a submission completing after selection changes |

Incident recall adds service-scoped, bounded matching with run-specific evidence references.
The new browser check also preserves expanded match explanations during polling.

The dashboard regression check reproduced lost follow-up text before the fix.
Requests now use a sequence number to reject superseded results; changing the
selected incident immediately removes the previous incident's controls. Drafts
stay with their incident in browser-tab memory, and background rendering preserves
the focused input and expanded evidence. Drafts do not survive a page reload.

Run the commands in [README validation](../README.md#validate) to reproduce these
checks. The browser runner starts a temporary local workspace and removes its
database on exit. CI is configured to run all six browser suites and retain their screenshots,
as well as the harness and replay reports. Hosted CI status was not retrieved for
this increment; the results above are from local checks.

Model protocols, connectors, MCP, and action approval/execution are covered by
local fixtures, synthetic subprocesses, and mock servers. No live vendor accounts
or hosted model credentials were available for this verification. Operator outcomes record human judgments with self-reported actor labels; these
checks do not independently establish the truth of a cause or recovery claim. The browser
operations run did not enable the optional lab action-execution path; Rust tests
exercise the synthetic action lifecycle. Windows, production load, and all
third-party services remain outside this verification. See the
[coverage inventory](coverage.md) for remaining implementation gaps.
