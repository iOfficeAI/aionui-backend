//! HTTP integration tests for the built-in skills migration surface:
//! `/api/skills/builtin-auto`, `/api/skills/builtin-skill`, `/api/skills`,
//! and the new `/api/skills/materialize-for-agent` (POST + DELETE).
//!
//! Covers the spec's §9.2 scenarios end-to-end through
//! `aionui_app::create_router_with_states` against an in-memory DB.

mod common;

use std::sync::Arc;

use aionui_app::{ModuleStates, build_module_states, create_router_with_states};
use aionui_db::init_database_memory;
use aionui_extension::{ExternalPathsManager, SkillPaths, SkillRouterState};
use axum::http::StatusCode;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

use common::{body_json, delete_with_token, get_with_token, json_with_token, setup_and_login};

// ---------------------------------------------------------------------------
// Fixture — build router with embedded-corpus paths rooted at a temp dir
// ---------------------------------------------------------------------------

struct Fixture {
    app: axum::Router,
    token: String,
    csrf: String,
    data_dir: std::path::PathBuf,
    _tmp: TempDir,
}

/// Build an app whose skill state points at a freshly materialized
/// builtin-skills tree rooted at a temp `data_dir`. `write_skill` can
/// still seed user skills under `{data_dir}/skills/`.
async fn fixture_embedded() -> Fixture {
    // Ensure no env override interferes.
    // SAFETY: tests in this file may mutate this env var across async
    // tasks on the same process. Rust 2024 marks `remove_var` as unsafe
    // for exactly that reason. The var is only read at router-state
    // construction time, and each test calls `fixture_embedded` once at
    // the top, so the mutation is race-free in practice.
    unsafe {
        std::env::remove_var("AIONUI_BUILTIN_SKILLS_PATH");
    }

    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().to_path_buf();

    // Materialize the embedded corpus onto the temp data dir so the
    // per-test router can read it just like production would.
    aionui_extension::materialize_if_needed(
        &data_dir,
        aionui_extension::builtin_skills_corpus(),
        "test-fixture",
    )
    .await
    .expect("failed to materialize embedded builtin skills for test fixture");

    let db = init_database_memory().await.unwrap();
    let services =
        aionui_app::AppServices::from_database_with_data_dir(db, "data".to_string(), false)
            .await
            .unwrap();
    let mut states: ModuleStates = build_module_states(&services).await;

    // Replace the skill state with a deterministic one rooted at tmp.
    // `build_module_states` builds a state pointing at `~/.aionui/`,
    // which is fine for production but unsuitable here.
    let skill_paths = SkillPaths {
        data_dir: data_dir.clone(),
        user_skills_dir: data_dir.join("skills"),
        builtin_skills_dir: data_dir.join("builtin-skills"),
        builtin_rules_dir: data_dir.join("builtin-rules"),
        assistant_rules_dir: data_dir.join("assistant-rules"),
        assistant_skills_dir: data_dir.join("assistant-skills"),
    };
    let ext_paths_mgr =
        Arc::new(ExternalPathsManager::with_file(data_dir.join("paths.json")).await);
    states.skill = SkillRouterState {
        skill_paths,
        external_paths_manager: ext_paths_mgr,
        assistant_dispatcher: states.skill.assistant_dispatcher.clone(),
    };

    let mut app = create_router_with_states(&services, states);
    let (token, csrf) = setup_and_login(&mut app, &services, "builtin-e2e", "StrongP@ss1").await;

    Fixture {
        app,
        token,
        csrf,
        data_dir,
        _tmp: tmp,
    }
}

fn write_user_skill(dir: &std::path::Path, name: &str, desc: &str) {
    let skill_dir = dir.join("skills").join(name);
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {desc}\n---\nBody for {name}."),
    )
    .unwrap();
}

// ===========================================================================
// GET /api/skills/builtin-auto — embedded corpus
// ===========================================================================

#[tokio::test]
async fn builtin_auto_lists_entries_from_embedded_corpus() {
    let fx = fixture_embedded().await;

    let resp = fx
        .app
        .clone()
        .oneshot(get_with_token("/api/skills/builtin-auto", &fx.token))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_json(resp).await;
    assert_eq!(json["success"], true);
    let arr = json["data"].as_array().unwrap();
    assert!(
        arr.len() >= 4,
        "expected ≥4 auto-inject entries, got {}",
        arr.len()
    );
    for item in arr {
        assert!(item["name"].is_string());
        assert!(item["description"].is_string());
        let loc = item["location"].as_str().unwrap();
        assert!(loc.starts_with("auto-inject/"), "location={loc}");
        assert!(loc.ends_with("/SKILL.md"));
    }
}

// ===========================================================================
// POST /api/skills/builtin-skill
// ===========================================================================

