//! HTTP routes for the ai-agent crate, grouped by capability.
//!
//! - [`session_ops`] — endpoints that dispatch on the concrete
//!   [`AgentInstance`](crate::agent_task::AgentInstance) variant
//!   (mode / model / config / usage / agent-capabilities /
//!   openclaw-runtime / side-question / slash-commands).
//! - [`conversation_ops`] — endpoints that do **not** need agent-type
//!   dispatch (workspace / reload-context).
//!
//! Both sub-routers share the same [`SessionRouterState`] so the caller
//! only has to construct one state object.
//!
//! Note: this replaces the previous monolithic `auxiliary_routes.rs`
//! (~500 LOC). ACP-probe routes (`/api/acp/*`) and agent-registry routes
//! (`/api/agents*`) still live in the top-level `acp_routes.rs` and
//! `agent_routes.rs`.

use std::sync::Arc;

use axum::Router;

use aionui_db::IConversationRepository;

use crate::task_manager::IWorkerTaskManager;

pub mod conversation_ops;
pub mod session_ops;

pub use conversation_ops::conversation_ops_routes;
pub use session_ops::session_ops_routes;

/// Shared router state for conversation-scoped routes.
///
/// Previously named `AuxiliaryRouterState`; renamed here because the
/// "auxiliary" bucket was just "everything else that wasn't in the two
/// existing router files" — a non-categorisation. All conversation-level
/// operations now go through this single state.
#[derive(Clone)]
pub struct SessionRouterState {
    pub worker_task_manager: Arc<dyn IWorkerTaskManager>,
    pub conversation_repo: Arc<dyn IConversationRepository>,
}

/// Build the combined session router, merging
/// [`session_ops_routes`] and [`conversation_ops_routes`].
///
/// The caller is responsible for wrapping this with the auth middleware
/// (the same treatment the old `auxiliary_routes` function received).
pub fn session_routes(state: SessionRouterState) -> Router {
    Router::new()
        .merge(session_ops_routes(state.clone()))
        .merge(conversation_ops_routes(state))
}
