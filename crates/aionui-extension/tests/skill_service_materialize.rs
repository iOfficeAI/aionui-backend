use aionui_extension::{resolve_skill_paths, skill_service};
use tempfile::TempDir;

/// `BUILTIN_SKILLS_ENV_VAR` is process-global; this test mutates it, so
/// it must not run in parallel with other `skill_service` tests that
/// touch the same env var. Vitest-style serialization inside a single
/// test is sufficient here.
#[tokio::test]
async fn materialize_writes_only_listed_skills() {
    let tmp = TempDir::new().unwrap();
    // Stage two builtin auto-inject skills on disk.
    let auto_dir = tmp.path().join("builtin-skills").join("auto-inject");
    std::fs::create_dir_all(auto_dir.join("cron")).unwrap();
    std::fs::write(
        auto_dir.join("cron").join("SKILL.md"),
        "---\nname: cron\ndescription: \n---",
    )
    .unwrap();
    std::fs::create_dir_all(auto_dir.join("todo")).unwrap();
    std::fs::write(
        auto_dir.join("todo").join("SKILL.md"),
        "---\nname: todo\ndescription: \n---",
    )
    .unwrap();

    // SAFETY: single-threaded test harness.
    unsafe {
        std::env::set_var(
            aionui_extension::BUILTIN_SKILLS_ENV_VAR,
            tmp.path().join("builtin-skills"),
        );
    }
    let paths = resolve_skill_paths(tmp.path(), tmp.path());

    let dir = skill_service::materialize_skills_for_agent(&paths, "conv-1", &["cron".to_owned()])
        .await
        .unwrap();

    assert!(dir.join("cron").join("SKILL.md").exists());
    assert!(
        !dir.join("todo").exists(),
        "todo should not be materialized"
    );

    unsafe {
        std::env::remove_var(aionui_extension::BUILTIN_SKILLS_ENV_VAR);
    }
}
