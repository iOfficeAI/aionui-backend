mod database;
mod error;
pub mod models;
mod repository;

pub use database::{init_database, init_database_memory, Database};
pub use error::DbError;
pub use repository::{
    IClientPreferenceRepository, IConversationRepository, IMcpServerRepository,
    IOAuthTokenRepository, IProviderRepository, IRemoteAgentRepository, ISettingsRepository,
    IUserRepository, SqliteClientPreferenceRepository, SqliteConversationRepository,
    SqliteMcpServerRepository, SqliteOAuthTokenRepository, SqliteProviderRepository,
    SqliteRemoteAgentRepository, SqliteSettingsRepository, SqliteUserRepository,
};
pub use repository::conversation::{
    ConversationFilters, ConversationRowUpdate, MessageRowUpdate, MessageSearchRow, SortOrder,
};
pub use repository::mcp_server::{CreateMcpServerParams, UpdateMcpServerParams};
pub use repository::oauth_token::UpsertOAuthTokenParams;
pub use repository::provider::{CreateProviderParams, UpdateProviderParams};
pub use repository::remote_agent::{CreateRemoteAgentParams, UpdateRemoteAgentParams};

// Re-export sqlx pool type for downstream crates
pub use sqlx::SqlitePool;
