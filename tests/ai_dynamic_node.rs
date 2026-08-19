use camino::Utf8PathBuf;
use gold_band::app::App;
use gold_band::config::ProviderDiagnosticSnapshot;
use gold_band::domain::{PauseReason, RunOutcome, RunStatus, SessionMode};
use gold_band::dsl::WorkflowValidationError;
use gold_band::dynamic::{
    DynamicCompletionSchemaPolicy, DynamicGraphState, DynamicGroupStatus, DynamicNodeKind,
    DynamicNodeStatus, DynamicProposalValidationStatus, DynamicRunStatus, WorkspaceStatus,
    dynamic_completion_effective_schema,
};
use gold_band::provider::{
    AcpContentBlock, AcpLiveUpdate, AcpPromptAccepted, AcpSessionUpdate, DoctorResult,
    OutputArtifactPayload, OutputEmissionMode, PromptVisibility, ProviderAdapter,
    ProviderCapabilities, ProviderInfo, ProviderResultPayload, ProviderRunResult,
    ProviderRunStatus, SessionRef, UserPromptRenderMode, WorkerInvocation, render_prompt_bundle,
};
use gold_band::runtime_error::{
    DEFAULT_AUTO_RETRY_MAX_ATTEMPTS, RuntimeErrorDomain, auto_runtime_error_info,
};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

#[derive(Clone)]
enum DynamicScenario {
    Fanout,
    WorktreeFanout,
    NestedFanout,
    InvalidWorkflowInvocation,
    SingleWorktreeRepair,
    FanoutRepair,
    MultiValidationRepair,
    MergeAcceptanceProfileRepair,
    ParseRepair,
    MissingArtifactRepair,
    SessionContinuePrompt,
    InvalidSessionContinue,
    ProviderRuntimeError,
    MergePauseThenContinue,
    WorkflowInvocation { workflow_id: Arc<Mutex<String>> },
    WorkflowInvocationPauseThenContinue { workflow_id: Arc<Mutex<String>> },
}

#[derive(Clone)]
struct DynamicProvider {
    scenario: DynamicScenario,
    invocations: Arc<Mutex<Vec<WorkerInvocation>>>,
}

impl DynamicProvider {
    fn new(scenario: DynamicScenario) -> Self {
        Self {
            scenario,
            invocations: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn fanout() -> Self {
        Self::new(DynamicScenario::Fanout)
    }

    fn worktree_fanout() -> Self {
        Self::new(DynamicScenario::WorktreeFanout)
    }

    fn nested_fanout() -> Self {
        Self::new(DynamicScenario::NestedFanout)
    }

    fn invalid_workflow_invocation() -> Self {
        Self::new(DynamicScenario::InvalidWorkflowInvocation)
    }

    fn single_worktree_repair() -> Self {
        Self::new(DynamicScenario::SingleWorktreeRepair)
    }

    fn fanout_repair() -> Self {
        Self::new(DynamicScenario::FanoutRepair)
    }

    fn multi_validation_repair() -> Self {
        Self::new(DynamicScenario::MultiValidationRepair)
    }

    fn merge_acceptance_profile_repair() -> Self {
        Self::new(DynamicScenario::MergeAcceptanceProfileRepair)
    }

    fn parse_repair() -> Self {
        Self::new(DynamicScenario::ParseRepair)
    }

    fn missing_artifact_repair() -> Self {
        Self::new(DynamicScenario::MissingArtifactRepair)
    }

    fn session_continue_prompt() -> Self {
        Self::new(DynamicScenario::SessionContinuePrompt)
    }

    fn invalid_session_continue() -> Self {
        Self::new(DynamicScenario::InvalidSessionContinue)
    }

    fn provider_runtime_error() -> Self {
        Self::new(DynamicScenario::ProviderRuntimeError)
    }

    fn merge_pause_then_continue() -> Self {
        Self::new(DynamicScenario::MergePauseThenContinue)
    }

    fn workflow_invocation(workflow_id: Arc<Mutex<String>>) -> Self {
        Self::new(DynamicScenario::WorkflowInvocation { workflow_id })
    }

    fn workflow_invocation_pause_then_continue(workflow_id: Arc<Mutex<String>>) -> Self {
        Self::new(DynamicScenario::WorkflowInvocationPauseThenContinue { workflow_id })
    }
}

impl ProviderAdapter for DynamicProvider {
    fn describe_provider(&self) -> ProviderInfo {
        ProviderInfo {
            provider_id: "fake".to_string(),
            display_name: "Fake".to_string(),
            capabilities: ProviderCapabilities {
                supports_open_session: true,
                supports_continue_session: true,
                supports_system_prompt: true,
                supports_raw_stream: false,
            },
            is_default: false,
        }
    }

    fn doctor(&self) -> DoctorResult {
        DoctorResult {
            available: true,
            reason: None,
            capabilities: None,
        }
    }

    fn run_worker(&self, req: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        self.run_worker_once(req)
    }

    fn run_worker_with_callbacks(
        &self,
        req: WorkerInvocation,
        _live_update: Option<AcpLiveUpdate<'_>>,
        _session_update: Option<AcpSessionUpdate<'_>>,
        prompt_accepted: Option<AcpPromptAccepted<'_>>,
    ) -> anyhow::Result<ProviderRunResult> {
        if let Some(callback) = prompt_accepted {
            callback(req.resume_prompt_id.as_deref().unwrap_or("test-prompt"))?;
        }
        if req.output_contract.as_ref().is_some_and(|contract| {
            contract.emission_mode == OutputEmissionMode::PostTurnProjection
        }) {
            let resumed_control_turn = matches!(
                req.user_prompt_render_mode,
                UserPromptRenderMode::RuntimeFinalize | UserPromptRenderMode::RuntimeRepair
            );
            if !resumed_control_turn {
                let work_result = self.run_worker_once(req.clone())?;
                if work_result.runtime_error.is_some()
                    || work_result.status != ProviderRunStatus::Success
                {
                    return Ok(work_result);
                }
            }

            let mut finalize_req = req;
            finalize_req
                .output_contract
                .as_mut()
                .expect("post-turn projection requires output contract")
                .emission_mode = OutputEmissionMode::InlineControl;
            finalize_req.session_mode = SessionMode::Continue;
            finalize_req.resume_prompt_visibility = PromptVisibility::Hidden;
            finalize_req.task_input_attachment_paths.clear();
            finalize_req.user_input_attachment_paths.clear();
            if !resumed_control_turn {
                finalize_req.resume_prompt = Some("finalize artifact".to_string());
                finalize_req.resume_prompt_id = Some(format!(
                    "artifact-finalize-{}",
                    finalize_req.runtime_context.attempt_id
                ));
                finalize_req.user_prompt_render_mode = UserPromptRenderMode::RuntimeFinalize;
            }
            return self.run_worker_once(finalize_req);
        }

        self.run_worker_once(req)
    }

    fn open_session(&self, _worker_ref: &gold_band::domain::SessionRef) -> anyhow::Result<()> {
        Ok(())
    }

    fn build_continue_command(
        &self,
        worker_ref: &gold_band::domain::SessionRef,
    ) -> anyhow::Result<Option<String>> {
        Ok(worker_ref.open_command.clone())
    }
}

fn with_available_claude_diagnostics(app: App) -> App {
    app.with_provider_diagnostics_source(Arc::new(|| {
        Ok(std::collections::BTreeMap::from([(
            "claude-acp".to_string(),
            ProviderDiagnosticSnapshot {
                available: true,
                reason: None,
                checked_at: "2026-08-17T00:00:00Z".to_string(),
                capabilities: None,
            },
        )]))
    }))
}

impl DynamicProvider {
    fn run_worker_once(&self, req: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        self.invocations.lock().unwrap().push(req.clone());
        if matches!(self.scenario, DynamicScenario::ProviderRuntimeError) {
            return Ok(ProviderRunResult {
                status: ProviderRunStatus::Failure,
                exit_code: None,
                result_payload: None,
                worker_ref_seed: None,
                stream_path: None,
                runtime_error: Some(auto_runtime_error_info(
                    RuntimeErrorDomain::Provider,
                    "provider.server-unavailable",
                    "provider is temporarily unavailable",
                    json!({}),
                )),
            });
        }
        let (status, output_artifact) = match (
            &self.scenario,
            req.runtime_context.run_id.as_str(),
            req.runtime_context.node_id.as_str(),
            req.session_mode,
        ) {
            (
                DynamicScenario::WorkflowInvocationPauseThenContinue { .. },
                "run-002",
                "child",
                SessionMode::New,
            ) => (ProviderRunStatus::Interrupted, None),
            (DynamicScenario::MergePauseThenContinue, _, "group-core-merge", SessionMode::New) => {
                (ProviderRunStatus::Interrupted, None)
            }
            _ => {
                let output_artifact = match self.dynamic_artifact_for(&req) {
                    Some(content) => Some(OutputArtifactPayload {
                        name: req
                            .output_contract
                            .as_ref()
                            .map(|contract| contract.artifact.clone())
                            .unwrap_or_else(|| "dynamic-node-completion".to_string()),
                        content,
                    }),
                    None => None,
                };
                (ProviderRunStatus::Success, output_artifact)
            }
        };

        Ok(ProviderRunResult {
            status,
            exit_code: Some(0),
            result_payload: Some(ProviderResultPayload { output_artifact }),
            worker_ref_seed: Some(SessionRef {
                provider: "claude-acp".to_string(),
                mode: req.session_mode,
                supports_open_session: true,
                supports_continue_session: true,
                continue_ref: Some(serde_json::json!({
                    "sessionId": format!("{}-{}", req.runtime_context.node_id, req.runtime_context.attempt_id)
                })),
                open_command: Some(format!(
                    "claude -c {}-{}",
                    req.runtime_context.node_id, req.runtime_context.attempt_id
                )),
            }),
            stream_path: None,
            runtime_error: None,
        })
    }

    fn dynamic_artifact_for(&self, req: &WorkerInvocation) -> Option<String> {
        if req.output_contract.is_none() {
            return None;
        }
        let is_runtime_repair = req.user_prompt_render_mode == UserPromptRenderMode::RuntimeRepair;
        let profile = req.profile.as_deref().unwrap_or("profile");
        match (&self.scenario, req.runtime_context.node_id.as_str()) {
            (DynamicScenario::Fanout, "bootstrap") => Some(fanout_completion(profile)),
            (DynamicScenario::Fanout, "branch-a" | "branch-b") => {
                Some(end_completion("branch done"))
            }
            (DynamicScenario::WorktreeFanout, "bootstrap") => {
                Some(worktree_fanout_completion(profile))
            }
            (DynamicScenario::WorktreeFanout, "branch-a" | "branch-b") => {
                std::fs::write(
                    req.workspace_dir
                        .join(format!("{}.txt", req.runtime_context.node_id)),
                    format!("{} done", req.runtime_context.node_id),
                )
                .unwrap();
                Some(end_completion("branch done"))
            }
            (DynamicScenario::NestedFanout, "bootstrap") => Some(fanout_completion(profile)),
            (DynamicScenario::NestedFanout, "branch-a") => Some(nested_fanout_completion(profile)),
            (DynamicScenario::NestedFanout, "branch-b" | "branch-a-1" | "branch-a-2") => {
                Some(end_completion("branch done"))
            }
            (DynamicScenario::InvalidWorkflowInvocation, "bootstrap") => {
                Some(invalid_workflow_invocation_completion(profile))
            }
            (DynamicScenario::SingleWorktreeRepair, "bootstrap") => {
                if is_runtime_repair {
                    Some(fanout_completion(profile))
                } else {
                    Some(single_worktree_completion())
                }
            }
            (DynamicScenario::SingleWorktreeRepair, "branch-a" | "branch-b") => {
                Some(end_completion("branch done"))
            }
            (DynamicScenario::FanoutRepair, "bootstrap") => {
                if is_runtime_repair {
                    Some(fanout_completion(profile))
                } else {
                    Some(too_many_fanout_branches_completion(profile))
                }
            }
            (DynamicScenario::FanoutRepair, "branch-a" | "branch-b") => {
                Some(end_completion("branch done"))
            }
            (DynamicScenario::MultiValidationRepair, "bootstrap") => {
                if is_runtime_repair {
                    Some(fanout_completion(profile))
                } else {
                    Some(invalid_profile_and_overflow_completion())
                }
            }
            (DynamicScenario::MergeAcceptanceProfileRepair, "bootstrap") => {
                if is_runtime_repair {
                    Some(fanout_completion(profile))
                } else {
                    Some(merge_acceptance_profile_completion())
                }
            }
            (DynamicScenario::ParseRepair, "bootstrap") => {
                if is_runtime_repair {
                    Some(fanout_completion(profile))
                } else {
                    Some(missing_merge_task_completion())
                }
            }
            (DynamicScenario::MissingArtifactRepair, "bootstrap") => {
                if is_runtime_repair {
                    Some(fanout_completion(profile))
                } else {
                    Some(String::new())
                }
            }
            (DynamicScenario::MissingArtifactRepair, "branch-a" | "branch-b") => {
                Some(end_completion("branch done"))
            }
            (DynamicScenario::MultiValidationRepair, "branch-a" | "branch-b") => {
                Some(end_completion("branch done"))
            }
            (DynamicScenario::MergeAcceptanceProfileRepair, "branch-a" | "branch-b") => {
                Some(end_completion("branch done"))
            }
            (DynamicScenario::ParseRepair, "branch-a" | "branch-b") => {
                Some(end_completion("branch done"))
            }
            (DynamicScenario::SessionContinuePrompt, "bootstrap") => {
                Some(session_continue_fanout_completion())
            }
            (DynamicScenario::SessionContinuePrompt, "branch-a") => {
                Some(end_completion("branch A done"))
            }
            (DynamicScenario::SessionContinuePrompt, "branch-b") => {
                Some(session_continue_single_completion())
            }
            (DynamicScenario::SessionContinuePrompt, "branch-c") => {
                Some(end_completion("branch C done"))
            }
            (DynamicScenario::InvalidSessionContinue, "bootstrap") => {
                Some(invalid_session_continue_completion())
            }
            (DynamicScenario::MergePauseThenContinue, "bootstrap") => {
                Some(fanout_completion(profile))
            }
            (DynamicScenario::MergePauseThenContinue, "branch-a" | "branch-b") => {
                Some(end_completion("branch done"))
            }
            (DynamicScenario::WorkflowInvocation { workflow_id }, "bootstrap")
            | (DynamicScenario::WorkflowInvocationPauseThenContinue { workflow_id }, "bootstrap") =>
            {
                let workflow_id = workflow_id.lock().unwrap().clone();
                Some(workflow_invocation_completion(&workflow_id))
            }
            (_, node_id) if node_id.ends_with("-accept") => Some(end_completion("accepted")),
            _ => None,
        }
    }
}

fn fanout_completion(_profile: &str) -> String {
    r#"{
            "version": "0.1",
            "kind": "dynamic-node-completion",
            "status": "success",
            "summary": "split into two branches",
            "next": {
                "type": "fanout",
                "groupId": "group-core",
                "nodes": [
                    {
                        "id": "branch-a",
                        "kind": "worker",
                        "title": "Branch A",
                        "task": "Finish branch A",
                        "profile": "pf-builtin-dev",
                        "dependsOn": ["bootstrap"]
                    },
                    {
                        "id": "branch-b",
                        "kind": "worker",
                        "title": "Branch B",
                        "task": "Finish branch B",
                        "profile": "pf-builtin-dev",
                        "dependsOn": ["bootstrap"]
                    }
                ],
                "merge": {
                    "title": "Merge core",
                    "task": "Merge branch outputs"
                },
                "acceptance": {
                    "title": "Accept core",
                    "task": "Accept merged branch outputs"
                }
            }
        }"#
    .to_string()
}

fn worktree_fanout_completion(_profile: &str) -> String {
    r#"{
            "version": "0.1",
            "kind": "dynamic-node-completion",
            "status": "success",
            "summary": "split into two writable branches",
            "next": {
                "type": "fanout",
                "groupId": "group-core",
                "nodes": [
                    {
                        "id": "branch-a",
                        "kind": "worker",
                        "title": "Branch A",
                        "task": "Write branch A",
                        "profile": "pf-builtin-dev",
                        "dependsOn": ["bootstrap"]
                    },
                    {
                        "id": "branch-b",
                        "kind": "worker",
                        "title": "Branch B",
                        "task": "Write branch B",
                        "profile": "pf-builtin-dev",
                        "dependsOn": ["bootstrap"]
                    }
                ],
                "merge": {
                    "title": "Merge writable branches",
                    "task": "Merge branch worktrees"
                },
                "acceptance": {
                    "title": "Accept writable branches",
                    "task": "Accept merged branch worktrees"
                }
            }
        }"#
    .to_string()
}

