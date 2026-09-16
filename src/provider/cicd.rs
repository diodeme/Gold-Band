use super::ProviderRunResult;
use crate::runtime_error::{RecoveryMode, normalize_runtime_error, runtime_error};
use anyhow::Result;

pub(crate) fn is_single_submission(profile: Option<&str>) -> bool {
    profile == Some("pf-builtin-cicd")
}

pub(super) fn run_once(
    action: impl FnOnce() -> Result<ProviderRunResult>,
) -> Result<ProviderRunResult> {
    let result = action();
    match result {
        Err(error) => {
            let mut info = normalize_runtime_error(&error);
            info.recovery = RecoveryMode::Manual;
            info.retry_policy = None;
            Err(runtime_error(info))
        }
        Ok(mut result) => {
            if let Some(error) = &mut result.runtime_error {
                error.recovery = RecoveryMode::Manual;
                error.retry_policy = None;
            }
            Ok(result)
        }
    }
}
