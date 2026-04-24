//! System services: provider management, model fetching, settings, and version checks.
pub mod client_pref;
pub mod model_fetcher;
pub mod protocol;
pub mod provider;
pub mod provider_migration;
pub mod routes;
pub mod settings;
pub mod sysinfo;
pub mod version;

pub use client_pref::ClientPrefService;
pub use model_fetcher::ModelFetchService;
pub use protocol::ProtocolDetectionService;
pub use provider::ProviderService;
pub use provider_migration::migrate_legacy_providers;
pub use routes::{SystemRouterState, settings_routes, system_routes};
pub use settings::SettingsService;
pub use version::VersionCheckService;
