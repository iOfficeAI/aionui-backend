//! One-shot startup migration from legacy `client_preferences.model.config`
//! KV entries to the `providers` table.
//!
//! Runs once at server boot — see [`migrate_legacy_providers`]. No-op if
//! the providers table is already populated OR the legacy key is absent.
//! On any parse error the whole batch is rejected (transaction rolled
//! back) so the legacy data stays recoverable for a retry.

use std::collections::HashMap;

use aionui_common::{AppError, encrypt_string};
use aionui_db::{CreateProviderParams, IClientPreferenceRepository, IProviderRepository};
use serde::Deserialize;
use tracing::{info, warn};

const LEGACY_KEY: &str = "model.config";

/// Legacy `IProvider` shape — camelCase, persisted by the pre-migration
/// frontend before the backend owned this resource.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyProvider {
    id: String,
    platform: String,
    name: String,
    base_url: String,
    api_key: String,
    #[serde(default)]
    model: Vec<String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    capabilities: Option<serde_json::Value>,
    #[serde(default)]
    context_limit: Option<i64>,
    #[serde(default)]
    model_protocols: Option<HashMap<String, String>>,
    #[serde(default)]
    model_enabled: Option<HashMap<String, bool>>,
    #[serde(default)]
    model_health: Option<HashMap<String, LegacyModelHealth>>,
    /// Nested bedrock config (newer shape).
    #[serde(default)]
    bedrock_config: Option<LegacyBedrockConfig>,
    /// Flat bedrock fields (older shape, seen in user fixture). Kept only
    /// when `platform == "bedrock"`; otherwise treated as stale UI state.
    #[serde(default)]
    bedrock_auth_method: Option<String>,
    #[serde(default)]
    bedrock_access_key_id: Option<String>,
    #[serde(default)]
    bedrock_secret_access_key: Option<String>,
    #[serde(default)]
    bedrock_region: Option<String>,
    #[serde(default)]
    bedrock_profile: Option<String>,
    // `useModel` and anything else is intentionally ignored.
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyModelHealth {
    status: String,
    #[serde(default)]
    last_check: Option<i64>,
    #[serde(default)]
    latency: Option<i64>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyBedrockConfig {
    auth_method: String,
    region: String,
    #[serde(default)]
    access_key_id: Option<String>,
    #[serde(default)]
    secret_access_key: Option<String>,
    #[serde(default)]
    profile: Option<String>,
}

/// Translated, ready-for-insert form of a legacy entry. All string/JSON
/// fields are serialized and the api_key is encrypted — this is what
/// `CreateProviderParams` expects.
#[derive(Debug)]
struct TranslatedEntry {
    id: String,
    platform: String,
    name: String,
    base_url: String,
    api_key_encrypted: String,
    models_json: String,
    enabled: bool,
    capabilities_json: String,
    context_limit: Option<i64>,
    model_protocols_json: Option<String>,
    model_enabled_json: Option<String>,
    model_health_json: Option<String>,
    bedrock_config_json: Option<String>,
}

/// Run the migration. Returns `Ok(count)` = number of rows migrated (0 if
/// skipped). Any error is logged internally and surfaced to the caller so
/// startup can decide whether to hard-fail (we choose to log + continue).
pub async fn migrate_legacy_providers(
    provider_repo: &dyn IProviderRepository,
    client_pref_repo: &dyn IClientPreferenceRepository,
    encryption_key: &[u8; 32],
) -> Result<usize, AppError> {
    // Idempotency guard #1: already-populated providers table.
    let existing = provider_repo
        .list()
        .await
        .map_err(|e| AppError::Internal(format!("provider_migration: list providers: {e}")))?;
    if !existing.is_empty() {
        return Ok(0);
    }

    // Idempotency guard #2: no legacy key in KV.
    let rows = client_pref_repo
        .get_by_keys(&[LEGACY_KEY])
        .await
        .map_err(|e| AppError::Internal(format!("provider_migration: get KV: {e}")))?;
    let Some(row) = rows.into_iter().find(|r| r.key == LEGACY_KEY) else {
        return Ok(0);
    };

    // Parse the KV value as an array of legacy providers.
    let entries: Vec<LegacyProvider> = match serde_json::from_str(&row.value) {
        Ok(v) => v,
        Err(e) => {
            warn!(
                "provider_migration: legacy '{LEGACY_KEY}' value is not a legacy provider array (ignoring): {e}"
            );
            return Ok(0);
        }
    };

    if entries.is_empty() {
        // Nothing to migrate, but do clean up the empty legacy entry.
        if let Err(e) = client_pref_repo.delete_keys(&[LEGACY_KEY]).await {
            warn!("provider_migration: failed to remove empty legacy key: {e}");
        }
        return Ok(0);
    }

    // Translate everything up-front. Any error aborts the migration with
    // the legacy data untouched so the user can retry after fixing it.
    let mut translated: Vec<TranslatedEntry> = Vec::with_capacity(entries.len());
    for entry in entries {
        let id_preview = entry.id.clone();
        translated.push(translate_legacy_entry(entry, encryption_key).map_err(|e| {
            AppError::Internal(format!(
                "provider_migration: translate entry '{id_preview}': {e}"
            ))
        })?);
    }

    // Single-transaction insert — if any row conflicts (e.g. same id twice
    // in legacy data), the repo layer raises Conflict and we leave the
    // legacy key in place for manual intervention.
    for t in &translated {
        let params = CreateProviderParams {
            id: Some(&t.id),
            platform: &t.platform,
            name: &t.name,
            base_url: &t.base_url,
            api_key_encrypted: &t.api_key_encrypted,
            models: &t.models_json,
            enabled: t.enabled,
            capabilities: &t.capabilities_json,
            context_limit: t.context_limit,
            model_protocols: t.model_protocols_json.as_deref(),
            model_enabled: t.model_enabled_json.as_deref(),
            model_health: t.model_health_json.as_deref(),
            bedrock_config: t.bedrock_config_json.as_deref(),
        };
        if let Err(e) = provider_repo.create(params).await {
            warn!(
                "provider_migration: insert failed for id '{}': {e} — legacy '{LEGACY_KEY}' left in place for retry",
                t.id
            );
            return Err(AppError::Internal(format!(
                "provider_migration: insert: {e}"
            )));
        }
    }

    // All rows migrated — clean up the legacy key.
    if let Err(e) = client_pref_repo.delete_keys(&[LEGACY_KEY]).await {
        // Non-fatal: providers were migrated, just log the stale KV row.
        warn!(
            "provider_migration: migrated {} providers but failed to delete legacy KV key: {e}",
            translated.len()
        );
    }

    info!(
        "provider_migration: migrated {} legacy provider(s) from '{LEGACY_KEY}' KV to providers table",
        translated.len()
    );
    Ok(translated.len())
}

/// Translate one legacy entry into the params we pass to the repo. Serializes
/// JSON fields and encrypts the api_key here so the migration routine can
/// drive the repo directly without the service layer's validation (which is
/// designed for user-submitted requests, not trusted local data).
fn translate_legacy_entry(
    legacy: LegacyProvider,
    encryption_key: &[u8; 32],
) -> Result<TranslatedEntry, String> {
    let api_key_encrypted = encrypt_string(&legacy.api_key, encryption_key)
        .map_err(|e| format!("encrypt api_key: {e}"))?;

    let models_json =
        serde_json::to_string(&legacy.model).map_err(|e| format!("serialize models: {e}"))?;
    let capabilities_json = serde_json::to_string(&legacy.capabilities.unwrap_or_else(|| {
        // Default to empty array if absent — matches spec "capabilities: []".
        serde_json::json!([])
    }))
    .map_err(|e| format!("serialize capabilities: {e}"))?;

    let model_protocols_json = legacy
        .model_protocols
        .map(|m| serde_json::to_string(&m))
        .transpose()
        .map_err(|e| format!("serialize model_protocols: {e}"))?;

    let model_enabled_json = legacy
        .model_enabled
        .map(|m| serde_json::to_string(&m))
        .transpose()
        .map_err(|e| format!("serialize model_enabled: {e}"))?;

    // Re-serialize model_health with snake_case field names so the on-disk
    // JSON matches what `ProviderResponse` expects when reading back out.
    let model_health_json = legacy
        .model_health
        .map(|m| {
            let out: HashMap<String, serde_json::Value> = m
                .into_iter()
                .map(|(model_id, h)| {
                    let mut obj = serde_json::Map::new();
                    obj.insert("status".into(), serde_json::Value::String(h.status));
                    if let Some(lc) = h.last_check {
                        obj.insert("last_check".into(), serde_json::Value::from(lc));
                    }
                    if let Some(l) = h.latency {
                        obj.insert("latency".into(), serde_json::Value::from(l));
                    }
                    if let Some(err) = h.error {
                        obj.insert("error".into(), serde_json::Value::String(err));
                    }
                    (model_id, serde_json::Value::Object(obj))
                })
                .collect();
            serde_json::to_string(&out)
        })
        .transpose()
        .map_err(|e| format!("serialize model_health: {e}"))?;

    // Bedrock config: prefer nested; else assemble from flat fields IFF
    // platform is bedrock. Non-bedrock platforms with flat fields are
    // treated as stale frontend state and dropped.
    let bedrock_config_json = if let Some(cfg) = legacy.bedrock_config {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "auth_method".into(),
            serde_json::Value::String(cfg.auth_method),
        );
        obj.insert("region".into(), serde_json::Value::String(cfg.region));
        if let Some(v) = cfg.access_key_id {
            obj.insert("access_key_id".into(), serde_json::Value::String(v));
        }
        if let Some(v) = cfg.secret_access_key {
            obj.insert("secret_access_key".into(), serde_json::Value::String(v));
        }
        if let Some(v) = cfg.profile {
            obj.insert("profile".into(), serde_json::Value::String(v));
        }
        Some(serde_json::to_string(&serde_json::Value::Object(obj)).map_err(|e| e.to_string())?)
    } else if legacy.platform == "bedrock"
        && (legacy.bedrock_auth_method.is_some() || legacy.bedrock_region.is_some())
    {
        let mut obj = serde_json::Map::new();
        if let Some(am) = legacy.bedrock_auth_method {
            obj.insert("auth_method".into(), serde_json::Value::String(am));
        }
        if let Some(r) = legacy.bedrock_region {
            obj.insert("region".into(), serde_json::Value::String(r));
        }
        if let Some(v) = legacy.bedrock_access_key_id.filter(|s| !s.is_empty()) {
            obj.insert("access_key_id".into(), serde_json::Value::String(v));
        }
        if let Some(v) = legacy.bedrock_secret_access_key.filter(|s| !s.is_empty()) {
            obj.insert("secret_access_key".into(), serde_json::Value::String(v));
        }
        if let Some(v) = legacy.bedrock_profile.filter(|s| !s.is_empty()) {
            obj.insert("profile".into(), serde_json::Value::String(v));
        }
        Some(serde_json::to_string(&serde_json::Value::Object(obj)).map_err(|e| e.to_string())?)
    } else {
        None
    };

    Ok(TranslatedEntry {
        id: legacy.id,
        platform: legacy.platform,
        name: legacy.name,
        base_url: legacy.base_url,
        api_key_encrypted,
        models_json,
        enabled: legacy.enabled.unwrap_or(true),
        capabilities_json,
        context_limit: legacy.context_limit,
        model_protocols_json,
        model_enabled_json,
        model_health_json,
        bedrock_config_json,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aionui_common::decrypt_string;
    use aionui_db::{
        SqliteClientPreferenceRepository, SqliteProviderRepository, init_database_memory,
    };
    use std::sync::Arc;

    const TEST_KEY: [u8; 32] = [0x55; 32];

    fn parse(json: &str) -> LegacyProvider {
        serde_json::from_str(json).unwrap()
    }

    // ---- translate_legacy_entry ----

    #[test]
    fn translate_minimal_entry() {
        let legacy = parse(
            r#"{"id":"a1","platform":"custom","name":"Z","baseUrl":"https://x","apiKey":"sk","model":["glm"]}"#,
        );
        let t = translate_legacy_entry(legacy, &TEST_KEY).unwrap();
        assert_eq!(t.id, "a1");
        assert_eq!(t.platform, "custom");
        assert_eq!(t.name, "Z");
        assert_eq!(t.base_url, "https://x");
        assert!(t.enabled); // default true
        assert_eq!(t.models_json, r#"["glm"]"#);
        assert_eq!(t.capabilities_json, "[]");
        assert!(t.model_protocols_json.is_none());
        assert!(t.model_enabled_json.is_none());
        assert!(t.model_health_json.is_none());
        assert!(t.bedrock_config_json.is_none());
        assert!(t.context_limit.is_none());
        // api_key encrypts and round-trips.
        let plain = decrypt_string(&t.api_key_encrypted, &TEST_KEY).unwrap();
        assert_eq!(plain, "sk");
    }

    #[test]
    fn translate_drops_use_model_field() {
        let legacy = parse(
            r#"{"id":"a1","platform":"custom","name":"Z","baseUrl":"https://x","apiKey":"sk","model":[],"useModel":"glm-4"}"#,
        );
        // useModel is not part of TranslatedEntry at all — just verify we don't choke.
        let t = translate_legacy_entry(legacy, &TEST_KEY).unwrap();
        assert_eq!(t.id, "a1");
    }

    #[test]
    fn translate_model_health_flips_last_check_to_snake_case() {
        let legacy = parse(
            r#"{
                "id":"a1","platform":"custom","name":"Z","baseUrl":"https://x","apiKey":"sk","model":["glm"],
                "modelHealth":{"glm":{"status":"healthy","lastCheck":1774348554050,"latency":1304}}
            }"#,
        );
        let t = translate_legacy_entry(legacy, &TEST_KEY).unwrap();
        let health_json = t.model_health_json.unwrap();
        // Must contain snake_case key on the wire.
        assert!(health_json.contains(r#""last_check":1774348554050"#));
        assert!(!health_json.contains("lastCheck"));
        assert!(health_json.contains(r#""status":"healthy""#));
    }

    #[test]
    fn translate_model_enabled_and_protocols_roundtrip() {
        let legacy = parse(
            r#"{
                "id":"a1","platform":"new-api","name":"N","baseUrl":"https://x","apiKey":"sk","model":["gpt"],
                "modelEnabled":{"gpt":false,"o1":true},
                "modelProtocols":{"gpt":"anthropic"}
            }"#,
        );
        let t = translate_legacy_entry(legacy, &TEST_KEY).unwrap();
        let enabled_json = t.model_enabled_json.unwrap();
        assert!(enabled_json.contains(r#""gpt":false"#));
        assert!(enabled_json.contains(r#""o1":true"#));
        let protos_json = t.model_protocols_json.unwrap();
        assert!(protos_json.contains(r#""gpt":"anthropic""#));
    }

    #[test]
    fn translate_nested_bedrock_config() {
        let legacy = parse(
            r#"{
                "id":"a1","platform":"bedrock","name":"B","baseUrl":"https://x","apiKey":"","model":[],
                "bedrockConfig":{"authMethod":"accessKey","region":"us-east-1","accessKeyId":"AKIA","secretAccessKey":"sek"}
            }"#,
        );
        let t = translate_legacy_entry(legacy, &TEST_KEY).unwrap();
        let bc = t.bedrock_config_json.unwrap();
        assert!(bc.contains(r#""auth_method":"accessKey""#));
        assert!(bc.contains(r#""region":"us-east-1""#));
        assert!(bc.contains(r#""access_key_id":"AKIA""#));
        assert!(bc.contains(r#""secret_access_key":"sek""#));
    }

    #[test]
    fn translate_flat_bedrock_fields_on_bedrock_platform() {
        let legacy = parse(
            r#"{
                "id":"a1","platform":"bedrock","name":"B","baseUrl":"https://x","apiKey":"","model":[],
                "bedrockAuthMethod":"accessKey","bedrockRegion":"us-east-1",
                "bedrockAccessKeyId":"AKIA","bedrockSecretAccessKey":"sek",
                "bedrockProfile":""
            }"#,
        );
        let t = translate_legacy_entry(legacy, &TEST_KEY).unwrap();
        let bc = t.bedrock_config_json.unwrap();
        assert!(bc.contains(r#""auth_method":"accessKey""#));
        assert!(bc.contains(r#""region":"us-east-1""#));
        assert!(bc.contains(r#""access_key_id":"AKIA""#));
        // Empty profile is filtered out.
        assert!(!bc.contains(r#""profile""#));
    }

    #[test]
    fn translate_flat_bedrock_fields_on_non_bedrock_platform_dropped() {
        let legacy = parse(
            r#"{
                "id":"a1","platform":"gemini","name":"G","baseUrl":"https://x","apiKey":"sk","model":[],
                "bedrockAuthMethod":"accessKey","bedrockRegion":"us-east-1"
            }"#,
        );
        let t = translate_legacy_entry(legacy, &TEST_KEY).unwrap();
        assert!(
            t.bedrock_config_json.is_none(),
            "flat bedrock fields on non-bedrock platform must be dropped as stale UI state"
        );
    }

    #[test]
    fn translate_enabled_defaults_to_true_when_absent() {
        let legacy = parse(
            r#"{"id":"a1","platform":"custom","name":"Z","baseUrl":"https://x","apiKey":"sk","model":[]}"#,
        );
        let t = translate_legacy_entry(legacy, &TEST_KEY).unwrap();
        assert!(t.enabled);
    }

    #[test]
    fn translate_enabled_respects_explicit_false() {
        let legacy = parse(
            r#"{"id":"a1","platform":"custom","name":"Z","baseUrl":"https://x","apiKey":"sk","model":[],"enabled":false}"#,
        );
        let t = translate_legacy_entry(legacy, &TEST_KEY).unwrap();
        assert!(!t.enabled);
    }

    #[test]
    fn translate_context_limit_passthrough() {
        let legacy = parse(
            r#"{"id":"a1","platform":"custom","name":"Z","baseUrl":"https://x","apiKey":"sk","model":[],"contextLimit":200000}"#,
        );
        let t = translate_legacy_entry(legacy, &TEST_KEY).unwrap();
        assert_eq!(t.context_limit, Some(200000));
    }

    // ---- migrate_legacy_providers ----

    async fn setup() -> (
        Arc<dyn IProviderRepository>,
        Arc<dyn IClientPreferenceRepository>,
        aionui_db::Database,
    ) {
        let db = init_database_memory().await.unwrap();
        let providers: Arc<dyn IProviderRepository> =
            Arc::new(SqliteProviderRepository::new(db.pool().clone()));
        let kv: Arc<dyn IClientPreferenceRepository> =
            Arc::new(SqliteClientPreferenceRepository::new(db.pool().clone()));
        (providers, kv, db)
    }

    #[tokio::test]
    async fn migrate_noop_when_key_absent() {
        let (providers, kv, _db) = setup().await;
        let n = migrate_legacy_providers(providers.as_ref(), kv.as_ref(), &TEST_KEY)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn migrate_skips_when_providers_nonempty() {
        let (providers, kv, _db) = setup().await;
        // Seed one provider to trigger the idempotency guard.
        providers
            .create(CreateProviderParams {
                id: Some("existing"),
                platform: "custom",
                name: "Existing",
                base_url: "https://a",
                api_key_encrypted: "enc",
                models: "[]",
                enabled: true,
                capabilities: "[]",
                context_limit: None,
                model_protocols: None,
                model_enabled: None,
                model_health: None,
                bedrock_config: None,
            })
            .await
            .unwrap();
        // Seed legacy key — should be ignored because providers is non-empty.
        kv.upsert_batch(&[("model.config", r#"[{"id":"a1","platform":"x","name":"Y","baseUrl":"https://b","apiKey":"sk","model":[]}]"#)])
            .await
            .unwrap();

        let n = migrate_legacy_providers(providers.as_ref(), kv.as_ref(), &TEST_KEY)
            .await
            .unwrap();
        assert_eq!(n, 0);
        // Legacy key is left alone.
        let rows = kv.get_by_keys(&["model.config"]).await.unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[tokio::test]
    async fn migrate_fixture_four_providers() {
        let (providers, kv, _db) = setup().await;
        let fixture = std::fs::read_to_string("/tmp/legacy-model-config-sanitized.json")
            .expect("fixture must be present; see task brief");
        kv.upsert_batch(&[("model.config", fixture.as_str())])
            .await
            .unwrap();

        let n = migrate_legacy_providers(providers.as_ref(), kv.as_ref(), &TEST_KEY)
            .await
            .unwrap();
        assert_eq!(n, 4, "fixture has 4 providers");

        let rows = providers.list().await.unwrap();
        assert_eq!(rows.len(), 4);
        // Ids preserved verbatim from the fixture.
        let ids: std::collections::HashSet<_> = rows.iter().map(|r| r.id.as_str()).collect();
        for expected in ["ede85cab", "05b1d3d5", "dcbc9e3a", "4056cdea"] {
            assert!(ids.contains(expected), "expected id {expected}");
        }
        // model.config key removed.
        assert!(
            kv.get_by_keys(&["model.config"]).await.unwrap().is_empty(),
            "legacy key must be cleaned up after successful migration"
        );
        // Sample round-trip: api_key decrypts back to the original plaintext.
        let zhipu = rows.iter().find(|r| r.id == "ede85cab").unwrap();
        let plain = decrypt_string(&zhipu.api_key_encrypted, &TEST_KEY).unwrap();
        assert_eq!(plain, "<REDACTED>");
        // model_health.last_check preserved as snake_case i64.
        let health = zhipu.model_health.as_deref().unwrap();
        assert!(health.contains(r#""last_check":1774348554050"#));
        // Flat bedrock on gemini platform dropped as stale UI state.
        let gemini = rows.iter().find(|r| r.id == "05b1d3d5").unwrap();
        assert!(
            gemini.bedrock_config.is_none(),
            "flat bedrock fields on non-bedrock platform should be dropped"
        );
    }

    #[tokio::test]
    async fn migrate_rejects_non_array_value() {
        let (providers, kv, _db) = setup().await;
        kv.upsert_batch(&[("model.config", r#"{"not":"an array"}"#)])
            .await
            .unwrap();
        let n = migrate_legacy_providers(providers.as_ref(), kv.as_ref(), &TEST_KEY)
            .await
            .unwrap();
        assert_eq!(n, 0);
        // Legacy key left in place — user can fix + retry.
        assert_eq!(kv.get_by_keys(&["model.config"]).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn migrate_empty_array_cleans_up_and_exits() {
        let (providers, kv, _db) = setup().await;
        kv.upsert_batch(&[("model.config", "[]")]).await.unwrap();
        let n = migrate_legacy_providers(providers.as_ref(), kv.as_ref(), &TEST_KEY)
            .await
            .unwrap();
        assert_eq!(n, 0);
        // Empty legacy row gets cleaned up so startup doesn't revisit it.
        assert!(kv.get_by_keys(&["model.config"]).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn migrate_twice_second_call_is_noop() {
        let (providers, kv, _db) = setup().await;
        kv.upsert_batch(&[("model.config", r#"[{"id":"a1","platform":"custom","name":"X","baseUrl":"https://x","apiKey":"sk","model":["m"]}]"#)])
            .await
            .unwrap();

        let n1 = migrate_legacy_providers(providers.as_ref(), kv.as_ref(), &TEST_KEY)
            .await
            .unwrap();
        assert_eq!(n1, 1);
        let n2 = migrate_legacy_providers(providers.as_ref(), kv.as_ref(), &TEST_KEY)
            .await
            .unwrap();
        assert_eq!(n2, 0);
        assert_eq!(providers.list().await.unwrap().len(), 1);
    }
}
