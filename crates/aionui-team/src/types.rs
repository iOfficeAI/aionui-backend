use std::fmt;

use aionui_api_types::TeamAgentResponse;
use aionui_common::TimestampMs;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// TeammateRole
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TeammateRole {
    Lead,
    Teammate,
}

impl fmt::Display for TeammateRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lead => write!(f, "lead"),
            Self::Teammate => write!(f, "teammate"),
        }
    }
}

impl TeammateRole {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "lead" => Some(Self::Lead),
            "teammate" => Some(Self::Teammate),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// TeammateStatus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeammateStatus {
    Idle,
    Working,
    Thinking,
    ToolUse,
    Error,
}

impl fmt::Display for TeammateStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Working => write!(f, "working"),
            Self::Thinking => write!(f, "thinking"),
            Self::ToolUse => write!(f, "tool_use"),
            Self::Error => write!(f, "error"),
        }
    }
}

impl TeammateStatus {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "idle" => Some(Self::Idle),
            "working" => Some(Self::Working),
            "thinking" => Some(Self::Thinking),
            "tool_use" => Some(Self::ToolUse),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// TeamAgent
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamAgent {
    pub slot_id: String,
    pub name: String,
    pub role: TeammateRole,
    pub conversation_id: String,
    pub backend: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<TeammateStatus>,
}

impl TeamAgent {
    pub fn to_response(&self) -> TeamAgentResponse {
        TeamAgentResponse {
            slot_id: self.slot_id.clone(),
            name: self.name.clone(),
            role: self.role.to_string(),
            conversation_id: self.conversation_id.clone(),
            backend: self.backend.clone(),
            model: self.model.clone(),
            custom_agent_id: self.custom_agent_id.clone(),
            status: self.status.map(|s| s.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Team
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    pub id: String,
    pub name: String,
    pub agents: Vec<TeamAgent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lead_agent_id: Option<String>,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

// ---------------------------------------------------------------------------
// MailboxMessageType
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailboxMessageType {
    Message,
    IdleNotification,
    ShutdownRequest,
}

impl fmt::Display for MailboxMessageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Message => write!(f, "message"),
            Self::IdleNotification => write!(f, "idle_notification"),
            Self::ShutdownRequest => write!(f, "shutdown_request"),
        }
    }
}

impl MailboxMessageType {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "message" => Some(Self::Message),
            "idle_notification" => Some(Self::IdleNotification),
            "shutdown_request" => Some(Self::ShutdownRequest),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// MailboxMessage
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxMessage {
    pub id: String,
    pub team_id: String,
    pub to_agent_id: String,
    pub from_agent_id: String,
    #[serde(rename = "type")]
    pub msg_type: MailboxMessageType,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub read: bool,
    pub created_at: TimestampMs,
}

// ---------------------------------------------------------------------------
// TaskStatus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Deleted,
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Completed => write!(f, "completed"),
            Self::Deleted => write!(f, "deleted"),
        }
    }
}

