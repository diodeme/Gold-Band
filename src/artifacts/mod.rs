use crate::domain::CommandStatus;
use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecPlanArtifact {
    pub version: String,
    pub commands: Vec<ExecPlanCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecPlanCommand {
    pub id: String,
    pub run: String,
    pub purpose: String,
    pub cwd: Option<String>,
    pub timeout_sec: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResultArtifact {
    pub version: String,
    pub status: ExecResultStatus,
    pub commands: Vec<ExecCommandResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecResultStatus {
    Success,
    Failure,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecCommandResult {
    pub id: String,
    pub exit_code: Option<i32>,
    pub status: CommandStatus,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub duration_ms: Option<u64>,
    pub timed_out: Option<bool>,
    pub stdout_path: Option<String>,
    pub stderr_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyResultArtifact {
    pub version: String,
    pub status: VerifyStatus,
    pub summary: String,
    pub unmet_requirements: Vec<String>,
    pub validation_gaps: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerifyStatus {
    Success,
    Failure,
}

pub fn validate_exec_plan(plan: &ExecPlanArtifact) -> Result<()> {
    ensure!(plan.version == "0.1", "unsupported exec-plan version: {}", plan.version);
    ensure!(!plan.commands.is_empty(), "exec-plan commands cannot be empty");

    let mut ids = std::collections::HashSet::new();
    for command in &plan.commands {
        ensure!(!command.id.trim().is_empty(), "command id cannot be empty");
        ensure!(ids.insert(command.id.clone()), "duplicate command id: {}", command.id);
        ensure!(!command.run.trim().is_empty(), "command run cannot be empty");
        ensure!(!command.purpose.trim().is_empty(), "command purpose cannot be empty");
        if let Some(timeout_sec) = command.timeout_sec {
            ensure!(timeout_sec > 0, "timeoutSec must be positive");
        }
    }

    Ok(())
}

pub fn validate_exec_result(result: &ExecResultArtifact) -> Result<()> {
    ensure!(result.version == "0.1", "unsupported exec-result version: {}", result.version);
    ensure!(!result.commands.is_empty(), "exec-result commands cannot be empty");

    let has_failure = result.commands.iter().any(|command| command.status == CommandStatus::Failure);
    let expected = if has_failure { ExecResultStatus::Failure } else { ExecResultStatus::Success };
    ensure!(result.status == expected, "exec-result top-level status does not match command aggregation");

    for command in &result.commands {
        ensure!(!command.id.trim().is_empty(), "exec-result command id cannot be empty");
        if command.status == CommandStatus::Skipped {
            ensure!(command.exit_code.is_none(), "skipped command must not include exitCode");
        } else {
            ensure!(command.exit_code.is_some(), "executed command must include exitCode");
            ensure!(command.stdout_path.as_deref().is_some_and(|value| !value.is_empty()), "executed command must include stdoutPath");
            ensure!(command.stderr_path.as_deref().is_some_and(|value| !value.is_empty()), "executed command must include stderrPath");
        }
    }

    Ok(())
}

pub fn validate_verify_result(result: &VerifyResultArtifact) -> Result<()> {
    ensure!(result.version == "0.1", "unsupported verify-result version: {}", result.version);
    ensure!(!result.summary.trim().is_empty(), "verify-result summary cannot be empty");

    match result.status {
        VerifyStatus::Success => {
            ensure!(result.unmet_requirements.is_empty(), "success verify-result must not contain unmetRequirements");
            ensure!(result.validation_gaps.is_empty(), "success verify-result must not contain validationGaps");
        }
        VerifyStatus::Failure => {
            if result.unmet_requirements.is_empty() && result.validation_gaps.is_empty() {
                bail!("failure verify-result must contain unmetRequirements or validationGaps");
            }
        }
    }

    Ok(())
}
