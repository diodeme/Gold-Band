use camino::Utf8PathBuf;
use gold_band::app::{App, CreateTaskInput, ProfileCommandError, ProfileInput, is_run_continuable};
use gold_band::config::{DesktopLanguage, ProviderDiagnosticSnapshot, RuntimeConfig};
use gold_band::domain::{RunStatus, SessionMode};
use gold_band::dsl::{WorkflowDsl, WorkflowValidationError};
use gold_band::provider::{
    DoctorResult, ProviderAdapter, ProviderCapabilities, ProviderInfo, ProviderRunResult,
    ProviderRunStatus, SessionRef, WorkerInvocation,
};
use gold_band::workflow_model_binding::{
    WorkerModelBinding, WorkflowModelBindings, definition_revision,
};
use std::collections::BTreeMap;
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
            runtime_error: None,
        })
    }

    fn run_worker_with_callbacks(
        &self,
        req: WorkerInvocation,
        _live_update: Option<gold_band::provider::AcpLiveUpdate<'_>>,
        _session_update: Option<gold_band::provider::AcpSessionUpdate<'_>>,
        prompt_accepted: Option<gold_band::provider::AcpPromptAccepted<'_>>,
    ) -> anyhow::Result<ProviderRunResult> {
        if let Some(callback) = prompt_accepted {
            callback(req.resume_prompt_id.as_deref().unwrap_or("test-prompt"))?;
        }
        self.run_worker(req)
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
            runtime_error: None,
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

fn configured_bindings(workflow: &WorkflowDsl) -> WorkflowModelBindings {
    WorkflowModelBindings {
        definition_revision: String::new(),
        binding_revision: 1,
        bindings: workflow
            .nodes
            .iter()
            .filter_map(|node| {
                let gold_band::dsl::NodeDsl::Worker(worker) = node else {
                    return None;
                };
                Some(WorkerModelBinding {
                    execution_slot_id: worker.execution_slot_id.clone().unwrap(),
                    agent_id: "claude-acp".to_string(),
                    model_id: None,
                    permission_mode_id: None,
                    config_options: BTreeMap::new(),
                })
            })
            .collect(),
    }
}

fn with_available_claude_diagnostics(app: App) -> App {
    app.with_provider_diagnostics_source(std::sync::Arc::new(|| {
        Ok(BTreeMap::from([(
            "claude-acp".to_string(),
            ProviderDiagnosticSnapshot {
                available: true,
                reason: None,
                checked_at: "2026-08-14T00:00:00Z".to_string(),
                capabilities: None,
            },
        )]))
    }))
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
fn create_task_accepts_lightweight_authoring_workflow_with_model_bindings() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    let template = app
        .workflow_templates()
        .unwrap()
        .templates
        .into_iter()
        .find(|template| template.id == "default-lightweight")
        .unwrap();
    let bindings = WorkflowModelBindings {
        definition_revision: template.model_bindings.definition_revision,
        binding_revision: template.model_bindings.binding_revision + 1,
        bindings: template
            .workflow
            .nodes
            .iter()
            .filter_map(|node| {
                let gold_band::dsl::NodeDsl::Worker(worker) = node else {
                    return None;
                };
                Some(WorkerModelBinding {
                    execution_slot_id: worker.execution_slot_id.clone().unwrap(),
                    agent_id: "claude-acp".to_string(),
                    model_id: Some("claude-sonnet-4-6".to_string()),
                    permission_mode_id: None,
                    config_options: BTreeMap::new(),
                })
            })
            .collect(),
    };

    let summary = app
        .create_task_from_requirement_with_bindings(
            CreateTaskInput {
                title: Some("Configured lightweight task".to_string()),
                description: None,
                requirement_file_name: None,
                requirement_content: "Implement the configured task".to_string(),
                workflow: template.workflow.clone(),
                workflow_template_id: Some(template.id),
            },
            template.workflow,
            bindings,
        )
        .expect("configured authoring workflow should create a task");

    let authoring = app.task_authoring_workflow(&summary.task.id).unwrap();
    let grill = authoring
        .workflow
        .nodes
        .iter()
        .find(|node| node.id() == "grill")
        .unwrap();
    let gold_band::dsl::NodeDsl::Worker(grill) = grill else {
        panic!("grill should be a worker node");
    };
    assert!(grill.provider.is_none());
    assert!(grill.model.is_none());
    assert_eq!(authoring.model_bindings.bindings.len(), 3);
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
    assert_eq!(default.workflow.control.max_attempts, Some(10));
    assert_eq!(default.workflow.control.max_rounds, Some(3));
    assert!(default.is_built_in);
    assert_eq!(
        default.optional_entry_stage.as_ref().unwrap().node_id,
        "interview"
    );
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
    assert!(
        default
            .workflow
            .edges
            .iter()
            .any(|edge| edge.from == "accept"
                && edge.to == "$new-round"
                && edge.new_round_entry.as_deref() == Some("dev"))
    );
    let plan = default
        .workflow
        .nodes
        .iter()
        .find(|node| node.id() == "plan")
        .unwrap();
    let gold_band::dsl::NodeDsl::Worker(plan) = plan else {
        panic!("plan should be a worker node");
    };
    assert_eq!(plan.goal.as_deref(), Some("分析导入的需求并产出实施方案。"));
}

#[test]
fn built_in_workflow_templates_include_lightweight_topology_and_are_idempotent() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);

    let first = app.workflow_templates().unwrap();
    let second = app.workflow_templates().unwrap();
    assert_eq!(
        second
            .templates
            .iter()
            .map(|template| template.id.as_str())
            .collect::<Vec<_>>(),
        vec!["default", "default-lightweight"]
    );
    assert_eq!(first.templates.len(), second.templates.len());

    let lightweight = second
        .templates
        .iter()
        .find(|template| template.id == "default-lightweight")
        .unwrap();
    assert!(lightweight.is_built_in);
    assert_eq!(lightweight.workflow.control.max_attempts, Some(10));
    assert_eq!(lightweight.workflow.control.max_rounds, Some(3));
    assert_eq!(lightweight.workflow.entry, "grill");
    assert_eq!(lightweight.workflow.nodes.len(), 3);
    assert_eq!(lightweight.workflow.edges.len(), 4);
    assert!(
        lightweight
            .workflow
            .nodes
            .iter()
            .any(|node| node.id() == "dev-test")
    );
    assert!(lightweight.workflow.edges.iter().any(|edge| {
        edge.from == "accept"
            && edge.to == gold_band::dsl::NEW_ROUND_NODE
            && edge.new_round_entry.as_deref() == Some("dev-test")
    }));
    assert_eq!(
        lightweight.optional_entry_stage.as_ref().unwrap().node_id,
        "grill"
    );
}

