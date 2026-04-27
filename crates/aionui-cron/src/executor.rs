use std::sync::Arc;

use aionui_ai_agent::task_manager::IWorkerTaskManager;
use aionui_ai_agent::types::{BuildTaskOptions, SendMessageData};
use aionui_api_types::CreateConversationRequest;
use aionui_common::{AgentType, ProviderWithModel, generate_id};
use aionui_conversation::ConversationService;
use aionui_db::IConversationRepository;
use tracing::{error, info, warn};

use crate::busy_guard::CronBusyGuard;
use crate::error::CronError;
use crate::types::{CronJob, ExecutionMode};

pub const RETRY_INTERVAL_MS: u64 = 30_000;
pub const MAX_RETRIES_DEFAULT: i64 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionResult {
    Success { conversation_id: String },
    Retrying { attempt: i64 },
    Skipped,
    Error { message: String },
}

pub struct JobExecutor {
    task_manager: Arc<dyn IWorkerTaskManager>,
    conversation_repo: Arc<dyn IConversationRepository>,
    conversation_service: Arc<ConversationService>,
    busy_guard: Arc<CronBusyGuard>,
}

impl JobExecutor {
    pub fn new(
        task_manager: Arc<dyn IWorkerTaskManager>,
        conversation_repo: Arc<dyn IConversationRepository>,
        conversation_service: Arc<ConversationService>,
        busy_guard: Arc<CronBusyGuard>,
    ) -> Self {
        Self {
            task_manager,
            conversation_repo,
            conversation_service,
            busy_guard,
        }
    }

    pub async fn execute(&self, job: &CronJob) -> ExecutionResult {
        let conversation_id = &job.conversation_id;

        if self.busy_guard.is_busy(conversation_id) {
            return self.handle_busy(job);
        }

        let target_conversation_id = match self.resolve_conversation(job).await {
            Ok(id) => id,
            Err(e) => {
                error!(job_id = %job.id, error = %e, "Failed to resolve conversation");
                return ExecutionResult::Error {
                    message: e.to_string(),
                };
            }
        };

        self.busy_guard
            .set_processing(&target_conversation_id, true);

        let result = self.execute_inner(job, &target_conversation_id).await;

        self.busy_guard
            .set_processing(&target_conversation_id, false);

        result
    }

    pub async fn execute_run_now(&self, job: &CronJob) -> ExecutionResult {
        let target_conversation_id = match self.resolve_conversation(job).await {
            Ok(id) => id,
            Err(e) => {
                error!(job_id = %job.id, error = %e, "Failed to resolve conversation for run-now");
                return ExecutionResult::Error {
                    message: e.to_string(),
                };
            }
        };

        self.busy_guard
            .set_processing(&target_conversation_id, true);

        let result = self.execute_inner(job, &target_conversation_id).await;

        self.busy_guard
            .set_processing(&target_conversation_id, false);

        result
    }

    pub fn busy_guard(&self) -> &CronBusyGuard {
        &self.busy_guard
    }
}

impl JobExecutor {
    fn handle_busy(&self, job: &CronJob) -> ExecutionResult {
        let max_retries = job.max_retries;
        let current_retry = job.retry_count;

        if current_retry >= max_retries {
            warn!(
                job_id = %job.id,
                retries = current_retry,
                "Max retries exceeded, skipping"
            );
            return ExecutionResult::Skipped;
        }

        let attempt = current_retry + 1;
        info!(
            job_id = %job.id,
            attempt,
            max_retries,
            "Conversation busy, scheduling retry"
        );
        ExecutionResult::Retrying { attempt }
    }

    async fn resolve_conversation(&self, job: &CronJob) -> Result<String, CronError> {
        match job.execution_mode {
            ExecutionMode::Existing => {
                self.verify_conversation_exists(&job.conversation_id)
                    .await?;
                Ok(job.conversation_id.clone())
            }
            ExecutionMode::NewConversation => self.create_new_conversation(job).await,
        }
    }

    async fn verify_conversation_exists(&self, conversation_id: &str) -> Result<(), CronError> {
        let exists = self
            .conversation_repo
            .get(conversation_id)
            .await
            .map_err(CronError::Database)?;
        if exists.is_none() {
            return Err(CronError::Scheduler(format!(
                "conversation {conversation_id} not found"
            )));
        }
        Ok(())
    }

