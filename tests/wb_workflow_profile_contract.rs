use camino::Utf8PathBuf;
use gold_band::{
    app::App,
    channel::{ProfileChannelCapability, RELEASE_CHANNEL, profile_channel_capability_enabled},
    config::{DesktopLanguage, RuntimeConfig},
    prompts::{
        PROFILE_DEV_TEST_EN, PROFILE_DEV_TEST_ZH_CN, PROFILE_GRILLME_EN, PROFILE_GRILLME_ZH_CN,
        PROFILE_INTERVIEW_EN, PROFILE_INTERVIEW_ZH_CN,
    },
    storage::{StoragePathConfig, configure_storage_paths},
};

#[test]
fn profile_supplements_are_channel_scoped_and_complete() {
    configure_storage_paths(StoragePathConfig {
        app_key: "maling",
        config_dir_name: ".maling",
        home_env_var: "WB_WORKFLOW_PROFILE_CONTRACT_HOME",
    });
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();

    for language in [DesktopLanguage::ZhCn, DesktopLanguage::En] {
        let app = App::with_config(
            root.clone(),
            RuntimeConfig {
                desktop_language: language,
                ..RuntimeConfig::default()
            },
        );
        let interview = app.profile_show("pf-builtin-interview").unwrap();
        let grill = app.profile_show("pf-builtin-grill").unwrap();
        let dev_test = app.profile_show("pf-builtin-dev-test").unwrap();

        let requirement_identity_enabled = profile_channel_capability_enabled(
            RELEASE_CHANNEL,
            ProfileChannelCapability::RequirementIdentity,
        );
        let dev_test_auto_commit_enabled = profile_channel_capability_enabled(
            RELEASE_CHANNEL,
            ProfileChannelCapability::DevTestAutoCommit,
        );
        if requirement_identity_enabled || dev_test_auto_commit_enabled {
            let identity_markers = match language {
                DesktopLanguage::ZhCn => [
                    "需求身份前置检查",
                    "本节开始时先调用 `memory_read`",
                    "`memory_read`",
                    "`storyId`",
                    "`storyName`",
                    "`不存在`",
                    "`其他（用户自行输入）`",
                    "`memory_write`",
                    "`operation=\"create\"`",
                    "`operation=\"update\"`",
                    "任务作用域",
                    "最多 40 个 Unicode 字符",
                    "不是阻塞",
                ],
                DesktopLanguage::En => [
                    "Requirement Identity Precheck",
                    "At the start of this section, first call `memory_read`",
                    "`memory_read`",
                    "`storyId`",
                    "`storyName`",
                    "`不存在`",
                    "`其他（用户自行输入）`",
                    "`memory_write`",
                    "`operation=\"create\"`",
                    "`operation=\"update\"`",
                    "task scope",
                    "at most 40 Unicode characters",
                    "Neither case is a blocker",
                ],
            };
            for content in [&interview.content, &grill.content] {
                for marker in identity_markers {
                    assert!(
                        content.contains(marker),
                        "requirement identity marker missing: {marker}"
                    );
                }
                for marker in [
                    "hidden memory projection",
                    "记忆投影",
                    "<memory-data>",
                    "Gold Band current memory",
                ] {
                    assert!(
                        !content.contains(marker),
                        "requirement identity must not depend on automatic projection: {marker}"
                    );
                }
            }

            let commit_markers = match language {
                DesktopLanguage::ZhCn => [
                    "开发测试自动提交",
                    "1. 使用 `memory_read`",
                    "`operation=\"create\"`",
                    "`operation=\"update\"`",
                    "不创建空提交",
                    "提交过程不向用户询问提交消息",
                    "读取失败时说明身份未读取",
                    "写入或核验失败时说明未持久化",
                    "二者都不是阻塞",
                    "禁止 `git add -A`",
                    "--story=[{storyId}] {storyName}",
                    "#AI COMMIT#",
                    "commit OID",
                    "CICD 将使用",
                ],
                DesktopLanguage::En => [
                    "Development and Testing Auto-Commit",
                    "1. Use `memory_read`",
                    "`operation=\"create\"`",
                    "`operation=\"update\"`",
                    "do not create an empty commit",
                    "Do not ask the user to confirm the commit message",
                    "state that the identity was not read",
                    "state that it was not persisted",
                    "Neither case is a blocker",
                    "Never use `git add -A`",
                    "--story=[{storyId}] {storyName}",
                    "#AI COMMIT#",
                    "commit OID",
                    "CICD uses these commit OIDs",
                ],
            };
            assert_eq!(
                requirement_identity_enabled, dev_test_auto_commit_enabled,
                "the current channel policy enables both profile overlays together"
            );
            for marker in commit_markers {
                assert!(
                    dev_test.content.contains(marker),
                    "dev-test auto-commit marker missing: {marker}"
                );
            }
            for marker in [
                "hidden memory projection",
                "记忆投影",
                "<memory-data>",
                "Gold Band current memory",
                "收尾开始时先调用 `memory_read`",
                "At the start of this closing step, first call `memory_read`",
            ] {
                assert!(
                    !dev_test.content.contains(marker),
                    "dev-test auto-commit must not depend on automatic projection: {marker}"
                );
            }
        } else {
            let expected = match language {
                DesktopLanguage::ZhCn => [
                    (interview.content.as_str(), PROFILE_INTERVIEW_ZH_CN),
                    (grill.content.as_str(), PROFILE_GRILLME_ZH_CN),
                    (dev_test.content.as_str(), PROFILE_DEV_TEST_ZH_CN),
                ],
                DesktopLanguage::En => [
                    (interview.content.as_str(), PROFILE_INTERVIEW_EN),
                    (grill.content.as_str(), PROFILE_GRILLME_EN),
                    (dev_test.content.as_str(), PROFILE_DEV_TEST_EN),
                ],
            };
            for (actual, expected) in expected {
                assert_eq!(actual, expected);
                assert!(!actual.contains("需求身份前置检查"));
                assert!(!actual.contains("Requirement Identity Precheck"));
                assert!(!actual.contains("开发测试自动提交"));
                assert!(!actual.contains("Development and Testing Auto-Commit"));
            }
        }
    }
}