fn nested_fanout_completion(_profile: &str) -> String {
    r#"{
            "version": "0.1",
            "kind": "dynamic-node-completion",
            "status": "success",
            "summary": "split branch A into deeper work",
            "next": {
                "type": "fanout",
                "groupId": "group-branch-a",
                "nodes": [
                    {
                        "id": "branch-a-1",
                        "kind": "worker",
                        "title": "Branch A 1",
                        "task": "Finish branch A part 1",
                        "profile": "pf-builtin-dev",
                        "dependsOn": ["branch-a"]
                    },
                    {
                        "id": "branch-a-2",
                        "kind": "worker",
                        "title": "Branch A 2",
                        "task": "Finish branch A part 2",
                        "profile": "pf-builtin-dev",
                        "dependsOn": ["branch-a"]
                    }
                ],
                "merge": {
                    "title": "Merge branch A",
                    "task": "Merge branch A outputs"
                },
                "acceptance": {
                    "title": "Accept branch A",
                    "task": "Accept branch A outputs"
                }
            }
        }"#
    .to_string()
}

fn end_completion(summary: &str) -> String {
    format!(
        r#"{{
            "version": "0.1",
            "kind": "dynamic-node-completion",
            "status": "success",
            "summary": "{summary}",
            "next": {{ "type": "end" }}
        }}"#
    )
}

fn invalid_workflow_invocation_completion(_profile: &str) -> String {
    r#"{
            "version": "0.1",
            "kind": "dynamic-node-completion",
            "status": "success",
            "summary": "try unallowed workflow",
            "next": {
                "type": "single",
                "node": {
                    "id": "invoke-missing",
                    "kind": "workflow-invocation",
                    "title": "Invoke missing workflow",
                    "task": "Run a workflow that is not allowed",
                    "dependsOn": ["bootstrap"],
                    "workflowId": "missing-workflow"
                }
            }
        }"#
    .to_string()
}

fn too_many_fanout_branches_completion(_profile: &str) -> String {
    r#"{
            "version": "0.1",
            "kind": "dynamic-node-completion",
            "status": "success",
            "summary": "split into too many branches",
            "next": {
                "type": "fanout",
                "groupId": "group-overflow",
                "nodes": [
                    {
                        "id": "branch-a",
                        "kind": "worker",
                        "title": "Branch A",
                        "task": "Finish branch A",
                        "profile": "pf-builtin-dev",
                        "dependsOn": ["bootstrap"]
                    },
                    {
                        "id": "branch-b",
                        "kind": "worker",
                        "title": "Branch B",
                        "task": "Finish branch B",
                        "profile": "pf-builtin-dev",
                        "dependsOn": ["bootstrap"]
                    },
                    {
                        "id": "branch-c",
                        "kind": "worker",
                        "title": "Branch C",
                        "task": "Finish branch C",
                        "profile": "pf-builtin-dev",
                        "dependsOn": ["bootstrap"]
                    }
                ],
                "merge": {
                    "title": "Merge overflow",
                    "task": "Merge branch outputs"
                },
                "acceptance": {
                    "title": "Accept overflow",
                    "task": "Accept merged branch outputs"
                }
            }
        }"#
    .to_string()
}

