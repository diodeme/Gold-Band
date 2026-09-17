use std::fs;
use std::path::Path;

use walkdir::WalkDir;

#[test]
fn profile_prompts_do_not_embed_release_channel_identity() {
    let prompts = [
        (
            "zh requirement identity",
            include_str!("../src/prompts/zh-CN/profile/overlays/requirement-identity.md"),
        ),
        (
            "en requirement identity",
            include_str!("../src/prompts/en/profile/overlays/requirement-identity.md"),
        ),
        (
            "zh dev-test auto-commit",
            include_str!("../src/prompts/zh-CN/profile/overlays/dev-test-auto-commit.md"),
        ),
        (
            "en dev-test auto-commit",
            include_str!("../src/prompts/en/profile/overlays/dev-test-auto-commit.md"),
        ),
        (
            "zh cicd",
            include_str!("../src/prompts/zh-CN/profile/cicd.md"),
        ),
        ("en cicd", include_str!("../src/prompts/en/profile/cicd.md")),
    ];

    for (name, prompt) in prompts {
        assert!(
            !prompt.to_ascii_lowercase().contains("wb"),
            "profile prompt `{name}` must remain channel-neutral"
        );
    }
}

#[test]
fn all_prompt_assets_are_channel_neutral() {
    let prompts_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/prompts");
    let mut checked = 0;

    for entry in WalkDir::new(&prompts_root)
        .follow_links(false)
        .sort_by_file_name()
    {
        let entry = entry.expect("prompt tree should be readable");
        if !entry.file_type().is_file()
            || entry.path().extension().and_then(|value| value.to_str()) != Some("md")
        {
            continue;
        }

        let content = fs::read_to_string(entry.path()).expect("prompt asset should be UTF-8");
        assert!(
            !entry
                .path()
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains("wb"),
            "prompt asset path `{}` must remain channel-neutral",
            entry.path().display()
        );
        assert!(
            !content.to_ascii_lowercase().contains("wb"),
            "prompt asset `{}` must remain channel-neutral",
            entry.path().display()
        );
        checked += 1;
    }

    assert!(checked > 0, "prompt tree must contain markdown assets");
}
