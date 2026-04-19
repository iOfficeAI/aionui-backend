use std::collections::HashMap;
use std::sync::Arc;

use aionui_realtime::EventBroadcaster;
use tokio::sync::Mutex;
use tracing::debug;

use crate::error::TeamError;
use crate::events::TeamEventEmitter;
use crate::mailbox::Mailbox;
use crate::task_board::TaskBoard;
use crate::types::{
    MailboxMessage, MailboxMessageType, TeamAgent, TeamTask, TeammateRole, TeammateStatus,
};

pub const WAKE_TIMEOUT_MS: u64 = 60_000;

// ---------------------------------------------------------------------------
// SchedulerAction — actions parsed from an agent's turn response
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum SchedulerAction {
    SendMessage {
        to: String,
        message: String,
    },
    TaskCreate {
        subject: String,
        description: Option<String>,
        owner: Option<String>,
        blocked_by: Vec<String>,
    },
    TaskUpdate {
        task_id: String,
        status: Option<String>,
        description: Option<String>,
        owner: Option<String>,
        blocked_by: Option<Vec<String>>,
    },
    SpawnAgent {
        name: String,
        role: String,
        backend: String,
    },
    IdleNotification {
        summary: Option<String>,
    },
    ShutdownAgent {
        slot_id: String,
        reason: Option<String>,
    },
    RenameAgent {
        slot_id: String,
        new_name: String,
    },
}

// ---------------------------------------------------------------------------
// WakePayload — context assembled for an agent when it is woken up
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct WakePayload {
    pub agent: TeamAgent,
    pub tasks: Vec<TeamTask>,
    pub unread_messages: Vec<MailboxMessage>,
}

// ---------------------------------------------------------------------------
// AgentSlot — per-agent runtime state tracked by the scheduler
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct AgentSlot {
    agent: TeamAgent,
    status: TeammateStatus,
}

// ---------------------------------------------------------------------------
// TeammateManager
// ---------------------------------------------------------------------------

pub struct TeammateManager {
    team_id: String,
    slots: Mutex<HashMap<String, AgentSlot>>,
    mailbox: Arc<Mailbox>,
    task_board: Arc<TaskBoard>,
    events: TeamEventEmitter,
}

impl TeammateManager {
    pub fn new(
        team_id: String,
        agents: &[TeamAgent],
        mailbox: Arc<Mailbox>,
        task_board: Arc<TaskBoard>,
        broadcaster: Arc<dyn EventBroadcaster>,
    ) -> Self {
        let mut slots = HashMap::new();
        for agent in agents {
            slots.insert(
                agent.slot_id.clone(),
                AgentSlot {
                    agent: agent.clone(),
                    status: TeammateStatus::Idle,
                },
            );
        }
        let events = TeamEventEmitter::new(team_id.clone(), broadcaster);
        Self {
            team_id,
            slots: Mutex::new(slots),
            mailbox,
            task_board,
            events,
        }
    }

    pub async fn set_status(
        &self,
        slot_id: &str,
        status: TeammateStatus,
    ) -> Result<(), TeamError> {
        {
            let mut slots = self.slots.lock().await;
            let slot = slots
                .get_mut(slot_id)
                .ok_or_else(|| TeamError::AgentNotFound(slot_id.to_owned()))?;
            slot.status = status;
            slot.agent.status = Some(status);
        }
        self.events.broadcast_agent_status(slot_id, status);
        debug!(team_id = %self.team_id, slot_id, %status, "agent status changed");
        Ok(())
    }

    pub async fn get_status(&self, slot_id: &str) -> Result<TeammateStatus, TeamError> {
        let slots = self.slots.lock().await;
        let slot = slots
            .get(slot_id)
            .ok_or_else(|| TeamError::AgentNotFound(slot_id.to_owned()))?;
        Ok(slot.status)
    }

    pub async fn get_agent(&self, slot_id: &str) -> Result<TeamAgent, TeamError> {
        let slots = self.slots.lock().await;
        let slot = slots
            .get(slot_id)
            .ok_or_else(|| TeamError::AgentNotFound(slot_id.to_owned()))?;
        Ok(slot.agent.clone())
    }

    pub async fn build_wake_payload(
        &self,
        slot_id: &str,
    ) -> Result<WakePayload, TeamError> {
        let agent = self.get_agent(slot_id).await?;
        let tasks = self.task_board.list_tasks(&self.team_id).await?;
        let unread = self.mailbox.read_unread(&self.team_id, slot_id).await?;
        Ok(WakePayload {
            agent,
            tasks,
            unread_messages: unread,
        })
    }

