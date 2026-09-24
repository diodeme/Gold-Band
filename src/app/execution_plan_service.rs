use serde::{Deserialize, Serialize};

use std::fs;

use crate::config::ConversationAutoConfig;
use crate::dsl::{WorkflowDsl, validate_authoring_workflow, validate_workflow_snapshot};
use crate::execution_plan::{
    ExecutionPlanError, ExecutionPlanPayload, ExecutionPlanPreflight, ExecutionPlanRunMode,
    ExecutionPlanSaveCommand, ExecutionPlanSaveResult, ExecutionPlanSnapshot, ExecutionPlanTarget,
    ExecutionPlanTargetResult, ExecutionPlanView, OperationCommit, PlanResult,
    WorkflowAuthoringDraft, affected_node_ids, authoring_lock_key, compile_auto_workflow,
    current_guard, current_run_editable, evaluate_current_workflow, load_current, outcome_label,
    payload_eq, publish_revision_checked, read_authoring_revision, read_operation_commit,
    read_run_state, status_label, try_acquire_plan_write, validate_operation_id,
    workflow_from_snapshot, write_operation_commit,
};
use crate::runtime::RunState;
use crate::storage::{read_json, write_json};
use crate::workflow_model_binding::{
    TaskAuthoringWorkflow, WorkflowModelBindingError, WorkflowModelBindings,
    migrate_authoring_workflow, validate_and_inject,
};

use super::App;

