//! Rust subsystem roots and stable compatibility exports.
#[path = "../bootstrap/mod.rs"]
pub mod bootstrap;
#[path = "../config/mod.rs"]
pub mod config;
#[path = "../core/mod.rs"]
pub mod core;
#[path = "../gateway/mod.rs"]
pub mod gateway;
#[path = "../infrastructure/mod.rs"]
pub mod infrastructure;
#[path = "../integrations/mod.rs"]
pub mod integrations;
#[path = "../surfaces/mod.rs"]
pub mod surfaces;
#[path = "../tools/mod.rs"]
pub mod tools;
pub use core::{agent, agent_harness as harness, domain, llm as providers};
pub use gateway::web::api;
pub(crate) use gateway::web::operations as ops_api;
pub use infrastructure::{
    database as store,
    evaluation::{harness as evaluation, lab},
    operations,
};
pub use integrations::mcp;
pub use surfaces::cli::commands as cli_ops;
pub use tools::action as actions;
