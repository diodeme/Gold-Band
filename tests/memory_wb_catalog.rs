use camino::Utf8PathBuf;
use gold_band::{app::App, config::RuntimeConfig};

#[test]
fn memory_wb_catalog_visibility_and_optional_entry() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let wb = gold_band::memory::is_wb();
    let app = App::with_config(root, RuntimeConfig::default());
    assert_eq!(
        app.profiles()
            .unwrap()
            .profiles
            .iter()
            .filter(|profile| profile.id == "pf-builtin-cicd")
            .count(),
        usize::from(wb)
    );
    assert_eq!(app.profile_show("pf-builtin-cicd").is_ok(), wb);
    let templates = app.workflow_templates().unwrap();
    let template = templates
        .templates
        .iter()
        .find(|template| template.id == "wb-development-cicd");
    assert_eq!(template.is_some(), wb);
    if let Some(template) = template {
        assert_eq!(template.workflow.nodes.len(), 4);
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
