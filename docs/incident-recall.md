# Incident recall

AI SRE can search earlier investigations for the same service and explain the
observations that connect them to the current incident. This makes retained
investigations useful during a recurrence without requiring a model or uploading
incident data to a search service.

```sh
ai-sre recall INCIDENT_ID
ai-sre recall INCIDENT_ID --json
```

In the REPL, use `/recall`. In the dashboard, select **Recall**, expand **Why this
case matches**, and follow a current or past evidence citation. Past citations
open the historical case's evidence; they are not added to the current case.
The authenticated API exposes `GET /api/incidents/{id}/recall`.

## Try a recurrence

```sh
cargo build --locked
bash scripts/recall-example.sh
```

The script opens two real offline investigations against copied synthetic fixtures,
records an explicit synthetic operator assessment, marks the first resolved, and prints the second investigation's recall results.
It retains both cases, JSON exports, and the explanation under a fresh
`.ai-sre/recall-example.*` directory. Python is used only by this demonstration to
read JSON IDs. The product itself remains a Rust binary.

Expected match: **three distinct exact payloads and two shared symptom rules**, plus one explicitly labeled historical operator assessment.
The memory and dependency-connection observations match; the deployment payload
also matches exactly. That does not establish a deployment-related root cause.
The demonstration deliberately reuses its inputs and makes no recovery claim.

## How matching works

1. Use observations linked to the current incident's latest successful run. A
   newly claimed, failed, cancelled, unlinked, or incomplete run cannot silently
   fall back to old evidence. Collection-age and future-clock concerns are shown.
2. Search at most 100 earlier, same-service cases with triaged, resolved, failed,
   or cancelled status. Failed/cancelled cases qualify only through an earlier
   successful run; observations from unsuccessful runs cannot supply a match.
   Service filtering happens before the limit. An additional candidate detects
   whether the search was truncated. A SQLite index supports this selection.
3. Compare each eligible completed run in a past case. Return its strongest
   matching run, preferring the newest run on a tie. This can recover a matching
   failure snapshot from before a later recovery run.
4. Rank by distinct exact retained payload matches, then shared symptom rules,
   then case update time, with a deterministic ID tie-break. Return five cases.
   These counts are explanations, not confidence percentages.
5. Show current-only and past-only symptom categories alongside the matches.
   Differences are prompts for investigation, not proof of contradiction or
   recovery. Every match carries current and past evidence IDs plus the past run ID.

Exact matching includes source, summary, and data; it excludes evidence IDs and
collection timestamps. Object key order is canonical; array order and values
matter. Repeated identical observations count once. Broader matches use the
existing four explicit symptom rules: memory pressure, dependency connectivity,
rate limits, and exhausted storage. Titles and generated report text never drive
matching. Runbooks, operator context, proposals, truncated payloads, runs with
failed collection, and incidents at the 128-item evidence cap are excluded.

Recall, outcome recording, and replay share the same run-lifecycle check: one
start, followed by run activity, and one final completion. Duplicate starts or
completions, activity before the start or after completion, and failed/cancelled
terminal events disqualify that run. Validation is scoped to each run, so a later
failed attempt does not erase an earlier successful snapshot. Failed or cancelled
current investigations still cannot fall back to their old observations.
Past failed/cancelled cases require a non-nil current run ID present in the trace;
the matched historical run must end before that attempt's first retained event.
This validates retained event order, not timestamp authenticity or recovery.

Recall also displays up to five non-superseded [operator assessments](operator-outcomes.md)
for the matched historical run, with the total count. These annotations do not
change ranking and are never treated as evidence about the current incident.

## Boundaries

A matching past case is a lead, not a verified cause or recommended remediation.
A resolved status does not establish which action helped. The matched snapshot may
precede the past case's latest report. Inspect its run and evidence before using
that report as context. Recall does not modify stored cases, raise finding
confidence, inject historical claims into model prompts, or execute actions.

This is deterministic matching, not semantic retrieval. Changed event timestamps
inside a payload prevent an exact match; keyword rules can miss unfamiliar
symptoms or misread negation. Queries, source coverage, and collection windows may
differ. Service names match exactly. The 100-case window and five-result cap are
explicit; there is no claim of exhaustive historical search.

Run `cargo test --locked --test recall` for matching, ranking, isolation,
incomplete-history, persistence, API, CLI, and REPL checks. `npm run test:browser`
also exercises two real investigations, historical citations, escaped titles, and
mobile recall layout. These synthetic checks do not measure production retrieval
precision or prove competitive superiority.