#[test]
fn built_in_workflow_entry_nodes_require_manual_check() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    let store = app.workflow_templates().unwrap();

    for (template_id, node_id) in [("default", "interview"), ("default-lightweight", "grill")] {
        let template = store
            .templates
            .iter()
            .find(|template| template.id == template_id)
            .unwrap();
        let node = template
            .workflow
            .nodes
            .iter()
            .find(|node| node.id() == node_id)
            .unwrap();
        let gold_band::dsl::NodeDsl::Worker(worker) = node else {
            panic!("{template_id}/{node_id} should be a worker node");
        };
        assert_eq!(worker.manual_check, Some(true));
        assert!(worker.output.is_none());
        assert!(worker.success_condition.is_none());
    }
}

#[test]
fn optional_entry_preference_trims_only_built_in_optional_entry() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    let store = app.workflow_templates().unwrap();

    for (template_id, expected_entry, removed_node) in [
        ("default", "plan", "interview"),
        ("default-lightweight", "dev-test", "grill"),
    ] {
        let template = store
            .templates
            .iter()
            .find(|item| item.id == template_id)
            .unwrap();
        let original = template.workflow.clone();
        let mut effective = original.clone();
        assert_eq!(
            gold_band::app::apply_optional_entry_preference(template, Some(false), &mut effective)
                .unwrap(),
            Some(false)
        );
        assert_eq!(effective.entry, expected_entry);
        assert!(!effective.nodes.iter().any(|node| node.id() == removed_node));
        assert_eq!(
            serde_json::to_value(&template.workflow).unwrap(),
            serde_json::to_value(&original).unwrap()
        );
    }

    let mut custom = store.templates[0].clone();
    custom.id = "custom".to_string();
    custom.is_built_in = false;
    custom.optional_entry_stage = None;
    let original = custom.workflow.clone();
    let mut effective = original.clone();
    assert_eq!(
        gold_band::app::apply_optional_entry_preference(&custom, Some(false), &mut effective)
            .unwrap(),
        None
    );
    assert_eq!(
        serde_json::to_value(&effective).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
}

