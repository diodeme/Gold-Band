mod auto_workflow;
pub mod error;
mod guard;
mod lock;
mod model;
mod store;

pub use auto_workflow::compile_auto_workflow;
pub use error::{ExecutionPlanError, PlanResult};
pub(crate) use model::OperationCommit;
pub use model::{
    ExecutionPlanLocator, ExecutionPlanPayload, ExecutionPlanPreflight, ExecutionPlanRunMode,
    ExecutionPlanSaveCommand, ExecutionPlanSaveResult, ExecutionPlanSnapshot, ExecutionPlanTarget,
    ExecutionPlanTargetResult, ExecutionPlanView, WorkflowAuthoringDraft,
};
pub use store::{
    ai_dynamic_node_for_dispatch, bump_authoring_revision, load_current, load_workflow_projection,
    publish_initial_auto, publish_initial_workflow,
};

pub(crate) use guard::{affected_node_ids, current_run_editable, evaluate_current_workflow};
pub(crate) use lock::{authoring_lock_key, try_acquire_plan_write, with_dispatch_read};
pub(crate) use store::{
    ai_dynamic_node_for_dispatch_under_lock, current_guard, lock_key, outcome_label, payload_eq,
    publish_revision_checked, read_authoring_revision, read_operation_commit, read_run_state,
    status_label, validate_operation_id, workflow_from_snapshot, write_operation_commit,
};

#[cfg(test)]
mod tests;
