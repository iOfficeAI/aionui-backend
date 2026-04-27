use std::sync::Arc;

use aionui_ai_agent::{
    AcpRouterState, AgentRouterState, AuxiliaryRouterState, ConnectionTestRouterState,
    ConnectionTestService, RemoteAgentRouterState, RemoteAgentService,
};
use aionui_api_types::{AgentSource, DetectedAgent};
use aionui_assistant::{AssistantRouterState, AssistantService, BuiltinAssistantRegistry};
use aionui_auth::extract_token_from_ws_headers;
use aionui_channel::ChannelRouterState;
use aionui_common::{AgentType, EnvVar};
use aionui_conversation::{ConversationRouterState, ConversationService};
use aionui_cron::{CronEventEmitter, CronRouterState};
use aionui_db::{
    IAssistantOverrideRepository, IAssistantRepository, SqliteAssistantOverrideRepository,
    SqliteAssistantRepository, SqliteClientPreferenceRepository, SqliteConversationRepository,
    SqliteProviderRepository, SqliteRemoteAgentRepository, SqliteSettingsRepository,
};
use aionui_extension::{
    AssistantRuleDispatcher, ExtensionRegistry, ExtensionRouterState, ExtensionStateStore,
    ExternalPathsManager, HubIndexManager, HubInstaller, HubRouterState, SkillRouterState,
};
use aionui_file::{FileRouterState, FileService, FileWatchService, SnapshotService};
use aionui_mcp::{
    AionrsAdapter, AionuiAdapter, ClaudeAdapter, CodeBuddyAdapter, CodexAdapter, GeminiAdapter,
    McpAgentAdapter, McpConfigService, McpConnectionTestService, McpRouterState, McpSyncService,
    OpencodeAdapter, QwenAdapter,
};
use aionui_office::{
    ConversionService, OfficeRouterState, OfficecliWatchManager, ProxyService,
    SnapshotService as OfficeSnapshotService, StarOfficeDetector,
};
use aionui_realtime::{NoopMessageRouter, WsHandlerState};
use aionui_shell::ShellRouterState;
use aionui_system::{
    ClientPrefService, ModelFetchService, ProtocolDetectionService, ProviderService,
    SettingsService, SystemRouterState, VersionCheckService,
};
use aionui_team::{TeamRouterState, TeamSessionService};

use crate::{AppServices, ModuleStates, derive_encryption_key};

/// Convert extension-contributed ACP adapters into `DetectedAgent` values.
pub(crate) async fn resolve_extension_agents(registry: &ExtensionRegistry) -> Vec<DetectedAgent> {
    registry
        .get_acp_adapters()
        .await
        .into_iter()
        .filter(|a| {
            a.connection_type
                .as_deref()
                .is_none_or(|ct| ct == "cli" || ct == "stdio")
        })
        .map(|a| DetectedAgent {
            id: a.id,
            name: a.name,
            agent_type: AgentType::Acp,
            backend: None,
            available: true,
            source: AgentSource::Extension,
            command: a.default_cli_path.or(a.cli_command),
            args: a.acp_args,
            env: a
                .env
                .into_iter()
                .map(|(k, v)| EnvVar { name: k, value: v })
                .collect(),
        })
        .collect()
}

