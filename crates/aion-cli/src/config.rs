/// Minimal CLI configuration for Phase 2.
#[derive(Debug, Clone)]
pub struct CliConfig {
    pub server_url: String,
    pub agent_type: String,
    pub model: Option<String>,
}

impl CliConfig {
    pub fn ws_url(&self) -> String {
        let base = self
            .server_url
            .replace("http://", "ws://")
            .replace("https://", "wss://");
        format!("{base}/ws")
    }

    pub fn api_url(&self, path: &str) -> String {
        format!("{}{path}", self.server_url)
    }
}
