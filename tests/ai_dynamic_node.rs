use camino::Utf8PathBuf;
use gold_band::app::App;
use gold_band::domain::{NodeOutcome, PauseReason, RunOutcome, RunStatus, SessionMode};
use gold_band::dsl::WorkflowValidationError;
use gold_band::dynamic::{
    DynamicCompletionSchemaPolicy, DynamicGraphState, DynamicGroupStatus, DynamicNodeKind,
    DynamicNodeStatus, DynamicProposalValidationStatus, DynamicRunStatus,
    dynamic_completion_effective_schema,
};
use gold_band::provider::{
    AcpContentBlock, DoctorResult, OutputArtifactPayload, ProviderAdapter, ProviderCapabilities,
    ProviderInfo, ProviderResultPayload, ProviderRunResult, ProviderRunStatus, SessionRef,
    WorkerInvocation, render_prompt_bundle,
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
    FanoutRepair,
    MultiValidationRepair,
    MergeAcceptanceProfileRepair,
    ParseRepair,
    SessionContinuePrompt,
    InvalidSessionContinue,
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

    fn session_continue_prompt() -> Self {
        Self::new(DynamicScenario::SessionContinuePrompt)
    }

    fn invalid_session_continue() -> Self {
        Self::new(DynamicScenario::InvalidSessionContinue)
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
        self.invocations.lock().unwrap().push(req.clone());
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
        })
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

impl DynamicProvider {
    fn dynamic_artifact_for(&self, req: &WorkerInvocation) -> Option<String> {
        if req.output_contract.is_none() {
            return None;
        }
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
            (DynamicScenario::FanoutRepair, "bootstrap") => {
                if req.session_mode == SessionMode::Continue {
                    Some(fanout_completion(profile))
                } else {
                    Some(too_many_fanout_branches_completion(profile))
                }
            }
            (DynamicScenario::FanoutRepair, "branch-a" | "branch-b") => {
                Some(end_completion("branch done"))
            }
            (DynamicScenario::MultiValidationRepair, "bootstrap") => {
                if req.session_mode == SessionMode::Continue {
                    Some(fanout_completion(profile))
                } else {
                    Some(invalid_profile_and_overflow_completion())
                }
            }
            (DynamicScenario::MergeAcceptanceProfileRepair, "bootstrap") => {
                if req.session_mode == SessionMode::Continue {
                    Some(fanout_completion(profile))
                } else {
                    Some(merge_acceptance_profile_completion())
                }
            }
            (DynamicScenario::ParseRepair, "bootstrap") => {
                if req.session_mode == SessionMode::Continue {
                    Some(fanout_completion(profile))
                } else {
                    Some(missing_merge_task_completion())
                }
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
            (DynamicScenario::WorkflowInvocation { workflow_id }, "bootstrap")
            | (DynamicScenario::WorkflowInvocationPauseThenContinue { workflow_id }, "bootstrap") =>
            {
                let workflow_id = workflow_id.lock().unwrap().clone();
                Some(workflow_invocation_completion(&workflow_id))
            }
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
                        "workspace": { "mode": "readonly" },
                        "dependsOn": ["bootstrap"]
                    },
                    {
                        "id": "branch-b",
                        "kind": "worker",
                        "title": "Branch B",
                        "task": "Finish branch B",
                        "profile": "pf-builtin-dev",
                        "workspace": { "mode": "readonly" },
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
                        "workspace": { "mode": "worktree" },
                        "dependsOn": ["bootstrap"]
                    },
                    {
                        "id": "branch-b",
                        "kind": "worker",
                        "title": "Branch B",
                        "task": "Write branch B",
                        "profile": "pf-builtin-dev",
                        "workspace": { "mode": "worktree" },
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
                        "workspace": { "mode": "readonly" },
                        "dependsOn": ["branch-a"]
                    },
                    {
                        "id": "branch-a-2",
                        "kind": "worker",
                        "title": "Branch A 2",
                        "task": "Finish branch A part 2",
                        "profile": "pf-builtin-dev",
                        "workspace": { "mode": "readonly" },
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
                    "workspace": { "mode": "readonly" },
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
                        "workspace": { "mode": "readonly" },
                        "dependsOn": ["bootstrap"]
                    },
                    {
                        "id": "branch-b",
                        "kind": "worker",
                        "title": "Branch B",
                        "task": "Finish branch B",
                        "profile": "pf-builtin-dev",
                        "workspace": { "mode": "readonly" },
                        "dependsOn": ["bootstrap"]
                    },
                    {
                        "id": "branch-c",
                        "kind": "worker",
                        "title": "Branch C",
                        "task": "Finish branch C",
                        "profile": "pf-builtin-dev",
                        "workspace": { "mode": "readonly" },
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
                        "workspace": { "mode": "readonly" },
                        "dependsOn": ["bootstrap"]
                    },
                    {
                        "id": "branch-b",
                        "kind": "worker",
                        "title": "Branch B",
                        "task": "Finish branch B",
                        "profile": "missing-profile",
                        "workspace": { "mode": "readonly" },
                        "dependsOn": ["bootstrap"]
                    },
                    {
                        "id": "branch-c",
                        "kind": "worker",
                        "title": "Branch C",
                        "task": "Finish branch C",
                        "profile": "missing-profile",
                        "workspace": { "mode": "readonly" },
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
                        "workspace": { "mode": "readonly" },
                        "dependsOn": ["bootstrap"]
                    },
                    {
                        "id": "branch-b",
                        "kind": "worker",
                        "title": "Branch B",
                        "task": "Finish branch B",
                        "profile": "pf-builtin-dev",
                        "workspace": { "mode": "readonly" },
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
                        "workspace": { "mode": "readonly" },
                        "dependsOn": ["bootstrap"]
                    },
                    {
                        "id": "branch-b",
                        "kind": "worker",
                        "title": "Branch B",
                        "task": "Finish branch B",
                        "profile": "pf-builtin-dev",
                        "workspace": { "mode": "readonly" },
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
                        "workspace": { "mode": "readonly" },
                        "dependsOn": ["bootstrap"]
                    },
                    {
                        "id": "branch-b",
                        "kind": "worker",
                        "title": "Branch B",
                        "task": "Finish branch B then continue same chat for final wrap-up",
                        "profile": "pf-builtin-dev",
                        "workspace": { "mode": "readonly" },
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
                    "workspace": { "mode": "readonly" },
                    "dependsOn": ["branch-b"]
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
                    "workspace": { "mode": "readonly" },
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
                    "workspace": {{ "mode": "readonly" }},
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
    gold_band::storage::read_json(&app.paths.dynamic_graph_file(
        task_id,
        "run-001",
        "round-001",
        "router",
        "attempt-001",
    ))
    .unwrap()
}

fn init_git_repo(repo_root: &camino::Utf8Path) {
    let init = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root.as_str())
        .arg("init")
        .status()
        .unwrap();
    assert!(init.success());
    std::fs::write(repo_root.join("README.md"), "fixture").unwrap();
    let add = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root.as_str())
        .args(["add", "README.md"])
        .status()
        .unwrap();
    assert!(add.success());
    let commit = std::process::Command::new("git")
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
        .status()
        .unwrap();
    assert!(commit.success());
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
    assert_eq!(graph.groups[0].terminal_node_ids.len(), 2);
    assert_eq!(graph.proposals.len(), 3);
    assert!(graph.proposals.iter().all(|proposal| {
        proposal.validation_status == DynamicProposalValidationStatus::Accepted
    }));