/// Build all default `ModuleStates` from application services.
pub async fn build_module_states(services: &AppServices) -> ModuleStates {
    let (ext_state, hub_state, mut skill_state) = build_extension_states(services).await;
    let assistant = build_assistant_state(services, ext_state.registry.clone());

    let extensions = resolve_extension_agents(&ext_state.registry).await;
    services.agent_registry.initialize(extensions, vec![]).await;

    let dispatcher: Arc<dyn AssistantRuleDispatcher> = assistant.service.clone();
    skill_state.assistant_dispatcher = Some(dispatcher);

    run_orphan_agent_skills_cleanup(services, &skill_state.skill_paths).await;

    ModuleStates {
        system: build_system_state(services),
        conversation: build_conversation_state(services),
        remote_agent: build_remote_agent_state(services),
        acp: build_acp_state(services),
        connection_test: build_connection_test_state(),
        auxiliary: build_auxiliary_state(services),
        file: build_file_state(services),
        mcp: build_mcp_state(services),
        extension: ext_state,
        hub: hub_state,
        skill: skill_state,
        channel: build_channel_state(services),
        team: build_team_state(services),
        cron: build_cron_state(services),
        office: build_office_state(services),
        shell: build_shell_state(services),
        assistant,
        agent: AgentRouterState {
            agent_registry: services.agent_registry.clone(),
        },
    }
}

/// Best-effort orphan sweep of `{data_dir}/agent-skills/`.
async fn run_orphan_agent_skills_cleanup(
    services: &AppServices,
    skill_paths: &aionui_extension::SkillPaths,
) {
    let pool = services.database.pool().clone();
    let repo: Arc<dyn aionui_db::IConversationRepository> =
        Arc::new(SqliteConversationRepository::new(pool));
    let handle = tokio::runtime::Handle::current();
    let is_live = {
        let repo = repo.clone();
        move |id: &str| -> bool {
            let id = id.to_string();
            let repo = repo.clone();
            tokio::task::block_in_place(|| {
                handle.block_on(async move { repo.get(&id).await.ok().flatten().is_some() })
            })
        }
    };
    match aionui_extension::cleanup_orphan_agent_skills(skill_paths, is_live).await {
        Ok(removed) if removed > 0 => {
            tracing::info!(removed, "swept orphan agent-skills dirs on startup")
        }
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "orphan agent-skills sweep failed (non-fatal)"),
    }
}

/// Build the default `AssistantRouterState` from application services.
pub fn build_assistant_state(
    services: &AppServices,
    extension_registry: ExtensionRegistry,
) -> AssistantRouterState {
    let pool = services.database.pool().clone();
    let repo: Arc<dyn IAssistantRepository> =
        Arc::new(SqliteAssistantRepository::new(pool.clone()));
    let override_repo: Arc<dyn IAssistantOverrideRepository> =
        Arc::new(SqliteAssistantOverrideRepository::new(pool));
    let builtin = Arc::new(BuiltinAssistantRegistry::load());
    let service = Arc::new(AssistantService::new(
        repo,
        override_repo,
        builtin,
        extension_registry,
    ));
    AssistantRouterState { service }
}

/// Build the default `SystemRouterState` from application services.
pub fn build_system_state(services: &AppServices) -> SystemRouterState {
    let encryption_key = derive_encryption_key(&services.jwt_secret_raw);
    let pool = services.database.pool().clone();
    let provider_repo = Arc::new(SqliteProviderRepository::new(pool.clone()));
    let http_client = reqwest::Client::new();

    SystemRouterState {
        settings_service: SettingsService::new(Arc::new(SqliteSettingsRepository::new(
            pool.clone(),
        ))),
        client_pref_service: ClientPrefService::new(Arc::new(
            SqliteClientPreferenceRepository::new(pool),
        )),
        provider_service: ProviderService::new(provider_repo.clone(), encryption_key),
        model_fetch_service: ModelFetchService::new(
            provider_repo,
            encryption_key,
            http_client.clone(),
        ),
        protocol_detection_service: ProtocolDetectionService::new(http_client.clone()),
        version_check_service: VersionCheckService::new(
            http_client,
            env!("CARGO_PKG_VERSION").to_owned(),
        ),
    }
}

/// Build the default `ConversationRouterState` from application services.
pub fn build_conversation_state(services: &AppServices) -> ConversationRouterState {
    let pool = services.database.pool().clone();
    let repo = Arc::new(SqliteConversationRepository::new(pool));
    ConversationRouterState {
        conversation_service: ConversationService::new_with_workspace_root(
            repo,
            services.event_bus.clone(),
            std::path::PathBuf::from(&services.data_dir),
        ),
        worker_task_manager: services.worker_task_manager.clone(),
    }
}