fn invalid_profile_and_overflow_completion() -> String {
    r#"{
            "version": "0.1",
            "kind": "dynamic-node-completion",
            "status": "success",
            "summary": "invalid split",
            "next": {
                "type": "fanout",
                "groupId": "group-overflow",
                "nodes": [
                    {
                        "id": "branch-a",
                        "kind": "worker",
                        "title": "Branch A",
                        "task": "Finish branch A",
                        "profile": "missing-profile",
                        "dependsOn": ["bootstrap"]
                    },
                    {
                        "id": "branch-b",
                        "kind": "worker",
                        "title": "Branch B",
                        "task": "Finish branch B",
                        "profile": "missing-profile",
                        "dependsOn": ["bootstrap"]
                    },
                    {
                        "id": "branch-c",
                        "kind": "worker",
                        "title": "Branch C",
                        "task": "Finish branch C",
                        "profile": "missing-profile",
                        "dependsOn": ["bootstrap"]
                    }
                ],
                "merge": {
                    "title": "Merge overflow",
                    "task": "Merge branch outputs"
                },
                "acceptance": {
                    "title": "Accept overflow",
                    "task": "Accept merged branch outputs"
                }
            }
        }"#
    .to_string()
}

fn merge_acceptance_profile_completion() -> String {
    r#"{
            "version": "0.1",
            "kind": "dynamic-node-completion",
            "status": "success",
            "summary": "split into two branches with unsupported group profiles",
            "next": {
                "type": "fanout",
                "groupId": "group-core",
                "nodes": [
                    {
                        "id": "branch-a",
                        "kind": "worker",
                        "title": "Branch A",
                        "task": "Finish branch A",
                        "profile": "pf-builtin-dev",
                        "dependsOn": ["bootstrap"]
                    },
                    {
                        "id": "branch-b",
                        "kind": "worker",
                        "title": "Branch B",
                        "task": "Finish branch B",
                        "profile": "pf-builtin-dev",
                        "dependsOn": ["bootstrap"]
                    }
                ],
                "merge": {
                    "title": "Merge core",
                    "profile": "pf-builtin-review",
                    "task": "Merge branch outputs"
                },
                "acceptance": {
                    "title": "Accept core",
                    "profile": "pf-builtin-accept",
                    "task": "Accept merged branch outputs"
                }
            }
        }"#
    .to_string()
}

fn missing_merge_task_completion() -> String {
    r#"{
            "version": "0.1",
            "kind": "dynamic-node-completion",
            "status": "success",
            "summary": "split into two branches with malformed merge spec",
            "next": {
                "type": "fanout",
                "groupId": "group-core",
                "nodes": [
                    {
                        "id": "branch-a",
                        "kind": "worker",
                        "title": "Branch A",
                        "task": "Finish branch A",
                        "profile": "pf-builtin-dev",
                        "dependsOn": ["bootstrap"]
                    },
                    {
                        "id": "branch-b",
                        "kind": "worker",
                        "title": "Branch B",
                        "task": "Finish branch B",
                        "profile": "pf-builtin-dev",
                        "dependsOn": ["bootstrap"]
                    }
                ],
                "merge": {
                    "title": "Merge core"
                },
                "acceptance": {
                    "title": "Accept core",
                    "task": "Accept merged branch outputs"
                }
            }
        }"#
    .to_string()
}

fn session_continue_fanout_completion() -> String {
    r#"{
            "version": "0.1",
            "kind": "dynamic-node-completion",
            "status": "success",
            "summary": "split into branches and leave one follow-up",
            "next": {
                "type": "fanout",
                "groupId": "group-core",
                "nodes": [
                    {
                        "id": "branch-a",
                        "kind": "worker",
                        "title": "Branch A",
                        "task": "Finish branch A",
                        "profile": "pf-builtin-dev",
                        "dependsOn": ["bootstrap"]
                    },
                    {
                        "id": "branch-b",
                        "kind": "worker",
                        "title": "Branch B",
                        "task": "Finish branch B then continue same chat for final wrap-up",
                        "profile": "pf-builtin-dev",
                        "dependsOn": ["bootstrap"]
                    }
                ],
                "merge": {
                    "title": "Merge core",
                    "task": "Merge branch outputs"
                },
                "acceptance": {
                    "title": "Accept core",
                    "task": "Accept merged branch outputs"
                }
            }
        }"#
    .to_string()
}

fn session_continue_single_completion() -> String {
    r#"{
            "version": "0.1",
            "kind": "dynamic-node-completion",
            "status": "success",
            "summary": "continue branch B conversation into final wrap-up node",
            "next": {
                "type": "single",
                "node": {
                    "id": "branch-c",
                    "kind": "worker",
                    "title": "Branch C",
                    "task": "Continue branch B conversation and wrap up remaining branch work",
                    "profile": "pf-builtin-dev",
                    "sessionMode": "continue",
                    "continueFromNodeId": "branch-b",
                    "dependsOn": ["branch-b"]
                }
            }
        }"#
    .to_string()
}

fn single_worktree_completion() -> String {
    r#"{
            "version": "0.1",
            "kind": "dynamic-node-completion",
            "status": "success",
            "summary": "create one writable node",
            "next": {
                "type": "single",
                "node": {
                    "id": "single-write",
                    "kind": "worker",
                    "title": "Single Write",
                    "task": "Write one change in an isolated worktree",
                    "profile": "pf-builtin-dev",
                    "workspace": { "mode": "worktree" },
                    "dependsOn": ["bootstrap"]
                }
            }
        }"#
    .to_string()
}

fn invalid_session_continue_completion() -> String {
    r#"{
            "version": "0.1",
            "kind": "dynamic-node-completion",
            "status": "success",
            "summary": "try invalid continue target",
            "next": {
                "type": "single",
                "node": {
                    "id": "child-flow-node",
                    "kind": "workflow-invocation",
                    "title": "Run child flow with invalid continue",
                    "task": "Try to continue a workflow invocation session",
                    "sessionMode": "continue",
                    "continueFromNodeId": "bootstrap",
                    "dependsOn": ["bootstrap"],
                    "workflowId": "missing-workflow"
                }
            }
        }"#
    .to_string()
}

fn workflow_invocation_completion(workflow_id: &str) -> String {
    format!(
        r#"{{
            "version": "0.1",
            "kind": "dynamic-node-completion",
            "status": "success",
            "summary": "invoke allowed workflow",
            "next": {{
                "type": "single",
                "node": {{
                    "id": "child-flow-node",
                    "kind": "workflow-invocation",
                    "title": "Run child flow",
                    "task": "Run child workflow from frozen snapshot",
                    "dependsOn": ["bootstrap"],
                    "workflowId": "{workflow_id}"
                }}
            }}
        }}"#
    )
}

fn first_profile_id(app: &App) -> String {
    app.profiles().unwrap().profiles[0].id.clone()
}

fn write_task_file(app: &App, task_id: &str) {
    if !app.paths.repo_root.join(".git").exists() {
        init_git_repo(&app.paths.repo_root);
    }
    write_task_file_without_git(app, task_id);
}

fn write_task_file_without_git(app: &App, task_id: &str) {
    std::fs::create_dir_all(app.paths.task_dir(task_id).join("authoring").as_std_path()).unwrap();
    std::fs::write(
        app.paths.requirement_file(task_id).as_std_path(),
        "Exercise AI-DYNAMIC",
    )
    .unwrap();
    std::fs::write(
        app.paths.task_file(task_id).as_std_path(),
        format!(r#"{{"version":"0.1","id":"{task_id}"}}"#),
    )
    .unwrap();
}

fn write_task_input_image(app: &App, task_id: &str, name: &str) -> Utf8PathBuf {
    let inputs_dir = app.paths.task_dir(task_id).join("authoring").join("inputs");
    std::fs::create_dir_all(inputs_dir.as_std_path()).unwrap();
    let path = inputs_dir.join(name);
    std::fs::write(path.as_std_path(), b"\x89PNG\r\n\x1a\nimage").unwrap();
    path
}

fn write_dynamic_workflow(app: &App, task_id: &str, _profile: &str, allowed_workflows: &str) {
    write_dynamic_workflow_with_agent_strategy(
        app,
        task_id,
        r#"{
                            "mode": "fixed",
                            "provider": "claude-acp",
                            "model": "test-model"
                        }"#,
        allowed_workflows,
    );
}

fn write_dynamic_workflow_with_agent_strategy(
    app: &App,
    task_id: &str,
    agent_strategy: &str,
    allowed_workflows: &str,
) {
    std::fs::write(
        app.paths.workflow_file(task_id).as_std_path(),
        format!(
            r#"{{
                "version": "0.1",
                "id": "dynamic-flow",
                "entry": "router",
                "control": {{ "max_attempts": 1, "max_rounds": 1 }},
                "nodes": [
                    {{
                        "id": "router",
                        "type": "ai-dynamic",
                        "agentStrategy": {agent_strategy},
                        "control": {{
                            "maxDynamicNodes": 10,
                            "maxFanout": 2,
                            "maxDepth": 4,
                            "maxParallel": 2,
                            "maxGroupDepth": 2,
                            "maxWorkflowInvocations": 2,
                            "allowNestedDynamic": false
                        }},
                        "allowedWorkflows": {allowed_workflows}
                    }}
                ],
                "edges": [
                    {{ "from": "router", "to": "$end", "on": "success" }}
                ]
            }}"#,
            agent_strategy = agent_strategy,
        ),
    )
    .unwrap();
}

fn dynamic_graph(app: &App, task_id: &str) -> DynamicGraphState {
    gold_band::dynamic_store::load_dynamic_graph(
        &app.paths
            .dynamic_graph_file(task_id, "run-001", "round-001", "router", "attempt-001"),
        &app.paths.repo_root,
    )
    .unwrap()
}

