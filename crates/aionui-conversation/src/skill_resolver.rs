//! Abstraction over "what are the auto-inject skill names right now?" so
//! `ConversationService` can compute the initial snapshot without forcing
//! every test setup to stand up a real `SkillPaths`.

use std::sync::Arc;

use async_trait::async_trait;

#[async_trait]
pub trait SkillResolver: Send + Sync {
    /// Returns the sorted list of auto-inject builtin skill names currently
    /// available on this installation.
    async fn auto_inject_names(&self) -> Vec<String>;
}

/// Production adapter backed by `aionui_extension::skill_service`.
pub struct ExtensionSkillResolver {
    paths: Arc<aionui_extension::SkillPaths>,
}

impl ExtensionSkillResolver {
    pub fn new(paths: Arc<aionui_extension::SkillPaths>) -> Self {
        Self { paths }
    }
}

#[async_trait]
impl SkillResolver for ExtensionSkillResolver {
    async fn auto_inject_names(&self) -> Vec<String> {
        match aionui_extension::list_builtin_auto_skills(&self.paths).await {
            Ok(items) => {
                let mut names: Vec<String> = items.into_iter().map(|i| i.name).collect();
                names.sort();
                names
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "auto_inject_names: list_builtin_auto_skills failed, falling back to empty"
                );
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
pub struct FixedSkillResolver {
    pub names: Vec<String>,
}

#[cfg(test)]
#[async_trait]
impl SkillResolver for FixedSkillResolver {
    async fn auto_inject_names(&self) -> Vec<String> {
        self.names.clone()
    }
}
