# Workflow registry and shell history

Workflows restrict tool access and add investigation instructions. Publish a TOML workflow as an immutable, content-addressed release in the configured SQLite database:

```toml
name = "dependency-triage"
instructions = ["Collect evidence before making a dependency hypothesis."]
allowed_tools = ["memory_read", "runbooks"]
```

```sh
ai-sre skill publish workflow.toml
ai-sre skill history dependency-triage
ai-sre skill activate dependency-triage DIGEST
ai-sre --skill dependency-triage ask 'Check dependency health' --service checkout-api
ai-sre skill rollback dependency-triage
```

Publishing identical content returns the same release. New content receives the next revision under a SQLite immediate transaction; concurrent publishers receive distinct revisions. Publishing does not activate a release. Activation is explicit, and rollback selects the immediately preceding revision of the active release. A missing target or missing predecessor leaves the active release unchanged. Each workflow supports up to 100 releases. `--skill` loads the active release at process startup and conflicts with `--workflow`; changes require restarting a running server to select the new release.

The shared bearer token protects these API routes:

| Method | Path | Behavior |
| --- | --- | --- |
| GET / POST | `/api/skills` | List latest 100 workflow names / publish a JSON Workflow |
| GET | `/api/skills/{name}/history` | List releases, newest first |
| POST | `/api/skills/{name}/activate` | Activate JSON `{"digest":"..."}` |
| POST | `/api/skills/{name}/rollback` | Activate preceding revision |
| GET | `/api/skills/{name}/release` | Active `{digest,workflow}` bundle, with ETag / conditional 304 |

Import from an operator-selected registry:

```sh
ai-sre skill pull https://registry.example/api/skills/dependency-triage/release --token-env REGISTRY_TOKEN
```

Downloads require HTTPS except on loopback, inherit configured proxy and CA trust, disable redirects, and enforce response/field limits. The client validates the content digest before publishing locally. Conditional requests reuse a cached local release only after a previous successful import. Import never activates. A digest detects corruption; it is not a signature or publisher identity. Only use a trusted registry. Automatic synchronization, multi-file bundles, and signatures are not implemented.

The interactive shell persists the latest 200 questions per service, with a global limit of 5,000 entries. Credential-shaped text and configured secrets are redacted before storage. `/history` shows the latest 20 questions, `/clear-history` clears only the current service, and `/trace` prints the current incident's harness traces. Slash commands are not stored as questions. History survives process restarts; redaction is best effort and SQLite storage is not encrypted.
