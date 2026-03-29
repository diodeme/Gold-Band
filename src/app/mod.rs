mod ids;
mod node_executor;
mod orchestrator;
mod state_access;
mod state_factory;
mod transition_context;

use crate::artifacts::{validate_exec_plan, validate_exec_result, validate_verify_result, ExecPlanArtifact, ExecResultArtifact, VerifyResultArtifact};
use crate::control::{decide_next_step, ControlDecision};
use crate::domain::{NodeOutcome, RunOutcome, RunStatus};
use crate::dsl::{validate_workflow, WorkflowDsl};
use crate::provider::{default_provider, ProviderAdapter};
use crate::runtime::{validate_node_state, validate_round_state, validate_run_state, validate_worker_ref_state, NodeState, RoundState, RunState, TaskState, WorkerRefState};
use crate::storage::{read_json, write_json, GoldBandPaths};
use anyhow::{anyhow, bail, Result};
use camino::{Utf8Path, Utf8PathBuf};
use std::fs;

use self::ids::now_rfc3339_like;
use self::orchestrator::{run_continue as orchestrator_run_continue, run_retry as orchestrator_run_retry, run_start as orchestrator_run_start};

pub struct App {
    pub paths: GoldBandPaths,
    provider: Box<dyn ProviderAdapter>,
}

impl App {
    pub fn new(repo_root: Utf8PathBuf) -> Self {
        Self {
            paths: GoldBandPaths::new(repo_root),
            provider: default_provider(),
        }
    }

    pub fn with_provider(repo_root: Utf8PathBuf, provider: Box<dyn ProviderAdapter>) -> Self {
        Self {
            paths: GoldBandPaths::new(repo_root),
            provider,
        }
    }

    pub fn task_show(&self, task_id: &str) -> Result<TaskState> {
        read_json(&self.paths.task_file(task_id))
    }

    pub fn artifact_show_path(&self, path: &Utf8Path) -> Result<String> {
        Ok(fs::read_to_string(path)?)
    }

    pub fn artifact_show(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str, name: &str) -> Result<String> {
        let path = self.paths.artifact_file(task_id, run_id, round_id, node_id, attempt_id, name);
        self.artifact_show_path(&path)
    }

    pub fn artifact_list(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str) -> Result<Vec<String>> {
        let dir = self.paths.artifacts_dir(task_id, run_id, round_id, node_id, attempt_id);
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut names = fs::read_dir(dir.as_std_path())?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| entry.file_name().to_str().map(ToOwned::to_owned))
            .collect::<Vec<_>>();
        names.sort();
        Ok(names)
    }

    pub fn run_status(&self, task_id: &str, run_id: &str) -> Result<RunState> {
        let run: RunState = read_json(&self.paths.run_file(task_id, run_id))?;
        validate_run_state(&run)?;
        Ok(run)
    }

    pub fn run_kill(&self, task_id: &str, run_id: &str) -> Result<RunState> {
        let mut run = self.run_status(task_id, run_id)?;
        run.status = RunStatus::Completed;
        run.outcome = Some(RunOutcome::Killed);
        run.pause_reason = None;
        run.updated_at = now_rfc3339_like();
        validate_run_state(&run)?;
        write_json(&self.paths.run_file(task_id, run_id), &run)?;

        if let Some(round_id) = &run.current_round {
            let mut round: RoundState = read_json(&self.paths.round_file(task_id, run_id, round_id))?;
            round.status = RunStatus::Completed;
            round.outcome = Some(RunOutcome::Killed);
            validate_round_state(&round)?;
            write_json(&self.paths.round_file(task_id, run_id, round_id), &round)?;

            if let (Some(node_id), Some(attempt_id)) = (&run.current_node, &run.current_attempt) {
                let node_path = self.paths.node_file(task_id, run_id, round_id, node_id, attempt_id);
                if node_path.exists() {
                    let mut node: NodeState = read_json(&node_path)?;
                    node.status = RunStatus::Completed;
                    node.outcome = Some(NodeOutcome::Killed);
                    node.finished_at = Some(now_rfc3339_like());
                    validate_node_state(&node)?;
                    write_json(&node_path, &node)?;
                }
            }
        }

        Ok(run)
    }

    pub fn run_open_session(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str) -> Result<String> {
        let worker_ref: WorkerRefState = read_json(&self.paths.worker_ref_file(task_id, run_id, round_id, node_id, attempt_id))?;
        validate_worker_ref_state(&worker_ref)?;
        if !worker_ref.supports_open_session {
            bail!("provider does not support open-session");
        }
        if let Some(command) = worker_ref.open_command.as_ref() {
            return Ok(command.clone());
        }
        let session_ref = crate::domain::SessionRef {
            provider: worker_ref.provider.clone(),
            mode: worker_ref.mode,
            supports_open_session: worker_ref.supports_open_session,
            supports_continue_session: worker_ref.supports_continue_session,
            continue_ref: worker_ref.continue_ref.clone(),
            open_command: worker_ref.open_command.clone(),
        };
        self.provider
            .build_continue_command(&session_ref)?
            .ok_or_else(|| anyhow!("provider did not return an open-session command"))
    }

    pub fn run_continue(&self, task_id: &str, run_id: &str) -> Result<RunState> {
        orchestrator_run_continue(self, task_id, run_id)
    }

    pub fn run_retry(&self, task_id: &str, run_id: &str) -> Result<RunState> {
        orchestrator_run_retry(self, task_id, run_id)
    }

    pub fn run_start(&self, task_id: &str, workflow_override: Option<&Utf8Path>) -> Result<RunState> {
        orchestrator_run_start(self, task_id, workflow_override)
    }

    pub fn decide(&self, workflow: WorkflowDsl, run: &RunState, round: &RoundState, node: &NodeState) -> Result<ControlDecision> {
        let validated = validate_workflow(workflow)?;
        Ok(decide_next_step(&validated, run, round, node))
    }

    pub fn validate_artifact_samples(
        &self,
        exec_plan: &ExecPlanArtifact,
        exec_result: &ExecResultArtifact,
        verify_result: &VerifyResultArtifact,
    ) -> Result<()> {
        validate_exec_plan(exec_plan)?;
        validate_exec_result(exec_result)?;
        validate_verify_result(verify_result)?;
        Ok(())
    }

}
