//! Shared first-message prefix injection for ACP agents.
//!
//! Takes the conversation's first-message content and produces a new content
//! string that may include an `[Assistant Rules]` block with preset context
//! and a skills index. The shape depends on whether the agent's native CLI
//! can read skills from the workspace directly.

use std::sync::Arc;

use crate::skill_manager::{AcpSkillManager, prepare_first_message_with_skills_index};

/// Configuration for the first-message injector.
pub struct InjectionConfig<'a> {
    /// Preset context (assistant-level system prompt injection).
    pub preset_context: Option<&'a str>,
    /// Skills the user explicitly enabled for this session.
    pub enabled_skills: &'a [String],
    /// Builtin auto-inject skills the user disabled.
    pub exclude_builtin_skills: &'a [String],
    /// True iff the agent's native CLI reads skills from the workspace
    /// without needing prompt injection. Derived by callers from
    /// `AcpBackend::native_skills_dirs().is_some()` for ACP, or hardcoded
    /// `false` for aionrs / custom workspace scenarios.
    pub native_skill_support: bool,
    /// True iff the user chose a custom workspace (symlinks may not exist).
    pub custom_workspace: bool,
}

/// Produce the content string to send as the first ACP prompt.
///
/// - If `native_skill_support && !custom_workspace`: **light mode** — only
///   `preset_context` prepended as an `[Assistant Rules]` block (if present).
///   The native CLI handles skill discovery via workspace symlinks.
/// - Else: **heavy mode** — `preset_context` + skills index injected via
///   `prepare_first_message_with_skills_index`.
pub async fn inject_first_message_prefix(
    content: &str,
    manager: &Arc<AcpSkillManager>,
    config: InjectionConfig<'_>,
) -> String {
    let use_native = config.native_skill_support && !config.custom_workspace;

    if use_native {
        // Light mode: only preset_context, no skill discovery
        match config.preset_context {
            Some(ctx) if !ctx.is_empty() => {
                format!("[Assistant Rules]\n{ctx}\n[/Assistant Rules]\n\n{content}")
            }
            _ => content.to_string(),
        }
    } else {
        // Heavy mode: discover skills then inject index
        let enabled = if config.enabled_skills.is_empty() {
            None
        } else {
            Some(config.enabled_skills)
        };
        let exclude = if config.exclude_builtin_skills.is_empty() {
            None
        } else {
            Some(config.exclude_builtin_skills)
        };

        let skills = manager.discover_skills(enabled, exclude).await;

        let has_context = config.preset_context.is_some_and(|s| !s.is_empty());
        if skills.is_empty() && !has_context {
            return content.to_string();
        }

        prepare_first_message_with_skills_index(content, &skills, config.preset_context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aionui_extension::{BUILTIN_SKILLS_ENV_VAR, resolve_skill_paths};
    use std::sync::Mutex;
    use tempfile::TempDir;

    /// `BUILTIN_SKILLS_ENV_VAR` is process-global; serialize tests that set it.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    fn test_mgr(base: &std::path::Path) -> Arc<AcpSkillManager> {
        let paths = Arc::new(resolve_skill_paths(base, base));
        AcpSkillManager::new(paths)
    }

    /// Point the embedded corpus at an empty dir so tests don't pick up
    /// real auto-inject builtin skills.
    struct EmptyBuiltinGuard(std::sync::MutexGuard<'static, ()>);
    impl EmptyBuiltinGuard {
        fn new(empty_path: &std::path::Path) -> Self {
            let g = ENV_MUTEX.lock().unwrap();
            unsafe {
                std::env::set_var(BUILTIN_SKILLS_ENV_VAR, empty_path);
            }
            Self(g)
        }
    }
    impl Drop for EmptyBuiltinGuard {
        fn drop(&mut self) {
            unsafe {
                std::env::remove_var(BUILTIN_SKILLS_ENV_VAR);
            }
        }
    }

    #[tokio::test]
    async fn light_mode_with_preset_context() {
        let tmp = TempDir::new().unwrap();
        let mgr = test_mgr(tmp.path());

        let out = inject_first_message_prefix(
            "Hello",
            &mgr,
            InjectionConfig {
                preset_context: Some("Be concise."),
                enabled_skills: &[],
                exclude_builtin_skills: &[],
                native_skill_support: true,
                custom_workspace: false,
            },
        )
        .await;

        assert!(out.contains("[Assistant Rules]"));
        assert!(out.contains("Be concise."));
        assert!(out.ends_with("Hello"));
    }

    #[tokio::test]
    async fn light_mode_empty_context_passes_through() {
        let tmp = TempDir::new().unwrap();
        let mgr = test_mgr(tmp.path());

        let out = inject_first_message_prefix(
            "Hello",
            &mgr,
            InjectionConfig {
                preset_context: None,
                enabled_skills: &[],
                exclude_builtin_skills: &[],
                native_skill_support: true,
                custom_workspace: false,
            },
        )
        .await;
        assert_eq!(out, "Hello");
    }

    #[tokio::test]
    async fn heavy_mode_no_skills_no_context_passes_through() {
        let tmp = TempDir::new().unwrap();
        let _guard = EmptyBuiltinGuard::new(tmp.path());
        let mgr = test_mgr(tmp.path());

        let out = inject_first_message_prefix(
            "Hello",
            &mgr,
            InjectionConfig {
                preset_context: None,
                enabled_skills: &[],
                exclude_builtin_skills: &[],
                native_skill_support: false,
                custom_workspace: false,
            },
        )
        .await;
        assert_eq!(out, "Hello");
    }

    #[tokio::test]
    async fn heavy_mode_with_preset_context_no_skills() {
        let tmp = TempDir::new().unwrap();
        let _guard = EmptyBuiltinGuard::new(tmp.path());
        let mgr = test_mgr(tmp.path());

        let out = inject_first_message_prefix(
            "Go.",
            &mgr,
            InjectionConfig {
                preset_context: Some("Rule 1."),
                enabled_skills: &[],
                exclude_builtin_skills: &[],
                native_skill_support: false,
                custom_workspace: false,
            },
        )
        .await;

        assert!(out.contains("[Assistant Rules]"));
        assert!(out.contains("Rule 1."));
        assert!(out.ends_with("Go."));
    }

    #[tokio::test]
    async fn custom_workspace_forces_heavy_even_when_native_supported() {
        let tmp = TempDir::new().unwrap();
        let _guard = EmptyBuiltinGuard::new(tmp.path());
        let mgr = test_mgr(tmp.path());

        let out = inject_first_message_prefix(
            "Do stuff",
            &mgr,
            InjectionConfig {
                preset_context: Some("Custom rule"),
                enabled_skills: &[],
                exclude_builtin_skills: &[],
                native_skill_support: true,
                custom_workspace: true, // <-- overrides native
            },
        )
        .await;

        assert!(out.contains("[Assistant Rules]"));
        assert!(out.contains("Custom rule"));
    }
}
