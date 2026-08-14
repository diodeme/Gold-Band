use gold_band::config::{
    CURRENT_SETTINGS_SCHEMA_VERSION, ColorSchemePreference, SettingsConfig, VisualQuality,
};

#[test]
fn legacy_desktop_palettes_migrate_to_theme_id_and_color_scheme() {
    let cases = [
        ("light", "builtin.gold-band", ColorSchemePreference::Light),
        ("dark", "builtin.gold-band", ColorSchemePreference::Dark),
        (
            "light-gray",
            "builtin.tech-neutral",
            ColorSchemePreference::Light,
        ),
        ("black", "builtin.tech-neutral", ColorSchemePreference::Dark),
        ("system", "builtin.gold-band", ColorSchemePreference::System),
    ];

    for (legacy, expected_theme, expected_scheme) in cases {
        let (settings, migrated) =
            SettingsConfig::from_json_value_with_migration(serde_json::json!({
                "settingsSchemaVersion": 4,
                "desktopTheme": legacy,
            }))
            .expect("legacy settings should migrate");

        assert!(migrated, "{legacy} should trigger a schema migration");
        assert_eq!(
            settings.settings_schema_version.0,
            CURRENT_SETTINGS_SCHEMA_VERSION
        );
        let appearance = settings
            .appearance
            .as_ref()
            .expect("migration should create appearance");
        assert_eq!(appearance.schema_version, 2);
        assert_eq!(appearance.theme_id, expected_theme);
        assert_eq!(appearance.color_scheme, expected_scheme);
        assert!(appearance.visual_quality_by_theme.is_empty());

        let persisted = serde_json::to_value(settings).expect("migrated settings should serialize");
        assert!(persisted.get("desktopTheme").is_none());
    }
}

#[test]
fn canonical_appearance_wins_and_removes_the_legacy_field() {
    let (settings, migrated) = SettingsConfig::from_json_value_with_migration(serde_json::json!({
        "settingsSchemaVersion": 4,
        "desktopTheme": "dark",
        "appearance": {
            "schemaVersion": 2,
            "themeId": "builtin.glass",
            "colorScheme": "light",
            "visualQualityByTheme": {
                "builtin.glass": "performance"
            }
        }
    }))
    .expect("canonical appearance should survive migration");

    assert!(migrated);
    let appearance = settings
        .appearance
        .as_ref()
        .expect("appearance should remain present");
    assert_eq!(appearance.theme_id, "builtin.glass");
    assert_eq!(appearance.color_scheme, ColorSchemePreference::Light);
    assert_eq!(
        appearance.visual_quality_by_theme.get("builtin.glass"),
        Some(&VisualQuality::Performance)
    );
    let persisted = serde_json::to_value(settings).expect("migrated settings should serialize");
    assert!(persisted.get("desktopTheme").is_none());
}
