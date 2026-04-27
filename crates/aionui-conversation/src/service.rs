use std::sync::Arc;

use aionui_ai_agent::{BuildTaskOptions, IWorkerTaskManager, SendMessageData};
use aionui_api_types::{
    ApprovalCheckResponse, CloneConversationRequest, ConfirmRequest, ConfirmationListResponse,
    ConversationListResponse, ConversationResponse, CreateConversationRequest,
    ListConversationsQuery, ListMessagesQuery, MessageListResponse, MessageResponse,
    MessageSearchResponse, SearchMessagesQuery, SendMessageRequest, UpdateConversationRequest,
    WebSocketMessage,
};
use aionui_common::{
    AppError, ConversationSource, ConversationStatus, PaginatedResult, generate_id,
    generate_short_id, now_ms,
};
use aionui_db::{ConversationFilters, ConversationRowUpdate, IConversationRepository, SortOrder};
use aionui_realtime::EventBroadcaster;
use tracing::{debug, info, warn};

use crate::convert::{
    row_to_message_response, row_to_response, search_row_to_item, string_to_enum,
};
use crate::stream_relay::StreamRelay;

#[async_trait::async_trait]
pub trait OnConversationDelete: Send + Sync {
    async fn on_conversation_deleted(&self, conversation_id: &str);
}

#[derive(Clone)]
pub struct ConversationService {
    repo: Arc<dyn IConversationRepository>,
    broadcaster: Arc<dyn EventBroadcaster>,
    delete_hooks: Vec<Arc<dyn OnConversationDelete>>,
    workspace_root: std::path::PathBuf,
}

impl ConversationService {
    pub fn new(
        repo: Arc<dyn IConversationRepository>,
        broadcaster: Arc<dyn EventBroadcaster>,
    ) -> Self {
        Self {
            repo,
            broadcaster,
            delete_hooks: Vec::new(),
            workspace_root: std::path::PathBuf::from("data"),
        }
    }

    pub fn new_with_workspace_root(
        repo: Arc<dyn IConversationRepository>,
        broadcaster: Arc<dyn EventBroadcaster>,
        workspace_root: std::path::PathBuf,
    ) -> Self {
        Self {
            repo,
            broadcaster,
            delete_hooks: Vec::new(),
            workspace_root,
        }
    }

    pub fn with_delete_hooks(
        repo: Arc<dyn IConversationRepository>,
        broadcaster: Arc<dyn EventBroadcaster>,
        delete_hooks: Vec<Arc<dyn OnConversationDelete>>,
    ) -> Self {
        Self {
            repo,
            broadcaster,
            delete_hooks,
            workspace_root: std::path::PathBuf::from("data"),
        }
    }

    /// Create a new conversation.
    ///
    /// Generates a UUID v7, sets status to `pending`, defaults source
    /// to `aionui`, and broadcasts `conversation.listChanged(created)`.
    pub async fn create(
        &self,
        user_id: &str,
        req: CreateConversationRequest,
    ) -> Result<ConversationResponse, AppError> {
        let id = generate_short_id();
        let now = now_ms();
        let source = req.source.unwrap_or(ConversationSource::Aionui);

        let mut extra = req.extra;
        if extra
            .get("workspace")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty()
        {
            let agent_type_label = match req.r#type {
                aionui_common::AgentType::Acp => {
                    let backend = extra.get("backend").and_then(|v| {
                        serde_json::from_value::<aionui_common::AcpBackend>(v.clone()).ok()
                    });
                    match backend {
                        Some(b) => b.display_name().to_lowercase(),
                        None => "acp".to_owned(),
                    }
                }
                aionui_common::AgentType::OpenclawGateway => "openclaw".to_owned(),
                ref t => format!("{:?}", t).to_lowercase(),
            };
            let ws_path = self
                .workspace_root
                .join(format!("{}-temp-{}", agent_type_label, now));
            std::fs::create_dir_all(&ws_path)
                .map_err(|e| AppError::Internal(format!("Failed to create workspace: {e}")))?;
            extra["workspace"] = serde_json::Value::String(ws_path.to_string_lossy().into_owned());
        }