    async fn create_new_conversation(&self, job: &CronJob) -> Result<String, CronError> {
        let agent_type = parse_agent_type(&job.agent_type);
        let model = resolve_model(job);

        let extra = serde_json::json!({
            "cronJobId": job.id,
        });

        let req = CreateConversationRequest {
            r#type: agent_type,
            name: Some(job.name.clone()),
            model: Some(model),
            source: None,
            channel_chat_id: None,
            extra,
        };

        let response = self
            .conversation_service
            .create("cron", req)
            .await
            .map_err(|e| CronError::Scheduler(format!("create conversation: {e}")))?;

        info!(
            job_id = %job.id,
            conversation_id = %response.id,
            "Created new conversation for cron job"
        );

        Ok(response.id)
    }

    async fn execute_inner(&self, job: &CronJob, conversation_id: &str) -> ExecutionResult {
        let agent_type = parse_agent_type(&job.agent_type);
        let model = resolve_model(job);
        let workspace = job
            .agent_config
            .as_ref()
            .and_then(|c| c.workspace.clone())
            .unwrap_or_default();

        let build_extra = build_task_extra(job);

        let options = BuildTaskOptions {
            agent_type,
            workspace,
            model,
            conversation_id: conversation_id.to_owned(),
            extra: build_extra,
        };

        let agent = match self
            .task_manager
            .get_or_build_task(conversation_id, options)
        {
            Ok(handle) => handle,
            Err(e) => {
                error!(
                    job_id = %job.id,
                    error = %e,
                    "Failed to get or build agent task"
                );
                return ExecutionResult::Error {
                    message: e.to_string(),
                };
            }
        };

        let prompt = build_prompt(job);
        let msg_id = generate_id();

        let send_data = SendMessageData {
            content: prompt,
            msg_id,
            files: vec![],
            inject_skills: vec![],
        };

        match agent.send_message(send_data).await {
            Ok(()) => {
                info!(
                    job_id = %job.id,
                    conversation_id,
                    "Cron job message sent successfully"
                );
                ExecutionResult::Success {
                    conversation_id: conversation_id.to_owned(),
                }
            }
            Err(e) => {
                error!(
                    job_id = %job.id,
                    conversation_id,
                    error = %e,
                    "Failed to send cron job message"
                );
                ExecutionResult::Error {
                    message: e.to_string(),
                }
            }
        }
    }
}

fn parse_agent_type(agent_type_str: &str) -> AgentType {
    serde_json::from_value(serde_json::Value::String(agent_type_str.to_owned()))
        .unwrap_or(AgentType::Acp)
}

fn resolve_model(job: &CronJob) -> ProviderWithModel {
    if let Some(config) = &job.agent_config {
        ProviderWithModel {
            provider_id: config.backend.clone(),
            model: config
                .model_id
                .clone()
                .unwrap_or_else(|| "default".to_owned()),
            use_model: None,
        }
    } else {
        ProviderWithModel {
            provider_id: job.agent_type.clone(),
            model: "default".to_owned(),
            use_model: None,
        }
    }
}

fn build_task_extra(job: &CronJob) -> serde_json::Value {
    let mut extra = serde_json::Map::new();
    extra.insert(
        "cronJobId".to_owned(),
        serde_json::Value::String(job.id.clone()),
    );

    if let Some(config) = &job.agent_config {
        extra.insert(
            "backend".to_owned(),
            serde_json::Value::String(config.backend.clone()),
        );
        if let Some(cli_path) = &config.cli_path {
            extra.insert(
                "cliPath".to_owned(),
                serde_json::Value::String(cli_path.clone()),
            );
        }
        if !config.name.is_empty() {
            extra.insert(
                "agentName".to_owned(),
                serde_json::Value::String(config.name.clone()),
            );
        }
        if let Some(custom_agent_id) = &config.custom_agent_id {
            extra.insert(
                "customAgentId".to_owned(),
                serde_json::Value::String(custom_agent_id.clone()),
            );
        }
        if let Some(mode) = &config.mode {
            extra.insert(
                "sessionMode".to_owned(),
                serde_json::Value::String(mode.clone()),
            );
        }
    }

    serde_json::Value::Object(extra)
}

