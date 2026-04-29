//! Leader prompt template constant and builder.
//!
//! The template constant is provided by D5b-1 as `include_str!("prompt_templates/lead.txt")`.
//! This file hosts a stub (`""`) until D5b-1 lands; D5b-2 (this module) implements the
//! `build_lead_prompt()` builder per `docs/teams/phase1/interface-contracts.md` §5.

use std::collections::HashMap;
use std::fmt::Write;

use crate::types::TeamAgent;

/// Byte-for-byte copy of AionUi `leadPrompt.ts` template literal.
/// The `${...}` placeholders are substituted by `build_lead_prompt()`.
pub const LEAD_PROMPT_TEMPLATE: &str = include_str!("prompt_templates/lead.txt");

/// A generic agent type (CLI backend) that the leader may spawn.
/// Phase1 shape per interface-contracts §5 (line 211).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableAgentType {
    pub agent_type: String,
    pub display_name: String,
}

/// A preset assistant the leader may spawn via `custom_agent_id`.
/// Phase1 shape per interface-contracts §5 (lines 212-218).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableAssistant {
    pub custom_agent_id: String,
    pub name: String,
    pub backend: String,
    pub description: String,
    pub skills: Vec<String>,
}

/// Inputs for `build_lead_prompt`. Phase1 callers may pass empty slices/maps and `None`.
pub struct LeadPromptParams<'a> {
    pub team_name: &'a str,
    pub teammates: &'a [TeamAgent],
    pub available_agent_types: &'a [AvailableAgentType],
    pub available_assistants: &'a [AvailableAssistant],
    pub renamed_agents: &'a HashMap<String, String>,
    pub team_workspace: Option<&'a str>,
}

/// Build the leader role prompt by appending dynamic sections after the static template.
///
/// Sections per team-prompts.md §3:
/// - `## Your Teammates` — always present (empty-list copy when `teammates` is empty)
/// - `## Available Agent Types for Spawning` — only when `available_agent_types` non-empty
/// - `## Available Preset Assistants for Spawning` — only when `available_assistants` non-empty
/// - `## Team Workspace` — only when `team_workspace` is `Some`
pub fn build_lead_prompt(params: &LeadPromptParams<'_>) -> String {
    let mut out = String::with_capacity(LEAD_PROMPT_TEMPLATE.len() + 512);
    out.push_str(LEAD_PROMPT_TEMPLATE);
    if !LEAD_PROMPT_TEMPLATE.is_empty() && !LEAD_PROMPT_TEMPLATE.ends_with("\n\n") {
        out.push_str("\n\n");
    }

    let _ = writeln!(out, "## Your Teammates (team: {})", params.team_name);
    if params.teammates.is_empty() {
        out.push_str(
            "No teammates yet. Present a spawn proposal to the user and wait for confirmation \
             before calling team_spawn_agent.\n\n",
        );
    } else {
        for m in params.teammates {
            let display_name = params
                .renamed_agents
                .get(&m.slot_id)
                .map(String::as_str)
                .unwrap_or(m.name.as_str());
            let status = m
                .status
                .map(|s| s.to_string())
                .unwrap_or_else(|| "unknown".to_owned());
            let _ = writeln!(
                out,
                "- {} (slot={}, agentType={}, status={})",
                display_name, m.slot_id, m.backend, status,
            );
        }
        out.push('\n');
    }

    if !params.available_agent_types.is_empty() {
        out.push_str("## Available Agent Types for Spawning\n");
        for t in params.available_agent_types {
            let _ = writeln!(out, "- {} — {}", t.agent_type, t.display_name);
        }
        out.push('\n');
    }

    if !params.available_assistants.is_empty() {
        out.push_str("## Available Preset Assistants for Spawning\n");
        for a in params.available_assistants {
            let skills = if a.skills.is_empty() {
                String::new()
            } else {
                format!(" [skills: {}]", a.skills.join(", "))
            };
            let _ = writeln!(
                out,
                "- {} ({}) — {} — {}{}",
                a.custom_agent_id, a.backend, a.name, a.description, skills,
            );
        }
        out.push('\n');
    }

    if let Some(ws) = params.team_workspace {
        let _ = writeln!(out, "## Team Workspace\n{}\n", ws);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{TeamAgent, TeammateRole};

    fn params_min<'a>(renamed: &'a HashMap<String, String>) -> LeadPromptParams<'a> {
        LeadPromptParams {
            team_name: "Alpha",
            teammates: &[],
            available_agent_types: &[],
            available_assistants: &[],
            renamed_agents: renamed,
            team_workspace: None,
        }
    }

    #[test]
    fn snapshot_minimal_params() {
        let renamed = HashMap::new();
        let out = build_lead_prompt(&params_min(&renamed));

        assert!(out.contains("## Your Teammates (team: Alpha)"));
        assert!(out.contains("No teammates yet"));
        assert!(!out.contains("## Available Agent Types for Spawning"));
        assert!(!out.contains("## Available Preset Assistants for Spawning"));
        assert!(!out.contains("## Team Workspace"));
    }

    #[test]
    fn snapshot_with_available_agent_types() {
        let renamed = HashMap::new();
        let teammate = TeamAgent {
            slot_id: "w1".into(),
            name: "Worker1".into(),
            role: TeammateRole::Teammate,
            conversation_id: "conv-w1".into(),
            backend: "claude".into(),
            model: "sonnet".into(),
            custom_agent_id: None,
            status: None,
            conversation_type: None,
            cli_path: None,
        };
        let agent_types = vec![
            AvailableAgentType {
                agent_type: "claude".into(),
                display_name: "general-purpose AI assistant".into(),
            },
            AvailableAgentType {
                agent_type: "codex".into(),
                display_name: "code generation specialist".into(),
            },
        ];
        let params = LeadPromptParams {
            team_name: "Beta",
            teammates: std::slice::from_ref(&teammate),
            available_agent_types: &agent_types,
            available_assistants: &[],
            renamed_agents: &renamed,
            team_workspace: Some("/tmp/team-ws"),
        };

        let out = build_lead_prompt(&params);

        assert!(out.contains("## Your Teammates (team: Beta)"));
        assert!(out.contains("Worker1 (slot=w1, agentType=claude, status=unknown)"));
        assert!(out.contains("## Available Agent Types for Spawning"));
        assert!(out.contains("- claude — general-purpose AI assistant"));
        assert!(out.contains("- codex — code generation specialist"));
        assert!(out.contains("## Team Workspace\n/tmp/team-ws"));
        assert!(!out.contains("## Available Preset Assistants for Spawning"));
    }
}
