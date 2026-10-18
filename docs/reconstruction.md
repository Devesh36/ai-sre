# Behavior reconstruction and evidence

This migration uses source inspection to define contracts, followed by Rust
implementations and executable conformance checks. An inventory entry or shared
directory name does not establish behavioral equivalence. Full migration remains
incomplete; [coverage](coverage.md) tracks the remaining areas.

## Methodology reviewed

Reviewed [REA](https://github.com/morluto/rea) at revision
`a25eb5fbb424197cb8e699e15a82a6ff869658aa`: README, `docs/cli.md`,
`docs/native-investigation.md`, and `docs/mcp-contracts.md`. Its applicable ideas
are pinned artifact identity, focused feature tracing, evidence with explicit
unknowns, shared CLI/MCP contracts, and bounded retained-result projections.

REA provides binary/application analysis tools, not automatic Python-to-Rust
translation. For this source-available Python target, direct code and test
inspection establish the relevant contracts without decompilation. No REA
installer, analysis engine, or MCP server was executed, and no REA implementation
code was copied. This is methodology applied to source analysis, not a claim
that REA validated the Rust implementation.

Reviewed document SHA-256 values:

| REA file | SHA-256 |
| --- | --- |
| `README.md` | `75ca44765dfa3d5bce1dfba3c9ddad35fca29f8e8fbc8cae60b4d9078e43e211` |
| `docs/mcp-contracts.md` | `233bbec17421ca524a78ac1480bc0ef95672e7d0addcdc660f6ccc53d2009302` |

## Local MCP transport reconstruction

Reference snapshot: `7eadd2018c7672bc17c2c5cf6ae70eeac576c3c1` (source and license
in [third-party notices](../THIRD_PARTY_NOTICES.md)). Reviewed
`integrations/mcp_client.py`, `integrations/mcp_transport.py`, and their tests.

| Source file | SHA-256 |
| --- | --- |
| `integrations/mcp_client.py` | `2a5de369a45021b43b135d5c90902f2076477ec43b2e9e59181a4f0e7a5be0b1` |
| `integrations/mcp_transport.py` | `75724f842dc0d5cb72115469ab83066b3fc8b3ee8d6ae8c9af001f22d420e1ad` |
| `tests/integrations/test_mcp_client.py` | `3888a1972ece9ef3bc604ff4566c6d1f47d0674bb74dc317e33c6e17c5369caf` |

| Observed contract | Rust implementation / evidence | Difference or unknown |
| --- | --- | --- |
| Explicit stdio command, argv, environment; disabled terminal styling | `Server::validate`, `stdio::Process::spawn`; real CLI/subprocess tests | Explicit credential mappings replace broad environment inheritance |
| Initialize before discovery/calls; session closed on exit | `Client::initialize`, owned child; handshake and Linux reaping assertions | Rust sessions persist across calls and share clone ownership |
| Timeout and cleanup | Per-RPC timeout and cancellation-owned child; hanging peer tests | Cancelling destroys the session; no automatic replay; descendants not managed |
| Transport vocabulary | `Transport::{Stdio, StreamableHttp}` and serialization test | Legacy `sse` mode remains unsupported and fails config decoding |
| Tool errors and structured content | Native MCP envelopes retained by the existing toolbox; tool error test | Python-specific text/resource normalization is not reproduced |
| Delegated protocol behavior | Native Rust framing; split frames, ping, unsupported sampling, wrong IDs, invalid JSON, EOF and flood tests | General server requests, background notifications, and vendor certification remain absent |

Conformance tests run a synthetic Python peer solely as development scaffolding.
They exercise actual OS pipes and the compiled Rust CLI. The reference Python
SDK was not executed side by side: these are contract tests informed by inspected
source and tests, not differential parity results. The older tool-layer port has
separate differential fixtures documented in [its port report](tool-layer-port.md).

```sh
cargo test --locked --test mcp_stdio
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo run --locked -- eval --suite harness --repeat 3 --output .ai-sre/harness.json
```

The added bounds and explicit environment policy are concrete product choices.
They do not establish overall superiority or full compatibility with another
implementation. Broader quality claims require comparable workloads and measured
outcomes, including live services and model providers.

## Verification recorded on 2026-10-18

All 58 Rust tests passed, including seven local MCP conformance tests. Formatting
and strict Clippy checks passed. The 28 runtime scenarios passed three repetitions
(84/84), with zero baseline regressions against the previous tool-layer artifact.
The local report is `.ai-sre/stdio-harness.json`; its runtime implementation digest
is `20e701cfec0bd4361e5f9b1eb710e1ea44ad97a43334ad472e33b5fac58f1968`.
That digest covers the runtime/evaluator and module inventory, not these docs or
the separate subprocess test files. No live provider or third-party MCP server
was involved in this verification.
