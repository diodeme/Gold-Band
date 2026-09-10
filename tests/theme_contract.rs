use std::collections::BTreeSet;

use gold_band::theme::{ThemeMaterialModel, builtin_theme, builtin_theme_catalog};

#[test]
fn rust_catalog_deserializes_every_generated_declarative_theme() {
    let catalog = builtin_theme_catalog().expect("generated theme catalog should be valid");
    let ids = catalog
        .iter()
        .map(|theme| theme.id.as_str())
        .collect::<BTreeSet<_>>();

    assert_eq!(catalog.len(), 2);
    assert_eq!(
        ids,
        BTreeSet::from(["builtin.gold-band", "builtin.tech-neutral"])
    );
    for theme in catalog {
        assert!(!theme.name.zh_cn.is_empty());
        assert!(!theme.name.en.is_empty());
        assert!(!theme.schemes.light.semantic.foreground.is_empty());
        assert!(!theme.schemes.dark.semantic.foreground.is_empty());
        for scheme in [&theme.schemes.light, &theme.schemes.dark] {
            for token in [
                &scheme.semantic.status_running_surface,
                &scheme.semantic.status_running_border,
                &scheme.semantic.status_success_surface,
                &scheme.semantic.status_success_border,
                &scheme.semantic.status_warning_surface,
                &scheme.semantic.status_warning_border,
                &scheme.semantic.status_danger_surface,
                &scheme.semantic.status_danger_border,
            ] {
                assert!(
                    !token.is_empty(),
                    "status surfaces and borders must survive Rust decoding"
                );
            }
        }
        let ui_stack = theme
            .fonts
            .as_ref()
            .and_then(|fonts| fonts.stacks.iter().find(|stack| stack.id == "theme-ui"))
            .expect("every built-in theme should declare its UI font stack");
        assert_eq!(
            ui_stack.default_faces,
            ["inter-variable", "misans-variable"],
            "theme font order must not depend on interface language"
        );
    }
}

#[test]
fn rust_catalog_contains_only_the_supported_builtin_packages() {
    let gold_band = builtin_theme("builtin.gold-band")
        .expect("catalog should deserialize")
        .expect("safe fallback should be registered");
    let tech_neutral = builtin_theme("builtin.tech-neutral")
        .expect("catalog should deserialize")
        .expect("technology-neutral theme should be registered");

    assert!(gold_band.visual_quality_profiles.is_none());
    assert!(tech_neutral.visual_quality_profiles.is_none());
    assert_eq!(
        gold_band.schemes.light.material.model,
        ThemeMaterialModel::Solid
    );
    assert_eq!(
        tech_neutral.schemes.light.material.model,
        ThemeMaterialModel::Solid
    );
    assert_ne!(tech_neutral.recipes, gold_band.recipes);
    assert_ne!(
        tech_neutral.recipes.message_disclosure,
        gold_band.recipes.message_disclosure
    );
    assert_ne!(
        tech_neutral.recipes.runtime_control,
        gold_band.recipes.runtime_control
    );
    assert_ne!(
        tech_neutral.schemes.light.semantic.primary,
        gold_band.schemes.light.semantic.primary
    );
    assert!(
        builtin_theme("builtin.glass")
            .expect("catalog should deserialize")
            .is_none()
    );
    assert!(
        builtin_theme("builtin.neo-brutalist")
            .expect("catalog should deserialize")
            .is_none()
    );
}
