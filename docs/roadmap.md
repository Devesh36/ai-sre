# Remaining implementation

Full behavioral coverage is not complete. See [coverage](coverage.md) for the implementation inventory and verification boundaries.

1. Native cloud SDKs and database/data-platform drivers with workload identity and scoped credentials.
2. Agent delegation, multimodal input, token streaming, isolated code execution, repository modification, and automated CI repair.
3. Rich skill discovery/versioning, repository analysis, preferences, attachments, search, and interrupted-tool continuation.
4. Bidirectional messaging, Socket Mode/polling/gateway sessions, pairing/revocation, and conversation identity isolation.
5. Hosted accounts, organizations, SSO/RBAC, billing/credits, credential administration, remote synchronization, and managed gateways.
6. Distributed workers, shared database state, migrations, backup/retention automation, and operational monitoring.
7. Signed releases, update/uninstall, cross-platform packaging, containers, and cloud deployment validation.
8. Cloud-backed failure scenarios, production datasets, diagnosis metrics, training interfaces, benchmarks, and live vendor certification.

Complete each with behavioral tests and deployment evidence. A catalog entry, mock response, or Rust implementation alone does not prove production equivalence or competitive superiority.


## Product development priorities

[Incident recall](incident-recall.md) now connects a recurrence to specific runs
and observations in local case history. The next increments should make that
history more trustworthy and measure its practical value:

1. Outcome quality workflows: build on [operator-recorded assessments](operator-outcomes.md)
   with verified actor identities, structured intervention/recovery criteria, and
   adjudication of conflicting claims. The shipped append-only history records
   human judgments; resolution alone is never a successful-remediation label.
2. Extend [offline replay](incident-replay.md) beyond its bounded symptom-detector
   fixtures: reviewed dataset export, permissioned real cases, semantic claim
   adjudication, and retrieval/model replay. The current suite compares declared
   labels to deterministic detector output; it does not measure production diagnosis.
3. Source-aware event matching: distinguish meaningful changes from volatile
   response metadata, with explicit per-source rules and false-match tests.
4. Measure retrieval precision and time to useful evidence on permissioned real
   incidents. Use those results to choose the next integration or workflow.

These are planned increments, not background jobs or capabilities already shipped.
