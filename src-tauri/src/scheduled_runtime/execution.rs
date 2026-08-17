use anyhow::Result;
use gold_band::scheduler::db::ScheduledTaskDatabase;
use gold_band::scheduler::occurrence::ScheduledOccurrence;
use gold_band::scheduler::{ScheduledMode, ScheduledTaskDefinition};
use tauri::AppHandle;

use super::{ExecutionResult, ScheduledExecutionAction, execute_definition_with_action};
use gold_band::app::App;

/// All scheduled modes receive the same immutable runtime inputs and return the
/// links that bind the occurrence to the accepted task/run/session chain.
pub struct ScheduledExecutionContext<'a> {
    pub app_handle: &'a AppHandle,
    pub app: &'a App,
    pub database: &'a ScheduledTaskDatabase,
    pub owner_id: &'a str,
    pub definition: &'a mut ScheduledTaskDefinition,
    pub occurrence: &'a ScheduledOccurrence,
    pub trigger_kind: &'a str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutionBinding {
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub round_id: Option<String>,
    pub attempt_id: Option<String>,
    pub session_id: Option<String>,
}

impl From<Option<gold_band::scheduler::occurrence::OccurrenceLinks>> for ExecutionBinding {
    fn from(links: Option<gold_band::scheduler::occurrence::OccurrenceLinks>) -> Self {
        let Some(links) = links else {
            return Self::default();
        };
        Self {
            task_id: links.task_id,
            run_id: links.run_id,
            round_id: links.round_id,
            attempt_id: links.attempt_id,
            session_id: None,
        }
    }
}

pub trait ScheduledExecutionAdapter: Send + Sync {
    fn start(&self, context: ScheduledExecutionContext<'_>) -> Result<ExecutionBinding>;
}

struct DirectNewAdapter;
struct DirectContinuousAdapter {
    task_id: String,
}
struct WorkflowAdapter {
    task_id: Option<String>,
}
struct AutoAdapter {
    task_id: Option<String>,
}

fn start_with_action(
    context: ScheduledExecutionContext<'_>,
    action: ScheduledExecutionAction,
) -> Result<ExecutionBinding> {
    execute_definition_with_action(
        context.app_handle,
        context.app,
        context.database,
        context.owner_id,
        context.definition,
        context.occurrence,
        context.trigger_kind,
        action,
    )
    .map(|result: ExecutionResult| result.immediate_links.into())
}

impl ScheduledExecutionAdapter for DirectNewAdapter {
    fn start(&self, context: ScheduledExecutionContext<'_>) -> Result<ExecutionBinding> {
        start_with_action(context, ScheduledExecutionAction::MaterializeTaskAndRun)
    }
}

impl ScheduledExecutionAdapter for DirectContinuousAdapter {
    fn start(&self, context: ScheduledExecutionContext<'_>) -> Result<ExecutionBinding> {
        start_with_action(
            context,
            ScheduledExecutionAction::ContinueSession {
                task_id: self.task_id.clone(),
            },
        )
    }
}

impl ScheduledExecutionAdapter for WorkflowAdapter {
    fn start(&self, context: ScheduledExecutionContext<'_>) -> Result<ExecutionBinding> {
        start_with_action(
            context,
            self.task_id
                .clone()
                .map(|task_id| ScheduledExecutionAction::StartNewRun { task_id })
                .unwrap_or(ScheduledExecutionAction::MaterializeTaskAndRun),
        )
    }
}

impl ScheduledExecutionAdapter for AutoAdapter {
    fn start(&self, context: ScheduledExecutionContext<'_>) -> Result<ExecutionBinding> {
        start_with_action(
            context,
            self.task_id
                .clone()
                .map(|task_id| ScheduledExecutionAction::StartNewRun { task_id })
                .unwrap_or(ScheduledExecutionAction::MaterializeTaskAndRun),
        )
    }
}

pub fn adapter_for(
    definition: &ScheduledTaskDefinition,
    action: &ScheduledExecutionAction,
) -> Box<dyn ScheduledExecutionAdapter> {
    match (&definition.mode, action) {
        (ScheduledMode::Direct, ScheduledExecutionAction::ContinueSession { task_id }) => {
            Box::new(DirectContinuousAdapter {
                task_id: task_id.clone(),
            })
        }
        (ScheduledMode::Workflow, ScheduledExecutionAction::StartNewRun { task_id }) => {
            Box::new(WorkflowAdapter {
                task_id: Some(task_id.clone()),
            })
        }
        (ScheduledMode::Workflow, ScheduledExecutionAction::MaterializeTaskAndRun) => {
            Box::new(WorkflowAdapter { task_id: None })
        }
        (ScheduledMode::Auto, ScheduledExecutionAction::StartNewRun { task_id }) => {
            Box::new(AutoAdapter {
                task_id: Some(task_id.clone()),
            })
        }
        (ScheduledMode::Auto, ScheduledExecutionAction::MaterializeTaskAndRun) => {
            Box::new(AutoAdapter { task_id: None })
        }
        _ => Box::new(DirectNewAdapter),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_defaults_without_links() {
        assert_eq!(ExecutionBinding::from(None), ExecutionBinding::default());
    }
}