impl TaskStatus {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "in_progress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            "deleted" => Some(Self::Deleted),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// TeamTask
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamTask {
    pub id: String,
    pub team_id: String,
    pub subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub blocked_by: Vec<String>,
    pub blocks: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

// ---------------------------------------------------------------------------
// Conversion helpers: DB rows ↔ domain types
// ---------------------------------------------------------------------------

use aionui_db::models::{MailboxMessageRow, TeamRow, TeamTaskRow};

impl Team {
    pub fn from_row(row: &TeamRow) -> Result<Self, serde_json::Error> {
        let agents: Vec<TeamAgent> = serde_json::from_str(&row.agents)?;
        Ok(Self {
            id: row.id.clone(),
            name: row.name.clone(),
            agents,
            lead_agent_id: row.lead_agent_id.clone(),
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

impl MailboxMessage {
    pub fn from_row(row: &MailboxMessageRow) -> Option<Self> {
        let msg_type = MailboxMessageType::parse(&row.msg_type)?;
        Some(Self {
            id: row.id.clone(),
            team_id: row.team_id.clone(),
            to_agent_id: row.to_agent_id.clone(),
            from_agent_id: row.from_agent_id.clone(),
            msg_type,
            content: row.content.clone(),
            summary: row.summary.clone(),
            read: row.read,
            created_at: row.created_at,
        })
    }
}

impl TeamTask {
    pub fn from_row(row: &TeamTaskRow) -> Result<Self, serde_json::Error> {
        let status = TaskStatus::parse(&row.status).unwrap_or(TaskStatus::Pending);
        let blocked_by: Vec<String> = serde_json::from_str(&row.blocked_by)?;
        let blocks: Vec<String> = serde_json::from_str(&row.blocks)?;
        let metadata: Option<serde_json::Value> = row
            .metadata
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?;
        Ok(Self {
            id: row.id.clone(),
            team_id: row.team_id.clone(),
            subject: row.subject.clone(),
            description: row.description.clone(),
            status,
            owner: row.owner.clone(),
            blocked_by,
            blocks,
            metadata,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- TeammateRole ---------------------------------------------------------

    #[test]
    fn teammate_role_display() {
        assert_eq!(TeammateRole::Lead.to_string(), "lead");
        assert_eq!(TeammateRole::Teammate.to_string(), "teammate");
    }

    #[test]
    fn teammate_role_parse() {
        assert_eq!(TeammateRole::parse("lead"), Some(TeammateRole::Lead));
        assert_eq!(
            TeammateRole::parse("teammate"),
            Some(TeammateRole::Teammate)
        );
        assert_eq!(TeammateRole::parse("unknown"), None);
    }

    #[test]
    fn teammate_role_serde_roundtrip() {
        let role = TeammateRole::Lead;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, r#""lead""#);
        let parsed: TeammateRole = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, role);
    }

    // -- TeammateStatus -------------------------------------------------------

    #[test]
    fn teammate_status_display() {
        assert_eq!(TeammateStatus::Idle.to_string(), "idle");
        assert_eq!(TeammateStatus::Working.to_string(), "working");
        assert_eq!(TeammateStatus::Thinking.to_string(), "thinking");
        assert_eq!(TeammateStatus::ToolUse.to_string(), "tool_use");
        assert_eq!(TeammateStatus::Error.to_string(), "error");
    }

    #[test]
    fn teammate_status_parse_all_variants() {
        assert_eq!(TeammateStatus::parse("idle"), Some(TeammateStatus::Idle));
        assert_eq!(
            TeammateStatus::parse("working"),
            Some(TeammateStatus::Working)
        );
        assert_eq!(
            TeammateStatus::parse("thinking"),
            Some(TeammateStatus::Thinking)
        );
        assert_eq!(
            TeammateStatus::parse("tool_use"),
            Some(TeammateStatus::ToolUse)
        );
        assert_eq!(TeammateStatus::parse("error"), Some(TeammateStatus::Error));
        assert_eq!(TeammateStatus::parse("bad"), None);
    }

    #[test]
    fn teammate_status_serde_roundtrip() {
        for status in [
            TeammateStatus::Idle,
            TeammateStatus::Working,
            TeammateStatus::Thinking,
            TeammateStatus::ToolUse,
            TeammateStatus::Error,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: TeammateStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, status);
        }
    }

    // -- MailboxMessageType ---------------------------------------------------

    #[test]
    fn mailbox_message_type_display() {
        assert_eq!(MailboxMessageType::Message.to_string(), "message");
        assert_eq!(
            MailboxMessageType::IdleNotification.to_string(),
            "idle_notification"
        );
        assert_eq!(
            MailboxMessageType::ShutdownRequest.to_string(),
            "shutdown_request"
        );
    }

    #[test]
    fn mailbox_message_type_parse() {
        assert_eq!(
            MailboxMessageType::parse("message"),
            Some(MailboxMessageType::Message)
        );
        assert_eq!(
            MailboxMessageType::parse("idle_notification"),
            Some(MailboxMessageType::IdleNotification)
        );
        assert_eq!(
            MailboxMessageType::parse("shutdown_request"),
            Some(MailboxMessageType::ShutdownRequest)
        );
        assert_eq!(MailboxMessageType::parse("unknown"), None);
    }

    #[test]
    fn mailbox_message_type_serde_roundtrip() {
        for mt in [
            MailboxMessageType::Message,
            MailboxMessageType::IdleNotification,
            MailboxMessageType::ShutdownRequest,
        ] {
            let json = serde_json::to_string(&mt).unwrap();
            let parsed: MailboxMessageType = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, mt);
        }
    }

    // -- TaskStatus -----------------------------------------------------------

    #[test]
    fn task_status_display() {
        assert_eq!(TaskStatus::Pending.to_string(), "pending");
        assert_eq!(TaskStatus::InProgress.to_string(), "in_progress");
        assert_eq!(TaskStatus::Completed.to_string(), "completed");
        assert_eq!(TaskStatus::Deleted.to_string(), "deleted");
    }

    #[test]
    fn task_status_parse_all_variants() {
        assert_eq!(TaskStatus::parse("pending"), Some(TaskStatus::Pending));
        assert_eq!(
            TaskStatus::parse("in_progress"),
            Some(TaskStatus::InProgress)
        );
        assert_eq!(
            TaskStatus::parse("completed"),
            Some(TaskStatus::Completed)
        );
        assert_eq!(TaskStatus::parse("deleted"), Some(TaskStatus::Deleted));
        assert_eq!(TaskStatus::parse("bad"), None);
    }

    #[test]
    fn task_status_serde_roundtrip() {
        for status in [
            TaskStatus::Pending,
            TaskStatus::InProgress,
            TaskStatus::Completed,
            TaskStatus::Deleted,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: TaskStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, status);
        }
    }

    // -- TeamAgent conversion -------------------------------------------------

    #[test]
    fn team_agent_to_response() {
        let agent = TeamAgent {
            slot_id: "s1".into(),
            name: "Lead".into(),
            role: TeammateRole::Lead,
            conversation_id: "c1".into(),
            backend: "acp".into(),
            model: "claude".into(),
            custom_agent_id: Some("custom-1".into()),
            status: Some(TeammateStatus::Working),
        };
        let resp = agent.to_response();
        assert_eq!(resp.slot_id, "s1");
        assert_eq!(resp.role, "lead");
        assert_eq!(resp.status.as_deref(), Some("working"));
        assert_eq!(resp.custom_agent_id.as_deref(), Some("custom-1"));
    }

    #[test]
    fn team_agent_serde_roundtrip() {
        let agent = TeamAgent {
            slot_id: "s1".into(),
            name: "Worker".into(),
            role: TeammateRole::Teammate,
            conversation_id: "c1".into(),
            backend: "acp".into(),
            model: "claude".into(),
            custom_agent_id: None,
            status: None,
        };
        let json = serde_json::to_string(&agent).unwrap();
        let parsed: TeamAgent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, agent);
    }

    #[test]
    fn team_agent_camel_case_serialization() {
        let agent = TeamAgent {
            slot_id: "s1".into(),
            name: "A".into(),
            role: TeammateRole::Lead,
            conversation_id: "c1".into(),
            backend: "acp".into(),
            model: "claude".into(),
            custom_agent_id: Some("x".into()),
            status: Some(TeammateStatus::Idle),
        };
        let val = serde_json::to_value(&agent).unwrap();
        assert!(val.get("slotId").is_some());
        assert!(val.get("conversationId").is_some());
        assert!(val.get("customAgentId").is_some());
    }

    // -- Team from_row --------------------------------------------------------

    #[test]
    fn team_from_row_success() {
        let agents_json = serde_json::to_string(&vec![TeamAgent {
            slot_id: "s1".into(),
            name: "Lead".into(),
            role: TeammateRole::Lead,
            conversation_id: "c1".into(),
            backend: "acp".into(),
            model: "claude".into(),
            custom_agent_id: None,
            status: None,
        }])
        .unwrap();
        let row = TeamRow {
            id: "t1".into(),
            name: "Alpha".into(),
            agents: agents_json,
            lead_agent_id: Some("s1".into()),
            created_at: 1000,
            updated_at: 2000,
        };
        let team = Team::from_row(&row).unwrap();
        assert_eq!(team.id, "t1");
        assert_eq!(team.agents.len(), 1);
        assert_eq!(team.agents[0].slot_id, "s1");
        assert_eq!(team.lead_agent_id.as_deref(), Some("s1"));
    }

    #[test]
    fn team_from_row_invalid_json() {
        let row = TeamRow {
            id: "t1".into(),
            name: "Bad".into(),
            agents: "not-json".into(),
            lead_agent_id: None,
            created_at: 0,
            updated_at: 0,
        };
        assert!(Team::from_row(&row).is_err());
    }

    // -- MailboxMessage from_row ----------------------------------------------

    #[test]
    fn mailbox_message_from_row_success() {
        let row = MailboxMessageRow {
            id: "m1".into(),
            team_id: "t1".into(),
            to_agent_id: "a1".into(),
            from_agent_id: "a2".into(),
            msg_type: "message".into(),
            content: "hello".into(),
            summary: None,
            read: false,
            created_at: 1000,
        };
        let msg = MailboxMessage::from_row(&row).unwrap();
        assert_eq!(msg.msg_type, MailboxMessageType::Message);
        assert!(!msg.read);
    }

    #[test]
    fn mailbox_message_from_row_idle_notification() {
        let row = MailboxMessageRow {
            id: "m2".into(),
            team_id: "t1".into(),
            to_agent_id: "lead".into(),
            from_agent_id: "a1".into(),
            msg_type: "idle_notification".into(),
            content: "done".into(),
            summary: Some("Finished task".into()),
            read: false,
            created_at: 2000,
        };
        let msg = MailboxMessage::from_row(&row).unwrap();
        assert_eq!(msg.msg_type, MailboxMessageType::IdleNotification);
        assert_eq!(msg.summary.as_deref(), Some("Finished task"));
    }

    #[test]
    fn mailbox_message_from_row_unknown_type() {
        let row = MailboxMessageRow {
            id: "m3".into(),
            team_id: "t1".into(),
            to_agent_id: "a1".into(),
            from_agent_id: "a2".into(),
            msg_type: "unknown_type".into(),
            content: "x".into(),
            summary: None,
            read: false,
            created_at: 0,
        };
        assert!(MailboxMessage::from_row(&row).is_none());
    }

    #[test]
    fn mailbox_message_serializes_type_field() {
        let msg = MailboxMessage {
            id: "m1".into(),
            team_id: "t1".into(),
            to_agent_id: "a1".into(),
            from_agent_id: "a2".into(),
            msg_type: MailboxMessageType::Message,
            content: "hello".into(),
            summary: None,
            read: false,
            created_at: 1000,
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert!(json.get("type").is_some(), "field must serialize as 'type'");
        assert!(json.get("msgType").is_none(), "must not serialize as 'msgType'");
        assert_eq!(json["type"], "message");
    }

    // -- TeamTask from_row ----------------------------------------------------

    #[test]
    fn team_task_from_row_success() {
        let row = TeamTaskRow {
            id: "tk1".into(),
            team_id: "t1".into(),
            subject: "Implement".into(),
            description: Some("Details".into()),
            status: "in_progress".into(),
            owner: Some("a1".into()),
            blocked_by: r#"["tk0"]"#.into(),
            blocks: r#"["tk2"]"#.into(),
            metadata: Some(r#"{"priority":"high"}"#.into()),
            created_at: 1000,
            updated_at: 2000,
        };
        let task = TeamTask::from_row(&row).unwrap();
        assert_eq!(task.status, TaskStatus::InProgress);
        assert_eq!(task.blocked_by, vec!["tk0"]);
        assert_eq!(task.blocks, vec!["tk2"]);
        assert!(task.metadata.is_some());
    }

    #[test]
    fn team_task_from_row_empty_deps() {
        let row = TeamTaskRow {
            id: "tk1".into(),
            team_id: "t1".into(),
            subject: "Simple".into(),
            description: None,
            status: "pending".into(),
            owner: None,
            blocked_by: "[]".into(),
            blocks: "[]".into(),
            metadata: None,
            created_at: 0,
            updated_at: 0,
        };
        let task = TeamTask::from_row(&row).unwrap();
        assert_eq!(task.status, TaskStatus::Pending);
        assert!(task.blocked_by.is_empty());
        assert!(task.blocks.is_empty());
        assert!(task.metadata.is_none());
    }

    #[test]
    fn team_task_from_row_unknown_status_defaults_to_pending() {
        let row = TeamTaskRow {
            id: "tk1".into(),
            team_id: "t1".into(),
            subject: "S".into(),
            description: None,
            status: "unknown".into(),
            owner: None,
            blocked_by: "[]".into(),
            blocks: "[]".into(),
            metadata: None,
            created_at: 0,
            updated_at: 0,
        };
        let task = TeamTask::from_row(&row).unwrap();
        assert_eq!(task.status, TaskStatus::Pending);
    }

    #[test]
    fn team_task_from_row_invalid_blocked_by_json() {
        let row = TeamTaskRow {
            id: "tk1".into(),
            team_id: "t1".into(),
            subject: "S".into(),
            description: None,
            status: "pending".into(),
            owner: None,
            blocked_by: "not-json".into(),
            blocks: "[]".into(),
            metadata: None,
            created_at: 0,
            updated_at: 0,
        };
        assert!(TeamTask::from_row(&row).is_err());
    }
}