#[test]
fn built_in_workflow_templates_are_read_only_by_metadata() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    let store = app.workflow_templates().unwrap();

    for template_id in ["default", "default-lightweight"] {
        let workflow = store
            .templates
            .iter()
            .find(|template| template.id == template_id)
            .unwrap()
            .workflow
            .clone();
        let update_error = app
            .update_workflow_template(template_id, workflow)
            .unwrap_err();
        assert_eq!(
            update_error
                .downcast_ref::<gold_band::app::WorkflowTemplateCommandError>()
                .unwrap()
                .code(),
            "workflow-template.readonly-built-in"
        );
        let delete_error = app.delete_workflow_template(template_id).unwrap_err();
        assert_eq!(
            delete_error
                .downcast_ref::<gold_band::app::WorkflowTemplateCommandError>()
                .unwrap()
                .code(),
            "workflow-template.readonly-built-in"
        );
    }
}

#[test]
fn default_workflow_template_localizes_goals_for_english_desktop_language() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let mut config = RuntimeConfig::default();
    config.desktop_language = DesktopLanguage::En;
    let app = App::with_config(repo_root, config);

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
    assert_eq!(
        plan.goal.as_deref(),
        Some("Analyze the imported requirement and produce an implementation plan.")
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
        ("interview", "访谈"),
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

    assert_eq!(default.workflow.entry, "interview");
}

#[test]
fn built_in_review_profile_scopes_review_to_current_changes() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);

    let profiles = app.profiles().unwrap();
    let review = profiles
        .profiles
        .iter()
        .find(|profile| profile.id == "pf-builtin-review")
        .unwrap();

    assert!(
        review
            .content
            .contains("只审查当前开发节点 / 本轮迭代产生的改动")
    );
    assert!(
        review
            .content
            .contains("优先以 `dev-report.md` 中列出的文件和行号作为审查范围")
    );
    assert!(review.content.contains("当前 git 工作区 diff"));
    assert!(review.content.contains("不得因此 REJECT"));
}

#[test]
fn built_in_validation_profiles_do_not_block_on_missing_external_evidence() {
    for (language, test_rule, review_rule) in [
        (
            DesktopLanguage::ZhCn,
            "环境问题或需要人工验收导致当前验证无法继续时，应如实记录未执行项和证据缺口，但不构成阻塞条件",
            "前序存在开发节点但没有产出 `dev-report.md`，不构成阻塞条件",
        ),
        (
            DesktopLanguage::En,
            "Environment issues or required manual acceptance may prevent validation from continuing, but do not constitute blocking conditions",
            "If a predecessor dev node did not produce `dev-report.md`, that absence is not a blocking condition",
        ),
    ] {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let mut config = RuntimeConfig::default();
        config.desktop_language = language;
        let app = App::with_config(repo_root, config);
        let profiles = app.profiles().unwrap();

        let test = profiles
            .profiles
            .iter()
            .find(|profile| profile.id == "pf-builtin-test")
            .unwrap();
        assert!(test.content.contains(test_rule));

        let review = profiles
            .profiles
            .iter()
            .find(|profile| profile.id == "pf-builtin-review")
            .unwrap();
        assert!(review.content.contains(review_rule));
        assert!(
            review.content.contains("git working tree") || review.content.contains("git 工作区")
        );
    }
}