#[tokio::test]
async fn builtin_skill_read_auto_inject_returns_frontmatter_content() {
    let fx = fixture_embedded().await;

    let resp = fx
        .app
        .clone()
        .oneshot(json_with_token(
            "POST",
            "/api/skills/builtin-skill",
            json!({"file_name": "auto-inject/cron/SKILL.md"}),
            &fx.token,
            &fx.csrf,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_json(resp).await;
    assert_eq!(json["success"], true);
    let content = json["data"].as_str().unwrap();
    assert!(content.trim_start().starts_with("---"), "content={content}");
}

#[tokio::test]
async fn builtin_skill_read_opt_in_returns_frontmatter_content() {
    let fx = fixture_embedded().await;

    // mermaid is a well-known opt-in skill in the corpus.
    let resp = fx
        .app
        .clone()
        .oneshot(json_with_token(
            "POST",
            "/api/skills/builtin-skill",
            json!({"file_name": "mermaid/SKILL.md"}),
            &fx.token,
            &fx.csrf,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    let content = json["data"].as_str().unwrap();
    assert!(!content.is_empty(), "mermaid SKILL.md is empty");
}

#[tokio::test]
async fn builtin_skill_missing_file_returns_empty_string() {
    let fx = fixture_embedded().await;

    let resp = fx
        .app
        .clone()
        .oneshot(json_with_token(
            "POST",
            "/api/skills/builtin-skill",
            json!({"file_name": "unknown/SKILL.md"}),
            &fx.token,
            &fx.csrf,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["data"], "");
}

#[tokio::test]
async fn builtin_skill_rejects_traversal() {
    let fx = fixture_embedded().await;

    for bad in [
        "../etc/passwd",
        "/etc/passwd",
        "auto-inject/../../escape",
        "",
    ] {
        let resp = fx
            .app
            .clone()
            .oneshot(json_with_token(
                "POST",
                "/api/skills/builtin-skill",
                json!({"file_name": bad}),
                &fx.token,
                &fx.csrf,
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "file_name={bad:?} should be rejected",
        );
    }
}

// ===========================================================================
// GET /api/skills — merged list with relative_location for builtin
// ===========================================================================

#[tokio::test]
async fn list_skills_builtin_entries_carry_relative_location() {
    let fx = fixture_embedded().await;

    // Seed one user skill so the merge is non-trivial.
    write_user_skill(&fx.data_dir, "my-custom", "Custom skill for test");

    let resp = fx
        .app
        .clone()
        .oneshot(get_with_token("/api/skills", &fx.token))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    let arr = json["data"].as_array().unwrap();

    let mut saw_builtin = false;
    let mut saw_custom = false;
    for item in arr {
        match item["source"].as_str().unwrap() {
            "builtin" => {
                saw_builtin = true;
                let rel = item["relative_location"].as_str().unwrap();
                assert!(rel.ends_with("/SKILL.md"));
                let loc = item["location"].as_str().unwrap();
                assert!(
                    loc.contains("builtin-skills"),
                    "builtin location should live under builtin-skills dir: {loc}"
                );
                // The builtin-skills tree is materialized at startup, so
                // SKILL.md must already exist on disk.
                assert!(
                    std::path::Path::new(loc).exists(),
                    "builtin skill file missing on disk: {loc}"
                );
            }
            "custom" => {
                saw_custom = true;
                assert!(item.get("relative_location").is_none());
                assert!(item.get("relative_location").is_none());
                assert_eq!(item["name"], "my-custom");
            }
            other => panic!("unexpected source: {other}"),
        }
    }
    assert!(saw_builtin, "expected at least one builtin entry");
    assert!(saw_custom, "expected the seeded custom entry");
}

// ===========================================================================
// POST /api/skills/materialize-for-agent
// ===========================================================================

#[tokio::test]
async fn materialize_for_agent_writes_named_skill_flat() {
    // Post-snapshot contract: `materialize-for-agent` does NOT implicitly
    // include auto-inject. Callers pass the fully resolved snapshot;
    // naming an auto-inject skill produces a flat `{name}/SKILL.md` entry.
    let fx = fixture_embedded().await;

    let resp = fx
        .app
        .clone()
        .oneshot(json_with_token(
            "POST",
            "/api/skills/materialize-for-agent",
            json!({
                "conversation_id": "conv-happy",
                "skills": ["cron"],
            }),
            &fx.token,
            &fx.csrf,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: Value = body_json(resp).await;
    let dir_path = json["data"]["dir_path"].as_str().unwrap().to_owned();
    let dir = std::path::Path::new(&dir_path);
    assert!(dir.is_absolute(), "dir_path must be absolute: {dir_path}");
    assert!(dir.is_dir(), "agent-skills dir must exist: {dir_path}");
    assert!(
        dir.join("cron").join("SKILL.md").exists(),
        "cron skill not materialized at {dir_path}/cron/SKILL.md",
    );
    // Flat layout — no `auto-inject` wrapper remains.
    assert!(
        !dir.join("auto-inject").exists(),
        "auto-inject wrapper should be flattened away",
    );
}

#[tokio::test]
async fn materialize_for_agent_includes_opt_in_skill() {
    let fx = fixture_embedded().await;

    let resp = fx
        .app
        .clone()
        .oneshot(json_with_token(
            "POST",
            "/api/skills/materialize-for-agent",
            json!({
                "conversation_id": "conv-opt",
                "enabled_skills": ["mermaid"],
            }),
            &fx.token,
            &fx.csrf,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: Value = body_json(resp).await;
    let dir_path = json["data"]["dir_path"].as_str().unwrap().to_owned();
    assert!(
        std::path::Path::new(&dir_path)
            .join("mermaid")
            .join("SKILL.md")
            .exists(),
        "mermaid not materialized at {dir_path}/mermaid/SKILL.md",
    );
}

#[tokio::test]
async fn materialize_for_agent_silently_skips_unknown_skill() {
    let fx = fixture_embedded().await;

    let resp = fx
        .app
        .clone()
        .oneshot(json_with_token(
            "POST",
            "/api/skills/materialize-for-agent",
            json!({
                "conversation_id": "conv-bogus",
                "enabled_skills": ["this-does-not-exist"],
            }),
            &fx.token,
            &fx.csrf,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: Value = body_json(resp).await;
    let dir_path = json["data"]["dir_path"].as_str().unwrap().to_owned();
    assert!(std::path::Path::new(&dir_path).is_dir());
    // Unknown skill does not materialize into a subdir.
    assert!(
        !std::path::Path::new(&dir_path)
            .join("this-does-not-exist")
            .exists(),
        "unknown skill must be silently skipped, not created",
    );
}

#[tokio::test]
async fn materialize_for_agent_fresh_on_each_call() {
    let fx = fixture_embedded().await;

    // First call.
    let resp1 = fx
        .app
        .clone()
        .oneshot(json_with_token(
            "POST",
            "/api/skills/materialize-for-agent",
            json!({"conversation_id": "conv-fresh", "enabled_skills": []}),
            &fx.token,
            &fx.csrf,
        ))
        .await
        .unwrap();
    let json1 = body_json(resp1).await;
    let dir = json1["data"]["dir_path"].as_str().unwrap().to_owned();

    // Sentinel file under the materialized dir.
    std::fs::write(
        std::path::Path::new(&dir).join("sentinel.txt"),
        b"stale-state",
    )
    .unwrap();

    // Second call — must wipe the sentinel.
    let resp2 = fx
        .app
        .clone()
        .oneshot(json_with_token(
            "POST",
            "/api/skills/materialize-for-agent",
            json!({"conversation_id": "conv-fresh", "enabled_skills": []}),
            &fx.token,
            &fx.csrf,
        ))
        .await
        .unwrap();
    let json2 = body_json(resp2).await;
    let dir2 = json2["data"]["dir_path"].as_str().unwrap().to_owned();
    assert_eq!(dir, dir2);
    assert!(
        !std::path::Path::new(&dir2).join("sentinel.txt").exists(),
        "materialize must start fresh",
    );
}

#[tokio::test]
async fn materialize_for_agent_rejects_empty_conversation_id() {
    let fx = fixture_embedded().await;

    let resp = fx
        .app
        .clone()
        .oneshot(json_with_token(
            "POST",
            "/api/skills/materialize-for-agent",
            json!({"conversation_id": "", "enabled_skills": []}),
            &fx.token,
            &fx.csrf,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn materialize_for_agent_rejects_traversal_in_conversation_id() {
    let fx = fixture_embedded().await;

    let resp = fx
        .app
        .clone()
        .oneshot(json_with_token(
            "POST",
            "/api/skills/materialize-for-agent",
            json!({"conversation_id": "../evil", "enabled_skills": []}),
            &fx.token,
            &fx.csrf,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ===========================================================================
// DELETE /api/skills/materialize-for-agent/:conversation_id
// ===========================================================================

#[tokio::test]
async fn cleanup_for_agent_is_idempotent() {
    let fx = fixture_embedded().await;

    // Materialize first so there is something to clean up.
    fx.app
        .clone()
        .oneshot(json_with_token(
            "POST",
            "/api/skills/materialize-for-agent",
            json!({"conversation_id": "conv-del", "enabled_skills": []}),
            &fx.token,
            &fx.csrf,
        ))
        .await
        .unwrap();

    // First delete — dir exists → 200 + removed from disk.
    let resp1 = fx
        .app
        .clone()
        .oneshot(delete_with_token(
            "/api/skills/materialize-for-agent/conv-del",
            &fx.token,
            &fx.csrf,
        ))
        .await
        .unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    assert!(!fx.data_dir.join("agent-skills").join("conv-del").exists());

    // Second delete — dir missing → still 200 (idempotent).
    let resp2 = fx
        .app
        .clone()
        .oneshot(delete_with_token(
            "/api/skills/materialize-for-agent/conv-del",
            &fx.token,
            &fx.csrf,
        ))
        .await
        .unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
}

#[tokio::test]
async fn cleanup_for_agent_unknown_conversation_still_succeeds() {
    let fx = fixture_embedded().await;

    let resp = fx
        .app
        .clone()
        .oneshot(delete_with_token(
            "/api/skills/materialize-for-agent/never-existed",
            &fx.token,
            &fx.csrf,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
