use camino::Utf8PathBuf;
use gold_band::app::{
    App, CreateTaskInput, ProfileCommandError, ProfileInput, ProfileScope, is_run_continuable,
};
use gold_band::domain::{RunStatus, SessionMode};
use gold_band::dsl::{WorkflowDsl, WorkflowValidationError};
use gold_band::provider::{
    DoctorResult, ProviderAdapter, ProviderCapabilities, ProviderInfo, ProviderRunResult,
    ProviderRunStatus, SessionRef, WorkerInvocation,
};
use tempfile::tempdir;

#[derive(Clone)]
struct SuccessProvider;

#[derive(Clone)]
struct InterruptThenSuccessProvider {
    interrupted: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl InterruptThenSuccessProvider {
    fn new() -> Self {
        Self {
            interrupted: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

impl ProviderAdapter for SuccessProvider {
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

    fn run_worker(&self, _req: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        Ok(ProviderRunResult {
            status: ProviderRunStatus::Success,
            exit_code: Some(0),
            result_payload: None,
            worker_ref_seed: Some(SessionRef {
                provider: "claude-acp".to_string(),
                mode: SessionMode::New,
                supports_open_session: true,
                supports_continue_session: true,
                continue_ref: Some(serde_json::json!({"sessionId":"session-1"})),
                open_command: Some("claude -c session-1".to_string()),
            }),
            stream_path: None,
        })
    }

    fn open_session(&self, _worker_ref: &SessionRef) -> anyhow::Result<()> {
        Ok(())
    }

    fn build_continue_command(&self, _worker_ref: &SessionRef) -> anyhow::Result<Option<String>> {
        Ok(Some("claude -c session-1".to_string()))
    }
}

impl ProviderAdapter for InterruptThenSuccessProvider {
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

    fn run_worker(&self, _req: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        let status = if self
            .interrupted
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            ProviderRunStatus::Success
        } else {
            ProviderRunStatus::Interrupted
        };
        Ok(ProviderRunResult {
            status,
            exit_code: Some(0),
            result_payload: None,
            worker_ref_seed: Some(SessionRef {
                provider: "claude-acp".to_string(),
                mode: SessionMode::Continue,
                supports_open_session: true,
                supports_continue_session: true,
                continue_ref: Some(serde_json::json!({"sessionId":"session-1"})),
                open_command: Some("claude -c session-1".to_string()),
            }),
            stream_path: None,
        })
    }

    fn open_session(&self, _worker_ref: &SessionRef) -> anyhow::Result<()> {
        Ok(())
    }

    fn build_continue_command(&self, _worker_ref: &SessionRef) -> anyhow::Result<Option<String>> {
        Ok(Some("claude -c session-1".to_string()))
    }
}

fn workflow(app: &App, entry: &str) -> WorkflowDsl {
    let mut workflow = app
        .workflow_templates()
        .unwrap()
        .templates
        .into_iter()
        .find(|template| template.id == "default")
        .unwrap()
        .workflow;
    workflow.entry = entry.to_string();
    let mut reachable = std::collections::HashSet::new();
    let mut pending = vec![entry.to_string()];
    while let Some(node_id) = pending.pop() {
        if !reachable.insert(node_id.clone()) {
            continue;
        }
        pending.extend(
            workflow
                .edges
                .iter()
                .filter(|edge| edge.from == node_id && edge.to != gold_band::dsl::END_NODE)
                .map(|edge| edge.to.clone()),
        );
    }
    workflow.nodes.retain(|node| reachable.contains(node.id()));
    workflow.edges.retain(|edge| {
        reachable.contains(&edge.from)
            && (edge.to == gold_band::dsl::END_NODE || reachable.contains(&edge.to))
    });
    workflow
}

#[test]
fn create_task_from_requirement_writes_authoring_files() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);

    let summary = app
        .create_task_from_requirement(CreateTaskInput {
            title: Some("Imported requirement".to_string()),
            description: Some("created from md".to_string()),
            requirement_file_name: None,
            requirement_content: "Build a workflow".to_string(),
            workflow: workflow(&app, "plan"),
            workflow_template_id: None,
        })
        .unwrap();

    assert_eq!(summary.task.id, "task-001");
    assert!(app.paths.task_file("task-001").exists());
    assert!(app.paths.requirement_file("task-001").exists());
    assert!(app.paths.workflow_file("task-001").exists());
}

#[test]
fn default_workflow_template_includes_simplified_output_schema() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);

    let store = app.workflow_templates().unwrap();
    let default = store
        .templates
        .iter()
        .find(|template| template.id == "default")
        .unwrap();
    assert!(default.workflow.control.max_attempts.is_none());
    assert!(default.workflow.control.max_rounds.is_none());
    let review = default
        .workflow
        .nodes
        .iter()
        .find(|node| node.id() == "review")
        .unwrap();
    let gold_band::dsl::NodeDsl::Worker(worker) = review else {
        panic!("review should be a worker node");
    };
    assert_eq!(
        worker
            .output
            .as_ref()
            .and_then(|output| output.schema.as_ref()),
        Some(&serde_json::json!({
            "reason": "String",
            "result": "boolean",
        }))
    );
    let cleanup = default
        .workflow
        .nodes
        .iter()
        .find(|node| node.id() == "cleanup")
        .unwrap();
    let gold_band::dsl::NodeDsl::Worker(cleanup) = cleanup else {
        panic!("cleanup should be a worker node");
    };
    assert!(cleanup.output.is_none());
    assert!(cleanup.success_condition.is_none());
    assert!(
        default
            .workflow
            .edges
            .iter()
            .any(|edge| edge.from == "accept" && edge.to == "cleanup")
    );
    assert!(
        default
            .workflow
            .edges
            .iter()
            .any(|edge| edge.from == "cleanup" && edge.to == "$end")
    );
}

