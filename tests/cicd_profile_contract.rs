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
