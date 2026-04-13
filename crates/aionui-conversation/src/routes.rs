use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Json, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Router;

use aionui_api_types::{
    ApiResponse, ApprovalCheckQuery, ApprovalCheckResponse, CloneConversationRequest,
    ConfirmRequest, ConfirmationListResponse, ConversationListResponse, ConversationResponse,
    CreateConversationRequest, ListConversationsQuery, ListMessagesQuery, MessageListResponse,
    MessageSearchResponse, SearchMessagesQuery, SendMessageRequest, UpdateConversationRequest,
};
use aionui_auth::CurrentUser;
use aionui_common::AppError;

use crate::state::ConversationRouterState;

/// Build the conversation router (CRUD + message flow + confirmation + extended operations).
///
/// All routes require authentication (applied by the caller).
pub fn conversation_routes(state: ConversationRouterState) -> Router {
    Router::new()
        .route("/api/conversations", post(create).get(list))
        // Static path must come before `{id}` wildcard
        .route("/api/conversations/clone", post(clone))
        .route(
            "/api/conversations/{id}",
            get(get_one).patch(update).delete(delete_one),
        )
        .route("/api/conversations/{id}/reset", post(reset))
        .route("/api/conversations/{id}/associated", get(associated))
        .route(
            "/api/conversations/{id}/messages",
            get(list_messages).post(send_message),
        )
        .route("/api/conversations/{id}/stop", post(stop_stream))
        .route("/api/conversations/{id}/warmup", post(warmup))
        // Confirmation system
        .route(
            "/api/conversations/{id}/confirmations",
            get(list_confirmations),
        )
        .route(
            "/api/conversations/{id}/confirmations/{callId}/confirm",
            post(confirm),
        )
        .route(
            "/api/conversations/{id}/approvals/check",
            get(check_approval),
        )
        .route("/api/messages/search", get(search_messages))
        .with_state(state)
}

// ── Handlers ───────────────────────────────────────────────────────

async fn create(
    State(state): State<ConversationRouterState>,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<CreateConversationRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<ApiResponse<ConversationResponse>>), AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let conversation = state.conversation_service.create(&user.id, req).await?;
    Ok((StatusCode::CREATED, Json(ApiResponse::ok(conversation))))
}

async fn list(
    State(state): State<ConversationRouterState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<ListConversationsQuery>,
) -> Result<Json<ApiResponse<ConversationListResponse>>, AppError> {
    let result = state.conversation_service.list(&user.id, query).await?;
    Ok(Json(ApiResponse::ok(result)))
}

async fn clone(
    State(state): State<ConversationRouterState>,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<CloneConversationRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<ApiResponse<ConversationResponse>>), AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let conversation = state
        .conversation_service
        .clone_create(&user.id, req)
        .await?;
    Ok((StatusCode::CREATED, Json(ApiResponse::ok(conversation))))
}

async fn get_one(
    State(state): State<ConversationRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<ConversationResponse>>, AppError> {
    let conversation = state.conversation_service.get(&user.id, &id).await?;
    Ok(Json(ApiResponse::ok(conversation)))
}

async fn update(
    State(state): State<ConversationRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    body: Result<Json<UpdateConversationRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<ConversationResponse>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let conversation = state
        .conversation_service
        .update(&user.id, &id, req)
        .await?;
    Ok(Json(ApiResponse::ok(conversation)))
}

async fn delete_one(
    State(state): State<ConversationRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    state.conversation_service.delete(&user.id, &id).await?;
    Ok(Json(ApiResponse::success()))
}

async fn reset(
    State(state): State<ConversationRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    state.conversation_service.reset(&user.id, &id).await?;
    Ok(Json(ApiResponse::success()))
}

async fn associated(
    State(state): State<ConversationRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<Vec<ConversationResponse>>>, AppError> {
    let items = state
        .conversation_service
        .list_associated(&user.id, &id)
        .await?;
    Ok(Json(ApiResponse::ok(items)))
}

async fn list_messages(
    State(state): State<ConversationRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Query(query): Query<ListMessagesQuery>,
) -> Result<Json<ApiResponse<MessageListResponse>>, AppError> {
    let result = state
        .conversation_service
        .list_messages(&user.id, &id, query)
        .await?;
    Ok(Json(ApiResponse::ok(result)))
}

async fn send_message(
    State(state): State<ConversationRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    body: Result<Json<SendMessageRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<ApiResponse<()>>), AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    state
        .conversation_service
        .send_message(&user.id, &id, req, &state.worker_task_manager)
        .await?;
    Ok((StatusCode::ACCEPTED, Json(ApiResponse::success())))
}

async fn stop_stream(
    State(state): State<ConversationRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    state
        .conversation_service
        .stop_stream(&user.id, &id, &state.worker_task_manager)
        .await?;
    Ok(Json(ApiResponse::success()))
}

async fn warmup(
    State(state): State<ConversationRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    state
        .conversation_service
        .warmup(&user.id, &id, &state.worker_task_manager)
        .await?;
    Ok(Json(ApiResponse::success()))
}

async fn search_messages(
    State(state): State<ConversationRouterState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<SearchMessagesQuery>,
) -> Result<Json<ApiResponse<MessageSearchResponse>>, AppError> {
    let result = state
        .conversation_service
        .search_messages(&user.id, query)
        .await?;
    Ok(Json(ApiResponse::ok(result)))
}

// ── Confirmation handlers ─────────────────────────────────────────

async fn list_confirmations(
    State(state): State<ConversationRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<ConfirmationListResponse>>, AppError> {
    let items = state
        .conversation_service
        .list_confirmations(&user.id, &id, &state.worker_task_manager)
        .await?;
    Ok(Json(ApiResponse::ok(items)))
}

#[derive(serde::Deserialize)]
struct ConfirmPathParams {
    id: String,
    #[serde(rename = "callId")]
    call_id: String,
}

async fn confirm(
    State(state): State<ConversationRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(params): Path<ConfirmPathParams>,
    body: Result<Json<ConfirmRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    state
        .conversation_service
        .confirm(
            &user.id,
            &params.id,
            &params.call_id,
            req,
            &state.worker_task_manager,
        )
        .await?;
    Ok(Json(ApiResponse::success()))
}

async fn check_approval(
    State(state): State<ConversationRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Query(query): Query<ApprovalCheckQuery>,
) -> Result<Json<ApiResponse<ApprovalCheckResponse>>, AppError> {
    if query.action.trim().is_empty() {
        return Err(AppError::BadRequest("action must not be empty".into()));
    }

    let result = state
        .conversation_service
        .check_approval(
            &user.id,
            &id,
            &query.action,
            query.command_type.as_deref(),
            &state.worker_task_manager,
        )
        .await?;
    Ok(Json(ApiResponse::ok(result)))
}
