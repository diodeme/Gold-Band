use camino::Utf8PathBuf;
use gold_band::app::App;
use gold_band::domain::{PauseReason, RunOutcome, RunStatus, SessionMode};
use gold_band::dsl::WorkflowValidationError;
use gold_band::dynamic::{
    DynamicGraphState, DynamicGroupStatus, DynamicNodeKind, DynamicNodeStatus,
    DynamicProposalValidationStatus,
};
use gold_band::provider::{
    DoctorResult, OutputArtifactPayload, ProviderAdapter, ProviderCapabilities, ProviderInfo,
    ProviderResultPayload, ProviderRunResult, ProviderRunStatus, SessionRef, WorkerInvocation,
    render_prompt_bundle,
};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

#[derive(Clone)]
enum DynamicScenario {
    Fanout,
    NestedFanout,
    InvalidWorkflowInvocation,
    FanoutRepair,
    WorkflowInvocation { workflow_id: Arc<Mutex<String>> },
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

    fn nested_fanout() -> Self {
        Self::new(DynamicScenario::NestedFanout)
    }

    fn invalid_workflow_invocation() -> Self {
        Self::new(DynamicScenario::InvalidWorkflowInvocation)
    }

    fn fanout_repair() -> Self {
        Self::new(DynamicScenario::FanoutRepair)
    }

    fn workflow_invocation(workflow_id: Arc<Mutex<String>>) -> Self {
        Self::new(DynamicScenario::WorkflowInvocation { workflow_id })
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

        Ok(ProviderRunResult {
            status: ProviderRunStatus::Success,
            exit_code: Some(0),
            result_payload: Some(ProviderResultPayload { output_artifact }),
            worker_ref_seed: Some(SessionRef {
                provider: "claude-acp".to_string(),
                mode: SessionMode::New,
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
            (DynamicScenario::WorkflowInvocation { workflow_id }, "bootstrap") => {
                let workflow_id = workflow_id.lock().unwrap().clone();
                Some(workflow_invocation_completion(&workflow_id))
            }
            _ => None,
        }
    }
}

fn fanout_completion(profile: &str) -> String {
    format!(
        r#"{{
            "version": "0.1",
            "kind": "dynamic-node-completion",
            "status": "success",
            "summary": "split into two branches",
            "next": {{
                "type": "fanout",
                "groupId": "group-core",
                "nodes": [
                    {{
                        "id": "branch-a",
                        "kind": "worker",
                        "title": "Branch A",
                        "task": "Finish branch A",
                        "provider": "claude-acp",
                        "profile": "{profile}",
                        "workspace": {{ "mode": "readonly" }},
                        "dependsOn": ["bootstrap"]
                    }},
                    {{
                        "id": "branch-b",
                        "kind": "worker",
                        "title": "Branch B",
                        "task": "Finish branch B",
                        "provider": "claude-acp",
                        "profile": "{profile}",
                        "workspace": {{ "mode": "readonly" }},
                        "dependsOn": ["bootstrap"]
                    }}
                ],
                "merge": {{
                    "title": "Merge core",
                    "provider": "claude-acp",
                    "profile": "{profile}",
                    "task": "Merge branch outputs"
                }},
                "acceptance": {{
                    "title": "Accept core",
                    "provider": "claude-acp",
                    "profile": "{profile}",
                    "task": "Accept merged branch outputs"
                }}
            }}
        }}"#
    )
}

fn nested_fanout_completion(profile: &str) -> String {
    format!(
        r#"{{
            "version": "0.1",
            "kind": "dynamic-node-completion",
            "status": "success",
            "summary": "split branch A into deeper work",
            "next": {{
                "type": "fanout",
                "groupId": "group-branch-a",
                "nodes": [
                    {{
                        "id": "branch-a-1",
                        "kind": "worker",
                        "title": "Branch A 1",
                        "task": "Finish branch A part 1",
                        "provider": "claude-acp",
                        "profile": "{profile}",
                        "workspace": {{ "mode": "readonly" }},
                        "dependsOn": ["branch-a"]
                    }},
                    {{
                        "id": "branch-a-2",
                        "kind": "worker",
                        "title": "Branch A 2",
                        "task": "Finish branch A part 2",
                        "provider": "claude-acp",
                        "profile": "{profile}",
                        "workspace": {{ "mode": "readonly" }},
                        "dependsOn": ["branch-a"]
                    }}
                ],
                "merge": {{
                    "title": "Merge branch A",
                    "provider": "claude-acp",
                    "profile": "{profile}",
                    "task": "Merge branch A outputs"
                }},
                "acceptance": {{
                    "title": "Accept branch A",
                    "provider": "claude-acp",
                    "profile": "{profile}",
                    "task": "Accept branch A outputs"
                }}
            }}
        }}"#
    )
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

fn invalid_workflow_invocation_completion(profile: &str) -> String {
    format!(
        r#"{{
            "version": "0.1",
            "kind": "dynamic-node-completion",
            "status": "success",
            "summary": "try unallowed workflow",
            "next": {{
                "type": "single",
                "node": {{
                    "id": "invoke-missing",
                    "kind": "workflow-invocation",
                    "title": "Invoke missing workflow",
                    "task": "Run a workflow that is not allowed",
                    "provider": "claude-acp",
                    "profile": "{profile}",
                    "workspace": {{ "mode": "readonly" }},
                    "dependsOn": ["bootstrap"],
                    "workflowId": "missing-workflow"
                }}
            }}
        }}"#
    )
}

