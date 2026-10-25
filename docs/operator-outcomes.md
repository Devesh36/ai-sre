# Operator-recorded outcomes

An operator can record whether a specific claim is **supported**, **refuted**, or
**inconclusive**, with a rationale and observation references from a completed run.
These are human assessments, not automatically verified facts. Recording one does
not resolve the incident, change its report, or authorize remediation.

This increment supports recording through the CLI and authenticated API. The REPL
and dashboard display history; they do not yet provide an assessment-entry form.
Actor names are self-reported labels. The workspace API token controls access but
does not establish a verified identity for the named actor.

## Record and inspect

From a checkout, run `cargo build --locked` and substitute `target/debug/ai-sre`
for `ai-sre` in these examples.

Inspect the saved incident with `ai-sre show INCIDENT_ID`. Use its latest completed
`run_id` and observation IDs collected in that run. Runbook text, operator notes,
proposals, and observations from older runs cannot be used as outcome citations.

```sh
ai-sre outcome record INCIDENT_ID --run RUN_ID \
  --actor oncall-alex \
  --claim 'The container event reports memory exhaustion' \
  --verdict supported \
  --rationale 'Reviewed the termination event and configured memory limit; the cause of the growth remains unknown.' \
  --evidence EVIDENCE_ID

ai-sre outcome list INCIDENT_ID
ai-sre outcome list INCIDENT_ID --json
```

`--evidence` accepts comma-separated IDs or repeated flags. A record requires
1–16 distinct observation IDs, an actor (1–128 bytes), a claim (1–1000 bytes), and a
rationale (1–4000 bytes). Text is trimmed and passed through the configured
best-effort redaction before persistence. Avoid typing credentials into claims.
The command prints JSON containing `created` and `outcome`.

Use `/outcomes` to inspect the active incident in the REPL. The dashboard's
**Outcomes** tab shows every retained assessment, correction, run reference, and
evidence fingerprint. Evidence links open the retained observation.

Run `bash scripts/recall-example.sh` after `cargo build --locked` for a complete
synthetic example: investigate, record an explicit assessment, resolve the past
case, investigate a recurrence, and recall the historical assessment. Marking the
past case resolved is a separate action; neither step proves a recovery.

## Provenance, retries, and corrections

Each record retains its UUID, incident and service, run ID, incident revision at
recording, server recording time, actor, claim, verdict, rationale, and cited
evidence references. Each reference includes source, collection time, and the
SHA-256 of the canonical retained source/summary/data. The hash identifies the
retained payload; it is not a signature, trusted event timestamp, or proof that a
provider's data was correct. Raw payloads are not copied into outcome records.

Pass `--id RECORD_UUID` when reliable retries are needed. Retrying the same
normalized request returns the original record with `created: false`, even if a
later run has started. Reusing that ID for different content returns a conflict.

To correct an assessment, submit a new record with `--supersedes OLD_RECORD_UUID`.
The correction must belong to the same incident and reference the current
successful run's observations. The original remains unchanged. Two concurrent
corrections to the same record cannot both succeed; reload history and correct
the newest record if further changes are needed.

There is a hard limit of 100 records per incident, including corrections. Once the
limit is reached, new submissions fail rather than silently dropping provenance;
identical retries remain readable. There is no update/delete endpoint. SQLite
triggers reject direct row updates/deletes, but a filesystem administrator can
replace the database or remove its schema: this is not tamper-proof storage.

## API

`GET /api/incidents/{id}/outcomes` returns `{incident_id, limit, records, notice}`,
with records newest first. Supersession links preserve the complete history.
`POST` on the same route accepts:

```json
{
  "id": "RECORD_UUID",
  "run_id": "CURRENT_RUN_UUID",
  "actor": "oncall-alex",
  "claim": "The container event reports memory exhaustion",
  "verdict": "supported",
  "rationale": "Reviewed the termination event; the cause remains unknown.",
  "evidence_ids": ["E1"],
  "supersedes": null
}
```

Replace placeholders with actual UUIDs and current-run evidence IDs. Recording
returns 201; an identical retry returns 200. Missing incidents return 404;
invalid citations or text return 400; stale runs, conflicting record IDs,
concurrent corrections, and the history limit return 409. Unknown fields or invalid
JSON types return 422. The normal API token, host/origin restrictions, and request
body limit apply. Recording time, fingerprints, and incident identity are derived
by the server and cannot be supplied as extra fields.

The incident must be triaged or resolved with the specified latest successful
run, a complete trace, no recorded collection failures, and retained observation
references. Active, failed, cancelled, unlinked, or capped investigations cannot
accept new assessments. Validation and insertion occur in one write transaction
so another worker cannot change the run between the check and the write.

## Historical assessments stay historical

Recall attaches up to five non-superseded assessments **only for its matched past
run**, with an explicit total count and historical labels. They do not change
recall ranking, become current observations, enter model prompts, or change
confidence or status. An assessment from a different run is not attached merely
because it belongs to the same past incident. A resolved case without an explicit
assessment continues to have no outcome record.

The system checks provenance and input constraints. It does not independently
validate an operator's reasoning, identify a human caller, authenticate the actor
label, or certify an intervention's effectiveness. Reviewed dataset export and
replayable outcome evaluations remain future work.

## Acceptance checks

`cargo test --locked --test outcomes` exercises persistence, immutable history,
concurrent retries/corrections, caps, API authorization and status codes,
redaction, CLI/REPL access, run scoping, and isolation from future investigations.
`npm run test:browser` adds a real local workflow through the API and dashboard,
including corrections, escaped text, evidence fingerprints, historical citations,
current-incident isolation, and mobile layout. These checks use synthetic data.
