use std::sync::Arc;

use aionui_api_types::{
    CloneConversationRequest, CreateConversationRequest, ListMessagesQuery, SearchMessagesQuery,
    WebSocketMessage,
};
use aionui_common::{ConversationStatus, generate_prefixed_id, now_ms};
use aionui_conversation::ConversationService;
use aionui_db::models::MessageRow;
use aionui_db::{IConversationRepository, SqliteConversationRepository, init_database_memory};
use aionui_realtime::EventBroadcaster;
use serde_json::json;
use std::sync::Mutex;

// ── Test infrastructure ────────────────────────────────────────────

struct TestBroadcaster {
    events: Mutex<Vec<WebSocketMessage<serde_json::Value>>>,
}

impl TestBroadcaster {
    fn new() -> Self {
        Self {
            events: Mutex::new(vec![]),
        }
    }
}

impl EventBroadcaster for TestBroadcaster {
    fn broadcast(&self, event: WebSocketMessage<serde_json::Value>) {
        self.events.lock().unwrap().push(event);
    }
}

async fn setup() -> (
    ConversationService,
    Arc<SqliteConversationRepository>,
    Arc<TestBroadcaster>,
) {
    let db = init_database_memory().await.unwrap();
    let repo = Arc::new(SqliteConversationRepository::new(db.pool().clone()));
    let broadcaster = Arc::new(TestBroadcaster::new());
    let svc = ConversationService::new(repo.clone(), broadcaster.clone());
    (svc, repo, broadcaster)
}

const USER_ID: &str = "system_default_user";

fn make_create_req() -> CreateConversationRequest {
    serde_json::from_value(json!({
        "type": "acp",
        "model": { "provider_id": "p1", "model": "claude-sonnet-4-20250514" },
        "extra": { "workspace": "/home/user/project" }
    }))
    .unwrap()
}

