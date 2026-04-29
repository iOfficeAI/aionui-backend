use std::collections::HashMap;

use crate::stream_event::AgentStreamEvent;
use agent_client_protocol::schema::{
    AgentCapabilities, AuthMethod, AvailableCommand, LoadSessionResponse, SessionConfigOption,
    SessionModeState, SessionModelState, UsageUpdate,
};

/// Decoded per-session runtime state loaded from `acp_session.session_config.runtime`.
///
/// Only carries the user's last *choices* — the enumerations of what
/// the agent supports (mode list, model list, config schema) come from
/// the `agent_metadata` cache on the frontend, or from the ACP
/// load-session response on the backend. See
/// [`AcpRuntimeSnapshot::preload_persisted`] for how these merge.
#[derive(Debug, Clone, Default)]
pub struct PersistedSessionState {
    pub current_mode_id: Option<String>,
    pub current_model_id: Option<String>,
    /// Map of `config_id -> value` captured from user selections.
    pub config_selections: HashMap<String, String>,
    pub context_usage: Option<UsageUpdate>,
}

#[derive(Debug, Clone, Default)]
pub struct AcpRuntimeSnapshot {
    modes: Option<SessionModeState>,
    model_info: Option<SessionModelState>,
    config_options: Option<Vec<SessionConfigOption>>,
    context_usage: Option<UsageUpdate>,
    agent_capabilities: Option<AgentCapabilities>,
    auth_methods: Option<Vec<AuthMethod>>,
    available_commands: Option<Vec<AvailableCommand>>,

    /// User-selected `config_id -> value` pairs. Populated from
    /// `acp_session.session_config.runtime` on resume; on a fresh
    /// `session/new` it stays empty because the CLI's own response
    /// carries the initial values via `config_options`.
    ///
    /// Kept separate from `config_options` because the DB row only
    /// stores the values, not the labels/schema — those live in
    /// `config_options` once the CLI replies.
    config_selections: HashMap<String, String>,
}

impl AcpRuntimeSnapshot {
    pub fn modes(&self) -> Option<&SessionModeState> {
        self.modes.as_ref()
    }
    pub fn model_info(&self) -> Option<&SessionModelState> {
        self.model_info.as_ref()
    }
    pub fn config_options(&self) -> Option<&[SessionConfigOption]> {
        self.config_options.as_deref()
    }
    pub fn context_usage(&self) -> Option<&UsageUpdate> {
        self.context_usage.as_ref()
    }
    pub fn agent_capabilities(&self) -> Option<&AgentCapabilities> {
        self.agent_capabilities.as_ref()
    }
    pub fn auth_methods(&self) -> Option<&[AuthMethod]> {
        self.auth_methods.as_deref()
    }
    pub fn available_commands(&self) -> Option<&[AvailableCommand]> {
        self.available_commands.as_deref()
    }

    pub fn set_modes(&mut self, modes: SessionModeState) {
        self.modes = Some(modes);
    }
    pub fn set_model_info(&mut self, model_info: SessionModelState) {
        self.model_info = Some(model_info);
    }
    pub fn set_config_options(&mut self, config_options: Vec<SessionConfigOption>) {
        self.config_options = Some(config_options);
    }
    pub fn set_context_usage(&mut self, context_usage: UsageUpdate) {
        self.context_usage = Some(context_usage);
    }
    pub fn set_agent_capabilities(&mut self, agent_capabilities: AgentCapabilities) {
        self.agent_capabilities = Some(agent_capabilities);
    }
    pub fn set_auth_methods(&mut self, auth_methods: Vec<AuthMethod>) {
        self.auth_methods = Some(auth_methods);
    }
    pub fn set_available_commands(&mut self, available_commands: Vec<AvailableCommand>) {
        self.available_commands = Some(available_commands);
    }
}

impl AcpRuntimeSnapshot {
    pub fn config_selections(&self) -> &HashMap<String, String> {
        &self.config_selections
    }