#[test]
fn default_workflow_template_binds_seeded_profile_ids() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);

    let profiles = app.profiles().unwrap();
    let store = app.workflow_templates().unwrap();
    let default = store
        .templates
        .iter()
        .find(|template| template.id == "default")
        .unwrap();

    for (node_id, profile_name) in [
        ("plan", "方案"),
        ("dev", "开发"),
        ("review", "审查"),
        ("test", "测试"),
        ("accept", "验收"),
        ("cleanup", "清理"),
    ] {
        let expected = profiles
            .profiles
            .iter()
            .find(|profile| profile.name == profile_name)
            .unwrap();
        let node = default
            .workflow
            .nodes
            .iter()
            .find(|node| node.id() == node_id)
            .unwrap();
        let gold_band::dsl::NodeDsl::Worker(worker) = node else {
            panic!("{node_id} should be a worker node");
        };
        assert_eq!(worker.profile.as_deref(), Some(expected.id.as_str()));
    }
}

#[test]
fn default_workflow_keeps_seeded_profile_ids_when_project_role_has_same_name() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    let project_profile = app
        .create_profile(ProfileInput {
            scope: ProfileScope::Project,
            name: "方案".to_string(),
            summary: "项目方案角色".to_string(),
            content: "Project plan role".to_string(),
        })
        .unwrap();

    let store = app.workflow_templates().unwrap();
    let default = store
        .templates
        .iter()
        .find(|template| template.id == "default")
        .unwrap();
    let plan = default
        .workflow
        .nodes
        .iter()
        .find(|node| node.id() == "plan")
        .unwrap();
    let gold_band::dsl::NodeDsl::Worker(plan) = plan else {
        panic!("plan should be a worker node");
    };
    assert_ne!(project_profile.id, "pf-builtin-plan");
    assert_eq!(plan.profile.as_deref(), Some("pf-builtin-plan"));
}

#[test]
fn saving_workflow_requires_visible_profile() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);

    let mut missing_profile = workflow(&app, "plan");
    let gold_band::dsl::NodeDsl::Worker(plan) = missing_profile
        .nodes
        .iter_mut()
        .find(|node| node.id() == "plan")
        .unwrap()
    else {
        panic!("plan should be a worker node");
    };
    plan.profile = None;
    let err = app
        .save_workflow_template("Missing profile".to_string(), missing_profile)
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("node `plan` is not associated with role")
    );

    let mut hidden_profile = workflow(&app, "plan");
    let gold_band::dsl::NodeDsl::Worker(plan) = hidden_profile
        .nodes
        .iter_mut()
        .find(|node| node.id() == "plan")
        .unwrap()
    else {
        panic!("plan should be a worker node");
    };
    plan.profile = Some("missing-profile".to_string());
    let err = app
        .save_workflow_template("Hidden profile".to_string(), hidden_profile)
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("node `plan` associated role visibility changed; reset it")
    );
}