fn wait_for_invocation(
    provider: &DynamicProvider,
    node_id: &str,
    render_mode: UserPromptRenderMode,
) {
    for _ in 0..1000 {
        if provider
            .invocations
            .lock()
            .unwrap()
            .iter()
            .any(|invocation| {
                invocation.runtime_context.node_id == node_id
                    && invocation.user_prompt_render_mode == render_mode
            })
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("expected invocation for node `{node_id}` with render mode {render_mode:?}");
}

fn is_business_invocation(invocation: &WorkerInvocation) -> bool {
    !matches!(
        invocation.user_prompt_render_mode,
        UserPromptRenderMode::RuntimeFinalize | UserPromptRenderMode::RuntimeRepair
    )
}

fn init_git_repo(repo_root: &camino::Utf8Path) {
    let init = gold_band::process::background_command("git")
        .arg("-C")
        .arg(repo_root.as_str())
        .arg("init")
        .output()
        .unwrap();
    assert!(init.status.success());
    std::fs::write(repo_root.join("README.md"), "fixture").unwrap();
    let add = gold_band::process::background_command("git")
        .arg("-C")
        .arg(repo_root.as_str())
        .args(["add", "README.md"])
        .output()
        .unwrap();
    assert!(add.status.success());
    let commit = gold_band::process::background_command("git")
        .arg("-C")
        .arg(repo_root.as_str())
        .args([
            "-c",
            "user.name=Gold Band Test",
            "-c",
            "user.email=gold-band@example.test",
            "commit",
            "-m",
            "initial",
        ])
        .output()
        .unwrap();
    assert!(commit.status.success());
}

#[test]
fn ai_dynamic_fanout_runs_merge_acceptance_and_persists_graph() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-fanout";
    let provider = DynamicProvider::fanout();
    let app = App::with_provider(repo_root, Box::new(provider.clone()));
    let profile = first_profile_id(&app);
    write_task_file(&app, task_id);
    write_dynamic_workflow(&app, task_id, &profile, "[]");

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.outcome, Some(RunOutcome::Success));

    let graph = dynamic_graph(&app, task_id);
    assert_eq!(
        graph.run.status,
        gold_band::dynamic::DynamicRunStatus::Completed
    );
    assert_eq!(graph.run.outcome, Some(RunOutcome::Success));
    assert_eq!(graph.nodes.len(), 5);
    assert!(
        graph
            .nodes
            .iter()
            .all(|node| { node.status == DynamicNodeStatus::Completed && node.outcome.is_some() })
    );
    assert_eq!(graph.groups.len(), 1);
    assert_eq!(graph.groups[0].status, DynamicGroupStatus::Closed);
    assert_eq!(graph.groups[0].terminal_node_ids.len(), 3);
    assert_eq!(graph.proposals.len(), 4);
    assert!(graph.proposals.iter().all(|proposal| {
        proposal.validation_status == DynamicProposalValidationStatus::Accepted
    }));

    let invocations = provider.invocations.lock().unwrap();
    let business_invocations = invocations
        .iter()
        .filter(|invocation| is_business_invocation(invocation))
        .collect::<Vec<_>>();
    let node_ids = business_invocations
        .iter()
        .map(|invocation| invocation.runtime_context.node_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(node_ids[0], "bootstrap");
    assert_eq!(node_ids[3], "group-core-merge");
    assert_eq!(node_ids[4], "group-core-accept");
    let branch_nodes = node_ids[1..3].to_vec();
    assert!(branch_nodes.contains(&"branch-a"));
    assert!(branch_nodes.contains(&"branch-b"));
    let bootstrap = render_prompt_bundle(business_invocations[0]).unwrap();
    assert!(!bootstrap.system_prompt.contains("dynamic-run-001"));
    assert!(bootstrap.user_prompt.contains("dynamic-run-001"));
    assert!(bootstrap.user_prompt.contains("bootstrap"));
    assert!(bootstrap.user_prompt.contains("claude-acp"));
    assert!(bootstrap.system_prompt.contains("dynamic-node-completion"));
    assert!(
        bootstrap
            .user_prompt
            .contains("# 需求\nExercise AI-DYNAMIC")
    );
    assert!(
        bootstrap
            .user_prompt
            .contains("Design the first internal dynamic step")
    );
    let merge = render_prompt_bundle(business_invocations[3]).unwrap();
    assert!(merge.user_prompt.contains("group-core"));
    assert!(merge.user_prompt.contains("branch-a"));
    assert!(merge.user_prompt.contains("branch-b"));
}

#[test]
fn ai_dynamic_provider_runtime_error_does_not_enter_proposal_repair() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-provider-runtime-error";
    let provider = DynamicProvider::provider_runtime_error();
    let app = App::with_provider(repo_root, Box::new(provider.clone()));
    let profile = first_profile_id(&app);
    write_task_file(&app, task_id);
    write_dynamic_workflow(&app, task_id, &profile, "[]");

    let run = app.run_start(task_id, None).unwrap();

    assert_eq!(run.status, RunStatus::Paused);
    assert_eq!(run.pause_reason, Some(PauseReason::RuntimeAbnormal));
    let graph = dynamic_graph(&app, task_id);
    assert_eq!(graph.run.status, DynamicRunStatus::Paused);
    assert_eq!(graph.run.pause_reason, Some(PauseReason::RuntimeAbnormal));
    let bootstrap = graph
        .nodes
        .iter()
        .find(|node| node.id == "bootstrap")
        .expect("bootstrap node");
    assert_eq!(bootstrap.status, DynamicNodeStatus::Paused);
    assert_eq!(bootstrap.pause_reason, Some(PauseReason::RuntimeAbnormal));
    assert_eq!(
        bootstrap
            .runtime_error
            .as_ref()
            .map(|error| error.code.code.as_str()),
        Some("provider.server-unavailable")
    );

    let invocations = provider.invocations.lock().unwrap();
    assert_eq!(
        invocations.len(),
        (DEFAULT_AUTO_RETRY_MAX_ATTEMPTS + 1) as usize
    );
    assert!(
        invocations
            .iter()
            .all(|invocation| invocation.session_mode == SessionMode::New)
    );
    assert!(
        invocations
            .iter()
            .all(|invocation| invocation.resume_prompt.is_none())
    );
}

#[test]
fn ai_dynamic_merge_inner_continue_uses_user_message_render_mode() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-merge-pause-continue";
    let provider = DynamicProvider::merge_pause_then_continue();
    let app = App::with_provider(repo_root, Box::new(provider.clone()));
    let profile = first_profile_id(&app);
    write_task_file(&app, task_id);
    write_dynamic_workflow(&app, task_id, &profile, "[]");

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Paused);
    assert_eq!(run.pause_reason, Some(PauseReason::ProcessInterrupted));

    let graph = dynamic_graph(&app, task_id);
    let merge = graph
        .nodes
        .iter()
        .find(|node| node.id == "group-core-merge")
        .unwrap();
    assert_eq!(merge.status, DynamicNodeStatus::Paused);

    app.run_continue_dynamic_inner_background(
        task_id,
        "run-001",
        "round-001",
        "router",
        "attempt-001",
        "group-core-merge",
        "attempt-001",
        Some("merge-resume-001".to_string()),
        Some("继续".to_string().into()),
        Vec::new(),
        None,
        None,
    )
    .unwrap();
    wait_for_invocation(
        &provider,
        "group-core-merge",
        UserPromptRenderMode::UserMessage,
    );

    let invocations = provider.invocations.lock().unwrap();
    let merge_continue = invocations
        .iter()
        .find(|invocation| {
            invocation.runtime_context.node_id == "group-core-merge"
                && invocation.user_prompt_render_mode == UserPromptRenderMode::UserMessage
        })
        .unwrap();
    assert_eq!(
        merge_continue.user_prompt_render_mode,
        UserPromptRenderMode::UserMessage
    );
    let resume_prompt = merge_continue.resume_prompt.as_deref().unwrap_or_default();
    assert_eq!(resume_prompt.lines().next(), Some("继续"));
    assert!(resume_prompt.contains("show=\"false\""));
    assert!(resume_prompt.contains("请先完整执行本消息中的用户指令"));
    assert!(!resume_prompt.contains("artifact 输出约束"));
    assert!(!resume_prompt.contains("后续独立 turn"));
    assert!(!resume_prompt.contains("按当前输出契约输出 artifact"));
    assert_eq!(
        merge_continue.resume_prompt_id.as_deref(),
        Some("merge-resume-001")
    );

    let prompt = render_prompt_bundle(merge_continue).unwrap();
    assert_eq!(prompt.user_prompt.lines().next(), Some("继续"));
    assert!(prompt.user_prompt.contains("data-gold-band-hidden"));
    assert_eq!(prompt.display_text.as_deref(), Some("继续"));
    assert!(!prompt.user_prompt.contains("# 目标"));
    assert!(!prompt.user_prompt.contains("# Goal"));
    assert!(!prompt.user_prompt.contains("# 用户提示"));
    assert!(!prompt.user_prompt.contains("# User Tips"));
    assert!(!prompt.user_prompt.contains("# 任务"));
    assert!(!prompt.user_prompt.contains("# Task"));
}

#[test]
fn ai_dynamic_run_rejects_non_git_workspace_before_provider() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-non-git-prompt";
    let provider = DynamicProvider::fanout();
    let app = App::with_provider(repo_root, Box::new(provider.clone()));
    let profile = first_profile_id(&app);
    write_task_file_without_git(&app, task_id);
    write_dynamic_workflow(&app, task_id, &profile, "[]");

    let error = app.run_start(task_id, None).unwrap_err();
    assert_eq!(error.to_string(), "run.git-repository-required");
    assert!(provider.invocations.lock().unwrap().is_empty());
}

#[test]
fn ai_dynamic_worktree_fanout_is_rejected_before_provider_in_non_git_workspace() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-non-git-worktree-fanout";
    let provider = DynamicProvider::worktree_fanout();
    let app = App::with_provider(repo_root.clone(), Box::new(provider.clone()));
    let profile = first_profile_id(&app);
    write_task_file_without_git(&app, task_id);
    write_dynamic_workflow(&app, task_id, &profile, "[]");

    let error = app.run_start(task_id, None).unwrap_err();
    assert_eq!(error.to_string(), "run.git-repository-required");
    assert!(provider.invocations.lock().unwrap().is_empty());
}