fn make_message(conv_id: &str, content: &str, offset_ms: i64) -> MessageRow {
    MessageRow {
        id: generate_prefixed_id("msg"),
        conversation_id: conv_id.to_string(),
        msg_id: Some(generate_prefixed_id("client")),
        r#type: "text".to_string(),
        content: format!(r#"{{"content":"{content}"}}"#),
        position: Some("right".to_string()),
        status: Some("finish".to_string()),
        hidden: false,
        created_at: now_ms() + offset_ms,
    }
}

// ── T6: Clone conversation ─────────────────────────────────────────

#[tokio::test]
async fn t6_1_clone_from_source() {
    let (svc, _repo, _b) = setup().await;

    // Create source
    let source_req: CreateConversationRequest = serde_json::from_value(json!({
        "type": "acp",
        "name": "Source",
        "model": { "provider_id": "p1", "model": "m1" },
        "extra": { "workspace": "/src", "contextFileName": "ctx.md" }
    }))
    .unwrap();
    let source = svc.create(USER_ID, source_req).await.unwrap();

    // Clone with overrides
    let clone_req: CloneConversationRequest = serde_json::from_value(json!({
        "source_conversation_id": source.id,
        "conversation": {
            "type": "acp",
            "name": "Cloned",
            "model": { "provider_id": "p1", "model": "m1" },
            "extra": { "workspace": "/cloned" }
        }
    }))
    .unwrap();
    let cloned = svc.clone_create(USER_ID, clone_req).await.unwrap();

    assert_ne!(cloned.id, source.id);
    assert_eq!(cloned.name, "Cloned");
    // workspace overridden, contextFileName inherited from source
    assert_eq!(cloned.extra["workspace"], "/cloned");
    assert_eq!(cloned.extra["contextFileName"], "ctx.md");
    assert_eq!(cloned.status, ConversationStatus::Pending);
}

#[tokio::test]
async fn t6_2_clone_without_source() {
    let (svc, _repo, _b) = setup().await;

    let req: CloneConversationRequest = serde_json::from_value(json!({
        "conversation": {
            "type": "acp",
            "name": "Direct",
            "model": { "provider_id": "p1", "model": "m1" },
            "extra": {}
        }
    }))
    .unwrap();
    let resp = svc.clone_create(USER_ID, req).await.unwrap();
    assert_eq!(resp.name, "Direct");
}

#[tokio::test]
async fn t6_3_clone_source_not_found() {
    let (svc, _repo, _b) = setup().await;

    let req: CloneConversationRequest = serde_json::from_value(json!({
        "source_conversation_id": "nonexistent",
        "conversation": {
            "type": "acp",
            "model": { "provider_id": "p1", "model": "m1" },
            "extra": {}
        }
    }))
    .unwrap();
    let err = svc.clone_create(USER_ID, req).await.unwrap_err();
    assert!(matches!(err, aionui_common::AppError::NotFound(_)));
}

#[tokio::test]
async fn t6_4_clone_messages_not_copied() {
    let (svc, repo, _b) = setup().await;

    let source = svc.create(USER_ID, make_create_req()).await.unwrap();
    // Insert message into source
    repo.insert_message(&make_message(&source.id, "hello", 0))
        .await
        .unwrap();

    let clone_req: CloneConversationRequest = serde_json::from_value(json!({
        "source_conversation_id": source.id,
        "conversation": {
            "type": "acp",
            "model": { "provider_id": "p1", "model": "m1" },
            "extra": {}
        }
    }))
    .unwrap();
    let cloned = svc.clone_create(USER_ID, clone_req).await.unwrap();

    // Cloned conversation should have no messages
    let messages = svc
        .list_messages(USER_ID, &cloned.id, ListMessagesQuery::default())
        .await
        .unwrap();
    assert!(messages.items.is_empty());
    assert_eq!(messages.total, 0);
}

// ── T7: Reset conversation ─────────────────────────────────────────

#[tokio::test]
async fn t7_1_reset_clears_messages_and_status() {
    let (svc, repo, _b) = setup().await;

    let conv = svc.create(USER_ID, make_create_req()).await.unwrap();
    // Insert messages
    for i in 0..3 {
        repo.insert_message(&make_message(&conv.id, &format!("msg {i}"), i))
            .await
            .unwrap();
    }

    svc.reset(USER_ID, &conv.id).await.unwrap();

    let fetched = svc.get(USER_ID, &conv.id).await.unwrap();
    assert_eq!(fetched.status, ConversationStatus::Pending);

    let messages = svc
        .list_messages(USER_ID, &conv.id, ListMessagesQuery::default())
        .await
        .unwrap();
    assert!(messages.items.is_empty());
    assert_eq!(messages.total, 0);
}

#[tokio::test]
async fn t7_3_reset_not_found() {
    let (svc, _repo, _b) = setup().await;
    let err = svc.reset(USER_ID, "nonexistent").await.unwrap_err();
    assert!(matches!(err, aionui_common::AppError::NotFound(_)));
}

// ── T8: Message list ───────────────────────────────────────────────

#[tokio::test]
async fn t8_1_empty_messages() {
    let (svc, _repo, _b) = setup().await;
    let conv = svc.create(USER_ID, make_create_req()).await.unwrap();

    let result = svc
        .list_messages(USER_ID, &conv.id, ListMessagesQuery::default())
        .await
        .unwrap();
    assert!(result.items.is_empty());
    assert_eq!(result.total, 0);
}

#[tokio::test]
async fn t8_2_pagination() {
    let (svc, repo, _b) = setup().await;
    let conv = svc.create(USER_ID, make_create_req()).await.unwrap();

    for i in 0..10 {
        repo.insert_message(&make_message(&conv.id, &format!("msg {i}"), i * 100))
            .await
            .unwrap();
    }

    let query = ListMessagesQuery {
        page: Some(1),
        page_size: Some(3),
        order: None,
    };
    let result = svc.list_messages(USER_ID, &conv.id, query).await.unwrap();
    assert_eq!(result.items.len(), 3);
    assert_eq!(result.total, 10);
    assert!(result.has_more);
}

#[tokio::test]
async fn t8_3_asc_order_default() {
    let (svc, repo, _b) = setup().await;
    let conv = svc.create(USER_ID, make_create_req()).await.unwrap();

    for i in 0..3 {
        repo.insert_message(&make_message(&conv.id, &format!("msg {i}"), i * 1000))
            .await
            .unwrap();
    }

    let result = svc
        .list_messages(USER_ID, &conv.id, ListMessagesQuery::default())
        .await
        .unwrap();
    // ASC (default): oldest first
    assert!(result.items[0].created_at <= result.items[1].created_at);
    assert!(result.items[1].created_at <= result.items[2].created_at);
}

#[tokio::test]
async fn t8_4_asc_order() {
    let (svc, repo, _b) = setup().await;
    let conv = svc.create(USER_ID, make_create_req()).await.unwrap();

    for i in 0..3 {
        repo.insert_message(&make_message(&conv.id, &format!("msg {i}"), i * 1000))
            .await
            .unwrap();
    }

    let query = ListMessagesQuery {
        order: Some("ASC".into()),
        ..Default::default()
    };
    let result = svc.list_messages(USER_ID, &conv.id, query).await.unwrap();
    assert!(result.items[0].created_at <= result.items[1].created_at);
    assert!(result.items[1].created_at <= result.items[2].created_at);
}

#[tokio::test]
async fn t8_5_conversation_not_found() {
    let (svc, _repo, _b) = setup().await;
    let err = svc
        .list_messages(USER_ID, "nonexistent", ListMessagesQuery::default())
        .await
        .unwrap_err();
    assert!(matches!(err, aionui_common::AppError::NotFound(_)));
}

// ── T9: Message search ─────────────────────────────────────────────

#[tokio::test]
async fn t9_1_keyword_match() {
    let (svc, repo, _b) = setup().await;
    let conv = svc.create(USER_ID, make_create_req()).await.unwrap();

    repo.insert_message(&make_message(&conv.id, "Rust review report", 0))
        .await
        .unwrap();
    repo.insert_message(&make_message(&conv.id, "Python test", 100))
        .await
        .unwrap();

    let query = SearchMessagesQuery {
        keyword: "review".into(),
        page: None,
        page_size: None,
    };
    let result = svc.search_messages(USER_ID, query).await.unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.total, 1);
}