#[test]
fn deleting_unreferenced_profile_succeeds_without_force() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    let created = app
        .create_profile(ProfileInput {
            scope: ProfileScope::User,
            name: "未引用角色".to_string(),
            summary: "可直接删除".to_string(),
            content: "role body".to_string(),
        })
        .unwrap();

    let profiles = app.delete_profile(&created.id, false).unwrap();
    assert!(
        profiles
            .profiles
            .iter()
            .all(|profile| profile.id != created.id)
    );
}

#[test]
fn deleting_referenced_profile_requires_confirmation_for_templates_and_tasks() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    let created = app
        .create_profile(ProfileInput {
            scope: ProfileScope::User,
            name: "被引用角色".to_string(),
            summary: "template/task reference".to_string(),
            content: "role body".to_string(),
        })
        .unwrap();

    let mut template_workflow = workflow(&app, "plan");
    let gold_band::dsl::NodeDsl::Worker(plan) = template_workflow
        .nodes
        .iter_mut()
        .find(|node| node.id() == "plan")
        .unwrap()
    else {
        panic!("plan should be a worker node");
    };
    plan.profile = Some(created.id.clone());
    app.save_workflow_template(
        "Delete referenced profile".to_string(),
        template_workflow.clone(),
    )
    .unwrap();

    app.create_task_from_requirement(CreateTaskInput {
        title: Some("Referenced task".to_string()),
        description: None,
        requirement_file_name: None,
        requirement_content: "Task workflow uses custom profile".to_string(),
        workflow: template_workflow,
        workflow_template_id: None,
    })
    .unwrap();

    let err = app.delete_profile(&created.id, false).unwrap_err();
    let typed = err.downcast_ref::<ProfileCommandError>().unwrap();
    assert_eq!(typed.code(), "profile.delete-confirmation-required");
    assert_eq!(typed.params()["templateCount"], 1);
    assert_eq!(typed.params()["taskCount"], 1);
    assert_eq!(typed.params()["runCount"], 0);
}

#[test]
fn deleting_referenced_profile_requires_confirmation_for_actionable_runs() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::with_provider(repo_root, Box::new(InterruptThenSuccessProvider::new()));
    let created = app
        .create_profile(ProfileInput {
            scope: ProfileScope::User,
            name: "可恢复运行角色".to_string(),
            summary: "run snapshot reference".to_string(),
            content: "role body".to_string(),
        })
        .unwrap();

    let mut run_workflow = workflow(&app, "plan");
    let gold_band::dsl::NodeDsl::Worker(plan) = run_workflow
        .nodes
        .iter_mut()
        .find(|node| node.id() == "plan")
        .unwrap()
    else {
        panic!("plan should be a worker node");
    };
    plan.profile = Some(created.id.clone());

    app.create_task_from_requirement(CreateTaskInput {
        title: Some("Actionable run".to_string()),
        description: None,
        requirement_file_name: None,
        requirement_content: "Task workflow uses resumable role".to_string(),
        workflow: run_workflow,
        workflow_template_id: None,
    })
    .unwrap();

    let paused = app.run_start("task-001", None).unwrap();
    assert_eq!(paused.status, RunStatus::Paused);
    assert!(is_run_continuable(&paused));

    let err = app.delete_profile(&created.id, false).unwrap_err();
    let typed = err.downcast_ref::<ProfileCommandError>().unwrap();
    assert_eq!(typed.code(), "profile.delete-confirmation-required");
    assert_eq!(typed.params()["runCount"], 1);
}

