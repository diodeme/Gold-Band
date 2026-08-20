mod ids;
mod node_executor;
mod notification;
pub mod observability;
mod orchestrator;
mod profile_resolver;
mod profiles;
mod runtime_recovery;
mod state_access;
mod state_factory;
mod transition_context;

pub use self::notification::{
    INITIAL_DIRECT_TURN_ID, InterventionNotification, InterventionType, NotificationDedup,
    direct_conversation_agent_label, make_dedup_key, make_dedup_key_with_suffix,
    make_turn_dedup_key, reason_key,
};
pub use self::orchestrator::{AcceptedRun, PreparedRun};
pub use self::runtime_recovery::{
    RuntimeCandidateRegistration, RuntimeRecoveryCoordinator, RuntimeRecoveryError,
};

use crate::acp::client as acp_client;
use crate::acp::commands::AcpCommandItem;
use crate::config::{
    AppearancePreference, ConsoleThemeName, ConversationAutoConfig, DesktopAvailableUpdate,
    DesktopLanguage, DesktopUpdateBadgeState, ManagedAgentConfig, ManagedAgentId, McpServerConfig,
    McpServerHealthResult, PersonalizationPreference, ProviderDiagnosticSnapshot, RuntimeConfig,
    RuntimeLogLevel, SettingsConfig, SkillMeta, SkillSource, StateConfig,
};
use crate::control::{ControlDecision, decide_next_step};
use crate::domain::{NodeOutcome, RunOutcome};
use crate::domain::{PauseReason, RunStatus, SessionMode, VERSION};
use crate::dsl::{
    AiDynamicAgentStrategy, END_NODE, EdgeDsl, EdgeOutcome, JsonConditionDsl, NEW_ROUND_NODE,
    NodeDsl, OutputContractDsl, OutputKind, ValidatedWorkflow, WorkerNode, WorkflowControl,
    WorkflowDsl, WorkflowValidationError, validate_authoring_workflow, validate_workflow,
    validate_workflow_snapshot, workflow_contains_ai_dynamic,
};
use crate::dynamic::{
    DynamicNodeStatus, DynamicRunPhase, DynamicRunStatus, dynamic_leaf_is_active,
    refresh_dynamic_current_leaf_ids, write_dynamic_node_state,
};
use crate::dynamic_store::load_dynamic_graph;
use crate::mcp::McpManager;
use crate::process::recover_persisted_process_group;
use crate::provider::{
    ConversationPromptInput, DoctorResult, PromptBundle, PromptVisibility, ProviderAdapter,
    ProviderCapabilities, ProviderInfo, UserPromptRenderMode, provider_from_agent,
    render_prompt_bundle, supported_modes_from_capabilities,
};
use crate::runtime::{
    NodeState, RoundState, RunState, RuntimeAttemptLocator, RuntimeExecutionPhase, TaskState,
    WorkerRefState, validate_node_state, validate_round_state, validate_run_state,
    validate_task_state, validate_worker_ref_state, write_node_state,
};
use crate::storage::{
    GoldBandPaths, StoragePathConfig, ensure_parent_dir, load_settings_file, read_json, sqlite,
    write_json,
};
use crate::workflow_model_binding::{
    TaskAuthoringWorkflow, TaskAuthoringWorkflowCompat, WorkflowModelBindings,
    migrate_authoring_workflow, reconcile_authoring_workflow_for_save, validate_and_inject,
};
use anyhow::{Context, Result, anyhow, bail};
use camino::{Utf8Path, Utf8PathBuf};
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{Read, Seek, SeekFrom};
use std::str::FromStr;
use std::sync::{Arc, Mutex, OnceLock};

use self::ids::{generate_uuid, next_workflow_id, now_rfc3339_like, reserve_next_task_dir};
pub use self::orchestrator::ManualCheckSubmissionLease;
use self::orchestrator::{
    dynamic_state_lock_for,
    launch_prepared_run_background as orchestrator_launch_prepared_run_background,
    pause_dynamic_leaf_runtime_state, pause_dynamic_leaf_runtime_state_if_active_execution,
    prepare_dynamic_acp_prompt, prepare_run as orchestrator_prepare_run,
    prepare_run_in_worktree as orchestrator_prepare_run_in_worktree,
    prepare_run_with_authoring as orchestrator_prepare_run_with_authoring,
    reserve_manual_check_submission as orchestrator_reserve_manual_check_submission,
    run_continue as orchestrator_run_continue,
    run_continue_background as orchestrator_run_continue_background,
    run_continue_with_prompt_input as orchestrator_run_continue_with_prompt_input,
    run_recover_completed_background as orchestrator_run_recover_completed_background,
    run_retry as orchestrator_run_retry, run_start as orchestrator_run_start,
    run_start_background as orchestrator_run_start_background,
    submit_manual_check as orchestrator_submit_manual_check,
    submit_manual_check_background as orchestrator_submit_manual_check_background,
    validate_manual_check_submission as orchestrator_validate_manual_check_submission,
};

#[derive(Debug, Clone)]
pub struct ProviderDoctorProbe {
    pub doctor: DoctorResult,
    pub commands: Vec<AcpCommandItem>,
}
use self::profile_resolver::resolve_workflow_profiles;

struct OwnedTaskDirectory {
    path: Utf8PathBuf,
    armed: bool,
}

impl OwnedTaskDirectory {
    fn new(path: Utf8PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for OwnedTaskDirectory {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_dir_all(self.path.as_std_path());
        }
    }
}
use self::profiles::{
    DefaultProfileIds, create_profile, delete_profile as delete_profile_file,
    ensure_default_user_profiles, import_profiles_from_folder, list_profiles, show_profile,
    update_profile,
};

const ATTEMPT_RUNTIME_STATE_LOCK_SHARDS: usize = 64;
static ATTEMPT_RUNTIME_STATE_LOCKS: OnceLock<Vec<Mutex<()>>> = OnceLock::new();

pub(crate) fn attempt_runtime_state_lock(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
) -> &'static Mutex<()> {
    let key = format!(
        "{}/{task_id}/{run_id}/{round_id}/{node_id}/{attempt_id}",
        app.paths.repo_root
    );
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    let shard = hasher.finish() as usize % ATTEMPT_RUNTIME_STATE_LOCK_SHARDS;
    &ATTEMPT_RUNTIME_STATE_LOCKS.get_or_init(|| {
        (0..ATTEMPT_RUNTIME_STATE_LOCK_SHARDS)
            .map(|_| Mutex::new(()))
            .collect()
    })[shard]
}
pub use self::profiles::{
    ImportProfilesInput, ImportProfilesResult, ProfileCommandError, ProfileEntry, ProfileInput,
    ProfileList, ProfileScope,
};

fn tail_text(text: &str, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }
    let normalized = text.strip_suffix('\n').unwrap_or(text);
    let lines = normalized.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(limit);
    lines[start..].join("\n")
}

fn logical_artifact_name(name: &str) -> &str {
    name.strip_suffix(".json").unwrap_or(name)
}

pub(crate) fn task_inputs_dir(app: &App, task_id: &str) -> Utf8PathBuf {
    app.paths.task_dir(task_id).join("authoring").join("inputs")
}

pub(crate) fn existing_task_inputs_dir(app: &App, task_id: &str) -> Option<Utf8PathBuf> {
    let dir = task_inputs_dir(app, task_id);
    dir.exists().then_some(dir)
}

pub(crate) fn task_input_attachment_paths(app: &App, task_id: &str) -> Vec<String> {
    let inputs_dir = task_inputs_dir(app, task_id);
    if !inputs_dir.exists() {
        return Vec::new();
    }

    let mut paths = std::fs::read_dir(inputs_dir.as_std_path())
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().map(|ty| ty.is_file()).unwrap_or(false))
                .map(|entry| entry.path().to_string_lossy().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    paths.sort();
    paths
}

pub const DEFAULT_WORKFLOW_TEMPLATE_ID: &str = "default";
pub const DEFAULT_LIGHTWEIGHT_WORKFLOW_TEMPLATE_ID: &str = "default-lightweight";
const DEFAULT_WORKFLOW_MAX_ATTEMPTS: u32 = 10;
const DEFAULT_WORKFLOW_MAX_ROUNDS: u32 = 3;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OptionalEntryStage {
    pub node_id: String,
    pub label_key: String,
    pub default_enabled: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkflowTemplateCommandError {
    #[error("workflow-template.readonly-built-in")]
    ReadonlyBuiltIn,
    #[error("workflow.optional-entry.invalid")]
    InvalidOptionalEntry { reason: &'static str },
}

impl WorkflowTemplateCommandError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ReadonlyBuiltIn => "workflow-template.readonly-built-in",
            Self::InvalidOptionalEntry { .. } => "workflow.optional-entry.invalid",
        }
    }

    pub fn params(&self) -> serde_json::Value {
        match self {
            Self::ReadonlyBuiltIn => serde_json::json!({}),
            Self::InvalidOptionalEntry { reason } => serde_json::json!({ "reason": reason }),
        }
    }
}

fn default_workflow_template(
    profiles: &DefaultProfileIds,
    language: DesktopLanguage,
) -> WorkflowTemplate {
    let now = now_rfc3339_like();
    WorkflowTemplate {
        id: DEFAULT_WORKFLOW_TEMPLATE_ID.to_string(),
        name: "默认完整工作流".to_string(),
        is_built_in: true,
        optional_entry_stage: Some(OptionalEntryStage {
            node_id: "interview".to_string(),
            label_key: "conversation.home.includeInterview".to_string(),
            default_enabled: true,
        }),
        workflow: default_workflow_dsl("claude-acp", profiles, language),
        model_bindings: WorkflowModelBindings::default(),
        created_at: now.clone(),
        updated_at: now,
    }
}

fn default_lightweight_workflow_template(
    profiles: &DefaultProfileIds,
    language: DesktopLanguage,
) -> WorkflowTemplate {
    let now = now_rfc3339_like();
    WorkflowTemplate {
        id: DEFAULT_LIGHTWEIGHT_WORKFLOW_TEMPLATE_ID.to_string(),
        name: "默认轻量工作流".to_string(),
        is_built_in: true,
        optional_entry_stage: Some(OptionalEntryStage {
            node_id: "grill".to_string(),
            label_key: "conversation.home.includeGrill".to_string(),
            default_enabled: true,
        }),
        workflow: default_lightweight_workflow_dsl("claude-acp", profiles, language),
        model_bindings: WorkflowModelBindings::default(),
        created_at: now.clone(),
        updated_at: now,
    }
}

fn default_workflow_goal(language: DesktopLanguage, key: &str) -> &'static str {
    match (language, key) {
        (DesktopLanguage::ZhCn, "plan") => "分析导入的需求并产出实施方案。",
        (DesktopLanguage::ZhCn, "dev") => "在当前工作区实现需求。",
        (DesktopLanguage::ZhCn, "review") => "审查实现质量并形成明确结论。",
        (DesktopLanguage::ZhCn, "test") => "执行验证并形成明确结论。",
        (DesktopLanguage::ZhCn, "accept") => "对照需求进行验收并形成明确结论。",
        (DesktopLanguage::ZhCn, "cleanup") => "清理资源、整理交付说明并清理 Git 工作区。",
        (DesktopLanguage::ZhCn, "grill") => {
            "持续拷问需求直至达成共同理解，并产出 grill-consensus.md。"
        }
        (DesktopLanguage::ZhCn, "dev-test") => "在当前工作区完成需求实现、自动化测试和必要回归。",
        (DesktopLanguage::En, "plan") => {
            "Analyze the imported requirement and produce an implementation plan."
        }
        (DesktopLanguage::En, "dev") => "Implement the requirement in the workspace.",
        (DesktopLanguage::En, "review") => {
            "Review the implementation and reach a clear conclusion."
        }
        (DesktopLanguage::En, "test") => "Run verification and reach a clear conclusion.",
        (DesktopLanguage::En, "accept") => {
            "Validate acceptance against the requirement and reach a clear conclusion."
        }
        (DesktopLanguage::En, "cleanup") => {
            "Clean up resources, finalize handoff notes, and clean up the Git workspace."
        }
        (DesktopLanguage::En, "grill") => {
            "Challenge the requirement until shared understanding is reached and produce grill-consensus.md."
        }
        (DesktopLanguage::En, "dev-test") => {
            "Implement the requirement and run automated verification in the current workspace."
        }
        _ => "Execute this workflow node.",
    }
}

fn default_workflow_dsl(
    provider: &str,
    profiles: &DefaultProfileIds,
    language: DesktopLanguage,
) -> WorkflowDsl {
    fn worker(
        _provider: &str,
        profiles: &DefaultProfileIds,
        id: &str,
        role_key: &str,
        goal: &str,
        validation: bool,
        manual_check: bool,
    ) -> NodeDsl {
        let artifact = validation.then(|| format!("{id}-result"));
        NodeDsl::Worker(WorkerNode {
            id: id.to_string(),
            execution_slot_id: None,
            provider: None,
            model: None,
            profile: Some(
                profiles
                    .get(role_key)
                    .expect("default role id exists")
                    .to_string(),
            ),
            goal: Some(goal.to_string()),
            output: artifact.clone().map(|artifact| OutputContractDsl {
                kind: OutputKind::Json,
                artifact,
                schema: Some(serde_json::json!({
                    "reason": "String",
                    "result": "boolean",
                })),
            }),
            success_condition: validation.then(|| JsonConditionDsl::Expression {
                expression: "$.result == true".to_string(),
            }),
            permission_mode: None,
            config_options: Default::default(),
            manual_check: manual_check.then_some(true),
            prompt_envelope: crate::dsl::PromptEnvelopeMode::RuntimeManaged,
        })
    }

    WorkflowDsl {
        version: "0.1".to_string(),
        id: "task-workflow".to_string(),
        entry: "interview".to_string(),
        control: WorkflowControl {
            max_attempts: Some(DEFAULT_WORKFLOW_MAX_ATTEMPTS),
            max_rounds: Some(DEFAULT_WORKFLOW_MAX_ROUNDS),
        },
        nodes: vec![
            worker(
                provider,
                profiles,
                "interview",
                "interview",
                "Conduct a deep interview to clarify the requirement and produce a clear specification.",
                false,
                true,
            ),
            worker(
                provider,
                profiles,
                "plan",
                "plan",
                default_workflow_goal(language, "plan"),
                false,
                true,
            ),
            worker(
                provider,
                profiles,
                "dev",
                "dev",
                default_workflow_goal(language, "dev"),
                false,
                false,
            ),
            worker(
                provider,
                profiles,
                "review",
                "review",
                default_workflow_goal(language, "review"),
                true,
                false,
            ),
            worker(
                provider,
                profiles,
                "test",
                "test",
                default_workflow_goal(language, "test"),
                true,
                false,
            ),
            worker(
                provider,
                profiles,
                "accept",
                "accept",
                default_workflow_goal(language, "accept"),
                true,
                false,
            ),
            worker(
                provider,
                profiles,
                "cleanup",
                "cleanup",
                default_workflow_goal(language, "cleanup"),
                false,
                false,
            ),
        ],
        edges: vec![
            EdgeDsl {
                from: "interview".to_string(),
                to: "plan".to_string(),
                on: EdgeOutcome::Success,
                session: None,
                new_round_entry: None,
            },
            EdgeDsl {
                from: "plan".to_string(),
                to: "dev".to_string(),
                on: EdgeOutcome::Success,
                session: None,
                new_round_entry: None,
            },
            EdgeDsl {
                from: "dev".to_string(),
                to: "review".to_string(),
                on: EdgeOutcome::Success,
                session: None,
                new_round_entry: None,
            },
            EdgeDsl {
                from: "review".to_string(),
                to: "test".to_string(),
                on: EdgeOutcome::Success,
                session: None,
                new_round_entry: None,
            },
            EdgeDsl {
                from: "review".to_string(),
                to: "dev".to_string(),
                on: EdgeOutcome::Failure,
                session: Some(SessionMode::Continue),
                new_round_entry: None,
            },
            EdgeDsl {
                from: "test".to_string(),
                to: "accept".to_string(),
                on: EdgeOutcome::Success,
                session: None,
                new_round_entry: None,
            },
            EdgeDsl {
                from: "test".to_string(),
                to: "dev".to_string(),
                on: EdgeOutcome::Failure,
                session: Some(SessionMode::Continue),
                new_round_entry: None,
            },
            EdgeDsl {
                from: "accept".to_string(),
                to: "cleanup".to_string(),
                on: EdgeOutcome::Success,
                session: None,
                new_round_entry: None,
            },
            EdgeDsl {
                from: "cleanup".to_string(),
                to: END_NODE.to_string(),
                on: EdgeOutcome::Success,
                session: None,
                new_round_entry: None,
            },
            EdgeDsl {
                from: "accept".to_string(),
                to: NEW_ROUND_NODE.to_string(),
                on: EdgeOutcome::Failure,
                session: None,
                new_round_entry: Some("dev".to_string()),
            },
        ],
    }
}

fn default_lightweight_workflow_dsl(
    provider: &str,
    profiles: &DefaultProfileIds,
    language: DesktopLanguage,
) -> WorkflowDsl {
    fn worker(
        _provider: &str,
        profiles: &DefaultProfileIds,
        id: &str,
        role_key: &str,
        goal: &str,
        validation: bool,
        manual_check: bool,
    ) -> NodeDsl {
        let artifact = validation.then(|| format!("{id}-result"));
        NodeDsl::Worker(WorkerNode {
            id: id.to_string(),
            execution_slot_id: None,
            provider: None,
            model: None,
            profile: Some(
                profiles
                    .get(role_key)
                    .expect("default role id exists")
                    .to_string(),
            ),
            goal: Some(goal.to_string()),
            output: artifact.clone().map(|artifact| OutputContractDsl {
                kind: OutputKind::Json,
                artifact,
                schema: Some(serde_json::json!({
                    "reason": "String",
                    "result": "boolean",
                })),
            }),
            success_condition: validation.then(|| JsonConditionDsl::Expression {
                expression: "$.result == true".to_string(),
            }),
            permission_mode: None,
            config_options: Default::default(),
            manual_check: manual_check.then_some(true),
            prompt_envelope: crate::dsl::PromptEnvelopeMode::RuntimeManaged,
        })
    }

    WorkflowDsl {
        version: "0.1".to_string(),
        id: "task-workflow-lightweight".to_string(),
        entry: "grill".to_string(),
        control: WorkflowControl {
            max_attempts: Some(DEFAULT_WORKFLOW_MAX_ATTEMPTS),
            max_rounds: Some(DEFAULT_WORKFLOW_MAX_ROUNDS),
        },
        nodes: vec![
            worker(
                provider,
                profiles,
                "grill",
                "grill",
                default_workflow_goal(language, "grill"),
                false,
                true,
            ),
            worker(
                provider,
                profiles,
                "dev-test",
                "dev-test",
                default_workflow_goal(language, "dev-test"),
                false,
                false,
            ),
            worker(
                provider,
                profiles,
                "accept",
                "accept",
                default_workflow_goal(language, "accept"),
                true,
                false,
            ),
        ],
        edges: vec![
            EdgeDsl {
                from: "grill".to_string(),
                to: "dev-test".to_string(),
                on: EdgeOutcome::Success,
                session: None,
                new_round_entry: None,
            },
            EdgeDsl {
                from: "dev-test".to_string(),
                to: "accept".to_string(),
                on: EdgeOutcome::Success,
                session: None,
                new_round_entry: None,
            },
            EdgeDsl {
                from: "accept".to_string(),
                to: END_NODE.to_string(),
                on: EdgeOutcome::Success,
                session: None,
                new_round_entry: None,
            },
            EdgeDsl {
                from: "accept".to_string(),
                to: NEW_ROUND_NODE.to_string(),
                on: EdgeOutcome::Failure,
                session: None,
                new_round_entry: Some("dev-test".to_string()),
            },
        ],
    }
}

fn unique_workflow_template_id(store: &WorkflowTemplateStore, name: &str) -> String {
    let slug = name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let base = if slug.is_empty() {
        "workflow".to_string()
    } else {
        slug
    };
    let mut candidate = base.clone();
    let mut index = 1;
    while store
        .templates
        .iter()
        .any(|template| template.id == candidate)
    {
        index += 1;
        candidate = format!("{base}-{index}");
    }
    candidate
}

fn upsert_built_in_workflow_template(
    templates: &mut Vec<WorkflowTemplate>,
    built_in: WorkflowTemplate,
    index: usize,
) -> Result<()> {
    if let Some(current_index) = templates
        .iter()
        .position(|template| template.id == built_in.id)
    {
        let mut next = built_in;
        let persisted = TaskAuthoringWorkflow {
            workflow: templates[current_index].workflow.clone(),
            model_bindings: templates[current_index].model_bindings.clone(),
        };
        next.model_bindings = persisted.model_bindings.clone();
        reconcile_authoring_workflow_for_save(
            &mut next.workflow,
            &mut next.model_bindings,
            Some(&persisted),
            Some(&next.id),
        )?;
        templates[current_index] = next;
        if current_index != index {
            let template = templates.remove(current_index);
            templates.insert(index.min(templates.len()), template);
        }
    } else {
        let mut built_in = built_in;
        reconcile_authoring_workflow_for_save(
            &mut built_in.workflow,
            &mut built_in.model_bindings,
            None,
            Some(&built_in.id),
        )?;
        templates.insert(index.min(templates.len()), built_in);
    }
    Ok(())
}

pub fn apply_optional_entry_preference(
    template: &WorkflowTemplate,
    include_optional_entry: Option<bool>,
    workflow: &mut WorkflowDsl,
) -> Result<Option<bool>> {
    let Some(stage) = template.optional_entry_stage.as_ref() else {
        return Ok(None);
    };
    if !template.is_built_in {
        return Err(WorkflowTemplateCommandError::InvalidOptionalEntry {
            reason: "requires-built-in-template",
        }
        .into());
    }
    if workflow.entry != stage.node_id {
        return Err(WorkflowTemplateCommandError::InvalidOptionalEntry {
            reason: "must-be-workflow-entry",
        }
        .into());
    }
    if !workflow.nodes.iter().any(|node| node.id() == stage.node_id) {
        return Err(WorkflowTemplateCommandError::InvalidOptionalEntry {
            reason: "entry-node-missing",
        }
        .into());
    }
    let mut successors = workflow
        .edges
        .iter()
        .filter(|edge| edge.from == stage.node_id && edge.on == EdgeOutcome::Success)
        .map(|edge| edge.to.as_str());
    let next_entry =
        successors
            .next()
            .ok_or(WorkflowTemplateCommandError::InvalidOptionalEntry {
                reason: "missing-success-successor",
            })?;
    if successors.next().is_some() {
        return Err(WorkflowTemplateCommandError::InvalidOptionalEntry {
            reason: "multiple-success-successors",
        }
        .into());
    }
    if next_entry == END_NODE || next_entry == NEW_ROUND_NODE {
        return Err(WorkflowTemplateCommandError::InvalidOptionalEntry {
            reason: "successor-must-be-real-node",
        }
        .into());
    }
    if !workflow.nodes.iter().any(|node| node.id() == next_entry) {
        return Err(WorkflowTemplateCommandError::InvalidOptionalEntry {
            reason: "successor-node-missing",
        }
        .into());
    }

    let include = include_optional_entry.unwrap_or(stage.default_enabled);
    if !include {
        let next_entry = next_entry.to_string();
        workflow.nodes.retain(|node| node.id() != stage.node_id);
        workflow
            .edges
            .retain(|edge| edge.from != stage.node_id && edge.to != stage.node_id);
        workflow.entry = next_entry;
    }
    Ok(Some(include))
}

fn next_auto_template_id(store: &AutoTemplateStore) -> String {
    loop {
        let candidate = format!("auto-template-{}", generate_uuid());
        if !store
            .templates
            .iter()
            .any(|template| template.id == candidate)
        {
            return candidate;
        }
    }
}