        let row = aionui_db::models::ConversationRow {
            id: id.clone(),
            user_id: user_id.to_owned(),
            name: req.name.unwrap_or_default(),
            r#type: enum_to_db(&req.r#type)?,
            extra: serde_json::to_string(&extra)
                .map_err(|e| AppError::Internal(format!("Failed to serialize extra: {e}")))?,
            model: req
                .model
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|e| AppError::Internal(format!("Failed to serialize model: {e}")))?,
            status: Some(enum_to_db(&ConversationStatus::Pending)?),
            source: Some(enum_to_db(&source)?),
            channel_chat_id: req.channel_chat_id,
            pinned: false,
            pinned_at: None,
            created_at: now,
            updated_at: now,
        };

        self.repo.create(&row).await?;

        let response = row_to_response(row)?;

        self.broadcast_list_changed(&response.id, "created", response.source.as_ref());

        Ok(response)
    }

    /// Get a single conversation by ID.
    ///
    /// Returns `NotFound` if the conversation does not exist or does not
    /// belong to the given user (avoids leaking existence to other users).
    pub async fn get(&self, user_id: &str, id: &str) -> Result<ConversationResponse, AppError> {
        let row = self
            .repo
            .get(id)
            .await?
            .filter(|r| r.user_id == user_id)
            .ok_or_else(|| AppError::NotFound(format!("Conversation {id} not found")))?;
        row_to_response(row)
    }

    /// List conversations with cursor-based pagination and optional filters.
    pub async fn list(
        &self,
        user_id: &str,
        query: ListConversationsQuery,
    ) -> Result<ConversationListResponse, AppError> {
        let filters = ConversationFilters {
            cursor: query.cursor,
            limit: query.limit.unwrap_or(0),
            source: query.source,
            cron_job_id: query.cron_job_id,
            pinned: query.pinned,
        };

        let result = self.repo.list_paginated(user_id, &filters).await?;

        // Tolerate per-row deserialization failures — a single legacy row
        // (e.g. an abandoned agent_type='gemini' conversation post-migration)
        // must not take down the whole listing. Skip-and-log is the
        // explicit resilience contract from the Gemini→ACP migration spec.
        let items = result
            .items
            .into_iter()
            .filter_map(|row| {
                let row_id = row.id.clone();
                match row_to_response(row) {
                    Ok(resp) => Some(resp),
                    Err(err) => {
                        warn!(
                            conversation_id = %row_id,
                            error = %err,
                            "Skipping unreadable conversation row in list"
                        );
                        None
                    }
                }
            })
            .collect();

        Ok(PaginatedResult {
            items,
            total: result.total,
            has_more: result.has_more,
        })
    }

    /// Update a conversation (partial update with extra-merge semantics).
    ///
    /// If `extra` is provided, it is merged into the existing extra JSON
    /// (top-level keys are overwritten, unlisted keys are preserved).
    /// Broadcasts `conversation.listChanged(updated)`.
    pub async fn update(
        &self,
        user_id: &str,
        id: &str,
        req: UpdateConversationRequest,
        task_manager: &Arc<dyn IWorkerTaskManager>,
    ) -> Result<ConversationResponse, AppError> {
        let existing = self
            .repo
            .get(id)
            .await?
            .filter(|r| r.user_id == user_id)
            .ok_or_else(|| AppError::NotFound(format!("Conversation {id} not found")))?;

        let now = now_ms();

        // Merge extra if provided
        let merged_extra = if let Some(new_extra) = &req.extra {
            let mut existing_extra: serde_json::Value =
                serde_json::from_str(&existing.extra).unwrap_or_else(|_| serde_json::json!({}));
            merge_json(&mut existing_extra, new_extra);
            Some(serde_json::to_string(&existing_extra).map_err(|e| {
                AppError::Internal(format!("Failed to serialize merged extra: {e}"))
            })?)
        } else {
            None
        };

        // Handle pinned_at: set timestamp on pin, clear on unpin
        let pinned_at = req.pinned.map(|p| if p { Some(now) } else { None });

        let model_changed = req.model.as_ref().is_some_and(|new_model| {
            let new_json = serde_json::to_string(new_model).unwrap_or_default();
            existing.model.as_deref() != Some(new_json.as_str())
        });

        let model_json = req
            .model
            .as_ref()
            .map(|m| {
                serde_json::to_string(m)
                    .map(Some)
                    .map_err(|e| AppError::Internal(format!("Failed to serialize model: {e}")))
            })
            .transpose()?;

        let updates = ConversationRowUpdate {
            name: req.name,
            pinned: req.pinned,
            pinned_at,
            model: model_json,
            extra: merged_extra,
            status: None,
            updated_at: Some(now),
        };

        self.repo.update(id, &updates).await?;

        if model_changed {
            let _ = task_manager.kill(id, None);
        }

        // Re-fetch to return the updated version
        let updated = self
            .repo
            .get(id)
            .await?
            .ok_or_else(|| AppError::Internal("Conversation vanished after update".into()))?;

        let response = row_to_response(updated)?;

        self.broadcast_list_changed(id, "updated", response.source.as_ref());

        Ok(response)
    }

    /// Delete a conversation (messages cascade via FK).
    ///
    /// Broadcasts `conversation.listChanged(deleted)`.
    pub async fn delete(&self, user_id: &str, id: &str) -> Result<(), AppError> {
        // Get existing to retrieve source for broadcast and verify ownership
        let existing = self
            .repo
            .get(id)
            .await?
            .filter(|r| r.user_id == user_id)
            .ok_or_else(|| AppError::NotFound(format!("Conversation {id} not found")))?;

        let source: Option<ConversationSource> = existing
            .source
            .as_deref()
            .and_then(|s| string_to_enum::<ConversationSource>(s).ok());

        self.repo.delete(id).await?;

        for hook in &self.delete_hooks {
            hook.on_conversation_deleted(id).await;
        }

        self.broadcast_list_changed(id, "deleted", source.as_ref());

        Ok(())
    }

    /// Clone a conversation from an optional source, creating a new one.
    ///
    /// If `source_conversation_id` is given, copies config (type, model,
    /// extra) from the source and merges with provided overrides.
    /// Optionally migrates `cronJobId` binding.
    /// Messages are never copied.
    pub async fn clone_create(
        &self,
        user_id: &str,
        req: CloneConversationRequest,
    ) -> Result<ConversationResponse, AppError> {
        let mut create_req = req.conversation;

        if let Some(source_id) = &req.source_conversation_id {
            let source_row = self
                .repo
                .get(source_id)
                .await?
                .filter(|r| r.user_id == user_id)
                .ok_or_else(|| {
                    AppError::NotFound(format!("Source conversation {source_id} not found"))
                })?;

            // Inherit name from source if not provided
            if create_req.name.is_none() {
                create_req.name = Some(source_row.name.clone());
            }

            // Merge source extra with provided extra
            let source_extra: serde_json::Value =
                serde_json::from_str(&source_row.extra).unwrap_or_else(|_| serde_json::json!({}));
            let mut merged = source_extra;
            merge_json(&mut merged, &create_req.extra);

            // Handle cronJobId migration
            if req.migrate_cron != Some(true)
                && let Some(obj) = merged.as_object_mut()
            {
                obj.remove("cronJobId");
            }

            create_req.extra = merged;
        }

        self.create(user_id, create_req).await
    }

    /// Reset a conversation: clear messages and set status back to pending.
    pub async fn reset(&self, user_id: &str, id: &str) -> Result<(), AppError> {
        // Verify existence and ownership
        self.repo
            .get(id)
            .await?
            .filter(|r| r.user_id == user_id)
            .ok_or_else(|| AppError::NotFound(format!("Conversation {id} not found")))?;

        // Delete all messages
        self.repo.delete_messages_by_conversation(id).await?;

        // Reset status to pending
        let now = now_ms();
        let updates = ConversationRowUpdate {
            status: Some(enum_to_db(&ConversationStatus::Pending)?),
            updated_at: Some(now),
            ..Default::default()
        };
        self.repo.update(id, &updates).await?;

        Ok(())
    }

    /// List conversations associated by the same workspace.
    pub async fn list_associated(
        &self,
        user_id: &str,
        id: &str,
    ) -> Result<Vec<ConversationResponse>, AppError> {
        let rows = self.repo.list_associated(user_id, id).await?;
        rows.into_iter().map(row_to_response).collect()
    }

    /// List conversations spawned by a specific cron job.
    pub async fn list_by_cron_job(
        &self,
        user_id: &str,
        cron_job_id: &str,
    ) -> Result<Vec<ConversationResponse>, AppError> {
        let rows = self.repo.list_by_cron_job(user_id, cron_job_id).await?;
        rows.into_iter().map(row_to_response).collect()
    }

    /// List messages for a conversation with page-based pagination.
    pub async fn list_messages(
        &self,
        user_id: &str,
        conversation_id: &str,
        query: ListMessagesQuery,
    ) -> Result<MessageListResponse, AppError> {
        // Verify conversation exists and belongs to user
        self.repo
            .get(conversation_id)
            .await?
            .filter(|r| r.user_id == user_id)
            .ok_or_else(|| {
                AppError::NotFound(format!("Conversation {conversation_id} not found"))
            })?;

        let page = query.page.unwrap_or(1);
        let page_size = query.page_size.unwrap_or(50);
        let order = match query.order.as_deref() {
            Some("DESC" | "desc") => SortOrder::Desc,
            _ => SortOrder::Asc,
        };

        let result = self
            .repo
            .get_messages(conversation_id, page, page_size, order)
            .await?;

        let items: Vec<MessageResponse> = result
            .items
            .into_iter()
            .map(row_to_message_response)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(PaginatedResult {
            items,
            total: result.total,
            has_more: result.has_more,
        })
    }

    /// Search messages across all conversations for the user.
    pub async fn search_messages(
        &self,
        user_id: &str,
        query: SearchMessagesQuery,
    ) -> Result<MessageSearchResponse, AppError> {
        if query.keyword.trim().is_empty() {
            return Err(AppError::BadRequest("keyword must not be empty".into()));
        }

        let page = query.page.unwrap_or(1);
        let page_size = query.page_size.unwrap_or(20);

        let result = self
            .repo
            .search_messages(user_id, &query.keyword, page, page_size)
            .await?;

        let items = result.items.into_iter().map(search_row_to_item).collect();

        Ok(PaginatedResult {
            items,
            total: result.total,
            has_more: result.has_more,
        })
    }

    // ── Confirmation System ──────────────────────────────────────────

    /// Get the list of pending confirmations for a conversation.
    pub async fn list_confirmations(
        &self,
        user_id: &str,
        conversation_id: &str,
        task_manager: &Arc<dyn IWorkerTaskManager>,
    ) -> Result<ConfirmationListResponse, AppError> {
        self.repo
            .get(conversation_id)
            .await?
            .filter(|r| r.user_id == user_id)
            .ok_or_else(|| {
                AppError::NotFound(format!("Conversation {conversation_id} not found"))
            })?;

        let agent = match task_manager.get_task(conversation_id) {
            Some(a) => a,
            None => return Ok(Vec::new()),
        };

        Ok(agent.get_confirmations())
    }

    /// Confirm a pending tool call.
    ///
    /// Sends the confirmation result to the agent and broadcasts a
    /// `confirmation.remove` WebSocket event.
    pub async fn confirm(
        &self,
        user_id: &str,
        conversation_id: &str,
        call_id: &str,
        req: ConfirmRequest,
        task_manager: &Arc<dyn IWorkerTaskManager>,
    ) -> Result<(), AppError> {
        self.repo
            .get(conversation_id)
            .await?
            .filter(|r| r.user_id == user_id)
            .ok_or_else(|| {
                AppError::NotFound(format!("Conversation {conversation_id} not found"))
            })?;

        let agent = task_manager
            .get_task(conversation_id)
            .ok_or_else(|| AppError::NotFound("No active agent for this conversation".into()))?;

        let confirmations = agent.get_confirmations();
        let conf_id = confirmations
            .iter()
            .find(|c| c.call_id == call_id)
            .map(|c| c.id.clone());

        agent.confirm(&req.msg_id, call_id, req.data, req.always_allow)?;

        if let Some(conf_id) = conf_id {
            let payload = serde_json::json!({
                "conversation_id": conversation_id,
                "id": conf_id,
            });
            let msg = WebSocketMessage::new("confirmation.remove", payload);
            self.broadcaster.broadcast(msg);
        }

        Ok(())
    }

    /// Check whether an action has been auto-approved in the current session.
    pub async fn check_approval(
        &self,
        user_id: &str,
        conversation_id: &str,
        action: &str,
        command_type: Option<&str>,
        task_manager: &Arc<dyn IWorkerTaskManager>,
    ) -> Result<ApprovalCheckResponse, AppError> {
        self.repo
            .get(conversation_id)
            .await?
            .filter(|r| r.user_id == user_id)
            .ok_or_else(|| {
                AppError::NotFound(format!("Conversation {conversation_id} not found"))
            })?;

        let approved = task_manager
            .get_task(conversation_id)
            .is_some_and(|agent| agent.check_approval(action, command_type));

        Ok(ApprovalCheckResponse { approved })
    }

    // ── Message Flow ─────────────────────────────────────────────────

    /// Send a user message to the conversation.
    ///
    /// 1. Validates the conversation belongs to the user
    /// 2. Stores the user message (position: "right", status: "finish")
    /// 3. Gets or builds the agent task
    /// 4. Sends the message to the agent
    /// 5. Spawns a background relay (agent events → WebSocket + DB)
    /// 6. Returns immediately (202 Accepted semantics)
    pub async fn send_message(
        &self,
        user_id: &str,
        conversation_id: &str,
        req: SendMessageRequest,
        task_manager: &Arc<dyn IWorkerTaskManager>,
    ) -> Result<(), AppError> {
        if req.content.trim().is_empty() {
            return Err(AppError::BadRequest(
                "Message content must not be empty".into(),
            ));
        }

        // Verify conversation exists and belongs to user
        let row = self
            .repo
            .get(conversation_id)
            .await?
            .filter(|r| r.user_id == user_id)
            .ok_or_else(|| {
                AppError::NotFound(format!("Conversation {conversation_id} not found"))
            })?;

        // Short-circuit for legacy Gemini conversations: the dedicated Gemini
        // runtime has been removed, so we cannot build an agent for this row.
        // Emit CONVERSATION_ARCHIVED (HTTP 410 Gone) without touching the
        // legacy `model` column, which may hold shapes the new parser can't
        // deserialize. The client identifies this case by `code` and renders
        // a dedicated archived-conversation UI rather than a generic banner.
        if row.r#type == "gemini" {
            return Err(AppError::ConversationArchived(
                "This conversation was created with the legacy Gemini runtime, which has been \
                 removed. Please start a new conversation with the Gemini ACP backend to continue."
                    .into(),
            ));
        }

        // Check if conversation is already processing (simple guard)
        let status: ConversationStatus = match row.status.as_deref() {
            None | Some("") => ConversationStatus::Finished,
            Some(s) => string_to_enum(s)?,
        };
        if status == ConversationStatus::Running {
            return Err(AppError::Conflict(
                "Conversation is already processing a message".into(),
            ));
        }

        // Store user message
        let user_msg = aionui_db::models::MessageRow {
            id: generate_id(),
            conversation_id: conversation_id.to_owned(),
            msg_id: Some(req.msg_id.clone()),
            r#type: "text".into(),
            content: serde_json::json!({ "content": req.content }).to_string(),
            position: Some("right".into()),
            status: Some("finish".into()),
            hidden: false,
            created_at: now_ms(),
        };
        self.repo.insert_message(&user_msg).await?;

        // Build task options from conversation row
        let build_opts = self.build_task_options(&row)?;
        let stored_workspace = build_opts.workspace.clone();
        let agent = task_manager.get_or_build_task(conversation_id, build_opts)?;

        // If the factory resolved a different workspace (e.g. auto-created temp
        // dir for a legacy conversation with empty workspace), persist it back.
        self.maybe_persist_workspace(conversation_id, &stored_workspace, agent.workspace())
            .await?;

        // Update conversation status to running
        let update = ConversationRowUpdate {
            status: Some(enum_to_db(&ConversationStatus::Running)?),
            updated_at: Some(now_ms()),
            ..Default::default()
        };
        self.repo.update(conversation_id, &update).await?;

        // Subscribe to agent events before sending (no events lost)
        let rx = agent.subscribe();

        // Spawn background relay BEFORE sending — prompt() blocks until the
        // agent turn completes, so the relay must already be running to
        // consume streaming events as they arrive.
        let relay = StreamRelay::new(
            conversation_id.to_owned(),
            generate_id(),
            Arc::clone(&self.repo),
            Arc::clone(&self.broadcaster),
        );
        tokio::spawn(relay.run(rx));

        // Send message to the agent in a background task.
        // prompt() blocks until the PromptResponse arrives (turn completed),
        // but the HTTP handler should return 202 immediately.
        let msg_id_log = req.msg_id.clone();
        let send_data = SendMessageData {
            content: req.content,
            msg_id: req.msg_id,
            files: req.files,
            inject_skills: req.inject_skills,
        };
        let conv_id = conversation_id.to_owned();
        tokio::spawn(async move {
            if let Err(e) = agent.send_message(send_data).await {
                tracing::error!(conversation_id = %conv_id, error = %e, "Agent send_message failed");
            }
        });

        info!(conversation_id, msg_id = %msg_id_log, "Message dispatched, stream relay started");
        Ok(())
    }

    /// Stop the current streaming response for a conversation.
    pub async fn stop_stream(
        &self,
        user_id: &str,
        conversation_id: &str,
        task_manager: &Arc<dyn IWorkerTaskManager>,
    ) -> Result<(), AppError> {
        // Verify conversation exists and belongs to user
        self.repo
            .get(conversation_id)
            .await?
            .filter(|r| r.user_id == user_id)
            .ok_or_else(|| {
                AppError::NotFound(format!("Conversation {conversation_id} not found"))
            })?;

        let agent = task_manager
            .get_task(conversation_id)
            .ok_or_else(|| AppError::Conflict("No active agent for this conversation".into()))?;

        agent.stop().await?;

        info!(conversation_id, "Stream stopped");
        Ok(())
    }

    /// Pre-initialize an agent task for a conversation (warmup).
    ///
    /// This builds the agent task without sending a message, so the
    /// first real message can be processed faster.
    pub async fn warmup(
        &self,
        user_id: &str,
        conversation_id: &str,
        task_manager: &Arc<dyn IWorkerTaskManager>,
    ) -> Result<(), AppError> {
        let row = self
            .repo
            .get(conversation_id)
            .await?
            .filter(|r| r.user_id == user_id)
            .ok_or_else(|| {
                AppError::NotFound(format!("Conversation {conversation_id} not found"))
            })?;

        let build_opts = self.build_task_options(&row)?;
        let stored_workspace = build_opts.workspace.clone();
        let agent = task_manager.get_or_build_task(conversation_id, build_opts)?;

        // Persist auto-resolved workspace if factory picked a different path.
        self.maybe_persist_workspace(conversation_id, &stored_workspace, agent.workspace())
            .await?;

        debug!(conversation_id, "Agent warmed up");
        Ok(())
    }

    /// Build [`BuildTaskOptions`] from a conversation database row.
    fn build_task_options(
        &self,
        row: &aionui_db::models::ConversationRow,
    ) -> Result<BuildTaskOptions, AppError> {
        let agent_type = string_to_enum(&row.r#type)?;

        let model = row
            .model
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| AppError::Internal(format!("Invalid model JSON: {e}")))?
            .unwrap_or_else(|| aionui_common::ProviderWithModel {
                provider_id: String::new(),
                model: String::new(),
                use_model: None,
            });

        let extra: serde_json::Value = serde_json::from_str(&row.extra)
            .map_err(|e| AppError::Internal(format!("Invalid extra JSON: {e}")))?;

        // Extract workspace from extra (common across agent types)
        let workspace = extra
            .get("workspace")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();

        Ok(BuildTaskOptions {
            agent_type,
            workspace,
            model,
            conversation_id: row.id.clone(),
            extra,
        })
    }

    /// Write the resolved workspace back to `conversation.extra.workspace` when
    /// the factory picked a different (auto-generated) path than what was stored.
    ///
    /// This handles legacy conversations whose `extra.workspace` was empty:
    /// the factory creates a temp dir at task-build time, and we persist that
    /// path here so the frontend can display the workspace panel correctly.
    async fn maybe_persist_workspace(
        &self,
        conversation_id: &str,
        stored_workspace: &str,
        resolved_workspace: &str,
    ) -> Result<(), AppError> {
        if resolved_workspace.is_empty() || resolved_workspace == stored_workspace {
            return Ok(());
        }

        // Fetch latest extra, merge the resolved workspace path in, and persist.
        let row = self.repo.get(conversation_id).await?.ok_or_else(|| {
            AppError::Internal("Conversation vanished during workspace sync".into())
        })?;

        let mut extra: serde_json::Value =
            serde_json::from_str(&row.extra).unwrap_or_else(|_| serde_json::json!({}));
        extra["workspace"] = serde_json::Value::String(resolved_workspace.to_owned());

        let extra_json = serde_json::to_string(&extra)
            .map_err(|e| AppError::Internal(format!("Failed to serialize extra: {e}")))?;

        let update = ConversationRowUpdate {
            extra: Some(extra_json),
            updated_at: Some(now_ms()),
            ..Default::default()
        };
        self.repo.update(conversation_id, &update).await?;

        debug!(
            conversation_id,
            workspace = resolved_workspace,
            "Persisted auto-resolved workspace to conversation.extra"
        );
        Ok(())
    }

    /// Broadcast a `conversation.listChanged` WebSocket event.
    fn broadcast_list_changed(
        &self,
        conversation_id: &str,
        action: &str,
        source: Option<&ConversationSource>,
    ) {
        let payload = serde_json::json!({
            "conversation_id": conversation_id,
            "action": action,
            "source": source,
        });
        let event = WebSocketMessage::new("conversation.listChanged", payload);
        self.broadcaster.broadcast(event);
    }
}

