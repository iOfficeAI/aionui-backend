//! Team Guide module — capability descriptor, lead-facing tool arg parsing,
//! `aion_*` MCP tool handlers, and Guide MCP server.
//!
//! The Guide MCP server is injected into single-chat agents to expose
//! `aion_create_team` / `aion_list_models` tools. Independent from the
//! per-team `TeamMcpServer`.
//!
//! Current tool set:
//! - `aion_create_team` — build a new team from a natural-language summary
//! - `aion_list_models` — enumerate backend × model options

pub mod capability;
pub mod handlers;
pub mod server;

pub use handlers::{CreateTeamParams, handle_aion_list_models, parse_create_team_args};
pub use server::GuideMcpServer;
