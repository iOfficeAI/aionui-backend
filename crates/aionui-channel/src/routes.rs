use std::sync::Arc;

use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Json, State};
use axum::routing::{get, post};
use tracing::warn;

use aionui_api_types::{
    ApiResponse, ApprovePairingRequest, BridgeResponse, ChannelSessionResponse, ChannelUserResponse,
    DisablePluginRequest, EnablePluginRequest, PairingRequestResponse, PluginStatusResponse, RejectPairingRequest,
    RevokeUserRequest, SyncChannelSettingsRequest, TestPluginRequest, TestPluginResponse,
};
use aionui_common::AppError;
use aionui_db::IChannelRepository;

use crate::channel_settings::ChannelSettingsService;
use crate::manager::{ChannelManager, PluginFactory};
use crate::pairing::PairingService;
use crate::session::SessionManager;
use crate::types::{PluginConfig, PluginCredentials, PluginType};

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Router state
// ---------------------------------------------------------------------------

/// Shared state for channel route handlers.
#[derive(Clone)]
pub struct ChannelRouterState {
    pub manager: Arc<ChannelManager>,
    pub pairing_service: Arc<PairingService>,
    pub session_manager: Arc<SessionManager>,
    pub repo: Arc<dyn IChannelRepository>,
    pub plugin_factory: Arc<PluginFactory>,
    pub settings_service: Arc<ChannelSettingsService>,
}

// ---------------------------------------------------------------------------
// Router builder
// ---------------------------------------------------------------------------

/// Build the channel router with all `/api/channel/*` routes.
///
/// All routes require authentication (applied by the caller).
pub fn channel_routes(state: ChannelRouterState) -> Router {
    Router::new()
        // Plugin management
        .route("/api/channel/plugins", get(get_plugin_status))
        .route("/api/channel/plugins/enable", post(enable_plugin))
        .route("/api/channel/plugins/disable", post(disable_plugin))
        .route("/api/channel/plugins/test", post(test_plugin))
        // Pairing management
        .route("/api/channel/pairings", get(get_pending_pairings))
        .route("/api/channel/pairings/approve", post(approve_pairing))
        .route("/api/channel/pairings/reject", post(reject_pairing))
        // User management
        .route("/api/channel/users", get(get_authorized_users))
        .route("/api/channel/users/revoke", post(revoke_user))
        // Session management
        .route("/api/channel/sessions", get(get_active_sessions))
        // Settings sync
        .route("/api/channel/settings/sync", post(sync_channel_settings))
        .with_state(state)
}