#[test]
fn force_deleting_referenced_profile_requires_workflow_reset_afterward() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    let created = app
        .create_profile(ProfileInput {
            scope: ProfileScope::User,
            name: "强制删除角色".to_string(),
            summary: "force delete".to_string(),
            content: "role body".to_string(),
        })
        .unwrap();

    let mut template_workflow = workflow(&app, "plan");
    let gold_band::dsl::NodeDsl::Worker(plan) = template_workflow
        .nodes
        .iter_mut()
        .find(|node| node.id() == "plan")
        .unwrap()
    else {
        panic!("plan should be a worker node");
    };
    plan.profile = Some(created.id.clone());
    app.create_task_from_requirement(CreateTaskInput {
        title: Some("Force delete task".to_string()),
        description: None,
        requirement_file_name: None,
        requirement_content: "Task workflow uses profile".to_string(),
        workflow: template_workflow,
        workflow_template_id: None,
    })
    .unwrap();

    let persisted_workflow: WorkflowDsl =
        gold_band::storage::read_json(&app.paths.workflow_file("task-001")).unwrap();

    let profiles = app.delete_profile(&created.id, true).unwrap();
    assert!(
        profiles
            .profiles
            .iter()
            .all(|profile| profile.id != created.id)
    );

    let err = app
        .save_task_workflow("task-001", persisted_workflow)
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("associated role visibility changed; reset it")
    );
}

#[test]
fn force_deleting_referenced_profile_breaks_run_continue() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::with_provider(repo_root, Box::new(InterruptThenSuccessProvider::new()));
    let created = app
        .create_profile(ProfileInput {
            scope: ProfileScope::User,
            name: "继续运行删除角色".to_string(),
            summary: "break continue".to_string(),
            content: "role body".to_string(),
        })
        .unwrap();

    let mut run_workflow = workflow(&app, "plan");
    let gold_band::dsl::NodeDsl::Worker(plan) = run_workflow
        .nodes
        .iter_mut()
        .find(|node| node.id() == "plan")
        .unwrap()
    else {
        panic!("plan should be a worker node");
    };
    plan.profile = Some(created.id.clone());

    app.create_task_from_requirement(CreateTaskInput {
        title: Some("Force delete continue task".to_string()),
        description: None,
        requirement_file_name: None,
        requirement_content: "Task workflow uses resumable profile".to_string(),
        workflow: run_workflow,
        workflow_template_id: None,
    })
    .unwrap();

    let paused = app.run_start("task-001", None).unwrap();
    assert_eq!(paused.status, RunStatus::Paused);
    assert!(is_run_continuable(&paused));

    let profiles = app.delete_profile(&created.id, true).unwrap();
    assert!(
        profiles
            .profiles
            .iter()
            .all(|profile| profile.id != created.id)
    );

    let err = app
        .run_continue("task-001", "run-001", None, None)
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("associated role visibility changed; reset it")
    );
}

#[test]
fn save_as_template_generates_new_workflow_id() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);

    let original = workflow(&app, "plan");
    let original_id = original.id.clone();
    let store = app
        .save_workflow_template("Copied workflow".to_string(), original)
        .unwrap();
    let saved = store
        .templates
        .iter()
        .find(|template| template.name == "Copied workflow")
        .unwrap();

    assert_ne!(saved.workflow.id, original_id);
    assert!(!saved.workflow.id.trim().is_empty());
}