/// Build the default `RemoteAgentRouterState` from application services.
pub fn build_remote_agent_state(services: &AppServices) -> RemoteAgentRouterState {
    let encryption_key = derive_encryption_key(&services.jwt_secret_raw);
    let pool = services.database.pool().clone();
    let repo = Arc::new(SqliteRemoteAgentRepository::new(pool));
    RemoteAgentRouterState {
        service: RemoteAgentService::new(repo, encryption_key),
    }
}

/// Build the default `AcpRouterState` from application services.
pub fn build_acp_state(services: &AppServices) -> AcpRouterState {
    AcpRouterState {
        worker_task_manager: services.worker_task_manager.clone(),
    }
}

/// Build the default `ConnectionTestRouterState`.
pub fn build_connection_test_state() -> ConnectionTestRouterState {
    ConnectionTestRouterState {
        service: ConnectionTestService::new(reqwest::Client::new()),
    }
}

/// Build the default `AuxiliaryRouterState` from application services.
pub fn build_auxiliary_state(services: &AppServices) -> AuxiliaryRouterState {
    let pool = services.database.pool().clone();
    let conversation_repo = Arc::new(SqliteConversationRepository::new(pool));
    AuxiliaryRouterState {
        worker_task_manager: services.worker_task_manager.clone(),
        conversation_repo,
    }
}

/// Build the default `FileRouterState` from application services.
pub fn build_file_state(services: &AppServices) -> FileRouterState {
    let broadcaster = services.event_bus.clone();
    let allowed_roots = vec![
        std::env::temp_dir(),
        dirs::home_dir().unwrap_or_else(std::env::temp_dir),
    ];
    let file_service = Arc::new(FileService::new(broadcaster.clone(), allowed_roots));
    let watch_service =
        Arc::new(FileWatchService::new(broadcaster).expect("file watch service initialization"));
    let snapshot_service = Arc::new(SnapshotService::new());
    FileRouterState {
        file_service,
        watch_service,
        snapshot_service,
    }
}

/// Build the default `McpRouterState` from application services.
pub fn build_mcp_state(services: &AppServices) -> McpRouterState {
    let pool = services.database.pool().clone();
    let repo: Arc<dyn aionui_db::IMcpServerRepository> =
        Arc::new(aionui_db::SqliteMcpServerRepository::new(pool));

    let adapters: Vec<Arc<dyn McpAgentAdapter>> = vec![
        Arc::new(ClaudeAdapter),
        Arc::new(GeminiAdapter),
        Arc::new(QwenAdapter),
        Arc::new(CodexAdapter),
        Arc::new(CodeBuddyAdapter),
        Arc::new(OpencodeAdapter),
        Arc::new(AionrsAdapter),
        Arc::new(AionuiAdapter::new(repo.clone())),
    ];

    let oauth_token_repo: Arc<dyn aionui_db::IOAuthTokenRepository> = Arc::new(
        aionui_db::SqliteOAuthTokenRepository::new(services.database.pool().clone()),
    );
    let http_client = reqwest::Client::new();

    McpRouterState {
        config_service: McpConfigService::new(repo.clone()),
        sync_service: McpSyncService::new(repo, adapters),
        connection_test_service: McpConnectionTestService::new(http_client.clone()),
        oauth_service: aionui_mcp::McpOAuthService::new(oauth_token_repo, http_client),
    }
}

