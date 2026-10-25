# Offline incident replay

Replay compares the existing offline symptom detector with explicit expectations
from operator-assessment records. The detector receives only the selected run's
retained evidence. Expected labels, claims, rationales, saved reports, incident
titles, and resolution status are not detector input.

This is a detector regression workflow, not a recovery verifier or a model
benchmark. Matching a human label does not make that label true. In the bundled
fixtures, the assessments are explicitly authored synthetic expectations, not
records of real operators investigating production incidents.

## Run the bundled fixtures

```sh
cargo run --locked -- eval --suite replay \
  --dataset examples/replay-cases.json --repeat 2 --output .ai-sre/replay.json

# Machine-readable output; no terminal styling.
cargo run --locked -- eval --suite replay \
  --dataset examples/replay-cases.json --json
```

The seven fixtures cover four recognized symptom families, a healthy current run
with older symptoms and distracting context, an inconclusive observation, and an
explicitly corrected expectation. Two repetitions yield 12 passed case executions,
zero failures, and two unscored case executions.

The command reads its local JSON dataset before configuration or database loading.
It does not use credentials, connectors, models, the incident store, or network
access. Default `sre.toml` is not loaded; explicit `--config`, `--workflow`, and
`--skill` options are rejected. The existing six-case `eval` default and
28-scenario `eval --suite harness` remain separate checks.

## Fixture structure

A version-1 dataset contains `schema_version`, `name`, `origin` (`synthetic` or
`local_fixture`), and a `cases` array. Origin is a declared label, not a verified
provenance assertion. Each case contains:

- `name`: a distinct case name.
- `incident`: a complete incident snapshot with retained evidence and run traces.
- `assessments`: [operator outcome records](operator-outcomes.md) for the snapshot's
  latest successful run, including any same-run correction ancestors.
- `expected`: mappings from an exact offline finding title to an `assessment_id`.

For example, one mapping is:

```json
{
  "finding": "Memory pressure detected",
  "assessment_id": "UUID_OF_THE_RETAINED_ASSESSMENT"
}
```

Use the complete [bundled dataset](../examples/replay-cases.json) as a template.
The other accepted titles are `Dependency connectivity failure`,
`Rate limiting observed`, and `Storage capacity exhausted`. Free-form claims are
not compared semantically to those titles; the fixture author explicitly chooses
the mapping and remains responsible for its meaning.

Validation reuses the operator-outcome recording rules: the incident/run must be
eligible, cited observations must belong to that run, and every stored evidence
fingerprint, source label, and collection timestamp must match the snapshot.
Assessment incident/service/run identifiers and schema version must match too.
Missing correction parents, cycles, multiple successors, duplicate IDs, and
expectations pointing to superseded records are rejected. The latest run must have
one start followed by collection and one final completion; failed/cancelled
terminal events, duplicate starts/completions, and entries after completion are
rejected. The lifecycle validator is shared with outcome recording and historical
recall. Fingerprints detect
inconsistent retained content; they do not authenticate an actor or prevent an
author from changing both a payload and its declared fingerprint.

The initial workflow requires all included assessments and correction ancestors
to belong to the same latest run. Cross-run correction histories are not supported.
There is no automatic database export, production-data discovery, or fixture
publication. Inputs and artifacts stay at the paths explicitly supplied.

## Scoring

| Declared verdict | Detector check |
| --- | --- |
| `supported` | The named finding must be emitted and cite every observation referenced by the assessment. Additional valid citations are allowed. |
| `refuted` | The named finding must not be emitted. This is a fixture expectation about detector output, not proof that the real-world claim is false. |
| `inconclusive` | Record the observation as unscored, regardless of whether the detector emits the finding. Do not convert uncertainty into a passing negative label. |

Any emitted finding not named in the case's expectations is a failure. A case
fails if any scored check fails or an unexpected finding appears; otherwise it
passes if at least one check passes, or remains unscored if none were scored.
A passing case can still contain unscored judgments, whose total is reported
separately. The overall gate requires zero failed cases and at least one passed
case. An entirely unscored dataset cannot produce a passing gate.

A valid dataset with failing checks still writes its result artifact and exits
nonzero. Invalid inputs fail before report replacement. The command refuses an
output path that would overwrite the input dataset. Harness `--baseline` artifacts
are incompatible and are rejected rather than silently compared.

## Fingerprints and reproducibility

Artifacts separate `expected` mappings and `operator_assessments` from the actual
`observed` report, checks, unexpected findings, and pass/fail/unscored state.
They include:

- A canonical parsed-dataset SHA-256 and the existing build-time implementation
  fingerprint, product version, suite/schema version, and repetition counts.
- Per-case `input_sha256` over the exact selected-run evidence vector supplied to
  the detector, including evidence IDs and collection times.
- `expected_sha256` over declared assessment history and expectation mappings.
- Per-evidence payload fingerprints using the same source/summary/data contract
  as operator outcomes, excluding collection time and evidence ID.
- `observed_sha256` over the actual detector report.

Canonical JSON sorts object keys; array order and value types remain significant.
The dataset fingerprint covers the decoded schema, not original file formatting.
Artifacts contain no execution timestamp or latency, so the same dataset and
implementation produce byte-identical JSON. Repetitions expose their ordinal but
do not introduce random IDs or current-time fields. Changing only judgment text
changes expectation/dataset fingerprints without changing detector input or output.

The implementation fingerprint has the same scope as the existing harness: Rust
source roots, manifests/lockfile, build script, and module inventory. Web assets
and integration-test files are not included. Artifact writing reuses the harness's
atomic local writer. Content fingerprints are not signatures or trusted labels.

## Bounds and remaining limits

Datasets are regular local JSON files capped at 2 MiB, with 1–16 cases and 1–10
repetitions. Each case requires 1–100 assessments, 1–4 distinct target findings,
and 1–127 retained evidence items. Evidence IDs must be unique; source/summary
fields are capped at 4096 bytes each and data at 16 KiB per item. Trace count is
capped at 4096. Truncated run input is rejected; context observations can be
included but the offline detector excludes them from findings.

Artifacts and JSON output redact recognized credential patterns (bearer tokens,
password/token/API-key assignments, and known token formats) in presentation
strings, including operator text, source labels, names, and citation IDs. The
`presentation` field describes this projection. Scoring and all fingerprints still
cover the original decoded content, before redaction; hashes cannot necessarily
be recomputed from the redacted artifact alone. The original fixture is unchanged.
Redaction may collapse distinct displayed strings; use original IDs locally to
resolve ambiguity. This does not detect every secret or personal detail: prepare
fixtures before sharing. Replay never reads environment credentials or uploads
files.

Assessment timestamps and actor names remain self-reported metadata; replay does
not authenticate their chronology or authorship.

The evaluator does not replay a model, vendor transport, remediation, recall
ranking, full multi-run agent session, or semantic causal reasoning. The existing
harness remains responsible for runtime/tool conformance. This suite does not
establish production diagnosis quality, label correctness, recovery, or superiority.

`cargo test --locked --test replay` checks deterministic artifacts, label/input
isolation, failure gates, correction lineage, fingerprints, bounds, CLI behavior,
and preservation of an existing output on invalid input. CI is configured to run
the bundled replay twice and retain its JSON report; hosted CI status must be
verified separately from local results.
