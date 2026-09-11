use crate::dsl::{JsonConditionDsl, WorkerNode};
use crate::provider::{OutputEmissionMode, PromptOutputContract};

fn success_condition_text(condition: &JsonConditionDsl) -> String {
    match condition {
        JsonConditionDsl::Expression { expression } => expression.clone(),
        JsonConditionDsl::PathEquals { path, equals } => {
            format!("JSON field `{}` equals `{}`", path, equals)
        }
    }
}

pub(super) fn worker_output_contract(worker: &WorkerNode) -> Option<PromptOutputContract> {
    worker.output.as_ref().map(|output| PromptOutputContract {
        artifact: output.artifact.clone(),
        kind: format!("{:?}", output.kind).to_ascii_lowercase(),
        schema: output.schema.clone(),
        schema_text: None,
        success_condition: worker
            .success_condition
            .as_ref()
            .map(success_condition_text),
        finalize_context: None,
        emission_mode: if crate::provider::cicd::is_single_submission(worker.profile.as_deref()) {
            OutputEmissionMode::InlineControl
        } else {
            OutputEmissionMode::PostTurnProjection
        },
    })
}