const OPERATION_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StagedCurrent {
    run_mode: ExecutionPlanRunMode,
    payload: ExecutionPlanPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StagedNext {
    workflow: Option<WorkflowAuthoringDraft>,
    auto_config: Option<ConversationAutoConfig>,
}

impl App {
    pub fn get_conversation_execution_plan(
        &self,
        project_id: &str,
        task_id: &str,
        task_uuid: &str,
        run_id: &str,
    ) -> PlanResult<ExecutionPlanView> {
        let run = self.ensure_plan_locator(project_id, task_id, task_uuid, run_id)?;
        self.execution_plan_view(&run)
    }

    pub fn preflight_conversation_execution_plan_save(
        &self,
        command: &ExecutionPlanSaveCommand,
    ) -> PlanResult<ExecutionPlanPreflight> {
        let run = self.ensure_plan_command_locator(command)?;
        let snapshot = load_current(&self.paths, &command.task_id, &command.run_id)?;
        self.preflight_for(&run, &snapshot, command)
    }

    pub fn save_conversation_execution_plan(
        &self,
        command: &ExecutionPlanSaveCommand,
    ) -> PlanResult<ExecutionPlanSaveResult> {
        if let Some(operation_id) = command.operation_id.as_deref() {
            validate_operation_id(operation_id)?;
            if let Some(result) =
                self.committed_operation_result(&command.task_id, &command.run_id, operation_id)?
            {
                return Ok(result);
            }
        }
        if command.target == ExecutionPlanTarget::CurrentAndNext && command.operation_id.is_none() {
            return Err(ExecutionPlanError::new(
                crate::execution_plan::error::VALIDATION_FAILED,
                serde_json::json!({ "field": "operationId" }),
            ));
        }
        let run = self.ensure_plan_command_locator(command)?;
        let snapshot = load_current(&self.paths, &command.task_id, &command.run_id)?;
        let preflight = self.preflight_for(&run, &snapshot, command)?;
        if let Some(blocking) = preflight.blocking.first() {
            return Err(blocking.clone());
        }
        let staged_current = self.staged_current(command, &snapshot)?;
        let staged_next = self.staged_next(command)?;
        if let Some(operation_id) = command.operation_id.as_deref() {
            self.write_operation_stage(command, operation_id, &staged_current, &staged_next)?;
        }

        let mut targets = Vec::new();
        let mut plan_revision = snapshot.plan_revision;
        let mut authoring_revision = preflight.authoring_revision;
        if command.target.includes_current() {
            match self.commit_current(command, staged_current.as_ref()) {
                Ok(published) => {
                    plan_revision = published.plan_revision;
                    targets.push(target_ok(
                        ExecutionPlanTarget::Current,
                        Some(plan_revision),
                        None,
                    ));
                    self.mark_operation(
                        command,
                        "current-published",
                        plan_revision,
                        authoring_revision,
                        true,
                        false,
                    )?;
                }
                Err(error) => {
                    targets.push(ExecutionPlanTargetResult {
                        target: ExecutionPlanTarget::Current,
                        committed: false,
                        plan_revision: None,
                        authoring_revision: None,
                        error: Some(error.clone()),
                    });
                    if command.target.includes_next() {
                        targets.push(ExecutionPlanTargetResult {
                            target: ExecutionPlanTarget::Next,
                            committed: false,
                            plan_revision: None,
                            authoring_revision: None,
                            error: Some(ExecutionPlanError::new(
                                crate::execution_plan::error::PARTIAL_COMMIT,
                                serde_json::json!({ "reason": "current-failed" }),
                            )),
                        });
                        return Ok(self.save_result(
                            command,
                            false,
                            plan_revision,
                            authoring_revision,
                            targets,
                        )?);
                    }
                    return Err(error);
                }
            }
        }
        if command.target.includes_next() {
            match self.commit_next(command, staged_next.as_ref()) {
                Ok(revision) => {
                    authoring_revision = revision;
                    targets.push(target_ok(
                        ExecutionPlanTarget::Next,
                        None,
                        Some(authoring_revision),
                    ));
                    self.mark_operation(
                        command,
                        "committed",
                        plan_revision,
                        authoring_revision,
                        command.target.includes_current(),
                        true,
                    )?;
                }
                Err(error) => {
                    if command.target.includes_current() {
                        targets.push(ExecutionPlanTargetResult {
                            target: ExecutionPlanTarget::Next,
                            committed: false,
                            plan_revision: None,
                            authoring_revision: None,
                            error: Some(error),
                        });
                        self.mark_operation(
                            command,
                            "current-published",
                            plan_revision,
                            authoring_revision,
                            true,
                            false,
                        )?;
                        return Ok(self.save_result(
                            command,
                            false,
                            plan_revision,
                            authoring_revision,
                            targets,
                        )?);
                    }
                    return Err(error);
                }
            }
        } else {
            self.mark_operation(
                command,
                "committed",
                plan_revision,
                authoring_revision,
                true,
                false,
            )?;
        }
        self.save_result(command, true, plan_revision, authoring_revision, targets)
    }

    pub fn recover_conversation_execution_plan_operation(
        &self,
        project_id: &str,
        task_id: &str,
        task_uuid: &str,
        run_id: &str,
        operation_id: &str,
    ) -> PlanResult<ExecutionPlanSaveResult> {
        validate_operation_id(operation_id)?;
        let _run = self.ensure_plan_locator(project_id, task_id, task_uuid, run_id)?;
        if let Some(result) = self.committed_operation_result(task_id, run_id, operation_id)? {
            return Ok(result);
        }
        let Some(commit) = read_operation_commit(&self.paths, task_id, run_id, operation_id)?
        else {
            return Err(ExecutionPlanError::new(
                crate::execution_plan::error::RECOVERY_REQUIRED,
                serde_json::json!({ "operationId": operation_id, "reason": "operation-missing" }),
            ));
        };
        let staged_current = self.read_staged_current(task_id, run_id, operation_id)?;
        let staged_next = self.read_staged_next(task_id, run_id, operation_id)?;
        let mut command = self.read_operation_command(task_id, run_id, operation_id)?;
        command.operation_id = Some(operation_id.to_string());
        let snapshot = load_current(&self.paths, task_id, run_id)?;
        let current_done = commit.current_committed
            || staged_current.as_ref().is_some_and(|staged| {
                snapshot.plan_revision == commit.plan_revision
                    && payload_eq(&snapshot.payload, &staged.payload)
            });
        if command.target.includes_current() && !current_done {
            if snapshot.plan_revision != command.expected_plan_revision {
                return Err(ExecutionPlanError::new(
                    crate::execution_plan::error::RECOVERY_REQUIRED,
                    serde_json::json!({ "operationId": operation_id, "reason": "plan-moved" }),
                ));
            }
            let published = self.commit_current(&command, staged_current.as_ref())?;
            self.mark_operation(
                &command,
                "current-published",
                published.plan_revision,
                commit.authoring_revision,
                true,
                false,
            )?;
        }
        let plan_revision = load_current(&self.paths, task_id, run_id)?.plan_revision;
        let authoring_revision = if command.target.includes_next() && !commit.next_committed {
            self.commit_next(&command, staged_next.as_ref())?
        } else {
            self.authoring_revision_for(
                self.effective_run_mode(task_id, &snapshot),
                &command.project_id,
                task_id,
            )?
        };
        self.mark_operation(
            &command,
            "committed",
            plan_revision,
            authoring_revision,
            command.target.includes_current(),
            command.target.includes_next(),
        )?;
        self.committed_operation_result(task_id, run_id, operation_id)?
            .ok_or_else(|| {
                ExecutionPlanError::new(
                    crate::execution_plan::error::RECOVERY_REQUIRED,
                    serde_json::json!({ "operationId": operation_id }),
                )
            })
    }
}

impl App {
    fn execution_plan_view(&self, run: &RunState) -> PlanResult<ExecutionPlanView> {
        let snapshot = load_current(&self.paths, &run.task_id, &run.id)?;
        let run_mode = self.effective_run_mode(&run.task_id, &snapshot);
        let (current_workflow, current_bindings, mut current_auto) =
            payload_view(&snapshot.payload);
        let (next_workflow, next_bindings, next_auto, authoring_revision) =
            self.next_authoring(run_mode, &run.task_id)?;
        if run_mode == ExecutionPlanRunMode::Auto && current_auto.is_none() {
            current_auto = next_auto.clone();
        }
        let diverged = self.plans_diverged(
            run_mode,
            &snapshot,
            next_workflow.as_ref(),
            &next_bindings,
            next_auto.as_ref(),
        );
        Ok(ExecutionPlanView {
            project_id: self.paths.project_id.clone(),
            task_id: run.task_id.clone(),
            task_uuid: run.task_uuid.clone().unwrap_or_else(|| run.task_id.clone()),
            run_id: run.id.clone(),
            run_mode: run_mode.as_str().to_string(),
            run_status: status_label(run.status).to_string(),
            run_outcome: run.outcome.map(outcome_label).map(str::to_string),
            plan_revision: snapshot.plan_revision,
            authoring_revision,
            execution_revision: run.execution.revision,
            current_editable: current_run_editable(run.status, run.outcome),
            diverged,
            current_round: run.current_round.clone(),
            current_node: run.current_node.clone(),
            current_attempt: run.current_attempt.clone(),
            current_workflow,
            current_model_bindings: current_bindings,
            current_auto_config: current_auto,
            next_workflow,
            next_model_bindings: next_bindings,
            next_auto_config: next_auto,
        })
    }

    fn preflight_for(
        &self,
        run: &RunState,
        snapshot: &ExecutionPlanSnapshot,
        command: &ExecutionPlanSaveCommand,
    ) -> PlanResult<ExecutionPlanPreflight> {
        self.recheck_command_expectations(run, snapshot, command, false)?;
        let run_mode = self.effective_run_mode(&run.task_id, snapshot);
        let mut blocking = Vec::new();
        let mut resume_identity_risks = Vec::new();
        let mut affected = Vec::new();
        if command.target.includes_current() {
            let staged = self.staged_current(command, snapshot);
            match staged {
                Ok(Some(staged)) => {
                    let next_workflow = workflow_from_payload(&staged.payload);
                    let previous = workflow_from_snapshot(snapshot);
                    let guard = current_guard(&self.paths, &run.task_id, run)?;
                    let issues = evaluate_current_workflow(&previous, &next_workflow, &guard);
                    affected = affected_node_ids(&previous, &next_workflow);
                    for issue in issues {
                        if matches!(
                            issue.code(),
                            crate::execution_plan::error::AGENT_IDENTITY_CHANGED
                                | crate::execution_plan::error::CONTINUE_UNSUPPORTED
                        ) {
                            resume_identity_risks.push(issue.clone());
                        }
                        blocking.push(issue);
                    }
                }
                Ok(None) => blocking.push(missing_draft(run_mode)),
                Err(error) => blocking.push(error),
            }
        }
        if command.target.includes_next() && self.staged_next(command)?.is_none() {
            blocking.push(missing_draft(run_mode));
        }
        let authoring_revision =
            self.authoring_revision_for(run_mode, &command.project_id, &command.task_id)?;
        if command.target.includes_next()
            && authoring_revision != command.expected_authoring_revision
        {
            blocking.push(ExecutionPlanError::new(
                crate::execution_plan::error::AUTHORING_CONFLICT,
                serde_json::json!({
                    "expectedAuthoringRevision": command.expected_authoring_revision,
                    "authoringRevision": authoring_revision,
                }),
            ));
        }
        if command.target.includes_current()
            && snapshot.plan_revision != command.expected_plan_revision
        {
            blocking.push(ExecutionPlanError::new(
                crate::execution_plan::error::REVISION_CONFLICT,
                serde_json::json!({
                    "expectedPlanRevision": command.expected_plan_revision,
                    "planRevision": snapshot.plan_revision,
                }),
            ));
        }
        let (next_workflow, next_bindings, next_auto, _) =
            self.next_authoring(run_mode, &run.task_id)?;
        Ok(ExecutionPlanPreflight {
            plan_revision: snapshot.plan_revision,
            authoring_revision,
            execution_revision: run.execution.revision,
            run_status: status_label(run.status).to_string(),
            run_outcome: run.outcome.map(outcome_label).map(str::to_string),
            current_round: run.current_round.clone(),
            current_node: run.current_node.clone(),
            current_attempt: run.current_attempt.clone(),
            current_editable: current_run_editable(run.status, run.outcome),
            diverged: self.plans_diverged(
                run_mode,
                snapshot,
                next_workflow.as_ref(),
                &next_bindings,
                next_auto.as_ref(),
            ),
            blocking,
            affected_node_ids: affected,
            resume_identity_risks,
        })
    }

    fn commit_current(
        &self,
        command: &ExecutionPlanSaveCommand,
        staged: Option<&StagedCurrent>,
    ) -> PlanResult<ExecutionPlanSnapshot> {
        let Some(staged) = staged else {
            return Err(missing_draft(ExecutionPlanRunMode::Workflow));
        };
        let executable = workflow_from_payload(&staged.payload);
        publish_revision_checked(
            &self.paths,
            &command.task_id,
            &command.run_id,
            command.expected_plan_revision,
            staged.run_mode,
            command.expected_authoring_revision,
            staged.payload.clone(),
            |previous| {
                let run = read_run_state(&self.paths, &command.task_id, &command.run_id)?;
                self.recheck_locator(&run, command)?;
                let guard = current_guard(&self.paths, &command.task_id, &run)?;
                let issues = evaluate_current_workflow(
                    &workflow_from_snapshot(previous),
                    &executable,
                    &guard,
                );
                if let Some(issue) = issues.into_iter().next() {
                    return Err(issue);
                }
                Ok(())
            },
        )
    }

    fn commit_next(
        &self,
        command: &ExecutionPlanSaveCommand,
        staged: Option<&StagedNext>,
    ) -> PlanResult<u64> {
        let _lock = try_acquire_plan_write(&authoring_lock_key(
            &self.paths.project_id,
            &command.task_id,
        ))?;
        let snapshot = load_current(&self.paths, &command.task_id, &command.run_id)?;
        let run_mode = self.effective_run_mode(&command.task_id, &snapshot);
        match run_mode {
            ExecutionPlanRunMode::Workflow => {
                let draft = staged
                    .and_then(|staged| staged.workflow.clone())
                    .ok_or_else(|| missing_draft(run_mode))?;
                let revision = read_authoring_revision(&self.paths, &command.task_id);
                if self
                    .task_authoring_workflow(&command.task_id)
                    .ok()
                    .is_some_and(|authoring| {
                        json_eq(&authoring.workflow, &draft.workflow)
                            && json_eq(&authoring.model_bindings, &draft.model_bindings)
                    })
                {
                    return Ok(revision);
                }
                if revision != command.expected_authoring_revision {
                    return Err(ExecutionPlanError::new(
                        crate::execution_plan::error::AUTHORING_CONFLICT,
                        serde_json::json!({
                            "expectedAuthoringRevision": command.expected_authoring_revision,
                            "authoringRevision": revision,
                        }),
                    ));
                }
                self.save_task_authoring_workflow_unlocked(
                    &command.task_id,
                    TaskAuthoringWorkflow {
                        workflow: draft.workflow,
                        model_bindings: draft.model_bindings,
                    },
                )
                .map_err(plan_anyhow)?;
                Ok(read_authoring_revision(&self.paths, &command.task_id))
            }
            ExecutionPlanRunMode::Auto => {
                let mut config = staged
                    .and_then(|staged| staged.auto_config.clone())
                    .ok_or_else(|| missing_draft(run_mode))?;
                config.active_template_id = None;
                config.active_template_name = None;
                let path = self.paths.task_auto_config_file(&command.task_id);
                let revision = read_authoring_revision(&self.paths, &command.task_id);
                let stored_matches = read_json::<ConversationAutoConfig>(&path)
                    .ok()
                    .is_some_and(|stored| json_eq(&stored, &config));
                if revision != command.expected_authoring_revision && !stored_matches {
                    return Err(ExecutionPlanError::new(
                        crate::execution_plan::error::AUTHORING_CONFLICT,
                        serde_json::json!({
                            "expectedAuthoringRevision": command.expected_authoring_revision,
                            "authoringRevision": revision,
                        }),
                    ));
                }
                if !stored_matches {
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent.as_std_path()).map_err(|error| {
                            ExecutionPlanError::new(
                                crate::execution_plan::error::VALIDATION_FAILED,
                                serde_json::json!({ "cause": "auto-authoring-dir", "detail": error.to_string() }),
                            )
                        })?;
                    }
                    write_json(&path, &config).map_err(|error| {
                        ExecutionPlanError::new(
                            crate::execution_plan::error::VALIDATION_FAILED,
                            serde_json::json!({ "cause": "auto-authoring-write", "detail": error.to_string() }),
                        )
                    })?;
                }
                let compiled = compile_auto_workflow(Some(&config));
                let projection_current = self
                    .task_authoring_workflow(&command.task_id)
                    .ok()
                    .is_some_and(|authoring| json_eq(&authoring.workflow, &compiled));
                if !projection_current {
                    self.save_task_authoring_workflow_unlocked(
                        &command.task_id,
                        TaskAuthoringWorkflow {
                            workflow: compiled,
                            model_bindings: WorkflowModelBindings::default(),
                        },
                    )
                    .map_err(plan_anyhow)?;
                } else if !stored_matches {
                    crate::execution_plan::bump_authoring_revision(&self.paths, &command.task_id)?;
                }
                Ok(read_authoring_revision(&self.paths, &command.task_id))
            }
        }
    }

    fn staged_current(
        &self,
        command: &ExecutionPlanSaveCommand,
        snapshot: &ExecutionPlanSnapshot,
    ) -> PlanResult<Option<StagedCurrent>> {
        if !command.target.includes_current() {
            return Ok(None);
        }
        match self.effective_run_mode(&command.task_id, snapshot) {
            ExecutionPlanRunMode::Workflow => {
                let Some(draft) = command.workflow.clone() else {
                    return Ok(None);
                };
                let prepared = prepare_workflow_draft(&draft)?;
                let executable = self.inject_workflow(&prepared)?;
                Ok(Some(StagedCurrent {
                    run_mode: ExecutionPlanRunMode::Workflow,
                    payload: ExecutionPlanPayload::Workflow {
                        workflow: executable,
                        model_bindings: prepared.model_bindings,
                    },
                }))
            }
            ExecutionPlanRunMode::Auto => {
                let Some(config) = command.auto_config.clone() else {
                    return Ok(None);
                };
                let workflow = compile_auto_workflow(Some(&config));
                let validated =
                    validate_workflow_snapshot(workflow.clone()).map_err(plan_anyhow)?;
                self.validate_workflow_agents(&validated)
                    .map_err(plan_anyhow)?;
                let template_store = self.load_workflow_template_store().map_err(plan_anyhow)?;
                super::validate_ai_dynamic_allowed_workflows(&workflow, &template_store)
                    .map_err(plan_anyhow)?;
                Ok(Some(StagedCurrent {
                    run_mode: ExecutionPlanRunMode::Auto,
                    payload: ExecutionPlanPayload::Auto { config },
                }))
            }
        }
    }

    fn staged_next(&self, command: &ExecutionPlanSaveCommand) -> PlanResult<Option<StagedNext>> {
        if !command.target.includes_next() {
            return Ok(None);
        }
        if let Some(draft) = command.workflow.clone() {
            let prepared = prepare_workflow_draft(&draft)?;
            return Ok(Some(StagedNext {
                workflow: Some(prepared),
                auto_config: command.auto_config.clone(),
            }));
        }
        if command.auto_config.is_some() {
            return Ok(Some(StagedNext {
                workflow: None,
                auto_config: command.auto_config.clone(),
            }));
        }
        Ok(None)
    }

    fn inject_workflow(&self, draft: &WorkflowAuthoringDraft) -> PlanResult<WorkflowDsl> {
        let diagnostics = self.provider_diagnostics();
        let executable = validate_and_inject(
            &draft.workflow,
            &draft.model_bindings,
            &self.config.agents,
            &diagnostics,
        )
        .map_err(binding_error)?;
        validate_workflow_snapshot(executable.clone()).map_err(plan_anyhow)?;
        Ok(executable)
    }

    fn ensure_plan_command_locator(
        &self,
        command: &ExecutionPlanSaveCommand,
    ) -> PlanResult<RunState> {
        self.ensure_plan_locator(
            &command.project_id,
            &command.task_id,
            &command.task_uuid,
            &command.run_id,
        )
    }

    fn ensure_plan_locator(
        &self,
        project_id: &str,
        task_id: &str,
        task_uuid: &str,
        run_id: &str,
    ) -> PlanResult<RunState> {
        if project_id != self.paths.project_id {
            return Err(locator_conflict("projectId"));
        }
        let run = read_run_state(&self.paths, task_id, run_id)?;
        if run.task_id != task_id || run.id != run_id {
            return Err(locator_conflict("runId"));
        }
        if run
            .task_uuid
            .as_deref()
            .is_some_and(|uuid| uuid != task_uuid)
        {
            return Err(locator_conflict("taskUuid"));
        }
        Ok(run)
    }

    fn recheck_command_expectations(
        &self,
        run: &RunState,
        snapshot: &ExecutionPlanSnapshot,
        command: &ExecutionPlanSaveCommand,
        enforce_revision: bool,
    ) -> PlanResult<()> {
        self.recheck_locator(run, command)?;
        if enforce_revision
            && command.target.includes_current()
            && snapshot.plan_revision != command.expected_plan_revision
        {
            return Err(ExecutionPlanError::new(
                crate::execution_plan::error::REVISION_CONFLICT,
                serde_json::json!({
                    "expectedPlanRevision": command.expected_plan_revision,
                    "planRevision": snapshot.plan_revision,
                }),
            ));
        }
        Ok(())
    }

    fn recheck_locator(
        &self,
        run: &RunState,
        command: &ExecutionPlanSaveCommand,
    ) -> PlanResult<()> {
        if status_label(run.status) != command.expected_run_status
            || run.current_round != command.expected_current_round
            || run.current_node != command.expected_current_node
            || run.current_attempt != command.expected_current_attempt
        {
            return Err(ExecutionPlanError::new(
                crate::execution_plan::error::CURRENT_LOCATOR_CONFLICT,
                serde_json::json!({
                    "runStatus": status_label(run.status),
                    "currentRound": run.current_round,
                    "currentNode": run.current_node,
                    "currentAttempt": run.current_attempt,
                }),
            ));
        }
        Ok(())
    }

    fn authoring_revision_for(
        &self,
        _run_mode: ExecutionPlanRunMode,
        _project_id: &str,
        task_id: &str,
    ) -> PlanResult<u64> {
        Ok(read_authoring_revision(&self.paths, task_id))
    }

    fn conversation_is_auto(&self, task_id: &str) -> bool {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct ModeFile {
            #[serde(default)]
            run_mode: String,
        }
        let path = self
            .paths
            .task_dir(task_id)
            .join("authoring/conversation.json");
        read_json::<ModeFile>(&path)
            .ok()
            .is_some_and(|file| file.run_mode == "auto")
    }

    fn effective_run_mode(
        &self,
        task_id: &str,
        snapshot: &ExecutionPlanSnapshot,
    ) -> ExecutionPlanRunMode {
        if self.conversation_is_auto(task_id) {
            ExecutionPlanRunMode::Auto
        } else {
            snapshot.run_mode
        }
    }

    pub fn conversation_auto_config_for_task(
        &self,
        task_id: &str,
    ) -> Result<ConversationAutoConfig, anyhow::Error> {
        let (_, _, config, _) = self
            .next_authoring(ExecutionPlanRunMode::Auto, task_id)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        config.ok_or_else(|| anyhow::anyhow!("conversation.execution-plan.not-found"))
    }

    fn next_authoring(
        &self,
        run_mode: ExecutionPlanRunMode,
        task_id: &str,
    ) -> PlanResult<(
        Option<WorkflowDsl>,
        WorkflowModelBindings,
        Option<ConversationAutoConfig>,
        u64,
    )> {
        match run_mode {
            ExecutionPlanRunMode::Workflow => {
                let authoring = self.task_authoring_workflow(task_id).map_err(plan_anyhow)?;
                let revision = read_authoring_revision(&self.paths, task_id);
                Ok((
                    Some(authoring.workflow),
                    authoring.model_bindings,
                    None,
                    revision,
                ))
            }
            ExecutionPlanRunMode::Auto => {
                let path = self.paths.task_auto_config_file(task_id);
                let stored = path
                    .exists()
                    .then(|| read_json::<ConversationAutoConfig>(&path).ok())
                    .flatten();
                let config = if stored.is_some() {
                    stored
                } else {
                    let state = self.load_state().map_err(plan_anyhow)?;
                    state
                        .conversation_run_modes
                        .get(&self.paths.project_id)
                        .and_then(|entry| entry.auto_config.clone())
                };
                Ok((
                    None,
                    WorkflowModelBindings::default(),
                    config,
                    read_authoring_revision(&self.paths, task_id),
                ))
            }
        }
    }

    fn plans_diverged(
        &self,
        run_mode: ExecutionPlanRunMode,
        snapshot: &ExecutionPlanSnapshot,
        next_workflow: Option<&WorkflowDsl>,
        next_bindings: &WorkflowModelBindings,
        next_auto: Option<&ConversationAutoConfig>,
    ) -> bool {
        match run_mode {
            ExecutionPlanRunMode::Auto => match &snapshot.payload {
                ExecutionPlanPayload::Auto { config } => {
                    next_auto.is_none_or(|next| !json_eq(config, next))
                }
                ExecutionPlanPayload::Workflow { .. } => false,
            },
            ExecutionPlanRunMode::Workflow => {
                let Some(next_workflow) = next_workflow else {
                    return true;
                };
                let Ok(executable) = self.inject_workflow(&WorkflowAuthoringDraft {
                    workflow: next_workflow.clone(),
                    model_bindings: next_bindings.clone(),
                }) else {
                    return false;
                };
                !json_eq(&workflow_from_snapshot(snapshot), &executable)
            }
        }
    }

    fn save_result(
        &self,
        command: &ExecutionPlanSaveCommand,
        complete: bool,
        plan_revision: u64,
        authoring_revision: u64,
        targets: Vec<ExecutionPlanTargetResult>,
    ) -> PlanResult<ExecutionPlanSaveResult> {
        let run = read_run_state(&self.paths, &command.task_id, &command.run_id)?;
        Ok(ExecutionPlanSaveResult {
            operation_id: command.operation_id.clone(),
            complete,
            plan_revision,
            authoring_revision,
            execution_revision: run.execution.revision,
            diverged: self.diverged_now(&command.task_id, &command.run_id)?,
            targets,
        })
    }

    fn diverged_now(&self, task_id: &str, run_id: &str) -> PlanResult<bool> {
        let snapshot = load_current(&self.paths, task_id, run_id)?;
        let run_mode = self.effective_run_mode(task_id, &snapshot);
        let (next_workflow, next_bindings, next_auto, _) =
            self.next_authoring(run_mode, task_id)?;
        Ok(self.plans_diverged(
            run_mode,
            &snapshot,
            next_workflow.as_ref(),
            &next_bindings,
            next_auto.as_ref(),
        ))
    }

    fn write_operation_stage(
        &self,
        command: &ExecutionPlanSaveCommand,
        operation_id: &str,
        current: &Option<StagedCurrent>,
        next: &Option<StagedNext>,
    ) -> PlanResult<()> {
        let dir = self.paths.execution_plan_operation_dir(
            &command.task_id,
            &command.run_id,
            operation_id,
        );
        crate::storage::write_json(&dir.join("request.json"), command).map_err(plan_anyhow)?;
        if let Some(current) = current {
            crate::storage::write_json(&dir.join("current.staged.json"), current)
                .map_err(plan_anyhow)?;
        }
        if let Some(next) = next {
            crate::storage::write_json(&dir.join("next.staged.json"), next).map_err(plan_anyhow)?;
        }
        Ok(())
    }

    fn mark_operation(
        &self,
        command: &ExecutionPlanSaveCommand,
        phase: &str,
        plan_revision: u64,
        authoring_revision: u64,
        current_committed: bool,
        next_committed: bool,
    ) -> PlanResult<()> {
        let Some(operation_id) = command.operation_id.clone() else {
            return Ok(());
        };
        write_operation_commit(
            &self.paths,
            &command.task_id,
            &command.run_id,
            &OperationCommit {
                schema_version: OPERATION_SCHEMA_VERSION,
                operation_id,
                phase: phase.to_string(),
                plan_revision,
                authoring_revision,
                current_committed,
                next_committed,
            },
        )
    }

    fn committed_operation_result(
        &self,
        task_id: &str,
        run_id: &str,
        operation_id: &str,
    ) -> PlanResult<Option<ExecutionPlanSaveResult>> {
        let Some(commit) = read_operation_commit(&self.paths, task_id, run_id, operation_id)?
        else {
            return Ok(None);
        };
        if commit.phase != "committed" {
            return Ok(None);
        }
        let run = read_run_state(&self.paths, task_id, run_id)?;
        let mut targets = Vec::new();
        if commit.current_committed {
            targets.push(target_ok(
                ExecutionPlanTarget::Current,
                Some(commit.plan_revision),
                None,
            ));
        }
        if commit.next_committed {
            targets.push(target_ok(
                ExecutionPlanTarget::Next,
                None,
                Some(commit.authoring_revision),
            ));
        }
        Ok(Some(ExecutionPlanSaveResult {
            operation_id: Some(operation_id.to_string()),
            complete: true,
            plan_revision: commit.plan_revision,
            authoring_revision: commit.authoring_revision,
            execution_revision: run.execution.revision,
            diverged: self.diverged_now(task_id, run_id)?,
            targets,
        }))
    }

    fn read_operation_command(
        &self,
        task_id: &str,
        run_id: &str,
        operation_id: &str,
    ) -> PlanResult<ExecutionPlanSaveCommand> {
        let path = self
            .paths
            .execution_plan_operation_dir(task_id, run_id, operation_id)
            .join("request.json");
        crate::storage::read_json(&path).map_err(plan_anyhow)
    }

    fn read_staged_current(
        &self,
        task_id: &str,
        run_id: &str,
        operation_id: &str,
    ) -> PlanResult<Option<StagedCurrent>> {
        let path = self
            .paths
            .execution_plan_operation_dir(task_id, run_id, operation_id)
            .join("current.staged.json");
        if !path.exists() {
            return Ok(None);
        }
        crate::storage::read_json(&path)
            .map(Some)
            .map_err(plan_anyhow)
    }

    fn read_staged_next(
        &self,
        task_id: &str,
        run_id: &str,
        operation_id: &str,
    ) -> PlanResult<Option<StagedNext>> {
        let path = self
            .paths
            .execution_plan_operation_dir(task_id, run_id, operation_id)
            .join("next.staged.json");
        if !path.exists() {
            return Ok(None);
        }
        crate::storage::read_json(&path)
            .map(Some)
            .map_err(plan_anyhow)
    }
}

