# Checkout incident response

1. Establish the incident window and affected request paths.
2. Compare container working-set memory with configured limits and historical baselines.
3. Verify inventory service readiness, endpoint discovery, and network policy.
4. Compare error rates before and after the latest deployment.
5. If evidence supports a release regression, have the service owner review a rollback and its data compatibility.
6. Validate recovery using successful checkout requests and sustained error-rate reduction.

This runbook is guidance, not evidence that a particular failure occurred. Never delete customer data or restart production services based only on a keyword match.
