use std::sync::Arc;
use std::time::Duration;

use aionui_common::{AgentKillReason, ErrorChain};
use tracing::{debug, info, warn};

use crate::task_manager::IWorkerTaskManager;

/// Default idle timeout for ACP agents (5 minutes).
const DEFAULT_IDLE_TIMEOUT_MS: i64 = 5 * 60 * 1000;

/// Scan interval for idle agent cleanup (1 minute).
const SCAN_INTERVAL_SECS: u64 = 60;

/// Start the background idle agent scanner.
///
/// Periodically scans active tasks and kills ACP agents that have been
/// idle (finished + no activity) beyond the configured threshold.
///
/// The scanner runs until the provided `shutdown` signal resolves.
pub fn start_idle_scanner(
    worker_task_manager: Arc<dyn IWorkerTaskManager>,
    idle_timeout_ms: Option<i64>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    let threshold = idle_timeout_ms.unwrap_or(DEFAULT_IDLE_TIMEOUT_MS);
    info!(
        threshold_ms = threshold,
        scan_interval_secs = SCAN_INTERVAL_SECS,
        "Starting idle agent scanner"
    );

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(SCAN_INTERVAL_SECS));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    scan_and_cleanup(&worker_task_manager, threshold);
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("Idle scanner received shutdown signal");
                        break;
                    }
                }
            }
        }

        info!("Idle scanner stopped");
    })
}

/// Perform one scan: find idle tasks and kill them.
fn scan_and_cleanup(manager: &Arc<dyn IWorkerTaskManager>, threshold_ms: i64) {
    let idle_ids = manager.collect_idle(threshold_ms);

    if idle_ids.is_empty() {
        debug!(active = manager.active_count(), "Idle scan: no idle agents found");
        return;
    }

    info!(count = idle_ids.len(), "Idle scan: cleaning up idle agents");

    for id in idle_ids {
        if let Err(e) = manager.kill(&id, Some(AgentKillReason::IdleTimeout)) {
            warn!(
                conversation_id = %id,
                error = %ErrorChain(&e),
                "Failed to kill idle agent"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_idle_timeout_is_5_minutes() {
        assert_eq!(DEFAULT_IDLE_TIMEOUT_MS, 300_000);
    }

    #[test]
    fn scan_interval_is_60_seconds() {
        assert_eq!(SCAN_INTERVAL_SECS, 60);
    }
}