#[test]
fn default_workflow_keeps_seeded_profile_ids_when_user_role_has_same_name() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    let user_profile = app
        .create_profile(ProfileInput {
            name: "方案".to_string(),
            summary: "用户方案角色".to_string(),
            content: "User plan role".to_string(),
            dynamic_template: false,
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
    assert_ne!(user_profile.id, "pf-builtin-plan");
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
            .contains("node `plan` associated role no longer exists; reset it")
    );
}

#[test]
fn deleting_unreferenced_profile_succeeds_without_force() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    let created = app
        .create_profile(ProfileInput {
            name: "未引用角色".to_string(),
            summary: "可直接删除".to_string(),
            content: "role body".to_string(),
            dynamic_template: false,
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
            name: "被引用角色".to_string(),
            summary: "template/task reference".to_string(),
            content: "role body".to_string(),
            dynamic_template: false,
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
    let app = with_available_claude_diagnostics(App::with_provider(
        repo_root,
        Box::new(InterruptThenSuccessProvider::new()),
    ));
    let created = app
        .create_profile(ProfileInput {
            name: "可恢复运行角色".to_string(),
            summary: "run snapshot reference".to_string(),
            content: "role body".to_string(),
            dynamic_template: false,
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

    let bindings = configured_bindings(&run_workflow);
    app.create_task_from_requirement_with_bindings(
        CreateTaskInput {
            title: Some("Actionable run".to_string()),
            description: None,
            requirement_file_name: None,
            requirement_content: "Task workflow uses resumable role".to_string(),
            workflow: run_workflow.clone(),
            workflow_template_id: None,
        },
        run_workflow,
        bindings,
    )
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
            name: "强制删除角色".to_string(),
            summary: "force delete".to_string(),
            content: "role body".to_string(),
            dynamic_template: false,
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

    let persisted_workflow = app.task_workflow("task-001").unwrap();

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
            .contains("associated role no longer exists; reset it")
    );
}

#[test]
fn force_deleting_referenced_profile_breaks_run_continue() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = with_available_claude_diagnostics(App::with_provider(
        repo_root,
        Box::new(InterruptThenSuccessProvider::new()),
    ));
    let created = app
        .create_profile(ProfileInput {
            name: "继续运行删除角色".to_string(),
            summary: "break continue".to_string(),
            content: "role body".to_string(),
            dynamic_template: false,
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

    let bindings = configured_bindings(&run_workflow);
    app.create_task_from_requirement_with_bindings(
        CreateTaskInput {
            title: Some("Force delete continue task".to_string()),
            description: None,
            requirement_file_name: None,
            requirement_content: "Task workflow uses resumable profile".to_string(),
            workflow: run_workflow.clone(),
            workflow_template_id: None,
        },
        run_workflow,
        bindings,
    )
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
            .contains("associated role no longer exists; reset it")
    );
}

#[test]
fn save_as_template_generates_new_workflow_id() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);

    let original = workflow(&app, "plan");
    let original_id = original.id.clone();
    let bindings = configured_bindings(&original);
    let store = app
        .save_workflow_template_with_bindings("Copied workflow".to_string(), original, bindings)
        .unwrap();
    let saved = store
        .templates
        .iter()
        .find(|template| template.name == "Copied workflow")
        .unwrap();

    assert_ne!(saved.workflow.id, original_id);
    assert!(!saved.workflow.id.trim().is_empty());
    assert_eq!(
        saved.model_bindings.definition_revision,
        definition_revision(&saved.workflow)
    );

    let reloaded = app.workflow_templates().unwrap();
    let reloaded = reloaded
        .templates
        .iter()
        .find(|template| template.id == saved.id)
        .unwrap();
    assert_eq!(reloaded.workflow.id, saved.workflow.id);
    assert_eq!(reloaded.model_bindings, saved.model_bindings);
}

