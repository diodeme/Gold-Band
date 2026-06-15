use anyhow::Result;

use crate::app::App;

use super::{ArtifactCommand, Command, CommandResult, RunCommand, TaskCommand};

pub fn execute_command(app: &App, command: Command) -> Result<CommandResult> {
    match command {
        Command::Task(command) => execute_task(app, command),
        Command::Run(command) => execute_run(app, command),
        Command::Artifact(command) => execute_artifact(app, command),
    }
}

fn execute_task(app: &App, command: TaskCommand) -> Result<CommandResult> {
    match command {
        TaskCommand::Show { task_id } => Ok(CommandResult::Json(serde_json::to_value(
            app.task_show(&task_id)?,
        )?)),
    }
}

fn execute_run(app: &App, command: RunCommand) -> Result<CommandResult> {
    match command {
        RunCommand::Start { task_id, workflow } => Ok(CommandResult::Json(serde_json::to_value(
            app.run_start(&task_id, workflow.as_deref())?,
        )?)),
        RunCommand::Status { task_id, run_id } => Ok(CommandResult::Json(serde_json::to_value(
            app.run_status(&task_id, &run_id)?,
        )?)),
        RunCommand::Continue { task_id, run_id } => Ok(CommandResult::Json(serde_json::to_value(
            app.run_continue(&task_id, &run_id, None, None)?,
        )?)),
        RunCommand::Retry { task_id, run_id } => Ok(CommandResult::Json(serde_json::to_value(
            app.run_retry(&task_id, &run_id)?,
        )?)),
        RunCommand::Kill { task_id, run_id } => Ok(CommandResult::Json(serde_json::to_value(
            app.run_kill(&task_id, &run_id)?,
        )?)),
        RunCommand::OpenSession {
            task_id,
            run_id,
            round,
            node,
            attempt,
        } => Ok(CommandResult::Text(
            app.run_open_session(&task_id, &run_id, &round, &node, &attempt)?,
        )),
    }
}

fn execute_artifact(app: &App, command: ArtifactCommand) -> Result<CommandResult> {
    match command {
        ArtifactCommand::List {
            task_id,
            run_id,
            round,
            node,
            attempt,
        } => Ok(CommandResult::Json(serde_json::to_value(
            app.artifact_list(&task_id, &run_id, &round, &node, &attempt)?,
        )?)),
        ArtifactCommand::Show {
            task_id,
            run_id,
            round,
            node,
            attempt,
            name,
        } => Ok(CommandResult::Text(app.artifact_show(
            &task_id, &run_id, &round, &node, &attempt, &name,
        )?)),
        ArtifactCommand::ShowPath { path } => {
            Ok(CommandResult::Text(app.artifact_show_path(&path)?))
        }
    }
}
