use camino::Utf8PathBuf;
use gold_band::{
    app::App,
    config::{DesktopLanguage, RuntimeConfig},
    storage::{StoragePathConfig, configure_storage_paths},
};

#[test]
fn formal_cicd_uses_the_shared_memory_tools_in_both_languages() {
    configure_storage_paths(StoragePathConfig {
        app_key: "maling",
        config_dir_name: ".maling",
        home_env_var: "CICD_CONTRACT_TEST_HOME",
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
        assert!(profile.content.contains("cicd.<S>.selected"));
        assert!(!profile.content.contains("\"targets\""));
        assert!(profile.content.contains("wetest --json build run"));
        assert!(profile.content.contains("resultCode == 0"));
        let commit_gate_markers = match language {
            DesktopLanguage::ZhCn => [
                "### 代码提交前置条件",
                "### CLI 与参数检查",
                "原始需求、当前 task / goal、runtime 明确提供的前序产物",
                "--story=[",
                "<type>: <中文描述>",
                "标准 Conventional Commits 类型 token",
                "feat: 添加登录功能",
                "#AI COMMIT#",
                "先从原始需求确认 `%id` 和 `%name`",
                "发现相关未提交改动时先协助用户提交",
                "按具体路径执行 git add 与 git commit",
                "禁止使用 git add -A",
                "提交后重新检查",
                "相关提交已存在但尚未推送时同样协助用户推送",
                "用户确认后执行普通 git push",
                "禁止 force push",
                "push 后核实远端分支已包含本次相关提交",
                "远端分支未包含相关提交",
                "系统需求",
            ],
            DesktopLanguage::En => [
                "### Code commit precondition",
                "### CLI and Parameter Checks",
                "original requirement, the current task / goal, predecessor artifacts explicitly supplied by the runtime",
                "--story=[",
                "<type>: <Chinese description>",
                "standard Conventional Commits type token",
                "feat: 添加登录功能",
                "#AI COMMIT#",
                "Resolve `%id` and `%name` from the original requirement first",
                "first help the user commit them",
                "run git add and git commit with the specific paths",
                "Never use git add -A",
                "recheck after committing",
                "also help the user push them",
                "run a normal git push",
                "Never force push",
                "verify that the remote branch contains the related commits",
                "remote branch lacks related commits",
                "系统需求",
            ],
        };
        for marker in commit_gate_markers {
            assert!(
                profile.content.contains(marker),
                "CICD commit gate marker missing: {marker}"
            );
        }
        for field in [
            "selected",
            "build.jobId",
            "build.branch",
            "build.appList",
            "deploy.mode",
            "deploy.templateId",
            "deploy.templateName",
            "deploy.deployType",
            "deploy.env",
            "deploy.ips",
            "deploy.containers",
            "deploy.pkgNames",
            "deploy.inputParams",
        ] {
            assert!(profile.content.contains(&format!("cicd.<S>.{field}")));
        }
        assert!(profile.content.contains("%HH"));
        assert!(profile.content.contains("32 KiB"));
        assert!(!profile.content.contains("cicd.not-configured"));
    }
}
