use crate::types::{ExtSettingsTab, ResolvedSettingsTab};

/// Resolve a single settings tab contribution.
///
/// Position information (relativeTo, placement) is preserved for the
/// frontend to handle insertion ordering.
pub fn resolve_settings_tab(tab: &ExtSettingsTab, extension_name: &str) -> ResolvedSettingsTab {
    ResolvedSettingsTab {
        extension_name: extension_name.to_owned(),
        id: tab.id.clone(),
        label: tab.label.clone(),
        icon: tab.icon.clone(),
        url: tab.url.clone(),
        position: tab.position.clone(),
    }
}

/// Resolve all settings tab contributions from an extension.
pub fn resolve_settings_tabs(tabs: &[ExtSettingsTab], extension_name: &str) -> Vec<ResolvedSettingsTab> {
    tabs.iter().map(|t| resolve_settings_tab(t, extension_name)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SettingsTabPosition;

    #[test]
    fn test_resolve_settings_tab_with_position() {
        let tab = ExtSettingsTab {
            id: "my-settings".into(),
            label: "My Settings".into(),
            icon: Some("gear".into()),
            url: "aion-asset://my-ext/settings.html".into(),
            position: Some(SettingsTabPosition {
                relative_to: "general".into(),
                placement: "after".into(),
            }),
        };

        let result = resolve_settings_tab(&tab, "my-ext");

        assert_eq!(result.extension_name, "my-ext");
        assert_eq!(result.id, "my-settings");
        assert_eq!(result.label, "My Settings");
        let pos = result.position.unwrap();
        assert_eq!(pos.relative_to, "general");
        assert_eq!(pos.placement, "after");
    }

    #[test]
    fn test_resolve_settings_tab_no_position() {
        let tab = ExtSettingsTab {
            id: "plain-tab".into(),
            label: "Plain".into(),
            icon: None,
            url: "https://example.com/settings".into(),
            position: None,
        };

        let result = resolve_settings_tab(&tab, "my-ext");
        assert!(result.position.is_none());
        assert!(result.icon.is_none());
    }

    #[test]
    fn test_resolve_settings_tabs_multiple() {
        let tabs = vec![
            ExtSettingsTab {
                id: "a".into(),
                label: "Tab A".into(),
                icon: None,
                url: "a.html".into(),
                position: None,
            },
            ExtSettingsTab {
                id: "b".into(),
                label: "Tab B".into(),
                icon: None,
                url: "b.html".into(),
                position: Some(SettingsTabPosition {
                    relative_to: "a".into(),
                    placement: "before".into(),
                }),
            },
        ];

        let result = resolve_settings_tabs(&tabs, "my-ext");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, "a");
        assert_eq!(result[1].id, "b");
    }
}