#[derive(Debug, Clone)]
pub struct CreateTaskInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub requirement_file_name: Option<String>,
    pub requirement_content: String,
    pub workflow: WorkflowDsl,
    pub workflow_template_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowTemplateStore {
    pub version: String,
    #[serde(alias = "last_used_template_id")]
    pub last_used_template_id: Option<String>,
    #[serde(alias = "last_created_workflow")]
    pub last_created_workflow: Option<WorkflowDsl>,
    pub templates: Vec<WorkflowTemplate>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowTemplate {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub is_built_in: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optional_entry_stage: Option<OptionalEntryStage>,
    pub workflow: WorkflowDsl,
    #[serde(default)]
    pub model_bindings: WorkflowModelBindings,
    #[serde(alias = "created_at")]
    pub created_at: String,
    #[serde(alias = "updated_at")]
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoTemplateStore {
    pub version: String,
    pub templates: Vec<AutoTemplate>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoTemplate {
    pub id: String,
    pub name: String,
    pub config: ConversationAutoConfig,
    #[serde(default, alias = "created_at")]
    pub created_at: String,
    #[serde(default, alias = "updated_at")]
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskSummary {
    pub task: TaskState,
    pub workflow_exists: bool,
    pub workflow_valid: bool,
    pub workflow_error: Option<String>,
    pub workflow_validation_error: Option<WorkflowValidationError>,
    pub latest_run: Option<RunState>,
    pub resumable_run_id: Option<String>,
    pub suggested_run_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogSource {
    ProgressEvents,
    RawStream,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NodeEdgeSummary {
    pub to: String,
    pub on: EdgeOutcome,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NodeRuntimeSummary {
    pub latest_attempt: Option<NodeState>,
    pub attempts: Vec<NodeState>,
    pub outgoing_edges: Vec<NodeEdgeSummary>,
}

/// Runtime lifecycle events emitted by the orchestrator via RuntimeLifecycleBus.
/// Subscribers observe these facts without changing runtime control flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeInterventionKind {
    ManualDecisionRequired,
    ElicitationRequested,
    PermissionRequested,
    RuntimeAbnormal,
    ErrorBlocked,
    ProcessInterrupted,
}

impl From<PauseReason> for RuntimeInterventionKind {
    fn from(reason: PauseReason) -> Self {
        match reason {
            PauseReason::WaitingForUserInput => Self::ManualDecisionRequired,
            PauseReason::PermissionRequested => Self::PermissionRequested,
            PauseReason::RuntimeAbnormal => Self::RuntimeAbnormal,
            PauseReason::ErrorBlocked => Self::ErrorBlocked,
            PauseReason::ProcessInterrupted => Self::ProcessInterrupted,
        }
    }
}

impl From<RuntimeInterventionKind> for PauseReason {
    fn from(kind: RuntimeInterventionKind) -> Self {
        match kind {
            RuntimeInterventionKind::ManualDecisionRequired => Self::WaitingForUserInput,
            RuntimeInterventionKind::ElicitationRequested => Self::WaitingForUserInput,
            RuntimeInterventionKind::PermissionRequested => Self::PermissionRequested,
            RuntimeInterventionKind::RuntimeAbnormal => Self::RuntimeAbnormal,
            RuntimeInterventionKind::ErrorBlocked => Self::ErrorBlocked,
            RuntimeInterventionKind::ProcessInterrupted => Self::ProcessInterrupted,
        }
    }
}

/// ACP 单次 prompt turn 的终态。该状态独立于 workflow run/node 终态：
/// 手动追问完成后 run 可能早已结束，但 turn 仍需要稳定的完成、失败与停止语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpTurnOutcome {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcpTurnBatchProgress {
    pub completed_reply_count: u32,
    pub continues: bool,
}

impl AcpTurnBatchProgress {
    pub const fn terminal(completed_reply_count: u32) -> Self {
        Self {
            completed_reply_count,
            continues: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActiveMetricTurn {
    pub execution_id: String,
    pub attempt_id: String,
    pub attempt_index: u32,
    pub usage_baseline_turn_seq: u64,
}

impl ActiveMetricTurn {
    pub fn new(
        execution_id: String,
        attempt_id: String,
        attempt_index: u32,
        usage_baseline_turn_seq: u64,
    ) -> Self {
        Self {
            execution_id,
            attempt_id,
            attempt_index,
            usage_baseline_turn_seq,
        }
    }
}

#[derive(Debug, Clone)]
pub enum RuntimeLifecycleEvent {
    /// The desktop process has reached the point where lifecycle subscribers
    /// are registered and may evaluate startup reporting configuration.
    ApplicationStarted,
    /// A real user activity signal was observed by the desktop client.
    UserActivityObserved,
    /// A user-triggered top-level Conversation run was accepted and received
    /// a new canonical run id. Follow-ups, continue, same-run retry, scheduled
    /// execution, and AUTO child runs do not emit this fact.
    ConversationRunStarted {
        project_id: String,
        task_id: String,
        run_id: String,
        run_mode: crate::config::ConversationRunMode,
    },
    /// A scheduled-task definition and its input snapshot were durably created.
    ScheduledTaskCreated {
        project_id: String,
        scheduled_task_id: String,
    },
    MetricsFact(observability::MetricsLifecycleFact),
    /// A node has started executing. The orchestrator is about to invoke the
    /// AI provider. `predecessor` carries the previous node's snapshot.
    NodeStarted {
        // ── IDs (display + UUID) ──
        task_id: String,
        task_uuid: Option<String>,
        run_id: String,
        run_uuid: Option<String>,
        round_id: String,
        round_uuid: Option<String>,
        round_index: Option<u32>,
        node_id: String,
        node_uuid: Option<String>,
        attempt_id: String,
        // ── Metadata ──
        repo_root: String,
        seq: Option<u32>,
        node_name: Option<String>,
        agent_type: Option<String>,
        resolved_model: Option<String>,
        started_at: String,
        /// Path to the current node's attempt directory. `None` because the
        /// node just started — attempt dir hasn't been populated yet.
        attempt_dir: Option<String>,
        /// The immediately preceding node in this run (None for first node).
        predecessor: Option<crate::runtime::LastExecutedNode>,
        metrics_unit_kind: Option<crate::dynamic::DynamicNodeKind>,
        child_run_id: Option<String>,
    },
    /// A node has completed execution (the AI provider returned). The
    /// orchestrator has already persisted runtime state.
    NodeCompleted {
        // ── IDs (display + UUID) ──
        task_id: String,
        task_uuid: Option<String>,
        run_id: String,
        run_uuid: Option<String>,
        round_id: String,
        round_uuid: Option<String>,
        round_index: Option<u32>,
        node_id: String,
        node_uuid: Option<String>,
        attempt_id: String,
        // ── Metadata ──
        repo_root: String,
        seq: Option<u32>,
        node_name: String,
        agent_type: Option<String>,
        resolved_model: Option<String>,
        started_at: String,
        finished_at: Option<String>,
        outcome: String, // "SUCCESS" | "FAILED"
        /// Path to this node's attempt directory for reading token data.
        attempt_dir: String,
        /// When true, the subscriber skips the 「结束」sentinel. Used by
        /// dynamic workers so that only the outer AiDynamic node produces
        /// the single begin/end sentinel pair for the whole workflow.
        suppress_sentinel: bool,
        metrics_unit_kind: Option<crate::dynamic::DynamicNodeKind>,
        child_run_id: Option<String>,
    },
    RunPaused {
        event_id: String,
        occurred_at: String,
        scheduled_occurrence_id: Option<String>,
        project_id: String,
        task_id: String,
        run_id: String,
        round_id: String,
        node_id: String,
        attempt_id: String,
        node_label: String,
        pause_reason: PauseReason,
        task_title: Option<String>,
    },
    InterventionRequested {
        event_id: String,
        occurred_at: String,
        scheduled_occurrence_id: Option<String>,
        project_id: String,
        task_id: String,
        run_id: String,
        round_id: String,
        node_id: String,
        attempt_id: String,
        node_label: String,
        kind: RuntimeInterventionKind,
        task_title: Option<String>,
    },
    RunCompleted {
        event_id: String,
        occurred_at: String,
        scheduled_occurrence_id: Option<String>,
        project_id: String,
        task_id: String,
        run_id: String,
        round_id: String,
        node_id: String,
        attempt_id: String,
        node_label: String,
        outcome: RunOutcome,
        task_title: Option<String>,
        /// Direct 首轮以 Agent 回复语义展示；普通 Workflow/AUTO 为 None。
        completion_agent_label: Option<String>,
    },
    /// 非 Runtime 控制的 ACP prompt turn 已结束。
    ///
    /// Direct 后续对话以及 Workflow/AUTO 节点完成后的手动追问统一走该事件；
    /// runtime 自身继续执行仍由 RunCompleted/InterventionRequested 表达，避免双重通知。
    AcpTurnFinished {
        event_id: String,
        occurred_at: String,
        scheduled_occurrence_id: Option<String>,
        project_id: String,
        task_id: String,
        run_id: String,
        round_id: String,
        node_id: String,
        attempt_id: String,
        turn_id: String,
        agent_label: String,
        outcome: AcpTurnOutcome,
        /// Progress of the uninterrupted Direct prompt-queue reply batch.
        /// Intermediate successes remain observable with `continues=true`; the
        /// terminal event carries the total count used by the desktop notification.
        batch_progress: AcpTurnBatchProgress,
        task_title: Option<String>,
    },
}

fn lifecycle_event_kind(event: &RuntimeLifecycleEvent) -> &'static str {
    match event {
        RuntimeLifecycleEvent::ApplicationStarted => "application-started",
        RuntimeLifecycleEvent::UserActivityObserved => "user-activity-observed",
        RuntimeLifecycleEvent::ConversationRunStarted { .. } => "conversation-run-started",
        RuntimeLifecycleEvent::ScheduledTaskCreated { .. } => "scheduled-task-created",
        RuntimeLifecycleEvent::MetricsFact(_) => "metrics-fact",
        RuntimeLifecycleEvent::NodeStarted { .. } => "node-started",
        RuntimeLifecycleEvent::NodeCompleted { .. } => "node-completed",
        RuntimeLifecycleEvent::RunPaused { .. } => "run-paused",
        RuntimeLifecycleEvent::InterventionRequested { .. } => "intervention-requested",
        RuntimeLifecycleEvent::RunCompleted { .. } => "run-completed",
        RuntimeLifecycleEvent::AcpTurnFinished { .. } => "acp-turn-finished",
    }
}

impl RuntimeLifecycleEvent {
    fn set_scheduled_occurrence_id(&mut self, occurrence_id: Option<String>) {
        match self {
            Self::RunPaused {
                scheduled_occurrence_id,
                ..
            }
            | Self::InterventionRequested {
                scheduled_occurrence_id,
                ..
            }
            | Self::RunCompleted {
                scheduled_occurrence_id,
                ..
            }
            | Self::AcpTurnFinished {
                scheduled_occurrence_id,
                ..
            } => {
                *scheduled_occurrence_id = occurrence_id;
            }
            Self::ApplicationStarted
            | Self::UserActivityObserved
            | Self::ConversationRunStarted { .. }
            | Self::ScheduledTaskCreated { .. }
            | Self::NodeStarted { .. }
            | Self::NodeCompleted { .. }
            | Self::MetricsFact(_) => {}
        }
    }
}

pub struct App {
    pub paths: GoldBandPaths,
    pub config: RuntimeConfig,
    task_search_indexer: Arc<dyn Fn(&Utf8Path, &str) + Send + Sync>,
    provider_override: Option<Arc<dyn ProviderAdapter>>,
    provider_diagnostics:
        Option<Arc<dyn Fn() -> Result<BTreeMap<String, ProviderDiagnosticSnapshot>> + Send + Sync>>,
    acp_live_update: Option<
        Arc<
            dyn Fn(
                    AcpLiveEventContext,
                    crate::acp::events::AcpUiEvent,
                    Option<(u64, u64)>,
                ) -> Result<()>
                + Send
                + Sync,
        >,
    >,
    acp_session_update: Option<Arc<dyn Fn(AcpLiveEventContext) -> Result<()> + Send + Sync>>,
    prompt_turn_lifecycle: Option<
        Arc<dyn Fn(AcpLiveEventContext, AcpPromptLifecycleEvent) -> Result<()> + Send + Sync>,
    >,
    pub lifecycle_bus: observability::RuntimeLifecycleBus,
    observability_states:
        Arc<std::sync::Mutex<HashMap<String, observability::ExecutionObservabilityState>>>,
    metrics_collection_enabled: bool,
    active_metric_turns: Arc<std::sync::Mutex<HashMap<String, ActiveMetricTurn>>>,
    scheduled_occurrence_id: Option<String>,
    scheduled_task_context: Option<crate::provider::ScheduledTaskContextInfo>,
    runtime_recovery: Option<Arc<RuntimeRecoveryCoordinator>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttemptRuntimePauseResult {
    Converged,
    Superseded,
}

#[derive(Debug, Clone, Copy)]
enum AttemptRuntimePausePolicy<'a> {
    CurrentAttempt,
    ActiveExecution(&'a str),
    ActiveAttemptWithoutExecution,
    PausedManualCheck,
}

fn default_task_search_indexer() -> Arc<dyn Fn(&Utf8Path, &str) + Send + Sync> {
    Arc::new(sqlite::index_task_with_retry)
}

#[derive(Debug, Clone)]
pub struct AcpLiveEventContext {
    pub task_id: String,
    pub run_id: String,
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
    pub outer_node_id: Option<String>,
    pub outer_attempt_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpPromptLifecycleEvent {
    Accepted {
        prompt_id: String,
    },
    Finished {
        prompt_id: Option<String>,
        successful: bool,
    },
}

pub fn is_run_continuable(run: &RunState) -> bool {
    run.status == RunStatus::Paused
        && run.outcome.is_none()
        && matches!(
            run.pause_reason,
            Some(PauseReason::ProcessInterrupted | PauseReason::RuntimeAbnormal)
        )
        && run.current_round.is_some()
        && run.current_node.is_some()
        && run.current_attempt.is_some()
}

pub fn is_completed_attempt_recoverable(run: &RunState, node: &NodeState) -> bool {
    is_run_continuable(run)
        && run.current_round.as_deref() == Some(node.round_id.as_str())
        && run.current_node.as_deref() == Some(node.node_id.as_str())
        && run.current_attempt.as_deref() == Some(node.attempt_id.as_str())
        && node.status == RunStatus::Completed
        && node.outcome == Some(NodeOutcome::Success)
        && !node.manual_check_pending
}

#[derive(Debug, Default, Clone, Copy)]
struct ProfileUsageCounts {
    template_count: usize,
    task_count: usize,
    run_count: usize,
}

fn duplicate_workflow_id_error(
    workflow_name: &str,
    workflow_id: &str,
    conflicts: Vec<String>,
) -> Result<()> {
    if conflicts.is_empty() {
        return Ok(());
    }
    Err(WorkflowValidationError::DuplicateWorkflowId {
        workflow_name: workflow_name.to_string(),
        workflow_id: workflow_id.to_string(),
        conflicts: conflicts.join(", "),
    }
    .into())
}

fn validate_unique_workflow_template_id(
    store: &WorkflowTemplateStore,
    workflow: &WorkflowDsl,
    workflow_name: &str,
    exclude_template_id: Option<&str>,
) -> Result<()> {
    let workflow_id = workflow.id.trim();
    let conflicts = store
        .templates
        .iter()
        .filter(|template| exclude_template_id != Some(template.id.as_str()))
        .filter(|template| template.workflow.id.trim() == workflow_id)
        .map(|template| template.name.clone())
        .collect::<Vec<_>>();
    duplicate_workflow_id_error(workflow_name, workflow_id, conflicts)
}

fn validate_ai_dynamic_allowed_workflows(
    workflow: &WorkflowDsl,
    store: &WorkflowTemplateStore,
) -> Result<()> {
    for node in &workflow.nodes {
        let NodeDsl::AiDynamic(dynamic) = node else {
            continue;
        };
        for allowed in &dynamic.allowed_workflows {
            let workflow_id = allowed.workflow_id.trim();
            let template = store
                .templates
                .iter()
                .find(|template| template.workflow.id.trim() == workflow_id)
                .ok_or_else(|| {
                    anyhow!(
                        "ai-dynamic node `{}` allowed workflow `{workflow_id}` not found",
                        dynamic.id
                    )
                })?;
            if let Err(error) = validate_unique_workflow_template_id(
                store,
                &template.workflow,
                &template.name,
                Some(&template.id),
            ) {
                return Err(WorkflowValidationError::AiDynamicInvalidWorkflow {
                    node_id: dynamic.id.clone(),
                    workflow_name: template.name.clone(),
                    reason: error.to_string(),
                }
                .into());
            }
            let validated =
                validate_authoring_workflow(template.workflow.clone()).map_err(|error| {
                    WorkflowValidationError::AiDynamicInvalidWorkflow {
                        node_id: dynamic.id.clone(),
                        workflow_name: template.name.clone(),
                        reason: error.to_string(),
                    }
                })?;
            if !dynamic.control.allow_nested_dynamic && workflow_contains_ai_dynamic(&validated.raw)
            {
                return Err(WorkflowValidationError::AiDynamicInvalidWorkflow {
                    node_id: dynamic.id.clone(),
                    workflow_name: template.name.clone(),
                    reason: format!("workflow `{workflow_id}` contains AI-DYNAMIC"),
                }
                .into());
            }
        }
    }
    Ok(())
}

fn workflow_uses_profile(workflow: &WorkflowDsl, profile_id: &str) -> bool {
    workflow.nodes.iter().any(|node| match node {
        NodeDsl::Worker(worker) => worker.profile.as_deref() == Some(profile_id),
        NodeDsl::AiDynamic(_) => false,
    })
}

fn providers_for_node(node: &NodeDsl) -> Vec<String> {
    match node {
        NodeDsl::Worker(worker) => worker.provider.iter().cloned().collect(),
        NodeDsl::AiDynamic(dynamic) => match &dynamic.agent_strategy {
            AiDynamicAgentStrategy::Fixed { provider, .. } => vec![provider.clone()],
            AiDynamicAgentStrategy::Dynamic {
                bootstrap_provider,
                available_agents,
                ..
            } => {
                let mut providers = vec![bootstrap_provider.clone()];
                for agent_ref in available_agents {
                    if !providers.contains(&agent_ref.provider) {
                        providers.push(agent_ref.provider.clone());
                    }
                }
                providers
            }
        },
    }
}

fn configured_permission_modes_for_node(node: &NodeDsl) -> Vec<(String, Option<String>)> {
    match node {
        NodeDsl::Worker(worker) => worker
            .provider
            .as_ref()
            .map(|provider| (provider.clone(), worker.permission_mode.clone()))
            .into_iter()
            .collect(),
        NodeDsl::AiDynamic(dynamic) => match &dynamic.agent_strategy {
            AiDynamicAgentStrategy::Fixed {
                provider,
                permission_mode,
                ..
            } => vec![(provider.clone(), permission_mode.clone())],
            AiDynamicAgentStrategy::Dynamic {
                bootstrap_provider,
                permission_mode,
                available_agents,
                ..
            } => std::iter::once((bootstrap_provider.clone(), permission_mode.clone()))
                .chain(
                    available_agents
                        .iter()
                        .map(|agent| (agent.provider.clone(), agent.permission_mode.clone())),
                )
                .collect(),
        },
    }
}

#[derive(Debug, Clone)]
pub struct PreparedAcpPrompt {
    pub prompt: PromptBundle,
    pub adapter_workspace_dir: Utf8PathBuf,
    pub session_workspace_dir: Utf8PathBuf,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn transition_runtime_execution_phase(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        outer_node_id: &str,
        outer_attempt_id: &str,
        phase: RuntimeExecutionPhase,
    ) -> Result<RunState> {
        let state_lock = attempt_runtime_state_lock(
            self,
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
        );
        let _guard = state_lock
            .lock()
            .map_err(|_| anyhow!("attempt runtime state lock poisoned"))?;
        let mut run = self.run_status(task_id, run_id)?;
        if run.status != RunStatus::Running
            || run.current_round.as_deref() != Some(round_id)
            || run.current_node.as_deref() != Some(outer_node_id)
            || run.current_attempt.as_deref() != Some(outer_attempt_id)
        {
            bail!("runtime execution locator is no longer current");
        }
        let locator = RuntimeAttemptLocator {
            round_id: round_id.to_string(),
            node_id: outer_node_id.to_string(),
            attempt_id: outer_attempt_id.to_string(),
            outer_node_id: None,
            outer_attempt_id: None,
        };
        let phase = if phase == RuntimeExecutionPhase::RunningNode
            && matches!(
                run.execution.phase,
                RuntimeExecutionPhase::FinalizingArtifact
                    | RuntimeExecutionPhase::RepairingArtifact
            ) {
            run.execution.phase
        } else {
            phase
        };
        run.updated_at = now_rfc3339_like();
        run.transition_execution(phase, Some(locator), run.updated_at.clone())?;
        validate_run_state(&run)?;
        write_json(&self.paths.run_file(task_id, run_id), &run)?;
        Ok(run)
    }

    pub fn new(repo_root: Utf8PathBuf) -> Self {
        Self::with_config(repo_root, RuntimeConfig::default())
    }

    pub fn clone_for_background(&self) -> Self {
        Self {
            paths: self.paths.clone(),
            config: self.config.clone(),
            task_search_indexer: self.task_search_indexer.clone(),
            provider_override: self.provider_override.clone(),
            provider_diagnostics: self.provider_diagnostics.clone(),
            acp_live_update: self.acp_live_update.clone(),
            acp_session_update: self.acp_session_update.clone(),
            prompt_turn_lifecycle: self.prompt_turn_lifecycle.clone(),
            lifecycle_bus: self.lifecycle_bus.clone(),
            observability_states: self.observability_states.clone(),
            metrics_collection_enabled: self.metrics_collection_enabled,
            active_metric_turns: self.active_metric_turns.clone(),
            scheduled_occurrence_id: self.scheduled_occurrence_id.clone(),
            scheduled_task_context: self.scheduled_task_context.clone(),
            runtime_recovery: self.runtime_recovery.clone(),
        }
    }

    pub fn with_provider_diagnostics_source(
        mut self,
        provider_diagnostics: Arc<
            dyn Fn() -> Result<BTreeMap<String, ProviderDiagnosticSnapshot>> + Send + Sync,
        >,
    ) -> Self {
        self.provider_diagnostics = Some(provider_diagnostics);
        self
    }

    pub fn provider_diagnostics(&self) -> BTreeMap<String, ProviderDiagnosticSnapshot> {
        if let Some(provider_diagnostics) = self
            .provider_diagnostics
            .as_ref()
            .and_then(|provider_diagnostics| provider_diagnostics().ok())
            .filter(|diagnostics| !diagnostics.is_empty())
        {
            return provider_diagnostics;
        }
        if let Ok(provider_diagnostics) = read_json::<BTreeMap<String, ProviderDiagnosticSnapshot>>(
            &self.paths.agent_diagnostics_file(),
        ) && !provider_diagnostics.is_empty()
        {
            return provider_diagnostics;
        }
        self.config.provider_diagnostics.clone()
    }

    pub fn with_repo_root(&self, repo_root: Utf8PathBuf, config: RuntimeConfig) -> Self {
        let paths = GoldBandPaths::new(repo_root);
        let _ = ensure_default_user_profiles(&paths);
        Self {
            paths,
            config,
            task_search_indexer: self.task_search_indexer.clone(),
            provider_override: self.provider_override.clone(),
            provider_diagnostics: self.provider_diagnostics.clone(),
            acp_live_update: self.acp_live_update.clone(),
            acp_session_update: self.acp_session_update.clone(),
            prompt_turn_lifecycle: self.prompt_turn_lifecycle.clone(),
            lifecycle_bus: self.lifecycle_bus.clone(),
            observability_states: self.observability_states.clone(),
            metrics_collection_enabled: self.metrics_collection_enabled,
            active_metric_turns: self.active_metric_turns.clone(),
            scheduled_occurrence_id: self.scheduled_occurrence_id.clone(),
            scheduled_task_context: self.scheduled_task_context.clone(),
            runtime_recovery: self.runtime_recovery.clone(),
        }
    }

    pub fn with_runtime_recovery(
        mut self,
        runtime_recovery: Arc<RuntimeRecoveryCoordinator>,
    ) -> Self {
        self.runtime_recovery = Some(runtime_recovery);
        self
    }

    pub(crate) fn begin_runtime_candidate(
        &self,
        task_id: &str,
        run_id: &str,
    ) -> Result<Option<RuntimeCandidateRegistration>> {
        self.runtime_recovery
            .as_ref()
            .map(|coordinator| {
                coordinator
                    .begin(&self.paths, task_id, run_id)
                    .map_err(Into::into)
            })
            .transpose()
    }

    pub(crate) fn finish_runtime_candidate_best_effort(
        &self,
        task_id: &str,
        run_id: &str,
        candidate_token: Option<&str>,
    ) {
        let Some(coordinator) = &self.runtime_recovery else {
            return;
        };
        if let Err(error) = coordinator.finish(&self.paths, task_id, run_id, candidate_token) {
            tracing::warn!(
                error = %error,
                project_id = %self.paths.project_id,
                task_id,
                run_id,
                "runtime recovery candidate cleanup failed"
            );
        }
    }

    pub fn with_acp_live_update(
        mut self,
        live_update: Arc<
            dyn Fn(
                    AcpLiveEventContext,
                    crate::acp::events::AcpUiEvent,
                    Option<(u64, u64)>,
                ) -> Result<()>
                + Send
                + Sync,
        >,
    ) -> Self {
        self.acp_live_update = Some(live_update);
        self
    }

    pub fn with_acp_session_update(
        mut self,
        session_update: Arc<dyn Fn(AcpLiveEventContext) -> Result<()> + Send + Sync>,
    ) -> Self {
        self.acp_session_update = Some(session_update);
        self
    }

    pub fn with_prompt_turn_lifecycle(
        mut self,
        callback: Arc<
            dyn Fn(AcpLiveEventContext, AcpPromptLifecycleEvent) -> Result<()> + Send + Sync,
        >,
    ) -> Self {
        self.prompt_turn_lifecycle = Some(callback);
        self
    }

    pub fn with_lifecycle_bus(mut self, lifecycle_bus: observability::RuntimeLifecycleBus) -> Self {
        self.lifecycle_bus = lifecycle_bus;
        self
    }

    pub fn with_observability_states(
        mut self,
        states: Arc<std::sync::Mutex<HashMap<String, observability::ExecutionObservabilityState>>>,
    ) -> Self {
        self.observability_states = states;
        self
    }

    pub fn with_active_metric_turns(
        mut self,
        turns: Arc<std::sync::Mutex<HashMap<String, ActiveMetricTurn>>>,
    ) -> Self {
        self.active_metric_turns = turns;
        self
    }

    pub fn with_scheduled_occurrence_id(mut self, occurrence_id: Option<String>) -> Self {
        self.scheduled_occurrence_id = occurrence_id;
        self
    }

    pub fn scheduled_occurrence_id(&self) -> Option<&str> {
        self.scheduled_occurrence_id.as_deref()
    }

    pub fn with_scheduled_task_context(
        mut self,
        context: Option<crate::provider::ScheduledTaskContextInfo>,
    ) -> Self {
        self.scheduled_task_context = context;
        self
    }

    pub fn scheduled_task_context(&self) -> Option<&crate::provider::ScheduledTaskContextInfo> {
        self.scheduled_task_context.as_ref()
    }

    /// Convert a scheduler-scoped app clone back to ordinary conversation
    /// semantics before dispatching a later user-authored prompt turn.
    pub fn without_scheduled_turn_context(mut self) -> Self {
        self.scheduled_occurrence_id = None;
        self.scheduled_task_context = None;
        self
    }

    pub fn with_lifecycle_subscriber(
        self,
        subscriber: Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync>,
    ) -> Self {
        self.lifecycle_bus.subscribe(subscriber);
        self
    }

    pub fn with_inline_lifecycle_subscriber(
        self,
        subscriber: Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync>,
    ) -> Self {
        self.lifecycle_bus.subscribe_inline(subscriber);
        self
    }

    pub fn acp_live_update_for<'a>(
        &'a self,
        context: AcpLiveEventContext,
    ) -> Option<impl Fn(&crate::acp::events::AcpUiEvent, Option<(u64, u64)>) -> Result<()> + 'a>
    {
        let live_update = self.acp_live_update.as_ref()?.clone();
        Some(
            move |event: &crate::acp::events::AcpUiEvent,
                  timeline_watermark: Option<(u64, u64)>| {
                live_update(context.clone(), event.clone(), timeline_watermark)
            },
        )
    }

    pub fn acp_session_update_for<'a>(
        &'a self,
        context: AcpLiveEventContext,
    ) -> Option<impl Fn() -> Result<()> + 'a> {
        let session_update = self.acp_session_update.as_ref()?.clone();
        Some(move || session_update(context.clone()))
    }

    pub fn acp_prompt_accepted_for<'a>(
        &'a self,
        context: AcpLiveEventContext,
    ) -> Option<impl Fn(&str) -> Result<()> + 'a> {
        let prompt_turn_lifecycle = self.prompt_turn_lifecycle.as_ref()?.clone();
        Some(move |prompt_id: &str| {
            prompt_turn_lifecycle(
                context.clone(),
                AcpPromptLifecycleEvent::Accepted {
                    prompt_id: prompt_id.to_string(),
                },
            )
        })
    }

    pub fn emit_acp_session_update(&self, context: AcpLiveEventContext) -> Result<()> {
        if let Some(session_update) = &self.acp_session_update {
            session_update(context)?;
        }
        Ok(())
    }

    pub fn notify_prompt_turn_finished(
        &self,
        context: AcpLiveEventContext,
        prompt_id: Option<String>,
        successful: bool,
    ) -> Result<()> {
        if let Some(callback) = &self.prompt_turn_lifecycle {
            callback(
                context,
                AcpPromptLifecycleEvent::Finished {
                    prompt_id,
                    successful,
                },
            )?;
        }
        Ok(())
    }

    pub fn emit_lifecycle_event(&self, mut event: RuntimeLifecycleEvent) {
        if let Some(occurrence_id) = self.scheduled_occurrence_id.clone() {
            event.set_scheduled_occurrence_id(Some(occurrence_id));
        }
        self.lifecycle_bus.emit(event);
    }

    pub fn create_metrics_fact_producer(&self) -> Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync> {
        let (sender, receiver) = std::sync::mpsc::sync_channel::<RuntimeLifecycleEvent>(2048);
        let app = self.clone_for_background();
        let _ = std::thread::Builder::new()
            .name("metrics-fact-producer".into())
            .spawn(move || {
                while let Ok(event) = receiver.recv() {
                    app.emit_derived_node_metrics_fact(&event);
                }
            });
        Arc::new(move |event| {
            if let Err(error) = sender.try_send(event) {
                match error {
                    std::sync::mpsc::TrySendError::Full(event) => tracing::warn!(
                        queue = "metrics-fact-producer",
                        capacity = 2048,
                        event_kind = lifecycle_event_kind(&event),
                        "metrics lifecycle fact input queue is full; event dropped"
                    ),
                    std::sync::mpsc::TrySendError::Disconnected(event) => tracing::warn!(
                        queue = "metrics-fact-producer",
                        event_kind = lifecycle_event_kind(&event),
                        "metrics lifecycle fact worker is disconnected; event dropped"
                    ),
                }
            }
        })
    }

    pub fn with_metrics_collection_enabled(mut self, enabled: bool) -> Self {
        self.metrics_collection_enabled = enabled;
        self
    }

    pub fn metrics_collection_enabled(&self) -> bool {
        self.metrics_collection_enabled
    }

    pub fn begin_metrics_turn(&self, attempt_key: String, turn: ActiveMetricTurn) {
        if self.metrics_collection_enabled {
            self.active_metric_turns
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(attempt_key, turn);
        }
    }

    pub fn active_metrics_turn(&self, attempt_key: &str) -> Option<ActiveMetricTurn> {
        self.active_metric_turns
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(attempt_key)
            .cloned()
    }

    pub fn end_metrics_turn(&self, attempt_key: &str) {
        self.active_metric_turns
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(attempt_key);
    }

    pub fn direct_usage_baseline(attempt_dir: Option<&camino::Utf8Path>) -> u64 {
        let Some(attempt_dir) = attempt_dir else {
            return 0;
        };
        crate::acp::usage::read_prompt_usage_segments(attempt_dir)
            .last()
            .map(|segment| segment.turn_seq)
            .unwrap_or(0)
    }

    pub fn direct_metrics_is_follow_up(
        &self,
        attempt_key: &str,
        attempt_dir: Option<&camino::Utf8Path>,
        attempt_path: &camino::Utf8Path,
    ) -> bool {
        if self.active_metrics_turn(attempt_key).is_some() {
            return true;
        }
        let has_usage_history = attempt_dir
            .map(|dir| Self::direct_usage_baseline(Some(dir)))
            .unwrap_or(0)
            > 0;
        has_usage_history
            || observability::load_observability_snapshot(attempt_path).event_revision > 0
    }

    pub fn direct_usage_segments_after(
        attempt_dir: Option<&camino::Utf8Path>,
        baseline_turn_seq: u64,
    ) -> Vec<crate::acp::usage::AcpPromptUsageSegment> {
        let Some(attempt_dir) = attempt_dir else {
            return Vec::new();
        };
        crate::acp::usage::read_prompt_usage_segments(attempt_dir)
            .into_iter()
            .filter(|segment| segment.turn_seq > baseline_turn_seq)
            .collect()
    }

    pub fn direct_model_usages_from_segments(
        segments: &[crate::acp::usage::AcpPromptUsageSegment],
        fallback_provider: Option<&str>,
        fallback_model: Option<&str>,
    ) -> Vec<observability::ModelUsage> {
        segments
            .iter()
            .filter_map(|segment| {
                let provider = segment.provider.as_deref().or(fallback_provider)?;
                let model = segment.model.as_deref().or(fallback_model)?;
                Some(observability::ModelUsage {
                    provider: provider.to_string(),
                    model: model.to_string(),
                    usage: observability::TokenUsage {
                        input_tokens: segment.usage.input_tokens,
                        output_tokens: segment.usage.output_tokens,
                        cache_read_tokens: segment.usage.cached_read_tokens,
                        total_tokens: segment.usage.effective_total_tokens(),
                    },
                    acp_session_elapsed_ms: segment.elapsed_ms,
                })
            })
            .collect()
    }

    /// Returns true when the run's current node is an AI-DYNAMIC node (AUTO mode).
    fn is_auto_run(&self, task_id: &str, run_id: &str) -> bool {
        let Ok(run) = self.run_status(task_id, run_id) else {
            return false;
        };
        run.current_round
            .as_deref()
            .zip(run.current_node.as_deref())
            .zip(run.current_attempt.as_deref())
            .and_then(|((round_id, node_id), attempt_id)| {
                read_json::<NodeState>(
                    &self
                        .paths
                        .node_file(task_id, run_id, round_id, node_id, attempt_id),
                )
                .ok()
            })
            .is_some_and(|node| node.node_type == crate::domain::NodeType::AiDynamic)
    }
    fn emit_derived_node_metrics_fact(&self, event: &RuntimeLifecycleEvent) {
        use observability::{
            ExecutionKind, ExecutionOutcome, LifecycleEventType, LifecycleTiming,
            MetricsLifecycleFact, MetricsSessionMode, ModelUsage, TerminalReason, TokenUsage,
            UnitKind,
        };
        let (
            event_type,
            task_id,
            run_id,
            task_uuid,
            run_uuid,
            node_uuid,
            logical_node_id,
            attempt_id,
            node_name,
            provider,
            model,
            started_at,
            ended_at,
            attempt_dir,
            outcome,
            round_index,
            child_run_id,
            dynamic_kind,
            repo_root,
        ) = match event {
            RuntimeLifecycleEvent::NodeStarted {
                task_id,
                run_id,
                task_uuid,
                run_uuid,
                node_uuid,
                node_id,
                attempt_id,
                node_name,
                agent_type,
                resolved_model,
                started_at,
                round_index,
                child_run_id,
                metrics_unit_kind,
                repo_root,
                ..
            } => (
                LifecycleEventType::ExecutionStarted,
                task_id.clone(),
                run_id.clone(),
                task_uuid.clone(),
                run_uuid.clone(),
                node_uuid.clone(),
                node_id.clone(),
                attempt_id.clone(),
                node_name.clone(),
                agent_type.clone(),
                resolved_model.clone(),
                started_at.clone(),
                None,
                None,
                None,
                *round_index,
                child_run_id.clone(),
                *metrics_unit_kind,
                repo_root.clone(),
            ),
            RuntimeLifecycleEvent::NodeCompleted {
                task_id,
                run_id,
                task_uuid,
                run_uuid,
                node_uuid,
                node_id,
                attempt_id,
                node_name,
                agent_type,
                resolved_model,
                started_at,
                finished_at,
                attempt_dir,
                outcome,
                round_index,
                child_run_id,
                metrics_unit_kind,
                repo_root,
                ..
            } => (
                LifecycleEventType::ExecutionCompleted,
                task_id.clone(),
                run_id.clone(),
                task_uuid.clone(),
                run_uuid.clone(),
                node_uuid.clone(),
                node_id.clone(),
                attempt_id.clone(),
                Some(node_name.clone()),
                agent_type.clone(),
                resolved_model.clone(),
                started_at.clone(),
                finished_at.clone(),
                Some(attempt_dir.clone()),
                Some(outcome.clone()),
                *round_index,
                child_run_id.clone(),
                *metrics_unit_kind,
                repo_root.clone(),
            ),
            _ => return,
        };
        let mut scoped_app = self.clone_for_background();
        scoped_app.paths = GoldBandPaths::new(Utf8PathBuf::from(repo_root));
        let (Some(task_uuid), Some(run_uuid), Some(node_uuid)) = (task_uuid, run_uuid, node_uuid)
        else {
            return;
        };
        if dynamic_kind.is_none()
            && direct_conversation_agent_label(&scoped_app, &task_id).is_some()
        {
            // Direct: one stable task UUID for task/execution/attempt.
            let execution_id = task_uuid.clone();
            let turn_key = format!("direct:{task_uuid}");
            let attempt_path = scoped_app
                .paths
                .run_dir(&task_id, &run_id)
                .join("observability")
                .join(&execution_id)
                .join(&execution_id)
                .join(observability::OBSERVABILITY_SNAPSHOT_FILE);
            let is_follow_up = if event_type == LifecycleEventType::ExecutionStarted {
                scoped_app.direct_metrics_is_follow_up(&turn_key, None, &attempt_path)
            } else {
                false
            };
            let active_turn = if event_type == LifecycleEventType::ExecutionStarted {
                match scoped_app.active_metrics_turn(&turn_key) {
                    Some(turn) => turn,
                    None => {
                        let usage_baseline = App::direct_usage_baseline(
                            attempt_dir.as_ref().map(|dir| camino::Utf8Path::new(dir)),
                        );
                        let turn = ActiveMetricTurn::new(
                            execution_id.clone(),
                            execution_id.clone(),
                            1,
                            usage_baseline,
                        );
                        scoped_app.begin_metrics_turn(turn_key.clone(), turn.clone());
                        turn
                    }
                }
            } else {
                let Some(turn) = scoped_app.active_metrics_turn(&turn_key) else {
                    return;
                };
                turn
            };
            let mut fallback_model = model.clone();
            if let Some(session_path) = attempt_dir
                .as_ref()
                .map(|dir| Utf8PathBuf::from(dir).join("acp.session.json"))
            {
                fallback_model = crate::acp::events::read_attempt_session_model_name(&session_path)
                    .or_else(|| fallback_model);
            }
            let attempt_state = scoped_app.update_observability_state(
                &active_turn.attempt_id,
                attempt_path,
                |state| {
                    if event_type == LifecycleEventType::ExecutionStarted {
                        state.record_started_at(started_at.clone());
                        if is_follow_up {
                            state.record_follow_up();
                        }
                    }
                    if event_type == LifecycleEventType::ExecutionCompleted {
                        let segments = App::direct_usage_segments_after(
                            attempt_dir.as_ref().map(|dir| camino::Utf8Path::new(dir)),
                            active_turn.usage_baseline_turn_seq,
                        );
                        let usages = App::direct_model_usages_from_segments(
                            &segments,
                            provider.as_deref(),
                            fallback_model.as_deref(),
                        );
                        for usage in usages {
                            state.record_model_usage(usage);
                        }
                        let usage_snapshot = attempt_dir.as_ref().map(|dir| {
                            crate::acp::events::read_attempt_metrics(
                                &Utf8PathBuf::from(dir).join("acp.session.json"),
                            )
                        });
                        if segments.is_empty()
                            && let (Some(usage), Some(p), Some(m)) =
                                (usage_snapshot, provider.as_ref(), fallback_model.as_ref())
                        {
                            state.record_cumulative_model_usage(
                                p.clone(),
                                m.clone(),
                                TokenUsage {
                                    input_tokens: usage.input_tokens,
                                    output_tokens: usage.output_tokens,
                                    cache_read_tokens: usage.cache_read_tokens,
                                    total_tokens: usage.total_tokens,
                                },
                                usage.elapsed_ms,
                            );
                        }
                    }
                    state.next_revision();
                },
            );
            let direct_revision = attempt_state.event_revision;
            let mut resolved_provider = provider.clone();
            let mut resolved_model = fallback_model.clone();
            if event_type == LifecycleEventType::ExecutionCompleted {
                if let Some(first_usage) = attempt_state.model_usages().into_iter().next() {
                    if resolved_provider.is_none() {
                        resolved_provider = Some(first_usage.provider.clone());
                    }
                    if resolved_model.is_none() {
                        resolved_model = Some(first_usage.model.clone());
                    }
                }
            }
            let mut turn_fact = MetricsLifecycleFact::new(
                event_type,
                direct_revision,
                ended_at.clone().unwrap_or_else(|| started_at.clone()),
                std::env::var("USERNAME")
                    .or_else(|_| std::env::var("USER"))
                    .unwrap_or_else(|_| "unknown".into()),
                scoped_app.paths.repo_root.to_string(),
                MetricsSessionMode::Direct,
                task_uuid.clone(),
                ExecutionKind::Turn,
                active_turn.execution_id.clone(),
            );
            turn_fact.task_title = scoped_app.task_show(&task_id).ok().and_then(|t| t.title);
            turn_fact.attempt_id = Some(active_turn.attempt_id.clone());
            turn_fact.attempt_index = Some(active_turn.attempt_index);
            turn_fact.provider = resolved_provider;
            turn_fact.model = resolved_model;
            turn_fact.collection_state_recovered = attempt_state.collection_state_recovered;
            if event_type == LifecycleEventType::ExecutionCompleted {
                if let Some(outcome_str) = &outcome {
                    if outcome_str.eq_ignore_ascii_case("success") {
                        turn_fact.outcome = Some(ExecutionOutcome::Completed);
                        turn_fact.terminal_reason = Some(TerminalReason::Completed);
                    } else if outcome_str.eq_ignore_ascii_case("killed") {
                        turn_fact.outcome = Some(ExecutionOutcome::Cancelled);
                        turn_fact.terminal_reason = Some(TerminalReason::ProcessKilled);
                    } else {
                        turn_fact.outcome = Some(ExecutionOutcome::Failed);
                        turn_fact.terminal_reason = Some(TerminalReason::ProviderError);
                    }
                }
                let usages = attempt_state.model_usages();
                let sum_tokens = |get: fn(&TokenUsage) -> Option<u64>| {
                    usages
                        .iter()
                        .filter_map(|u| get(&u.usage))
                        .fold(None, |acc, v| Some(acc.unwrap_or(0u64).saturating_add(v)))
                };
                if !usages.is_empty() {
                    turn_fact.usage = Some(TokenUsage {
                        input_tokens: sum_tokens(|u| u.input_tokens),
                        output_tokens: sum_tokens(|u| u.output_tokens),
                        cache_read_tokens: sum_tokens(|u| u.cache_read_tokens),
                        total_tokens: sum_tokens(|u| u.total_tokens),
                    });
                    turn_fact.model_usages = Some(usages);
                }
                turn_fact.timing = Some(LifecycleTiming {
                    started_at: attempt_state
                        .started_at
                        .clone()
                        .unwrap_or_else(|| started_at.clone()),
                    ended_at: ended_at.clone(),
                    acp_session_elapsed_ms: attempt_dir.as_ref().and_then(|dir| {
                        crate::acp::events::read_attempt_metrics(
                            &Utf8PathBuf::from(dir).join("acp.session.json"),
                        )
                        .elapsed_ms
                    }),
                });
                turn_fact.counters = Some(attempt_state.counters.clone());
            }
            scoped_app
                .lifecycle_bus
                .emit(RuntimeLifecycleEvent::MetricsFact(turn_fact));
            if event_type == LifecycleEventType::ExecutionCompleted {
                scoped_app.release_observability_state(&active_turn.execution_id);
                scoped_app.end_metrics_turn(&turn_key);
            }
            return;
        }
        // Skip AUTO wrapper nodes (not dynamic units) — they are implementation detail.
        if dynamic_kind.is_none() && scoped_app.is_auto_run(&task_id, &run_id) {
            return;
        }
        // executionId = taskId for all modes (Direct/AUTO/Workflow share the same identity).
        let execution_id = task_uuid.clone();
        // nodeId is the stable logical node identity; attemptId is unique per execution.
        let (node_metrics_id, metrics_attempt_id) = if dynamic_kind.is_some() {
            // AUTO unit: nodeId = DynamicNodeState.uuid, attemptId derived from nodeUuid.
            let Some(metrics_attempt_id) =
                observability::derive_attempt_id(&node_uuid, &attempt_id)
            else {
                return;
            };
            (node_uuid.clone(), metrics_attempt_id)
        } else {
            // Workflow node: nodeId = derived logical node (stable across retries),
            // attemptId = NodeState.uuid (new per concrete attempt).
            let Some(round_index) = round_index else {
                return;
            };
            let logical = observability::derive_execution_id(
                &run_uuid,
                &format!("round:{round_index}:node:{logical_node_id}"),
            )
            .unwrap_or_else(|| node_uuid.clone());
            (logical, node_uuid.clone())
        };

        let Some(attempt_index) = observability::attempt_index_from_local_id(&attempt_id) else {
            return;
        };
        let snapshot_path = scoped_app
            .paths
            .run_dir(&task_id, &run_id)
            .join("observability")
            .join(&execution_id)
            .join(&metrics_attempt_id)
            .join(observability::OBSERVABILITY_SNAPSHOT_FILE);
        let usage_snapshot = attempt_dir
            .as_ref()
            .map(|dir| {
                crate::acp::events::read_attempt_metrics(
                    &Utf8PathBuf::from(dir).join("acp.session.json"),
                )
            })
            .filter(|usage| {
                dynamic_kind != Some(crate::dynamic::DynamicNodeKind::WorkflowInvocation)
                    || usage.input_tokens.is_some()
                    || usage.output_tokens.is_some()
                    || usage.cache_read_tokens.is_some()
                    || usage.total_tokens.is_some()
                    || usage.elapsed_ms.is_some()
            });
        let usage_segments = attempt_dir
            .as_ref()
            .map(|dir| crate::acp::usage::read_prompt_usage_segments(Utf8Path::new(dir)))
            .unwrap_or_default();
        let metrics_model = attempt_dir
            .as_ref()
            .and_then(|dir| {
                crate::acp::events::read_attempt_session_model_name(
                    &Utf8PathBuf::from(dir).join("acp.session.json"),
                )
            })
            .or_else(|| model.clone());
        let state = scoped_app.update_observability_state(
            &metrics_attempt_id,
            snapshot_path.clone(),
            |state| {
                let mut recorded_segment = false;
                for segment in &usage_segments {
                    let (Some(segment_provider), Some(segment_model)) =
                        (segment.provider.as_ref(), segment.model.as_ref())
                    else {
                        continue;
                    };
                    state.record_model_usage(ModelUsage {
                        provider: segment_provider.clone(),
                        model: segment_model.clone(),
                        usage: TokenUsage {
                            input_tokens: segment.usage.input_tokens,
                            output_tokens: segment.usage.output_tokens,
                            cache_read_tokens: segment.usage.cached_read_tokens,
                            total_tokens: segment.usage.effective_total_tokens(),
                        },
                        acp_session_elapsed_ms: segment.elapsed_ms,
                    });
                    recorded_segment = true;
                }
                // Legacy attempts do not have provider/model captured per prompt.
                // Preserve their cumulative totals without inventing segments.
                if !recorded_segment
                    && let (Some(usage), Some(provider), Some(model)) =
                        (&usage_snapshot, provider.as_ref(), metrics_model.as_ref())
                {
                    state.record_cumulative_model_usage(
                        provider.clone(),
                        model.clone(),
                        TokenUsage {
                            input_tokens: usage.input_tokens,
                            output_tokens: usage.output_tokens,
                            cache_read_tokens: usage.cache_read_tokens,
                            total_tokens: usage.total_tokens,
                        },
                        usage.elapsed_ms,
                    );
                }
            },
        );

        // Global revision: all events for the same task share a single
        // monotonically-increasing revision counter via the task_uuid
        // observability state. Per-node state retains model_usages
        // and per-node counters; only revision comes from the global state.
        let outer_snapshot_path = scoped_app
            .paths
            .run_dir(&task_id, &run_id)
            .join("observability")
            .join(&task_uuid)
            .join(observability::OBSERVABILITY_SNAPSHOT_FILE);
        let outer_state = scoped_app.update_observability_state(
            &task_uuid,
            outer_snapshot_path.clone(),
            |state| {
                state.next_revision();
            },
        );
        let revision = outer_state.event_revision;
        let mut fact = MetricsLifecycleFact::new(
            event_type,
            revision,
            ended_at.clone().unwrap_or_else(|| started_at.clone()),
            std::env::var("USERNAME")
                .or_else(|_| std::env::var("USER"))
                .unwrap_or_else(|_| "unknown".into()),
            scoped_app.paths.repo_root.to_string(),
            if dynamic_kind.is_some() {
                MetricsSessionMode::Auto
            } else {
                MetricsSessionMode::Workflow
            },
            task_uuid.clone(),
            if dynamic_kind.is_some() {
                ExecutionKind::UnitAttempt
            } else {
                ExecutionKind::NodeAttempt
            },
            execution_id.clone(),
        );
        fact.task_title = scoped_app.task_show(&task_id).ok().and_then(|t| t.title);
        fact.node_id = Some(node_metrics_id.clone());
        fact.attempt_id = Some(metrics_attempt_id.clone());
        fact.attempt_index = Some(attempt_index);
        fact.role_name = node_name;
        fact.round_index = round_index;
        fact.provider = provider;
        fact.model = if event_type == LifecycleEventType::ExecutionStarted {
            None
        } else {
            metrics_model.clone()
        };
        fact.collection_state_recovered = state.collection_state_recovered;
        fact.unit_kind = dynamic_kind.map(|kind| match kind {
            crate::dynamic::DynamicNodeKind::Worker => UnitKind::Worker,
            crate::dynamic::DynamicNodeKind::WorkflowInvocation => UnitKind::WorkflowInvocation,
            crate::dynamic::DynamicNodeKind::Merge => UnitKind::Merge,
            crate::dynamic::DynamicNodeKind::Acceptance => UnitKind::Acceptance,
        });
        fact.child_run_id = child_run_id.and_then(|child_run_id| {
            read_json::<RunState>(&scoped_app.paths.run_file(&task_id, &child_run_id))
                .ok()
                .and_then(|run| run.uuid)
        });
        if let Some(outcome) = outcome {
            let success = outcome.eq_ignore_ascii_case("success");
            let killed = outcome.eq_ignore_ascii_case("killed");
            let invalid = outcome.eq_ignore_ascii_case("invalid");
            fact.outcome = Some(if success {
                ExecutionOutcome::Success
            } else if killed {
                ExecutionOutcome::Killed
            } else {
                ExecutionOutcome::Failure
            });
            fact.terminal_reason = Some(if success {
                TerminalReason::Completed
            } else if killed {
                TerminalReason::ProcessKilled
            } else if invalid {
                TerminalReason::ValidationError
            } else if dynamic_kind == Some(crate::dynamic::DynamicNodeKind::Acceptance) {
                TerminalReason::AcceptanceRejected
            } else {
                TerminalReason::ExecutionFailed
            });
            let usages = state.model_usages();
            let sum = |get: fn(&TokenUsage) -> Option<u64>| {
                usages
                    .iter()
                    .filter_map(|usage| get(&usage.usage))
                    .fold(None, |total, value| {
                        Some(total.unwrap_or(0u64).saturating_add(value))
                    })
            };
            if !usages.is_empty() {
                fact.usage = Some(TokenUsage {
                    input_tokens: sum(|u| u.input_tokens),
                    output_tokens: sum(|u| u.output_tokens),
                    cache_read_tokens: sum(|u| u.cache_read_tokens),
                    total_tokens: sum(|u| u.total_tokens),
                });
                fact.model_usages = Some(usages);
            }
            fact.timing = Some(LifecycleTiming {
                started_at: started_at.clone(),
                ended_at: ended_at.clone(),
                acp_session_elapsed_ms: usage_snapshot.and_then(|usage| usage.elapsed_ms),
            });
        }
        let acceptance_passed = (dynamic_kind == Some(crate::dynamic::DynamicNodeKind::Acceptance)
            && event_type == LifecycleEventType::ExecutionCompleted)
            .then(|| fact.outcome == Some(ExecutionOutcome::Success));
        self.lifecycle_bus
            .emit(RuntimeLifecycleEvent::MetricsFact(fact));
        if let Some(passed) = acceptance_passed {
            let outer_state =
                scoped_app.update_observability_state(&task_uuid, outer_snapshot_path, |state| {
                    state.next_revision();
                    state.next_acceptance_attempt();
                });
            let acceptance_revision = outer_state.event_revision;
            let acceptance_attempt = outer_state.next_acceptance_attempt_value();
            let mut acceptance = MetricsLifecycleFact::new(
                LifecycleEventType::AcceptanceCompleted,
                acceptance_revision,
                ended_at.unwrap_or_else(|| started_at.clone()),
                std::env::var("USERNAME")
                    .or_else(|_| std::env::var("USER"))
                    .unwrap_or_else(|_| "unknown".into()),
                scoped_app.paths.repo_root.to_string(),
                MetricsSessionMode::Auto,
                task_uuid,
                ExecutionKind::UnitAttempt,
                execution_id.clone(),
            );
            acceptance.attempt_id = Some(metrics_attempt_id.clone());
            acceptance.attempt_index = Some(attempt_index);
            acceptance.node_id = Some(node_metrics_id.clone());
            acceptance.unit_kind = Some(UnitKind::Acceptance);
            acceptance.passed = Some(passed);
            acceptance.acceptance_attempt = Some(acceptance_attempt);
            acceptance.first_pass = Some(passed && acceptance_attempt == 1);
            acceptance.collection_state_recovered = outer_state.collection_state_recovered;
            acceptance.task_title = scoped_app.task_show(&task_id).ok().and_then(|t| t.title);
            self.lifecycle_bus
                .emit(RuntimeLifecycleEvent::MetricsFact(acceptance));
        }
        if event_type == LifecycleEventType::ExecutionCompleted {
            scoped_app.release_observability_state(&metrics_attempt_id);
        }
    }

    pub fn update_observability_state(
        &self,
        execution_id: &str,
        snapshot_path: Utf8PathBuf,
        update: impl FnOnce(&mut observability::ExecutionObservabilityState),
    ) -> observability::ExecutionObservabilityState {
        let mut states = self
            .observability_states
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let state = states.entry(execution_id.to_string()).or_default();
        update(state);
        let snapshot = state.clone();
        observability::persist_observability_snapshot_best_effort(snapshot_path, snapshot.clone());
        snapshot
    }

    pub fn release_observability_state(&self, execution_id: &str) {
        self.observability_states
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(execution_id);
    }

    pub fn record_metrics_resume_cause(
        &self,
        task_id: &str,
        run_id: &str,
        cause: observability::ResumeCause,
    ) {
        if !self.metrics_collection_enabled() {
            return;
        }
        let Ok(run) = self.run_status(task_id, run_id) else {
            return;
        };
        let Some(task_uuid) = run.task_uuid.clone() else {
            return;
        };
        let path = self
            .paths
            .run_dir(task_id, run_id)
            .join("observability")
            .join(&task_uuid)
            .join(observability::OBSERVABILITY_SNAPSHOT_FILE);
        self.update_observability_state(&task_uuid, path, |state| {
            state.set_pending_resume_cause(cause);
        });
    }

    pub fn clear_metrics_resume_cause(
        &self,
        task_id: &str,
        run_id: &str,
        expected: observability::ResumeCause,
    ) {
        if !self.metrics_collection_enabled() {
            return;
        }
        let Ok(run) = self.run_status(task_id, run_id) else {
            return;
        };
        let Some(task_uuid) = run.task_uuid.clone() else {
            return;
        };
        let path = self
            .paths
            .run_dir(task_id, run_id)
            .join("observability")
            .join(&task_uuid)
            .join(observability::OBSERVABILITY_SNAPSHOT_FILE);
        self.update_observability_state(&task_uuid, path, |state| {
            state.clear_pending_resume_cause(expected);
        });
    }

    pub fn load_settings(&self) -> Result<SettingsConfig> {
        load_settings_file(&self.paths.user_settings_file())
    }

    pub fn save_settings(&self, settings: &SettingsConfig) -> Result<()> {
        write_json(&self.paths.user_settings_file(), settings)
    }

    pub fn load_state(&self) -> Result<StateConfig> {
        let path = self.paths.user_state_file();
        if !path.exists() {
            return Ok(StateConfig::default());
        }
        read_json(&path)
    }

    pub fn save_state(&self, state: &StateConfig) -> Result<()> {
        write_json(&self.paths.user_state_file(), state)
    }

    pub fn set_user_console_theme(&self, theme: ConsoleThemeName) -> Result<SettingsConfig> {
        let mut settings = self.load_settings()?;
        settings.console_theme = Some(theme);
        self.save_settings(&settings)?;
        Ok(settings)
    }

    pub fn set_user_desktop_appearance(
        &self,
        appearance: AppearancePreference,
    ) -> Result<SettingsConfig> {
        let mut settings = self.load_settings()?;
        settings.appearance = Some(appearance);
        self.save_settings(&settings)?;
        Ok(settings)
    }

    pub fn set_user_desktop_personalization(
        &self,
        personalization: PersonalizationPreference,
    ) -> Result<SettingsConfig> {
        let mut settings = self.load_settings()?;
        settings.personalization = Some(personalization.normalized());
        self.save_settings(&settings)?;
        Ok(settings)
    }

    pub fn set_user_desktop_language(&self, language: DesktopLanguage) -> Result<SettingsConfig> {
        let mut settings = self.load_settings()?;
        settings.desktop_language = Some(language);
        self.save_settings(&settings)?;
        Ok(settings)
    }

    pub fn set_user_desktop_preferences(
        &self,
        appearance: AppearancePreference,
        personalization: PersonalizationPreference,
        language: DesktopLanguage,
        use_local_claude: bool,
        verbose_logging: bool,
    ) -> Result<SettingsConfig> {
        let mut settings = self.load_settings()?;
        settings.appearance = Some(appearance);
        settings.personalization = Some(personalization.normalized());
        settings.desktop_language = Some(language);
        settings.use_local_claude = Some(use_local_claude);
        settings.log_level = Some(if verbose_logging {
            RuntimeLogLevel::Debug
        } else {
            RuntimeLogLevel::Info
        });
        self.save_settings(&settings)?;
        Ok(settings)
    }

    pub fn set_user_desktop_updater_url_override(
        &self,
        override_url: Option<String>,
    ) -> Result<SettingsConfig> {
        let mut settings = self.load_settings()?;
        settings.desktop_updater_url_override = override_url;
        self.save_settings(&settings)?;
        Ok(settings)
    }

    pub fn set_user_desktop_updater_last_checked_at(
        &self,
        checked_at: Option<String>,
    ) -> Result<StateConfig> {
        let mut state = self.load_state()?;
        state.desktop_updater_last_checked_at = checked_at;
        self.save_state(&state)?;
        Ok(state)
    }

    pub fn set_user_desktop_update_badges(
        &self,
        update_badges: DesktopUpdateBadgeState,
    ) -> Result<StateConfig> {
        let mut state = self.load_state()?;
        state.desktop_update_badges = update_badges;
        self.save_state(&state)?;
        Ok(state)
    }

    pub fn set_user_desktop_available_update(
        &self,
        available_update: Option<DesktopAvailableUpdate>,
    ) -> Result<StateConfig> {
        let mut state = self.load_state()?;
        state.desktop_available_update = available_update;
        self.save_state(&state)?;
        Ok(state)
    }

    pub fn record_user_recent_desktop_workspace(&self, workspace: &str) -> Result<StateConfig> {
        let mut state = self.load_state()?;
        state
            .recent_desktop_workspaces
            .retain(|item| item != workspace);
        state
            .recent_desktop_workspaces
            .insert(0, workspace.to_string());
        state.recent_desktop_workspaces.truncate(8);
        self.save_state(&state)?;

        Ok(state)
    }

    pub fn remove_user_recent_desktop_workspace(&self, workspace: &str) -> Result<StateConfig> {
        let mut state = self.load_state()?;
        state
            .recent_desktop_workspaces
            .retain(|item| item != workspace);
        self.save_state(&state)?;
        Ok(state)
    }

    pub fn set_user_agents(
        &self,
        agents: std::collections::BTreeMap<ManagedAgentId, ManagedAgentConfig>,
    ) -> Result<SettingsConfig> {
        let mut settings = self.load_settings()?;
        settings.agents = Some(agents);
        self.save_settings(&settings)?;
        Ok(settings)
    }

    pub fn managed_agents(
        &self,
    ) -> &std::collections::BTreeMap<ManagedAgentId, ManagedAgentConfig> {
        &self.config.agents
    }

    pub fn save_managed_agent(
        &self,
        agent_id: ManagedAgentId,
        config: ManagedAgentConfig,
    ) -> Result<SettingsConfig> {
        let mut agents = self.config.agents.clone();
        agents.insert(agent_id, config);
        self.set_user_agents(agents)
    }

    pub fn remove_managed_agent(&self, agent_id: &ManagedAgentId) -> Result<SettingsConfig> {
        let mut agents = self.config.agents.clone();
        agents.remove(agent_id);
        self.set_user_agents(agents)
    }

    // ── MCP (委托给 McpManager，对标 Zed ContextServerStore) ──

    fn mcp_manager(&self) -> McpManager {
        McpManager::new(self.paths.user_settings_file())
    }

    pub fn list_mcp_servers(&self) -> Result<Vec<McpServerConfig>> {
        Ok(self
            .mcp_manager()
            .list()?
            .into_iter()
            .map(|s| s.config)
            .collect())
    }

    pub fn add_mcp_server(&self, json_content: &str) -> Result<Vec<McpServerConfig>> {
        let (_, list) = self.mcp_manager().add(json_content)?;
        Ok(list.into_iter().map(|s| s.config).collect())
    }

    pub fn update_mcp_server(&self, id: &str, json_content: &str) -> Result<Vec<McpServerConfig>> {
        let (_, list) = self.mcp_manager().update(id, json_content)?;
        Ok(list.into_iter().map(|s| s.config).collect())
    }

    pub fn delete_mcp_server(&self, id: &str) -> Result<Vec<McpServerConfig>> {
        Ok(self
            .mcp_manager()
            .delete(id)?
            .into_iter()
            .map(|s| s.config)
            .collect())
    }

    pub fn toggle_mcp_server(&self, id: &str, enabled: bool) -> Result<Vec<McpServerConfig>> {
        Ok(self
            .mcp_manager()
            .toggle(id, enabled)?
            .into_iter()
            .map(|s| s.config)
            .collect())
    }

    pub fn check_mcp_server_health(&self, id: &str) -> Result<McpServerHealthResult> {
        self.mcp_manager().check_health(id)
    }

    pub fn list_mcp_tools(&self, id: &str) -> Result<Vec<crate::config::ToolInfo>> {
        self.mcp_manager().list_tools(id)
    }

    pub fn enabled_mcp_servers(&self) -> Result<Vec<McpServerConfig>> {
        self.mcp_manager().enabled_servers()
    }

    pub fn acp_mcp_servers(&self) -> Result<Vec<serde_json::Value>> {
        self.mcp_manager().configured_acp_mcp_servers()
    }

    // ── SKILL (delegates to skill::SkillManager) ──

    pub fn skill_manager(&self) -> crate::skill::SkillManager {
        crate::skill::SkillManager::new(self.paths.clone(), self.config.agents.clone())
    }

    pub fn list_skills(&self) -> Result<crate::skill::SkillListResult> {
        self.skill_manager().list()
    }

    pub fn read_skill(
        &self,
        name: &str,
        source: SkillSource,
    ) -> Result<crate::skill::SkillContent> {
        self.skill_manager().read(name, source)
    }

    pub fn write_skill(&self, name: &str, source: SkillSource, content: &str) -> Result<SkillMeta> {
        self.skill_manager().write(name, source, content)
    }

    pub fn delete_skill(&self, name: &str, source: SkillSource) -> Result<()> {
        self.skill_manager().delete(name, source)
    }

    /// 同步 SKILL symlink 到已配置 agent 的 skills 目录（保存/删除时自动调用）
    /// workspace_path 用于指定项目级 SKILL 的实际工作空间目录
    /// sync_target_types: 限定同步目标 agent（如 ["claude-acp", "codex-acp"]），None 表示同步到所有已配置 agent
    pub fn sync_skill_instance(
        &self,
        skill_name: &str,
        source_directory_path: &str,
        source: SkillSource,
        workspace_path: Option<&str>,
        sync_target_types: Option<&[String]>,
    ) -> Result<()> {
        self.skill_manager().sync_skill_instance(
            skill_name,
            source_directory_path,
            source,
            workspace_path,
            sync_target_types,
        )
    }

    pub fn reconcile_skill_instance_links(
        &self,
        skill_name: &str,
        source_directory_path: &str,
        source: SkillSource,
        workspace_path: Option<&str>,
        sync_target_types: Option<&[String]>,
    ) -> Result<()> {
        self.skill_manager().reconcile_skill_instance_links(
            skill_name,
            source_directory_path,
            source,
            workspace_path,
            sync_target_types,
        )
    }

    pub fn cleanup_skill_instance_links(
        &self,
        skill_name: &str,
        source_directory_path: &str,
        source: SkillSource,
        workspace_path: Option<&str>,
        sync_target_types: Option<&[String]>,
    ) {
        self.skill_manager().cleanup_skill_instance_links(
            skill_name,
            source_directory_path,
            source,
            workspace_path,
            sync_target_types,
        );
    }

    pub fn workflow_templates(&self) -> Result<WorkflowTemplateStore> {
        self.load_workflow_template_store()
    }

    pub fn auto_templates(&self) -> Result<AutoTemplateStore> {
        self.load_auto_template_store()
    }

    pub fn profiles(&self) -> Result<ProfileList> {
        list_profiles(&self.paths, self.config.desktop_language)
    }

    pub fn profile_show(&self, id: &str) -> Result<ProfileEntry> {
        show_profile(&self.paths, id, self.config.desktop_language)
    }

    pub fn create_profile(&self, input: ProfileInput) -> Result<ProfileEntry> {
        create_profile(&self.paths, input)
    }

    pub fn import_profiles_from_folder(
        &self,
        input: ImportProfilesInput,
    ) -> Result<ImportProfilesResult> {
        import_profiles_from_folder(&self.paths, input)
    }

    pub fn update_profile(&self, id: &str, input: ProfileInput) -> Result<ProfileEntry> {
        update_profile(&self.paths, id, input)
    }

    pub fn delete_profile(&self, id: &str, force: bool) -> Result<ProfileList> {
        let profile = show_profile(&self.paths, id, self.config.desktop_language)?;
        if profile.is_built_in {
            return Err(ProfileCommandError::ReadonlyBuiltIn.into());
        }
        let usage = self.profile_usage_counts(id)?;
        if !force && (usage.template_count > 0 || usage.task_count > 0 || usage.run_count > 0) {
            return Err(ProfileCommandError::DeleteConfirmationRequired {
                template_count: usage.template_count,
                task_count: usage.task_count,
                run_count: usage.run_count,
            }
            .into());
        }
        delete_profile_file(&self.paths, id)?;
        list_profiles(&self.paths, self.config.desktop_language)
    }

    pub fn save_workflow_template(
        &self,
        name: String,
        workflow: WorkflowDsl,
    ) -> Result<WorkflowTemplateStore> {
        self.save_workflow_template_with_bindings(name, workflow, WorkflowModelBindings::default())
    }

    pub fn save_workflow_template_with_bindings(
        &self,
        name: String,
        mut workflow: WorkflowDsl,
        mut model_bindings: WorkflowModelBindings,
    ) -> Result<WorkflowTemplateStore> {
        let name = name.trim();
        if name.is_empty() {
            bail!("workflow template name cannot be empty");
        }
        let mut store = self.load_workflow_template_store()?;
        for attempt in 0..3 {
            workflow.id = next_workflow_id();
            let conflicts = store
                .templates
                .iter()
                .any(|template| template.workflow.id == workflow.id);
            if !conflicts {
                break;
            }
            if attempt == 2 {
                bail!("failed to generate a unique workflow id after 3 attempts");
            }
        }
        reconcile_authoring_workflow_for_save(&mut workflow, &mut model_bindings, None, None)?;
        let validated = validate_authoring_workflow(workflow)?;
        resolve_workflow_profiles(&self.paths, &validated.raw, self.config.desktop_language)?;
        validate_unique_workflow_template_id(&store, &validated.raw, name, None)?;
        validate_ai_dynamic_allowed_workflows(&validated.raw, &store)?;

        let now = now_rfc3339_like();
        let id = unique_workflow_template_id(&store, name);
        store.templates.push(WorkflowTemplate {
            id: id.clone(),
            name: name.to_string(),
            is_built_in: false,
            optional_entry_stage: None,
            workflow: validated.raw,
            model_bindings,
            created_at: now.clone(),
            updated_at: now,
        });
        store.last_used_template_id = Some(id);
        self.save_workflow_template_store(&store)?;
        Ok(store)
    }

    pub fn update_workflow_template(
        &self,
        template_id: &str,
        workflow: WorkflowDsl,
    ) -> Result<WorkflowTemplateStore> {
        self.update_workflow_template_with_bindings(
            template_id,
            workflow,
            WorkflowModelBindings::default(),
        )
    }

    pub fn update_workflow_template_with_bindings(
        &self,
        template_id: &str,
        mut workflow: WorkflowDsl,
        mut model_bindings: WorkflowModelBindings,
    ) -> Result<WorkflowTemplateStore> {
        let template_id = template_id.trim();
        if template_id.is_empty() {
            bail!("workflow template id cannot be empty");
        }
        let mut store = self.load_workflow_template_store()?;
        if store
            .templates
            .iter()
            .find(|template| template.id == template_id)
            .is_some_and(|template| template.is_built_in)
        {
            return Err(WorkflowTemplateCommandError::ReadonlyBuiltIn.into());
        }
        let persisted_template = store
            .templates
            .iter()
            .find(|template| template.id == template_id)
            .with_context(|| format!("workflow template `{template_id}` not found"))?;
        let persisted = TaskAuthoringWorkflow {
            workflow: persisted_template.workflow.clone(),
            model_bindings: persisted_template.model_bindings.clone(),
        };
        reconcile_authoring_workflow_for_save(
            &mut workflow,
            &mut model_bindings,
            Some(&persisted),
            None,
        )?;
        let validated = validate_authoring_workflow(workflow)?;
        resolve_workflow_profiles(&self.paths, &validated.raw, self.config.desktop_language)?;
        validate_unique_workflow_template_id(
            &store,
            &validated.raw,
            template_id,
            Some(template_id),
        )?;
        validate_ai_dynamic_allowed_workflows(&validated.raw, &store)?;

        let template = store
            .templates
            .iter_mut()
            .find(|template| template.id == template_id)
            .expect("persisted workflow template was resolved before validation");
        template.workflow = validated.raw;
        template.model_bindings = model_bindings;
        template.updated_at = now_rfc3339_like();
        store.last_used_template_id = Some(template_id.to_string());
        self.save_workflow_template_store(&store)?;
        Ok(store)
    }

    pub fn update_built_in_workflow_template_bindings(
        &self,
        template_id: &str,
        mut model_bindings: WorkflowModelBindings,
    ) -> Result<WorkflowTemplateStore> {
        let mut store = self.load_workflow_template_store()?;
        let template_index = store
            .templates
            .iter()
            .position(|template| template.id == template_id && template.is_built_in)
            .ok_or(WorkflowTemplateCommandError::ReadonlyBuiltIn)?;
        let persisted = TaskAuthoringWorkflow {
            workflow: store.templates[template_index].workflow.clone(),
            model_bindings: store.templates[template_index].model_bindings.clone(),
        };
        let mut workflow = persisted.workflow.clone();
        reconcile_authoring_workflow_for_save(
            &mut workflow,
            &mut model_bindings,
            Some(&persisted),
            Some(template_id),
        )?;
        validate_and_inject(
            &workflow,
            &model_bindings,
            &self.config.agents,
            &self.provider_diagnostics(),
        )?;
        let template = &mut store.templates[template_index];
        template.workflow = workflow;
        template.model_bindings = model_bindings;
        template.updated_at = now_rfc3339_like();
        store.last_used_template_id = Some(template_id.to_string());
        self.save_workflow_template_store(&store)?;
        Ok(store)
    }

    pub fn delete_workflow_template(&self, template_id: &str) -> Result<WorkflowTemplateStore> {
        let template_id = template_id.trim();
        if template_id.is_empty() {
            bail!("workflow template id cannot be empty");
        }
        let mut store = self.load_workflow_template_store()?;
        if store
            .templates
            .iter()
            .find(|template| template.id == template_id)
            .is_some_and(|template| template.is_built_in)
        {
            return Err(WorkflowTemplateCommandError::ReadonlyBuiltIn.into());
        }
        let original_len = store.templates.len();
        store
            .templates
            .retain(|template| template.id != template_id);
        if store.templates.len() == original_len {
            bail!("workflow template `{template_id}` not found");
        }
        if store.last_used_template_id.as_deref() == Some(template_id) {
            store.last_used_template_id = Some("default".to_string());
        }
        self.save_workflow_template_store(&store)?;
        Ok(store)
    }

    pub fn save_auto_template(
        &self,
        name: String,
        config: ConversationAutoConfig,
    ) -> Result<AutoTemplateStore> {
        let name = name.trim();
        if name.is_empty() {
            bail!("auto template name cannot be empty");
        }
        let mut store = self.load_auto_template_store()?;
        if store.templates.iter().any(|template| template.name == name) {
            bail!("auto template name `{name}` already exists");
        }
        let now = now_rfc3339_like();
        let id = next_auto_template_id(&store);
        store.templates.push(AutoTemplate {
            id,
            name: name.to_string(),
            config,
            created_at: now.clone(),
            updated_at: now,
        });
        self.save_auto_template_store(&store)?;
        Ok(store)
    }

    pub fn update_auto_template(
        &self,
        template_id: &str,
        name: String,
        config: ConversationAutoConfig,
    ) -> Result<AutoTemplateStore> {
        let name = name.trim();
        if template_id.trim().is_empty() {
            bail!("auto template id cannot be empty");
        }
        if name.is_empty() {
            bail!("auto template name cannot be empty");
        }
        let mut store = self.load_auto_template_store()?;
        if store
            .templates
            .iter()
            .any(|template| template.id != template_id && template.name == name)
        {
            bail!("auto template name `{name}` already exists");
        }
        let template = store
            .templates
            .iter_mut()
            .find(|template| template.id == template_id)
            .with_context(|| format!("auto template `{template_id}` not found"))?;
        template.name = name.to_string();
        template.config = config;
        template.updated_at = now_rfc3339_like();
        self.save_auto_template_store(&store)?;
        Ok(store)
    }

    pub fn delete_auto_template(&self, template_id: &str) -> Result<AutoTemplateStore> {
        if template_id.trim().is_empty() {
            bail!("auto template id cannot be empty");
        }
        let mut store = self.load_auto_template_store()?;
        let original_len = store.templates.len();
        store
            .templates
            .retain(|template| template.id != template_id);
        if store.templates.len() == original_len {
            bail!("auto template `{template_id}` not found");
        }
        self.save_auto_template_store(&store)?;
        Ok(store)
    }

    pub fn replace_auto_templates(
        &self,
        templates: Vec<AutoTemplate>,
    ) -> Result<AutoTemplateStore> {
        let now = now_rfc3339_like();
        let mut store = AutoTemplateStore {
            version: VERSION.to_string(),
            templates: Vec::new(),
        };
        for template in templates {
            let name = template.name.trim();
            if name.is_empty() {
                continue;
            }
            let mut id = template.id.trim().to_string();
            if id.is_empty() || store.templates.iter().any(|item| item.id == id) {
                id = next_auto_template_id(&store);
            }
            if store.templates.iter().any(|item| item.name == name) {
                continue;
            }
            store.templates.push(AutoTemplate {
                id,
                name: name.to_string(),
                config: template.config,
                created_at: if template.created_at.trim().is_empty() {
                    now.clone()
                } else {
                    template.created_at
                },
                updated_at: if template.updated_at.trim().is_empty() {
                    now.clone()
                } else {
                    template.updated_at
                },
            });
        }
        self.save_auto_template_store(&store)?;
        Ok(store)
    }

    fn load_workflow_template_store(&self) -> Result<WorkflowTemplateStore> {
        let default_profiles = ensure_default_user_profiles(&self.paths)?;
        let default_template =
            default_workflow_template(&default_profiles, self.config.desktop_language);
        let lightweight_template =
            default_lightweight_workflow_template(&default_profiles, self.config.desktop_language);
        let path = self.paths.workflow_templates_file();
        if !path.exists() {
            let legacy_path = self.paths.legacy_project_workflow_templates_file();
            if legacy_path.exists() {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent.as_std_path())?;
                }
                fs::copy(legacy_path.as_std_path(), path.as_std_path())?;
            }
        }
        if path.exists() {
            let mut store: WorkflowTemplateStore = read_json(&path)?;
            for template in &mut store.templates {
                migrate_authoring_workflow(
                    &mut template.workflow,
                    &mut template.model_bindings,
                    template.is_built_in.then_some(template.id.as_str()),
                )?;
            }
            upsert_built_in_workflow_template(&mut store.templates, lightweight_template, 0)?;
            upsert_built_in_workflow_template(&mut store.templates, default_template, 0)?;
            if let Some(workflow) = store.last_created_workflow.as_mut() {
                let mut ignored = WorkflowModelBindings::default();
                migrate_authoring_workflow(workflow, &mut ignored, None)?;
            }
            self.save_workflow_template_store(&store)?;
            return Ok(store);
        }
        let mut store = WorkflowTemplateStore {
            version: VERSION.to_string(),
            last_used_template_id: Some("default".to_string()),
            last_created_workflow: None,
            templates: vec![default_template, lightweight_template],
        };
        for template in &mut store.templates {
            migrate_authoring_workflow(
                &mut template.workflow,
                &mut template.model_bindings,
                template.is_built_in.then_some(template.id.as_str()),
            )?;
        }
        self.save_workflow_template_store(&store)?;
        Ok(store)
    }

    fn save_workflow_template_store(&self, store: &WorkflowTemplateStore) -> Result<()> {
        fs::create_dir_all(self.paths.user_context_dir().as_std_path())?;
        write_json(&self.paths.workflow_templates_file(), store)
    }

    pub fn task_authoring_workflow(&self, task_id: &str) -> Result<TaskAuthoringWorkflow> {
        let path = self.paths.workflow_file(task_id);
        let compat: TaskAuthoringWorkflowCompat = read_json(&path)?;
        let (mut current, legacy) = compat.into_current();
        let migrated =
            migrate_authoring_workflow(&mut current.workflow, &mut current.model_bindings, None)?;
        if legacy || migrated {
            write_json(&path, &current)?;
        }
        Ok(current)
    }

    pub fn task_workflow(&self, task_id: &str) -> Result<WorkflowDsl> {
        Ok(self.task_authoring_workflow(task_id)?.workflow)
    }

    pub fn executable_task_workflow(&self, task_id: &str) -> Result<WorkflowDsl> {
        let authoring = self.task_authoring_workflow(task_id)?;
        Ok(validate_and_inject(
            &authoring.workflow,
            &authoring.model_bindings,
            &self.config.agents,
            &self.provider_diagnostics(),
        )?)
    }

    fn save_task_authoring_workflow(
        &self,
        task_id: &str,
        mut authoring: TaskAuthoringWorkflow,
    ) -> Result<()> {
        let path = self.paths.workflow_file(task_id);
        let persisted = if path.exists() {
            let compat: TaskAuthoringWorkflowCompat = read_json(&path)?;
            Some(compat.into_current().0)
        } else {
            None
        };
        reconcile_authoring_workflow_for_save(
            &mut authoring.workflow,
            &mut authoring.model_bindings,
            persisted.as_ref(),
            None,
        )?;
        write_json(&path, &authoring)
    }

    fn load_auto_template_store(&self) -> Result<AutoTemplateStore> {
        let path = self.paths.auto_templates_file();
        if path.exists() {
            return read_json(&path);
        }
        let store = AutoTemplateStore {
            version: VERSION.to_string(),
            templates: Vec::new(),
        };
        self.save_auto_template_store(&store)?;
        Ok(store)
    }

    fn save_auto_template_store(&self, store: &AutoTemplateStore) -> Result<()> {
        fs::create_dir_all(self.paths.user_context_dir().as_std_path())?;
        write_json(&self.paths.auto_templates_file(), store)
    }

    fn record_created_task_workflow(
        &self,
        workflow: WorkflowDsl,
        template_id: Option<String>,
    ) -> Result<()> {
        let mut store = self.load_workflow_template_store()?;
        store.last_created_workflow = Some(workflow);
        if let Some(template_id) = template_id.filter(|value| !value.trim().is_empty()) {
            store.last_used_template_id = Some(template_id);
        }
        self.save_workflow_template_store(&store)
    }

    pub fn managed_agent(&self, provider: &str) -> Result<(ManagedAgentId, &ManagedAgentConfig)> {
        let agent_id = ManagedAgentId::from_str(provider)?;
        let config = self
            .config
            .agents
            .get(&agent_id)
            .ok_or_else(|| anyhow!("agent `{provider}` is not configured"))?;
        Ok((agent_id, config))
    }

    pub fn provider_for_id(&self, provider: &str) -> Result<Arc<dyn ProviderAdapter>> {
        if let Some(provider_override) = &self.provider_override {
            return Ok(provider_override.clone());
        }
        let (agent_id, config) = self.managed_agent(provider)?;
        Ok(Arc::from(provider_from_agent(
            &agent_id,
            config,
            self.config.use_local_claude,
            self.config.require_local_claude_executable,
            self.config.acp_session_title_refresh_enabled,
            self.config.acp_raw_max_size_bytes,
            self.config.acp_raw_target_size_bytes,
            acp_client::AcpRuntimePolicy::from(&self.config)
                .with_external_session_sync_enabled(config.external_session_sync_enabled),
        )?))
    }

    pub fn provider_info(&self, provider: &str) -> Result<ProviderInfo> {
        Ok(self.provider_for_id(provider)?.describe_provider())
    }

    pub fn provider_doctor(&self, provider: &str) -> Result<DoctorResult> {
        Ok(self.provider_doctor_probe(provider)?.doctor)
    }

    pub fn provider_doctor_probe(&self, provider: &str) -> Result<ProviderDoctorProbe> {
        let (agent_id, config) = self.managed_agent(provider)?;
        match acp_client::doctor(
            &agent_id,
            &config.adapter,
            self.paths.repo_root.clone(),
            self.config.use_local_claude,
            self.config.require_local_claude_executable,
        ) {
            Ok(probe) => Ok(ProviderDoctorProbe {
                doctor: DoctorResult {
                    available: true,
                    reason: None,
                    capabilities: Some(probe.capabilities),
                },
                commands: probe.commands,
            }),
            Err(err) => Ok(ProviderDoctorProbe {
                doctor: DoctorResult {
                    available: false,
                    reason: Some(err.to_string()),
                    capabilities: None,
                },
                commands: Vec::new(),
            }),
        }
    }

    pub fn provider_capabilities(&self, provider: &str) -> Result<ProviderCapabilities> {
        Ok(self.provider_info(provider)?.capabilities)
    }

    pub fn with_config(repo_root: Utf8PathBuf, config: RuntimeConfig) -> Self {
        let paths = GoldBandPaths::new(repo_root);
        Self::with_config_and_paths(paths, config)
    }

    pub fn with_config_and_path_config(
        repo_root: Utf8PathBuf,
        config: RuntimeConfig,
        path_config: StoragePathConfig,
    ) -> Self {
        let paths = GoldBandPaths::new_with_path_config(repo_root, path_config);
        Self::with_config_and_paths(paths, config)
    }

    fn with_config_and_paths(paths: GoldBandPaths, config: RuntimeConfig) -> Self {
        let _ = ensure_default_user_profiles(&paths);
        Self {
            paths,
            config,
            task_search_indexer: default_task_search_indexer(),
            provider_override: None,
            provider_diagnostics: None,
            acp_live_update: None,
            acp_session_update: None,
            prompt_turn_lifecycle: None,
            lifecycle_bus: observability::RuntimeLifecycleBus::new(),
            observability_states: Arc::new(std::sync::Mutex::new(HashMap::new())),
            metrics_collection_enabled: false,
            active_metric_turns: Arc::new(std::sync::Mutex::new(HashMap::new())),
            scheduled_occurrence_id: None,
            scheduled_task_context: None,
            runtime_recovery: None,
        }
    }

    pub fn with_provider(repo_root: Utf8PathBuf, provider: Box<dyn ProviderAdapter>) -> Self {
        Self::with_provider_config(repo_root, RuntimeConfig::default(), provider)
    }

    pub fn with_provider_config(
        repo_root: Utf8PathBuf,
        config: RuntimeConfig,
        provider: Box<dyn ProviderAdapter>,
    ) -> Self {
        let mut app = Self::with_config(repo_root, config);
        app.provider_override = Some(Arc::from(provider));
        app
    }

    pub fn task_show(&self, task_id: &str) -> Result<TaskState> {
        let task: TaskState = read_json(&self.paths.task_file(task_id))?;
        validate_task_state(&task)?;
        Ok(task)
    }

    pub fn task_list(&self) -> Result<Vec<TaskState>> {
        let mut tasks: Vec<TaskState> = self.read_json_dir_sorted(&self.paths.tasks_dir())?;
        for task in &tasks {
            validate_task_state(task)?;
        }
        tasks.sort_by(|left, right| right.id.cmp(&left.id));
        Ok(tasks)
    }

    pub fn create_task_from_requirement(&self, input: CreateTaskInput) -> Result<TaskSummary> {
        if input.requirement_content.trim().is_empty() {
            bail!("requirement content cannot be empty");
        }

        let mut workflow = input.workflow.clone();
        let mut model_bindings = WorkflowModelBindings::default();
        migrate_authoring_workflow(&mut workflow, &mut model_bindings, None)?;
        self.create_task_from_requirement_with_bindings(input, workflow, model_bindings)
    }

    pub fn create_task_from_requirement_with_bindings(
        &self,
        input: CreateTaskInput,
        workflow: WorkflowDsl,
        model_bindings: WorkflowModelBindings,
    ) -> Result<TaskSummary> {
        self.create_task_from_payload_with_bindings(input, workflow, model_bindings, true)
    }

    pub fn create_conversation_task_from_payload_with_bindings(
        &self,
        input: CreateTaskInput,
        workflow: WorkflowDsl,
        model_bindings: WorkflowModelBindings,
    ) -> Result<TaskSummary> {
        self.create_task_from_payload_with_bindings(input, workflow, model_bindings, false)
    }

    fn create_task_from_payload_with_bindings(
        &self,
        input: CreateTaskInput,
        mut workflow: WorkflowDsl,
        mut model_bindings: WorkflowModelBindings,
        require_text: bool,
    ) -> Result<TaskSummary> {
        if require_text && input.requirement_content.trim().is_empty() {
            bail!("requirement content cannot be empty");
        }
        migrate_authoring_workflow(&mut workflow, &mut model_bindings, None)?;
        let validated = validate_authoring_workflow(workflow)?;
        resolve_workflow_profiles(&self.paths, &validated.raw, self.config.desktop_language)?;
        let store = self.load_workflow_template_store()?;
        let selected_template = input
            .workflow_template_id
            .as_deref()
            .and_then(|template_id| {
                store
                    .templates
                    .iter()
                    .find(|template| template.id == template_id)
            });
        if let Some(template) = selected_template {
            validate_unique_workflow_template_id(
                &store,
                &template.workflow,
                &template.name,
                Some(template.id.as_str()),
            )?;
        }
        validate_ai_dynamic_allowed_workflows(&validated.raw, &store)?;

        let (task_id, task_dir) = reserve_next_task_dir(&self.paths.tasks_dir())?;
        let mut owned_task_dir = OwnedTaskDirectory::new(task_dir.clone());
        let task = TaskState {
            version: VERSION.to_string(),
            id: task_id.clone(),
            title: input.title.filter(|value| !value.trim().is_empty()),
            description: input.description.filter(|value| !value.trim().is_empty()),
            uuid: Some(generate_uuid()),
        };
        validate_task_state(&task)?;
        fs::create_dir_all(task_dir.join("authoring").as_std_path())?;
        write_json(&self.paths.task_file(&task_id), &task)?;
        fs::write(
            self.paths.requirement_file(&task_id).as_std_path(),
            input.requirement_content,
        )?;
        self.save_task_authoring_workflow(
            &task_id,
            TaskAuthoringWorkflow {
                workflow: validated.raw.clone(),
                model_bindings,
            },
        )?;
        self.record_created_task_workflow(validated.raw, input.workflow_template_id)?;
        let summary = self.task_summary(&task_id)?;
        owned_task_dir.disarm();
        (self.task_search_indexer)(&self.paths.task_dir(&task_id), &task_id);
        Ok(summary)
    }

    pub fn update_task_metadata(
        &self,
        task_id: &str,
        title: &str,
        description: Option<&str>,
    ) -> Result<()> {
        let mut task = self.task_show(task_id)?;
        task.title = Some(title.to_string());
        if let Some(description) = description {
            task.description = Some(description.to_string());
        }
        validate_task_state(&task)?;
        write_json(&self.paths.task_file(task_id), &task)?;
        (self.task_search_indexer)(&self.paths.task_dir(task_id), task_id);
        Ok(())
    }

    pub fn save_task_workflow(&self, task_id: &str, workflow: WorkflowDsl) -> Result<TaskSummary> {
        let mut workflow = workflow;
        let mut model_bindings = WorkflowModelBindings::default();
        migrate_authoring_workflow(&mut workflow, &mut model_bindings, None)?;
        self.save_task_workflow_with_bindings(task_id, workflow, model_bindings)
    }

    pub fn save_task_workflow_with_bindings(
        &self,
        task_id: &str,
        mut workflow: WorkflowDsl,
        mut model_bindings: WorkflowModelBindings,
    ) -> Result<TaskSummary> {
        self.task_show(task_id)?;
        migrate_authoring_workflow(&mut workflow, &mut model_bindings, None)?;
        let validated = validate_authoring_workflow(workflow)?;
        resolve_workflow_profiles(&self.paths, &validated.raw, self.config.desktop_language)?;
        let store = self.load_workflow_template_store()?;
        validate_ai_dynamic_allowed_workflows(&validated.raw, &store)?;
        fs::create_dir_all(self.paths.task_dir(task_id).join("authoring").as_std_path())?;
        self.save_task_authoring_workflow(
            task_id,
            TaskAuthoringWorkflow {
                workflow: validated.raw,
                model_bindings,
            },
        )?;
        self.task_summary(task_id)
    }

    pub fn task_summaries(&self) -> Result<Vec<TaskSummary>> {
        let mut summaries = self
            .task_list()?
            .into_iter()
            .map(|task| self.task_summary(&task.id))
            .collect::<Result<Vec<_>>>()?;
        summaries.sort_by(|left, right| right.task.id.cmp(&left.task.id));
        Ok(summaries)
    }

    pub fn task_summary(&self, task_id: &str) -> Result<TaskSummary> {
        let task = self.task_show(task_id)?;
        let workflow_exists = self.paths.workflow_file(task_id).exists();
        let (workflow_error, workflow_validation_error) =
            self.workflow_validation_error(task_id)?;
        let workflow_valid = workflow_exists && workflow_error.is_none();
        let latest_run = self.latest_run(task_id)?;
        let resumable_run_id = self.find_resumable_run_id(task_id)?;
        let suggested_run_id = self.find_active_or_resumable_run_id(task_id)?;
        Ok(TaskSummary {
            task,
            workflow_exists,
            workflow_valid,
            workflow_error,
            workflow_validation_error,
            latest_run,
            resumable_run_id,
            suggested_run_id,
        })
    }

    pub fn run_list(&self, task_id: &str) -> Result<Vec<RunState>> {
        let mut runs: Vec<RunState> = self.read_json_dir_sorted(&self.paths.runs_dir(task_id))?;
        for run in &mut runs {
            if run.reconcile_legacy_execution() {
                write_json(&self.paths.run_file(task_id, &run.id), run)?;
            }
            validate_run_state(run)?;
        }
        Ok(runs)
    }

    pub fn latest_run(&self, task_id: &str) -> Result<Option<RunState>> {
        Ok(self.run_list(task_id)?.into_iter().last())
    }

    pub fn round_list(&self, task_id: &str, run_id: &str) -> Result<Vec<RoundState>> {
        self.read_json_dir_sorted_by_file(
            &self.paths.run_dir(task_id, run_id).join("rounds"),
            "round.json",
        )
    }

    pub fn node_list(&self, task_id: &str, run_id: &str, round_id: &str) -> Result<Vec<NodeState>> {
        let nodes_dir = self
            .paths
            .round_dir(task_id, run_id, round_id)
            .join("nodes");
        let mut nodes = Vec::new();
        if !nodes_dir.exists() {
            return Ok(nodes);
        }

        let mut node_dirs = fs::read_dir(nodes_dir.as_std_path())?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        node_dirs.sort();

        for node_dir in node_dirs {
            if !node_dir.is_dir() {
                continue;
            }
            let mut attempt_dirs = fs::read_dir(&node_dir)?
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .collect::<Vec<_>>();
            attempt_dirs.sort();
            if let Some(latest_attempt_dir) =
                attempt_dirs.into_iter().rev().find(|path| path.is_dir())
            {
                let node_file = latest_attempt_dir.join("node.json");
                if node_file.exists() {
                    let utf8 = Utf8PathBuf::from_path_buf(node_file)
                        .map_err(|_| anyhow!("path is not valid UTF-8"))?;
                    let node: NodeState = read_json(&utf8)?;
                    validate_node_state(&node)?;
                    nodes.push(node);
                }
            }
        }
        Ok(nodes)
    }

    pub fn attempt_list(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
    ) -> Result<Vec<NodeState>> {
        let mut attempts: Vec<NodeState> = self.read_json_dir_sorted_by_file(
            &self.paths.node_dir(task_id, run_id, round_id, node_id),
            "node.json",
        )?;
        for attempt in &attempts {
            validate_node_state(attempt)?;
        }
        attempts.sort_by(|left, right| left.attempt_id.cmp(&right.attempt_id));
        Ok(attempts)
    }

    pub fn attachment_list(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Result<Vec<String>> {
        let dir = self
            .paths
            .attachments_dir(task_id, run_id, round_id, node_id, attempt_id);
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

    pub fn attachment_show(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        name: &str,
    ) -> Result<String> {
        let path = self
            .paths
            .attachments_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join(name);
        self.artifact_show_path(path.as_path())
    }

    pub fn run_progress(&self, task_id: &str, run_id: &str) -> Result<Option<serde_json::Value>> {
        self.read_optional_json_value(&self.paths.run_progress_file(task_id, run_id))
    }

    pub fn run_events(&self, task_id: &str, run_id: &str) -> Result<Option<String>> {
        self.read_optional_text(&self.paths.run_events_file(task_id, run_id))
    }

    pub fn attempt_progress_events(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Result<Option<String>> {
        self.read_optional_text(
            &self
                .paths
                .progress_events_file(task_id, run_id, round_id, node_id, attempt_id),
        )
    }

    pub fn attempt_raw_stream(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Result<Option<String>> {
        self.read_optional_text(
            &self
                .paths
                .raw_stream_file(task_id, run_id, round_id, node_id, attempt_id),
        )
    }

    pub fn workflow_snapshot_show(&self, task_id: &str, run_id: &str) -> Result<Option<String>> {
        self.read_optional_text(&self.paths.workflow_snapshot_file(task_id, run_id))
    }

    pub fn worker_ref_show(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Result<Option<String>> {
        let path = self
            .paths
            .worker_ref_file(task_id, run_id, round_id, node_id, attempt_id);
        if !path.exists() {
            return Ok(None);
        }
        let worker_ref: WorkerRefState = read_json(&path)?;
        validate_worker_ref_state(&worker_ref)?;
        Ok(Some(serde_json::to_string_pretty(&worker_ref)?))
    }

    pub fn runtime_log_show(&self) -> Result<Option<String>> {
        self.read_optional_text(&self.paths.runtime_log_file())
    }

    pub fn runtime_log_tail_show(&self, limit: usize) -> Result<Option<String>> {
        let path = self.paths.runtime_log_file();
        if !path.exists() {
            return Ok(None);
        }
        if limit == 0 {
            return Ok(Some(String::new()));
        }

        let mut file = fs::File::open(path.as_std_path())?;
        let file_len = file.metadata()?.len();
        if file_len == 0 {
            return Ok(Some(String::new()));
        }

        let mut position = file_len;
        let mut chunks = Vec::new();
        let mut newline_count = 0usize;
        let mut buffer = [0u8; 8192];

        while position > 0 && newline_count <= limit {
            let read_len = position.min(buffer.len() as u64) as usize;
            position -= read_len as u64;
            file.seek(SeekFrom::Start(position))?;
            file.read_exact(&mut buffer[..read_len])?;
            newline_count += buffer[..read_len]
                .iter()
                .filter(|&&byte| byte == b'\n')
                .count();
            chunks.push(buffer[..read_len].to_vec());
        }

        chunks.reverse();
        let text = String::from_utf8(chunks.concat())?;
        let normalized = text.strip_suffix('\n').unwrap_or(&text);
        let lines = normalized.lines().collect::<Vec<_>>();
        let start = lines.len().saturating_sub(limit);
        Ok(Some(lines[start..].join("\n")))
    }

    pub fn attempt_log(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        source: LogSource,
    ) -> Result<Option<String>> {
        match source {
            LogSource::ProgressEvents => {
                self.attempt_progress_events(task_id, run_id, round_id, node_id, attempt_id)
            }
            LogSource::RawStream => {
                self.attempt_raw_stream(task_id, run_id, round_id, node_id, attempt_id)
            }
        }
    }

    pub fn attempt_log_exists(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        source: LogSource,
    ) -> bool {
        match source {
            LogSource::ProgressEvents => self
                .paths
                .progress_events_file(task_id, run_id, round_id, node_id, attempt_id)
                .exists(),
            LogSource::RawStream => self
                .paths
                .raw_stream_file(task_id, run_id, round_id, node_id, attempt_id)
                .exists(),
        }
    }

    pub fn attempt_log_tail(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        source: LogSource,
        limit: usize,
    ) -> Result<Option<String>> {
        Ok(self
            .attempt_log(task_id, run_id, round_id, node_id, attempt_id, source)?
            .map(|content| tail_text(&content, limit)))
    }

    pub fn provider_output(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Result<Option<String>> {
        if let Some(progress) = self.attempt_log(
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            LogSource::ProgressEvents,
        )? {
            return Ok(Some(progress));
        }
        self.attempt_log(
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            LogSource::RawStream,
        )
    }

    pub fn current_attempt_selection(
        &self,
        task_id: &str,
        run_id: &str,
    ) -> Result<Option<(String, String, String)>> {
        let run = self.run_status(task_id, run_id)?;
        match (run.current_round, run.current_node, run.current_attempt) {
            (Some(round_id), Some(node_id), Some(attempt_id)) => {
                Ok(Some((round_id, node_id, attempt_id)))
            }
            _ => Ok(None),
        }
    }

    pub fn node_runtime_summary(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        workflow: &WorkflowDsl,
        node_id: &str,
    ) -> Result<NodeRuntimeSummary> {
        let attempts = self.attempt_list(task_id, run_id, round_id, node_id)?;
        let latest_attempt = attempts.last().cloned();
        let outgoing_edges = workflow
            .edges
            .iter()
            .filter(|edge| edge.from == node_id)
            .map(|edge| NodeEdgeSummary {
                to: edge.to.clone(),
                on: edge.on,
            })
            .collect::<Vec<_>>();
        Ok(NodeRuntimeSummary {
            latest_attempt,
            attempts,
            outgoing_edges,
        })
    }

    pub fn artifact_show_path(&self, path: &Utf8Path) -> Result<String> {
        Ok(fs::read_to_string(path)?)
    }

    pub fn artifact_show(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        name: &str,
    ) -> Result<String> {
        let artifact_name = logical_artifact_name(name);
        let path = self.paths.artifact_file(
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            artifact_name,
        );
        self.artifact_show_path(&path)
    }

    pub fn artifact_list(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Result<Vec<String>> {
        let dir = self
            .paths
            .artifacts_dir(task_id, run_id, round_id, node_id, attempt_id);
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut names = fs::read_dir(dir.as_std_path())?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| entry.file_name().to_str().map(ToOwned::to_owned))
            .map(|name| logical_artifact_name(&name).to_string())
            .collect::<Vec<_>>();
        names.sort();
        Ok(names)
    }

    pub fn run_status(&self, task_id: &str, run_id: &str) -> Result<RunState> {
        let path = self.paths.run_file(task_id, run_id);
        let mut run: RunState = read_json(&path)?;
        if run.reconcile_legacy_execution() {
            write_json(&path, &run)?;
        }
        validate_run_state(&run)?;
        Ok(run)
    }

    pub fn pause_all_running_sessions(&self) -> Result<Vec<RunState>> {
        let mut paused = Vec::new();
        for task in self.task_list()? {
            let Ok(runs) = self.run_list(&task.id) else {
                continue;
            };
            for run in runs {
                if run.status != RunStatus::Running {
                    continue;
                }
                if let Ok(paused_run) =
                    self.run_pause(&task.id, &run.id, PauseReason::ProcessInterrupted)
                {
                    paused.push(paused_run);
                }
            }
        }
        Ok(paused)
    }

    pub fn stop_all_running_sessions(&self) -> Result<Vec<RunState>> {
        let paused = self.pause_all_running_sessions()?;
        self.close_active_runtime_connections()?;
        Ok(paused)
    }

    pub fn close_active_runtime_connections(&self) -> Result<()> {
        self.cancel_all_active_acp_attempts_best_effort();
        acp_client::close_all_connections_bounded()?;
        Ok(())
    }

    pub fn recover_interrupted_running_sessions(&self) -> Result<Vec<RunState>> {
        let paused = self.pause_all_running_sessions()?;
        self.cancel_all_active_acp_attempts_best_effort();
        Ok(paused)
    }

    fn interrupt_run_descendants_best_effort(
        &self,
        task_id: &str,
        run_id: &str,
        run: &RunState,
        reason: PauseReason,
    ) {
        let (Some(round_id), Some(node_id), Some(attempt_id)) =
            (&run.current_round, &run.current_node, &run.current_attempt)
        else {
            return;
        };
        self.interrupt_attempt_artifacts_best_effort(
            task_id, run_id, round_id, node_id, attempt_id,
        );
        self.update_dynamic_descendants_best_effort(
            task_id, run_id, round_id, node_id, attempt_id, reason,
        );
    }

    fn interrupt_attempt_artifacts_best_effort(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) {
        let attempt_dir = self
            .paths
            .attempt_dir(task_id, run_id, round_id, node_id, attempt_id);
        self.request_attempt_prompt_cancel_best_effort(&attempt_dir);
        self.persist_cancelled_session_snapshot_best_effort(&attempt_dir);
    }

    fn update_dynamic_descendants_best_effort(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        pause_reason: PauseReason,
    ) {
        let graph_path = self
            .paths
            .dynamic_graph_file(task_id, run_id, round_id, node_id, attempt_id);
        let Ok(state_lock) = dynamic_state_lock_for(
            &self.paths.repo_root,
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
        ) else {
            return;
        };
        let _guard = state_lock.lock();
        let Ok(mut graph) = load_dynamic_graph(&graph_path, &self.paths.repo_root) else {
            return;
        };
        let interrupted_nodes = graph
            .nodes
            .iter()
            .map(|dynamic_node| {
                (
                    dynamic_node.id.clone(),
                    dynamic_leaf_is_active(dynamic_node.status),
                    dynamic_node.child_run_id.clone(),
                )
            })
            .collect::<Vec<_>>();

        for dynamic_node in &mut graph.nodes {
            if dynamic_node.status != DynamicNodeStatus::Completed {
                dynamic_node.status = DynamicNodeStatus::Paused;
                dynamic_node.outcome = None;
                dynamic_node.pause_reason = Some(pause_reason);
                dynamic_node.runtime_error = None;
                let now = now_rfc3339_like();
                dynamic_node.pause_runtime_execution(now.clone());
                dynamic_node.finished_at = Some(now);
            }
        }

        graph.run.status = DynamicRunStatus::Paused;
        graph.run.phase = DynamicRunPhase::Executing;
        graph.run.outcome = None;
        graph.run.pause_reason = Some(pause_reason);
        refresh_dynamic_current_leaf_ids(&mut graph);
        graph.run.updated_at = now_rfc3339_like();
        let _ = write_json(&graph_path, &graph);
        let _ = write_json(
            &self
                .paths
                .dynamic_run_file(task_id, run_id, round_id, node_id, attempt_id),
            &graph.run,
        );
        for dynamic_node in &graph.nodes {
            let _ = write_dynamic_node_state(
                &self.paths.dynamic_node_file(
                    task_id,
                    run_id,
                    round_id,
                    node_id,
                    attempt_id,
                    &dynamic_node.id,
                ),
                dynamic_node,
            );
        }
        drop(_guard);

        for (dynamic_node_id, was_active, child_run_id) in interrupted_nodes {
            if was_active {
                let dynamic_node_dir = self.paths.dynamic_node_dir(
                    task_id,
                    run_id,
                    round_id,
                    node_id,
                    attempt_id,
                    &dynamic_node_id,
                );
                if let Ok(entries) = fs::read_dir(dynamic_node_dir.as_std_path()) {
                    for entry in entries.flatten() {
                        let attempt_path = entry.path();
                        if !attempt_path.is_dir() {
                            continue;
                        }
                        let Ok(attempt_dir) = Utf8PathBuf::from_path_buf(attempt_path) else {
                            continue;
                        };
                        self.request_attempt_prompt_cancel_best_effort(attempt_dir.as_path());
                        self.persist_cancelled_session_snapshot_best_effort(attempt_dir.as_path());
                    }
                }
            }
            if let Some(child_run_id) = child_run_id {
                let _ = self.run_pause(task_id, &child_run_id, pause_reason);
            }
        }
    }

    pub fn run_pause(&self, task_id: &str, run_id: &str, reason: PauseReason) -> Result<RunState> {
        loop {
            let observed = self.run_status(task_id, run_id)?;
            if observed.status != RunStatus::Running {
                self.finish_runtime_candidate_best_effort(
                    task_id,
                    run_id,
                    observed.execution.recovery_candidate_token.as_deref(),
                );
                return Ok(observed);
            }
            let (Some(round_id), Some(node_id), Some(attempt_id)) = (
                observed.current_round.clone(),
                observed.current_node.clone(),
                observed.current_attempt.clone(),
            ) else {
                return Err(anyhow!("running run has no current attempt locator"));
            };
            let state_lock =
                attempt_runtime_state_lock(self, task_id, run_id, &round_id, &node_id, &attempt_id);
            let guard = state_lock
                .lock()
                .map_err(|_| anyhow!("attempt runtime state lock poisoned"))?;
            let mut run = self.run_status(task_id, run_id)?;
            if run.status != RunStatus::Running {
                drop(guard);
                self.finish_runtime_candidate_best_effort(
                    task_id,
                    run_id,
                    run.execution.recovery_candidate_token.as_deref(),
                );
                return Ok(run);
            }
            if run.current_round.as_deref() != Some(round_id.as_str())
                || run.current_node.as_deref() != Some(node_id.as_str())
                || run.current_attempt.as_deref() != Some(attempt_id.as_str())
            {
                drop(guard);
                continue;
            }

            let now = now_rfc3339_like();
            run.status = RunStatus::Paused;
            run.outcome = None;
            run.pause_reason = Some(reason);
            run.updated_at = now.clone();
            run.transition_current_execution(RuntimeExecutionPhase::Paused, now.clone())?;
            validate_run_state(&run)?;
            write_json(&self.paths.run_file(task_id, run_id), &run)?;

            let round_path = self.paths.round_file(task_id, run_id, &round_id);
            let mut round: RoundState = read_json(&round_path)?;
            round.status = RunStatus::Paused;
            round.outcome = None;
            validate_round_state(&round)?;
            write_json(&round_path, &round)?;

            let node_path = self
                .paths
                .node_file(task_id, run_id, &round_id, &node_id, &attempt_id);
            if node_path.exists() {
                let mut node: NodeState = read_json(&node_path)?;
                if node.status != RunStatus::Completed {
                    node.status = RunStatus::Paused;
                    node.outcome = None;
                    node.runtime_execution_id = None;
                    node.finished_at = Some(now);
                    validate_node_state(&node)?;
                    write_node_state(&node_path, &node)?;
                }
            }
            drop(guard);

            self.interrupt_run_descendants_best_effort(task_id, run_id, &run, reason);
            self.finish_runtime_candidate_best_effort(
                task_id,
                run_id,
                run.execution.recovery_candidate_token.as_deref(),
            );
            return Ok(run);
        }
    }

    pub fn pause_attempt_runtime_state(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        reason: PauseReason,
    ) -> Result<()> {
        self.pause_attempt_runtime_state_with_policy(
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            reason,
            AttemptRuntimePausePolicy::CurrentAttempt,
        )
        .map(|_| ())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn pause_attempt_runtime_state_if_active_execution(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        execution_id: &str,
        reason: PauseReason,
    ) -> Result<AttemptRuntimePauseResult> {
        self.pause_attempt_runtime_state_with_policy(
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            reason,
            AttemptRuntimePausePolicy::ActiveExecution(execution_id),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn pause_attempt_runtime_state_if_active_without_execution(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        reason: PauseReason,
    ) -> Result<AttemptRuntimePauseResult> {
        self.pause_attempt_runtime_state_with_policy(
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            reason,
            AttemptRuntimePausePolicy::ActiveAttemptWithoutExecution,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn pause_attempt_runtime_state_if_paused_manual_check(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        reason: PauseReason,
    ) -> Result<AttemptRuntimePauseResult> {
        self.pause_attempt_runtime_state_with_policy(
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            reason,
            AttemptRuntimePausePolicy::PausedManualCheck,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn pause_attempt_runtime_state_with_policy(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        reason: PauseReason,
        policy: AttemptRuntimePausePolicy<'_>,
    ) -> Result<AttemptRuntimePauseResult> {
        let state_lock =
            attempt_runtime_state_lock(self, task_id, run_id, round_id, node_id, attempt_id);
        let guard = state_lock
            .lock()
            .map_err(|_| anyhow!("attempt runtime state lock poisoned"))?;
        let now = now_rfc3339_like();
        let node_path = self
            .paths
            .node_file(task_id, run_id, round_id, node_id, attempt_id);
        let mut node = if node_path.exists() {
            Some(read_json::<NodeState>(&node_path)?)
        } else {
            None
        };
        let run_path = self.paths.run_file(task_id, run_id);
        let mut run = if run_path.exists() {
            Some(read_json::<RunState>(&run_path)?)
        } else {
            None
        };
        let locator_matches = run.as_ref().is_some_and(|run| {
            run.current_round.as_deref() == Some(round_id)
                && run.current_node.as_deref() == Some(node_id)
                && run.current_attempt.as_deref() == Some(attempt_id)
        });
        let active_attempt = locator_matches
            && run
                .as_ref()
                .is_some_and(|run| run.status == RunStatus::Running);
        let paused_manual_check = locator_matches
            && run
                .as_ref()
                .is_some_and(|run| run.status == RunStatus::Paused)
            && node
                .as_ref()
                .is_some_and(|node| node.status == RunStatus::Paused && node.manual_check_pending);
        let policy_matches = match policy {
            AttemptRuntimePausePolicy::CurrentAttempt => true,
            AttemptRuntimePausePolicy::ActiveExecution(expected_execution_id) => {
                active_attempt
                    && node
                        .as_ref()
                        .and_then(|node| node.runtime_execution_id.as_deref())
                        == Some(expected_execution_id)
            }
            AttemptRuntimePausePolicy::ActiveAttemptWithoutExecution => {
                active_attempt
                    && node
                        .as_ref()
                        .is_some_and(|node| node.runtime_execution_id.is_none())
            }
            AttemptRuntimePausePolicy::PausedManualCheck => paused_manual_check,
        };
        if !policy_matches {
            return Ok(AttemptRuntimePauseResult::Superseded);
        }

        if let Some(run) = run.as_mut() {
            if active_attempt || matches!(policy, AttemptRuntimePausePolicy::PausedManualCheck) {
                run.status = RunStatus::Paused;
                run.outcome = None;
                run.pause_reason = Some(reason);
                run.updated_at = now.clone();
                run.transition_current_execution(RuntimeExecutionPhase::Paused, now.clone())?;
                validate_run_state(&run)?;
                write_json(&run_path, &run)?;
            }
        }

        let round_path = self.paths.round_file(task_id, run_id, round_id);
        if round_path.exists() {
            let mut round: RoundState = read_json(&round_path)?;
            if round.status == RunStatus::Running {
                round.status = RunStatus::Paused;
                validate_round_state(&round)?;
                write_json(&round_path, &round)?;
            }
        }

        if let Some(node) = node.as_mut() {
            if node.status != RunStatus::Completed {
                node.status = RunStatus::Paused;
                node.outcome = None;
                node.runtime_execution_id = None;
                node.finished_at = Some(now);
                validate_node_state(&node)?;
                write_node_state(&node_path, &node)?;
            }
        }

        let run_became_inactive =
            active_attempt || matches!(policy, AttemptRuntimePausePolicy::PausedManualCheck);
        let recovery_candidate_token = run
            .as_ref()
            .and_then(|run| run.execution.recovery_candidate_token.clone());
        drop(guard);
        if run_became_inactive {
            self.finish_runtime_candidate_best_effort(
                task_id,
                run_id,
                recovery_candidate_token.as_deref(),
            );
        }
        Ok(AttemptRuntimePauseResult::Converged)
    }

    pub fn pause_dynamic_attempt_runtime_state(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        outer_node_id: &str,
        outer_attempt_id: &str,
        node_id: &str,
        reason: PauseReason,
    ) -> Result<()> {
        pause_dynamic_leaf_runtime_state(
            self,
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            node_id,
            reason,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn pause_dynamic_attempt_runtime_state_if_active_execution(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        outer_node_id: &str,
        outer_attempt_id: &str,
        node_id: &str,
        execution_id: &str,
        reason: PauseReason,
    ) -> Result<bool> {
        pause_dynamic_leaf_runtime_state_if_active_execution(
            self,
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            node_id,
            execution_id,
            reason,
        )
    }

    pub fn request_attempt_prompt_cancel_best_effort(&self, attempt_dir: &Utf8Path) {
        let _ = acp_client::cancel_attempt_prompt(attempt_dir);
    }

    pub fn kill_provider_pid_file_best_effort(&self, pid_path: &Utf8Path) {
        let Ok(pid_text) = fs::read_to_string(pid_path.as_std_path()) else {
            return;
        };
        let Ok(pid) = pid_text.trim().parse::<u32>() else {
            return;
        };
        let _ = recover_persisted_process_group(pid);
        let _ = fs::remove_file(pid_path.as_std_path());
    }

    pub fn persist_cancelled_session_snapshot_best_effort(&self, attempt_dir: &Utf8Path) {
        let _ = self.persist_cancelled_session_snapshot(attempt_dir);
    }

    pub fn persist_cancelled_session_snapshot(&self, attempt_dir: &Utf8Path) -> Result<()> {
        self.persist_cancelled_session_file(&attempt_dir.join("acp.snapshot.json"))?;
        self.persist_cancelled_session_file(&attempt_dir.join("acp.session.json"))?;
        Ok(())
    }

    fn persist_cancelled_session_file(&self, path: &Utf8Path) -> Result<()> {
        let mut session = if path.exists() {
            read_json::<serde_json::Value>(path)?
        } else {
            let session_id = path
                .parent()
                .and_then(|attempt_dir| attempt_dir.file_name())
                .unwrap_or("session");
            serde_json::json!({
                "sessionId": session_id,
                "availability": "established",
                "latestTurnStatus": "cancelled",
                "restored": false,
                "createdAt": crate::acp::events::current_timestamp(),
            })
        };
        let now = crate::acp::events::current_timestamp();
        if let Some(object) = session.as_object_mut() {
            object.remove("status");
        }
        session["availability"] = serde_json::json!("established");
        session["latestTurnStatus"] = serde_json::json!("cancelled");
        session["stopReason"] = serde_json::json!("cancelled");
        session["updatedAt"] = serde_json::json!(now.clone());
        if session.get("updated_at").is_some() {
            session["updated_at"] = serde_json::json!(now);
        }
        ensure_parent_dir(path)?;
        write_json(path, &session)
    }

    pub fn cancel_all_active_acp_attempts_best_effort(&self) {
        let Ok(tasks) = self.task_list() else {
            return;
        };
        for task in tasks {
            let Ok(runs) = self.run_list(&task.id) else {
                continue;
            };
            for run in runs {
                let Ok(rounds) = self.round_list(&task.id, &run.id) else {
                    continue;
                };
                for round in rounds {
                    let Ok(nodes) = self.node_list(&task.id, &run.id, &round.id) else {
                        continue;
                    };
                    for node in nodes {
                        let Ok(attempts) =
                            self.attempt_list(&task.id, &run.id, &round.id, &node.node_id)
                        else {
                            continue;
                        };
                        for attempt in attempts {
                            let attempt_dir = self.paths.attempt_dir(
                                &task.id,
                                &run.id,
                                &round.id,
                                &node.node_id,
                                &attempt.attempt_id,
                            );
                            if !attempt_dir.exists()
                                || !self.attempt_has_active_acp_session(attempt_dir.as_path())
                            {
                                continue;
                            }
                            self.request_attempt_prompt_cancel_best_effort(attempt_dir.as_path());
                            self.persist_cancelled_session_snapshot_best_effort(
                                attempt_dir.as_path(),
                            );
                        }
                    }
                }
            }
        }
    }

    fn attempt_has_active_acp_session(&self, attempt_dir: &Utf8Path) -> bool {
        if acp_client::prompt_activity(attempt_dir).is_some() {
            return true;
        }
        let path = ["acp.snapshot.json", "acp.session.json"]
            .into_iter()
            .map(|name| attempt_dir.join(name))
            .find(|path| path.exists());
        path.and_then(|path| crate::acp::events::load_session_metadata_value(&path, None).ok())
            .is_some_and(|metadata| {
                metadata
                    .get("sessionId")
                    .and_then(serde_json::Value::as_str)
                    .is_some()
                    && metadata
                        .get("latestTurnStatus")
                        .and_then(serde_json::Value::as_str)
                        == Some("none")
            })
    }

    pub fn run_open_session(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Result<String> {
        let worker_ref: WorkerRefState = read_json(
            &self
                .paths
                .worker_ref_file(task_id, run_id, round_id, node_id, attempt_id),
        )?;
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
        self.provider_for_id(&worker_ref.provider)?
            .build_continue_command(&session_ref)?
            .ok_or_else(|| anyhow!("provider did not return an open-session command"))
    }

    pub fn prepare_acp_prompt_for_attempt(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        prompt: String,
        prompt_id: Option<String>,
        continue_ref: Option<serde_json::Value>,
    ) -> Result<PreparedAcpPrompt> {
        let workflow = self::state_access::load_run_workflow(self, task_id, run_id)?;
        let validated = validate_workflow_snapshot(workflow)?;
        self.validate_workflow_agents(&validated)?;
        let round: RoundState = read_json(&self.paths.round_file(task_id, run_id, round_id))?;
        let node: NodeState = read_json(
            &self
                .paths
                .node_file(task_id, run_id, round_id, node_id, attempt_id),
        )?;
        validate_round_state(&round)?;
        validate_node_state(&node)?;
        let run: RunState = read_json(&self.paths.run_file(task_id, run_id))?;
        validate_run_state(&run)?;
        let mut invocation = self::node_executor::build_worker_invocation(
            self,
            task_id,
            run_id,
            &round,
            attempt_id,
            &validated,
            node_id,
            SessionMode::Continue,
            continue_ref,
            Some(prompt),
            prompt_id,
            None,
            PromptVisibility::Visible,
            UserPromptRenderMode::UserMessage,
            Vec::new(),
            None,
            None,
        )?;
        invocation.turn_control_mode = crate::domain::TurnControlMode::NonRuntimeControlled;
        invocation.runtime_control_intent = crate::provider::RuntimeControlIntent::ManualFollowUp;
        invocation.extra_hidden_sections.clear();
        Ok(PreparedAcpPrompt {
            prompt: render_prompt_bundle(&invocation)?,
            adapter_workspace_dir: invocation.adapter_workspace_dir,
            session_workspace_dir: invocation.workspace_dir,
        })
    }

    pub fn prepare_dynamic_acp_prompt_for_attempt(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        outer_node_id: &str,
        outer_attempt_id: &str,
        dynamic_node_id: &str,
        dynamic_attempt_id: &str,
        prompt: String,
        prompt_id: Option<String>,
        continue_ref: Option<serde_json::Value>,
    ) -> Result<PreparedAcpPrompt> {
        prepare_dynamic_acp_prompt(
            self,
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            dynamic_node_id,
            dynamic_attempt_id,
            prompt,
            prompt_id,
            continue_ref,
        )
    }

    pub fn run_continue(
        &self,
        task_id: &str,
        run_id: &str,
        prompt_id: Option<String>,
        prompt: Option<String>,
    ) -> Result<RunState> {
        orchestrator_run_continue(
            self,
            task_id,
            run_id,
            prompt_id,
            prompt,
            Vec::new(),
            None,
            None,
        )
    }

    pub fn run_continue_with_prompt_input(
        &self,
        task_id: &str,
        run_id: &str,
        prompt_id: Option<String>,
        input: Option<ConversationPromptInput>,
    ) -> Result<RunState> {
        orchestrator_run_continue_with_prompt_input(self, task_id, run_id, prompt_id, input)
    }

    pub fn run_continue_with_model_override(
        &self,
        task_id: &str,
        run_id: &str,
        prompt_id: Option<String>,
        prompt: Option<String>,
        model_override: Option<String>,
    ) -> Result<RunState> {
        orchestrator_run_continue(
            self,
            task_id,
            run_id,
            prompt_id,
            prompt,
            Vec::new(),
            model_override,
            None,
        )
    }

    pub fn run_continue_with_config_overrides(
        &self,
        task_id: &str,
        run_id: &str,
        prompt_id: Option<String>,
        prompt: Option<String>,
        model_override: Option<String>,
        permission_mode_override: Option<String>,
    ) -> Result<RunState> {
        orchestrator_run_continue(
            self,
            task_id,
            run_id,
            prompt_id,
            prompt,
            Vec::new(),
            model_override,
            permission_mode_override,
        )
    }

    pub fn run_continue_background(
        &self,
        task_id: &str,
        run_id: &str,
        prompt_id: Option<String>,
        prompt: Option<String>,
    ) -> Result<RunState> {
        orchestrator_run_continue_background(
            self,
            task_id,
            run_id,
            prompt_id,
            prompt.map(ConversationPromptInput::from),
            Vec::new(),
            None,
            None,
        )
    }

    pub fn run_continue_background_with_model_override(
        &self,
        task_id: &str,
        run_id: &str,
        prompt_id: Option<String>,
        prompt: Option<String>,
        model_override: Option<String>,
    ) -> Result<RunState> {
        orchestrator_run_continue_background(
            self,
            task_id,
            run_id,
            prompt_id,
            prompt.map(ConversationPromptInput::from),
            Vec::new(),
            model_override,
            None,
        )
    }

    pub fn run_continue_background_with_config_overrides(
        &self,
        task_id: &str,
        run_id: &str,
        prompt_id: Option<String>,
        input: Option<ConversationPromptInput>,
        attachment_paths: Vec<String>,
        model_override: Option<String>,
        permission_mode_override: Option<String>,
    ) -> Result<RunState> {
        orchestrator_run_continue_background(
            self,
            task_id,
            run_id,
            prompt_id,
            input,
            attachment_paths,
            model_override,
            permission_mode_override,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn run_recover_completed_background(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        expected_revision: u64,
    ) -> Result<RunState> {
        orchestrator_run_recover_completed_background(
            self,
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            expected_revision,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn run_continue_dynamic_inner_background(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        outer_node_id: &str,
        outer_attempt_id: &str,
        dynamic_node_id: &str,
        dynamic_attempt_id: &str,
        prompt_id: Option<String>,
        input: Option<ConversationPromptInput>,
        attachment_paths: Vec<String>,
        model_override: Option<String>,
        permission_mode_override: Option<String>,
    ) -> Result<RunState> {
        orchestrator::run_continue_dynamic_inner_background(
            self,
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            dynamic_node_id,
            dynamic_attempt_id,
            prompt_id,
            input,
            attachment_paths,
            model_override,
            permission_mode_override,
        )
    }

    pub fn submit_manual_check(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        outcome: NodeOutcome,
    ) -> Result<RunState> {
        orchestrator_submit_manual_check(
            self, task_id, run_id, round_id, node_id, attempt_id, outcome,
        )
    }

    pub fn validate_manual_check_submission(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Result<()> {
        orchestrator_validate_manual_check_submission(
            self, task_id, run_id, round_id, node_id, attempt_id,
        )
    }

    pub fn reserve_manual_check_submission(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Result<ManualCheckSubmissionLease> {
        orchestrator_reserve_manual_check_submission(
            self, task_id, run_id, round_id, node_id, attempt_id,
        )
    }

    pub fn submit_manual_check_background(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        outcome: NodeOutcome,
        lease: ManualCheckSubmissionLease,
    ) -> Result<RunState> {
        orchestrator_submit_manual_check_background(
            self, task_id, run_id, round_id, node_id, attempt_id, outcome, lease,
        )
    }

    pub fn run_retry(&self, task_id: &str, run_id: &str) -> Result<RunState> {
        orchestrator_run_retry(self, task_id, run_id)
    }

    pub fn run_start(
        &self,
        task_id: &str,
        workflow_override: Option<&Utf8Path>,
    ) -> Result<RunState> {
        orchestrator_run_start(self, task_id, workflow_override)
    }

    pub fn run_start_background(
        &self,
        task_id: &str,
        workflow_override: Option<&Utf8Path>,
    ) -> Result<RunState> {
        orchestrator_run_start_background(self, task_id, workflow_override)
    }

    pub fn prepare_run(
        &self,
        task_id: &str,
        workflow_override: Option<&Utf8Path>,
    ) -> Result<PreparedRun> {
        orchestrator_prepare_run(self, task_id, workflow_override)
    }

    pub fn prepare_run_in_worktree(
        &self,
        task_id: &str,
        workflow_override: Option<&Utf8Path>,
    ) -> Result<PreparedRun> {
        orchestrator_prepare_run_in_worktree(self, task_id, workflow_override)
    }

    pub fn prepare_run_with_authoring(
        &self,
        task_id: &str,
        authoring: &TaskAuthoringWorkflow,
    ) -> Result<PreparedRun> {
        orchestrator_prepare_run_with_authoring(self, task_id, authoring)
    }

    pub fn launch_prepared_run_background(
        &self,
        task_id: &str,
        prepared: AcceptedRun,
    ) -> Result<RunState> {
        orchestrator_launch_prepared_run_background(self, task_id, prepared)
    }

    pub fn validate_workflow_node_agent_options(&self, node: &NodeDsl) -> Result<()> {
        let provider_diagnostics = self.provider_diagnostics();
        for (provider, permission_mode) in configured_permission_modes_for_node(node) {
            if let Some(permission_mode) = permission_mode {
                let permission_mode = permission_mode.trim();
                if permission_mode.is_empty() {
                    continue;
                }
                let supported_modes = provider_diagnostics
                    .get(&provider)
                    .filter(|diagnostic| diagnostic.available)
                    .map(|diagnostic| {
                        supported_modes_from_capabilities(diagnostic.capabilities.as_ref())
                    })
                    .unwrap_or_default();
                if !supported_modes.is_empty()
                    && !supported_modes
                        .iter()
                        .any(|option| option.id == permission_mode)
                {
                    bail!(
                        "worker permissionMode `{permission_mode}` is not supported by provider `{provider}`"
                    );
                }
            }
        }
        Ok(())
    }

    pub fn validate_workflow_agents(&self, workflow: &ValidatedWorkflow) -> Result<()> {
        for node in workflow.nodes_by_id.values() {
            for provider in providers_for_node(node) {
                self.managed_agent(&provider)?;
            }
        }
        for node in workflow.nodes_by_id.values() {
            self.validate_workflow_node_agent_options(node)?;
        }
        for edge in &workflow.raw.edges {
            if matches!(edge.session, Some(crate::domain::SessionMode::Continue)) {
                let target = workflow.get_node(&edge.to).ok_or_else(|| {
                    anyhow!("session=continue requires a real node target: {}", edge.to)
                })?;
                let provider = target
                    .provider()
                    .ok_or_else(|| anyhow!("target node `{}` is missing provider", edge.to))?;
                if !self
                    .provider_capabilities(provider)?
                    .supports_continue_session
                {
                    bail!(
                        "session=continue currently only supports agents with continue-session capability: {provider}"
                    );
                }
            }
        }
        Ok(())
    }

    pub fn decide(
        &self,
        workflow: WorkflowDsl,
        run: &RunState,
        round: &RoundState,
        node: &NodeState,
    ) -> Result<ControlDecision> {
        let validated = validate_workflow(workflow)?;
        Ok(decide_next_step(&validated, run, round, node))
    }

    fn profile_usage_counts(&self, profile_id: &str) -> Result<ProfileUsageCounts> {
        let mut counts = ProfileUsageCounts::default();
        let store = self.load_workflow_template_store()?;
        counts.template_count = store
            .templates
            .iter()
            .filter(|template| workflow_uses_profile(&template.workflow, profile_id))
            .count();

        let tasks_dir = self.paths.tasks_dir();
        if !tasks_dir.exists() {
            return Ok(counts);
        }

        let mut task_paths = fs::read_dir(tasks_dir.as_std_path())?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        task_paths.sort();

        for path in task_paths {
            if !path.is_dir() {
                continue;
            }
            let Some(task_dir) = Utf8PathBuf::from_path_buf(path).ok() else {
                continue;
            };
            let Some(task_id) = task_dir.file_name() else {
                continue;
            };

            let workflow_path = self.paths.workflow_file(task_id);
            if workflow_path.exists() {
                let workflow = self.task_workflow(task_id)?;
                if workflow_uses_profile(&workflow, profile_id) {
                    counts.task_count += 1;
                }
            }

            let runs_dir = self.paths.runs_dir(task_id);
            if !runs_dir.exists() {
                continue;
            }
            let mut run_paths = fs::read_dir(runs_dir.as_std_path())?
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .collect::<Vec<_>>();
            run_paths.sort();
            for run_path in run_paths {
                if !run_path.is_dir() {
                    continue;
                }
                let Some(run_dir) = Utf8PathBuf::from_path_buf(run_path).ok() else {
                    continue;
                };
                let Some(run_id) = run_dir.file_name() else {
                    continue;
                };
                let run_file = self.paths.run_file(task_id, run_id);
                let snapshot_file = self.paths.workflow_snapshot_file(task_id, run_id);
                if !run_file.exists() || !snapshot_file.exists() {
                    continue;
                }
                let run = read_json::<RunState>(&run_file)?;
                if !self.run_snapshot_is_actionable(task_id, &run)? {
                    continue;
                }
                let workflow = read_json::<WorkflowDsl>(&snapshot_file)?;
                if workflow_uses_profile(&workflow, profile_id) {
                    counts.run_count += 1;
                }
            }
        }

        Ok(counts)
    }

    fn run_snapshot_is_actionable(&self, task_id: &str, run: &RunState) -> Result<bool> {
        if run.status == RunStatus::Running || is_run_continuable(run) {
            return Ok(true);
        }
        let (Some(round_id), Some(node_id), Some(attempt_id)) = (
            run.current_round.as_deref(),
            run.current_node.as_deref(),
            run.current_attempt.as_deref(),
        ) else {
            return Ok(false);
        };
        let node_file = self
            .paths
            .node_file(task_id, &run.id, round_id, node_id, attempt_id);
        if !node_file.exists() {
            return Ok(false);
        }
        let node = read_json::<NodeState>(&node_file)?;
        Ok(node.outcome == Some(NodeOutcome::Invalid))
    }

    fn read_json_dir_sorted<T: DeserializeOwned>(&self, dir: &Utf8Path) -> Result<Vec<T>> {
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut paths = fs::read_dir(dir.as_std_path())?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        paths.sort();

        let mut items = Vec::new();
        for path in paths {
            if path.is_dir() {
                let file = path.join("task.json");
                let run_file = path.join("run.json");
                if file.exists() {
                    let utf8 = Utf8PathBuf::from_path_buf(file)
                        .map_err(|_| anyhow!("path is not valid UTF-8"))?;
                    items.push(read_json(&utf8)?);
                } else if run_file.exists() {
                    let utf8 = Utf8PathBuf::from_path_buf(run_file)
                        .map_err(|_| anyhow!("path is not valid UTF-8"))?;
                    items.push(read_json(&utf8)?);
                }
            }
        }
        Ok(items)
    }

    fn read_json_dir_sorted_by_file<T: DeserializeOwned>(
        &self,
        dir: &Utf8Path,
        file_name: &str,
    ) -> Result<Vec<T>> {
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut paths = fs::read_dir(dir.as_std_path())?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        paths.sort();

        let mut items = Vec::new();
        for path in paths {
            if path.is_dir() {
                let file = path.join(file_name);
                if file.exists() {
                    let utf8 = Utf8PathBuf::from_path_buf(file)
                        .map_err(|_| anyhow!("path is not valid UTF-8"))?;
                    items.push(read_json(&utf8)?);
                }
            }
        }
        Ok(items)
    }

    fn read_optional_text(&self, path: &Utf8Path) -> Result<Option<String>> {
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(fs::read_to_string(path)?))
    }

    fn read_optional_json_value(&self, path: &Utf8Path) -> Result<Option<serde_json::Value>> {
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(read_json(path)?))
    }

    fn workflow_validation_error(
        &self,
        task_id: &str,
    ) -> Result<(Option<String>, Option<WorkflowValidationError>)> {
        let path = self.paths.workflow_file(task_id);
        if !path.exists() {
            return Ok((Some("missing authoring/workflow.json".to_string()), None));
        }

        let authoring = match self.task_authoring_workflow(task_id) {
            Ok(authoring) => authoring,
            Err(err) => return Ok((Some(err.to_string()), None)),
        };
        let workflow = authoring.workflow;

        let validated = match validate_authoring_workflow(workflow.clone()) {
            Ok(validated) => validated,
            Err(err) => {
                let validation_error = err.downcast_ref::<WorkflowValidationError>().cloned();
                return Ok((Some(err.to_string()), validation_error));
            }
        };

        let executable = match validate_and_inject(
            &validated.raw,
            &authoring.model_bindings,
            &self.config.agents,
            &self.provider_diagnostics(),
        ) {
            Ok(executable) => executable,
            Err(err) => return Ok((Some(err.to_string()), None)),
        };
        if let Err(err) = validate_workflow(executable) {
            let validation_error = err.downcast_ref::<WorkflowValidationError>().cloned();
            return Ok((Some(err.to_string()), validation_error));
        }

        match resolve_workflow_profiles(&self.paths, &validated.raw, self.config.desktop_language) {
            Ok(_) => Ok((None, None)),
            Err(err) => Ok((Some(err.to_string()), None)),
        }
    }

    pub fn find_active_or_resumable_run_id(&self, task_id: &str) -> Result<Option<String>> {
        let runs = self.run_list(task_id)?;
        if let Some(run) = runs
            .iter()
            .rev()
            .find(|run| run.status == RunStatus::Running)
        {
            return Ok(Some(run.id.clone()));
        }
        if let Some(run) = runs.iter().rev().find(|run| is_run_continuable(run)) {
            return Ok(Some(run.id.clone()));
        }
        Ok(runs.into_iter().last().map(|run| run.id))
    }

    fn find_resumable_run_id(&self, task_id: &str) -> Result<Option<String>> {
        for run in self.run_list(task_id)?.into_iter().rev() {
            if is_run_continuable(&run) {
                return Ok(Some(run.id));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AcpLiveEventContext, AcpPromptLifecycleEvent, App, AttemptRuntimePauseResult,
        AutoTemplateStore, CreateTaskInput, OwnedTaskDirectory, RuntimeLifecycleEvent,
        WorkflowTemplate, WorkflowTemplateStore, next_auto_template_id,
    };
    use crate::acp::elicitation::{PendingElicitationState, pending_elicitation_file};
    use crate::config::{
        AppearancePreference, ColorSchemePreference, ConsoleThemeName, DesktopLanguage,
        DesktopUpdateBadgeState, FontSizePreference, FontStackPreference,
        PersonalizationPreference, ProviderDiagnosticSnapshot, RuntimeConfig, RuntimeLogLevel,
        catalog_agent_default_config,
    };
    use crate::domain::{
        NodeOutcome, NodeType, PauseReason, RoundTrigger, RunOutcome, RunStatus, SessionMode,
        VERSION,
    };
    use crate::dsl::{
        AiDynamicAgentStrategy, NodeDsl, WorkerNode, WorkflowControl, WorkflowDsl,
        validate_workflow,
    };
    use crate::dynamic::{
        DynamicGraphState, DynamicNodeKind, DynamicNodeState, DynamicNodeStatus, DynamicRunPhase,
        DynamicRunState, DynamicRunStatus, WorkspaceKind, WorkspaceOwnership, WorkspaceState,
        WorkspaceStatus,
    };
    use crate::observability::touch_log_file_best_effort;
    use crate::runtime::{NodeState, RoundState, RunState, RuntimeExecutionPhase, TaskState};
    use crate::storage::{StoragePathConfig, read_json, sqlite::SearchIndex, write_json};
    use crate::workflow_model_binding::{
        TaskAuthoringWorkflow, WorkerModelBinding, WorkflowModelBindingError, WorkflowModelBindings,
    };
    use camino::Utf8PathBuf;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        static ENV_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        ENV_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap()
    }

    #[test]
    fn app_construction_does_not_provision_project_manifest() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("workspace")).unwrap();
        std::fs::create_dir_all(repo_root.as_std_path()).unwrap();

        let app = App::with_config(repo_root, RuntimeConfig::default());

        assert!(!app.paths.project_manifest_file().exists());
    }

    fn write_fixed_attempt_fixture(
        app: &App,
        status: RunStatus,
        reason: Option<PauseReason>,
        runtime_execution_id: Option<&str>,
    ) {
        let run = RunState {
            version: VERSION.to_string(),
            id: "run-001".to_string(),
            task_id: "task-001".to_string(),
            task_uuid: None,
            status,
            outcome: None,
            started_at: "2026-08-10T00:00:00Z".to_string(),
            updated_at: "2026-08-10T00:00:01Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some("round-001".to_string()),
            current_node: Some("worker".to_string()),
            current_attempt: Some("attempt-001".to_string()),
            new_rounds_opened: 0,
            pause_reason: reason,
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: Default::default(),
        };
        let round = RoundState {
            version: VERSION.to_string(),
            id: "round-001".to_string(),
            run_id: "run-001".to_string(),
            index: 1,
            status,
            outcome: None,
            trigger: RoundTrigger::Initial,
            started_at: "2026-08-10T00:00:00Z".to_string(),
            trace: Vec::new(),
            uuid: None,
        };
        let node = NodeState {
            version: VERSION.to_string(),
            acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
            node_id: "worker".to_string(),
            node_type: NodeType::Worker,
            run_id: "run-001".to_string(),
            round_id: "round-001".to_string(),
            attempt_id: "attempt-001".to_string(),
            status,
            outcome: None,
            started_at: "2026-08-10T00:00:00Z".to_string(),
            finished_at: (status == RunStatus::Paused).then(|| "2026-08-10T00:00:01Z".to_string()),
            manual_check_pending: false,
            runtime_execution_id: runtime_execution_id.map(str::to_string),
            resolved_config: Default::default(),
            uuid: None,
        };
        write_json(&app.paths.run_file("task-001", "run-001"), &run).unwrap();
        write_json(
            &app.paths.round_file("task-001", "run-001", "round-001"),
            &round,
        )
        .unwrap();
        write_json(
            &app.paths
                .node_file("task-001", "run-001", "round-001", "worker", "attempt-001"),
            &node,
        )
        .unwrap();
    }

    #[test]
    fn background_continue_prelaunch_failure_converges_to_runtime_abnormal_pause() {
        let temp = tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        write_fixed_attempt_fixture(
            &app,
            RunStatus::Paused,
            Some(PauseReason::ProcessInterrupted),
            None,
        );

        let error = app
            .run_continue_background("task-001", "run-001", None, None)
            .unwrap_err();
        let runtime_error = error
            .downcast_ref::<crate::runtime_error::RuntimeError>()
            .expect("background continue returns a structured launch error");
        assert_eq!(
            runtime_error.info.code_str(),
            "runtime.continue-launch-failed"
        );
        let run: RunState = read_json(&app.paths.run_file("task-001", "run-001")).unwrap();
        assert_eq!(run.status, RunStatus::Paused);
        assert_eq!(run.pause_reason, Some(PauseReason::RuntimeAbnormal));
        assert_eq!(run.execution.phase, RuntimeExecutionPhase::Paused);
        assert_eq!(run.execution.revision, 3);
    }

    #[test]
    fn late_fixed_continue_failure_only_pauses_the_original_active_attempt() {
        let temp = tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        write_fixed_attempt_fixture(&app, RunStatus::Running, None, Some("execution-a"));

        assert_eq!(
            app.pause_attempt_runtime_state_if_active_execution(
                "task-001",
                "run-001",
                "round-001",
                "worker",
                "attempt-001",
                "execution-a",
                PauseReason::RuntimeAbnormal,
            )
            .unwrap(),
            AttemptRuntimePauseResult::Converged
        );
        let run: RunState = read_json(&app.paths.run_file("task-001", "run-001")).unwrap();
        assert_eq!(run.pause_reason, Some(PauseReason::RuntimeAbnormal));

        write_fixed_attempt_fixture(&app, RunStatus::Running, None, Some("execution-a"));
        app.pause_attempt_runtime_state(
            "task-001",
            "run-001",
            "round-001",
            "worker",
            "attempt-001",
            PauseReason::ProcessInterrupted,
        )
        .unwrap();
        write_fixed_attempt_fixture(&app, RunStatus::Running, None, Some("execution-b"));
        assert_eq!(
            app.pause_attempt_runtime_state_if_active_execution(
                "task-001",
                "run-001",
                "round-001",
                "worker",
                "attempt-001",
                "execution-a",
                PauseReason::RuntimeAbnormal,
            )
            .unwrap(),
            AttemptRuntimePauseResult::Superseded
        );
        let run: RunState = read_json(&app.paths.run_file("task-001", "run-001")).unwrap();
        let node: NodeState = read_json(&app.paths.node_file(
            "task-001",
            "run-001",
            "round-001",
            "worker",
            "attempt-001",
        ))
        .unwrap();
        assert_eq!(run.status, RunStatus::Running);
        assert_eq!(run.pause_reason, None);
        assert_eq!(node.status, RunStatus::Running);
        assert_eq!(node.runtime_execution_id.as_deref(), Some("execution-b"));
    }

    #[test]
    fn run_pause_invalidates_the_execution_before_a_late_runtime_write() {
        let temp = tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        write_fixed_attempt_fixture(&app, RunStatus::Running, None, Some("execution-a"));
        let mut stale_run: RunState =
            read_json(&app.paths.run_file("task-001", "run-001")).unwrap();
        let mut stale_round: RoundState =
            read_json(&app.paths.round_file("task-001", "run-001", "round-001")).unwrap();
        let mut stale_node: NodeState = read_json(&app.paths.node_file(
            "task-001",
            "run-001",
            "round-001",
            "worker",
            "attempt-001",
        ))
        .unwrap();

        app.run_pause("task-001", "run-001", PauseReason::ProcessInterrupted)
            .unwrap();
        stale_run.status = RunStatus::Completed;
        stale_run.outcome = Some(RunOutcome::Success);
        stale_round.status = RunStatus::Completed;
        stale_round.outcome = Some(RunOutcome::Success);
        stale_node.status = RunStatus::Completed;
        stale_node.outcome = Some(NodeOutcome::Success);
        stale_node.finished_at = Some("2026-08-10T00:00:02Z".to_string());
        assert!(
            !super::state_access::persist_runtime_state_if_execution_current(
                &app,
                "task-001",
                &mut stale_run,
                &stale_round,
                &stale_node,
            )
            .unwrap()
        );

        let durable_run: RunState = read_json(&app.paths.run_file("task-001", "run-001")).unwrap();
        let durable_node: NodeState = read_json(&app.paths.node_file(
            "task-001",
            "run-001",
            "round-001",
            "worker",
            "attempt-001",
        ))
        .unwrap();
        assert_eq!(durable_run.status, RunStatus::Paused);
        assert_eq!(
            durable_run.pause_reason,
            Some(PauseReason::ProcessInterrupted)
        );
        assert_eq!(durable_run.execution.phase, RuntimeExecutionPhase::Paused);
        assert_eq!(durable_run.execution.revision, 2);
        assert_eq!(durable_node.status, RunStatus::Paused);
        assert_eq!(durable_node.runtime_execution_id, None);
    }

    #[test]
    fn node_completion_preserves_newer_durable_execution_phase() {
        let temp = tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        write_fixed_attempt_fixture(&app, RunStatus::Running, None, Some("execution-a"));
        let mut stale_run = app.run_status("task-001", "run-001").unwrap();
        let round: RoundState =
            read_json(&app.paths.round_file("task-001", "run-001", "round-001")).unwrap();
        let mut node: NodeState = read_json(&app.paths.node_file(
            "task-001",
            "run-001",
            "round-001",
            "worker",
            "attempt-001",
        ))
        .unwrap();
        let mut durable_run = stale_run.clone();
        durable_run
            .transition_current_execution(
                RuntimeExecutionPhase::FinalizingArtifact,
                "2026-08-10T00:00:02Z",
            )
            .unwrap();
        write_json(&app.paths.run_file("task-001", "run-001"), &durable_run).unwrap();
        node.status = RunStatus::Completed;
        node.outcome = Some(NodeOutcome::Success);
        node.finished_at = Some("2026-08-10T00:00:03Z".to_string());

        assert!(
            super::state_access::persist_runtime_state_if_execution_current(
                &app,
                "task-001",
                &mut stale_run,
                &round,
                &node,
            )
            .unwrap()
        );

        let persisted: RunState = read_json(&app.paths.run_file("task-001", "run-001")).unwrap();
        assert_eq!(
            persisted.execution.phase,
            RuntimeExecutionPhase::FinalizingArtifact
        );
        assert_eq!(persisted.execution.revision, durable_run.execution.revision);
    }

    #[test]
    fn startup_recovery_pauses_running_runtime_execution_authoritatively() {
        let temp = tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        write_fixed_attempt_fixture(&app, RunStatus::Running, None, Some("execution-a"));
        write_json(
            &app.paths.task_file("task-001"),
            &TaskState::new("task-001"),
        )
        .unwrap();

        let recovered = app.recover_interrupted_running_sessions().unwrap();

        assert_eq!(recovered.len(), 1);
        let run = app.run_status("task-001", "run-001").unwrap();
        assert_eq!(run.status, RunStatus::Paused);
        assert_eq!(run.pause_reason, Some(PauseReason::ProcessInterrupted));
        assert_eq!(run.execution.phase, RuntimeExecutionPhase::Paused);
        assert_eq!(run.execution.revision, 2);
    }

    #[test]
    fn active_or_resumable_run_selection_does_not_depend_on_progress_files() {
        let temp = tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        write_fixed_attempt_fixture(
            &app,
            RunStatus::Paused,
            Some(PauseReason::ProcessInterrupted),
            None,
        );
        write_json(
            &app.paths.run_progress_file("task-001", "run-999"),
            &serde_json::json!({ "status": "running" }),
        )
        .unwrap();

        assert_eq!(
            app.find_active_or_resumable_run_id("task-001").unwrap(),
            Some("run-001".to_string())
        );
        assert!(app.paths.run_progress_file("task-001", "run-001").exists() == false);
    }

    fn test_path_config() -> StoragePathConfig {
        StoragePathConfig {
            app_key: "gold-band-app-test",
            config_dir_name: ".gold-band-app-test",
            home_env_var: "GOLD_BAND_APP_TEST_HOME",
        }
    }

    #[test]
    fn auto_template_ids_are_name_independent_distributed_ids() {
        let store = AutoTemplateStore {
            version: VERSION.to_string(),
            templates: Vec::new(),
        };

        let first = next_auto_template_id(&store);
        let second = next_auto_template_id(&store);

        assert!(first.starts_with("auto-template-"));
        assert!(second.starts_with("auto-template-"));
        assert_eq!(first.len(), "auto-template-".len() + 32);
        assert_ne!(first, second);
    }

    fn set_test_home(repo_root: &Utf8PathBuf) {
        unsafe {
            std::env::set_var(
                test_path_config().home_env_var,
                repo_root.join("home").as_str(),
            )
        };
    }

    fn test_app(repo_root: Utf8PathBuf) -> App {
        set_test_home(&repo_root);
        App::with_config_and_path_config(repo_root, RuntimeConfig::default(), test_path_config())
    }

    fn test_app_with_provider_capabilities(
        repo_root: Utf8PathBuf,
        capabilities: serde_json::Value,
    ) -> App {
        test_app_with_named_provider_capabilities(repo_root, "claude-acp", capabilities)
    }

    fn test_app_with_named_provider_capabilities(
        repo_root: Utf8PathBuf,
        provider: &str,
        capabilities: serde_json::Value,
    ) -> App {
        set_test_home(&repo_root);
        let mut config = RuntimeConfig::default();
        if let Some(agent_config) = catalog_agent_default_config(provider) {
            config
                .agents
                .insert(provider.parse().unwrap(), agent_config);
        }
        App::with_config_and_path_config(
            repo_root,
            config.with_provider_diagnostics(std::collections::BTreeMap::from([(
                provider.to_string(),
                ProviderDiagnosticSnapshot {
                    available: true,
                    reason: None,
                    checked_at: "2026-06-24T00:00:00Z".to_string(),
                    capabilities: Some(capabilities),
                },
            )])),
            test_path_config(),
        )
    }

    fn worker_workflow(model: Option<&str>, permission_mode: Option<&str>) -> WorkflowDsl {
        WorkflowDsl {
            version: "0.1".to_string(),
            id: "workflow-test".to_string(),
            entry: "dev".to_string(),
            nodes: vec![NodeDsl::Worker(WorkerNode {
                id: "dev".to_string(),
                execution_slot_id: None,
                provider: Some("claude-acp".to_string()),
                profile: None,
                permission_mode: permission_mode.map(str::to_string),
                config_options: Default::default(),
                model: model.map(str::to_string),
                goal: Some("do work".to_string()),
                success_condition: None,
                output: None,
                manual_check: None,
                prompt_envelope: crate::dsl::PromptEnvelopeMode::RuntimeManaged,
            })],
            edges: vec![crate::dsl::EdgeDsl {
                from: "dev".to_string(),
                to: crate::dsl::END_NODE.to_string(),
                on: crate::dsl::EdgeOutcome::Success,
                session: None,
                new_round_entry: None,
            }],
            control: WorkflowControl::default(),
        }
    }

    #[test]
    fn task_create_and_metadata_update_refresh_search_index() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        std::fs::create_dir_all(repo_root.as_std_path()).unwrap();
        let db_path = Utf8PathBuf::from_path_buf(temp.path().join("search.db")).unwrap();
        let index = Arc::new(SearchIndex::open(&db_path).unwrap());
        let mut app = test_app(repo_root);
        app.task_search_indexer = {
            let index = index.clone();
            Arc::new(move |task_dir, task_id| index.index_task_with_retry(task_dir, task_id))
        };
        let mut workflow = worker_workflow(None, None);
        let NodeDsl::Worker(worker) = &mut workflow.nodes[0] else {
            panic!("expected worker workflow")
        };
        worker.prompt_envelope = crate::dsl::PromptEnvelopeMode::RawAgent;

        let created = app
            .create_task_from_requirement(CreateTaskInput {
                title: Some("Searchable conversation".to_string()),
                description: None,
                requirement_file_name: None,
                requirement_content: "find a project file".to_string(),
                workflow,
                workflow_template_id: None,
            })
            .unwrap();

        let created_results = index.search_tasks("searchable", 10).unwrap();
        assert_eq!(created_results.len(), 1);
        assert_eq!(created_results[0].task_id, created.task.id);

        app.update_task_metadata(&created.task.id, "Renamed conversation", None)
            .unwrap();

        assert!(index.search_tasks("searchable", 10).unwrap().is_empty());
        let renamed_results = index.search_tasks("renamed", 10).unwrap();
        assert_eq!(renamed_results.len(), 1);
        assert_eq!(renamed_results[0].task_id, created.task.id);
    }

    #[test]
    fn prepared_run_drop_removes_only_the_unaccepted_run() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        std::fs::create_dir_all(repo_root.as_std_path()).unwrap();
        let app = test_app_with_provider_capabilities(repo_root, serde_json::json!({}));
        let mut workflow = worker_workflow(None, None);
        let NodeDsl::Worker(worker) = &mut workflow.nodes[0] else {
            panic!("expected worker workflow")
        };
        worker.prompt_envelope = crate::dsl::PromptEnvelopeMode::RawAgent;
        let created = app
            .create_task_from_requirement(CreateTaskInput {
                title: Some("Prepared run".to_string()),
                description: None,
                requirement_file_name: None,
                requirement_content: "prepare without launching".to_string(),
                workflow,
                workflow_template_id: None,
            })
            .unwrap();

        let prepared = app.prepare_run(&created.task.id, None).unwrap();
        let run_id = prepared.run().id.clone();
        assert!(app.paths.run_dir(&created.task.id, &run_id).exists());

        drop(prepared);

        assert!(!app.paths.run_dir(&created.task.id, &run_id).exists());
        assert!(app.paths.task_file(&created.task.id).exists());
    }

    #[test]
    fn prepared_run_authoring_override_is_run_scoped() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        std::fs::create_dir_all(repo_root.as_std_path()).unwrap();
        let app = test_app_with_provider_capabilities(
            repo_root,
            serde_json::json!({
                "configOptions": [{
                    "id": "model",
                    "category": "model",
                    "options": [{ "value": "sonnet", "name": "Sonnet" }]
                }]
            }),
        );
        let mut workflow = worker_workflow(None, None);
        let NodeDsl::Worker(worker) = &mut workflow.nodes[0] else {
            panic!("expected worker workflow")
        };
        worker.prompt_envelope = crate::dsl::PromptEnvelopeMode::RawAgent;
        let created = app
            .create_task_from_requirement(CreateTaskInput {
                title: Some("Scheduled override".to_string()),
                description: None,
                requirement_file_name: None,
                requirement_content: "use the scheduled model".to_string(),
                workflow,
                workflow_template_id: None,
            })
            .unwrap();
        let authoring_path = app.paths.workflow_file(&created.task.id);
        let original_authoring = std::fs::read(authoring_path.as_std_path()).unwrap();
        let mut scheduled_authoring = app.task_authoring_workflow(&created.task.id).unwrap();
        scheduled_authoring.model_bindings.bindings[0].model_id = Some("sonnet".to_string());
        scheduled_authoring.model_bindings.binding_revision += 1;

        let scheduled = app
            .prepare_run_with_authoring(&created.task.id, &scheduled_authoring)
            .unwrap();
        let scheduled_snapshot: WorkflowDsl = read_json(
            &app.paths
                .workflow_snapshot_file(&created.task.id, &scheduled.run().id),
        )
        .unwrap();
        let NodeDsl::Worker(scheduled_worker) = &scheduled_snapshot.nodes[0] else {
            panic!("expected worker workflow")
        };
        assert_eq!(scheduled_worker.model.as_deref(), Some("sonnet"));
        assert_eq!(
            std::fs::read(authoring_path.as_std_path()).unwrap(),
            original_authoring
        );
        drop(scheduled);

        let manual = app.prepare_run(&created.task.id, None).unwrap();
        let manual_snapshot: WorkflowDsl = read_json(
            &app.paths
                .workflow_snapshot_file(&created.task.id, &manual.run().id),
        )
        .unwrap();
        let NodeDsl::Worker(manual_worker) = &manual_snapshot.nodes[0] else {
            panic!("expected worker workflow")
        };
        assert_eq!(manual_worker.model, None);
    }

    #[test]
    fn owned_task_directory_rolls_back_until_disarmed() {
        let temp = tempdir().unwrap();
        let rollback_dir = Utf8PathBuf::from_path_buf(temp.path().join("rollback")).unwrap();
        std::fs::create_dir_all(rollback_dir.as_std_path()).unwrap();
        drop(OwnedTaskDirectory::new(rollback_dir.clone()));
        assert!(!rollback_dir.exists());

        let committed_dir = Utf8PathBuf::from_path_buf(temp.path().join("committed")).unwrap();
        std::fs::create_dir_all(committed_dir.as_std_path()).unwrap();
        let mut owned = OwnedTaskDirectory::new(committed_dir.clone());
        owned.disarm();
        drop(owned);
        assert!(committed_dir.exists());
    }

    fn sample_run_paused_event() -> RuntimeLifecycleEvent {
        RuntimeLifecycleEvent::RunPaused {
            event_id: "project-1:run-1:round-1:node-1:attempt-1:waiting-for-user-input".to_string(),
            occurred_at: "2026-01-01T00:00:00".to_string(),
            scheduled_occurrence_id: None,
            project_id: "project-1".to_string(),
            task_id: "task-1".to_string(),
            run_id: "run-1".to_string(),
            round_id: "round-1".to_string(),
            node_id: "node-1".to_string(),
            attempt_id: "attempt-1".to_string(),
            node_label: "节点".to_string(),
            pause_reason: PauseReason::WaitingForUserInput,
            task_title: Some("标题".to_string()),
        }
    }

    fn resumability_run(reason: PauseReason) -> RunState {
        RunState {
            version: VERSION.to_string(),
            id: "run-001".to_string(),
            task_id: "task-001".to_string(),
            task_uuid: None,
            status: RunStatus::Paused,
            outcome: None,
            started_at: "1Z".to_string(),
            updated_at: "1Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some("round-001".to_string()),
            current_node: Some("node-001".to_string()),
            current_attempt: Some("attempt-001".to_string()),
            new_rounds_opened: 0,
            pause_reason: Some(reason),
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: Default::default(),
        }
    }

    #[test]
    fn runtime_abnormal_is_continuable_but_error_blocked_is_not() {
        assert!(super::is_run_continuable(&resumability_run(
            PauseReason::RuntimeAbnormal
        )));
        assert!(super::is_run_continuable(&resumability_run(
            PauseReason::ProcessInterrupted
        )));
        assert!(!super::is_run_continuable(&resumability_run(
            PauseReason::WaitingForUserInput
        )));
        assert!(!super::is_run_continuable(&resumability_run(
            PauseReason::ErrorBlocked
        )));
        assert!(!super::is_run_continuable(&resumability_run(
            PauseReason::PermissionRequested
        )));
    }

    #[test]
    fn provider_diagnostics_fallback_reads_persisted_agent_diagnostics() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = test_app(repo_root)
            .with_provider_diagnostics_source(Arc::new(|| Ok(std::collections::BTreeMap::new())));
        write_json(
            &app.paths.agent_diagnostics_file(),
            &std::collections::BTreeMap::from([(
                "claude-acp".to_string(),
                ProviderDiagnosticSnapshot {
                    available: true,
                    reason: None,
                    checked_at: "2026-06-24T00:00:00Z".to_string(),
                    capabilities: Some(serde_json::json!({
                        "configOptions": [
                            {
                                "category": "model",
                                "options": [{ "value": "sonnet", "name": "Sonnet" }]
                            }
                        ]
                    })),
                },
            )]),
        )
        .unwrap();

        let diagnostics = app.provider_diagnostics();

        let models = crate::provider::supported_models_from_capabilities(
            diagnostics
                .get("claude-acp")
                .and_then(|diagnostic| diagnostic.capabilities.as_ref()),
        );
        assert_eq!(models[0].id, "sonnet");
    }

    #[test]
    fn workflow_agent_validation_allows_stale_models_but_rejects_invalid_permission_modes() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = test_app_with_provider_capabilities(
            repo_root,
            serde_json::json!({
                "configOptions": [
                    {
                        "id": "mode",
                        "category": "mode",
                        "options": [{ "value": "acceptEdits", "name": "Ask" }]
                    },
                    {
                        "id": "model",
                        "category": "model",
                        "options": [{ "value": "sonnet", "name": "Sonnet" }]
                    }
                ]
            }),
        );

        let accepted =
            validate_workflow(worker_workflow(Some("sonnet"), Some("acceptEdits"))).unwrap();
        let rejected_model =
            validate_workflow(worker_workflow(Some("opus"), Some("acceptEdits"))).unwrap();
        let rejected_mode =
            validate_workflow(worker_workflow(Some("sonnet"), Some("full_access"))).unwrap();

        assert!(app.validate_workflow_agents(&accepted).is_ok());
        assert!(app.validate_workflow_agents(&rejected_model).is_ok());
        assert!(app.validate_workflow_agents(&rejected_mode).is_err());
    }

    #[test]
    fn ai_dynamic_accepts_native_permission_modes_per_agent() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = test_app_with_named_provider_capabilities(
            repo_root,
            "codex-acp",
            serde_json::json!({
                "configOptions": [
                    {
                        "id": "mode",
                        "category": "mode",
                        "options": [
                            { "value": "read-only", "name": "Read-only" },
                            { "value": "agent", "name": "Agent" },
                            { "value": "agent-full-access", "name": "Agent (full access)" }
                        ]
                    }
                ]
            }),
        );
        let node = NodeDsl::AiDynamic(crate::dsl::AiDynamicNode {
            id: "route".to_string(),
            agent_strategy: AiDynamicAgentStrategy::Dynamic {
                bootstrap_provider: "codex-acp".to_string(),
                bootstrap_model: None,
                permission_mode: Some("agent-full-access".to_string()),
                bootstrap_config_options: Default::default(),
                acceptance_model: None,
                acceptance_config_options: Default::default(),
                routing_prompt: String::new(),
                available_agents: vec![crate::dsl::DynamicAgentRef {
                    provider: "codex-acp".to_string(),
                    model: None,
                    permission_mode: Some("agent-full-access".to_string()),
                    config_options: Default::default(),
                }],
            },
            config_options: Default::default(),
            allowed_profiles: Vec::new(),
            global_goal: None,
            control: crate::dsl::DynamicControlDsl::default(),
            allowed_workflows: Vec::new(),
        });

        assert!(app.validate_workflow_node_agent_options(&node).is_ok());
    }

    #[test]
    fn ai_dynamic_stale_models_are_preserved_for_explicit_validation() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let _app = test_app_with_provider_capabilities(
            repo_root,
            serde_json::json!({
                "configOptions": [
                    {
                        "id": "model",
                        "category": "model",
                        "options": [{ "value": "sonnet", "name": "Sonnet" }]
                    }
                ]
            }),
        );
        let workflow = WorkflowDsl {
            version: "0.1".to_string(),
            id: "workflow-dynamic".to_string(),
            entry: "route".to_string(),
            nodes: vec![NodeDsl::AiDynamic(crate::dsl::AiDynamicNode {
                id: "route".to_string(),
                agent_strategy: AiDynamicAgentStrategy::Dynamic {
                    bootstrap_provider: "claude-acp".to_string(),
                    bootstrap_model: Some("sonnet".to_string()),
                    permission_mode: None,
                    bootstrap_config_options: Default::default(),
                    acceptance_model: Some("sonnet".to_string()),
                    acceptance_config_options: Default::default(),
                    routing_prompt: String::new(),
                    available_agents: vec![crate::dsl::DynamicAgentRef {
                        provider: "claude-acp".to_string(),
                        model: Some("future-model".to_string()),
                        permission_mode: None,
                        config_options: Default::default(),
                    }],
                },
                config_options: Default::default(),
                allowed_profiles: Vec::new(),
                global_goal: None,
                control: crate::dsl::DynamicControlDsl::default(),
                allowed_workflows: Vec::new(),
            })],
            edges: vec![crate::dsl::EdgeDsl {
                from: "route".to_string(),
                to: crate::dsl::END_NODE.to_string(),
                on: crate::dsl::EdgeOutcome::Success,
                session: None,
                new_round_entry: None,
            }],
            control: WorkflowControl::default(),
        };
        let NodeDsl::AiDynamic(dynamic) = &workflow.nodes[0] else {
            unreachable!();
        };
        let AiDynamicAgentStrategy::Dynamic {
            available_agents, ..
        } = &dynamic.agent_strategy
        else {
            unreachable!();
        };
        assert_eq!(available_agents[0].model.as_deref(), Some("future-model"));
        assert!(validate_workflow(workflow).is_ok());
    }

    #[test]
    fn workflow_template_and_task_authoring_preserve_stale_model_ids() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = test_app_with_named_provider_capabilities(
            repo_root,
            "codex-acp",
            serde_json::json!({
                "configOptions": [{
                    "id": "model",
                    "category": "model",
                    "options": [{ "value": "gpt-5.6-sol", "name": "GPT-5.6-Sol" }]
                }]
            }),
        );
        let stale_workflow = WorkflowDsl {
            version: "0.1".to_string(),
            id: "workflow-stale-models".to_string(),
            entry: "route".to_string(),
            nodes: vec![
                NodeDsl::AiDynamic(crate::dsl::AiDynamicNode {
                    id: "route".to_string(),
                    agent_strategy: AiDynamicAgentStrategy::Dynamic {
                        bootstrap_provider: "codex-acp".to_string(),
                        bootstrap_model: Some("gpt-5.6-sol".to_string()),
                        permission_mode: None,
                        bootstrap_config_options: Default::default(),
                        acceptance_model: Some("gpt-5.6-sol".to_string()),
                        acceptance_config_options: Default::default(),
                        routing_prompt: String::new(),
                        available_agents: vec![crate::dsl::DynamicAgentRef {
                            provider: "codex-acp".to_string(),
                            model: Some("gpt-5.4".to_string()),
                            permission_mode: None,
                            config_options: Default::default(),
                        }],
                    },
                    config_options: Default::default(),
                    allowed_profiles: Vec::new(),
                    global_goal: None,
                    control: crate::dsl::DynamicControlDsl::default(),
                    allowed_workflows: Vec::new(),
                }),
                NodeDsl::AiDynamic(crate::dsl::AiDynamicNode {
                    id: "fixed".to_string(),
                    agent_strategy: AiDynamicAgentStrategy::Fixed {
                        provider: "codex-acp".to_string(),
                        model: Some("gpt-5.4".to_string()),
                        permission_mode: None,
                    },
                    config_options: Default::default(),
                    allowed_profiles: Vec::new(),
                    global_goal: None,
                    control: crate::dsl::DynamicControlDsl::default(),
                    allowed_workflows: Vec::new(),
                }),
            ],
            edges: vec![
                crate::dsl::EdgeDsl {
                    from: "route".to_string(),
                    to: "fixed".to_string(),
                    on: crate::dsl::EdgeOutcome::Success,
                    session: None,
                    new_round_entry: None,
                },
                crate::dsl::EdgeDsl {
                    from: "fixed".to_string(),
                    to: crate::dsl::END_NODE.to_string(),
                    on: crate::dsl::EdgeOutcome::Success,
                    session: None,
                    new_round_entry: None,
                },
            ],
            control: WorkflowControl::default(),
        };
        let store = WorkflowTemplateStore {
            version: VERSION.to_string(),
            last_used_template_id: Some("custom".to_string()),
            last_created_workflow: Some(stale_workflow.clone()),
            templates: vec![WorkflowTemplate {
                id: "custom".to_string(),
                name: "Custom".to_string(),
                is_built_in: false,
                optional_entry_stage: None,
                workflow: stale_workflow.clone(),
                model_bindings: WorkflowModelBindings::default(),
                created_at: "2026-07-28T00:00:00Z".to_string(),
                updated_at: "2026-07-28T00:00:00Z".to_string(),
            }],
        };
        write_json(&app.paths.workflow_templates_file(), &store).unwrap();

        let loaded = app.workflow_templates().unwrap();
        let persisted: WorkflowTemplateStore =
            read_json(&app.paths.workflow_templates_file()).unwrap();
        for store in [&loaded, &persisted] {
            let workflow = &store
                .templates
                .iter()
                .find(|template| template.id == "custom")
                .unwrap()
                .workflow;
            let NodeDsl::AiDynamic(route) = &workflow.nodes[0] else {
                unreachable!();
            };
            let AiDynamicAgentStrategy::Dynamic {
                available_agents, ..
            } = &route.agent_strategy
            else {
                unreachable!();
            };
            assert_eq!(available_agents[0].model.as_deref(), Some("gpt-5.4"));
            let NodeDsl::AiDynamic(fixed) = &workflow.nodes[1] else {
                unreachable!();
            };
            let AiDynamicAgentStrategy::Fixed { model, .. } = &fixed.agent_strategy else {
                unreachable!();
            };
            assert_eq!(model.as_deref(), Some("gpt-5.4"));
        }

        let task = app
            .create_task_from_requirement(CreateTaskInput {
                title: Some("Stale model authoring".to_string()),
                description: None,
                requirement_file_name: None,
                requirement_content: "verify authoring normalization".to_string(),
                workflow: stale_workflow,
                workflow_template_id: None,
            })
            .unwrap();
        let persisted_authoring: TaskAuthoringWorkflow =
            read_json(&app.paths.workflow_file(&task.task.id)).unwrap();
        let NodeDsl::AiDynamic(route) = &persisted_authoring.workflow.nodes[0] else {
            unreachable!();
        };
        let AiDynamicAgentStrategy::Dynamic {
            available_agents, ..
        } = &route.agent_strategy
        else {
            unreachable!();
        };
        assert_eq!(available_agents[0].model.as_deref(), Some("gpt-5.4"));
        let NodeDsl::AiDynamic(fixed) = &persisted_authoring.workflow.nodes[1] else {
            unreachable!();
        };
        let AiDynamicAgentStrategy::Fixed { model, .. } = &fixed.agent_strategy else {
            unreachable!();
        };
        assert_eq!(model.as_deref(), Some("gpt-5.4"));
    }

    #[test]
    fn save_task_workflow_rejects_duplicate_model_binding_slots() {
        let temp = tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap());
        write_json(
            &app.paths.task_file("task-001"),
            &TaskState::new("task-001"),
        )
        .unwrap();
        let workflow = WorkflowDsl {
            version: VERSION.to_string(),
            id: "workflow-duplicate-bindings".to_string(),
            entry: "dev".to_string(),
            control: WorkflowControl::default(),
            nodes: vec![NodeDsl::Worker(WorkerNode {
                id: "dev".to_string(),
                execution_slot_id: Some("slot-dev".to_string()),
                provider: None,
                model: None,
                profile: None,
                goal: None,
                output: None,
                success_condition: None,
                permission_mode: None,
                config_options: BTreeMap::new(),
                manual_check: None,
                prompt_envelope: Default::default(),
            })],
            edges: Vec::new(),
        };
        let duplicate = WorkerModelBinding {
            execution_slot_id: "slot-dev".to_string(),
            agent_id: "agent-a".to_string(),
            model_id: None,
            permission_mode_id: None,
            config_options: BTreeMap::new(),
        };
        let bindings = WorkflowModelBindings {
            definition_revision: String::new(),
            binding_revision: 0,
            bindings: vec![duplicate.clone(), duplicate],
        };

        let error = app
            .save_task_workflow_with_bindings("task-001", workflow, bindings)
            .unwrap_err();
        let binding_error = error.downcast_ref::<WorkflowModelBindingError>().unwrap();

        assert_eq!(
            binding_error,
            &WorkflowModelBindingError::BindingDuplicate {
                execution_slot_id: "slot-dev".to_string(),
            }
        );
        assert!(!app.paths.workflow_file("task-001").exists());
    }

    #[test]
    fn lifecycle_subscriber_invoked_and_propagated_to_background() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_callback = seen.clone();
        let app = test_app(repo_root).with_inline_lifecycle_subscriber(Arc::new(move |event| {
            if let RuntimeLifecycleEvent::RunPaused { event_id, .. } = event {
                seen_for_callback.lock().unwrap().push(event_id);
            }
        }));

        app.emit_lifecycle_event(sample_run_paused_event());
        assert_eq!(seen.lock().unwrap().len(), 1);

        let bg = app.clone_for_background();
        bg.emit_lifecycle_event(sample_run_paused_event());
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    #[test]
    fn scheduled_origin_is_injected_into_background_lifecycle_events() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_callback = seen.clone();
        let app = test_app(repo_root)
            .with_scheduled_occurrence_id(Some("occurrence-001".to_string()))
            .with_inline_lifecycle_subscriber(Arc::new(move |event| {
                if let RuntimeLifecycleEvent::RunCompleted {
                    scheduled_occurrence_id,
                    ..
                } = event
                {
                    seen_for_callback
                        .lock()
                        .unwrap()
                        .push(scheduled_occurrence_id);
                }
            }));

        app.emit_lifecycle_event(RuntimeLifecycleEvent::RunCompleted {
            event_id: "run-completed".to_string(),
            occurred_at: "2026-08-03T00:00:00Z".to_string(),
            scheduled_occurrence_id: None,
            project_id: "project-001".to_string(),
            task_id: "task-001".to_string(),
            run_id: "run-001".to_string(),
            round_id: "round-001".to_string(),
            node_id: "node-001".to_string(),
            attempt_id: "attempt-001".to_string(),
            node_label: "node".to_string(),
            outcome: RunOutcome::Success,
            task_title: None,
            completion_agent_label: None,
        });

        assert_eq!(
            seen.lock().unwrap().as_slice(),
            &[Some("occurrence-001".to_string())]
        );
    }

    #[test]
    fn queued_user_turn_drops_scheduler_occurrence_and_prompt_context() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = test_app(repo_root)
            .with_scheduled_occurrence_id(Some("occurrence-001".to_string()))
            .with_scheduled_task_context(Some(crate::provider::ScheduledTaskContextInfo {
                title: "Daily review".to_string(),
                mode: "direct".to_string(),
                session_policy: "continuous".to_string(),
                trigger_kind: "cron".to_string(),
                triggered_at: "2026-08-03T00:00:00Z".to_string(),
                instruction: Some("Review changes".to_string()),
            }));

        let ordinary_turn = app.clone_for_background().without_scheduled_turn_context();

        assert_eq!(app.scheduled_occurrence_id(), Some("occurrence-001"));
        assert!(app.scheduled_task_context().is_some());
        assert_eq!(ordinary_turn.scheduled_occurrence_id(), None);
        assert!(ordinary_turn.scheduled_task_context().is_none());
    }

    #[test]
    fn lifecycle_bus_silent_when_no_subscribers() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = test_app(repo_root);
        app.emit_lifecycle_event(sample_run_paused_event());
    }

    #[test]
    fn metrics_fact_producer_is_explicitly_gated() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let node_started = |repo_root: String| RuntimeLifecycleEvent::NodeStarted {
            task_id: "task-001".into(),
            task_uuid: Some(uuid::Uuid::new_v4().to_string()),
            run_id: "run-001".into(),
            run_uuid: Some(uuid::Uuid::new_v4().to_string()),
            round_id: "round-001".into(),
            round_uuid: Some(uuid::Uuid::new_v4().to_string()),
            round_index: Some(1),
            node_id: "node-001".into(),
            node_uuid: Some(uuid::Uuid::new_v4().to_string()),
            attempt_id: "attempt-002".into(),
            repo_root,
            seq: Some(1),
            node_name: Some("worker".into()),
            agent_type: Some("provider".into()),
            resolved_model: Some("model".into()),
            started_at: "2026-08-01T00:00:00Z".into(),
            attempt_dir: None,
            predecessor: None,
            metrics_unit_kind: None,
            child_run_id: None,
        };
        let disabled_seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let disabled_callback = disabled_seen.clone();
        let disabled =
            test_app(repo_root.clone()).with_inline_lifecycle_subscriber(Arc::new(move |event| {
                if matches!(event, RuntimeLifecycleEvent::MetricsFact(_)) {
                    disabled_callback.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            }));
        disabled.emit_lifecycle_event(node_started(disabled.paths.repo_root.to_string()));
        assert_eq!(disabled_seen.load(std::sync::atomic::Ordering::SeqCst), 0);
        let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen_callback = seen.clone();
        let app = test_app(repo_root).with_metrics_collection_enabled(true);
        app.lifecycle_bus
            .subscribe_inline(app.create_metrics_fact_producer());
        app.lifecycle_bus.subscribe_inline(Arc::new(move |event| {
            if matches!(event, RuntimeLifecycleEvent::MetricsFact(_)) {
                seen_callback.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }));
        app.emit_lifecycle_event(node_started(app.paths.repo_root.to_string()));
        for _ in 0..50 {
            if seen.load(std::sync::atomic::Ordering::SeqCst) == 1 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn metrics_fact_producer_uses_event_workspace_paths() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let producer_root =
            Utf8PathBuf::from_path_buf(temp.path().join("producer-workspace")).unwrap();
        let event_root = Utf8PathBuf::from_path_buf(temp.path().join("event-workspace")).unwrap();
        std::fs::create_dir_all(producer_root.as_std_path()).unwrap();
        std::fs::create_dir_all(event_root.as_std_path()).unwrap();
        let app = test_app(producer_root.clone()).with_metrics_collection_enabled(true);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_callback = seen.clone();
        app.lifecycle_bus
            .subscribe_inline(app.create_metrics_fact_producer());
        app.lifecycle_bus.subscribe_inline(Arc::new(move |event| {
            if let RuntimeLifecycleEvent::MetricsFact(fact) = event {
                seen_callback.lock().unwrap().push(fact);
            }
        }));
        let node_uuid = uuid::Uuid::new_v4().to_string();
        let run_uuid = uuid::Uuid::new_v4().to_string();
        let task_uuid = uuid::Uuid::new_v4().to_string();
        app.emit_lifecycle_event(RuntimeLifecycleEvent::NodeStarted {
            task_id: "task-001".into(),
            task_uuid: Some(task_uuid.clone()),
            run_id: "run-001".into(),
            run_uuid: Some(run_uuid),
            round_id: "round-001".into(),
            round_uuid: Some(uuid::Uuid::new_v4().to_string()),
            round_index: Some(1),
            node_id: "node-001".into(),
            node_uuid: Some(node_uuid.clone()),
            attempt_id: "attempt-002".into(),
            repo_root: event_root.to_string(),
            seq: Some(1),
            node_name: Some("worker".into()),
            agent_type: Some("provider".into()),
            resolved_model: Some("model".into()),
            started_at: "2026-08-01T00:00:00Z".into(),
            attempt_dir: None,
            predecessor: None,
            metrics_unit_kind: None,
            child_run_id: None,
        });
        for _ in 0..50 {
            if !seen.lock().unwrap().is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let facts = seen.lock().unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].workspace, event_root.to_string());
        assert_eq!(facts[0].attempt_index, Some(2));
        drop(facts);
        let event_snapshot = crate::storage::GoldBandPaths::new(event_root)
            .run_dir("task-001", "run-001")
            .join("observability")
            .join(&task_uuid)
            .join(&node_uuid)
            .join(super::observability::OBSERVABILITY_SNAPSHOT_FILE);
        for _ in 0..50 {
            if event_snapshot.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(event_snapshot.exists());
        assert!(
            !producer_root
                .join("tasks/task-001/runs/run-001/observability")
                .exists()
        );
    }

    #[test]
    fn metrics_fact_producer_preserves_per_prompt_model_usage_segments() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let attempt_dir = repo_root.join("attempt");
        std::fs::create_dir_all(attempt_dir.as_std_path()).unwrap();
        let journal = attempt_dir.join("acp.prompt-usage.jsonl");
        for (seq, provider, model, tokens) in [
            (1, "provider-a", "model-a", 10),
            (2, "provider-b", "model-b", 20),
            (3, "provider-a", "model-a", 30),
        ] {
            let turn = format!("turn-{seq}");
            crate::acp::usage::append_prompt_started(
                &journal,
                &turn,
                seq,
                "2026-08-01T00:00:00Z",
                Some(provider),
                Some(model),
            )
            .unwrap();
            crate::acp::usage::append_prompt_completed(
                &journal,
                &turn,
                seq,
                "2026-08-01T00:00:01Z",
                None,
                &crate::acp::usage::AcpPromptTokenUsage {
                    input_tokens: Some(tokens),
                    total_tokens: Some(tokens),
                    ..Default::default()
                },
                Some(provider),
                Some(model),
            )
            .unwrap();
        }
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_callback = seen.clone();
        let app = test_app(repo_root.clone()).with_metrics_collection_enabled(true);
        app.lifecycle_bus
            .subscribe_inline(app.create_metrics_fact_producer());
        app.lifecycle_bus.subscribe_inline(Arc::new(move |event| {
            if let RuntimeLifecycleEvent::MetricsFact(fact) = event
                && fact.event_type
                    == crate::app::observability::LifecycleEventType::ExecutionCompleted
            {
                seen_callback.lock().unwrap().push(fact);
            }
        }));
        let execution_id = uuid::Uuid::new_v4().to_string();
        app.emit_lifecycle_event(RuntimeLifecycleEvent::NodeCompleted {
            task_id: "task-001".into(),
            task_uuid: Some(uuid::Uuid::new_v4().to_string()),
            run_id: "run-001".into(),
            run_uuid: Some(uuid::Uuid::new_v4().to_string()),
            round_id: "round-001".into(),
            round_uuid: Some(uuid::Uuid::new_v4().to_string()),
            round_index: Some(1),
            node_id: "node-001".into(),
            node_uuid: Some(execution_id.clone()),
            attempt_id: "attempt-001".into(),
            repo_root: repo_root.to_string(),
            seq: Some(1),
            node_name: "worker".into(),
            agent_type: Some("provider-a".into()),
            resolved_model: Some("model-a".into()),
            started_at: "2026-08-01T00:00:00Z".into(),
            finished_at: Some("2026-08-01T00:00:01Z".into()),
            outcome: "SUCCESS".into(),
            attempt_dir: attempt_dir.to_string(),
            suppress_sentinel: false,
            metrics_unit_kind: None,
            child_run_id: None,
        });
        for _ in 0..50 {
            if !seen.lock().unwrap().is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let facts = seen.lock().unwrap();
        assert_eq!(facts.len(), 1);
        let usages = facts[0].model_usages.as_ref().unwrap();
        assert_eq!(facts[0].attempt_id.as_deref(), Some(execution_id.as_str()));
        assert_eq!(facts[0].attempt_index, Some(1));
        assert_ne!(facts[0].execution_id, execution_id);
        assert_eq!(usages.len(), 2);
        assert_eq!(usages[0].model, "model-a");
        assert_eq!(usages[0].usage.total_tokens, Some(40));
        assert_eq!(usages[1].model, "model-b");
        assert_eq!(usages[1].usage.total_tokens, Some(20));
        assert_eq!(facts[0].usage.as_ref().unwrap().total_tokens, Some(60));
    }

    #[test]
    fn direct_usage_segments_are_filtered_by_baseline() {
        let temp = tempdir().unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        std::fs::create_dir_all(attempt_dir.as_std_path()).unwrap();
        let journal = attempt_dir.join("acp.prompt-usage.jsonl");
        for (seq, provider, model, tokens) in [
            (1, "provider-a", "model-a", 10),
            (2, "provider-b", "model-b", 20),
        ] {
            let turn = format!("turn-{seq}");
            crate::acp::usage::append_prompt_started(
                &journal,
                &turn,
                seq,
                "2026-08-01T00:00:00Z",
                Some(provider),
                Some(model),
            )
            .unwrap();
            crate::acp::usage::append_prompt_completed(
                &journal,
                &turn,
                seq,
                "2026-08-01T00:00:01Z",
                None,
                &crate::acp::usage::AcpPromptTokenUsage {
                    input_tokens: Some(tokens),
                    total_tokens: Some(tokens),
                    ..Default::default()
                },
                Some(provider),
                Some(model),
            )
            .unwrap();
        }
        assert_eq!(App::direct_usage_baseline(Some(attempt_dir.as_path())), 2);
        let after = App::direct_usage_segments_after(Some(attempt_dir.as_path()), 1);
        assert_eq!(after.len(), 1);
        let usages = App::direct_model_usages_from_segments(&after, None, None);
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].model, "model-b");
        assert_eq!(usages[0].usage.total_tokens, Some(20));
    }

    #[test]
    fn direct_metrics_is_follow_up_detects_history_after_turn_ends() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = test_app(repo_root).with_metrics_collection_enabled(true);
        let task_uuid = uuid::Uuid::new_v4().to_string();
        let attempt_key = format!("direct:{task_uuid}");
        let attempt_path = app
            .paths
            .run_dir("task-001", "run-001")
            .join("observability")
            .join(&task_uuid)
            .join(&task_uuid)
            .join(super::observability::OBSERVABILITY_SNAPSHOT_FILE);

        assert!(!app.direct_metrics_is_follow_up(&attempt_key, None, &attempt_path));
        app.begin_metrics_turn(
            attempt_key.clone(),
            super::ActiveMetricTurn::new(task_uuid.clone(), task_uuid.clone(), 1, 0),
        );
        assert!(app.direct_metrics_is_follow_up(&attempt_key, None, &attempt_path));
        app.end_metrics_turn(&attempt_key);

        let mut state = super::observability::ExecutionObservabilityState::default();
        state.next_revision();
        super::observability::persist_observability_snapshot_best_effort(
            attempt_path.clone(),
            state,
        );
        for _ in 0..100 {
            if super::observability::load_observability_snapshot(&attempt_path).event_revision > 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(app.direct_metrics_is_follow_up(&attempt_key, None, &attempt_path));
    }

    #[test]
    fn observability_update_starts_from_memory_without_reading_snapshot() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = test_app(repo_root);
        let snapshot_path = Utf8PathBuf::from_path_buf(
            temp.path()
                .join("observability")
                .join("observability.snapshot.json"),
        )
        .unwrap();
        let mut persisted = super::observability::ExecutionObservabilityState::recovered();
        persisted.event_revision = 41;
        write_json(&snapshot_path, &persisted).unwrap();

        let first = app.update_observability_state(
            "execution-with-existing-snapshot",
            snapshot_path.clone(),
            |state| {
                state.next_revision();
            },
        );
        let second = app.update_observability_state(
            "execution-with-existing-snapshot",
            snapshot_path,
            |state| {
                state.next_revision();
            },
        );

        assert_eq!(first.event_revision, 1);
        assert_eq!(first.collection_state_recovered, None);
        assert_eq!(second.event_revision, 2);
    }

    #[test]
    fn emits_acp_session_update_context() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_callback = seen.clone();
        let app = test_app(repo_root).with_acp_session_update(Arc::new(move |context| {
            seen_for_callback.lock().unwrap().push(context);
            Ok(())
        }));

        app.emit_acp_session_update(AcpLiveEventContext {
            task_id: "task-001".to_string(),
            run_id: "run-001".to_string(),
            round_id: "round-001".to_string(),
            node_id: "验收".to_string(),
            attempt_id: "attempt-001".to_string(),
            outer_node_id: None,
            outer_attempt_id: None,
        })
        .unwrap();

        let calls = seen.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].run_id, "run-001");
        assert_eq!(calls[0].node_id, "验收");
        assert_eq!(calls[0].attempt_id, "attempt-001");
    }

    #[test]
    fn acp_session_update_for_emits_context() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_callback = seen.clone();
        let app = test_app(repo_root).with_acp_session_update(Arc::new(move |context| {
            seen_for_callback.lock().unwrap().push(context);
            Ok(())
        }));

        let context = AcpLiveEventContext {
            task_id: "task-001".to_string(),
            run_id: "run-001".to_string(),
            round_id: "round-001".to_string(),
            node_id: "dev".to_string(),
            attempt_id: "attempt-002".to_string(),
            outer_node_id: Some("outer-node".to_string()),
            outer_attempt_id: Some("outer-attempt".to_string()),
        };
        let callback = app.acp_session_update_for(context.clone()).unwrap();
        callback().unwrap();

        let calls = seen.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].task_id, context.task_id);
        assert_eq!(calls[0].run_id, context.run_id);
        assert_eq!(calls[0].round_id, context.round_id);
        assert_eq!(calls[0].node_id, context.node_id);
        assert_eq!(calls[0].attempt_id, context.attempt_id);
        assert_eq!(calls[0].outer_node_id, context.outer_node_id);
        assert_eq!(calls[0].outer_attempt_id, context.outer_attempt_id);
    }

    #[test]
    fn prompt_lifecycle_reports_accepted_identity_and_finished_outcome() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_callback = seen.clone();
        let app =
            test_app(repo_root).with_prompt_turn_lifecycle(Arc::new(move |context, event| {
                seen_for_callback.lock().unwrap().push((context, event));
                Ok(())
            }));
        let context = AcpLiveEventContext {
            task_id: "task-001".to_string(),
            run_id: "run-001".to_string(),
            round_id: "round-001".to_string(),
            node_id: "direct-agent".to_string(),
            attempt_id: "attempt-001".to_string(),
            outer_node_id: None,
            outer_attempt_id: None,
        };

        app.acp_prompt_accepted_for(context.clone()).unwrap()("turn-queued-001").unwrap();
        app.notify_prompt_turn_finished(context, Some("turn-queued-001".to_string()), false)
            .unwrap();

        let calls = seen.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0.task_id, "task-001");
        assert_eq!(
            calls[0].1,
            AcpPromptLifecycleEvent::Accepted {
                prompt_id: "turn-queued-001".to_string(),
            }
        );
        assert_eq!(
            calls[1].1,
            AcpPromptLifecycleEvent::Finished {
                prompt_id: Some("turn-queued-001".to_string()),
                successful: false,
            }
        );
    }

    fn dynamic_pause_test_app(temp: &tempfile::TempDir) -> App {
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        std::fs::create_dir_all(repo_root.as_std_path()).unwrap();
        test_app(repo_root)
    }

    fn dynamic_pause_node(id: &str, status: DynamicNodeStatus) -> DynamicNodeState {
        let mut node = DynamicNodeState {
            version: VERSION.to_string(),
            acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
            id: id.to_string(),
            dynamic_run_id: "dynamic-run-001".to_string(),
            kind: DynamicNodeKind::Worker,
            title: id.to_string(),
            task: id.to_string(),
            status,
            outcome: None,
            pause_reason: None,
            runtime_error: None,
            runtime_execution_id: None,
            runtime_execution_phase: None,
            runtime_execution_revision: 0,
            runtime_execution_updated_at: None,
            group_id: None,
            chain_id: id.to_string(),
            depth: 1,
            depends_on: Vec::new(),
            workspace_id: "workspace-main".to_string(),
            provider: Some("claude-acp".to_string()),
            profile: None,
            permission_mode: None,
            model: None,
            session_mode: SessionMode::New,
            continue_from_node_id: None,
            workflow_id: None,
            workflow_snapshot_id: None,
            child_run_id: None,
            started_at: Some("2026-06-16T00:00:00Z".to_string()),
            finished_at: None,
            uuid: None,
        };
        if status == DynamicNodeStatus::Running {
            let execution_id = format!("execution-{id}");
            node.begin_runtime_execution(execution_id.clone(), "2026-06-16T00:00:00Z".to_string());
            node.transition_runtime_execution(
                &execution_id,
                RuntimeExecutionPhase::RunningNode,
                "2026-06-16T00:00:01Z".to_string(),
            )
            .unwrap();
        }
        node
    }

    fn write_dynamic_pause_fixture(app: &App, nodes: Vec<DynamicNodeState>) {
        let task_id = "task-001";
        let run_id = "run-001";
        let round_id = "round-001";
        let outer_node_id = "ai-dynamic";
        let outer_attempt_id = "attempt-001";
        let started_at = "2026-06-16T00:00:00Z".to_string();
        write_json(
            &app.paths.run_file(task_id, run_id),
            &RunState {
                version: VERSION.to_string(),
                id: run_id.to_string(),
                task_id: task_id.to_string(),
                task_uuid: None,
                status: RunStatus::Running,
                outcome: None,
                started_at: started_at.clone(),
                updated_at: started_at.clone(),
                workflow_snapshot: "workflow.snapshot.json".to_string(),
                current_round: Some(round_id.to_string()),
                current_node: Some(outer_node_id.to_string()),
                current_attempt: Some(outer_attempt_id.to_string()),
                new_rounds_opened: 0,
                pause_reason: None,
                uuid: None,
                last_executed_node: None,
                worktree: None,
                execution: crate::runtime::RuntimeExecutionState::new(
                    RuntimeExecutionPhase::RunningNode,
                    Some(crate::runtime::RuntimeAttemptLocator {
                        round_id: round_id.to_string(),
                        node_id: outer_node_id.to_string(),
                        attempt_id: outer_attempt_id.to_string(),
                        outer_node_id: None,
                        outer_attempt_id: None,
                    }),
                    started_at.clone(),
                ),
            },
        )
        .unwrap();
        write_json(
            &app.paths.round_file(task_id, run_id, round_id),
            &RoundState {
                version: VERSION.to_string(),
                id: round_id.to_string(),
                run_id: run_id.to_string(),
                index: 1,
                status: RunStatus::Running,
                outcome: None,
                trigger: RoundTrigger::Initial,
                started_at: started_at.clone(),
                trace: Vec::new(),
                uuid: None,
            },
        )
        .unwrap();
        write_json(
            &app.paths
                .node_file(task_id, run_id, round_id, outer_node_id, outer_attempt_id),
            &NodeState {
                version: VERSION.to_string(),
                acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
                node_id: outer_node_id.to_string(),
                node_type: NodeType::AiDynamic,
                run_id: run_id.to_string(),
                round_id: round_id.to_string(),
                attempt_id: outer_attempt_id.to_string(),
                status: RunStatus::Running,
                outcome: None,
                started_at: started_at.clone(),
                finished_at: None,
                manual_check_pending: false,
                runtime_execution_id: None,
                resolved_config: Default::default(),
                uuid: None,
            },
        )
        .unwrap();
        let graph = DynamicGraphState {
            version: crate::dynamic_store::CURRENT_DYNAMIC_GRAPH_VERSION.to_string(),
            run: DynamicRunState {
                version: VERSION.to_string(),
                id: "dynamic-run-001".to_string(),
                parent_run_id: run_id.to_string(),
                parent_round_id: round_id.to_string(),
                parent_node_id: outer_node_id.to_string(),
                parent_attempt_id: outer_attempt_id.to_string(),
                status: DynamicRunStatus::Running,
                phase: DynamicRunPhase::Executing,
                outcome: None,
                pause_reason: None,
                started_at: started_at.clone(),
                updated_at: started_at,
                control: Default::default(),
                allowed_workflow_snapshots: Vec::new(),
                current_node_ids: nodes.iter().map(|node| node.id.clone()).collect(),
            },
            nodes,
            groups: Vec::new(),
            workspaces: vec![WorkspaceState {
                version: VERSION.to_string(),
                id: "workspace-main".to_string(),
                dynamic_run_id: "dynamic-run-001".to_string(),
                kind: WorkspaceKind::Main,
                ownership: WorkspaceOwnership::User,
                repo_root: app.paths.repo_root.clone(),
                path: app.paths.repo_root.clone(),
                branch: None,
                parent_workspace_id: None,
                created_by_group_id: None,
                fork_commit: "test-head".to_string(),
                checkpoint_commit: None,
                status: WorkspaceStatus::Active,
                created_at: "2026-06-16T00:00:00Z".to_string(),
                updated_at: "2026-06-16T00:00:00Z".to_string(),
            }],
            proposals: Vec::new(),
        };
        write_json(
            &app.paths.dynamic_graph_file(
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
            ),
            &graph,
        )
        .unwrap();
        write_json(
            &app.paths
                .dynamic_run_file(task_id, run_id, round_id, outer_node_id, outer_attempt_id),
            &graph.run,
        )
        .unwrap();
        for node in &graph.nodes {
            write_json(
                &app.paths.dynamic_node_file(
                    task_id,
                    run_id,
                    round_id,
                    outer_node_id,
                    outer_attempt_id,
                    &node.id,
                ),
                node,
            )
            .unwrap();
            let attempt_dir = app.paths.dynamic_node_attempt_dir(
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
                &node.id,
                "attempt-001",
            );
            std::fs::create_dir_all(attempt_dir.as_std_path()).unwrap();
            if node.status == DynamicNodeStatus::Completed {
                write_json(
                    &attempt_dir.join("acp.session.json"),
                    &serde_json::json!({
                        "sessionId": format!("{}-session", node.id),
                        "availability": "established",
                        "latestTurnStatus": "completed"
                    }),
                )
                .unwrap();
            }
        }
    }

    #[test]
    fn run_pause_marks_dynamic_descendant_attempts_cancelled() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let app = dynamic_pause_test_app(&temp);
        write_dynamic_pause_fixture(
            &app,
            vec![
                dynamic_pause_node("good-morning", DynamicNodeStatus::Running),
                dynamic_pause_node("good-night", DynamicNodeStatus::Running),
            ],
        );

        app.run_pause("task-001", "run-001", PauseReason::ProcessInterrupted)
            .unwrap();

        for node_id in ["good-morning", "good-night"] {
            let attempt_dir = app.paths.dynamic_node_attempt_dir(
                "task-001",
                "run-001",
                "round-001",
                "ai-dynamic",
                "attempt-001",
                node_id,
                "attempt-001",
            );
            let session: serde_json::Value =
                read_json(&attempt_dir.join("acp.session.json")).unwrap();
            assert_eq!(
                session
                    .get("latestTurnStatus")
                    .and_then(|value| value.as_str()),
                Some("cancelled")
            );
            assert_eq!(
                session.get("stopReason").and_then(|value| value.as_str()),
                Some("cancelled")
            );
        }
    }

    #[test]
    fn run_pause_keeps_completed_dynamic_descendant_session_terminal() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let app = dynamic_pause_test_app(&temp);
        let mut completed = dynamic_pause_node("good-night", DynamicNodeStatus::Completed);
        completed.outcome = Some(NodeOutcome::Success);
        completed.finished_at = Some("2026-06-16T00:00:01Z".to_string());
        write_dynamic_pause_fixture(
            &app,
            vec![
                dynamic_pause_node("good-morning", DynamicNodeStatus::Running),
                completed,
            ],
        );

        app.run_pause("task-001", "run-001", PauseReason::ProcessInterrupted)
            .unwrap();

        let running_attempt_dir = app.paths.dynamic_node_attempt_dir(
            "task-001",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
            "good-morning",
            "attempt-001",
        );
        let completed_attempt_dir = app.paths.dynamic_node_attempt_dir(
            "task-001",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
            "good-night",
            "attempt-001",
        );
        let running_session: serde_json::Value =
            read_json(&running_attempt_dir.join("acp.session.json")).unwrap();
        let completed_session: serde_json::Value =
            read_json(&completed_attempt_dir.join("acp.session.json")).unwrap();

        assert_eq!(
            running_session
                .get("latestTurnStatus")
                .and_then(|value| value.as_str()),
            Some("cancelled")
        );
        assert_eq!(
            completed_session
                .get("latestTurnStatus")
                .and_then(|value| value.as_str()),
            Some("completed")
        );
    }

    #[test]
    fn pause_dynamic_attempt_keeps_parent_running_when_sibling_is_active() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let app = dynamic_pause_test_app(&temp);
        write_dynamic_pause_fixture(
            &app,
            vec![
                dynamic_pause_node("good-morning", DynamicNodeStatus::Running),
                dynamic_pause_node("good-night", DynamicNodeStatus::Running),
            ],
        );

        app.pause_dynamic_attempt_runtime_state(
            "task-001",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
            "good-morning",
            PauseReason::ProcessInterrupted,
        )
        .unwrap();

        let run: RunState = read_json(&app.paths.run_file("task-001", "run-001")).unwrap();
        let graph: DynamicGraphState = read_json(&app.paths.dynamic_graph_file(
            "task-001",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
        ))
        .unwrap();
        let target: DynamicNodeState = read_json(&app.paths.dynamic_node_file(
            "task-001",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
            "good-morning",
        ))
        .unwrap();

        assert_eq!(run.status, RunStatus::Running);
        assert_eq!(run.pause_reason, None);
        assert_eq!(graph.run.status, DynamicRunStatus::Running);
        assert_eq!(graph.run.current_node_ids, vec!["good-night".to_string()]);
        assert_eq!(target.status, DynamicNodeStatus::Paused);
        assert_eq!(target.outcome, None);
        assert_eq!(target.runtime_execution_id, None);
        assert_eq!(
            target.runtime_execution_phase,
            Some(RuntimeExecutionPhase::Paused)
        );
        assert_eq!(run.execution.phase, RuntimeExecutionPhase::RunningNode);
    }

    #[test]
    fn pause_dynamic_attempt_pauses_parent_when_no_active_leaf_remains() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let app = dynamic_pause_test_app(&temp);
        write_dynamic_pause_fixture(
            &app,
            vec![dynamic_pause_node("good-night", DynamicNodeStatus::Running)],
        );

        app.pause_dynamic_attempt_runtime_state(
            "task-001",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
            "good-night",
            PauseReason::ProcessInterrupted,
        )
        .unwrap();

        let run: RunState = read_json(&app.paths.run_file("task-001", "run-001")).unwrap();
        let round: RoundState =
            read_json(&app.paths.round_file("task-001", "run-001", "round-001")).unwrap();
        let outer_node: NodeState = read_json(&app.paths.node_file(
            "task-001",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
        ))
        .unwrap();
        let graph: DynamicGraphState = read_json(&app.paths.dynamic_graph_file(
            "task-001",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
        ))
        .unwrap();

        assert_eq!(run.status, RunStatus::Paused);
        assert_eq!(run.pause_reason, Some(PauseReason::ProcessInterrupted));
        assert_eq!(round.status, RunStatus::Paused);
        assert_eq!(outer_node.status, RunStatus::Paused);
        assert_eq!(outer_node.outcome, None);
        assert_eq!(graph.run.status, DynamicRunStatus::Paused);
        assert_eq!(
            graph.run.pause_reason,
            Some(PauseReason::ProcessInterrupted)
        );
        assert!(graph.run.current_node_ids.is_empty());
        assert_eq!(
            graph.nodes[0].runtime_execution_phase,
            Some(RuntimeExecutionPhase::Paused)
        );
        assert_eq!(run.execution.phase, RuntimeExecutionPhase::Paused);
    }

    #[test]
    fn late_dynamic_continue_failure_does_not_override_user_stop() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let app = dynamic_pause_test_app(&temp);
        let mut first_execution = dynamic_pause_node("good-night", DynamicNodeStatus::Running);
        first_execution.begin_runtime_execution("execution-a", "2026-06-16T00:00:02Z".to_string());
        write_dynamic_pause_fixture(&app, vec![first_execution]);

        app.pause_dynamic_attempt_runtime_state(
            "task-001",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
            "good-night",
            PauseReason::ProcessInterrupted,
        )
        .unwrap();
        let mut resumed_execution = dynamic_pause_node("good-night", DynamicNodeStatus::Running);
        resumed_execution
            .begin_runtime_execution("execution-b", "2026-06-16T00:00:03Z".to_string());
        write_dynamic_pause_fixture(&app, vec![resumed_execution]);
        assert!(
            !app.pause_dynamic_attempt_runtime_state_if_active_execution(
                "task-001",
                "run-001",
                "round-001",
                "ai-dynamic",
                "attempt-001",
                "good-night",
                "execution-a",
                PauseReason::RuntimeAbnormal,
            )
            .unwrap()
        );

        let run: RunState = read_json(&app.paths.run_file("task-001", "run-001")).unwrap();
        let graph: DynamicGraphState = read_json(&app.paths.dynamic_graph_file(
            "task-001",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
        ))
        .unwrap();
        assert_eq!(run.status, RunStatus::Running);
        assert_eq!(run.pause_reason, None);
        assert_eq!(graph.run.status, DynamicRunStatus::Running);
        assert_eq!(graph.run.pause_reason, None);
        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Running);
        assert_eq!(
            graph.nodes[0].runtime_execution_id.as_deref(),
            Some("execution-b")
        );
    }

    #[test]
    fn dynamic_continue_failure_reclaims_rearmed_leaf_state() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let app = dynamic_pause_test_app(&temp);
        let mut rearmed = dynamic_pause_node("good-night", DynamicNodeStatus::Ready);
        rearmed.begin_runtime_execution("execution-a", "2026-06-16T00:00:02Z".to_string());
        write_dynamic_pause_fixture(&app, vec![rearmed]);

        assert!(
            app.pause_dynamic_attempt_runtime_state_if_active_execution(
                "task-001",
                "run-001",
                "round-001",
                "ai-dynamic",
                "attempt-001",
                "good-night",
                "execution-a",
                PauseReason::RuntimeAbnormal,
            )
            .unwrap()
        );

        let run: RunState = read_json(&app.paths.run_file("task-001", "run-001")).unwrap();
        let graph: DynamicGraphState = read_json(&app.paths.dynamic_graph_file(
            "task-001",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
        ))
        .unwrap();
        assert_eq!(run.status, RunStatus::Paused);
        assert_eq!(run.pause_reason, Some(PauseReason::RuntimeAbnormal));
        assert_eq!(graph.run.status, DynamicRunStatus::Paused);
        assert_eq!(graph.run.pause_reason, Some(PauseReason::RuntimeAbnormal));
        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Paused);
        assert_eq!(
            graph.nodes[0].pause_reason,
            Some(PauseReason::RuntimeAbnormal)
        );
    }

    #[test]
    fn cancel_all_active_acp_attempts_also_cancels_follow_up_session_on_completed_run() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        std::fs::create_dir_all(repo_root.as_std_path()).unwrap();
        let app = test_app(repo_root);
        let task_id = "task-001";
        let run_id = "run-001";
        let round_id = "round-001";
        let node_id = "plan";
        let attempt_id = "attempt-001";

        write_json(&app.paths.task_file(task_id), &TaskState::new(task_id)).unwrap();
        write_json(
            &app.paths.run_file(task_id, run_id),
            &RunState {
                version: VERSION.to_string(),
                id: run_id.to_string(),
                task_id: task_id.to_string(),
                task_uuid: None,
                status: RunStatus::Completed,
                outcome: Some(crate::domain::RunOutcome::Success),
                started_at: "2026-06-28T00:00:00Z".to_string(),
                updated_at: "2026-06-28T00:00:01Z".to_string(),
                workflow_snapshot: "workflow.snapshot.json".to_string(),
                current_round: Some(round_id.to_string()),
                current_node: Some(node_id.to_string()),
                current_attempt: Some(attempt_id.to_string()),
                new_rounds_opened: 0,
                pause_reason: None,
                uuid: None,
                last_executed_node: None,
                worktree: None,
                execution: Default::default(),
            },
        )
        .unwrap();
        write_json(
            &app.paths.round_file(task_id, run_id, round_id),
            &RoundState {
                version: VERSION.to_string(),
                id: round_id.to_string(),
                run_id: run_id.to_string(),
                index: 1,
                status: RunStatus::Completed,
                outcome: Some(crate::domain::RunOutcome::Success),
                trigger: RoundTrigger::Initial,
                started_at: "2026-06-28T00:00:00Z".to_string(),
                trace: Vec::new(),
                uuid: None,
            },
        )
        .unwrap();
        write_json(
            &app.paths
                .node_file(task_id, run_id, round_id, node_id, attempt_id),
            &NodeState {
                version: VERSION.to_string(),
                acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
                node_id: node_id.to_string(),
                node_type: NodeType::Worker,
                run_id: run_id.to_string(),
                round_id: round_id.to_string(),
                attempt_id: attempt_id.to_string(),
                status: RunStatus::Completed,
                outcome: Some(NodeOutcome::Success),
                started_at: "2026-06-28T00:00:00Z".to_string(),
                finished_at: Some("2026-06-28T00:00:01Z".to_string()),
                manual_check_pending: false,
                runtime_execution_id: None,
                resolved_config: Default::default(),
                uuid: None,
            },
        )
        .unwrap();
        let attempt_dir = app
            .paths
            .attempt_dir(task_id, run_id, round_id, node_id, attempt_id);
        std::fs::create_dir_all(attempt_dir.as_std_path()).unwrap();
        write_json(
            &attempt_dir.join("acp.snapshot.json"),
            &serde_json::json!({
                "sessionId": "session-follow-up",
                "availability": "established",
                "latestTurnStatus": "none"
            }),
        )
        .unwrap();
        write_json(
            &pending_elicitation_file(&attempt_dir, "elicit-001"),
            &PendingElicitationState {
                elicitation_id: "elicit-001".to_string(),
                jsonrpc_id: serde_json::json!(1),
                request: serde_json::from_value(serde_json::json!({
                    "mode": "form",
                    "sessionId": "session-test",
                    "message": "继续吗",
                    "requestedSchema": { "type": "object", "properties": {} }
                }))
                .unwrap(),
                created_at: "1Z".to_string(),
                timeline_identity: None,
            },
        )
        .unwrap();

        app.cancel_all_active_acp_attempts_best_effort();

        let session: serde_json::Value = read_json(&attempt_dir.join("acp.snapshot.json")).unwrap();
        assert_eq!(
            session
                .get("latestTurnStatus")
                .and_then(|value| value.as_str()),
            Some("cancelled")
        );
        assert!(
            attempt_dir
                .join("acp.elicitation-response.elicit-001.json")
                .exists()
        );
    }

    #[test]
    fn runtime_control_only_metadata_is_not_an_active_acp_session() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        std::fs::create_dir_all(repo_root.as_std_path()).unwrap();
        let app = test_app(repo_root);
        let attempt_dir = app.paths.attempt_dir(
            "task-001",
            "run-001",
            "round-001",
            "node-001",
            "attempt-001",
        );
        write_json(
            &attempt_dir.join("acp.snapshot.json"),
            &serde_json::json!({
                "availability": "established",
                "latestTurnStatus": "none",
                "runtimeControl": {
                    "currentMode": "non-runtime-controlled",
                    "transitionId": "runtime-control-test",
                    "transitionCause": "runtime-interrupted",
                    "changedAt": "1Z"
                }
            }),
        )
        .unwrap();

        assert!(!app.attempt_has_active_acp_session(&attempt_dir));
    }

    #[test]
    fn runtime_log_tail_reads_only_last_requested_lines() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = test_app(repo_root);
        std::fs::create_dir_all(app.paths.logs_dir().as_std_path()).unwrap();
        std::fs::write(
            app.paths.runtime_log_file().as_std_path(),
            (1..=1000)
                .map(|n| format!("line-{n}"))
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();

        let tail = app.runtime_log_tail_show(3).unwrap().unwrap();
        assert_eq!(tail, "line-998\nline-999\nline-1000");
    }

    #[test]
    fn touch_runtime_log_creates_file_before_first_event() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = test_app(repo_root);
        touch_log_file_best_effort(&app.paths);

        assert!(app.paths.runtime_log_file().as_std_path().exists());
    }

    #[test]
    fn user_console_theme_is_persisted_to_settings() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = test_app(repo_root.clone());
        app.set_user_console_theme(ConsoleThemeName::Nord).unwrap();

        let settings = app.load_settings().unwrap();
        assert_eq!(settings.console_theme, Some(ConsoleThemeName::Nord));
    }

    #[test]
    fn desktop_preferences_persisted_to_settings() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = test_app(repo_root.clone());
        let mut personalization = PersonalizationPreference::default();
        personalization.typography.ui.font_stack = FontStackPreference::Custom {
            families: vec!["Microsoft YaHei UI".to_string()],
        };
        personalization.typography.ui.font_size = FontSizePreference::Custom { px: 16 };
        personalization.typography.editor.font_stack = FontStackPreference::Custom {
            families: vec!["Fira Code".to_string()],
        };
        personalization.typography.editor.font_size = FontSizePreference::Custom { px: 13 };
        app.set_user_desktop_preferences(
            AppearancePreference {
                schema_version: 2,
                theme_id: "builtin.tech-neutral".to_string(),
                color_scheme: ColorSchemePreference::Dark,
                visual_quality_by_theme: BTreeMap::new(),
            },
            personalization.clone(),
            DesktopLanguage::En,
            true,
            true,
        )
        .unwrap();

        let settings = app.load_settings().unwrap();
        let appearance = settings.appearance.expect("appearance should persist");
        assert_eq!(appearance.theme_id, "builtin.tech-neutral");
        assert_eq!(appearance.color_scheme, ColorSchemePreference::Dark);
        assert!(appearance.visual_quality_by_theme.is_empty());
        assert_eq!(settings.desktop_language, Some(DesktopLanguage::En));
        assert_eq!(settings.personalization, Some(personalization));
        assert_eq!(settings.use_local_claude, Some(true));
        assert!(matches!(settings.log_level, Some(RuntimeLogLevel::Debug)));

        let state = app.load_state().unwrap();
        assert!(state.desktop_updater_last_checked_at.is_none());
        assert!(state.desktop_available_update.is_none());
    }

    #[test]
    fn updater_state_persisted_to_state_json() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = test_app(repo_root.clone());
        app.set_user_desktop_updater_last_checked_at(Some("2026-05-27 10:00:00".to_string()))
            .unwrap();
        let badges = DesktopUpdateBadgeState {
            settings_entry_seen_version: Some("0.3.1".to_string()),
            settings_advanced_seen_version: None,
            announcement_closed_version: Some("0.3.0".to_string()),
        };
        app.set_user_desktop_update_badges(badges).unwrap();

        let state = app.load_state().unwrap();
        assert_eq!(
            state.desktop_updater_last_checked_at.as_deref(),
            Some("2026-05-27 10:00:00")
        );
        assert_eq!(
            state
                .desktop_update_badges
                .settings_entry_seen_version
                .as_deref(),
            Some("0.3.1")
        );
        assert_eq!(
            state
                .desktop_update_badges
                .announcement_closed_version
                .as_deref(),
            Some("0.3.0")
        );

        let settings = app.load_settings().unwrap();
        assert!(settings.console_theme.is_none());
    }

    #[test]
    fn workspace_selection_only_updates_legacy_recent_list() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = test_app(repo_root.clone());
        app.record_user_recent_desktop_workspace("D:/Projects/MyRepo")
            .unwrap();

        let state = app.load_state().unwrap();
        assert!(
            state
                .recent_desktop_workspaces
                .contains(&"D:/Projects/MyRepo".to_string())
        );
    }

    #[test]
    fn recent_workspaces_deduplicated_and_truncated() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = test_app(repo_root.clone());
        app.record_user_recent_desktop_workspace("D:/Projects/A")
            .unwrap();
        app.record_user_recent_desktop_workspace("D:/Projects/B")
            .unwrap();
        app.record_user_recent_desktop_workspace("D:/Projects/A")
            .unwrap();

        let state = app.load_state().unwrap();
        // A should be at position 0 (most recent), B at position 1, no duplicates
        assert_eq!(state.recent_desktop_workspaces.len(), 2);
        assert_eq!(state.recent_desktop_workspaces[0], "D:/Projects/A");
        assert_eq!(state.recent_desktop_workspaces[1], "D:/Projects/B");
    }

    #[test]
    fn recent_workspace_can_be_removed_without_switching_workspace() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = test_app(repo_root.clone());
        app.record_user_recent_desktop_workspace("D:/Projects/A")
            .unwrap();
        app.record_user_recent_desktop_workspace("D:/Projects/B")
            .unwrap();

        let state = app
            .remove_user_recent_desktop_workspace("D:/Projects/A")
            .unwrap();

        assert_eq!(state.recent_desktop_workspaces, vec!["D:/Projects/B"]);
    }

    #[test]
    fn recent_workspaces_truncated_at_eight() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = test_app(repo_root.clone());
        for i in 0..10 {
            app.record_user_recent_desktop_workspace(&format!("D:/Projects/Repo{i}"))
                .unwrap();
        }

        let state = app.load_state().unwrap();
        assert_eq!(state.recent_desktop_workspaces.len(), 8);
        assert_eq!(state.recent_desktop_workspaces[0], "D:/Projects/Repo9");
    }
    #[test]
    fn default_workflow_interview_node_requires_manual_check() {
        let paths =
            crate::storage::GoldBandPaths::new(Utf8PathBuf::from("/tmp/interview-default-success"));
        let profiles = super::ensure_default_user_profiles(&paths).unwrap();
        let workflow = super::default_workflow_dsl("claude-acp", &profiles, DesktopLanguage::ZhCn);

        let interview = workflow
            .nodes
            .iter()
            .find_map(|node| match node {
                NodeDsl::Worker(worker) if worker.id == "interview" => Some(worker),
                _ => None,
            })
            .expect("default workflow contains an interview node");
        let interview_node = NodeDsl::Worker(interview.clone());

        // 采访节点不使用 AI 输出验证，产出 interview-spec.md 后等待用户人工判定。
        assert!(
            interview_node.manual_check_enabled(),
            "interview node must require manual check"
        );
        assert!(
            interview.output.is_none() && interview.success_condition.is_none(),
            "interview node must declare no output contract or success condition"
        );
    }
}
