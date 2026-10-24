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

1. Operator-reviewed outcomes: record which hypotheses were confirmed or rejected,
   the evidence used, and which intervention was verified. Resolution alone must
   never be used as a successful-remediation label.
2. Replayable cases: turn reviewed outcomes into versioned evaluation fixtures,
   retaining provenance and counterexamples. Gate retrieval and diagnosis changes
   against these cases before adding them to a release.
3. Source-aware event matching: distinguish meaningful changes from volatile
   response metadata, with explicit per-source rules and false-match tests.
4. Measure retrieval precision and time to useful evidence on permissioned real
   incidents. Use those results to choose the next integration or workflow.

These are planned increments, not background jobs or capabilities already shipped.
