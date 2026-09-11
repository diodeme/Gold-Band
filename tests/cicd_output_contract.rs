pub use gold_band::{
    dsl,
    provider::{ProviderRunResult, ProviderRunStatus},
    runtime_error,
};
#[allow(dead_code)]
#[path = "../src/provider/cicd.rs"]
pub(crate) mod cicd;
pub mod provider {
    pub(crate) use crate::cicd;
    pub use gold_band::provider::*;
}
use camino::Utf8PathBuf;
use gold_band::{
    app::App,
    config::RuntimeConfig,
    storage::{StoragePathConfig, configure_storage_paths},
};

#[path = "../src/app/node_executor/output_contract.rs"]
mod output_contract;

#[test]
fn wb_cicd_emits_its_existing_artifact_in_the_single_execution_turn() {
    configure_storage_paths(StoragePathConfig {
        app_key: "maling",
        config_dir_name: ".maling",
        home_env_var: "CICD_OUTPUT_TEST_HOME",
    });
    let temp = tempfile::tempdir().unwrap();
    let app = App::with_config(
        Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap(),
        RuntimeConfig::default(),
    );
    let templates = app.workflow_templates().unwrap();
    let template = templates
        .templates
        .iter()
        .find(|t| t.id == "wb-development-cicd")
        .unwrap();
    let mut cicd_nodes = 0;
    for node in &template.workflow.nodes {
        let dsl::NodeDsl::Worker(worker) = node else {
            continue;
        };
        let Some(contract) = output_contract::worker_output_contract(worker) else {
            continue;
        };
        if worker.profile.as_deref() == Some("pf-builtin-cicd") {
            cicd_nodes += 1;
            assert_eq!(
                contract.emission_mode,
                provider::OutputEmissionMode::InlineControl,
                "single submission must include the output contract"
            );
            assert_eq!(contract.artifact, "cicd-result");
            assert_eq!(contract.schema, worker.output.as_ref().unwrap().schema);
            assert!(contract.success_condition.is_some());
        } else {
            assert_eq!(
                contract.emission_mode,
                provider::OutputEmissionMode::PostTurnProjection
            );
        }
    }
    assert_eq!(cicd_nodes, 1);
}
