use crate::manager::acp::AcpAgentManager;
use crate::protocol::events::AgentStreamEvent;
use crate::shared_kernel::ModeId;
use agent_client_protocol::schema::{SessionConfigOption, SessionModeState, SessionModelState, UsageUpdate};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::broadcast;

impl AcpAgentManager {
    /// Start the session event tracker loop.
    pub fn start_session_event_tracker(self: &Arc<Self>) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let mut rx = this.event_tx.subscribe();
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        this.apply_event_to_session(&event).await;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                }
            }
        });
    }

    /// Mirror a stream event into the `AcpSession` aggregate's observed/advertised
    /// layer and forward any resulting domain events to the persistence consumer.
    async fn apply_event_to_session(&self, event: &AgentStreamEvent) {
        match event {
            AgentStreamEvent::AcpModeInfo(value) => {
                if let Ok(update) = serde_json::from_value::<SessionModeState>(value.clone()) {
                    let mut s = self.session.write().await;
                    s.apply_advertised_modes(update);
                    self.commit_session_changes(&mut s).await;
                } else if let Some(current_id) = value.get("currentModeId").and_then(|v: &Value| v.as_str()) {
                    let mut s = self.session.write().await;
                    s.apply_observed_mode(ModeId::new(current_id));
                    self.commit_session_changes(&mut s).await;
                }
            }
            AgentStreamEvent::AcpModelInfo(value) => {
                if let Ok(update) = serde_json::from_value::<SessionModelState>(value.clone()) {
                    let mut s = self.session.write().await;
                    s.apply_advertised_models(update);
                    self.commit_session_changes(&mut s).await;
                }
            }
            AgentStreamEvent::AcpConfigOption(value) => {
                if let Ok(update) = serde_json::from_value::<Vec<SessionConfigOption>>(value.clone()) {
                    let mut s = self.session.write().await;
                    s.apply_advertised_config_options(update);
                    self.commit_session_changes(&mut s).await;
                }
            }
            AgentStreamEvent::AcpContextUsage(value) => {
                if let Ok(update) = serde_json::from_value::<UsageUpdate>(value.clone()) {
                    let mut s = self.session.write().await;
                    s.apply_context_usage(update);
                }
            }
            _ => {}
        }
    }
}