/// Build the default `ChannelRouterState` from application services.
pub fn build_channel_state(services: &AppServices) -> ChannelRouterState {
    let pool = services.database.pool().clone();
    let repo: Arc<dyn aionui_db::IChannelRepository> =
        Arc::new(aionui_db::SqliteChannelRepository::new(pool));
    let encryption_key = derive_encryption_key(&services.jwt_secret_raw);

    let (message_tx, _message_rx) = tokio::sync::mpsc::channel(256);
    let (confirm_tx, _confirm_rx) = tokio::sync::mpsc::channel(256);

    let manager = Arc::new(aionui_channel::manager::ChannelManager::new(
        repo.clone(),
        services.event_bus.clone(),
        encryption_key,
        message_tx,
        confirm_tx,
    ));

    let pairing_service = Arc::new(aionui_channel::pairing::PairingService::new(
        repo.clone(),
        services.event_bus.clone(),
    ));

    let session_manager = Arc::new(aionui_channel::session::SessionManager::new(repo.clone()));

    let plugin_factory: Arc<aionui_channel::manager::PluginFactory> =
        Arc::new(Box::new(aionui_channel::plugins::create_plugin));

    ChannelRouterState {
        manager,
        pairing_service,
        session_manager,
        repo,
        plugin_factory,
    }
}

/// Build the default `TeamRouterState` from application services.
pub fn build_team_state(services: &AppServices) -> TeamRouterState {
    let pool = services.database.pool().clone();
    let team_repo: Arc<dyn aionui_db::ITeamRepository> =
        Arc::new(aionui_db::SqliteTeamRepository::new(pool.clone()));
    let conv_repo: Arc<dyn aionui_db::IConversationRepository> =
        Arc::new(SqliteConversationRepository::new(pool));
    let conv_service = ConversationService::new_with_workspace_root(
        conv_repo,
        services.event_bus.clone(),
        std::path::PathBuf::from(&services.data_dir),
    );
    let service = Arc::new(TeamSessionService::new(
        team_repo,
        conv_service,
        services.event_bus.clone(),
    ));
    TeamRouterState { service }
}

/// Build the default `CronRouterState` from application services.
pub fn build_cron_state(services: &AppServices) -> CronRouterState {
    let pool = services.database.pool().clone();
    let cron_repo: Arc<dyn aionui_db::ICronRepository> =
        Arc::new(aionui_db::SqliteCronRepository::new(pool.clone()));

    let conv_repo: Arc<dyn aionui_db::IConversationRepository> =
        Arc::new(SqliteConversationRepository::new(pool));
    let conv_service = ConversationService::new_with_workspace_root(
        conv_repo.clone(),
        services.event_bus.clone(),
        std::path::PathBuf::from(&services.data_dir),
    );

    let busy_guard = Arc::new(aionui_cron::busy_guard::CronBusyGuard::new());
    let executor = Arc::new(aionui_cron::executor::JobExecutor::new(
        services.worker_task_manager.clone(),
        conv_repo,
        Arc::new(conv_service.clone()),
        busy_guard,
    ));

    let tick_service_ref: Arc<CronServiceTickRef> = Arc::new(CronServiceTickRef::default());
    let tick_ref = tick_service_ref.clone();
    let scheduler = Arc::new(aionui_cron::scheduler::CronScheduler::new(Arc::new(
        move |job_id: String| {
            let svc = tick_ref.0.lock().unwrap().clone();
            tokio::spawn(async move {
                if let Some(svc) = svc {
                    svc.tick(&job_id).await;
                }
            });
        },
    )));

    let emitter = CronEventEmitter::new(services.event_bus.clone());
    let cron_service = Arc::new(aionui_cron::service::CronService::new(
        cron_repo, scheduler, executor, emitter,
    ));

    tick_service_ref
        .0
        .lock()
        .unwrap()
        .replace(cron_service.clone());

    CronRouterState {
        cron_service,
        conversation_service: conv_service,
    }
}