fn prepare_workflow_draft(draft: &WorkflowAuthoringDraft) -> PlanResult<WorkflowAuthoringDraft> {
    let mut workflow = draft.workflow.clone();
    let mut model_bindings = draft.model_bindings.clone();
    migrate_authoring_workflow(&mut workflow, &mut model_bindings, None).map_err(binding_error)?;
    validate_authoring_workflow(workflow.clone()).map_err(plan_anyhow)?;
    Ok(WorkflowAuthoringDraft {
        workflow,
        model_bindings,
    })
}

fn payload_view(
    payload: &ExecutionPlanPayload,
) -> (
    Option<WorkflowDsl>,
    WorkflowModelBindings,
    Option<ConversationAutoConfig>,
) {
    match payload {
        ExecutionPlanPayload::Workflow {
            workflow,
            model_bindings,
        } => (Some(workflow.clone()), model_bindings.clone(), None),
        ExecutionPlanPayload::Auto { config } => (
            Some(compile_auto_workflow(Some(config))),
            WorkflowModelBindings::default(),
            Some(config.clone()),
        ),
    }
}

fn workflow_from_payload(payload: &ExecutionPlanPayload) -> WorkflowDsl {
    match payload {
        ExecutionPlanPayload::Workflow { workflow, .. } => workflow.clone(),
        ExecutionPlanPayload::Auto { config } => compile_auto_workflow(Some(config)),
    }
}

