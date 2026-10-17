# Repository layout

The Rust implementation uses eight functional layers. `src/lib.rs` wires these directories into one library crate and preserves the existing public imports. This keeps cross-layer changes compile-checked without introducing separate packages prematurely.

| Directory | Compiled implementation |
| --- | --- |
| `bootstrap/` | File initialization, shutdown, architecture inventory |
| `config/` | Typed settings and validation |
| `core/` | Agent, harness policies/context/traces, model protocols, domain models, tool registry, history contracts |
| `gateway/` | Authenticated HTTP API, Slack/Telegram transports, workflow release API |
| `infrastructure/` | SQLite, scheduling, delivery, local processes, evaluation/lab, workflow registry, shell history |
| `integrations/` | Shared HTTP boundary, MCP client, individual modules for 27 native connector types |
| `surfaces/` | Executable entry point, CLI commands, headless execution, interactive shell, output |
| `tools/` | Tool exports and reviewed action executor |

`web/` holds embedded browser assets. `tests/` contains integration tests. `build.rs` fingerprints the implementation for evaluation artifacts. The architecture test walks Rust module declarations from both entry points and rejects orphaned source files.

## Audited and planned structure

[module-inventory.json](module-inventory.json) records 4,385 tracked files across 755 directories, including 2,208 source modules, 1,538 test modules, and 32,284 declarations. [planned-directory-tree.txt](planned-directory-tree.txt) preserves the normalized logical directory inventory. These are planning artifacts, not thousands of empty folders presented as working Rust code.

Every source/test entry records a proposed Rust target and declaration names. `related_rust` points to an existing implementation in the same subsystem. `partial` means a related subsystem exists, **not that the individual module or its declarations have been ported**. `pending` means no related implementation was mapped. Neither status establishes behavioral parity. Proposed target paths are planning hints and may need consolidation before implementation.

```sh
cargo run --locked -- architecture
cargo run --locked -- architecture --prefix core/agent_harness
cargo run --locked -- architecture --pending
```

The snapshot hash identifies the audited revision; product-specific names are normalized. The inventory contains metadata, not copied implementation, documentation text, or assets. The [capability coverage](coverage.md) describes implemented behavior and outstanding gaps. Full module-by-module implementation and parity testing remain unfinished.
