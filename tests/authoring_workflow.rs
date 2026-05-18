use camino::Utf8PathBuf;
use gold_band::app::{App, CreateTaskInput, ProfileInput, ProfileScope};
use gold_band::domain::SessionMode;
use gold_band::dsl::WorkflowDsl;
use gold_band::provider::{
    DoctorResult, ProviderAdapter, ProviderCapabilities, ProviderInfo, ProviderRunResult,
    ProviderRunStatus, SessionRef, WorkerInvocation,
};
use tempfile::tempdir;

#[derive(Clone)]
struct SuccessProvider;

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
        }
    }

    fn run_worker(&self, _req: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        Ok(ProviderRunResult {
            status: ProviderRunStatus::Success,
            exit_code: Some(0),
            result_payload: None,
            worker_ref_seed: Some(SessionRef {
                provider: "claude-code".to_string(),
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
            requirement_file_name: "requirement.md".to_string(),
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
fn default_workflow_uses_project_role_override_by_name() {
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
    assert_eq!(plan.profile.as_deref(), Some(project_profile.id.as_str()));
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
fn editing_authoring_workflow_does_not_mutate_run_snapshot() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::with_provider(repo_root, Box::new(SuccessProvider));

    app.create_task_from_requirement(CreateTaskInput {
        title: Some("Snapshot task".to_string()),
        description: None,
        requirement_file_name: "requirement.txt".to_string(),
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