#[tokio::test]
async fn t9_2_no_match() {
    let (svc, repo, _b) = setup().await;
    let conv = svc.create(USER_ID, make_create_req()).await.unwrap();
    repo.insert_message(&make_message(&conv.id, "hello world", 0))
        .await
        .unwrap();

    let query = SearchMessagesQuery {
        keyword: "xxxxnotexist".into(),
        page: None,
        page_size: None,
    };
    let result = svc.search_messages(USER_ID, query).await.unwrap();
    assert!(result.items.is_empty());
    assert_eq!(result.total, 0);
}

#[tokio::test]
async fn t9_3_search_pagination() {
    let (svc, repo, _b) = setup().await;
    let conv = svc.create(USER_ID, make_create_req()).await.unwrap();

    for i in 0..5 {
        repo.insert_message(&make_message(
            &conv.id,
            &format!("match keyword item {i}"),
            i * 100,
        ))
        .await
        .unwrap();
    }

    let query = SearchMessagesQuery {
        keyword: "keyword".into(),
        page: Some(1),
        page_size: Some(2),
    };
    let result = svc.search_messages(USER_ID, query).await.unwrap();
    assert_eq!(result.items.len(), 2);
    assert_eq!(result.total, 5);
    assert!(result.has_more);
}

#[tokio::test]
async fn t9_4_empty_keyword() {
    let (svc, _repo, _b) = setup().await;

    let query = SearchMessagesQuery {
        keyword: "".into(),
        page: None,
        page_size: None,
    };
    let err = svc.search_messages(USER_ID, query).await.unwrap_err();
    assert!(matches!(err, aionui_common::AppError::BadRequest(_)));
}

