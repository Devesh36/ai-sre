# Evidence review, run comparisons, and handoffs

AI SRE helps answer three questions after an investigation: what changed between
runs, how much support the report actually has, and what the next operator needs
to know. These features read saved evidence and traces; they do not need a model,
query external evidence sources, or change incident status.

## Compare observations between runs

```sh
ai-sre --config examples/demo.toml changes INCIDENT_UUID
ai-sre --config examples/demo.toml changes INCIDENT_UUID --json
```

Replace `INCIDENT_UUID` with an ID from `ai-sre --config examples/demo.toml sessions`.
Use the configuration/database that collected the incident. In the REPL, use
`/changes` on the active session. The dashboard's **Review** tab shows the same
comparison with clickable evidence references.

The last two recorded runs are compared using exact fingerprints of the retained
source, summary, and JSON payload. Evidence IDs and local collection timestamps
are excluded; timestamps embedded inside a provider's payload remain significant.
Object keys are canonicalized, while array order and value types are preserved.
Runbooks, operator context, and action proposals are excluded from comparison.

- **Added:** a latest-run payload has no identical baseline payload.
- **Not repeated:** a baseline payload has no identical latest-run counterpart.
- **Same retained payload:** references from both runs share an exact fingerprint.

“Not repeated” does not establish recovery. A different query, time window,
filter, or source response can change what was collected. This feature compares
observed payloads; it does not infer resource identity or semantic field changes.

Comparison is withheld when there are fewer than two recorded runs, a required
run start/success trace is missing, tool attempts failed/were rejected/were blocked
by a circuit, referenced evidence is missing, a run is active, or the 128-item
incident evidence limit was reached. Failed runs are not silently skipped to find
a baseline. Truncated payloads carry an explicit limitation: equal retained
excerpts cannot establish equality of the original responses.

## Review the support behind a report

```sh
ai-sre --config examples/demo.toml review INCIDENT_UUID
ai-sre --config examples/demo.toml review INCIDENT_UUID --json
```

Use `/review` in the REPL, or **Review evidence** / **Review** in the dashboard.
The review shows:

- Observation count, distinct retained payloads, repeated payloads, and context items.
- Each finding's citations linked to the latest run, plus older or unlinked citations.
- Missing citations and context-only support.
- Repeated support and the source labels represented by each finding.
- Supporting observations collected over 30 minutes ago or dated in the future.
- Follow-up checks for incomplete collection, insufficient current support, and repetition.

The bundled two-run demo has **six observations, three distinct payloads, three
repeats, and two runbook context items**. Its eight evidence IDs do not constitute
eight independent observations. A cached tool result referencing existing IDs
does not increase these counts.

Collection age is checked when the review is generated, using a fixed 30-minute
window. It is not provider event age or a service-specific freshness SLA. Open
the Review tab again or click Refresh to update age checks. Distinct source
labels also do not prove independence: connectors may serve the same underlying data.

Reviews can expose historical-only support even if the report still contains
the finding. They do not rewrite reports, change confidence, prove contradictions,
verify causality, or authorize actions. With legacy records lacking run traces,
current-run support is unknown and comparison remains unavailable.

## Create a handoff for the next operator

```sh
ai-sre --config examples/demo.toml handoff INCIDENT_UUID > handoff.md
```

Use the dashboard's **Handoff** button to download Markdown. `/handoff` prints a
wrapped preview inside the REPL; use the CLI export for an unwrapped file.

The handoff contains the service, incident status, retained report, cited findings,
evidence concerns, suggested checks, reported unknowns, run comparison, evidence
index with collection times, and run outcomes. A report retained after an
unsuccessful run is labeled. No report is fabricated when one is unavailable.

Untrusted fields are escaped for Markdown and control characters are removed.
Raw tool payloads are omitted; the text still contains incident summaries and
follows the existing storage/redaction policy. Export creates a local file or
browser download. Nothing is sent to a teammate or external channel automatically.

## API and verification

Both endpoints use the existing API authentication and loopback-host policy:

- `GET /api/incidents/{id}/review`: structured review, including `changes`,
  `findings`, `checks`, `limitations`, and the incident revision reviewed.
- `GET /api/incidents/{id}/handoff`: `{ "markdown": "..." }`.

Neither endpoint mutates the incident. The dashboard verifies that review and
incident revisions match. A run that changes between requests causes the review
to refresh instead of displaying mixed revisions.

```sh
cargo test --locked --test review
python3 scripts/terminal-smoke.py target/debug/ai-sre
SRE_TEST_URL=http://127.0.0.1:8080 node scripts/review-smoke.cjs
```

The browser check needs Playwright and Chromium and a server configured with the
bundled evidence and runbook. It exercises two actual runs, duplicate counts,
citation navigation, Markdown download, and mobile layout. Rust tests also cover
changed inputs through the real investigator, partial/cancelled runs, missing
traces/citations, collection age, future clocks, retained payload limits,
Markdown escaping, authenticated API access, persistence, and CLI exports.
