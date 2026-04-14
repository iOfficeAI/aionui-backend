use aionui_common::TimestampMs;
use serde::{Deserialize, Serialize};

/// Row mapping for the `mcp_servers` table.
///
/// Enum-like fields (`transport_type`, `status`) are stored as TEXT.
/// The service layer converts them to/from domain enums.
///
/// JSON fields (`transport_config`, `tools`) are stored as TEXT in SQLite
/// and deserialized by the service layer.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct McpServerRow {
    pub id: String,
    /// Unique server name (used as identifier when syncing to Agent CLIs).
    pub name: String,
    pub description: Option<String>,
    /// Whether this server is synced to Agent CLIs.
    pub enabled: bool,
    /// One of: "stdio", "sse", "http".
    pub transport_type: String,
    /// JSON object: command/args/env (stdio) or url/headers (sse/http).
    pub transport_config: String,
    /// JSON array of tool descriptions (populated after connection test).
    pub tools: Option<String>,
    /// One of: "connected", "disconnected", "error", "testing".
    pub status: String,
    pub last_connected: Option<TimestampMs>,
    /// Original JSON text for editing restoration.
    pub original_json: Option<String>,
    /// Whether this is a built-in server (hidden from edit/delete in UI).
    pub builtin: bool,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}
