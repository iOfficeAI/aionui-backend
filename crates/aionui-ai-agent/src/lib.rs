//! AI agent lifecycle, worker task dispatch, and skill management.
pub mod agent_task;
pub mod capability;
pub mod factory;
pub mod idle_scanner;
pub mod manager;
pub mod persistence;
pub mod protocol;
pub mod registry;
pub mod routes;
pub mod shared_kernel;
pub mod task_manager;
pub mod types;

#[cfg(any(test, feature = "test-support"))]
pub use agent_task::IMockAgent;
pub use agent_task::{AgentInstance, IAgentTask};
pub use aionui_api_types::{
    AcpBuildExtra, AcpModelInfo, AcpSessionConfigOption, AionrsBuildExtra, OpenClawBuildExtra, OpenClawGatewayConfig,
    RemoteBuildExtra, SlashCommandItem,
};
pub use capability::skill_manager::AcpSkillManager;
pub use factory::{AgentFactoryDeps, build_agent_factory};
pub use idle_scanner::start_idle_scanner;
pub use manager::remote::{RemoteAgentRouterState, RemoteAgentService, remote_agent_routes};
pub use persistence::AcpSessionSyncService;
pub use protocol::events::AgentStreamEvent;
pub use registry::AgentRegistry;
pub use routes::{AcpRouterState, AgentRouterState, SessionRouterState, acp_routes, agent_routes, session_routes};
pub use task_manager::{IWorkerTaskManager, WorkerTaskManagerImpl};