fn missing_draft(run_mode: ExecutionPlanRunMode) -> ExecutionPlanError {
    ExecutionPlanError::new(
        crate::execution_plan::error::VALIDATION_FAILED,
        serde_json::json!({ "field": "draft", "runMode": run_mode.as_str() }),
    )
}

fn locator_conflict(field: &str) -> ExecutionPlanError {
    ExecutionPlanError::new(
        crate::execution_plan::error::CURRENT_LOCATOR_CONFLICT,
        serde_json::json!({ "field": field }),
    )
}

fn binding_error(error: WorkflowModelBindingError) -> ExecutionPlanError {
    ExecutionPlanError::new(
        crate::execution_plan::error::VALIDATION_FAILED,
        serde_json::json!({ "cause": error.code(), "params": error.params() }),
    )
}

fn plan_anyhow(error: anyhow::Error) -> ExecutionPlanError {
    if let Some(validation) = error.downcast_ref::<crate::dsl::WorkflowValidationError>() {
        return ExecutionPlanError::new(
            crate::execution_plan::error::VALIDATION_FAILED,
            serde_json::to_value(validation)
                .unwrap_or_else(|_| serde_json::json!({ "cause": "workflow" })),
        );
    }
    if let Some(binding) = error.downcast_ref::<WorkflowModelBindingError>() {
        return binding_error(binding.clone());
    }
    if let Some(plan) = error.downcast_ref::<ExecutionPlanError>() {
        return plan.clone();
    }
    ExecutionPlanError::new(
        crate::execution_plan::error::VALIDATION_FAILED,
        serde_json::json!({ "cause": "workflow-invalid" }),
    )
}