fn too_many_fanout_branches_completion(profile: &str) -> String {
    format!(
        r#"{{
            "version": "0.1",
            "kind": "dynamic-node-completion",
            "status": "success",
            "summary": "split into too many branches",
            "next": {{
                "type": "fanout",
                "groupId": "group-overflow",
                "nodes": [
                    {{
                        "id": "branch-a",
                        "kind": "worker",
                        "title": "Branch A",
                        "task": "Finish branch A",
                        "provider": "claude-acp",
                        "profile": "{profile}",
                        "workspace": {{ "mode": "readonly" }},
                        "dependsOn": ["bootstrap"]
                    }},
                    {{
                        "id": "branch-b",
                        "kind": "worker",
                        "title": "Branch B",
                        "task": "Finish branch B",
                        "provider": "claude-acp",
                        "profile": "{profile}",
                        "workspace": {{ "mode": "readonly" }},
                        "dependsOn": ["bootstrap"]
                    }},
                    {{
                        "id": "branch-c",
                        "kind": "worker",
                        "title": "Branch C",
                        "task": "Finish branch C",
                        "provider": "claude-acp",
                        "profile": "{profile}",
                        "workspace": {{ "mode": "readonly" }},
                        "dependsOn": ["bootstrap"]
                    }}
                ],
                "merge": {{
                    "title": "Merge overflow",
                    "provider": "claude-acp",
                    "profile": "{profile}",
                    "task": "Merge branch outputs"
                }},
                "acceptance": {{
                    "title": "Accept overflow",
                    "provider": "claude-acp",
                    "profile": "{profile}",
                    "task": "Accept merged branch outputs"
                }}
            }}
        }}"#
    )
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

fn write_dynamic_workflow(app: &App, task_id: &str, _profile: &str, allowed_workflows: &str) {
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
                        "provider": "claude-acp",
                        "control": {{
                            "maxDynamicNodes": 10,
                            "maxFanout": 2,
                            "maxDepth": 4,
                            "maxParallel": 2,
                            "maxGroupDepth": 2,
                            "maxWorkflowInvocations": 2,
                            "allowNestedDynamic": false
                        }},
                        "allowedWorkflows": {allowed_workflows},
                        "merge": {{
                            "provider": "claude-acp"
                        }},
                        "acceptance": {{
                            "provider": "claude-acp"
                        }}
                    }}
                ],
                "edges": [
                    {{ "from": "router", "to": "$end", "on": "success" }}
                ]
            }}"#
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
    assert_eq!(
        node_ids,
        vec![
            "bootstrap",
            "branch-a",
            "branch-b",
            "group-core-merge",
            "group-core-accept"
        ]
    );
    let bootstrap = render_prompt_bundle(&invocations[0]).unwrap();
    assert!(bootstrap.system_prompt.contains("dynamic-run-001"));
    assert!(bootstrap.system_prompt.contains("bootstrap"));
    assert!(bootstrap.system_prompt.contains("claude-acp"));
    assert!(bootstrap.system_prompt.contains("dynamic-node-completion"));
    assert!(bootstrap.user_prompt.contains("# Requirement\nExercise AI-DYNAMIC"));
    assert!(bootstrap.user_prompt.contains("# Task\nDesign the first internal dynamic step"));
    let merge = render_prompt_bundle(&invocations[3]).unwrap();
    assert!(merge.system_prompt.contains("group-core"));
    assert!(merge.system_prompt.contains("branch-a"));
    assert!(merge.system_prompt.contains("branch-b"));
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
    assert!(graph.proposals.last().unwrap().validation_errors[0].contains("references unallowed workflow"));
}

#[test]
fn ai_dynamic_rejects_allowed_workflow_with_duplicate_workflow_id() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    let profile = first_profile_id(&app);

    let workflows_path = app.paths.workflow_templates_file();
    std::fs::create_dir_all(app.paths.authoring_dir().as_std_path()).unwrap();
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
                                {{ "id": "child", "type": "worker", "provider": "claude-acp", "profile": "{profile}", "goal": "Run child work" }}
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
                                {{ "id": "child", "type": "worker", "provider": "claude-acp", "profile": "{profile}", "goal": "Run child work again" }}
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
                    "allowedWorkflows": [{{ "workflowId": "shared-workflow" }}],
                    "merge": {{ "provider": "claude-acp" }},
                    "acceptance": {{ "provider": "claude-acp" }}
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
    assert!(graph.proposals[0].validation_errors[0].contains("maxFanout"));
    assert!(graph.proposals.iter().any(|proposal| {
        proposal.validation_status == DynamicProposalValidationStatus::Accepted
    }));

    let invocations = provider.invocations.lock().unwrap();
    assert!(invocations.iter().any(|invocation| invocation.session_mode == SessionMode::Continue));
    let repair_invocation = invocations
        .iter()
        .find(|invocation| invocation.session_mode == SessionMode::Continue)
        .unwrap();
    assert!(repair_invocation
        .resume_prompt
        .as_deref()
        .unwrap()
        .contains("maxFanout"));
    assert!(repair_invocation
        .resume_prompt
        .as_deref()
        .unwrap()
        .contains("remaining dynamic nodes"));
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
                            "profile": "{profile}",
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
    assert!(child_invocation.user_prompt.contains("Run child workflow from frozen snapshot"));
    assert!(child_invocation.user_prompt.contains("Run child work"));
    assert!(child_invocation.user_prompt.contains("# Requirement\nExercise AI-DYNAMIC"));
}
