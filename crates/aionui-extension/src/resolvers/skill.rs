use std::path::Path;

use crate::types::{ExtSkill, ResolvedSkill};

/// Resolve a single skill contribution.
///
/// Skill paths are resolved relative to the extension directory.
pub fn resolve_skill(skill: &ExtSkill, extension_name: &str, ext_dir: &Path) -> ResolvedSkill {
    let path = skill
        .path
        .as_ref()
        .map(|p| ext_dir.join(p).to_string_lossy().into_owned());

    ResolvedSkill {
        extension_name: extension_name.to_owned(),
        name: skill.name.clone(),
        description: skill.description.clone(),
        path,
    }
}

/// Resolve all skill contributions from an extension.
pub fn resolve_skills(skills: &[ExtSkill], extension_name: &str, ext_dir: &Path) -> Vec<ResolvedSkill> {
    skills
        .iter()
        .map(|s| resolve_skill(s, extension_name, ext_dir))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_skill_with_path() {
        let skill = ExtSkill {
            name: "code-review".into(),
            description: Some("Code review skill".into()),
            path: Some("skills/code-review".into()),
        };

        let result = resolve_skill(&skill, "my-ext", Path::new("/ext/my-ext"));

        assert_eq!(result.extension_name, "my-ext");
        assert_eq!(result.name, "code-review");
        assert!(result.path.as_ref().unwrap().contains("skills/code-review"));
    }

    #[test]
    fn test_resolve_skill_no_path() {
        let skill = ExtSkill {
            name: "inline-skill".into(),
            description: None,
            path: None,
        };

        let result = resolve_skill(&skill, "my-ext", Path::new("/ext/my-ext"));
        assert!(result.path.is_none());
    }

    #[test]
    fn test_resolve_skills_multiple() {
        let skills = vec![
            ExtSkill {
                name: "a".into(),
                description: None,
                path: None,
            },
            ExtSkill {
                name: "b".into(),
                description: None,
                path: Some("skills/b".into()),
            },
        ];

        let result = resolve_skills(&skills, "my-ext", Path::new("/ext/my-ext"));
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "a");
        assert_eq!(result[1].name, "b");
    }
}