    /// Seed the snapshot with the user's last choices from
    /// `acp_session.session_config.runtime`. Called by
    /// `AcpAgentManager::session_resume_and_send` **before** the CLI
    /// `session/load` response arrives — so the snapshot has valid
    /// `current_*` values (and context usage) immediately, but
    /// enumeration fields like `available_modes` remain empty until
    /// the CLI replies.
    ///
    /// Does not overwrite `config_options` (schema comes from the
    /// CLI); instead it stores selections in `config_selections` and
    /// lets the merge step align them with the CLI response.
    pub fn preload_persisted(&mut self, state: PersistedSessionState) {
        if let Some(mode_id) = state.current_mode_id {
            self.modes = Some(SessionModeState::new(mode_id, Vec::new()));
        }
        if let Some(model_id) = state.current_model_id {
            self.model_info = Some(SessionModelState::new(model_id, Vec::new()));
        }
        if !state.config_selections.is_empty() {
            self.config_selections = state.config_selections;
        }
        if let Some(usage) = state.context_usage {
            self.context_usage = Some(usage);
        }
    }

    pub fn apply_event(&mut self, event: &AgentStreamEvent) {
        match event {
            AgentStreamEvent::AcpModeInfo(value) => {
                // Full-state payloads (carry `availableModes`) replace
                // the cache outright. Partial updates (`currentModeId`
                // only, which Gemini/Codex send when the user switches
                // modes) must not clobber the enumeration — mutate
                // `current_mode_id` in place and keep the known modes.
                if let Ok(update) = serde_json::from_value::<SessionModeState>(value.clone()) {
                    self.modes = Some(update);
                } else if let Some(current_id) = value.get("currentModeId").and_then(|v| v.as_str())
                {
                    if let Some(existing) = self.modes.as_ref() {
                        let available = existing.available_modes.clone();
                        self.modes = Some(SessionModeState::new(current_id.to_owned(), available));
                    } else {
                        self.modes = Some(SessionModeState::new(current_id.to_owned(), Vec::new()));
                    }
                }
            }
            AgentStreamEvent::AcpModelInfo(value) => {
                if let Ok(update) = serde_json::from_value::<SessionModelState>(value.clone()) {
                    self.model_info = Some(update);
                }
            }
            AgentStreamEvent::AcpConfigOption(value) => {
                if let Ok(update) =
                    serde_json::from_value::<Vec<SessionConfigOption>>(value.clone())
                {
                    self.config_options = Some(update);
                }
            }
            AgentStreamEvent::AcpContextUsage(value) => {
                if let Ok(update) = serde_json::from_value::<UsageUpdate>(value.clone()) {
                    self.context_usage = Some(update);
                }
            }
            AgentStreamEvent::AvailableCommands(data) => {
                self.available_commands = Some(data.commands.clone());
            }
            _ => {}
        }
    }

    pub fn apply_response(&mut self, response: LoadSessionResponse) {
        if let Some(modes) = response.modes {
            self.set_modes(modes);
        }
        if let Some(models) = response.models {
            self.set_model_info(models);
        }
        if let Some(configs) = response.config_options {
            self.set_config_options(configs);
        }
    }

