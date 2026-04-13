mod auth;
mod confirmation;
mod conversation;
mod lifecycle;
mod provider;
mod response;
mod system;
mod websocket;

pub use conversation::{
    CloneConversationRequest, ConversationListResponse, ConversationResponse,
    CreateConversationRequest, ListConversationsQuery, ListMessagesQuery, MessageListResponse,
    MessageResponse, MessageSearchItem, MessageSearchResponse, SearchMessagesQuery,
    SendMessageRequest, UpdateConversationRequest,
};
pub use confirmation::{
    ApprovalCheckQuery, ApprovalCheckResponse, ConfirmRequest, ConfirmationListResponse,
};
pub use auth::{
    AuthStatusResponse, ChangePasswordRequest, LoginRequest, LoginResponse, PublicUser,
    QrLoginRequest, RefreshResponse, RefreshTokenRequest, UserInfoResponse, WsTokenResponse,
};
pub use lifecycle::{
    GitHubReleaseAsset, SystemInfoResponse, UpdateCheckRequest, UpdateCheckResult,
    UpdateReleaseInfo,
};
pub use provider::{
    BedrockAuthMethod, BedrockConfig, CreateProviderRequest, DetectProtocolRequest,
    DetectionSuggestion, FetchModelsRequest, FetchModelsResponse, HealthStatus, KeyTestResult,
    ModelCapability, ModelHealthStatus, ModelInfo, ModelType, MultiKeyResult,
    ProtocolDetectionResponse, ProviderResponse, SuggestionType, UpdateProviderRequest,
};
pub use response::{ApiResponse, ErrorResponse};
pub use system::{
    ClientPreferencesResponse, SystemSettingsResponse, UpdateClientPreferencesRequest,
    UpdateSettingsRequest,
};
pub use websocket::WebSocketMessage;
