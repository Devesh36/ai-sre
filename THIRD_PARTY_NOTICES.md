# Third-party notices

Selected tool-layer modules and MCP transport lifecycle are Rust adaptations of code from
https://github.com/Tracer-Cloud/opensre at revision
`7eadd2018c7672bc17c2c5cf6ae70eeac576c3c1`.

Copyright 2026 Tracer Cloud

Licensed under the Apache License, Version 2.0. The full license is retained in
[licenses/Apache-2.0.txt](licenses/Apache-2.0.txt). No upstream NOTICE file was
present in the audited checkout. Product names in this legal attribution do not
imply endorsement.

Modified files carry a translation notice. Rust types replace Python classes
and dynamic function introspection. Error handling uses Result, configuration
uses explicit per-instance budgets, and the executor uses asynchronous Rust
callbacks. Runtime analytics integrations are not included. Exact translated
functions and remaining gaps are tracked in docs/tool-layer-port.md.
The MCP transport adaptation in integrations/mcp/mod.rs uses a native Rust
process owner and framing implementation. It adds bounded exchanges and explicit
environment mappings; legacy SSE and vendor result normalization remain absent.
Its contracts and remaining differences are tracked in docs/reconstruction.md.

The source application and documentation as a whole have not been copied.
