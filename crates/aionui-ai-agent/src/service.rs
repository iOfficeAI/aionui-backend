//! Business-logic layer for the ai-agent crate.
//!
//! Per `AGENTS.md` "Domain Crate Structure", this is the sole location
//! for agent-related business logic. HTTP handlers in `routes/` should
//! only extract inputs, call methods on this service, and wrap the
//! result in `ApiResponse`. Methods will be added in Stage 2b–2f.

use std::sync::Arc;

use aionui_db::IConversationRepository;

use crate::persistence::AcpSessionSyncService;
use crate::registry::AgentRegistry;
use crate::task_manager::IWorkerTaskManager;

// Fields are used by methods added in Stage 2b onwards. `dead_code` is
// temporary scaffolding tolerance — remove when the first method lands.
#[allow(dead_code)]
pub struct AgentService {
    task_manager: Arc<dyn IWorkerTaskManager>,
    registry: Arc<AgentRegistry>,
    conversation_repo: Arc<dyn IConversationRepository>,
    acp_session_sync: Arc<AcpSessionSyncService>,
}

impl AgentService {
    pub fn new(
        task_manager: Arc<dyn IWorkerTaskManager>,
        registry: Arc<AgentRegistry>,
        conversation_repo: Arc<dyn IConversationRepository>,
        acp_session_sync: Arc<AcpSessionSyncService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            task_manager,
            registry,
            conversation_repo,
            acp_session_sync,
        })
    }
}
