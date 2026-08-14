use std::collections::BTreeSet;

use gold_band::theme::{ThemeCapability, ThemeVisualQuality, builtin_theme, builtin_theme_catalog};

#[test]
fn rust_catalog_deserializes_every_generated_declarative_theme() {
    let catalog = builtin_theme_catalog().expect("generated theme catalog should be valid");
    let ids = catalog
        .iter()
        .map(|theme| theme.id.as_str())
        .collect::<BTreeSet<_>>();

    assert_eq!(catalog.len(), 4);
    assert_eq!(
        ids,
        BTreeSet::from([
            "builtin.glass",
            "builtin.gold-band",
            "builtin.neo-brutalist",
            "builtin.tech-neutral",
        ])
    );
    for theme in catalog {
        assert!(!theme.name.zh_cn.is_empty());
        assert!(!theme.name.en.is_empty());
        assert!(!theme.schemes.light.semantic.foreground.is_empty());
        assert!(!theme.schemes.dark.semantic.foreground.is_empty());
    }
}

#[test]
fn rust_catalog_exposes_quality_capability_without_theme_id_special_cases() {
    let glass = builtin_theme("builtin.glass")
        .expect("catalog should deserialize")
        .expect("glass should be registered");
    let gold_band = builtin_theme("builtin.gold-band")
        .expect("catalog should deserialize")
        .expect("safe fallback should be registered");
    let neo_brutalist = builtin_theme("builtin.neo-brutalist")
        .expect("catalog should deserialize")
        .expect("declarative validation package should be registered");

    assert!(
        glass
            .capabilities
            .contains(&ThemeCapability::VisualQualityProfiles)
    );
    let profiles = glass
        .visual_quality_profiles
        .as_ref()
        .expect("quality capability should have profiles");
    assert_eq!(profiles.default, ThemeVisualQuality::Full);
    assert!(profiles.performance.blur < glass.schemes.dark.material.blur);

    assert!(
        !gold_band
            .capabilities
            .contains(&ThemeCapability::VisualQualityProfiles)
    );
    assert!(gold_band.visual_quality_profiles.is_none());
    assert_ne!(neo_brutalist.recipes, gold_band.recipes);
    assert_ne!(
        neo_brutalist.schemes.light.semantic.primary,
        gold_band.schemes.light.semantic.primary
    );
}