    /// Attempt to wake an idle agent. Returns the payload to send.
    /// Transitions agent from Idle → Working.
    /// Returns `None` if the agent is not idle (skip duplicate wake).
    pub async fn try_wake(
        &self,
        slot_id: &str,
    ) -> Result<Option<WakePayload>, TeamError> {
        let current = self.get_status(slot_id).await?;
        if current != TeammateStatus::Idle {
            debug!(
                team_id = %self.team_id,
                slot_id,
                current_status = %current,
                "skip wake: agent not idle"
            );
            return Ok(None);
        }
        self.set_status(slot_id, TeammateStatus::Working).await?;
        let payload = self.build_wake_payload(slot_id).await?;
        Ok(Some(payload))
    }

    /// Mark agent as idle after turn completion or timeout.
    /// Then check if all teammates are idle → maybe wake leader.
    pub async fn mark_idle(
        &self,
        slot_id: &str,
    ) -> Result<Option<String>, TeamError> {
        self.set_status(slot_id, TeammateStatus::Idle).await?;

        let is_lead = {
            let slots = self.slots.lock().await;
            let slot = slots
                .get(slot_id)
                .ok_or_else(|| TeamError::AgentNotFound(slot_id.to_owned()))?;
            slot.agent.role == TeammateRole::Lead
        };

        if is_lead {
            return Ok(None);
        }

        self.maybe_wake_leader_when_all_idle().await
    }

    /// Execute a single action from the agent's turn response.
    pub async fn execute_action(
        &self,
        from_slot_id: &str,
        action: &SchedulerAction,
    ) -> Result<Option<String>, TeamError> {
        match action {
            SchedulerAction::SendMessage { to, message } => {
                self.handle_send_message(from_slot_id, to, message).await?;
                Ok(None)
            }
            SchedulerAction::TaskCreate {
                subject,
                description,
                owner,
                blocked_by,
            } => {
                self.task_board
                    .create_task(
                        &self.team_id,
                        subject,
                        description.as_deref(),
                        owner.as_deref(),
                        blocked_by,
                    )
                    .await?;
                Ok(None)
            }
            SchedulerAction::TaskUpdate {
                task_id,
                status,
                description,
                owner,
                blocked_by,
            } => {
                use crate::task_board::TaskUpdate;
                use crate::types::TaskStatus;

                let update = TaskUpdate {
                    status: status.as_deref().and_then(TaskStatus::parse),
                    description: description.clone(),
                    owner: owner.clone(),
                    blocked_by: blocked_by.clone(),
                    ..Default::default()
                };
                self.task_board
                    .update_task(&self.team_id, task_id, &update)
                    .await?;
                Ok(None)
            }
            SchedulerAction::IdleNotification { summary } => {
                self.handle_idle_notification(from_slot_id, summary.as_deref())
                    .await
            }
            SchedulerAction::SpawnAgent {
                name,
                role,
                backend,
            } => {
                debug!(
                    team_id = %self.team_id,
                    from = from_slot_id,
                    name, role, backend,
                    "spawn_agent action — requires TeamSession to complete"
                );
                Ok(None)
            }
            SchedulerAction::ShutdownAgent { slot_id, reason } => {
                self.handle_shutdown_agent(from_slot_id, slot_id, reason.as_deref())
                    .await?;
                Ok(None)
            }
            SchedulerAction::RenameAgent { slot_id, new_name } => {
                self.handle_rename_agent(slot_id, new_name).await?;
                Ok(None)
            }
        }
    }

    /// Finalize a turn: execute a batch of actions, then mark agent idle.
    /// Returns an optional leader slot_id to wake if all teammates are idle.
    pub async fn finalize_turn(
        &self,
        slot_id: &str,
        actions: &[SchedulerAction],
    ) -> Result<Option<String>, TeamError> {
        let mut wake_signal = None;
        for action in actions {
            if let Some(leader_id) = self.execute_action(slot_id, action).await? {
                wake_signal = Some(leader_id);
            }
        }

        let has_idle_notification = actions
            .iter()
            .any(|a| matches!(a, SchedulerAction::IdleNotification { .. }));

        if !has_idle_notification {
            self.mark_idle(slot_id).await
        } else {
            Ok(wake_signal)
        }
    }

    /// Add a new agent slot at runtime (for spawn_agent).
    pub async fn add_agent(&self, agent: &TeamAgent) {
        let mut slots = self.slots.lock().await;
        slots.insert(
            agent.slot_id.clone(),
            AgentSlot {
                agent: agent.clone(),
                status: TeammateStatus::Idle,
            },
        );
        self.events.broadcast_agent_spawned(agent);
        debug!(
            team_id = %self.team_id,
            slot_id = %agent.slot_id,
            name = %agent.name,
            "agent added to scheduler"
        );
    }

    /// Remove an agent slot at runtime.
    pub async fn remove_agent(&self, slot_id: &str) -> Result<(), TeamError> {
        let mut slots = self.slots.lock().await;
        slots
            .remove(slot_id)
            .ok_or_else(|| TeamError::AgentNotFound(slot_id.to_owned()))?;
        drop(slots);
        self.events.broadcast_agent_removed(slot_id);
        debug!(team_id = %self.team_id, slot_id, "agent removed from scheduler");
        Ok(())
    }

