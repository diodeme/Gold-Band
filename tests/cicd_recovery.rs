pub use gold_band::{
    provider::{ProviderRunResult, ProviderRunStatus},
    runtime_error,
};

#[path = "../src/provider/cicd.rs"]
mod cicd;

fn outcome(status: ProviderRunStatus) -> ProviderRunResult {
    ProviderRunResult {
        status,
        exit_code: Some(0),
        result_payload: None,
        worker_ref_seed: None,
        stream_path: None,
        runtime_error: None,
        runtime_control_output: None,
    }
}

#[test]
fn successful_cicd_is_not_replaced_by_a_placeholder_failure() {
    assert!(cicd::is_single_submission(Some("pf-builtin-cicd")));
    assert!(!cicd::is_single_submission(Some("pf-builtin-dev")));
    assert!(!cicd::is_single_submission(None));
    let result = cicd::run_once(|| Ok(outcome(ProviderRunStatus::Success))).unwrap();
    assert_eq!(result.status, ProviderRunStatus::Success);
    assert!(result.runtime_error.is_none());
}

#[test]
fn failure_is_manual_and_never_replays_the_action() {
    use runtime_error::*;
    let mut calls = 0;
    let result = cicd::run_once(|| {
        calls += 1;
        Err(runtime_error(auto_runtime_error_info(
            RuntimeErrorDomain::Provider,
            "provider.temporary",
            "temporary",
            serde_json::json!({"operationId":"deploy-1"}),
        )))
    })
    .unwrap_err();
    assert_eq!(calls, 1);
    let error = normalize_runtime_error(&result);
    assert_eq!(error.code_str(), "provider.temporary");
    assert_eq!(error.recovery, RecoveryMode::Manual);
    assert!(error.retry_policy.is_none());
}

#[test]
fn embedded_failures_and_interaction_states_are_preserved() {
    use runtime_error::*;
    let mut failed = outcome(ProviderRunStatus::Failure);
    failed.runtime_error = Some(auto_runtime_error_info(
        RuntimeErrorDomain::Provider,
        "provider.temporary",
        "temporary",
        serde_json::json!({}),
    ));
    let result = cicd::run_once(|| Ok(failed)).unwrap();
    assert_eq!(result.status, ProviderRunStatus::Failure);
    let error = result.runtime_error.unwrap();
    assert_eq!(error.recovery, RecoveryMode::Manual);
    assert!(error.retry_policy.is_none());
    for status in [
        ProviderRunStatus::Interrupted,
        ProviderRunStatus::WaitingForUserInput,
        ProviderRunStatus::PermissionRequested,
    ] {
        let result = cicd::run_once(|| Ok(outcome(status))).unwrap();
        assert_eq!(result.status, status);
        assert!(result.runtime_error.is_none());
    }
}
