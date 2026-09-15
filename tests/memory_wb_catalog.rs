use camino::Utf8PathBuf;
use gold_band::{
    app::App,
    channel::{RELEASE_CHANNEL, WB_CHANNEL},
    config::RuntimeConfig,
    storage::{StoragePathConfig, configure_storage_paths},
};

#[test]
fn memory_wb_catalog_visibility_and_optional_entry() {
    // A dedicated integration-test process isolates the process-wide channel configuration.
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let expect_cicd = RELEASE_CHANNEL == WB_CHANNEL;
    for (_wb, config) in [
        (
            false,
            StoragePathConfig {
                app_key: "gold-band",
                config_dir_name: ".gold-band",
                home_env_var: "MEMORY_CATALOG_TEST_HOME",
            },
        ),
        (
            true,
            StoragePathConfig {
                app_key: "maling",
                config_dir_name: ".maling",
                home_env_var: "MEMORY_CATALOG_TEST_HOME",
            },
        ),
    ] {
        configure_storage_paths(config);
        let app = App::with_config(root.clone(), RuntimeConfig::default());
        assert_eq!(
            app.profiles()
                .unwrap()
                .profiles
                .iter()
                .filter(|profile| profile.id == "pf-builtin-cicd")
                .count(),
            usize::from(expect_cicd)
        );
        assert_eq!(app.profile_show("pf-builtin-cicd").is_ok(), expect_cicd);
        let templates = app.workflow_templates().unwrap();
        let template = templates
            .templates
            .iter()
            .find(|template| template.id == "wb-development-cicd");
        assert_eq!(template.is_some(), expect_cicd);
        if let Some(template) = template {
            assert_eq!(template.workflow.nodes.len(), 4);
            let cicd = template
                .workflow
                .nodes
                .iter()
                .find_map(|node| match node {
                    gold_band::dsl::NodeDsl::Worker(worker) if worker.id == "cicd" => Some(worker),
                    _ => None,
                })
                .unwrap();
            assert_eq!(cicd.manual_check, Some(true));
            assert!(cicd.output.is_none());
            assert!(cicd.success_condition.is_none());
            let mut workflow = template.workflow.clone();
            gold_band::app::apply_optional_entry_preference(template, Some(false), &mut workflow)
                .unwrap();
            assert_eq!(workflow.entry, "dev-test");
            assert!(!workflow.nodes.iter().any(|node| node.id() == "grill"));
            assert_eq!(
                app.workflow_templates()
                    .unwrap()
                    .templates
                    .iter()
                    .filter(|template| template.id == "wb-development-cicd")
                    .count(),
                1
            );
        }
    }
}
