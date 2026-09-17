use camino::Utf8PathBuf;
use gold_band::{
    app::App,
    channel::{RELEASE_CHANNEL, WB_CHANNEL},
    config::{DesktopLanguage, RuntimeConfig},
    storage::{StoragePathConfig, configure_storage_paths},
};

#[test]
fn formal_cicd_uses_the_shared_memory_tools_in_both_languages() {
    if !gold_band::memory::is_wb() {
        return;
    }
    configure_storage_paths(StoragePathConfig {
        app_key: "maling",
        config_dir_name: ".maling",
        home_env_var: "CICD_CONTRACT_TEST_HOME",
    });
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    if RELEASE_CHANNEL != WB_CHANNEL {
        let app = App::with_config(root, RuntimeConfig::default());
        assert!(app.profile_show("pf-builtin-cicd").is_err());
        return;
    }
    for language in [DesktopLanguage::ZhCn, DesktopLanguage::En] {
        let app = App::with_config(
            root.clone(),
            RuntimeConfig {
                desktop_language: language,
                ..RuntimeConfig::default()
            },
        );
        let profile = app.profile_show("pf-builtin-cicd").unwrap();
        let listed = app
            .profiles()
            .unwrap()
            .profiles
            .into_iter()
            .find(|p| p.id == profile.id)
            .unwrap();
        assert_eq!(listed.content, profile.content);
        assert!(profile.is_built_in);
        assert!(!profile.dynamic_template);
        assert_eq!(profile.path, "builtin://profiles/cicd");
        assert!(
            profile.content.contains("memory_read"),
            "CICD must use shared memory reads"
        );
        assert!(profile.content.contains("memory_write"));
        assert!(profile.content.contains("expectedRevision"));
        assert!(profile.content.contains("subSysId1"));
        assert!(profile.content.contains("cicd.build.jobId"));
        assert!(profile.content.contains("cicd.deploy.<S>.selected"));
        assert!(!profile.content.contains("\"targets\""));
        assert!(!profile.content.contains("Current-task `memory.json`"));
        assert!(!profile.content.contains("当前 task 的 `memory.json`"));
        assert!(profile.content.contains("wetest --json build run"));
        assert!(profile.content.contains("resultCode == 0"));
        assert!(profile.content.contains("deploy instance-list"));
        assert!(profile.content.contains("buildNum"));
        assert!(!profile.content.contains("`deployments`"));
        let push_precheck_markers = match language {
            DesktopLanguage::ZhCn => [
                "### 代码推送前置检查",
                "### CLI 与参数检查",
                "只检查当前分支是否存在未 push 的提交",
                "提醒用户并询问是否 push",
                "用户不 push",
                "push 失败",
                "询问是否继续构建",
                "按远端现状推进",
                "禁止 force push",
                "固定作用域不得混淆",
                "所有 `cicd.*` 参数固定写任务作用域",
                "再次 `memory_read`",
                "不是阻塞",
                "向用户询问或确认所需参数",
            ],
            DesktopLanguage::En => [
                "### Code Push Precheck",
                "### CLI and Parameter Checks",
                "only check whether the current branch has unpushed commits",
                "remind the user and ask whether to push",
                "declines to push",
                "push fails",
                "ask whether to continue building",
                "Continue from the current remote state",
                "Never force push",
                "The fixed scopes are mandatory",
                "Write every `cicd.*` parameter to task scope",
                "call `memory_read` again",
                "not blockers",
                "ask for or confirm the required parameters",
            ],
        };
        for marker in push_precheck_markers {
            assert!(
                profile.content.contains(marker),
                "CICD push precheck marker missing: {marker}"
            );
        }
        assert!(!profile.content.contains("--story=["));
        assert!(!profile.content.contains("#AI COMMIT#"));
        assert!(!profile.content.contains("Conventional Commits"));
        assert!(!profile.content.contains("代码提交前置条件"));
        assert!(!profile.content.contains("Code commit precondition"));
        for field in [
            "selected",
            "mode",
            "templateId",
            "templateName",
            "deployType",
            "env",
            "ips",
            "containers",
            "pkgNames",
            "inputParams",
        ] {
            assert!(
                profile
                    .content
                    .contains(&format!("cicd.deploy.<S>.{field}"))
            );
        }
        for field in ["jobId", "branch", "appList", "appCoverage"] {
            assert!(profile.content.contains(&format!("cicd.build.{field}")));
        }
        for marker in match language {
            DesktopLanguage::ZhCn => [
                "每次 run 都必须重新确认构建和部署参数",
                "不能复用上一次 run 的确认",
            ],
            DesktopLanguage::En => [
                "Every run must freshly confirm its build and deployment parameters",
                "A confirmation from a previous run cannot be reused",
            ],
        } {
            assert!(
                profile.content.contains(marker),
                "CICD per-run confirmation marker missing: {marker}"
            );
        }
        assert!(profile.content.contains("%HH"));
        assert!(profile.content.contains("32 KiB"));
        assert!(!profile.content.contains("cicd.not-configured"));
    }
}