    /// Rename an agent slot.
    pub async fn rename_agent(
        &self,
        slot_id: &str,
        new_name: &str,
    ) -> Result<(), TeamError> {
        let mut slots = self.slots.lock().await;
        let slot = slots
            .get_mut(slot_id)
            .ok_or_else(|| TeamError::AgentNotFound(slot_id.to_owned()))?;
        slot.agent.name = new_name.to_owned();
        drop(slots);
        self.events.broadcast_agent_renamed(slot_id, new_name);
        debug!(team_id = %self.team_id, slot_id, new_name, "agent renamed");
        Ok(())
    }

    pub async fn list_agents(&self) -> Vec<TeamAgent> {
        let slots = self.slots.lock().await;
        slots.values().map(|s| s.agent.clone()).collect()
    }

    pub async fn list_tasks(&self) -> Result<Vec<crate::types::TeamTask>, TeamError> {
        self.task_board.list_tasks(&self.team_id).await
    }

    pub async fn find_lead_slot_id(&self) -> Option<String> {
        let slots = self.slots.lock().await;
        slots
            .values()
            .find(|s| s.agent.role == TeammateRole::Lead)
            .map(|s| s.agent.slot_id.clone())
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    async fn handle_send_message(
        &self,
        from_slot_id: &str,
        to: &str,
        message: &str,
    ) -> Result<(), TeamError> {
        if to == "*" {
            let slots = self.slots.lock().await;
            let targets: Vec<String> = slots
                .keys()
                .filter(|id| id.as_str() != from_slot_id)
                .cloned()
                .collect();
            drop(slots);

            for target in &targets {
                self.mailbox
                    .write(
                        &self.team_id,
                        target,
                        from_slot_id,
                        MailboxMessageType::Message,
                        message,
                        None,
                    )
                    .await?;
            }
        } else {
            self.mailbox
                .write(
                    &self.team_id,
                    to,
                    from_slot_id,
                    MailboxMessageType::Message,
                    message,
                    None,
                )
                .await?;
        }
        Ok(())
    }

    async fn handle_idle_notification(
        &self,
        from_slot_id: &str,
        summary: Option<&str>,
    ) -> Result<Option<String>, TeamError> {
        let lead_slot_id = self
            .find_lead_slot_id()
            .await
            .ok_or_else(|| TeamError::AgentNotFound("no lead agent".into()))?;

        if from_slot_id != lead_slot_id {
            self.mailbox
                .write(
                    &self.team_id,
                    &lead_slot_id,
                    from_slot_id,
                    MailboxMessageType::IdleNotification,
                    summary.unwrap_or("idle"),
                    summary,
                )
                .await?;
        }

        self.mark_idle(from_slot_id).await
    }

    async fn handle_shutdown_agent(
        &self,
        from_slot_id: &str,
        target_slot_id: &str,
        reason: Option<&str>,
    ) -> Result<(), TeamError> {
        let from_role = {
            let slots = self.slots.lock().await;
            let slot = slots
                .get(from_slot_id)
                .ok_or_else(|| TeamError::AgentNotFound(from_slot_id.to_owned()))?;
            slot.agent.role
        };

        if from_role != TeammateRole::Lead {
            return Err(TeamError::InvalidRequest(
                "only lead can shutdown agents".into(),
            ));
        }

        {
            let slots = self.slots.lock().await;
            if !slots.contains_key(target_slot_id) {
                return Err(TeamError::AgentNotFound(target_slot_id.to_owned()));
            }
        }

        self.mailbox
            .write(
                &self.team_id,
                target_slot_id,
                from_slot_id,
                MailboxMessageType::ShutdownRequest,
                reason.unwrap_or("shutdown requested"),
                None,
            )
            .await?;

        Ok(())
    }

    async fn handle_rename_agent(
        &self,
        slot_id: &str,
        new_name: &str,
    ) -> Result<(), TeamError> {
        self.rename_agent(slot_id, new_name).await
    }

    async fn maybe_wake_leader_when_all_idle(
        &self,
    ) -> Result<Option<String>, TeamError> {
        let slots = self.slots.lock().await;

        let mut lead_slot_id = None;
        let mut all_teammates_idle = true;
        let mut has_teammates = false;

        for slot in slots.values() {
            if slot.agent.role == TeammateRole::Lead {
                lead_slot_id = Some(slot.agent.slot_id.clone());
                continue;
            }
            has_teammates = true;
            if slot.status != TeammateStatus::Idle {
                all_teammates_idle = false;
                break;
            }
        }

        let Some(lead_id) = lead_slot_id else {
            return Ok(None);
        };

        if !has_teammates {
            return Ok(None);
        }

        if !all_teammates_idle {
            return Ok(None);
        }

        let lead_is_idle = slots
            .get(&lead_id)
            .map(|s| s.status == TeammateStatus::Idle)
            .unwrap_or(false);

        if !lead_is_idle {
            return Ok(None);
        }

        drop(slots);

        debug!(
            team_id = %self.team_id,
            lead_slot_id = %lead_id,
            "all teammates idle — signaling to wake leader"
        );

        Ok(Some(lead_id))
    }

}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use aionui_api_types::WebSocketMessage;
    use crate::test_utils::MockTeamRepo;