// ── Helpers ────────────────────────────────────────────────────────

/// Serialize a serde-compatible enum to its JSON string form for DB storage.
///
/// e.g. `AgentType::Acp` → `"acp"`
fn enum_to_db<T: serde::Serialize>(val: &T) -> Result<String, AppError> {
    let json_val = serde_json::to_value(val)
        .map_err(|e| AppError::Internal(format!("Enum serialization failed: {e}")))?;
    json_val
        .as_str()
        .map(|s| s.to_owned())
        .ok_or_else(|| AppError::Internal("Expected string enum value".into()))
}

/// Merge `patch` into `base` (top-level key overwrite).
fn merge_json(base: &mut serde_json::Value, patch: &serde_json::Value) {
    if let (Some(base_obj), Some(patch_obj)) = (base.as_object_mut(), patch.as_object()) {
        for (key, value) in patch_obj {
            base_obj.insert(key.clone(), value.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn enum_to_db_agent_type() {
        use aionui_common::AgentType;
        assert_eq!(enum_to_db(&AgentType::Acp).unwrap(), "acp");
        assert_eq!(enum_to_db(&AgentType::Nanobot).unwrap(), "nanobot");
        assert_eq!(
            enum_to_db(&AgentType::OpenclawGateway).unwrap(),
            "openclaw-gateway"
        );
    }

    #[test]
    fn enum_to_db_status() {
        assert_eq!(enum_to_db(&ConversationStatus::Pending).unwrap(), "pending");
        assert_eq!(enum_to_db(&ConversationStatus::Running).unwrap(), "running");
        assert_eq!(
            enum_to_db(&ConversationStatus::Finished).unwrap(),
            "finished"
        );
    }

    #[test]
    fn enum_to_db_source() {
        assert_eq!(enum_to_db(&ConversationSource::Aionui).unwrap(), "aionui");
        assert_eq!(
            enum_to_db(&ConversationSource::Telegram).unwrap(),
            "telegram"
        );
    }

    #[test]
    fn merge_json_top_level_overwrite() {
        let mut base = json!({"a": 1, "b": 2});
        let patch = json!({"b": 3, "c": 4});
        merge_json(&mut base, &patch);
        assert_eq!(base, json!({"a": 1, "b": 3, "c": 4}));
    }

    #[test]
    fn merge_json_into_empty() {
        let mut base = json!({});
        let patch = json!({"x": "hello"});
        merge_json(&mut base, &patch);
        assert_eq!(base, json!({"x": "hello"}));
    }

    #[test]
    fn merge_json_non_object_noop() {
        let mut base = json!("string");
        let patch = json!({"a": 1});
        merge_json(&mut base, &patch);
        assert_eq!(base, json!("string"));
    }

    #[test]
    fn merge_json_empty_patch() {
        let mut base = json!({"a": 1});
        let patch = json!({});
        merge_json(&mut base, &patch);
        assert_eq!(base, json!({"a": 1}));
    }
}