// ── T10: Associated conversations ──────────────────────────────────

#[tokio::test]
async fn t10_1_same_workspace() {
    let (svc, _repo, _b) = setup().await;

    let req1: CreateConversationRequest = serde_json::from_value(json!({
        "type": "acp",
        "name": "Conv A",
        "model": { "provider_id": "p1", "model": "m1" },
        "extra": { "workspace": "/shared/path" }
    }))
    .unwrap();
    let conv1 = svc.create(USER_ID, req1).await.unwrap();

    let req2: CreateConversationRequest = serde_json::from_value(json!({
        "type": "acp",
        "name": "Conv B",
        "model": { "provider_id": "p1", "model": "m1" },
        "extra": { "workspace": "/shared/path" }
    }))
    .unwrap();
    let conv2 = svc.create(USER_ID, req2).await.unwrap();

    // Different workspace
    let req3: CreateConversationRequest = serde_json::from_value(json!({
        "type": "acp",
        "name": "Conv C",
        "model": { "provider_id": "p1", "model": "m1" },
        "extra": { "workspace": "/other/path" }
    }))
    .unwrap();
    svc.create(USER_ID, req3).await.unwrap();

    let associated = svc.list_associated(USER_ID, &conv1.id).await.unwrap();
    assert_eq!(associated.len(), 1);
    assert_eq!(associated[0].id, conv2.id);
}

#[tokio::test]
async fn t10_2_no_associated() {
    let (svc, _repo, _b) = setup().await;

    let req: CreateConversationRequest = serde_json::from_value(json!({
        "type": "acp",
        "model": { "provider_id": "p1", "model": "m1" },
        "extra": { "workspace": "/unique/path" }
    }))
    .unwrap();
    let conv = svc.create(USER_ID, req).await.unwrap();

    let associated = svc.list_associated(USER_ID, &conv.id).await.unwrap();
    assert!(associated.is_empty());
}

#[tokio::test]
async fn t10_3_associated_not_found() {
    let (svc, _repo, _b) = setup().await;
    let err = svc
        .list_associated(USER_ID, "nonexistent")
        .await
        .unwrap_err();
    assert!(matches!(err, aionui_common::AppError::NotFound(_)));
}

// ── T12: Boundary scenarios ────────────────────────────────────────

#[tokio::test]
async fn t12_4_search_sql_injection() {
    let (svc, repo, _b) = setup().await;
    let conv = svc.create(USER_ID, make_create_req()).await.unwrap();
    repo.insert_message(&make_message(&conv.id, "safe content", 0))
        .await
        .unwrap();

    let query = SearchMessagesQuery {
        keyword: "'; DROP TABLE messages; --".into(),
        page: None,
        page_size: None,
    };
    // Should return empty results, not crash
    let result = svc.search_messages(USER_ID, query).await.unwrap();
    assert!(result.items.is_empty());
}

// ── Ownership cross-cutting ────────────────────────────────────────

#[tokio::test]
async fn messages_wrong_user_returns_not_found() {
    let (svc, repo, _b) = setup().await;
    let conv = svc.create(USER_ID, make_create_req()).await.unwrap();
    repo.insert_message(&make_message(&conv.id, "hello", 0))
        .await
        .unwrap();

    let err = svc
        .list_messages("other_user", &conv.id, ListMessagesQuery::default())
        .await
        .unwrap_err();
    assert!(matches!(err, aionui_common::AppError::NotFound(_)));
}

#[tokio::test]
async fn reset_wrong_user_returns_not_found() {
    let (svc, _repo, _b) = setup().await;
    let conv = svc.create(USER_ID, make_create_req()).await.unwrap();

    let err = svc.reset("other_user", &conv.id).await.unwrap_err();
    assert!(matches!(err, aionui_common::AppError::NotFound(_)));
}
