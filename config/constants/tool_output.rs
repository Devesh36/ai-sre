// Copyright 2026 Tracer Cloud. SPDX-License-Identifier: Apache-2.0
// Modified: Rust translation; explicit per-instance configuration replaces process-global reads.
pub const DEFAULT_TOOL_OUTPUT_TOKENS: usize = 10_000;
pub const TOOL_OUTPUT_BYTES_PER_TOKEN: usize = 4;
pub const TOOL_OUTPUT_HISTORY_NUMERATOR: usize = 6;
pub const TOOL_OUTPUT_HISTORY_DENOMINATOR: usize = 5;
pub const TOOL_OUTPUT_CAPTURE_MAX_BYTES: usize = 1024 * 1024;
pub const TOOL_OUTPUT_PERSIST_MAX_BYTES: usize = 64 * 1024;