    let invocations = provider.invocations.lock().unwrap();
    let node_ids = invocations
        .iter()
        .map(|invocation| invocation.runtime_context.node_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(node_ids[0], "bootstrap");
    assert_eq!(node_ids[3], "group-core-merge");
    assert_eq!(node_ids[4], "group-core-accept");
    let branch_nodes = node_ids[1..3].to_vec();
    assert!(branch_nodes.contains(&"branch-a"));
    assert!(branch_nodes.contains(&"branch-b"));
    let bootstrap = render_prompt_bundle(&invocations[0]).unwrap();
    assert!(bootstrap.system_prompt.contains("dynamic-run-001"));
    assert!(bootstrap.system_prompt.contains("bootstrap"));
    assert!(bootstrap.system_prompt.contains("claude-acp"));
    assert!(bootstrap.system_prompt.contains("dynamic-node-completion"));
    assert!(
        bootstrap
            .user_prompt
            .contains("# Requirement\nExercise AI-DYNAMIC")
    );
    assert!(
        bootstrap
            .user_prompt
            .contains("# Task\nDesign the first internal dynamic step")
    );
    let merge = render_prompt_bundle(&invocations[3]).unwrap();
    assert!(merge.system_prompt.contains("group-core"));
    assert!(merge.system_prompt.contains("branch-a"));
    assert!(merge.system_prompt.contains("branch-b"));
}

#[test]
fn ai_dynamic_non_git_workspace_prompt_disables_worktree() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-non-git-prompt";
    let provider = DynamicProvider::fanout();
    let app = App::with_provider(repo_root, Box::new(provider.clone()));
    let profile = first_profile_id(&app);
    write_task_file(&app, task_id);
    write_dynamic_workflow(&app, task_id, &profile, "[]");

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.outcome, Some(RunOutcome::Success));

    let invocations = provider.invocations.lock().unwrap();
    let bootstrap = render_prompt_bundle(&invocations[0]).unwrap();
    assert!(
        bootstrap.system_prompt.contains("Workspace capability")
            || bootstrap.system_prompt.contains("Workspace 能力")
    );
    assert!(bootstrap.system_prompt.contains("supportsWorktree: false"));
    assert!(
        bootstrap
            .system_prompt
            .contains("workspace.mode=\"worktree\"")
    );
}

