//! Team guide module: capability descriptor + lead-facing MCP tool argument parsing.
pub mod capability;
pub mod handlers;

pub use handlers::{CreateTeamParams, parse_create_team_args};
