use aionui_api_types::{ConversationResponse, MessageResponse, MessageSearchItem};
use aionui_common::{
    AgentType, AppError, ConversationSource, ConversationStatus, MessagePosition, MessageStatus,
    MessageType, ProviderWithModel,
};
use aionui_db::MessageSearchRow;
use aionui_db::models::ConversationRow;
use aionui_db::models::MessageRow;

/// Convert a database row into an API response DTO.
///
/// Parses string enum fields and JSON text fields back into typed values.
pub fn row_to_response(row: ConversationRow) -> Result<ConversationResponse, AppError> {
    let agent_type: AgentType = string_to_enum(&row.r#type)?;
    let status: ConversationStatus = match row.status.as_deref() {
        None | Some("") => ConversationStatus::Finished,
        Some(s) => string_to_enum(s)?,
    };

    let source: Option<ConversationSource> =
        row.source.as_deref().map(string_to_enum).transpose()?;

    let model: Option<ProviderWithModel> = row
        .model
        .as_deref()
        .map(parse_provider_with_model)
        .transpose()?;

    let extra: serde_json::Value = serde_json::from_str(&row.extra)
        .map_err(|e| AppError::Internal(format!("Invalid extra JSON: {e}")))?;

    Ok(ConversationResponse {
        id: row.id,
        name: row.name,
        r#type: agent_type,
        model,
        status,
        source,
        pinned: row.pinned,
        pinned_at: row.pinned_at,
        channel_chat_id: row.channel_chat_id,
        created_at: row.created_at,
        modified_at: row.updated_at,
        extra,
    })
}

/// Parse the model JSON column into `ProviderWithModel`.
///
/// AionUi stores the full provider object (`TProviderWithModel`) which includes
/// fields like `id`, `platform`, `base_url`, `api_key`, `use_model`, and a `model`
/// field that can be an array of model objects. The backend only needs
/// `provider_id`, `model` (the selected model name), and `use_model`.
/// Accepts both snake_case and legacy camelCase key names for backward compatibility.
fn parse_provider_with_model(s: &str) -> Result<ProviderWithModel, AppError> {
    let v: serde_json::Value = serde_json::from_str(s)
        .map_err(|e| AppError::Internal(format!("Invalid model JSON: {e}")))?;

    if let Some(provider_id) = v
        .get("provider_id")
        .or_else(|| v.get("providerId"))
        .and_then(|v| v.as_str())
    {
        let model = v.get("model").and_then(|v| v.as_str()).unwrap_or_default();
        let use_model = v
            .get("use_model")
            .or_else(|| v.get("useModel"))
            .and_then(|v| v.as_str())
            .map(String::from);
        return Ok(ProviderWithModel {
            provider_id: provider_id.to_string(),
            model: model.to_string(),
            use_model,
        });
    }

    if let Some(id) = v.get("id").and_then(|v| v.as_str()) {
        let use_model_str = v
            .get("use_model")
            .or_else(|| v.get("useModel"))
            .and_then(|v| v.as_str())
            .map(String::from);
        return Ok(ProviderWithModel {
            provider_id: id.to_string(),
            model: use_model_str.clone().unwrap_or_default(),
            use_model: use_model_str,
        });
    }

    Err(AppError::Internal(format!(
        "Model JSON missing both 'provider_id'/'providerId' and 'id': {s}"
    )))
}

/// Parse a DB string value into a typed enum via serde.
///
/// e.g. `"acp"` → `AgentType::Acp`
pub fn string_to_enum<T: serde::de::DeserializeOwned>(s: &str) -> Result<T, AppError> {
    serde_json::from_value(serde_json::Value::String(s.to_owned()))
        .map_err(|e| AppError::Internal(format!("Invalid enum value '{s}': {e}")))
}

/// Convert a message database row into an API response DTO.
pub fn row_to_message_response(row: MessageRow) -> Result<MessageResponse, AppError> {
    let msg_type: MessageType = string_to_enum(&row.r#type)?;

    let position: Option<MessagePosition> =
        row.position.as_deref().map(string_to_enum).transpose()?;

    let status: Option<MessageStatus> = row.status.as_deref().map(string_to_enum).transpose()?;

    let content: serde_json::Value = serde_json::from_str(&row.content)
        .map_err(|e| AppError::Internal(format!("Invalid message content JSON: {e}")))?;

    Ok(MessageResponse {
        id: row.id,
        conversation_id: row.conversation_id,
        msg_id: row.msg_id,
        r#type: msg_type,
        content,
        position,
        status,
        hidden: row.hidden,
        created_at: row.created_at,
    })
}

/// Convert a search result row into an API search item DTO.
pub fn search_row_to_item(row: MessageSearchRow) -> MessageSearchItem {
    MessageSearchItem {
        message_id: row.message_id,
        conversation_id: row.conversation_id,
        conversation_name: row.conversation_name,
        r#type: row.r#type,
        content: row.content,
        created_at: row.created_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aionui_common::{AgentType, ConversationSource, ConversationStatus};
    use serde_json::json;

    fn make_row(
        agent_type: &str,
        status: &str,
        source: Option<&str>,
        model_json: Option<&str>,
        extra_json: &str,
    ) -> ConversationRow {
        ConversationRow {
            id: "conv_1".into(),
            user_id: "user_1".into(),
            name: "Test".into(),
            r#type: agent_type.into(),
            extra: extra_json.into(),
            model: model_json.map(|s| s.into()),
            status: Some(status.into()),
            source: source.map(|s| s.into()),
            channel_chat_id: None,
            pinned: false,
            pinned_at: None,
            created_at: 1000,
            updated_at: 2000,
        }
    }

    #[test]
    fn row_to_response_basic() {
        let model = json!({"providerId": "p1", "model": "m1"});
        let row = make_row(
            "acp",
            "pending",
            Some("aionui"),
            Some(&model.to_string()),
            r#"{"workspace": "/project"}"#,
        );
        let resp = row_to_response(row).unwrap();
        assert_eq!(resp.id, "conv_1");
        assert_eq!(resp.r#type, AgentType::Acp);
        assert_eq!(resp.status, ConversationStatus::Pending);
        assert_eq!(resp.source, Some(ConversationSource::Aionui));
        assert_eq!(resp.model.unwrap().model, "m1");
        assert_eq!(resp.extra["workspace"], "/project");
        assert_eq!(resp.modified_at, 2000);
    }

    #[test]
    fn row_to_response_no_source() {
        let row = make_row("acp", "running", None, None, "{}");
        let resp = row_to_response(row).unwrap();
        assert!(resp.source.is_none());
        assert!(resp.model.is_none());
    }

    #[test]
    fn row_to_response_invalid_type() {
        let row = make_row("invalid", "pending", None, None, "{}");
        let err = row_to_response(row).unwrap_err();
        assert!(matches!(err, AppError::Internal(_)));
    }

    #[test]
    fn row_to_response_invalid_extra_json() {
        let row = ConversationRow {
            id: "conv_1".into(),
            user_id: "user_1".into(),
            name: "Test".into(),
            r#type: "acp".into(),
            extra: "not-json".into(),
            model: None,
            status: Some("pending".into()),
            source: None,
            channel_chat_id: None,
            pinned: false,
            pinned_at: None,
            created_at: 1000,
            updated_at: 2000,
        };
        let err = row_to_response(row).unwrap_err();
        assert!(matches!(err, AppError::Internal(_)));
    }

    #[test]
    fn string_to_enum_valid() {
        let agent: AgentType = string_to_enum("acp").unwrap();
        assert_eq!(agent, AgentType::Acp);

        let status: ConversationStatus = string_to_enum("finished").unwrap();
        assert_eq!(status, ConversationStatus::Finished);

        let src: ConversationSource = string_to_enum("telegram").unwrap();
        assert_eq!(src, ConversationSource::Telegram);
    }

    #[test]
    fn string_to_enum_invalid() {
        let err = string_to_enum::<AgentType>("not_valid").unwrap_err();
        assert!(matches!(err, AppError::Internal(_)));
    }

    #[test]
    fn parse_provider_with_model_backend_format() {
        let json =
            r#"{"providerId":"p1","model":"claude-sonnet-4-20250514","useModel":"claude-sonnet"}"#;
        let result = parse_provider_with_model(json).unwrap();
        assert_eq!(result.provider_id, "p1");
        assert_eq!(result.model, "claude-sonnet-4-20250514");
        assert_eq!(result.use_model.as_deref(), Some("claude-sonnet"));
    }

    #[test]
    fn parse_provider_with_model_aionui_format() {
        let json = r#"{"id":"prov_1","platform":"openai","name":"My Provider","baseUrl":"https://api.openai.com","apiKey":"sk-xxx","model":[{"id":"gpt-4","name":"GPT-4"}],"capabilities":["text","vision"],"useModel":"gpt-4-turbo","enabled":true}"#;
        let result = parse_provider_with_model(json).unwrap();
        assert_eq!(result.provider_id, "prov_1");
        assert_eq!(result.model, "gpt-4-turbo");
        assert_eq!(result.use_model.as_deref(), Some("gpt-4-turbo"));
    }

    #[test]
    fn parse_provider_with_model_missing_both_ids() {
        let json = r#"{"name":"invalid"}"#;
        assert!(parse_provider_with_model(json).is_err());
    }

    #[test]
    fn row_with_pinned_at() {
        let row = ConversationRow {
            id: "conv_2".into(),
            user_id: "user_1".into(),
            name: "Pinned".into(),
            r#type: "acp".into(),
            extra: "{}".into(),
            model: None,
            status: Some("pending".into()),
            source: Some("aionui".into()),
            channel_chat_id: Some("chat:1".into()),
            pinned: true,
            pinned_at: Some(5000),
            created_at: 1000,
            updated_at: 3000,
        };
        let resp = row_to_response(row).unwrap();
        assert!(resp.pinned);
        assert_eq!(resp.pinned_at, Some(5000));
        assert_eq!(resp.channel_chat_id.as_deref(), Some("chat:1"));
    }
}