#[test]
fn ai_dynamic_rejects_worktree_fanout_in_non_git_workspace() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-non-git-worktree-fanout";
    let provider = DynamicProvider::worktree_fanout();
    let app = App::with_provider(repo_root.clone(), Box::new(provider.clone()));
    let profile = first_profile_id(&app);
    write_task_file(&app, task_id);
    write_dynamic_workflow(&app, task_id, &profile, "[]");

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Paused);
    assert_eq!(run.outcome, None);
    assert_eq!(run.pause_reason, Some(PauseReason::ErrorBlocked));

    let graph = dynamic_graph(&app, task_id);
    assert!(graph.proposals.iter().any(|proposal| {
        proposal.validation_status == DynamicProposalValidationStatus::Rejected
            && proposal.validation_errors.iter().any(|error| {
                error.code == "dynamic.node.workspace.worktree-git-required"
                    && error.params["workspacePath"] == repo_root.as_str()
                    && error.params["reasonCode"] == "git-unavailable-or-non-git"
            })
    }));

    let invocations = provider.invocations.lock().unwrap();
    assert!(invocations.iter().all(|invocation| {
        !matches!(
            invocation.runtime_context.node_id.as_str(),
            "branch-a" | "branch-b"
        )
    }));
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
    assert!(!branch_a.workspace_path.as_ref().unwrap().exists());
    assert!(!branch_b.workspace_path.as_ref().unwrap().exists());

    let invocations = provider.invocations.lock().unwrap();
    let merge_invocation = invocations
        .iter()
        .find(|invocation| invocation.runtime_context.node_id == "group-core-merge")
        .unwrap();
    let merge = render_prompt_bundle(merge_invocation).unwrap();
    assert!(merge.system_prompt.contains("branch workspaces"));
    assert!(
        merge
            .system_prompt
            .contains("branch=gb-dynamic-task-ai-dynamic-worktree-fanout-run-001-router-branch-a")
    );
    assert!(
        merge
            .system_prompt
            .contains("branch=gb-dynamic-task-ai-dynamic-worktree-fanout-run-001-router-branch-b")
    );
    assert!(merge.system_prompt.contains("head="));
    assert!(merge.system_prompt.contains("mergeBase="));
    assert!(merge.system_prompt.contains("status=?? branch-a.txt"));
    assert!(merge.system_prompt.contains("status=?? branch-b.txt"));
    assert!(merge.user_prompt.contains("Main workspace:"));
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
            .all(|invocation| invocation.input_attachment_paths == vec![image_path_string.clone()])
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
            assert_eq!(block.uri.as_deref(), Some(expected_uri.as_str()));
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
    assert!(branch_b.system_prompt.contains("branch-a"));
    assert!(branch_b.system_prompt.contains("branch-b"));
    assert!(!branch_b.system_prompt.contains("bootstrap title="));
    let branch_c = invocations
        .iter()
        .find(|invocation| invocation.runtime_context.node_id == "branch-c")
        .unwrap();
    assert_eq!(branch_c.session_mode, SessionMode::Continue);
    assert!(branch_c.continue_ref.is_some());
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
fn ai_dynamic_run_kill_recursively_marks_child_run_and_dynamic_nodes_killed() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-kill-child";
    let workflow_id = Arc::new(Mutex::new(String::new()));
    let provider = DynamicProvider::workflow_invocation(workflow_id.clone());
    let app = App::with_provider(repo_root, Box::new(provider.clone()));
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
    assert_eq!(run.outcome, Some(RunOutcome::Success));

    let killed = app.run_kill(task_id, "run-001").unwrap();
    assert_eq!(killed.outcome, Some(RunOutcome::Killed));

    let graph = dynamic_graph(&app, task_id);
    assert_eq!(graph.run.outcome, Some(RunOutcome::Killed));
    let child_node = graph
        .nodes
        .iter()
        .find(|node| node.id == "child-flow-node")
        .unwrap();
    assert_eq!(child_node.outcome, Some(NodeOutcome::Killed));
    let child_run: gold_band::runtime::RunState =
        gold_band::storage::read_json(&app.paths.run_file(task_id, "run-002")).unwrap();
    assert_eq!(child_run.outcome, Some(RunOutcome::Killed));
}

#[test]
fn ai_dynamic_workflow_invocation_pause_and_continue_resume_child_run() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-child-pause";
    let workflow_id = Arc::new(Mutex::new(String::new()));
    let provider = DynamicProvider::workflow_invocation_pause_then_continue(workflow_id.clone());
    let app = App::with_provider(repo_root, Box::new(provider.clone()));
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
    assert_eq!(invocation_node.child_run_id.as_deref(), Some("run-002"));

    let child_run: gold_band::runtime::RunState =
        gold_band::storage::read_json(&app.paths.run_file(task_id, "run-002")).unwrap();
    assert_eq!(child_run.status, RunStatus::Paused);
    assert_eq!(
        child_run.pause_reason,
        Some(PauseReason::ProcessInterrupted)
    );

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
fn ai_dynamic_pause_all_running_sessions_recursively_pauses_child_run() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-ai-dynamic-global-pause";
    let workflow_id = Arc::new(Mutex::new(String::new()));
    let provider = DynamicProvider::workflow_invocation_pause_then_continue(workflow_id.clone());
    let app = App::with_provider(repo_root, Box::new(provider));
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
    let app = App::with_provider(repo_root, Box::new(provider.clone()));
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
            .contains("# Requirement\nExercise AI-DYNAMIC")
    );
}