    pub fn current_mode_id(&self) -> Option<String> {
        self.modes
            .as_ref()
            .map(|modes| modes.current_mode_id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::schema::{
        AvailableCommand, ModelInfo, SessionConfigOption, SessionConfigSelectOption, SessionMode,
        SessionModeState, SessionModelState, UsageUpdate,
    };
    use serde_json::json;

    use super::*;

    #[test]
    fn preload_persisted_seeds_current_fields_without_listings() {
        let mut snapshot = AcpRuntimeSnapshot::default();
        let mut selections = HashMap::new();
        selections.insert("reasoning".into(), "high".into());

        snapshot.preload_persisted(PersistedSessionState {
            current_mode_id: Some("plan".into()),
            current_model_id: Some("claude-sonnet-4".into()),
            config_selections: selections,
            context_usage: Some(UsageUpdate::new(512, 8192)),
        });

        // `current_*` populated, enumerations stay empty.
        let modes = snapshot.modes().expect("modes preloaded");
        assert_eq!(modes.current_mode_id.to_string(), "plan");
        assert!(
            modes.available_modes.is_empty(),
            "available_modes must remain empty — filled by CLI load response"
        );

        let models = snapshot.model_info().expect("model info preloaded");
        assert_eq!(models.current_model_id.to_string(), "claude-sonnet-4");
        assert!(models.available_models.is_empty());

        assert_eq!(
            snapshot
                .config_selections()
                .get("reasoning")
                .map(String::as_str),
            Some("high")
        );
        assert!(
            snapshot.config_options().is_none(),
            "config_options stays None — schema comes from CLI reply"
        );

        assert_eq!(snapshot.context_usage().unwrap().used, 512);
    }

    #[test]
    fn preload_persisted_skips_none_fields() {
        let mut snapshot = AcpRuntimeSnapshot::default();
        snapshot.preload_persisted(PersistedSessionState::default());

        assert!(snapshot.modes().is_none());
        assert!(snapshot.model_info().is_none());
        assert!(snapshot.config_selections().is_empty());
        assert!(snapshot.context_usage().is_none());
    }

    #[test]
    fn stores_agent_capabilities() {
        let mut snapshot = AcpRuntimeSnapshot::default();
        snapshot.set_agent_capabilities(AgentCapabilities::new().load_session(true));

        let caps = snapshot
            .agent_capabilities()
            .expect("agent capabilities should be cached");
        assert!(caps.load_session);
    }

    #[test]
    fn applies_mode_update_into_session_mode_state() {
        let mut snapshot = AcpRuntimeSnapshot::default();
        snapshot.set_modes(SessionModeState::new(
            "code",
            vec![
                SessionMode::new("code", "Code"),
                SessionMode::new("plan", "Plan"),
            ],
        ));

        snapshot.apply_event(&AgentStreamEvent::AcpModeInfo(json!({
            "currentModeId": "plan"
        })));

        let modes = snapshot.modes().expect("modes should exist");
        assert_eq!(modes.current_mode_id.to_string(), "plan");
        assert_eq!(modes.available_modes.len(), 2);
    }

    #[test]
    fn applies_config_update_into_sdk_config_options() {
        let mut snapshot = AcpRuntimeSnapshot::default();
        snapshot.apply_event(&AgentStreamEvent::AcpConfigOption(json!([
            SessionConfigOption::select(
                "mode",
                "Mode",
                "code",
                vec![SessionConfigSelectOption::new("code", "Code")],
            )
        ])));

        let config_options = snapshot
            .config_options()
            .expect("config options should be cached");
        assert_eq!(config_options.len(), 1);
        assert_eq!(config_options[0].name, "Mode");
    }

    #[test]
    fn stores_model_info_and_usage() {
        let mut snapshot = AcpRuntimeSnapshot::default();
        snapshot.set_model_info(SessionModelState::new(
            "claude-sonnet-4",
            vec![ModelInfo::new("claude-sonnet-4", "Claude Sonnet 4")],
        ));
        snapshot.set_context_usage(UsageUpdate::new(1024, 8192));

        assert_eq!(
            snapshot
                .model_info()
                .expect("model info should be cached")
                .current_model_id
                .to_string(),
            "claude-sonnet-4"
        );
        assert_eq!(
            snapshot
                .context_usage()
                .expect("usage should be cached")
                .used,
            1024
        );
    }

    #[test]
    fn applies_available_commands_update() {
        let mut snapshot = AcpRuntimeSnapshot::default();
        assert!(snapshot.available_commands().is_none());

        let cmds = vec![
            AvailableCommand::new("review", "Review current changes"),
            AvailableCommand::new("compact", "Summarize conversation"),
        ];

        snapshot.apply_event(&AgentStreamEvent::AvailableCommands(
            crate::stream_event::AvailableCommandsEventData {
                commands: cmds.clone(),
            },
        ));

        let stored = snapshot
            .available_commands()
            .expect("available commands should be cached");
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].name, "review");
        assert_eq!(stored[1].name, "compact");
    }
}
