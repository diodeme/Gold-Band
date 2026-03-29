mod layout;

use crate::artifacts::{
    validate_exec_plan, validate_exec_result, ExecCommandResult, ExecPlanArtifact, ExecResultArtifact, ExecResultStatus,
};
use crate::domain::CommandStatus;
use anyhow::Result;
use camino::Utf8Path;
use serde::Serialize;
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use self::layout::{command_dir, command_json_path, commands_dir, stderr_log_path, stderr_rel_path, stdout_log_path, stdout_rel_path};

pub fn build_skipped_exec_result(plan: &ExecPlanArtifact) -> Result<ExecResultArtifact> {
    let commands = plan
        .commands
        .iter()
        .map(|command| ExecCommandResult {
            id: command.id.clone(),
            exit_code: None,
            status: CommandStatus::Skipped,
            start_time: None,
            end_time: None,
            duration_ms: None,
            timed_out: None,
            stdout_path: None,
            stderr_path: None,
        })
        .collect();

    Ok(ExecResultArtifact {
        version: "0.1".to_string(),
        status: ExecResultStatus::Success,
        commands,
    })
}

pub fn run_exec_plan(plan: &ExecPlanArtifact, workspace_dir: &Utf8Path, attempt_dir: &Utf8Path) -> Result<ExecResultArtifact> {
    validate_exec_plan(plan)?;

    let commands_dir = commands_dir(attempt_dir);
    std::fs::create_dir_all(commands_dir.as_std_path())?;

    let mut results = Vec::with_capacity(plan.commands.len());
    let mut failed = false;

    for (index, command) in plan.commands.iter().enumerate() {
        if failed {
            results.push(ExecCommandResult {
                id: command.id.clone(),
                exit_code: None,
                status: CommandStatus::Skipped,
                start_time: None,
                end_time: None,
                duration_ms: None,
                timed_out: None,
                stdout_path: None,
                stderr_path: None,
            });
            continue;
        }

        let command_dir = command_dir(attempt_dir, index, &command.id);
        std::fs::create_dir_all(command_dir.as_std_path())?;
        write_json_file(command_json_path(attempt_dir, index, &command.id).as_std_path(), command)?;

        let start_stamp = timestamp_like();
        let started = Instant::now();
        let cwd = command
            .cwd
            .as_deref()
            .map(|relative| workspace_dir.join(relative))
            .unwrap_or_else(|| workspace_dir.to_path_buf());

        let output = shell_command(command.run.as_str())
            .current_dir(cwd.as_std_path())
            .output()?;

        let end_stamp = timestamp_like();
        let duration_ms = started.elapsed().as_millis() as u64;

        let stdout_rel = stdout_rel_path(index, &command.id);
        let stderr_rel = stderr_rel_path(index, &command.id);
        std::fs::write(stdout_log_path(attempt_dir, index, &command.id).as_std_path(), &output.stdout)?;
        std::fs::write(stderr_log_path(attempt_dir, index, &command.id).as_std_path(), &output.stderr)?;

        let status = if output.status.success() {
            CommandStatus::Success
        } else {
            failed = true;
            CommandStatus::Failure
        };

        results.push(ExecCommandResult {
            id: command.id.clone(),
            exit_code: output.status.code(),
            status,
            start_time: Some(start_stamp),
            end_time: Some(end_stamp),
            duration_ms: Some(duration_ms),
            timed_out: Some(false),
            stdout_path: Some(stdout_rel),
            stderr_path: Some(stderr_rel),
        });
    }

    let top_level_status = if results.iter().any(|command| command.status == CommandStatus::Failure) {
        ExecResultStatus::Failure
    } else {
        ExecResultStatus::Success
    };

    let result = ExecResultArtifact {
        version: "0.1".to_string(),
        status: top_level_status,
        commands: results,
    };
    validate_exec_result(&result)?;
    Ok(result)
}

fn shell_command(command: &str) -> Command {
    if cfg!(target_os = "windows") {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.arg("-lc").arg(command);
        cmd
    }
}

fn timestamp_like() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("{secs}Z")
}

fn write_json_file(path: &std::path::Path, value: &impl Serialize) -> Result<()> {
    let content = serde_json::to_string_pretty(value)?;
    std::fs::write(path, content)?;
    Ok(())
}