fn target_ok(
    target: ExecutionPlanTarget,
    plan_revision: Option<u64>,
    authoring_revision: Option<u64>,
) -> ExecutionPlanTargetResult {
    ExecutionPlanTargetResult {
        target,
        committed: true,
        plan_revision,
        authoring_revision,
        error: None,
    }
}

fn json_eq<T: Serialize>(left: &T, right: &T) -> bool {
    serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    use crate::app::tests::{
        env_guard, test_app_with_named_provider_capabilities, worker_workflow,
    };
    use crate::app::{App, CreateTaskInput};
    use crate::config::{ConversationAutoConfig, ConversationRunMode, ConversationRunModeEntry};
    use crate::domain::{RunOutcome, RunStatus};
    use crate::dsl::NodeDsl;
    use crate::execution_plan::error;
    use crate::execution_plan::{
        ExecutionPlanSaveCommand, ExecutionPlanTarget, ExecutionPlanView, WorkflowAuthoringDraft,
        ai_dynamic_node_for_dispatch, compile_auto_workflow, publish_initial_auto,
        publish_initial_workflow,
    };
    use crate::runtime::{RunState, RuntimeExecutionPhase, RuntimeExecutionState};
    use crate::storage::{read_json, write_json};
    use crate::workflow_model_binding::WorkflowModelBindings;

    fn fixture() -> (tempfile::TempDir, App, String) {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        std::fs::create_dir_all(repo_root.as_std_path()).unwrap();
        let app = test_app_with_named_provider_capabilities(
            repo_root,
            "claude-acp",
            serde_json::json!({}),
        );
        let mut workflow = worker_workflow(None, None);
        let NodeDsl::Worker(worker) = &mut workflow.nodes[0] else {
            panic!("expected worker workflow")
        };
        worker.prompt_envelope = crate::dsl::PromptEnvelopeMode::RawAgent;
        let task_id = app
            .create_task_from_requirement(CreateTaskInput {
                title: Some("Execution plan".to_string()),
                description: None,
                requirement_file_name: None,
                requirement_content: "edit the plan".to_string(),
                workflow,
                workflow_template_id: None,
            })
            .unwrap()
            .task
            .id;
        (temp, app, task_id)
    }

    fn command(view: &ExecutionPlanView, target: ExecutionPlanTarget) -> ExecutionPlanSaveCommand {
        ExecutionPlanSaveCommand {
            project_id: view.project_id.clone(),
            task_id: view.task_id.clone(),
            task_uuid: view.task_uuid.clone(),
            run_id: view.run_id.clone(),
            operation_id: None,
            target,
            expected_plan_revision: view.plan_revision,
            expected_authoring_revision: view.authoring_revision,
            expected_run_status: view.run_status.clone(),
            expected_current_round: view.current_round.clone(),
            expected_current_node: view.current_node.clone(),
            expected_current_attempt: view.current_attempt.clone(),
            workflow: None,
            auto_config: None,
        }
    }

    fn workflow_draft(view: &ExecutionPlanView, goal: &str) -> WorkflowAuthoringDraft {
        let mut workflow = view.next_workflow.clone().unwrap();
        let NodeDsl::Worker(worker) = &mut workflow.nodes[0] else {
            panic!("expected worker workflow")
        };
        worker.goal = Some(goal.to_string());
        WorkflowAuthoringDraft {
            workflow,
            model_bindings: view.next_model_bindings.clone(),
        }
    }

    fn worker_goal(workflow: &crate::dsl::WorkflowDsl) -> Option<String> {
        match &workflow.nodes[0] {
            NodeDsl::Worker(worker) => worker.goal.clone(),
            _ => None,
        }
    }

    fn view(app: &App, task_id: &str, run_id: &str) -> ExecutionPlanView {
        let task_uuid = app.task_show(task_id).unwrap().uuid.unwrap_or_default();
        app.get_conversation_execution_plan(&app.paths.project_id, task_id, &task_uuid, run_id)
            .unwrap()
    }

    fn published_locator(view: &ExecutionPlanView, key: &str) -> Option<String> {
        serde_json::to_value(view)
            .unwrap()
            .get(key)
            .and_then(|value| value.as_str().map(str::to_string))
    }

    fn command_from_published_view(
        view: &ExecutionPlanView,
        target: ExecutionPlanTarget,
    ) -> ExecutionPlanSaveCommand {
        ExecutionPlanSaveCommand {
            project_id: view.project_id.clone(),
            task_id: view.task_id.clone(),
            task_uuid: view.task_uuid.clone(),
            run_id: view.run_id.clone(),
            operation_id: None,
            target,
            expected_plan_revision: view.plan_revision,
            expected_authoring_revision: view.authoring_revision,
            expected_run_status: view.run_status.clone(),
            expected_current_round: published_locator(view, "currentRound"),
            expected_current_node: published_locator(view, "currentNode"),
            expected_current_attempt: published_locator(view, "currentAttempt"),
            workflow: None,
            auto_config: None,
        }
    }

    #[test]
    fn positioned_run_save_uses_locator_published_on_the_view() {
        let _guard = env_guard();
        let (_temp, app, task_id) = fixture();
        let run_id = app
            .prepare_run(&task_id, None)
            .unwrap()
            .accept()
            .run()
            .id
            .clone();
        let initial = view(&app, &task_id, &run_id);
        let run: RunState = read_json(&app.paths.run_file(&task_id, &run_id)).unwrap();
        assert!(
            run.current_round.is_some()
                && run.current_node.is_some()
                && run.current_attempt.is_some(),
            "fixture run must already have a current position"
        );
        assert_eq!(
            published_locator(&initial, "currentRound").as_deref(),
            run.current_round.as_deref()
        );
        assert_eq!(
            published_locator(&initial, "currentNode").as_deref(),
            run.current_node.as_deref()
        );
        assert_eq!(
            published_locator(&initial, "currentAttempt").as_deref(),
            run.current_attempt.as_deref()
        );

        let mut missing = command_from_published_view(&initial, ExecutionPlanTarget::Current);
        missing.expected_current_round = None;
        missing.expected_current_node = None;
        missing.expected_current_attempt = None;
        missing.workflow = Some(workflow_draft(&initial, "missing locator"));
        assert_eq!(
            app.preflight_conversation_execution_plan_save(&missing)
                .unwrap_err()
                .code(),
            error::CURRENT_LOCATOR_CONFLICT
        );

        let mut current_only = command_from_published_view(&initial, ExecutionPlanTarget::Current);
        current_only.workflow = Some(workflow_draft(&initial, "current goal"));
        let saved_current = app.save_conversation_execution_plan(&current_only).unwrap();
        assert!(saved_current.complete);
        assert!(
            saved_current
                .targets
                .iter()
                .any(|target| target.target == ExecutionPlanTarget::Current && target.committed)
        );

        let after_current = view(&app, &task_id, &run_id);
        let mut next_only = command_from_published_view(&after_current, ExecutionPlanTarget::Next);
        next_only.workflow = Some(workflow_draft(&after_current, "next goal"));
        let saved_next = app.save_conversation_execution_plan(&next_only).unwrap();
        assert!(saved_next.complete);
        assert!(
            saved_next
                .targets
                .iter()
                .any(|target| target.target == ExecutionPlanTarget::Next && target.committed)
        );

        let after_next = view(&app, &task_id, &run_id);
        let mut both =
            command_from_published_view(&after_next, ExecutionPlanTarget::CurrentAndNext);
        both.operation_id = Some("op-view-locator".to_string());
        both.workflow = Some(workflow_draft(&after_next, "shared goal"));
        let saved_both = app.save_conversation_execution_plan(&both).unwrap();
        assert!(saved_both.complete);
        assert_eq!(saved_both.targets.len(), 2);
        assert!(saved_both.targets.iter().all(|target| target.committed));
    }

    #[test]
    fn unchanged_current_save_reports_the_plans_still_match() {
        let _guard = env_guard();
        let (_temp, app, task_id) = fixture();
        let run_id = app
            .prepare_run(&task_id, None)
            .unwrap()
            .accept()
            .run()
            .id
            .clone();
        let initial = view(&app, &task_id, &run_id);
        assert!(!initial.diverged);
        let mut command = command(&initial, ExecutionPlanTarget::Current);
        command.workflow = Some(WorkflowAuthoringDraft {
            workflow: initial.next_workflow.clone().unwrap(),
            model_bindings: initial.next_model_bindings.clone(),
        });
        let saved = app.save_conversation_execution_plan(&command).unwrap();
        assert!(saved.complete);
        assert!(!saved.diverged);
        assert!(!view(&app, &task_id, &run_id).diverged);
    }

    #[test]
    fn workflow_run_saves_current_next_and_both_with_independent_revisions() {
        let _guard = env_guard();
        let (_temp, app, task_id) = fixture();
        let run_id = app
            .prepare_run(&task_id, None)
            .unwrap()
            .accept()
            .run()
            .id
            .clone();
        let initial = view(&app, &task_id, &run_id);
        assert_eq!(initial.run_mode, "workflow");
        assert_eq!(initial.plan_revision, 1);
        assert!(initial.current_editable);
        assert!(!initial.diverged);
        assert_eq!(
            initial.current_model_bindings.bindings.len(),
            initial.next_model_bindings.bindings.len()
        );
        let execution_revision_before =
            read_json::<RunState>(&app.paths.run_file(&task_id, &run_id))
                .unwrap()
                .execution
                .revision;

        let mut current_only = command(&initial, ExecutionPlanTarget::Current);
        current_only.workflow = Some(workflow_draft(&initial, "current goal"));
        let preflight = app
            .preflight_conversation_execution_plan_save(&current_only)
            .unwrap();
        assert!(preflight.blocking.is_empty());
        assert_eq!(preflight.affected_node_ids, vec!["dev".to_string()]);
        let saved = app.save_conversation_execution_plan(&current_only).unwrap();
        assert!(saved.complete);
        assert!(saved.diverged);
        assert_eq!(saved.plan_revision, 2);
        assert_eq!(saved.targets.len(), 1);
        assert!(saved.targets[0].committed);
        assert_eq!(
            worker_goal(&app.current_run_workflow(&task_id, &run_id).unwrap()).as_deref(),
            Some("current goal")
        );
        assert_eq!(
            worker_goal(&app.task_authoring_workflow(&task_id).unwrap().workflow).as_deref(),
            Some("do work")
        );
        let run: RunState = read_json(&app.paths.run_file(&task_id, &run_id)).unwrap();
        assert_eq!(run.execution.revision, execution_revision_before);
        assert_eq!(run.current_attempt.as_deref(), Some("attempt-001"));
        let after_current = view(&app, &task_id, &run_id);
        assert!(after_current.diverged);
        assert_eq!(after_current.authoring_revision, initial.authoring_revision);

        let mut stale = command(&initial, ExecutionPlanTarget::Current);
        stale.workflow = Some(workflow_draft(&initial, "stale goal"));
        let stale_preflight = app
            .preflight_conversation_execution_plan_save(&stale)
            .unwrap();
        assert!(
            stale_preflight
                .blocking
                .iter()
                .any(|issue| issue.code() == error::REVISION_CONFLICT)
        );
        assert_eq!(
            app.save_conversation_execution_plan(&stale)
                .unwrap_err()
                .code(),
            error::REVISION_CONFLICT
        );

        let mut next_only = command(&after_current, ExecutionPlanTarget::Next);
        next_only.workflow = Some(workflow_draft(&after_current, "current goal"));
        let saved_next = app.save_conversation_execution_plan(&next_only).unwrap();
        assert!(saved_next.complete);
        assert_eq!(saved_next.plan_revision, 2);
        assert!(saved_next.authoring_revision > after_current.authoring_revision);
        assert_eq!(
            worker_goal(&app.task_authoring_workflow(&task_id).unwrap().workflow).as_deref(),
            Some("current goal")
        );
        let after_next = view(&app, &task_id, &run_id);
        assert!(!after_next.diverged);

        let mut stale_authoring = command(&after_current, ExecutionPlanTarget::Next);
        stale_authoring.workflow = Some(workflow_draft(&after_current, "another goal"));
        assert_eq!(
            app.save_conversation_execution_plan(&stale_authoring)
                .unwrap_err()
                .code(),
            error::AUTHORING_CONFLICT
        );

        let mut both = command(&after_next, ExecutionPlanTarget::CurrentAndNext);
        both.operation_id = Some("op-both-1".to_string());
        both.workflow = Some(workflow_draft(&after_next, "shared goal"));
        let saved_both = app.save_conversation_execution_plan(&both).unwrap();
        assert!(saved_both.complete);
        assert_eq!(saved_both.plan_revision, 3);
        assert_eq!(saved_both.targets.len(), 2);
        assert!(saved_both.targets.iter().all(|target| target.committed));
        let replay = app.save_conversation_execution_plan(&both).unwrap();
        assert_eq!(replay.plan_revision, 3);
        assert_eq!(replay.authoring_revision, saved_both.authoring_revision);
        let recovered = app
            .recover_conversation_execution_plan_operation(
                &after_next.project_id,
                &task_id,
                &after_next.task_uuid,
                &run_id,
                "op-both-1",
            )
            .unwrap();
        assert!(recovered.complete);
        assert_eq!(recovered.plan_revision, 3);
        assert_eq!(view(&app, &task_id, &run_id).plan_revision, 3);

        let without_operation = ExecutionPlanSaveCommand {
            operation_id: None,
            ..both.clone()
        };
        assert_eq!(
            app.save_conversation_execution_plan(&without_operation)
                .unwrap_err()
                .code(),
            error::VALIDATION_FAILED
        );
    }

    #[test]
    fn workflow_run_rejects_locator_drift_and_closed_runs_for_current_target() {
        let _guard = env_guard();
        let (_temp, app, task_id) = fixture();
        let run_id = app
            .prepare_run(&task_id, None)
            .unwrap()
            .accept()
            .run()
            .id
            .clone();
        let initial = view(&app, &task_id, &run_id);

        let mut moved = command(&initial, ExecutionPlanTarget::Current);
        moved.expected_current_attempt = Some("attempt-002".to_string());
        moved.workflow = Some(workflow_draft(&initial, "moved"));
        assert_eq!(
            app.preflight_conversation_execution_plan_save(&moved)
                .unwrap_err()
                .code(),
            error::CURRENT_LOCATOR_CONFLICT
        );

        let mut removed = command(&initial, ExecutionPlanTarget::Current);
        let mut draft = workflow_draft(&initial, "removed");
        let NodeDsl::Worker(worker) = &mut draft.workflow.nodes[0] else {
            panic!("expected worker workflow")
        };
        worker.id = "dev-renamed".to_string();
        draft.workflow.entry = "dev-renamed".to_string();
        for edge in &mut draft.workflow.edges {
            if edge.from == "dev" {
                edge.from = "dev-renamed".to_string();
            }
        }
        removed.workflow = Some(draft);
        assert_eq!(
            app.save_conversation_execution_plan(&removed)
                .unwrap_err()
                .code(),
            error::CURRENT_NODE_REMOVED
        );

        let run_path = app.paths.run_file(&task_id, &run_id);
        let mut run: RunState = read_json(&run_path).unwrap();
        run.status = RunStatus::Completed;
        run.outcome = Some(RunOutcome::Success);
        write_json(&run_path, &run).unwrap();
        let closed = view(&app, &task_id, &run_id);
        assert!(!closed.current_editable);
        let mut closed_current = command(&closed, ExecutionPlanTarget::Current);
        closed_current.workflow = Some(workflow_draft(&closed, "closed"));
        assert_eq!(
            app.save_conversation_execution_plan(&closed_current)
                .unwrap_err()
                .code(),
            error::CURRENT_RUN_NOT_EDITABLE
        );
        let mut closed_next = command(&closed, ExecutionPlanTarget::Next);
        closed_next.workflow = Some(workflow_draft(&closed, "closed next"));
        assert!(
            app.save_conversation_execution_plan(&closed_next)
                .unwrap()
                .complete
        );
    }

    #[test]
    fn current_save_always_reinjects_authoritative_model_binding() {
        let _guard = env_guard();
        let (_temp, app, task_id) = fixture();
        let run_id = app
            .prepare_run(&task_id, None)
            .unwrap()
            .accept()
            .run()
            .id
            .clone();
        let initial = view(&app, &task_id, &run_id);
        let mut draft = workflow_draft(&initial, "binding source");
        let NodeDsl::Worker(worker) = &mut draft.workflow.nodes[0] else {
            panic!("expected worker workflow")
        };
        worker.provider = Some("untrusted-direct-provider".to_string());
        let mut current = command(&initial, ExecutionPlanTarget::Current);
        current.workflow = Some(draft);

        let saved = app.save_conversation_execution_plan(&current).unwrap();
        assert!(saved.complete);
        let projected = app.current_run_workflow(&task_id, &run_id).unwrap();
        let NodeDsl::Worker(worker) = &projected.nodes[0] else {
            panic!("expected worker workflow")
        };
        assert_eq!(worker.provider.as_deref(), Some("claude-acp"));
    }

    #[test]
    fn auto_current_save_rejects_unknown_provider_on_the_backend() {
        let _guard = env_guard();
        let (_temp, app, task_id) = fixture();
        let run_id = "run-auto-validation";
        let mut config = auto_config(2);
        config.agent_type = "unknown-provider".to_string();
        fabricate_auto_run(&app, &task_id, run_id, &config);
        let initial = view(&app, &task_id, run_id);
        let mut current = command(&initial, ExecutionPlanTarget::Current);
        current.auto_config = Some(config);

        assert_eq!(
            app.save_conversation_execution_plan(&current)
                .unwrap_err()
                .code(),
            error::VALIDATION_FAILED
        );
    }

    #[test]
    fn ordinary_authoring_save_respects_the_authoring_lock() {
        let _guard = env_guard();
        let (_temp, app, task_id) = fixture();
        let key = crate::execution_plan::authoring_lock_key(&app.paths.project_id, &task_id);
        let _write = crate::execution_plan::try_acquire_plan_write(&key).unwrap();
        let authoring = app.task_authoring_workflow(&task_id).unwrap();
        let error = app
            .save_task_workflow_with_bindings(
                &task_id,
                authoring.workflow,
                WorkflowModelBindings::default(),
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("conversation.execution-plan.revision-conflict")
        );
    }

    fn auto_config(fanout: u32) -> ConversationAutoConfig {
        ConversationAutoConfig {
            agent_strategy: Some("fixed".to_string()),
            agent_type: "claude-acp".to_string(),
            bootstrap_agent_type: None,
            bootstrap_model_id: None,
            bootstrap_config_options: Default::default(),
            bootstrap_model_bound_overrides: Default::default(),
            acceptance_model_id: None,
            acceptance_config_options: Default::default(),
            acceptance_model_bound_overrides: Default::default(),
            model_id: None,
            permission_mode: None,
            auto_accept: false,
            config_options: Default::default(),
            model_bound_overrides: Default::default(),
            available_agents: None,
            routing_prompt: Some("route".to_string()),
            allowed_workflows: None,
            allowed_profiles: None,
            global_goal: None,
            control: Some(crate::config::ConversationDynamicControl {
                max_dynamic_nodes: 20,
                max_fanout: fanout,
                max_depth: 6,
                max_parallel: 3,
                max_group_depth: 1,
                max_workflow_invocations: 10,
                allow_nested_dynamic: false,
            }),
            active_template_id: None,
            active_template_name: None,
        }
    }

    fn fabricate_auto_run(app: &App, task_id: &str, run_id: &str, config: &ConversationAutoConfig) {
        let task_uuid = app.task_show(task_id).unwrap().uuid;
        let run = RunState {
            version: "0.1".to_string(),
            id: run_id.to_string(),
            task_id: task_id.to_string(),
            task_uuid,
            status: RunStatus::Running,
            outcome: None,
            started_at: "t".to_string(),
            updated_at: "t".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some("round-001".to_string()),
            current_node: Some("ai-dynamic".to_string()),
            current_attempt: Some("attempt-001".to_string()),
            new_rounds_opened: 0,
            pause_reason: None,
            uuid: Some("run-uuid".to_string()),
            last_executed_node: None,
            worktree: None,
            execution: RuntimeExecutionState::new(RuntimeExecutionPhase::StartingNode, None, "t"),
        };
        write_json(&app.paths.run_file(task_id, run_id), &run).unwrap();
        publish_initial_auto(&app.paths, task_id, run_id, config.clone()).unwrap();
        let project_id = app.paths.project_id.clone();
        let stored = config.clone();
        app.with_state(move |state| {
            state.conversation_run_modes.insert(
                project_id,
                ConversationRunModeEntry {
                    mode: ConversationRunMode::Auto,
                    workflow_template_id: None,
                    optional_entry_preferences: Default::default(),
                    direct_config: None,
                    direct_preferences: Default::default(),
                    auto_config: Some(stored),
                    authoring_revision: 0,
                },
            );
            (true, ())
        })
        .unwrap();
    }

    #[test]
    fn auto_run_saves_current_plan_and_project_authoring_separately() {
        let _guard = env_guard();
        let (_temp, app, task_id) = fixture();
        let run_id = "run-auto";
        fabricate_auto_run(&app, &task_id, run_id, &auto_config(2));
        let initial = view(&app, &task_id, run_id);
        assert_eq!(initial.run_mode, "auto");
        assert_eq!(initial.plan_revision, 1);
        assert_eq!(initial.authoring_revision, 1);
        assert!(!initial.diverged);
        assert_eq!(
            initial
                .current_auto_config
                .as_ref()
                .and_then(|config| config.control.as_ref())
                .map(|control| control.max_fanout),
            Some(2)
        );

        let mut current_only = command(&initial, ExecutionPlanTarget::Current);
        current_only.expected_current_node = Some("ai-dynamic".to_string());
        current_only.auto_config = Some(auto_config(9));
        let saved = app.save_conversation_execution_plan(&current_only).unwrap();
        assert_eq!(saved.plan_revision, 2);
        let dispatched = ai_dynamic_node_for_dispatch(&app.paths, &task_id, run_id, "ai-dynamic")
            .unwrap()
            .unwrap();
        assert_eq!(dispatched.control.max_fanout, 9);
        let state = app.load_state().unwrap();
        let entry = state
            .conversation_run_modes
            .get(&app.paths.project_id)
            .unwrap();
        assert_eq!(
            entry
                .auto_config
                .as_ref()
                .unwrap()
                .control
                .as_ref()
                .unwrap()
                .max_fanout,
            2
        );
        assert_eq!(entry.authoring_revision, 0);
        let after_current = view(&app, &task_id, run_id);
        assert!(after_current.diverged);

        let mut next_only = command(&after_current, ExecutionPlanTarget::Next);
        next_only.expected_current_node = Some("ai-dynamic".to_string());
        next_only.auto_config = Some(auto_config(9));
        let saved_next = app.save_conversation_execution_plan(&next_only).unwrap();
        assert_eq!(saved_next.plan_revision, 2);
        assert_eq!(saved_next.authoring_revision, 2);
        let state = app.load_state().unwrap();
        let entry = state
            .conversation_run_modes
            .get(&app.paths.project_id)
            .unwrap();
        assert_eq!(
            entry
                .auto_config
                .as_ref()
                .unwrap()
                .control
                .as_ref()
                .unwrap()
                .max_fanout,
            2
        );
        let task_auto: ConversationAutoConfig =
            read_json(&app.paths.task_auto_config_file(&task_id)).unwrap();
        assert_eq!(task_auto.control.as_ref().unwrap().max_fanout, 9);
        assert_eq!(
            serde_json::to_value(app.task_authoring_workflow(&task_id).unwrap().workflow).unwrap(),
            serde_json::to_value(compile_auto_workflow(Some(&auto_config(9)))).unwrap()
        );
        assert!(!view(&app, &task_id, run_id).diverged);

        let mut stale_next = command(&after_current, ExecutionPlanTarget::Next);
        stale_next.expected_current_node = Some("ai-dynamic".to_string());
        stale_next.auto_config = Some(auto_config(4));
        assert_eq!(
            app.save_conversation_execution_plan(&stale_next)
                .unwrap_err()
                .code(),
            error::AUTHORING_CONFLICT
        );
    }

    #[test]
    fn auto_conversation_keeps_the_auto_form_when_the_plan_was_published_as_workflow() {
        let _guard = env_guard();
        let (_temp, app, task_id) = fixture();
        let run_id = "run-002";
        let task_uuid = app.task_show(&task_id).unwrap().uuid;
        let run = RunState {
            version: "0.1".to_string(),
            id: run_id.to_string(),
            task_id: task_id.clone(),
            task_uuid,
            status: RunStatus::Running,
            outcome: None,
            started_at: "t".to_string(),
            updated_at: "t".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some("round-001".to_string()),
            current_node: Some("ai-dynamic".to_string()),
            current_attempt: Some("attempt-001".to_string()),
            new_rounds_opened: 0,
            pause_reason: None,
            uuid: Some("run-uuid".to_string()),
            last_executed_node: None,
            worktree: None,
            execution: RuntimeExecutionState::new(RuntimeExecutionPhase::StartingNode, None, "t"),
        };
        write_json(&app.paths.run_file(&task_id, run_id), &run).unwrap();
        publish_initial_workflow(
            &app.paths,
            &task_id,
            run_id,
            worker_workflow(None, None),
            WorkflowModelBindings::default(),
        )
        .unwrap();
        let authoring = app.paths.task_dir(&task_id).join("authoring");
        std::fs::create_dir_all(authoring.as_std_path()).unwrap();
        write_json(
            &authoring.join("conversation.json"),
            &serde_json::json!({ "runMode": "auto" }),
        )
        .unwrap();
        write_json(&app.paths.task_auto_config_file(&task_id), &auto_config(9)).unwrap();

        let seen = view(&app, &task_id, run_id);
        assert_eq!(seen.run_mode, "auto");
        assert_eq!(
            seen.next_auto_config
                .as_ref()
                .unwrap()
                .control
                .as_ref()
                .unwrap()
                .max_fanout,
            9
        );
        assert!(seen.current_auto_config.is_some());
        assert!(!seen.diverged);
    }
}