    struct RecordingBroadcaster {
        events: std::sync::Mutex<Vec<WebSocketMessage<serde_json::Value>>>,
    }

    impl RecordingBroadcaster {
        fn new() -> Self {
            Self {
                events: std::sync::Mutex::new(vec![]),
            }
        }

        fn events(&self) -> Vec<WebSocketMessage<serde_json::Value>> {
            self.events.lock().unwrap().clone()
        }
    }

    impl EventBroadcaster for RecordingBroadcaster {
        fn broadcast(&self, event: WebSocketMessage<serde_json::Value>) {
            self.events.lock().unwrap().push(event);
        }
    }

    fn make_agent(slot_id: &str, name: &str, role: TeammateRole) -> TeamAgent {
        TeamAgent {
            slot_id: slot_id.into(),
            name: name.into(),
            role,
            conversation_id: format!("conv-{slot_id}"),
            backend: "acp".into(),
            model: "claude".into(),
            custom_agent_id: None,
            status: None,
        }
    }

    fn make_team_agents() -> Vec<TeamAgent> {
        vec![
            make_agent("lead-1", "Lead", TeammateRole::Lead),
            make_agent("worker-1", "Worker1", TeammateRole::Teammate),
            make_agent("worker-2", "Worker2", TeammateRole::Teammate),
        ]
    }

    fn make_manager(
        agents: &[TeamAgent],
    ) -> (TeammateManager, Arc<RecordingBroadcaster>) {
        let repo = Arc::new(MockTeamRepo::new());
        let mailbox = Arc::new(Mailbox::new(repo.clone()));
        let task_board = Arc::new(TaskBoard::new(repo));
        let broadcaster = Arc::new(RecordingBroadcaster::new());
        let mgr = TeammateManager::new(
            "t1".into(),
            agents,
            mailbox,
            task_board,
            broadcaster.clone(),
        );
        (mgr, broadcaster)
    }

    // -- Status management ---------------------------------------------------

    #[tokio::test]
    async fn initial_status_is_idle() {
        let agents = make_team_agents();
        let (mgr, _) = make_manager(&agents);

        for agent in &agents {
            let status = mgr.get_status(&agent.slot_id).await.unwrap();
            assert_eq!(status, TeammateStatus::Idle);
        }
    }

