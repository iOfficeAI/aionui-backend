use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crate::registry::AgentRegistry;
use aionui_api_types::{AcpEnvResponse, AcpHealthCheckResponse, AgentMetadata, DetectCliResponse};
use aionui_runtime::resolve_command_path;
use tracing::debug;

/// Detect the CLI path for a given ACP backend using PATH lookup.
///
/// Resolves the vendor label to the first `builtin` row in the metadata
/// catalog, then checks that the row's spawn command is on `$PATH`.
pub(crate) async fn detect_cli(registry: &Arc<AgentRegistry>, backend: &str) -> DetectCliResponse {
    let Some(meta) = registry.find_builtin_by_backend(backend).await else {
        return DetectCliResponse { path: None };
    };

    let path = probe_command(&meta);
    debug!(backend, ?path, "CLI detection result");
    DetectCliResponse { path }
}

/// Perform a health check for an ACP backend.
///
/// Checks CLI availability and measures detection latency.
pub(crate) async fn health_check(registry: &Arc<AgentRegistry>, backend: &str) -> AcpHealthCheckResponse {
    let start = Instant::now();

    let Some(meta) = registry.find_builtin_by_backend(backend).await else {
        return AcpHealthCheckResponse {
            available: false,
            latency: None,
            error: Some(format!("No agent_metadata row for backend '{backend}'")),
        };
    };

    let path = probe_command(&meta);
    let latency_ms = start.elapsed().as_millis() as u64;
    let available = path.is_some();

    AcpHealthCheckResponse {
        available,
        latency: Some(latency_ms),
        error: if available {
            None
        } else {
            Some(format!("Spawn command for backend '{backend}' not found in PATH"))
        },
    }
}

fn probe_command(meta: &AgentMetadata) -> Option<String> {
    let cmd = meta.command.as_deref()?;
    resolve_command_path(cmd).map(|p| p.to_string_lossy().into_owned())
}

/// Get relevant environment variables for ACP operations.
pub(crate) fn get_env() -> AcpEnvResponse {
    let keys = ["PATH", "HOME", "USER", "SHELL", "LANG", "TERM"];
    let env: HashMap<String, String> = keys
        .iter()
        .filter_map(|&key| std::env::var(key).ok().map(|val| (key.into(), val)))
        .collect();

    AcpEnvResponse { env }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_env_returns_at_least_path() {
        let resp = get_env();
        assert!(resp.env.contains_key("PATH") || resp.env.contains_key("HOME"));
    }
}