#[test]
fn ai_dynamic_rejects_single_worktree_even_when_git_supports_worktree() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
    std::fs::create_dir_all(&repo_root).unwrap();
    init_git_repo(&repo_root);
    let task_id = "task-ai-dynamic-single-worktree";
    let provider = DynamicProvider::single_worktree_repair();
    let app = App::with_provider(repo_root, Box::new(provider.clone()));
    let profile = first_profile_id(&app);
    write_task_file(&app, task_id);
    write_dynamic_workflow(&app, task_id, &profile, "[]");

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.outcome, Some(RunOutcome::Success));

    let graph = dynamic_graph(&app, task_id);
    assert_eq!(
        graph.proposals[0].validation_status,
        DynamicProposalValidationStatus::Rejected
    );
    let error = graph.proposals[0]
        .validation_errors
        .iter()
        .find(|error| error.code == "dynamic.schema.additional-property")
        .expect("runtime-owned workspace field should be rejected by the schema");
    assert_eq!(error.path.as_deref(), Some("next.node.workspace"));
    assert_eq!(error.expected.as_deref(), Some("omit this field"));
    assert!(graph.proposals.iter().any(|proposal| {
        proposal.validation_status == DynamicProposalValidationStatus::Accepted
    }));

    let invocations = provider.invocations.lock().unwrap();
    let repair_invocation = invocations
        .iter()
        .find(|invocation| {
            invocation.user_prompt_render_mode == UserPromptRenderMode::RuntimeRepair
        })
        .unwrap();
    let resume_prompt = repair_invocation.resume_prompt.as_deref().unwrap();
    assert!(resume_prompt.contains("[dynamic.schema.additional-property]"));
    assert!(resume_prompt.contains("path: next.node.workspace"));
}

#[test]
fn ai_dynamic_worktree_fanout_injects_merge_workspace_metadata() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
    std::fs::create_dir_all(&repo_root).unwrap();
    init_git_repo(&repo_root);
    let task_id = "task-ai-dynamic-worktree-fanout";
    let provider = DynamicProvider::worktree_fanout();
    let app = App::with_provider(repo_root.clone(), Box::new(provider.clone()));
    let profile = first_profile_id(&app);
    write_task_file(&app, task_id);
    write_dynamic_workflow(&app, task_id, &profile, "[]");

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.outcome, Some(RunOutcome::Success));

    let graph = dynamic_graph(&app, task_id);
    let branch_a = graph
        .nodes
        .iter()
        .find(|node| node.id == "branch-a")
        .unwrap();
    let branch_b = graph
        .nodes
        .iter()
        .find(|node| node.id == "branch-b")
        .unwrap();
    assert_ne!(branch_a.workspace_id, branch_b.workspace_id);
    for branch in [branch_a, branch_b] {
        let workspace = graph
            .workspaces
            .iter()
            .find(|workspace| workspace.id == branch.workspace_id)
            .expect("fanout branch workspace should remain in the catalog");
        assert!(workspace.checkpoint_commit.is_some());
        assert_eq!(workspace.status, WorkspaceStatus::Released);
        assert!(!workspace.path.exists());
    }

    let invocations = provider.invocations.lock().unwrap();
    let merge_invocation = invocations
        .iter()
        .find(|invocation| invocation.runtime_context.node_id == "group-core-merge")
        .unwrap();
    let merge = render_prompt_bundle(merge_invocation).unwrap();
    assert_eq!(merge_invocation.session_mode, SessionMode::New);
    assert_eq!(
        merge_invocation.user_prompt_render_mode,
        UserPromptRenderMode::RequirementTask
    );
    assert!(merge.user_prompt.contains("# 需求"));
    assert!(merge.user_prompt.contains("# 任务"));
    assert!(!merge.user_prompt.contains("# 目标"));
    assert!(merge.user_prompt.contains("branch workspaces"));
    let branch_lines = merge
        .user_prompt
        .lines()
        .filter(|line| line.contains("branch=gb-dyn-task-ai-dynamic-worktree-fanout-run-001-dyn-"))
        .collect::<Vec<_>>();
    assert_eq!(branch_lines.len(), 2);
    assert_ne!(branch_lines[0], branch_lines[1]);
    assert!(merge.user_prompt.contains("head="));
    assert!(merge.user_prompt.contains("forkCommit="));
    assert!(merge.user_prompt.contains("checkpointCommit="));
    assert_eq!(merge.user_prompt.matches("status=clean").count(), 2);
    assert!(merge.user_prompt.contains(repo_root.as_str()));
}

#[test]
fn ai_dynamic_invocations_receive_task_input_attachments() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-input-attachments";
    let provider = DynamicProvider::fanout();
    let app = App::with_provider(repo_root, Box::new(provider.clone()));
    let profile = first_profile_id(&app);
    write_task_file(&app, task_id);
    let image_path = write_task_input_image(&app, task_id, "image.png");
    write_dynamic_workflow(&app, task_id, &profile, "[]");

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.outcome, Some(RunOutcome::Success));

    let image_path_string = image_path.to_string();
    let invocations = provider.invocations.lock().unwrap();
    assert!(!invocations.is_empty());
    assert!(
        invocations
            .iter()
            .filter(|invocation| is_business_invocation(invocation))
            .all(|invocation| {
                invocation.task_input_attachment_paths == vec![image_path_string.clone()]
                    && invocation.user_input_attachment_paths.is_empty()
            })
    );
    assert!(
        invocations
            .iter()
            .filter(|invocation| {
                invocation.user_prompt_render_mode == UserPromptRenderMode::RuntimeFinalize
            })
            .all(|invocation| {
                invocation.task_input_attachment_paths.is_empty()
                    && invocation.user_input_attachment_paths.is_empty()
            })
    );
    assert!(invocations.iter().all(|invocation| {
        invocation
            .runtime_context
            .task_inputs_dir
            .as_ref()
            .map(|dir| dir == &app.paths.task_dir(task_id).join("authoring").join("inputs"))
            .unwrap_or(false)
    }));

    let prompt = render_prompt_bundle(&invocations[0]).unwrap();
    assert_eq!(prompt.attachment_metas.len(), 1);
    assert_eq!(prompt.attachment_metas[0].name, "image.png");
    assert_eq!(prompt.attachment_metas[0].path, "task-inputs/image.png");
    match prompt.content_blocks.first() {
        Some(AcpContentBlock::Image(block)) => {
            let expected_uri = format!("file://{}", image_path_string.replace('\\', "/"));
            assert_eq!(block.mime_type, "image/png");
            assert_eq!(block.link.uri, expected_uri);
        }
        _ => panic!("expected image content block"),
    }
}

#[test]
fn ai_dynamic_nested_fanout_waits_for_child_group_before_parent_merge() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-nested-fanout";
    let provider = DynamicProvider::nested_fanout();
    let app = App::with_provider(repo_root, Box::new(provider.clone()));
    let profile = first_profile_id(&app);
    write_task_file(&app, task_id);
    write_dynamic_workflow(&app, task_id, &profile, "[]");

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.outcome, Some(RunOutcome::Success));

    let graph = dynamic_graph(&app, task_id);
    assert_eq!(graph.groups.len(), 2);
    let parent = graph
        .groups
        .iter()
        .find(|group| group.id == "group-core")
        .unwrap();
    let child = graph
        .groups
        .iter()
        .find(|group| group.id == "group-branch-a")
        .unwrap();
    assert_eq!(parent.status, DynamicGroupStatus::Closed);
    assert_eq!(parent.parent_group_id, None);
    assert_eq!(child.status, DynamicGroupStatus::Closed);
    assert_eq!(child.parent_group_id.as_deref(), Some("group-core"));
    assert_eq!(child.depth, 2);
    assert!(
        parent
            .terminal_node_ids
            .iter()
            .any(|node_id| node_id == "group-branch-a-accept")
    );
    assert!(
        parent
            .terminal_node_ids
            .iter()
            .any(|node_id| node_id == "branch-b")
    );
    let parent_merge = graph
        .nodes
        .iter()
        .find(|node| node.id == "group-core-merge")
        .unwrap();
    assert!(
        parent_merge
            .depends_on
            .iter()
            .any(|node_id| node_id == "group-branch-a-accept")
    );
    assert!(
        parent_merge
            .depends_on
            .iter()
            .any(|node_id| node_id == "branch-b")
    );

    let invocations = provider.invocations.lock().unwrap();
    let node_ids = invocations
        .iter()
        .map(|invocation| invocation.runtime_context.node_id.as_str())
        .collect::<Vec<_>>();
    let child_accept_position = node_ids
        .iter()
        .position(|node_id| *node_id == "group-branch-a-accept")
        .unwrap();
    let parent_merge_position = node_ids
        .iter()
        .position(|node_id| *node_id == "group-core-merge")
        .unwrap();
    assert!(child_accept_position < parent_merge_position);
}

#[test]
fn ai_dynamic_rejects_unallowed_workflow_invocation() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-invalid";
    let provider = DynamicProvider::invalid_workflow_invocation();
    let app = App::with_provider(repo_root, Box::new(provider));
    let profile = first_profile_id(&app);
    write_task_file(&app, task_id);
    write_dynamic_workflow(&app, task_id, &profile, "[]");

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Paused);
    assert_eq!(run.outcome, None);
    assert_eq!(run.pause_reason, Some(PauseReason::ErrorBlocked));

    let graph = dynamic_graph(&app, task_id);
    assert_eq!(graph.proposals.len(), 4);
    assert_eq!(
        graph.proposals.last().unwrap().validation_status,
        DynamicProposalValidationStatus::Rejected
    );
    assert_eq!(
        graph.proposals.last().unwrap().validation_errors[0].code,
        "dynamic.workflow-invocation.workflow-unallowed"
    );
    assert!(
        graph.proposals.last().unwrap().validation_errors[0]
            .message
            .contains("references unallowed workflow")
    );
}