#[test]
fn task_workflow_revisions_advance_once_per_binding_change() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    let initial_workflow = workflow(&app, "plan");
    let initial_bindings = configured_bindings(&initial_workflow);

    app.create_task_from_requirement_with_bindings(
        CreateTaskInput {
            title: Some("Revision task".to_string()),
            description: None,
            requirement_file_name: None,
            requirement_content: "Verify authoring revisions".to_string(),
            workflow: initial_workflow.clone(),
            workflow_template_id: None,
        },
        initial_workflow,
        initial_bindings,
    )
    .unwrap();

    let created = app.task_authoring_workflow("task-001").unwrap();
    assert_eq!(created.model_bindings.binding_revision, 1);

    app.save_task_workflow_with_bindings(
        "task-001",
        created.workflow.clone(),
        created.model_bindings.clone(),
    )
    .unwrap();
    let repeated = app.task_authoring_workflow("task-001").unwrap();
    assert_eq!(repeated.model_bindings.binding_revision, 1);

    let mut changed_bindings = repeated.model_bindings.clone();
    changed_bindings.bindings[0].agent_id = "codex-acp".to_string();
    app.save_task_workflow_with_bindings("task-001", repeated.workflow.clone(), changed_bindings)
        .unwrap();
    let binding_changed = app.task_authoring_workflow("task-001").unwrap();
    assert_eq!(binding_changed.model_bindings.binding_revision, 2);

    app.save_task_workflow_with_bindings(
        "task-001",
        binding_changed.workflow.clone(),
        binding_changed.model_bindings.clone(),
    )
    .unwrap();
    let binding_repeated = app.task_authoring_workflow("task-001").unwrap();
    assert_eq!(binding_repeated.model_bindings.binding_revision, 2);

    let previous_definition_revision = binding_repeated.model_bindings.definition_revision.clone();
    let mut definition_changed = binding_repeated.workflow.clone();
    let worker = definition_changed
        .nodes
        .iter_mut()
        .find_map(|node| match node {
            gold_band::dsl::NodeDsl::Worker(worker) => Some(worker),
            gold_band::dsl::NodeDsl::AiDynamic(_) => None,
        })
        .unwrap();
    worker.goal = Some("Changed definition only".to_string());
    app.save_task_workflow_with_bindings(
        "task-001",
        definition_changed,
        binding_repeated.model_bindings,
    )
    .unwrap();
    let definition_changed = app.task_authoring_workflow("task-001").unwrap();
    assert_eq!(definition_changed.model_bindings.binding_revision, 2);
    assert_ne!(
        definition_changed.model_bindings.definition_revision,
        previous_definition_revision
    );
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
    let app =
        with_available_claude_diagnostics(App::with_provider(repo_root, Box::new(SuccessProvider)));

    let run_workflow = workflow(&app, "plan");
    let bindings = configured_bindings(&run_workflow);
    app.create_task_from_requirement_with_bindings(
        CreateTaskInput {
            title: Some("Snapshot task".to_string()),
            description: None,
            requirement_file_name: Some("requirement.txt".to_string()),
            requirement_content: "Keep snapshot stable".to_string(),
            workflow: run_workflow.clone(),
            workflow_template_id: None,
        },
        run_workflow,
        bindings,
    )
    .unwrap();

    app.run_start("task-001", None).unwrap();
    app.save_task_workflow("task-001", workflow(&app, "dev"))
        .unwrap();

    let snapshot: WorkflowDsl =
        gold_band::storage::read_json(&app.paths.workflow_snapshot_file("task-001", "run-001"))
            .unwrap();
    let authoring = app.task_workflow("task-001").unwrap();
    assert_eq!(snapshot.entry, "plan");
    assert_eq!(authoring.entry, "dev");
}

#[test]
fn builtin_interview_profile_exists_and_has_content() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);

    let profiles = app.profiles().unwrap();
    let interview = profiles
        .profiles
        .iter()
        .find(|profile| profile.id == "pf-builtin-interview")
        .expect("interview builtin profile should exist");

    assert_eq!(interview.name, "访谈");
    assert!(interview.is_built_in);
    assert!(
        !interview.content.trim().is_empty(),
        "interview profile content must not be empty"
    );
    assert!(
        interview.content.contains("interview-spec.md"),
        "interview profile must declare its interview-spec.md artifact"
    );

    let shown = app.profile_show("pf-builtin-interview").unwrap();
    assert_eq!(shown.id, "pf-builtin-interview");
    assert!(shown.is_built_in);
    assert!(!shown.content.trim().is_empty());
}
