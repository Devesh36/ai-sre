// Copyright 2026 Tracer Cloud. SPDX-License-Identifier: Apache-2.0
// Modified: Rust enums with serde serialization replace Python string enums.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSurface {
    #[default]
    Chat,
    Action,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRole {
    #[default]
    Action,
    Bookkeeping,
    TurnEnding,
}
