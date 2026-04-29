//! Team session MCP stdio connection types.
//!
//! These are promoted from `aionui-team::mcp::bridge` so that downstream
//! crates (`aionui-ai-agent` deserializing `AcpBuildExtra`, etc.) can reference
//! the same shape without depending on `aionui-team`.

use serde::{Deserialize, Serialize};

/// Stdio connection triple for the team session MCP server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMcpStdioConfig {
    pub port: u16,
    pub token: String,
    pub slot_id: String,
}

impl TeamMcpStdioConfig {
    /// env key the stdio bridge reads to learn the backend TCP port.
    pub const ENV_PORT: &'static str = "TEAM_MCP_PORT";
    /// env key the stdio bridge reads to learn the auth token.
    pub const ENV_TOKEN: &'static str = "TEAM_MCP_TOKEN";
    /// env key the stdio bridge reads to learn which agent slot it represents.
    pub const ENV_SLOT_ID: &'static str = "TEAM_AGENT_SLOT_ID";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip_preserves_all_fields() {
        let cfg = TeamMcpStdioConfig {
            port: 54321,
            token: "tok-abc".into(),
            slot_id: "slot-1".into(),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: TeamMcpStdioConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, parsed);
    }

    #[test]
    fn deserialization_tolerates_unknown_fields() {
        // Forward-compat: extra fields in persisted `conversation.extra.team_mcp_stdio_config`
        // JSON (e.g. added by a later backend version) must still round-trip through
        // older binaries without error.
        let json = r#"{"port":1,"token":"t","slot_id":"s","future_field":42}"#;
        let parsed: TeamMcpStdioConfig = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.port, 1);
        assert_eq!(parsed.token, "t");
        assert_eq!(parsed.slot_id, "s");
    }
}