fn build_prompt(job: &CronJob) -> String {
    match (&job.execution_mode, &job.skill_content) {
        (ExecutionMode::NewConversation, Some(skill)) if !skill.is_empty() => {
            format!(
                "{}\n\n---\n\n## Skill Instructions\n\n{}",
                job.message, skill
            )
        }
        _ => job.message.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CreatedBy, CronAgentConfig, CronSchedule};

    fn sample_job() -> CronJob {
        CronJob {
            id: "cron_test1".into(),
            name: "Test Job".into(),
            enabled: true,
            schedule: CronSchedule::Every {
                every_ms: 60000,
                description: None,
            },
            message: "do something".into(),
            execution_mode: ExecutionMode::Existing,
            agent_config: Some(CronAgentConfig {
                backend: "acp".into(),
                name: "Claude".into(),
                cli_path: Some("/usr/bin/claude".into()),
                is_preset: None,
                custom_agent_id: None,
                preset_agent_type: None,
                mode: None,
                model_id: Some("claude-sonnet-4".into()),
                config_options: None,
                workspace: Some("/home/user/project".into()),
            }),
            conversation_id: "conv_1".into(),
            conversation_title: Some("Test Conv".into()),
            agent_type: "acp".into(),
            created_by: CreatedBy::User,
            skill_content: None,
            description: None,
            created_at: 1000,
            updated_at: 2000,
            next_run_at: Some(3000),
            last_run_at: None,
            last_status: None,
            last_error: None,
            run_count: 0,
            retry_count: 0,
            max_retries: 3,
        }
    }

    // -- handle_busy tests ---------------------------------------------------

    #[test]
    fn handle_busy_returns_retrying_when_under_limit() {
        let guard = CronBusyGuard::new();
        let executor = make_executor_for_busy_tests(Arc::new(guard));

        let job = CronJob {
            retry_count: 1,
            max_retries: 3,
            ..sample_job()
        };
        let result = executor.handle_busy(&job);
        assert_eq!(result, ExecutionResult::Retrying { attempt: 2 });
    }

    #[test]
    fn handle_busy_returns_skipped_when_at_limit() {
        let guard = CronBusyGuard::new();
        let executor = make_executor_for_busy_tests(Arc::new(guard));

        let job = CronJob {
            retry_count: 3,
            max_retries: 3,
            ..sample_job()
        };
        let result = executor.handle_busy(&job);
        assert_eq!(result, ExecutionResult::Skipped);
    }

    #[test]
    fn handle_busy_returns_skipped_when_over_limit() {
        let guard = CronBusyGuard::new();
        let executor = make_executor_for_busy_tests(Arc::new(guard));

        let job = CronJob {
            retry_count: 5,
            max_retries: 3,
            ..sample_job()
        };
        let result = executor.handle_busy(&job);
        assert_eq!(result, ExecutionResult::Skipped);
    }

    #[test]
    fn handle_busy_first_retry_returns_attempt_1() {
        let guard = CronBusyGuard::new();
        let executor = make_executor_for_busy_tests(Arc::new(guard));

        let job = CronJob {
            retry_count: 0,
            max_retries: 3,
            ..sample_job()
        };
        let result = executor.handle_busy(&job);
        assert_eq!(result, ExecutionResult::Retrying { attempt: 1 });
    }

    // -- build_prompt tests --------------------------------------------------

    #[test]
    fn build_prompt_existing_mode_no_skill() {
        let job = sample_job();
        let prompt = build_prompt(&job);
        assert_eq!(prompt, "do something");
    }

    #[test]
    fn build_prompt_existing_mode_with_skill_ignores_skill() {
        let job = CronJob {
            skill_content: Some("skill content".into()),
            ..sample_job()
        };
        let prompt = build_prompt(&job);
        assert_eq!(prompt, "do something");
    }

    #[test]
    fn build_prompt_new_conv_with_skill() {
        let job = CronJob {
            execution_mode: ExecutionMode::NewConversation,
            skill_content: Some("---\nname: test\n---\nDo X".into()),
            ..sample_job()
        };
        let prompt = build_prompt(&job);
        assert!(prompt.contains("do something"));
        assert!(prompt.contains("Skill Instructions"));
        assert!(prompt.contains("Do X"));
    }

    #[test]
    fn build_prompt_new_conv_no_skill() {
        let job = CronJob {
            execution_mode: ExecutionMode::NewConversation,
            ..sample_job()
        };
        let prompt = build_prompt(&job);
        assert_eq!(prompt, "do something");
    }

    #[test]
    fn build_prompt_new_conv_empty_skill() {
        let job = CronJob {
            execution_mode: ExecutionMode::NewConversation,
            skill_content: Some(String::new()),
            ..sample_job()
        };
        let prompt = build_prompt(&job);
        assert_eq!(prompt, "do something");
    }

    // -- parse_agent_type tests -----------------------------------------------

    #[test]
    fn parse_agent_type_known_types() {
        assert_eq!(parse_agent_type("acp"), AgentType::Acp);
        assert_eq!(parse_agent_type("nanobot"), AgentType::Nanobot);
    }

    #[test]
    fn parse_agent_type_unknown_defaults_to_acp() {
        assert_eq!(parse_agent_type("unknown_type"), AgentType::Acp);
    }

    // -- resolve_model tests -------------------------------------------------

    #[test]
    fn resolve_model_with_config() {
        let job = sample_job();
        let model = resolve_model(&job);
        assert_eq!(model.provider_id, "acp");
        assert_eq!(model.model, "claude-sonnet-4");
    }

    #[test]
    fn resolve_model_without_config() {
        let job = CronJob {
            agent_config: None,
            ..sample_job()
        };
        let model = resolve_model(&job);
        assert_eq!(model.provider_id, "acp");
        assert_eq!(model.model, "default");
    }

    #[test]
    fn resolve_model_config_no_model_id() {
        let job = CronJob {
            agent_config: Some(CronAgentConfig {
                backend: "gemini".into(),
                name: "Gemini".into(),
                cli_path: None,
                is_preset: None,
                custom_agent_id: None,
                preset_agent_type: None,
                mode: None,
                model_id: None,
                config_options: None,
                workspace: None,
            }),
            ..sample_job()
        };
        let model = resolve_model(&job);
        assert_eq!(model.provider_id, "gemini");
        assert_eq!(model.model, "default");
    }

    // -- build_task_extra tests -----------------------------------------------

    #[test]
    fn build_task_extra_includes_cron_job_id() {
        let job = sample_job();
        let extra = build_task_extra(&job);
        assert_eq!(extra["cronJobId"], "cron_test1");
    }

    #[test]
    fn build_task_extra_with_config_fields() {
        let job = sample_job();
        let extra = build_task_extra(&job);
        assert_eq!(extra["backend"], "acp");
        assert_eq!(extra["cliPath"], "/usr/bin/claude");
        assert_eq!(extra["agentName"], "Claude");
    }

    #[test]
    fn build_task_extra_without_config() {
        let job = CronJob {
            agent_config: None,
            ..sample_job()
        };
        let extra = build_task_extra(&job);
        assert_eq!(extra["cronJobId"], "cron_test1");
        assert!(extra.get("backend").is_none());
    }

    // -- execution_result display ---------------------------------------------

    #[test]
    fn execution_result_variants() {
        let success = ExecutionResult::Success {
            conversation_id: "conv_1".into(),
        };
        assert_eq!(
            success,
            ExecutionResult::Success {
                conversation_id: "conv_1".into()
            }
        );

        let retrying = ExecutionResult::Retrying { attempt: 2 };
        assert_eq!(retrying, ExecutionResult::Retrying { attempt: 2 });

        assert_eq!(ExecutionResult::Skipped, ExecutionResult::Skipped);

        let error = ExecutionResult::Error {
            message: "oops".into(),
        };
        assert_eq!(
            error,
            ExecutionResult::Error {
                message: "oops".into()
            }
        );
    }

    // -- helper ---------------------------------------------------------------

    fn make_executor_for_busy_tests(guard: Arc<CronBusyGuard>) -> JobExecutor {
        use aionui_ai_agent::agent_manager::AgentManagerHandle;
        use aionui_api_types::WebSocketMessage;
        use aionui_common::PaginatedResult;
        use aionui_db::{
            ConversationFilters, ConversationRowUpdate, MessageRowUpdate, MessageSearchRow,
            SortOrder,
        };

        struct StubTaskManager;
        impl IWorkerTaskManager for StubTaskManager {
            fn get_task(&self, _: &str) -> Option<AgentManagerHandle> {
                None
            }
            fn get_or_build_task(
                &self,
                _: &str,
                _: BuildTaskOptions,
            ) -> Result<AgentManagerHandle, aionui_common::AppError> {
                Err(aionui_common::AppError::Internal("stub".into()))
            }
            fn kill(
                &self,
                _: &str,
                _: Option<aionui_common::AgentKillReason>,
            ) -> Result<(), aionui_common::AppError> {
                Ok(())
            }
            fn clear(&self) {}
            fn active_count(&self) -> usize {
                0
            }
            fn collect_idle(&self, _: aionui_common::TimestampMs) -> Vec<String> {
                vec![]
            }
        }

        struct StubConvRepo;

        #[async_trait::async_trait]
        impl IConversationRepository for StubConvRepo {
            async fn get(
                &self,
                _id: &str,
            ) -> Result<Option<aionui_db::models::ConversationRow>, aionui_db::DbError>
            {
                Ok(None)
            }
            async fn create(
                &self,
                _row: &aionui_db::models::ConversationRow,
            ) -> Result<(), aionui_db::DbError> {
                Ok(())
            }
            async fn update(
                &self,
                _id: &str,
                _updates: &ConversationRowUpdate,
            ) -> Result<(), aionui_db::DbError> {
                Ok(())
            }
            async fn delete(&self, _id: &str) -> Result<(), aionui_db::DbError> {
                Ok(())
            }
            async fn list_paginated(
                &self,
                _user_id: &str,
                _filters: &ConversationFilters,
            ) -> Result<PaginatedResult<aionui_db::models::ConversationRow>, aionui_db::DbError>
            {
                Ok(PaginatedResult {
                    items: vec![],
                    total: 0,
                    has_more: false,
                })
            }
            async fn find_by_source_and_chat(
                &self,
                _user_id: &str,
                _source: &str,
                _chat_id: &str,
                _agent_type: &str,
            ) -> Result<Option<aionui_db::models::ConversationRow>, aionui_db::DbError>
            {
                Ok(None)
            }
            async fn list_by_cron_job(
                &self,
                _user_id: &str,
                _cron_job_id: &str,
            ) -> Result<Vec<aionui_db::models::ConversationRow>, aionui_db::DbError> {
                Ok(vec![])
            }
            async fn list_associated(
                &self,
                _user_id: &str,
                _conversation_id: &str,
            ) -> Result<Vec<aionui_db::models::ConversationRow>, aionui_db::DbError> {
                Ok(vec![])
            }
            async fn get_messages(
                &self,
                _conv_id: &str,
                _page: u32,
                _page_size: u32,
                _order: SortOrder,
            ) -> Result<PaginatedResult<aionui_db::models::MessageRow>, aionui_db::DbError>
            {
                Ok(PaginatedResult {
                    items: vec![],
                    total: 0,
                    has_more: false,
                })
            }
            async fn insert_message(
                &self,
                _message: &aionui_db::models::MessageRow,
            ) -> Result<(), aionui_db::DbError> {
                Ok(())
            }
            async fn update_message(
                &self,
                _id: &str,
                _updates: &MessageRowUpdate,
            ) -> Result<(), aionui_db::DbError> {
                Ok(())
            }
            async fn delete_messages_by_conversation(
                &self,
                _conv_id: &str,
            ) -> Result<(), aionui_db::DbError> {
                Ok(())
            }
            async fn get_message_by_msg_id(
                &self,
                _conv_id: &str,
                _msg_id: &str,
                _msg_type: &str,
            ) -> Result<Option<aionui_db::models::MessageRow>, aionui_db::DbError> {
                Ok(None)
            }
            async fn search_messages(
                &self,
                _user_id: &str,
                _keyword: &str,
                _page: u32,
                _page_size: u32,
            ) -> Result<PaginatedResult<MessageSearchRow>, aionui_db::DbError> {
                Ok(PaginatedResult {
                    items: vec![],
                    total: 0,
                    has_more: false,
                })
            }
        }

        struct StubBroadcaster;
        impl aionui_realtime::EventBroadcaster for StubBroadcaster {
            fn broadcast(&self, _: WebSocketMessage<serde_json::Value>) {}
        }

        let stub_broadcaster: Arc<dyn aionui_realtime::EventBroadcaster> =
            Arc::new(StubBroadcaster);
        let stub_repo: Arc<dyn IConversationRepository> = Arc::new(StubConvRepo);
        let conv_service = Arc::new(ConversationService::new(
            Arc::clone(&stub_repo),
            stub_broadcaster,
        ));

        JobExecutor::new(Arc::new(StubTaskManager), stub_repo, conv_service, guard)
    }
}
