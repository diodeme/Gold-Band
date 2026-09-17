use camino::Utf8PathBuf;
use gold_band::{
    app::App,
    channel::{RELEASE_CHANNEL, WB_CHANNEL},
    config::{DesktopLanguage, RuntimeConfig},
    prompts::{
        PROFILE_DEV_TEST_EN, PROFILE_DEV_TEST_ZH_CN, PROFILE_GRILLME_EN, PROFILE_GRILLME_ZH_CN,
        PROFILE_INTERVIEW_EN, PROFILE_INTERVIEW_ZH_CN,
    },
    storage::{StoragePathConfig, configure_storage_paths},
};

#[test]
fn wb_workflow_profile_supplements_are_channel_scoped_and_complete() {
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

        if RELEASE_CHANNEL == WB_CHANNEL {
            let identity_markers = match language {
                DesktopLanguage::ZhCn => [
                    "WB 需求身份前置检查",
                    "`memory_read`",
                    "`storyId`",
                    "`storyName`",
                    "`不存在`",
                    "`其他（用户自行输入）`",
                    "`memory_write`",
                    "任务作用域",
                    "最多 40 个 Unicode 字符",
                    "不是阻塞",
                ],
                DesktopLanguage::En => [
                    "WB Requirement Identity Precheck",
                    "`memory_read`",
                    "`storyId`",
                    "`storyName`",
                    "`不存在`",
                    "`其他（用户自行输入）`",
                    "`memory_write`",
                    "task scope",
                    "at most 40 Unicode characters",
                    "not blockers",
                ],
            };
            for content in [&interview.content, &grill.content] {
                for marker in identity_markers {
                    assert!(
                        content.contains(marker),
                        "WB requirement identity marker missing: {marker}"
                    );
                }
            }

            let commit_markers = match language {
                DesktopLanguage::ZhCn => [
                    "WB 开发测试自动提交",
                    "不创建空提交",
                    "提交过程不向用户询问提交消息",
                    "记忆工具不可用或写入失败不是阻塞",
                    "禁止 `git add -A`",
                    "--story=[{storyId}] {storyName}",
                    "#AI COMMIT#",
                    "commit OID",
                    "CICD 将使用",
                ],
                DesktopLanguage::En => [
                    "WB Development and Testing Auto-Commit",
                    "do not create an empty commit",
                    "Do not ask the user to confirm the commit message",
                    "Unavailable memory tools or a failed write are not blockers",
                    "Never use `git add -A`",
                    "--story=[{storyId}] {storyName}",
                    "#AI COMMIT#",
                    "commit OID",
                    "CICD uses these commit OIDs",
                ],
            };
            for marker in commit_markers {
                assert!(
                    dev_test.content.contains(marker),
                    "WB dev-test commit marker missing: {marker}"
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
                assert!(!actual.contains("WB 需求身份前置检查"));
                assert!(!actual.contains("WB Requirement Identity Precheck"));
                assert!(!actual.contains("WB 开发测试自动提交"));
                assert!(!actual.contains("WB Development and Testing Auto-Commit"));
            }
        }
    }
}