/// Build the default `OfficeRouterState` from application services.
pub fn build_office_state(services: &AppServices) -> OfficeRouterState {
    let data_dir = std::path::Path::new(&services.data_dir);

    let spawner: Arc<dyn aionui_office::ProcessSpawner> =
        Arc::new(aionui_office::DefaultProcessSpawner);
    let watch_manager = Arc::new(OfficecliWatchManager::new(
        spawner,
        services.event_bus.clone(),
    ));

    let snapshot_service = Arc::new(OfficeSnapshotService::new(data_dir));
    let star_office_detector = Arc::new(StarOfficeDetector::new(reqwest::Client::new()));
    let conversion_service = Arc::new(ConversionService::new(None));
    let proxy_service = Arc::new(ProxyService::new(watch_manager.clone()));

    OfficeRouterState {
        watch_manager,
        snapshot_service,
        star_office_detector,
        conversion_service,
        proxy_service,
    }
}

/// Build the default `ShellRouterState` from application services.
pub fn build_shell_state(services: &AppServices) -> ShellRouterState {
    let pool = services.database.pool().clone();
    let client_pref_repo = Arc::new(SqliteClientPreferenceRepository::new(pool));
    let client_pref_service = ClientPrefService::new(client_pref_repo);

    ShellRouterState {
        shell_service: Arc::new(aionui_shell::ShellService::new(Arc::new(
            aionui_shell::DefaultSystemOpener,
        ))),
        stt_service: Arc::new(aionui_shell::SttService::new(reqwest::Client::new())),
        client_pref_service,
    }
}

/// Helper to break the circular reference between CronScheduler and CronService.
#[derive(Default)]
struct CronServiceTickRef(std::sync::Mutex<Option<Arc<aionui_cron::service::CronService>>>);

/// Build the default extension-related router states.
///
/// Returns `(ExtensionRouterState, HubRouterState, SkillRouterState)`.
pub async fn build_extension_states(
    services: &AppServices,
) -> (ExtensionRouterState, HubRouterState, SkillRouterState) {
    let legacy_home_dir = dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".aionui");

    let skill_data_dir = std::path::PathBuf::from(&services.data_dir);

    let state_store = ExtensionStateStore::new(legacy_home_dir.join("extension-states.json"));
    let registry = ExtensionRegistry::new(
        state_store,
        services.event_bus.clone(),
        env!("CARGO_PKG_VERSION").to_string(),
    );

    let hub_dir = legacy_home_dir.join("extensions");
    let index_manager = HubIndexManager::new(hub_dir, registry.clone());
    let installer = HubInstaller::new(index_manager.clone(), registry.clone());

    let app_resource_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .and_then(|p| p.parent().map(|pp| pp.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let skill_paths = aionui_extension::resolve_skill_paths(&app_resource_dir, &skill_data_dir);

    let ext_paths_mgr = Arc::new(ExternalPathsManager::new(&legacy_home_dir).await);

    let ext_state = ExtensionRouterState {
        registry: registry.clone(),
    };

    let hub_state = HubRouterState {
        index_manager,
        installer,
    };

    let skill_state = SkillRouterState {
        skill_paths,
        external_paths_manager: ext_paths_mgr,
        assistant_dispatcher: None,
    };

    (ext_state, hub_state, skill_state)
}

/// Build the default `WsHandlerState` from application services.
pub fn build_ws_state(services: &AppServices) -> WsHandlerState {
    if services.local {
        return WsHandlerState {
            manager: services.ws_manager.clone(),
            router: Arc::new(NoopMessageRouter),
            token_validator: Arc::new(|_| true),
            token_extractor: Arc::new(|_| Some("local".into())),
        };
    }

    let jwt_service = services.jwt_service.clone();
    let token_validator = Arc::new(move |token: &str| jwt_service.verify(token).is_ok());

    let token_extractor =
        Arc::new(|headers: &axum::http::HeaderMap| extract_token_from_ws_headers(headers));

    WsHandlerState {
        manager: services.ws_manager.clone(),
        router: Arc::new(NoopMessageRouter),
        token_validator,
        token_extractor,
    }
}