    #[tokio::test]
    async fn set_status_updates_and_broadcasts() {
        let agents = make_team_agents();
        let (mgr, bc) = make_manager(&agents);

        mgr.set_status("worker-1", TeammateStatus::Working)
            .await
            .unwrap();

        assert_eq!(
            mgr.get_status("worker-1").await.unwrap(),
            TeammateStatus::Working
        );

        let events = bc.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name, "team.agent.status");
        assert_eq!(events[0].data["slotId"], "worker-1");
        assert_eq!(events[0].data["status"], "working");
    }

    #[tokio::test]
    async fn set_status_nonexistent_agent_fails() {
        let agents = make_team_agents();
        let (mgr, _) = make_manager(&agents);

        let result = mgr.set_status("ghost", TeammateStatus::Working).await;
        assert!(matches!(result, Err(TeamError::AgentNotFound(_))));
    }

    // -- Wake / try_wake -----------------------------------------------------

    #[tokio::test]
    async fn try_wake_idle_agent_returns_payload() {
        let agents = make_team_agents();
        let (mgr, bc) = make_manager(&agents);

        let payload = mgr.try_wake("worker-1").await.unwrap();
        assert!(payload.is_some());

        let p = payload.unwrap();
        assert_eq!(p.agent.slot_id, "worker-1");
        assert!(p.tasks.is_empty());
        assert!(p.unread_messages.is_empty());

        assert_eq!(
            mgr.get_status("worker-1").await.unwrap(),
            TeammateStatus::Working
        );

        let status_events: Vec<_> = bc
            .events()
            .into_iter()
            .filter(|e| e.name == "team.agent.status")
            .collect();
        assert_eq!(status_events.len(), 1);
        assert_eq!(status_events[0].data["status"], "working");
    }

    #[tokio::test]
    async fn try_wake_non_idle_agent_returns_none() {
        let agents = make_team_agents();
        let (mgr, _) = make_manager(&agents);

        mgr.set_status("worker-1", TeammateStatus::Working)
            .await
            .unwrap();

        let payload = mgr.try_wake("worker-1").await.unwrap();
        assert!(payload.is_none());
    }

    #[tokio::test]
    async fn try_wake_nonexistent_agent_fails() {
        let agents = make_team_agents();
        let (mgr, _) = make_manager(&agents);

        let result = mgr.try_wake("ghost").await;
        assert!(matches!(result, Err(TeamError::AgentNotFound(_))));
    }

    // -- Anti-deadloop: Lead idle after turn ----------------------------------

    #[tokio::test]
    async fn lead_mark_idle_does_not_wake_self() {
        let agents = make_team_agents();
        let (mgr, _) = make_manager(&agents);

        mgr.set_status("lead-1", TeammateStatus::Working)
            .await
            .unwrap();
        let wake_target = mgr.mark_idle("lead-1").await.unwrap();
        assert!(wake_target.is_none());
    }

    // -- Anti-deadloop: All teammates idle → wake leader ---------------------

    #[tokio::test]
    async fn all_teammates_idle_signals_wake_leader() {
        let agents = make_team_agents();
        let (mgr, _) = make_manager(&agents);

        mgr.set_status("worker-1", TeammateStatus::Working)
            .await
            .unwrap();
        mgr.set_status("worker-2", TeammateStatus::Working)
            .await
            .unwrap();

        let result = mgr.mark_idle("worker-1").await.unwrap();
        assert!(result.is_none(), "not all teammates idle yet");

        let result = mgr.mark_idle("worker-2").await.unwrap();
        assert_eq!(result.as_deref(), Some("lead-1"));
    }

    #[tokio::test]
    async fn partial_teammates_idle_does_not_wake_leader() {
        let agents = make_team_agents();
        let (mgr, _) = make_manager(&agents);

        mgr.set_status("worker-1", TeammateStatus::Working)
            .await
            .unwrap();
        mgr.set_status("worker-2", TeammateStatus::Working)
            .await
            .unwrap();

        let result = mgr.mark_idle("worker-1").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn leader_not_woken_if_already_working() {
        let agents = make_team_agents();
        let (mgr, _) = make_manager(&agents);

        mgr.set_status("lead-1", TeammateStatus::Working)
            .await
            .unwrap();
        mgr.set_status("worker-1", TeammateStatus::Working)
            .await
            .unwrap();

        let result = mgr.mark_idle("worker-1").await.unwrap();
        assert!(result.is_none());
    }

    // -- Solo team (lead only, no teammates) ---------------------------------

    #[tokio::test]
    async fn solo_team_no_teammates_no_wake_signal() {
        let agents = vec![make_agent("lead-1", "Lead", TeammateRole::Lead)];
        let (mgr, _) = make_manager(&agents);

        mgr.set_status("lead-1", TeammateStatus::Working)
            .await
            .unwrap();
        let result = mgr.mark_idle("lead-1").await.unwrap();
        assert!(result.is_none());
    }

    // -- Agent lifecycle (add/remove/rename) ---------------------------------

    #[tokio::test]
    async fn add_agent_broadcasts_spawned_event() {
        let agents = make_team_agents();
        let (mgr, bc) = make_manager(&agents);

        let new_agent = make_agent("worker-3", "Worker3", TeammateRole::Teammate);
        mgr.add_agent(&new_agent).await;

        let all = mgr.list_agents().await;
        assert_eq!(all.len(), 4);

        let spawned_events: Vec<_> = bc
            .events()
            .into_iter()
            .filter(|e| e.name == "team.agent.spawned")
            .collect();
        assert_eq!(spawned_events.len(), 1);
    }

    #[tokio::test]
    async fn remove_agent_broadcasts_removed_event() {
        let agents = make_team_agents();
        let (mgr, bc) = make_manager(&agents);

        mgr.remove_agent("worker-2").await.unwrap();

        let all = mgr.list_agents().await;
        assert_eq!(all.len(), 2);

        let removed_events: Vec<_> = bc
            .events()
            .into_iter()
            .filter(|e| e.name == "team.agent.removed")
            .collect();
        assert_eq!(removed_events.len(), 1);
        assert_eq!(removed_events[0].data["slotId"], "worker-2");
    }

    #[tokio::test]
    async fn remove_nonexistent_agent_fails() {
        let agents = make_team_agents();
        let (mgr, _) = make_manager(&agents);

        let result = mgr.remove_agent("ghost").await;
        assert!(matches!(result, Err(TeamError::AgentNotFound(_))));
    }

    #[tokio::test]
    async fn rename_agent_broadcasts_renamed_event() {
        let agents = make_team_agents();
        let (mgr, bc) = make_manager(&agents);

        mgr.rename_agent("worker-1", "Renamed Worker").await.unwrap();

        let agent = mgr.get_agent("worker-1").await.unwrap();
        assert_eq!(agent.name, "Renamed Worker");

        let renamed_events: Vec<_> = bc
            .events()
            .into_iter()
            .filter(|e| e.name == "team.agent.renamed")
            .collect();
        assert_eq!(renamed_events.len(), 1);
        assert_eq!(renamed_events[0].data["name"], "Renamed Worker");
    }

    // -- execute_action: SendMessage -----------------------------------------

    #[tokio::test]
    async fn execute_send_message_writes_to_mailbox() {
        let agents = make_team_agents();
        let repo = Arc::new(MockTeamRepo::new());
        let mailbox = Arc::new(Mailbox::new(repo.clone()));
        let task_board = Arc::new(TaskBoard::new(repo));
        let broadcaster: Arc<dyn EventBroadcaster> = Arc::new(RecordingBroadcaster::new());
        let mgr = TeammateManager::new(
            "t1".into(),
            &agents,
            mailbox.clone(),
            task_board,
            broadcaster,
        );

        let action = SchedulerAction::SendMessage {
            to: "worker-1".into(),
            message: "Do task X".into(),
        };
        mgr.execute_action("lead-1", &action).await.unwrap();

        let unread = mailbox.read_unread("t1", "worker-1").await.unwrap();
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].content, "Do task X");
        assert_eq!(unread[0].from_agent_id, "lead-1");
    }

    #[tokio::test]
    async fn execute_broadcast_message_writes_to_all_others() {
        let agents = make_team_agents();
        let repo = Arc::new(MockTeamRepo::new());
        let mailbox = Arc::new(Mailbox::new(repo.clone()));
        let task_board = Arc::new(TaskBoard::new(repo));
        let broadcaster: Arc<dyn EventBroadcaster> = Arc::new(RecordingBroadcaster::new());
        let mgr = TeammateManager::new(
            "t1".into(),
            &agents,
            mailbox.clone(),
            task_board,
            broadcaster,
        );

        let action = SchedulerAction::SendMessage {
            to: "*".into(),
            message: "Attention all".into(),
        };
        mgr.execute_action("lead-1", &action).await.unwrap();

        let u1 = mailbox.read_unread("t1", "worker-1").await.unwrap();
        assert_eq!(u1.len(), 1);
        let u2 = mailbox.read_unread("t1", "worker-2").await.unwrap();
        assert_eq!(u2.len(), 1);
        let u_lead = mailbox.read_unread("t1", "lead-1").await.unwrap();
        assert!(u_lead.is_empty());
    }

    // -- execute_action: TaskCreate ------------------------------------------

    #[tokio::test]
    async fn execute_task_create() {
        let agents = make_team_agents();
        let repo = Arc::new(MockTeamRepo::new());
        let mailbox = Arc::new(Mailbox::new(repo.clone()));
        let task_board = Arc::new(TaskBoard::new(repo));
        let broadcaster: Arc<dyn EventBroadcaster> = Arc::new(RecordingBroadcaster::new());
        let mgr = TeammateManager::new(
            "t1".into(),
            &agents,
            mailbox,
            task_board.clone(),
            broadcaster,
        );

        let action = SchedulerAction::TaskCreate {
            subject: "Implement feature".into(),
            description: Some("Details here".into()),
            owner: Some("worker-1".into()),
            blocked_by: vec![],
        };
        mgr.execute_action("lead-1", &action).await.unwrap();

        let tasks = task_board.list_tasks("t1").await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].subject, "Implement feature");
        assert_eq!(tasks[0].owner.as_deref(), Some("worker-1"));
    }

    // -- execute_action: TaskUpdate ------------------------------------------

    #[tokio::test]
    async fn execute_task_update() {
        let agents = make_team_agents();
        let repo = Arc::new(MockTeamRepo::new());
        let mailbox = Arc::new(Mailbox::new(repo.clone()));
        let task_board = Arc::new(TaskBoard::new(repo));
        let broadcaster: Arc<dyn EventBroadcaster> = Arc::new(RecordingBroadcaster::new());
        let mgr = TeammateManager::new(
            "t1".into(),
            &agents,
            mailbox,
            task_board.clone(),
            broadcaster,
        );

        let task = task_board
            .create_task("t1", "Work", None, None, &[])
            .await
            .unwrap();

        let action = SchedulerAction::TaskUpdate {
            task_id: task.id.clone(),
            status: Some("in_progress".into()),
            description: None,
            owner: None,
            blocked_by: None,
        };
        mgr.execute_action("worker-1", &action).await.unwrap();

        let tasks = task_board.list_tasks("t1").await.unwrap();
        assert_eq!(tasks[0].status, crate::types::TaskStatus::InProgress);
    }

    // -- execute_action: IdleNotification ------------------------------------

    #[tokio::test]
    async fn execute_idle_notification_writes_to_lead_mailbox() {
        let agents = make_team_agents();
        let repo = Arc::new(MockTeamRepo::new());
        let mailbox = Arc::new(Mailbox::new(repo.clone()));
        let task_board = Arc::new(TaskBoard::new(repo));
        let broadcaster: Arc<dyn EventBroadcaster> = Arc::new(RecordingBroadcaster::new());
        let mgr = TeammateManager::new(
            "t1".into(),
            &agents,
            mailbox.clone(),
            task_board,
            broadcaster,
        );

        mgr.set_status("worker-1", TeammateStatus::Working)
            .await
            .unwrap();

        let action = SchedulerAction::IdleNotification {
            summary: Some("Task done".into()),
        };
        mgr.execute_action("worker-1", &action).await.unwrap();

        assert_eq!(
            mgr.get_status("worker-1").await.unwrap(),
            TeammateStatus::Idle
        );

        let lead_msgs = mailbox.read_unread("t1", "lead-1").await.unwrap();
        assert_eq!(lead_msgs.len(), 1);
        assert_eq!(lead_msgs[0].msg_type, MailboxMessageType::IdleNotification);
        assert_eq!(lead_msgs[0].from_agent_id, "worker-1");
    }

    #[tokio::test]
    async fn lead_idle_notification_does_not_write_to_self() {
        let agents = make_team_agents();
        let repo = Arc::new(MockTeamRepo::new());
        let mailbox = Arc::new(Mailbox::new(repo.clone()));
        let task_board = Arc::new(TaskBoard::new(repo));
        let broadcaster: Arc<dyn EventBroadcaster> = Arc::new(RecordingBroadcaster::new());
        let mgr = TeammateManager::new(
            "t1".into(),
            &agents,
            mailbox.clone(),
            task_board,
            broadcaster,
        );

        mgr.set_status("lead-1", TeammateStatus::Working)
            .await
            .unwrap();

        let action = SchedulerAction::IdleNotification {
            summary: Some("Done delegating".into()),
        };
        mgr.execute_action("lead-1", &action).await.unwrap();

        let lead_msgs = mailbox.read_unread("t1", "lead-1").await.unwrap();
        assert!(lead_msgs.is_empty());
    }

    // -- execute_action: ShutdownAgent ---------------------------------------

    #[tokio::test]
    async fn execute_shutdown_agent_writes_shutdown_request() {
        let agents = make_team_agents();
        let repo = Arc::new(MockTeamRepo::new());
        let mailbox = Arc::new(Mailbox::new(repo.clone()));
        let task_board = Arc::new(TaskBoard::new(repo));
        let broadcaster: Arc<dyn EventBroadcaster> = Arc::new(RecordingBroadcaster::new());
        let mgr = TeammateManager::new(
            "t1".into(),
            &agents,
            mailbox.clone(),
            task_board,
            broadcaster,
        );

        let action = SchedulerAction::ShutdownAgent {
            slot_id: "worker-1".into(),
            reason: Some("No longer needed".into()),
        };
        mgr.execute_action("lead-1", &action).await.unwrap();

        let msgs = mailbox.read_unread("t1", "worker-1").await.unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].msg_type, MailboxMessageType::ShutdownRequest);
        assert_eq!(msgs[0].content, "No longer needed");
    }

    #[tokio::test]
    async fn non_lead_cannot_shutdown_agent() {
        let agents = make_team_agents();
        let (mgr, _) = make_manager(&agents);

        let action = SchedulerAction::ShutdownAgent {
            slot_id: "worker-2".into(),
            reason: None,
        };
        let result = mgr.execute_action("worker-1", &action).await;
        assert!(matches!(result, Err(TeamError::InvalidRequest(_))));
    }

    // -- execute_action: RenameAgent -----------------------------------------

    #[tokio::test]
    async fn execute_rename_agent() {
        let agents = make_team_agents();
        let (mgr, bc) = make_manager(&agents);

        let action = SchedulerAction::RenameAgent {
            slot_id: "worker-1".into(),
            new_name: "SuperWorker".into(),
        };
        mgr.execute_action("lead-1", &action).await.unwrap();

        let agent = mgr.get_agent("worker-1").await.unwrap();
        assert_eq!(agent.name, "SuperWorker");

        let renamed: Vec<_> = bc
            .events()
            .into_iter()
            .filter(|e| e.name == "team.agent.renamed")
            .collect();
        assert_eq!(renamed.len(), 1);
    }

    // -- finalize_turn -------------------------------------------------------

    #[tokio::test]
    async fn finalize_turn_executes_actions_and_marks_idle() {
        let agents = make_team_agents();
        let repo = Arc::new(MockTeamRepo::new());
        let mailbox = Arc::new(Mailbox::new(repo.clone()));
        let task_board = Arc::new(TaskBoard::new(repo));
        let broadcaster: Arc<dyn EventBroadcaster> = Arc::new(RecordingBroadcaster::new());
        let mgr = TeammateManager::new(
            "t1".into(),
            &agents,
            mailbox.clone(),
            task_board.clone(),
            broadcaster,
        );

        mgr.set_status("worker-1", TeammateStatus::Working)
            .await
            .unwrap();
        mgr.set_status("worker-2", TeammateStatus::Working)
            .await
            .unwrap();

        let actions = vec![
            SchedulerAction::TaskCreate {
                subject: "Sub-task".into(),
                description: None,
                owner: None,
                blocked_by: vec![],
            },
            SchedulerAction::SendMessage {
                to: "lead-1".into(),
                message: "Done with sub-task".into(),
            },
        ];

        let wake_signal = mgr.finalize_turn("worker-1", &actions).await.unwrap();

        assert_eq!(
            mgr.get_status("worker-1").await.unwrap(),
            TeammateStatus::Idle
        );

        let tasks = task_board.list_tasks("t1").await.unwrap();
        assert_eq!(tasks.len(), 1);

        let lead_msgs = mailbox.read_unread("t1", "lead-1").await.unwrap();
        assert_eq!(lead_msgs.len(), 1);

        assert!(wake_signal.is_none(), "worker-2 still working");
    }

    #[tokio::test]
    async fn finalize_turn_with_idle_notification_skips_double_idle() {
        let agents = make_team_agents();
        let repo = Arc::new(MockTeamRepo::new());
        let mailbox = Arc::new(Mailbox::new(repo.clone()));
        let task_board = Arc::new(TaskBoard::new(repo));
        let broadcaster = Arc::new(RecordingBroadcaster::new());
        let mgr = TeammateManager::new(
            "t1".into(),
            &agents,
            mailbox,
            task_board,
            broadcaster.clone(),
        );

        mgr.set_status("worker-1", TeammateStatus::Working)
            .await
            .unwrap();

        let actions = vec![SchedulerAction::IdleNotification {
            summary: Some("All done".into()),
        }];

        mgr.finalize_turn("worker-1", &actions).await.unwrap();

        assert_eq!(
            mgr.get_status("worker-1").await.unwrap(),
            TeammateStatus::Idle
        );

        let idle_events: Vec<_> = broadcaster
            .events()
            .into_iter()
            .filter(|e| e.name == "team.agent.status" && e.data["status"] == "idle")
            .collect();
        assert_eq!(idle_events.len(), 1, "idle should be set exactly once");
    }

    #[tokio::test]
    async fn finalize_turn_all_teammates_done_signals_leader_wake() {
        let agents = make_team_agents();
        let repo = Arc::new(MockTeamRepo::new());
        let mailbox = Arc::new(Mailbox::new(repo.clone()));
        let task_board = Arc::new(TaskBoard::new(repo));
        let broadcaster: Arc<dyn EventBroadcaster> = Arc::new(RecordingBroadcaster::new());
        let mgr = TeammateManager::new(
            "t1".into(),
            &agents,
            mailbox,
            task_board,
            broadcaster,
        );

        mgr.set_status("worker-1", TeammateStatus::Working)
            .await
            .unwrap();
        mgr.set_status("worker-2", TeammateStatus::Working)
            .await
            .unwrap();

        mgr.finalize_turn("worker-1", &[]).await.unwrap();

        let wake_signal = mgr.finalize_turn("worker-2", &[]).await.unwrap();
        assert_eq!(wake_signal.as_deref(), Some("lead-1"));
    }

    // -- build_wake_payload with unread messages and tasks --------------------

    #[tokio::test]
    async fn wake_payload_includes_tasks_and_unread() {
        let agents = make_team_agents();
        let repo = Arc::new(MockTeamRepo::new());
        let mailbox = Arc::new(Mailbox::new(repo.clone()));
        let task_board = Arc::new(TaskBoard::new(repo));
        let broadcaster: Arc<dyn EventBroadcaster> = Arc::new(RecordingBroadcaster::new());
        let mgr = TeammateManager::new(
            "t1".into(),
            &agents,
            mailbox.clone(),
            task_board.clone(),
            broadcaster,
        );

        task_board
            .create_task("t1", "Task A", None, None, &[])
            .await
            .unwrap();

        mailbox
            .write(
                "t1",
                "worker-1",
                "lead-1",
                MailboxMessageType::Message,
                "Do task A",
                None,
            )
            .await
            .unwrap();

        let payload = mgr.build_wake_payload("worker-1").await.unwrap();
        assert_eq!(payload.tasks.len(), 1);
        assert_eq!(payload.unread_messages.len(), 1);
        assert_eq!(payload.unread_messages[0].content, "Do task A");
    }
}