#[test]
fn updating_template_with_duplicate_workflow_id_fails() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);

    let first = app
        .save_workflow_template("First workflow".to_string(), workflow(&app, "plan"))
        .unwrap();
    let first_template = first
        .templates
        .iter()
        .find(|template| template.name == "First workflow")
        .unwrap()
        .clone();

    let second = app
        .save_workflow_template("Second workflow".to_string(), workflow(&app, "dev"))
        .unwrap();
    let second_template = second
        .templates
        .iter()
        .find(|template| template.name == "Second workflow")
        .unwrap()
        .clone();

    let mut duplicated = second_template.workflow.clone();
    duplicated.id = first_template.workflow.id.clone();
    let err = app
        .update_workflow_template(&second_template.id, duplicated)
        .unwrap_err();
    let typed = err.downcast_ref::<WorkflowValidationError>().unwrap();

    match typed {
        WorkflowValidationError::DuplicateWorkflowId {
            workflow_id,
            conflicts,
            ..
        } => {
            assert_eq!(workflow_id, &first_template.workflow.id);
            assert!(conflicts.contains(&first_template.name));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn creating_task_with_template_duplicate_workflow_id_fails() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);

    std::fs::create_dir_all(
        app.paths
            .workflow_templates_file()
            .parent()
            .unwrap()
            .as_std_path(),
    )
    .unwrap();
    std::fs::write(
        app.paths.workflow_templates_file().as_std_path(),
        r#"{
            "version": "0.1",
            "lastUsedTemplateId": "222",
            "lastCreatedWorkflow": null,
            "templates": [
                {
                    "id": "default",
                    "name": "默认工作流",
                    "workflow": {
                        "version": "0.1",
                        "id": "task-workflow",
                        "entry": "plan",
                        "control": {},
                        "nodes": [
                            { "id": "plan", "type": "worker", "provider": "claude-acp", "profile": "pf-builtin-plan", "goal": "Plan" }
                        ],
                        "edges": [{ "from": "plan", "to": "$end", "on": "success" }]
                    },
                    "createdAt": "2026-06-01T00:00:00Z",
                    "updatedAt": "2026-06-01T00:00:00Z"
                },
                {
                    "id": "attempt",
                    "name": "测试attempt",
                    "workflow": {
                        "version": "0.1",
                        "id": "workflow-dup",
                        "entry": "dev",
                        "control": {},
                        "nodes": [
                            { "id": "dev", "type": "worker", "provider": "claude-acp", "profile": "pf-builtin-dev", "goal": "Dev" }
                        ],
                        "edges": [{ "from": "dev", "to": "$end", "on": "success" }]
                    },
                    "createdAt": "2026-06-01T00:00:00Z",
                    "updatedAt": "2026-06-01T00:00:00Z"
                },
                {
                    "id": "222",
                    "name": "222",
                    "workflow": {
                        "version": "0.1",
                        "id": "workflow-dup",
                        "entry": "dev",
                        "control": {},
                        "nodes": [
                            { "id": "dev", "type": "worker", "provider": "claude-acp", "profile": "pf-builtin-dev", "goal": "Dev" }
                        ],
                        "edges": [{ "from": "dev", "to": "$end", "on": "success" }]
                    },
                    "createdAt": "2026-06-01T00:00:00Z",
                    "updatedAt": "2026-06-01T00:00:00Z"
                }
            ]
        }"#,
    ).unwrap();

    let task_workflow = serde_json::from_str(r#"{
        "version": "0.1",
        "id": "workflow-dup",
        "entry": "dev",
        "control": {},
        "nodes": [
            { "id": "dev", "type": "worker", "provider": "claude-acp", "profile": "pf-builtin-dev", "goal": "Dev" }
        ],
        "edges": [{ "from": "dev", "to": "$end", "on": "success" }]
    }"#).unwrap();

    let err = app
        .create_task_from_requirement(CreateTaskInput {
            title: Some("测试需求".to_string()),
            description: None,
            requirement_file_name: None,
            requirement_content: "duplicate template workflow id".to_string(),
            workflow: task_workflow,
            workflow_template_id: Some("222".to_string()),
        })
        .unwrap_err();
    let typed = err.downcast_ref::<WorkflowValidationError>().unwrap();
    match typed {
        WorkflowValidationError::DuplicateWorkflowId {
            workflow_name,
            workflow_id,
            conflicts,
        } => {
            assert_eq!(workflow_name, "222");
            assert_eq!(workflow_id, "workflow-dup");
            assert_eq!(conflicts, "测试attempt");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn editing_authoring_workflow_does_not_mutate_run_snapshot() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::with_provider(repo_root, Box::new(SuccessProvider));

    app.create_task_from_requirement(CreateTaskInput {
        title: Some("Snapshot task".to_string()),
        description: None,
        requirement_file_name: Some("requirement.txt".to_string()),
        requirement_content: "Keep snapshot stable".to_string(),
        workflow: workflow(&app, "plan"),
        workflow_template_id: None,
    })
    .unwrap();

    app.run_start("task-001", None).unwrap();
    app.save_task_workflow("task-001", workflow(&app, "dev"))
        .unwrap();

    let snapshot: WorkflowDsl =
        gold_band::storage::read_json(&app.paths.workflow_snapshot_file("task-001", "run-001"))
            .unwrap();
    let authoring: WorkflowDsl =
        gold_band::storage::read_json(&app.paths.workflow_file("task-001")).unwrap();
    assert_eq!(snapshot.entry, "plan");
    assert_eq!(authoring.entry, "dev");
}
