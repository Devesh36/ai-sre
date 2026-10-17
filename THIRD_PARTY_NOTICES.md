# Third-party notices

Selected tool-layer modules are Rust adaptations of code from
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

The source application and documentation as a whole have not been copied.