#[test]
fn ai_dynamic_rejects_allowed_workflow_with_duplicate_workflow_id() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    let workflows_path = app.paths.workflow_templates_file();
    std::fs::create_dir_all(workflows_path.parent().unwrap().as_std_path()).unwrap();
    std::fs::write(
        workflows_path.as_std_path(),
        format!(
            r#"{{
                "version": "0.1",
                "lastUsedTemplateId": "template-b",
                "lastCreatedWorkflow": null,
                "templates": [
                    {{
                        "id": "default",
                        "name": "默认工作流",
                        "workflow": {{
                            "version": "0.1",
                            "id": "task-workflow",
                            "entry": "plan",
                            "control": {{}},
                            "nodes": [
                                {{ "id": "plan", "type": "worker", "provider": "claude-acp", "profile": "pf-builtin-plan", "goal": "Plan" }}
                            ],
                            "edges": [{{ "from": "plan", "to": "$end", "on": "success" }}]
                        }},
                        "createdAt": "2026-05-31T00:00:00Z",
                        "updatedAt": "2026-05-31T00:00:00Z"
                    }},
                    {{
                        "id": "template-a",
                        "name": "Template A",
                        "workflow": {{
                            "version": "0.1",
                            "id": "shared-workflow",
                            "entry": "child",
                            "control": {{}},
                            "nodes": [
                                {{ "id": "child", "type": "worker", "provider": "claude-acp", "profile": "pf-builtin-dev", "goal": "Run child work" }}
                            ],
                            "edges": [{{ "from": "child", "to": "$end", "on": "success" }}]
                        }},
                        "createdAt": "2026-05-31T00:00:00Z",
                        "updatedAt": "2026-05-31T00:00:00Z"
                    }},
                    {{
                        "id": "template-b",
                        "name": "Template B",
                        "workflow": {{
                            "version": "0.1",
                            "id": "shared-workflow",
                            "entry": "child",
                            "control": {{}},
                            "nodes": [
                                {{ "id": "child", "type": "worker", "provider": "claude-acp", "profile": "pf-builtin-dev", "goal": "Run child work again" }}
                            ],
                            "edges": [{{ "from": "child", "to": "$end", "on": "success" }}]
                        }},
                        "createdAt": "2026-05-31T00:00:00Z",
                        "updatedAt": "2026-05-31T00:00:00Z"
                    }}
                ]
            }}"#
        ),
    )
    .unwrap();

    let invalid_parent = serde_json::from_str(&format!(
        r#"{{
            "version": "0.1",
            "id": "parent-flow",
            "entry": "router",
            "nodes": [
                {{
                    "id": "router",
                    "type": "ai-dynamic",
                    "provider": "claude-acp",
                    "control": {{
                        "maxDynamicNodes": 10,
                        "maxFanout": 2,
                        "maxDepth": 4,
                        "maxParallel": 2,
                        "maxGroupDepth": 1,
                        "maxWorkflowInvocations": 2,
                        "allowNestedDynamic": false
                    }},
                    "allowedWorkflows": [{{ "workflowId": "shared-workflow" }}]
                }}
            ],
            "edges": [
                {{ "from": "router", "to": "$end", "on": "success" }}
            ]
        }}"#,
    ))
    .unwrap();

    let err = app
        .save_workflow_template("Parent".to_string(), invalid_parent)
        .unwrap_err();
    let typed = err.downcast_ref::<WorkflowValidationError>().unwrap();
    match typed {
        WorkflowValidationError::AiDynamicInvalidWorkflow {
            node_id,
            workflow_name,
            reason,
        } => {
            assert_eq!(node_id, "router");
            assert_eq!(workflow_name, "Template A");
            assert!(reason.contains("shared-workflow"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn ai_dynamic_repairs_over_limit_fanout_before_pausing() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-fanout-repair";
    let provider = DynamicProvider::fanout_repair();
    let app = App::with_provider(repo_root, Box::new(provider.clone()));
    let profile = first_profile_id(&app);
    write_task_file(&app, task_id);
    write_dynamic_workflow(&app, task_id, &profile, "[]");

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.outcome, Some(RunOutcome::Success));

    let graph = dynamic_graph(&app, task_id);
    assert!(graph.proposals.len() >= 2);
    assert_eq!(
        graph.proposals[0].validation_status,
        DynamicProposalValidationStatus::Rejected
    );
    assert_eq!(
        graph.proposals[0].validation_errors[0].code,
        "dynamic.fanout.max-fanout-exceeded"
    );
    assert!(
        graph.proposals[0].validation_errors[0]
            .message
            .contains("maxFanout")
    );
    assert!(graph.proposals.iter().any(|proposal| {
        proposal.validation_status == DynamicProposalValidationStatus::Accepted
    }));

    let invocations = provider.invocations.lock().unwrap();
    assert!(
        invocations
            .iter()
            .any(|invocation| invocation.session_mode == SessionMode::Continue)
    );
    let repair_invocation = invocations
        .iter()
        .find(|invocation| invocation.session_mode == SessionMode::Continue)
        .unwrap();
    assert!(
        repair_invocation
            .resume_prompt
            .as_deref()
            .unwrap()
            .contains("maxFanout")
    );
    assert!(
        repair_invocation
            .resume_prompt
            .as_deref()
            .unwrap()
            .contains("remaining dynamic nodes")
    );
}

#[test]
fn ai_dynamic_repairs_multiple_validation_errors_in_one_retry() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-multi-repair";
    let provider = DynamicProvider::multi_validation_repair();
    let app = App::with_provider(repo_root, Box::new(provider.clone()));
    let profile = first_profile_id(&app);
    write_task_file(&app, task_id);
    write_dynamic_workflow(&app, task_id, &profile, "[]");

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.outcome, Some(RunOutcome::Success));

    let graph = dynamic_graph(&app, task_id);
    assert!(graph.proposals.len() >= 2);
    assert_eq!(
        graph.proposals[0].validation_status,
        DynamicProposalValidationStatus::Rejected
    );
    assert!(
        graph.proposals[0]
            .validation_errors
            .iter()
            .any(|error| error.code == "dynamic.fanout.max-fanout-exceeded")
    );
    assert!(
        graph.proposals[0]
            .validation_errors
            .iter()
            .any(|error| error.code == "dynamic.profile.unknown"
                && error.message.contains("unknown profile `missing-profile`"))
    );
    assert!(graph.proposals.iter().any(|proposal| {
        proposal.validation_status == DynamicProposalValidationStatus::Accepted
    }));

    let invocations = provider.invocations.lock().unwrap();
    let repair_invocation = invocations
        .iter()
        .find(|invocation| invocation.session_mode == SessionMode::Continue)
        .unwrap();
    let resume_prompt = repair_invocation.resume_prompt.as_deref().unwrap();
    assert!(resume_prompt.contains("maxFanout"));
    assert!(resume_prompt.contains("unknown profile `missing-profile`"));
    assert!(resume_prompt.contains("allowed values:"));
    assert!(resume_prompt.contains("Available worker profile IDs:"));
}

#[test]
fn ai_dynamic_rejects_merge_acceptance_profile_fields() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-group-profile-repair";
    let provider = DynamicProvider::merge_acceptance_profile_repair();
    let app = App::with_provider(repo_root, Box::new(provider.clone()));
    let profile = first_profile_id(&app);
    write_task_file(&app, task_id);
    write_dynamic_workflow(&app, task_id, &profile, "[]");

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.outcome, Some(RunOutcome::Success));

    let graph = dynamic_graph(&app, task_id);
    assert_eq!(
        graph.proposals[0].validation_status,
        DynamicProposalValidationStatus::Rejected
    );
    assert!(graph.proposals[0].validation_errors.iter().any(|error| {
        error.code == "dynamic.merge.profile.unsupported"
            && error.path.as_deref() == Some("next.merge.profile")
            && error.expected.as_deref() == Some("omit this field")
    }));
    assert!(graph.proposals[0].validation_errors.iter().any(|error| {
        error.code == "dynamic.acceptance.profile.unsupported"
            && error.path.as_deref() == Some("next.acceptance.profile")
            && error.expected.as_deref() == Some("omit this field")
    }));

    let invocations = provider.invocations.lock().unwrap();
    let repair_invocation = invocations
        .iter()
        .find(|invocation| invocation.session_mode == SessionMode::Continue)
        .unwrap();
    let resume_prompt = repair_invocation.resume_prompt.as_deref().unwrap();
    assert!(resume_prompt.contains("path: next.merge.profile"));
    assert!(resume_prompt.contains("expected: omit this field"));
}

#[test]
fn ai_dynamic_parse_repair_prompt_includes_json_path() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-parse-repair";
    let provider = DynamicProvider::parse_repair();
    let app = App::with_provider(repo_root, Box::new(provider.clone()));
    let profile = first_profile_id(&app);
    write_task_file(&app, task_id);
    write_dynamic_workflow(&app, task_id, &profile, "[]");

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.outcome, Some(RunOutcome::Success));

    let invocations = provider.invocations.lock().unwrap();
    let repair_invocation = invocations
        .iter()
        .find(|invocation| invocation.session_mode == SessionMode::Continue)
        .unwrap();
    let resume_prompt = repair_invocation.resume_prompt.as_deref().unwrap();
    assert!(resume_prompt.contains("[dynamic.schema.required]"));
    assert!(resume_prompt.contains("path: next.merge.task"));
}

#[test]
fn ai_dynamic_repairs_missing_completion_without_empty_artifact() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-missing-artifact-repair";
    let provider = DynamicProvider::missing_artifact_repair();
    let app = App::with_provider(repo_root, Box::new(provider.clone()));
    let profile = first_profile_id(&app);
    write_task_file(&app, task_id);
    write_dynamic_workflow(&app, task_id, &profile, "[]");

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.outcome, Some(RunOutcome::Success));

    let invocations = provider.invocations.lock().unwrap();
    let repair_invocation = invocations
        .iter()
        .find(|invocation| invocation.session_mode == SessionMode::Continue)
        .unwrap();
    let resume_prompt = repair_invocation.resume_prompt.as_deref().unwrap();
    assert!(resume_prompt.contains("did not produce dynamic-node-completion"));

    let artifact_path = app.paths.dynamic_node_artifact_file(
        task_id,
        "run-001",
        "round-001",
        "router",
        "attempt-001",
        "bootstrap",
        "attempt-001",
        "dynamic-node-completion",
    );
    let metadata = std::fs::metadata(artifact_path.as_std_path()).unwrap();
    assert!(metadata.len() > 0);
}