/// Build the WeChat login SSE route (feature-gated).
///
/// Separated from `channel_routes` because it's behind the `weixin` feature.
#[cfg(feature = "weixin")]
pub fn weixin_login_route(state: ChannelRouterState) -> Router {
    Router::new()
        .route("/api/channel/weixin/login", get(weixin_login_sse))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Plugin management handlers
// ---------------------------------------------------------------------------

/// `GET /api/channel/plugins` — get status of all registered plugins.
async fn get_plugin_status(
    State(state): State<ChannelRouterState>,
) -> Result<Json<ApiResponse<Vec<PluginStatusResponse>>>, AppError> {
    let statuses = state.manager.get_plugin_status().await?;
    Ok(Json(ApiResponse::ok(statuses)))
}

/// `POST /api/channel/plugins/enable` — enable a plugin with config.
async fn enable_plugin(
    State(state): State<ChannelRouterState>,
    body: Result<Json<EnablePluginRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<BridgeResponse>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;

    match state
        .manager
        .enable_plugin(&req.plugin_id, &req.config, state.plugin_factory.as_ref())
        .await
    {
        Ok(()) => Ok(Json(ApiResponse::ok(BridgeResponse {
            success: true,
            message: Some("Plugin enabled".into()),
            error: None,
        }))),
        Err(e) => {
            warn!(plugin_id = %req.plugin_id, error = %e, "enable plugin failed");
            Ok(Json(ApiResponse::ok(BridgeResponse {
                success: false,
                message: None,
                error: Some(e.to_string()),
            })))
        }
    }
}

/// `POST /api/channel/plugins/disable` — disable a plugin.
async fn disable_plugin(
    State(state): State<ChannelRouterState>,
    body: Result<Json<DisablePluginRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<BridgeResponse>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;

    match state.manager.disable_plugin(&req.plugin_id).await {
        Ok(()) => Ok(Json(ApiResponse::ok(BridgeResponse {
            success: true,
            message: Some("Plugin disabled".into()),
            error: None,
        }))),
        Err(e) => {
            warn!(plugin_id = %req.plugin_id, error = %e, "disable plugin failed");
            Ok(Json(ApiResponse::ok(BridgeResponse {
                success: false,
                message: None,
                error: Some(e.to_string()),
            })))
        }
    }
}

/// `POST /api/channel/plugins/test` — test plugin credentials.
async fn test_plugin(
    State(state): State<ChannelRouterState>,
    body: Result<Json<TestPluginRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<TestPluginResponse>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;

    let config = build_test_config(&req);

    match state
        .manager
        .test_plugin(&req.plugin_id, config, state.plugin_factory.as_ref())
        .await
    {
        Ok(bot_username) => Ok(Json(ApiResponse::ok(TestPluginResponse {
            success: true,
            bot_username,
            error: None,
        }))),
        Err(e) => Ok(Json(ApiResponse::ok(TestPluginResponse {
            success: false,
            bot_username: None,
            error: Some(e.to_string()),
        }))),
    }
}

// ---------------------------------------------------------------------------
// Pairing management handlers
// ---------------------------------------------------------------------------

/// `GET /api/channel/pairings` — get all pending pairing requests.
async fn get_pending_pairings(
    State(state): State<ChannelRouterState>,
) -> Result<Json<ApiResponse<Vec<PairingRequestResponse>>>, AppError> {
    let rows = state.pairing_service.get_pending_pairings().await?;
    let responses: Vec<PairingRequestResponse> = rows
        .into_iter()
        .map(|r| PairingRequestResponse {
            code: r.code,
            platform_user_id: r.platform_user_id,
            platform_type: r.platform_type,
            display_name: r.display_name,
            requested_at: r.requested_at,
            expires_at: r.expires_at,
        })
        .collect();
    Ok(Json(ApiResponse::ok(responses)))
}

/// `POST /api/channel/pairings/approve` — approve a pairing request.
async fn approve_pairing(
    State(state): State<ChannelRouterState>,
    body: Result<Json<ApprovePairingRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<BridgeResponse>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;

    state.pairing_service.approve_pairing(&req.code).await?;

    Ok(Json(ApiResponse::ok(BridgeResponse {
        success: true,
        message: Some("Pairing approved".into()),
        error: None,
    })))
}

/// `POST /api/channel/pairings/reject` — reject a pairing request.
async fn reject_pairing(
    State(state): State<ChannelRouterState>,
    body: Result<Json<RejectPairingRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<BridgeResponse>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;

    state.pairing_service.reject_pairing(&req.code).await?;

    Ok(Json(ApiResponse::ok(BridgeResponse {
        success: true,
        message: Some("Pairing rejected".into()),
        error: None,
    })))
}

// ---------------------------------------------------------------------------
// User management handlers
// ---------------------------------------------------------------------------

/// `GET /api/channel/users` — get all authorized users.
async fn get_authorized_users(
    State(state): State<ChannelRouterState>,
) -> Result<Json<ApiResponse<Vec<ChannelUserResponse>>>, AppError> {
    let rows = state.repo.get_all_users().await?;
    let responses: Vec<ChannelUserResponse> = rows
        .into_iter()
        .map(|r| ChannelUserResponse {
            id: r.id,
            platform_user_id: r.platform_user_id,
            platform_type: r.platform_type,
            display_name: r.display_name,
            authorized_at: r.authorized_at,
            last_active: r.last_active,
        })
        .collect();
    Ok(Json(ApiResponse::ok(responses)))
}

/// `POST /api/channel/users/revoke` — revoke a user's authorization.
///
/// Also cleans up the user's sessions.
async fn revoke_user(
    State(state): State<ChannelRouterState>,
    body: Result<Json<RevokeUserRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<BridgeResponse>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;

    // Clean up sessions first
    state.session_manager.cleanup_user_sessions(&req.user_id).await?;

    // Delete user record
    state.repo.delete_user(&req.user_id).await?;

    Ok(Json(ApiResponse::ok(BridgeResponse {
        success: true,
        message: Some("User revoked".into()),
        error: None,
    })))
}

// ---------------------------------------------------------------------------
// Session management handlers
// ---------------------------------------------------------------------------

/// `GET /api/channel/sessions` — get all active sessions.
async fn get_active_sessions(
    State(state): State<ChannelRouterState>,
) -> Result<Json<ApiResponse<Vec<ChannelSessionResponse>>>, AppError> {
    let rows = state.session_manager.get_active_sessions().await?;
    let responses: Vec<ChannelSessionResponse> = rows
        .into_iter()
        .map(|r| ChannelSessionResponse {
            id: r.id,
            user_id: r.user_id,
            agent_type: r.agent_type,
            conversation_id: r.conversation_id,
            workspace: r.workspace,
            chat_id: r.chat_id,
            created_at: r.created_at,
            last_activity: r.last_activity,
        })
        .collect();
    Ok(Json(ApiResponse::ok(responses)))
}

// ---------------------------------------------------------------------------
// Settings sync handler
// ---------------------------------------------------------------------------

/// `POST /api/channel/settings/sync` — invalidate channel sessions.
///
/// Clears all sessions so they are recreated with the latest
/// agent/model configuration on the next incoming message.
/// Agent/model config is persisted separately via `PUT /api/settings/client`.
async fn sync_channel_settings(
    State(state): State<ChannelRouterState>,
    body: Result<Json<SyncChannelSettingsRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<BridgeResponse>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;

    let _platform = PluginType::from_str_opt(&req.platform)
        .ok_or_else(|| AppError::BadRequest(format!("Invalid platform: {}", req.platform)))?;

    state.session_manager.clear_all_sessions().await?;

    Ok(Json(ApiResponse::ok(BridgeResponse {
        success: true,
        message: Some(format!("Sessions cleared for {}", req.platform)),
        error: None,
    })))
}

// ---------------------------------------------------------------------------
// WeChat login SSE handler
// ---------------------------------------------------------------------------

/// `GET /api/channel/weixin/login` — start WeChat QR code login SSE stream.
#[cfg(feature = "weixin")]
async fn weixin_login_sse(State(_state): State<ChannelRouterState>) -> impl axum::response::IntoResponse {
    use std::convert::Infallible;

    use axum::response::sse::{Event, KeepAlive, Sse};

    use tokio::sync::mpsc;

    use crate::plugins::weixin::WeixinLoginEvent;
    use crate::plugins::weixin::weixin_login_stream;

    let rx = weixin_login_stream();

    let sse_stream = futures_util::stream::unfold(rx, |mut rx: mpsc::Receiver<WeixinLoginEvent>| async move {
        match rx.recv().await {
            Some(event) => {
                let sse_event = Event::default().event(event.event_name()).data(event.to_json_data());
                Some((Ok::<_, Infallible>(sse_event), rx))
            }
            None => None,
        }
    });

    Sse::new(sse_stream).keep_alive(KeepAlive::default())
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Build a `PluginConfig` from a `TestPluginRequest`.
///
/// Maps the `token` and optional `extra_config` fields into the
/// correct credential fields based on the plugin type.
fn build_test_config(req: &TestPluginRequest) -> PluginConfig {
    let mut credentials = PluginCredentials {
        token: None,
        app_id: None,
        app_secret: None,
        encrypt_key: None,
        verification_token: None,
        client_id: None,
        client_secret: None,
        account_id: None,
        bot_token: None,
        extra: HashMap::new(),
    };

    match req.plugin_id.as_str() {
        "lark" => {
            if let Some(ref extra) = req.extra_config {
                credentials.app_id = extra.app_id.clone();
                credentials.app_secret = extra.app_secret.clone();
            }
            credentials.token = Some(req.token.clone());
        }
        "dingtalk" => {
            credentials.client_id = Some(req.token.clone());
            if let Some(ref extra) = req.extra_config {
                credentials.client_secret = extra.app_secret.clone();
            }
        }
        "weixin" => {
            credentials.bot_token = Some(req.token.clone());
            if let Some(ref extra) = req.extra_config {
                credentials.account_id = extra.app_id.clone();
            }
        }
        _ => {
            // Default: use token field (Telegram)
            credentials.token = Some(req.token.clone());
        }
    }

    PluginConfig {
        credentials,
        config: None,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use aionui_api_types::TestPluginExtraConfig;

    #[test]
    fn build_test_config_telegram() {
        let req = TestPluginRequest {
            plugin_id: "telegram".into(),
            token: "bot123:ABC".into(),
            extra_config: None,
        };
        let config = build_test_config(&req);
        assert_eq!(config.credentials.token.as_deref(), Some("bot123:ABC"));
    }

    #[test]
    fn build_test_config_lark() {
        let req = TestPluginRequest {
            plugin_id: "lark".into(),
            token: "xxx".into(),
            extra_config: Some(TestPluginExtraConfig {
                app_id: Some("cli_abc".into()),
                app_secret: Some("secret".into()),
            }),
        };
        let config = build_test_config(&req);
        assert_eq!(config.credentials.app_id.as_deref(), Some("cli_abc"));
        assert_eq!(config.credentials.app_secret.as_deref(), Some("secret"));
        assert_eq!(config.credentials.token.as_deref(), Some("xxx"));
    }

    #[test]
    fn build_test_config_dingtalk() {
        let req = TestPluginRequest {
            plugin_id: "dingtalk".into(),
            token: "client_id_123".into(),
            extra_config: Some(TestPluginExtraConfig {
                app_id: None,
                app_secret: Some("client_secret_456".into()),
            }),
        };
        let config = build_test_config(&req);
        assert_eq!(config.credentials.client_id.as_deref(), Some("client_id_123"));
        assert_eq!(config.credentials.client_secret.as_deref(), Some("client_secret_456"));
    }

    #[test]
    fn build_test_config_weixin() {
        let req = TestPluginRequest {
            plugin_id: "weixin".into(),
            token: "bot_token_xyz".into(),
            extra_config: Some(TestPluginExtraConfig {
                app_id: Some("account_abc".into()),
                app_secret: None,
            }),
        };
        let config = build_test_config(&req);
        assert_eq!(config.credentials.bot_token.as_deref(), Some("bot_token_xyz"));
        assert_eq!(config.credentials.account_id.as_deref(), Some("account_abc"));
    }
}
