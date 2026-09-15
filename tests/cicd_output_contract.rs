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
use gold_band::dsl::{JsonConditionDsl, OutputContractDsl, OutputKind, WorkerNode};

#[path = "../src/app/node_executor/output_contract.rs"]
mod output_contract;

#[test]
fn explicitly_validated_cicd_uses_inline_control_in_the_single_execution_turn() {
    let worker = WorkerNode {
        id: "cicd".into(),
        execution_slot_id: None,
        provider: None,
        model: None,
        profile: Some("pf-builtin-cicd".into()),
        goal: None,
        output: Some(OutputContractDsl {
            kind: OutputKind::Json,
            artifact: "cicd-result".into(),
            schema: Some(serde_json::json!({
                "reason": "String",
                "result": "boolean",
            })),
        }),
        success_condition: Some(JsonConditionDsl::Expression {
            expression: "$.result == true".into(),
        }),
        permission_mode: None,
        config_options: Default::default(),
        manual_check: None,
        prompt_envelope: gold_band::dsl::PromptEnvelopeMode::RuntimeManaged,
    };
    let contract = output_contract::worker_output_contract(&worker).unwrap();

    assert_eq!(
        contract.emission_mode,
        provider::OutputEmissionMode::InlineControl,
        "explicit output validation on CICD must stay in the single execution turn"
    );
    assert_eq!(contract.artifact, "cicd-result");
    assert!(contract.success_condition.is_some());

    let mut regular_worker = worker;
    regular_worker.profile = Some("pf-builtin-dev".into());
    let regular_contract = output_contract::worker_output_contract(&regular_worker).unwrap();
    assert_eq!(
        regular_contract.emission_mode,
        provider::OutputEmissionMode::PostTurnProjection
    );
}