#[test]
fn ai_dynamic_effective_schema_reflects_runtime_policy() {
    let schema = dynamic_completion_effective_schema(&DynamicCompletionSchemaPolicy {
        provider_required: false,
        node_model_required: false,
        agent_task_model_required: false,
        agent_task_model_visible: true,
        provider_ids: vec!["claude-acp".to_string()],
        model_names: Vec::new(),
        profile_ids: vec!["pf-builtin-dev".to_string()],
        workflow_ids: vec!["child-flow".to_string()],
        max_fanout: 2,
    });

    assert!(schema.pointer("/properties/source").is_none());
    assert_eq!(
        schema.pointer("/definitions/DynamicNext/properties/nodes/maxItems"),
        Some(&json!(2))
    );
    assert_eq!(
        schema.pointer("/definitions/DynamicNext/properties/nodes/minItems"),
        Some(&json!(2))
    );
    assert_eq!(
        schema.pointer("/definitions/DynamicNodeSpec/allOf/0/if/properties/kind/enum/0"),
        Some(&json!("worker"))
    );
    assert_eq!(
        schema.pointer("/definitions/DynamicNodeSpec/allOf/0/then/properties/provider"),
        Some(&json!(false))
    );
    assert_eq!(
        schema.pointer("/definitions/DynamicAgentTaskSpec/allOf/0/properties/provider"),
        Some(&json!(false))
    );
    assert_eq!(
        schema.pointer("/definitions/DynamicNodeSpec/properties/profile/enum/0"),
        Some(&json!("pf-builtin-dev"))
    );
    assert_eq!(
        schema.pointer("/definitions/DynamicNodeSpec/properties/workflowId/enum/0"),
        Some(&json!("child-flow"))
    );
}

#[test]
fn ai_dynamic_effective_schema_hides_agent_task_model_when_acceptance_model_is_configured() {
    let schema = dynamic_completion_effective_schema(&DynamicCompletionSchemaPolicy {
        provider_required: true,
        node_model_required: true,
        agent_task_model_required: false,
        agent_task_model_visible: false,
        provider_ids: vec!["claude-acp".to_string()],
        model_names: vec!["worker-model-a".to_string()],
        profile_ids: vec!["pf-builtin-dev".to_string()],
        workflow_ids: vec![],
        max_fanout: 2,
    });

    assert_eq!(
        schema.pointer("/definitions/DynamicNodeSpec/properties/model/type"),
        Some(&json!("string"))
    );
    assert_eq!(
        schema.pointer("/definitions/DynamicAgentTaskSpec/properties/model"),
        None
    );
}

#[test]
fn ai_dynamic_lists_resumable_session_nodes_and_uses_continue_session() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-session-continue";
    let provider = DynamicProvider::session_continue_prompt();
    let app = App::with_provider(repo_root, Box::new(provider.clone()));
    let profile = first_profile_id(&app);
    write_task_file(&app, task_id);
    write_dynamic_workflow(&app, task_id, &profile, "[]");

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.outcome, Some(RunOutcome::Success));

    let graph = dynamic_graph(&app, task_id);
    assert!(
        graph
            .nodes
            .iter()
            .any(|node| node.id == "branch-c" && node.session_mode == SessionMode::Continue)
    );
    assert!(
        graph.nodes.iter().any(|node| node.id == "branch-c"
            && node.continue_from_node_id.as_deref() == Some("branch-b"))
    );

    let invocations = provider.invocations.lock().unwrap();
    let branch_b = render_prompt_bundle(
        invocations
            .iter()
            .find(|invocation| invocation.runtime_context.node_id == "branch-b")
            .unwrap(),
    )
    .unwrap();
    assert!(branch_b.user_prompt.contains("branch-a"));
    assert!(branch_b.user_prompt.contains("branch-b"));
    assert!(!branch_b.system_prompt.contains("bootstrap title="));
    let branch_c = invocations
        .iter()
        .find(|invocation| invocation.runtime_context.node_id == "branch-c")
        .unwrap();
    assert_eq!(branch_c.session_mode, SessionMode::Continue);
    assert!(branch_c.continue_ref.is_some());
}

#[test]
fn ai_dynamic_continue_prompt_bundle_preserves_prompt_id() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-prompt-id";
    let provider = DynamicProvider::session_continue_prompt();
    let app = App::with_provider(repo_root, Box::new(provider));
    let profile = first_profile_id(&app);
    write_task_file(&app, task_id);
    write_dynamic_workflow(&app, task_id, &profile, "[]");

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Completed);

    let graph = dynamic_graph(&app, task_id);
    let branch_b = graph
        .nodes
        .iter()
        .find(|node| node.id == "branch-b")
        .unwrap();
    let branch_b_workspace = graph
        .workspaces
        .iter()
        .find(|workspace| workspace.id == branch_b.workspace_id)
        .unwrap();
    let continue_ref = serde_json::json!({ "sessionId": "branch-b-attempt-001" });
    let prepared_prompt = app
        .prepare_dynamic_acp_prompt_for_attempt(
            task_id,
            "run-001",
            "round-001",
            "router",
            "attempt-001",
            "branch-b",
            "attempt-001",
            "继续".to_string(),
            Some("acp-prompt-test".to_string()),
            Some(continue_ref),
        )
        .unwrap();
    assert_eq!(prepared_prompt.adapter_workspace_dir, app.paths.repo_root);
    assert_eq!(
        prepared_prompt.session_workspace_dir,
        branch_b_workspace.path
    );
    let prompt = prepared_prompt.prompt;

    assert_eq!(prompt.user_prompt, "继续");
    assert!(prompt.system_prompt.contains("用户主动打断当前工作"));
    assert!(prompt.system_prompt.contains("角色预设的执行流程"));
    assert!(!prompt.user_prompt.contains("Gold Band runtime context"));
    assert_eq!(prompt.prompt_id.as_deref(), Some("acp-prompt-test"));
}

#[test]
fn ai_dynamic_rejects_continue_target_outside_resumable_range() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-invalid-session-continue";
    let provider = DynamicProvider::invalid_session_continue();
    let app = App::with_provider(repo_root, Box::new(provider));
    let profile = first_profile_id(&app);
    write_task_file(&app, task_id);
    write_dynamic_workflow(&app, task_id, &profile, "[]");

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Paused);
    assert_eq!(run.pause_reason, Some(PauseReason::ErrorBlocked));

    let graph = dynamic_graph(&app, task_id);
    assert!(graph.proposals.iter().any(|proposal| {
        proposal
            .validation_errors
            .iter()
            .any(|error| error.code == "dynamic.node.session.workflow-invocation-disallowed")
    }));
}

#[test]
fn ai_dynamic_workflow_invocation_pause_and_continue_resume_child_run() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-child-pause";
    let workflow_id = Arc::new(Mutex::new(String::new()));
    let provider = DynamicProvider::workflow_invocation_pause_then_continue(workflow_id.clone());
    let app = with_available_claude_diagnostics(App::with_provider(
        repo_root,
        Box::new(provider.clone()),
    ));
    let profile = first_profile_id(&app);

    let store = app
        .save_workflow_template(
            "Child Flow".to_string(),
            serde_json::from_str(&format!(
                r#"{{
                    "version": "0.1",
                    "id": "child-flow",
                    "entry": "child",
                    "nodes": [
                        {{
                            "id": "child",
                            "type": "worker",
                            "provider": "claude-acp",
                            "profile": "pf-builtin-dev",
                            "goal": "Run child work"
                        }}
                    ],
                    "edges": [
                        {{ "from": "child", "to": "$end", "on": "success" }}
                    ]
                }}"#
            ))
            .unwrap(),
        )
        .unwrap();
    let child_template = store
        .templates
        .iter()
        .find(|template| template.name == "Child Flow")
        .unwrap();
    *workflow_id.lock().unwrap() = child_template.workflow.id.clone();

    write_task_file(&app, task_id);
    write_dynamic_workflow(
        &app,
        task_id,
        &profile,
        &format!(r#"[{{ "workflowId": "{}" }}]"#, child_template.workflow.id),
    );

    let paused = app.run_start(task_id, None).unwrap();
    assert_eq!(paused.status, RunStatus::Paused);
    assert_eq!(paused.pause_reason, Some(PauseReason::ProcessInterrupted));

    let graph = dynamic_graph(&app, task_id);
    assert_eq!(graph.run.status, DynamicRunStatus::Paused);
    assert_eq!(
        graph.run.pause_reason,
        Some(PauseReason::ProcessInterrupted)
    );
    let invocation_node = graph
        .nodes
        .iter()
        .find(|node| node.id == "child-flow-node")
        .unwrap();
    assert_eq!(invocation_node.status, DynamicNodeStatus::Paused);
    assert_eq!(invocation_node.outcome, None);
    assert_eq!(invocation_node.child_run_id.as_deref(), Some("run-002"));

    let child_run: gold_band::runtime::RunState =
        gold_band::storage::read_json(&app.paths.run_file(task_id, "run-002")).unwrap();
    assert_eq!(child_run.status, RunStatus::Paused);
    assert_eq!(
        child_run.pause_reason,
        Some(PauseReason::ProcessInterrupted)
    );

    let durable_parent = app.run_status(task_id, "run-001").unwrap();
    let parent_round: gold_band::runtime::RoundState =
        gold_band::storage::read_json(&app.paths.round_file(task_id, "run-001", "round-001"))
            .unwrap();
    let parent_node: gold_band::runtime::NodeState = gold_band::storage::read_json(
        &app.paths
            .node_file(task_id, "run-001", "round-001", "router", "attempt-001"),
    )
    .unwrap();
    assert_eq!(durable_parent.status, RunStatus::Paused);
    assert_eq!(parent_round.status, RunStatus::Paused);
    assert_eq!(parent_node.status, RunStatus::Paused);
    assert_eq!(parent_node.runtime_execution_id, None);
    assert_eq!(
        durable_parent.execution.phase,
        gold_band::runtime::RuntimeExecutionPhase::Paused
    );
    let locator = durable_parent.execution.locator.as_ref().unwrap();
    assert_eq!(locator.node_id, "child-flow-node");
    assert_eq!(locator.outer_node_id.as_deref(), Some("router"));
    assert_eq!(locator.outer_attempt_id.as_deref(), Some("attempt-001"));

    let resumed = app.run_continue(task_id, "run-001", None, None).unwrap();
    assert_eq!(resumed.status, RunStatus::Completed);
    assert_eq!(resumed.outcome, Some(RunOutcome::Success));

    let child_run: gold_band::runtime::RunState =
        gold_band::storage::read_json(&app.paths.run_file(task_id, "run-002")).unwrap();
    assert_eq!(child_run.status, RunStatus::Completed);
    assert_eq!(child_run.outcome, Some(RunOutcome::Success));

    let invocations = provider.invocations.lock().unwrap();
    assert!(invocations.iter().any(|invocation| {
        invocation.runtime_context.run_id == "run-002"
            && invocation.runtime_context.node_id == "child"
            && invocation.session_mode == SessionMode::Continue
    }));
}

#[test]
fn ai_dynamic_workflow_invocation_pause_and_continue_uses_user_message_render_mode() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-child-pause-user-message";
    let workflow_id = Arc::new(Mutex::new(String::new()));
    let provider = DynamicProvider::workflow_invocation_pause_then_continue(workflow_id.clone());
    let app = with_available_claude_diagnostics(App::with_provider(
        repo_root,
        Box::new(provider.clone()),
    ));
    let profile = first_profile_id(&app);

    let store = app
        .save_workflow_template(
            "Child Flow".to_string(),
            serde_json::from_str(&format!(
                r#"{{
                    "version": "0.1",
                    "id": "child-flow",
                    "entry": "child",
                    "nodes": [
                        {{
                            "id": "child",
                            "type": "worker",
                            "provider": "claude-acp",
                            "profile": "pf-builtin-dev",
                            "goal": "Run child work"
                        }}
                    ],
                    "edges": [
                        {{ "from": "child", "to": "$end", "on": "success" }}
                    ]
                }}"#
            ))
            .unwrap(),
        )
        .unwrap();
    let child_template = store
        .templates
        .iter()
        .find(|template| template.name == "Child Flow")
        .unwrap();
    *workflow_id.lock().unwrap() = child_template.workflow.id.clone();

    write_task_file(&app, task_id);
    write_dynamic_workflow(
        &app,
        task_id,
        &profile,
        &format!(r#"[{{ "workflowId": "{}" }}]"#, child_template.workflow.id),
    );

    let paused = app.run_start(task_id, None).unwrap();
    assert_eq!(paused.status, RunStatus::Paused);
    assert_eq!(paused.pause_reason, Some(PauseReason::ProcessInterrupted));

    let resumed = app
        .run_continue(
            task_id,
            "run-001",
            Some("prompt-continue-001".to_string()),
            Some("请继续检查这个会话".to_string()),
        )
        .unwrap();

    assert_eq!(resumed.status, RunStatus::Completed);
    assert_eq!(resumed.outcome, Some(RunOutcome::Success));

    let invocations = provider.invocations.lock().unwrap();
    let child_continue = invocations
        .iter()
        .find(|invocation| {
            invocation.runtime_context.run_id == "run-002"
                && invocation.runtime_context.node_id == "child"
                && invocation.session_mode == SessionMode::Continue
        })
        .unwrap();
    assert_eq!(
        child_continue.user_prompt_render_mode,
        UserPromptRenderMode::UserMessage
    );
    let resume_prompt = child_continue.resume_prompt.as_deref().unwrap_or_default();
    assert_eq!(resume_prompt.lines().next(), Some("请继续检查这个会话"));
    assert!(resume_prompt.contains("show=\"false\""));
    assert!(resume_prompt.contains("请先完整执行本消息中的用户指令"));
    assert_eq!(
        child_continue.resume_prompt_id.as_deref(),
        Some("prompt-continue-001")
    );
}

#[test]
fn ai_dynamic_pause_all_running_sessions_recursively_pauses_child_run() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-global-pause";
    let workflow_id = Arc::new(Mutex::new(String::new()));
    let provider = DynamicProvider::workflow_invocation_pause_then_continue(workflow_id.clone());
    let app = with_available_claude_diagnostics(App::with_provider(repo_root, Box::new(provider)));
    let profile = first_profile_id(&app);

    let store = app
        .save_workflow_template(
            "Child Flow".to_string(),
            serde_json::from_str(&format!(
                r#"{{
                    "version": "0.1",
                    "id": "child-flow",
                    "entry": "child",
                    "nodes": [
                        {{
                            "id": "child",
                            "type": "worker",
                            "provider": "claude-acp",
                            "profile": "pf-builtin-dev",
                            "goal": "Run child work"
                        }}
                    ],
                    "edges": [
                        {{ "from": "child", "to": "$end", "on": "success" }}
                    ]
                }}"#
            ))
            .unwrap(),
        )
        .unwrap();
    let child_template = store
        .templates
        .iter()
        .find(|template| template.name == "Child Flow")
        .unwrap();
    *workflow_id.lock().unwrap() = child_template.workflow.id.clone();

    write_task_file(&app, task_id);
    write_dynamic_workflow(
        &app,
        task_id,
        &profile,
        &format!(r#"[{{ "workflowId": "{}" }}]"#, child_template.workflow.id),
    );

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Paused);

    let paused_runs = app.pause_all_running_sessions().unwrap();
    assert!(paused_runs.is_empty());

    let paused = app
        .run_pause(task_id, "run-001", PauseReason::ProcessInterrupted)
        .unwrap();
    assert_eq!(paused.status, RunStatus::Paused);
    assert_eq!(paused.pause_reason, Some(PauseReason::ProcessInterrupted));

    let graph = dynamic_graph(&app, task_id);
    assert_eq!(
        graph.run.status,
        gold_band::dynamic::DynamicRunStatus::Paused
    );
    assert_eq!(
        graph.run.pause_reason,
        Some(PauseReason::ProcessInterrupted)
    );
    let child_run: gold_band::runtime::RunState =
        gold_band::storage::read_json(&app.paths.run_file(task_id, "run-002")).unwrap();
    assert_eq!(child_run.status, RunStatus::Paused);
    assert_eq!(
        child_run.pause_reason,
        Some(PauseReason::ProcessInterrupted)
    );
}

#[test]
fn ai_dynamic_workflow_invocation_uses_frozen_allowed_snapshot() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-child";
    let workflow_id = Arc::new(Mutex::new(String::new()));
    let provider = DynamicProvider::workflow_invocation(workflow_id.clone());
    let app = with_available_claude_diagnostics(App::with_provider(
        repo_root,
        Box::new(provider.clone()),
    ));
    let profile = first_profile_id(&app);

    let store = app
        .save_workflow_template(
            "Child Flow".to_string(),
            serde_json::from_str(&format!(
                r#"{{
                    "version": "0.1",
                    "id": "child-flow",
                    "entry": "child",
                    "nodes": [
                        {{
                            "id": "child",
                            "type": "worker",
                            "provider": "claude-acp",
                            "profile": "pf-builtin-dev",
                            "goal": "Run child work"
                        }}
                    ],
                    "edges": [
                        {{ "from": "child", "to": "$end", "on": "success" }}
                    ]
                }}"#
            ))
            .unwrap(),
        )
        .unwrap();
    let child_template = store
        .templates
        .iter()
        .find(|template| template.name == "Child Flow")
        .unwrap();
    *workflow_id.lock().unwrap() = child_template.workflow.id.clone();

    write_task_file(&app, task_id);
    write_dynamic_workflow(
        &app,
        task_id,
        &profile,
        &format!(r#"[{{ "workflowId": "{}" }}]"#, child_template.workflow.id),
    );

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.outcome, Some(RunOutcome::Success));

    let graph = dynamic_graph(&app, task_id);
    assert_eq!(graph.run.allowed_workflow_snapshots.len(), 1);
    assert_eq!(
        graph.run.allowed_workflow_snapshots[0].workflow_id,
        child_template.workflow.id
    );
    assert_eq!(
        graph.run.allowed_workflow_snapshots[0].workflow.id,
        child_template.workflow.id
    );
    let invocation_node = graph
        .nodes
        .iter()
        .find(|node| node.id == "child-flow-node")
        .unwrap();
    assert_eq!(invocation_node.kind, DynamicNodeKind::WorkflowInvocation);
    assert_eq!(
        invocation_node.workflow_snapshot_id.as_deref(),
        Some("wf-snapshot-001")
    );
    assert_eq!(invocation_node.child_run_id.as_deref(), Some("run-002"));

    let child_run: gold_band::runtime::RunState =
        gold_band::storage::read_json(&app.paths.run_file(task_id, "run-002")).unwrap();
    assert_eq!(child_run.status, RunStatus::Completed);
    assert_eq!(child_run.outcome, Some(RunOutcome::Success));

    let invocations = provider.invocations.lock().unwrap();
    let child_invocation = render_prompt_bundle(
        invocations
            .iter()
            .find(|invocation| invocation.runtime_context.run_id == "run-002")
            .unwrap(),
    )
    .unwrap();
    assert!(
        child_invocation
            .user_prompt
            .contains("Run child workflow from frozen snapshot")
    );
    assert!(child_invocation.user_prompt.contains("Run child work"));
    assert!(
        child_invocation
            .user_prompt
            .contains("# 需求\nExercise AI-DYNAMIC")
    );
}
