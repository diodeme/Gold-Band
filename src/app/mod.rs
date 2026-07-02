mod ids;
mod node_executor;
mod notification;
pub mod observability;
mod orchestrator;
mod profile_resolver;
mod profiles;
mod state_access;
mod state_factory;
mod transition_context;

pub use self::notification::{
    InterventionNotification, InterventionType, NotificationDedup, make_dedup_key, reason_key,
};

use crate::acp::client as acp_client;
use crate::acp::permission::cancel_pending_permission_requests;
use crate::config::{
    ConsoleThemeName, ConversationAutoConfig, DesktopAvailableUpdate, DesktopFontPreference,
    DesktopLanguage, DesktopThemePreference, DesktopUpdateBadgeState, ManagedAgentConfig,
    ManagedAgentType, McpServerConfig, McpServerHealthResult, RuntimeConfig, RuntimeLogLevel,
    SettingsConfig, SkillMeta, SkillSource, StateConfig,
};
use crate::control::{ControlDecision, decide_next_step};
use crate::domain::{NodeOutcome, RunOutcome};
use crate::domain::{PauseReason, RunStatus, SessionMode, VERSION};
use crate::dsl::{
    AiDynamicAgentStrategy, END_NODE, EdgeDsl, EdgeOutcome, JsonConditionDsl, NEW_ROUND_NODE,
    NodeDsl, OutputContractDsl, OutputKind, ValidatedWorkflow, WorkerNode, WorkflowControl,
    WorkflowDsl, WorkflowValidationError, validate_workflow, workflow_contains_ai_dynamic,
};
use crate::dynamic::{
    DynamicGraphState, DynamicGroupStatus, DynamicNodeStatus, DynamicRunStatus,
    dynamic_graph_has_active_leaf, dynamic_leaf_is_active, refresh_dynamic_current_leaf_ids,
};
use crate::mcp::McpManager;
use crate::process::kill_process_tree;
use crate::provider::{
    DoctorResult, PromptBundle, PromptVisibility, ProviderAdapter, ProviderCapabilities,
    ProviderInfo, provider_capabilities, provider_from_agent, render_prompt_bundle,
};
use crate::runtime::{
    NodeState, RoundState, RunState, TaskState, WorkerRefState, validate_node_state,
    validate_round_state, validate_run_state, validate_task_state, validate_worker_ref_state,
};
use crate::storage::{GoldBandPaths, ensure_parent_dir, read_json, write_json};
use anyhow::{Context, Result, anyhow, bail};
use camino::{Utf8Path, Utf8PathBuf};
use serde::de::DeserializeOwned;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::str::FromStr;
use std::sync::Arc;

use self::ids::{generate_uuid, next_task_id, next_workflow_id, now_rfc3339_like};
use self::orchestrator::{
    build_dynamic_prompt_bundle, run_continue as orchestrator_run_continue,
    run_continue_background as orchestrator_run_continue_background,
    run_retry as orchestrator_run_retry, run_start as orchestrator_run_start,
    run_start_background as orchestrator_run_start_background,
    submit_manual_check as orchestrator_submit_manual_check,
    submit_manual_check_background as orchestrator_submit_manual_check_background,
};
use self::profile_resolver::resolve_workflow_profiles;
use self::profiles::{
    DefaultProfileIds, create_profile, delete_profile as delete_profile_file,
    ensure_default_user_profiles, list_profiles, show_profile, update_profile,
};
pub use self::profiles::{
    ProfileCommandError, ProfileEntry, ProfileInput, ProfileList, ProfileScope,
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

fn default_workflow_template(profiles: &DefaultProfileIds) -> WorkflowTemplate {
    let now = now_rfc3339_like();
    WorkflowTemplate {
        id: "default".to_string(),
        name: "默认工作流".to_string(),
        workflow: default_workflow_dsl(ManagedAgentType::ClaudeAcp.as_str(), profiles),
        created_at: now.clone(),
        updated_at: now,
    }
}

fn default_workflow_dsl(provider: &str, profiles: &DefaultProfileIds) -> WorkflowDsl {
    fn worker(
        provider: &str,
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
            provider: Some(provider.to_string()),
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
            permission_mode: Some("bypassPermissions".to_string()),
            manual_check: manual_check.then_some(true),
        })
    }

    WorkflowDsl {
        version: "0.1".to_string(),
        id: "task-workflow".to_string(),
        entry: "plan".to_string(),
        control: WorkflowControl {
            max_attempts: None,
            max_rounds: None,
        },
        nodes: vec![
            worker(
                provider,
                profiles,
                "plan",
                "plan",
                "Analyze the imported requirement and produce an implementation plan.",
                false,
                true,
            ),
            worker(
                provider,
                profiles,
                "dev",
                "dev",
                "Implement the requirement in the workspace.",
                false,
                false,
            ),
            worker(
                provider,
                profiles,
                "review",
                "review",
                "Review the implementation and return JSON with result and reason fields.",
                true,
                false,
            ),
            worker(
                provider,
                profiles,
                "test",
                "test",
                "Run or describe verification and return JSON with result and reason fields.",
                true,
                false,
            ),
            worker(
                provider,
                profiles,
                "accept",
                "accept",
                "Validate acceptance and return JSON with result and reason fields.",
                true,
                false,
            ),
            worker(
                provider,
                profiles,
                "cleanup",
                "cleanup",
                "Clean up resources, finalize handoff notes, clean up Git workspace",
                false,
                false,
            ),
        ],
        edges: vec![
            EdgeDsl {
                from: "plan".to_string(),
                to: "dev".to_string(),
                on: EdgeOutcome::Success,
                session: None,
            },
            EdgeDsl {
                from: "dev".to_string(),
                to: "review".to_string(),
                on: EdgeOutcome::Success,
                session: None,
            },
            EdgeDsl {
                from: "review".to_string(),
                to: "test".to_string(),
                on: EdgeOutcome::Success,
                session: None,
            },
            EdgeDsl {
                from: "review".to_string(),
                to: "dev".to_string(),
                on: EdgeOutcome::Failure,
                session: Some(SessionMode::Continue),
            },
            EdgeDsl {
                from: "test".to_string(),
                to: "accept".to_string(),
                on: EdgeOutcome::Success,
                session: None,
            },
            EdgeDsl {
                from: "test".to_string(),
                to: "dev".to_string(),
                on: EdgeOutcome::Failure,
                session: Some(SessionMode::Continue),
            },
            EdgeDsl {
                from: "accept".to_string(),
                to: "cleanup".to_string(),
                on: EdgeOutcome::Success,
                session: None,
            },
            EdgeDsl {
                from: "cleanup".to_string(),
                to: END_NODE.to_string(),
                on: EdgeOutcome::Success,
                session: None,
            },
            EdgeDsl {
                from: "accept".to_string(),
                to: NEW_ROUND_NODE.to_string(),
                on: EdgeOutcome::Failure,
                session: None,
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

fn unique_auto_template_id(store: &AutoTemplateStore, name: &str) -> String {
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
        "auto-template".to_string()
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
    pub workflow: WorkflowDsl,
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
    PermissionRequested,
    ErrorBlocked,
    ProcessInterrupted,
}

impl From<PauseReason> for RuntimeInterventionKind {
    fn from(reason: PauseReason) -> Self {
        match reason {
            PauseReason::WaitingForUserInput => Self::ManualDecisionRequired,
            PauseReason::PermissionRequested => Self::PermissionRequested,
            PauseReason::ErrorBlocked => Self::ErrorBlocked,
            PauseReason::ProcessInterrupted => Self::ProcessInterrupted,
        }
    }
}

impl From<RuntimeInterventionKind> for PauseReason {
    fn from(kind: RuntimeInterventionKind) -> Self {
        match kind {
            RuntimeInterventionKind::ManualDecisionRequired => Self::WaitingForUserInput,
            RuntimeInterventionKind::PermissionRequested => Self::PermissionRequested,
            RuntimeInterventionKind::ErrorBlocked => Self::ErrorBlocked,
            RuntimeInterventionKind::ProcessInterrupted => Self::ProcessInterrupted,
        }
    }
}

#[derive(Debug, Clone)]
pub enum RuntimeLifecycleEvent {
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
        node_id: String,
        node_uuid: Option<String>,
        attempt_id: String,
        // ── Metadata ──
        repo_root: String,
        seq: Option<u32>,
        node_name: Option<String>,
        agent_type: Option<String>,
        started_at: String,
        /// Path to the current node's attempt directory. `None` because the
        /// node just started — attempt dir hasn't been populated yet.
        attempt_dir: Option<String>,
        /// The immediately preceding node in this run (None for first node).
        predecessor: Option<crate::runtime::LastExecutedNode>,
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
        node_id: String,
        node_uuid: Option<String>,
        attempt_id: String,
        // ── Metadata ──
        repo_root: String,
        seq: Option<u32>,
        node_name: String,
        agent_type: Option<String>,
        started_at: String,
        finished_at: Option<String>,
        outcome: String, // "SUCCESS" | "FAILED"
        /// Path to this node's attempt directory for reading token data.
        attempt_dir: String,
        /// When true, the subscriber skips the 「结束」sentinel. Used by
        /// dynamic workers so that only the outer AiDynamic node produces
        /// the single begin/end sentinel pair for the whole workflow.
        suppress_sentinel: bool,
    },
    RunPaused {
        event_id: String,
        occurred_at: String,
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
        task_id: String,
        run_id: String,
        round_id: String,
        node_id: String,
        attempt_id: String,
        node_label: String,
        outcome: RunOutcome,
        task_title: Option<String>,
    },
}

pub struct App {
    pub paths: GoldBandPaths,
    pub config: RuntimeConfig,
    provider_override: Option<Arc<dyn ProviderAdapter>>,
    acp_live_update: Option<
        Arc<
            dyn Fn(AcpLiveEventContext, crate::acp::events::AcpUiEvent) -> Result<()> + Send + Sync,
        >,
    >,
    acp_session_update: Option<Arc<dyn Fn(AcpLiveEventContext) -> Result<()> + Send + Sync>>,
    pub lifecycle_bus: observability::RuntimeLifecycleBus,
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

pub fn is_run_continuable(run: &RunState) -> bool {
    run.status == RunStatus::Paused
        && run.outcome.is_none()
        && matches!(
            run.pause_reason,
            Some(
                PauseReason::ProcessInterrupted
                    | PauseReason::WaitingForUserInput
                    | PauseReason::ErrorBlocked
            )
        )
        && run.current_round.is_some()
        && run.current_node.is_some()
        && run.current_attempt.is_some()
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
            let validated = validate_workflow(template.workflow.clone()).map_err(|error| {
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

/// Returns (provider, model) pairs for model validation.
fn models_for_node(node: &NodeDsl) -> Vec<(String, Option<String>)> {
    match node {
        NodeDsl::Worker(worker) => worker
            .provider
            .as_ref()
            .map(|p| (p.clone(), worker.model.clone()))
            .into_iter()
            .collect(),
        NodeDsl::AiDynamic(dynamic) => match &dynamic.agent_strategy {
            AiDynamicAgentStrategy::Fixed { provider, model } => {
                vec![(provider.clone(), model.clone())]
            }
            AiDynamicAgentStrategy::Dynamic {
                bootstrap_provider,
                bootstrap_model,
                available_agents,
                ..
            } => {
                let mut pairs = vec![(bootstrap_provider.clone(), bootstrap_model.clone())];
                for agent_ref in available_agents {
                    pairs.push((agent_ref.provider.clone(), agent_ref.model.clone()));
                }
                pairs
            }
        },
    }
}

impl App {
    pub fn new(repo_root: Utf8PathBuf) -> Self {
        Self::with_config(repo_root, RuntimeConfig::default())
    }

    pub fn clone_for_background(&self) -> Self {
        Self {
            paths: self.paths.clone(),
            config: self.config.clone(),
            provider_override: self.provider_override.clone(),
            acp_live_update: self.acp_live_update.clone(),
            acp_session_update: self.acp_session_update.clone(),
            lifecycle_bus: self.lifecycle_bus.clone(),
        }
    }

    pub fn with_acp_live_update(
        mut self,
        live_update: Arc<
            dyn Fn(AcpLiveEventContext, crate::acp::events::AcpUiEvent) -> Result<()> + Send + Sync,
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
    ) -> Option<impl Fn(&crate::acp::events::AcpUiEvent) -> Result<()> + 'a> {
        let live_update = self.acp_live_update.as_ref()?.clone();
        Some(move |event: &crate::acp::events::AcpUiEvent| {
            live_update(context.clone(), event.clone())
        })
    }

    pub fn acp_session_update_for<'a>(
        &'a self,
        context: AcpLiveEventContext,
    ) -> Option<impl Fn() -> Result<()> + 'a> {
        let session_update = self.acp_session_update.as_ref()?.clone();
        Some(move || session_update(context.clone()))
    }

    pub fn emit_acp_session_update(&self, context: AcpLiveEventContext) -> Result<()> {
        if let Some(session_update) = &self.acp_session_update {
            session_update(context)?;
        }
        Ok(())
    }

    pub fn emit_lifecycle_event(&self, event: RuntimeLifecycleEvent) {
        self.lifecycle_bus.emit(event);
    }

    pub fn load_settings(&self) -> Result<SettingsConfig> {
        let path = self.paths.user_settings_file();
        if !path.exists() {
            return Ok(SettingsConfig::default());
        }
        read_json(&path)
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

    pub fn set_user_desktop_theme(&self, theme: DesktopThemePreference) -> Result<SettingsConfig> {
        let mut settings = self.load_settings()?;
        settings.desktop_theme = Some(theme);
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
        theme: DesktopThemePreference,
        language: DesktopLanguage,
        font: DesktopFontPreference,
    ) -> Result<SettingsConfig> {
        let mut settings = self.load_settings()?;
        settings.desktop_theme = Some(theme);
        settings.desktop_language = Some(language);
        settings.desktop_font = Some(font);
        self.save_settings(&settings)?;
        Ok(settings)
    }

    pub fn set_user_use_local_claude(&self, use_local_claude: bool) -> Result<SettingsConfig> {
        let mut settings = self.load_settings()?;
        settings.use_local_claude = Some(use_local_claude);
        self.save_settings(&settings)?;
        Ok(settings)
    }

    pub fn set_user_log_level(&self, log_level: RuntimeLogLevel) -> Result<SettingsConfig> {
        let mut settings = self.load_settings()?;
        settings.log_level = Some(log_level);
        self.save_settings(&settings)?;
        Ok(settings)
    }

    pub fn set_user_verbose_logging(&self, enabled: bool) -> Result<SettingsConfig> {
        self.set_user_log_level(if enabled {
            RuntimeLogLevel::Debug
        } else {
            RuntimeLogLevel::Info
        })
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

    pub fn set_user_desktop_workspace(
        &self,
        workspace: &str,
    ) -> Result<(SettingsConfig, StateConfig)> {
        let mut settings = self.load_settings()?;
        settings.desktop_workspace = Some(workspace.to_string());
        self.save_settings(&settings)?;

        let mut state = self.load_state()?;
        state
            .recent_desktop_workspaces
            .retain(|item| item != workspace);
        state
            .recent_desktop_workspaces
            .insert(0, workspace.to_string());
        state.recent_desktop_workspaces.truncate(8);
        self.save_state(&state)?;

        Ok((settings, state))
    }

    pub fn set_user_agents(
        &self,
        agents: std::collections::BTreeMap<ManagedAgentType, ManagedAgentConfig>,
    ) -> Result<SettingsConfig> {
        let mut settings = self.load_settings()?;
        settings.agents = Some(agents);
        self.save_settings(&settings)?;
        Ok(settings)
    }

    pub fn managed_agents(
        &self,
    ) -> &std::collections::BTreeMap<ManagedAgentType, ManagedAgentConfig> {
        &self.config.agents
    }

    pub fn save_managed_agent(
        &self,
        agent_type: ManagedAgentType,
        config: ManagedAgentConfig,
    ) -> Result<SettingsConfig> {
        let mut agents = self.config.agents.clone();
        if !agent_type.is_supported() {
            bail!("agent `{}` is not supported yet", agent_type.as_str());
        }
        agents.insert(agent_type, config);
        self.set_user_agents(agents)
    }

    pub fn remove_managed_agent(&self, agent_type: ManagedAgentType) -> Result<SettingsConfig> {
        let mut agents = self.config.agents.clone();
        agents.remove(&agent_type);
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

    pub fn enabled_mcp_servers(&self) -> Result<Vec<McpServerConfig>> {
        self.mcp_manager().enabled_servers()
    }

    pub fn acp_mcp_servers(&self) -> Result<Vec<serde_json::Value>> {
        self.mcp_manager().to_acp_mcp_servers()
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
        let name = name.trim();
        if name.is_empty() {
            bail!("workflow template name cannot be empty");
        }
        let mut store = self.load_workflow_template_store()?;
        let mut workflow = workflow;
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
        let validated = validate_workflow(workflow)?;
        self.validate_workflow_agents(&validated)?;
        resolve_workflow_profiles(&self.paths, &validated.raw, self.config.desktop_language)?;
        validate_unique_workflow_template_id(&store, &validated.raw, name, None)?;
        validate_ai_dynamic_allowed_workflows(&validated.raw, &store)?;

        let now = now_rfc3339_like();
        let id = unique_workflow_template_id(&store, name);
        store.templates.push(WorkflowTemplate {
            id: id.clone(),
            name: name.to_string(),
            workflow: validated.raw,
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
        let template_id = template_id.trim();
        if template_id.is_empty() {
            bail!("workflow template id cannot be empty");
        }
        if template_id == "default" {
            bail!("default workflow template cannot be updated");
        }
        let mut store = self.load_workflow_template_store()?;
        let validated = validate_workflow(workflow)?;
        self.validate_workflow_agents(&validated)?;
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
            .with_context(|| format!("workflow template `{template_id}` not found"))?;
        template.workflow = validated.raw;
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
        if template_id == "default" {
            bail!("default workflow template cannot be deleted");
        }

        let mut store = self.load_workflow_template_store()?;
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
        let id = unique_auto_template_id(&store, name);
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
                id = unique_auto_template_id(&store, name);
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
        let default_template = default_workflow_template(&default_profiles);
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
            if store.templates.is_empty() {
                store.templates.push(default_template);
            } else if let Some(template) = store
                .templates
                .iter_mut()
                .find(|template| template.id == "default")
            {
                *template = default_template;
            } else {
                store.templates.insert(0, default_template);
            }
            self.save_workflow_template_store(&store)?;
            return Ok(store);
        }
        let store = WorkflowTemplateStore {
            version: VERSION.to_string(),
            last_used_template_id: Some("default".to_string()),
            last_created_workflow: None,
            templates: vec![default_template],
        };
        self.save_workflow_template_store(&store)?;
        Ok(store)
    }

    fn save_workflow_template_store(&self, store: &WorkflowTemplateStore) -> Result<()> {
        fs::create_dir_all(self.paths.user_context_dir().as_std_path())?;
        write_json(&self.paths.workflow_templates_file(), store)
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

    pub fn managed_agent(&self, provider: &str) -> Result<(ManagedAgentType, &ManagedAgentConfig)> {
        let agent_type = ManagedAgentType::from_str(provider)?;
        let config = self
            .config
            .agents
            .get(&agent_type)
            .ok_or_else(|| anyhow!("agent `{provider}` is not configured"))?;
        Ok((agent_type, config))
    }

    pub fn provider_for_id(&self, provider: &str) -> Result<Arc<dyn ProviderAdapter>> {
        if let Some(provider_override) = &self.provider_override {
            return Ok(provider_override.clone());
        }
        let (agent_type, config) = self.managed_agent(provider)?;
        Ok(Arc::from(provider_from_agent(
            agent_type,
            config,
            self.config.use_local_claude,
            self.config.acp_session_title_refresh_enabled,
            self.config.acp_raw_max_size_bytes,
            self.config.acp_raw_target_size_bytes,
        )?))
    }

    pub fn provider_info(&self, provider: &str) -> Result<ProviderInfo> {
        Ok(self.provider_for_id(provider)?.describe_provider())
    }

    pub fn provider_doctor(&self, provider: &str) -> Result<DoctorResult> {
        let (agent_type, config) = self.managed_agent(provider)?;
        if !agent_type.is_supported() {
            bail!("agent `{provider}` is not supported yet");
        }
        match acp_client::doctor(
            &config.adapter,
            self.paths.repo_root.clone(),
            self.config.use_local_claude,
        ) {
            Ok(capabilities) => Ok(DoctorResult {
                available: true,
                reason: None,
                capabilities: Some(capabilities),
            }),
            Err(err) => Ok(DoctorResult {
                available: false,
                reason: Some(err.to_string()),
                capabilities: None,
            }),
        }
    }

    pub fn provider_capabilities(&self, provider: &str) -> Result<ProviderCapabilities> {
        provider_capabilities(provider)
    }

    pub fn with_config(repo_root: Utf8PathBuf, config: RuntimeConfig) -> Self {
        let paths = GoldBandPaths::new(repo_root);
        let _ = paths.write_project_manifest();
        let _ = ensure_default_user_profiles(&paths);
        Self {
            paths,
            config,
            provider_override: None,
            acp_live_update: None,
            acp_session_update: None,
            lifecycle_bus: observability::RuntimeLifecycleBus::new(),
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
        let paths = GoldBandPaths::new(repo_root);
        let _ = paths.write_project_manifest();
        let _ = ensure_default_user_profiles(&paths);
        Self {
            paths,
            config,
            provider_override: Some(Arc::from(provider)),
            acp_live_update: None,
            acp_session_update: None,
            lifecycle_bus: observability::RuntimeLifecycleBus::new(),
        }
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

        let validated = validate_workflow(input.workflow.clone())?;
        self.validate_workflow_agents(&validated)?;
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

        let task_id = next_task_id(&self.paths.tasks_dir())?;
        let task = TaskState {
            version: VERSION.to_string(),
            id: task_id.clone(),
            title: input.title.filter(|value| !value.trim().is_empty()),
            description: input.description.filter(|value| !value.trim().is_empty()),
            uuid: Some(generate_uuid()),
        };
        validate_task_state(&task)?;
        fs::create_dir_all(
            self.paths
                .task_dir(&task_id)
                .join("authoring")
                .as_std_path(),
        )?;
        write_json(&self.paths.task_file(&task_id), &task)?;
        fs::write(
            self.paths.requirement_file(&task_id).as_std_path(),
            input.requirement_content,
        )?;
        write_json(&self.paths.workflow_file(&task_id), &validated.raw)?;
        self.record_created_task_workflow(validated.raw, input.workflow_template_id)?;
        self.task_summary(&task_id)
    }

    pub fn save_task_workflow(&self, task_id: &str, workflow: WorkflowDsl) -> Result<TaskSummary> {
        self.task_show(task_id)?;
        let validated = validate_workflow(workflow)?;
        self.validate_workflow_agents(&validated)?;
        resolve_workflow_profiles(&self.paths, &validated.raw, self.config.desktop_language)?;
        let store = self.load_workflow_template_store()?;
        validate_ai_dynamic_allowed_workflows(&validated.raw, &store)?;
        fs::create_dir_all(self.paths.task_dir(task_id).join("authoring").as_std_path())?;
        write_json(&self.paths.workflow_file(task_id), &validated.raw)?;
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
        self.read_json_dir_sorted(&self.paths.runs_dir(task_id))
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
        let run: RunState = read_json(&self.paths.run_file(task_id, run_id))?;
        validate_run_state(&run)?;
        Ok(run)
    }

    pub fn run_kill(&self, task_id: &str, run_id: &str) -> Result<RunState> {
        let mut run = self.run_status(task_id, run_id)?;
        self.close_current_run_attempt(task_id, run_id, &run)?;
        run.status = RunStatus::Completed;
        run.outcome = Some(RunOutcome::Killed);
        run.pause_reason = None;
        run.updated_at = now_rfc3339_like();
        validate_run_state(&run)?;
        write_json(&self.paths.run_file(task_id, run_id), &run)?;

        if let Some(round_id) = &run.current_round {
            let mut round: RoundState =
                read_json(&self.paths.round_file(task_id, run_id, round_id))?;
            round.status = RunStatus::Completed;
            round.outcome = Some(RunOutcome::Killed);
            validate_round_state(&round)?;
            write_json(&self.paths.round_file(task_id, run_id, round_id), &round)?;

            if let (Some(node_id), Some(attempt_id)) = (&run.current_node, &run.current_attempt) {
                let node_path = self
                    .paths
                    .node_file(task_id, run_id, round_id, node_id, attempt_id);
                if node_path.exists() {
                    let mut node: NodeState = read_json(&node_path)?;
                    node.status = RunStatus::Completed;
                    node.outcome = Some(NodeOutcome::Killed);
                    node.finished_at = Some(now_rfc3339_like());
                    validate_node_state(&node)?;
                    write_json(&node_path, &node)?;
                    self.kill_dynamic_descendants_best_effort(
                        task_id, run_id, round_id, node_id, attempt_id,
                    );
                }
            }
        }

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
        acp_client::close_all_connections_bounded()?;
        Ok(paused)
    }

    pub fn recover_interrupted_running_sessions(&self) -> Result<Vec<RunState>> {
        self.pause_all_running_sessions()
    }

    fn close_current_run_attempt(&self, task_id: &str, run_id: &str, run: &RunState) -> Result<()> {
        let (Some(round_id), Some(node_id), Some(attempt_id)) =
            (&run.current_round, &run.current_node, &run.current_attempt)
        else {
            return Ok(());
        };
        self.close_attempt_artifacts(task_id, run_id, round_id, node_id, attempt_id)?;
        self.kill_dynamic_descendants_best_effort(task_id, run_id, round_id, node_id, attempt_id);
        Ok(())
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
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            Some(reason),
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
        self.cancel_attempt_dir_best_effort(&attempt_dir);
        self.request_attempt_prompt_cancel_best_effort(&attempt_dir);
        self.persist_cancelled_session_snapshot_best_effort(&attempt_dir);
    }

    fn close_attempt_artifacts(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Result<()> {
        let attempt_dir = self
            .paths
            .attempt_dir(task_id, run_id, round_id, node_id, attempt_id);
        self.cancel_attempt_dir_best_effort(&attempt_dir);
        acp_client::close_attempt_session_bounded(&attempt_dir)?;
        self.persist_cancelled_session_snapshot_best_effort(&attempt_dir);
        Ok(())
    }

    fn update_dynamic_descendants_best_effort(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        pause_reason: Option<PauseReason>,
    ) {
        let graph_path = self
            .paths
            .dynamic_graph_file(task_id, run_id, round_id, node_id, attempt_id);
        let Ok(mut graph) = read_json::<DynamicGraphState>(&graph_path) else {
            return;
        };

        for dynamic_node in &mut graph.nodes {
            let should_interrupt_attempts = match pause_reason {
                Some(_) => dynamic_leaf_is_active(dynamic_node.status),
                None => dynamic_node.status != DynamicNodeStatus::Completed,
            };
            if should_interrupt_attempts {
                let dynamic_node_dir = self.paths.dynamic_node_dir(
                    task_id,
                    run_id,
                    round_id,
                    node_id,
                    attempt_id,
                    &dynamic_node.id,
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
                        self.cancel_attempt_dir_best_effort(attempt_dir.as_path());
                        if pause_reason.is_some() {
                            self.request_attempt_prompt_cancel_best_effort(attempt_dir.as_path());
                        } else {
                            let _ =
                                acp_client::close_attempt_session_bounded(attempt_dir.as_path());
                        }
                        self.persist_cancelled_session_snapshot_best_effort(attempt_dir.as_path());
                    }
                }
            }
            if let Some(child_run_id) = dynamic_node.child_run_id.clone() {
                if let Some(reason) = pause_reason {
                    let _ = self.run_pause(task_id, &child_run_id, reason);
                } else {
                    let _ = self.run_kill(task_id, &child_run_id);
                }
            }
            match pause_reason {
                Some(_) => {
                    if dynamic_node.status != DynamicNodeStatus::Completed {
                        dynamic_node.status = DynamicNodeStatus::Paused;
                        dynamic_node.outcome = None;
                        dynamic_node.finished_at = Some(now_rfc3339_like());
                    }
                }
                None => {
                    dynamic_node.status = DynamicNodeStatus::Completed;
                    dynamic_node.outcome = Some(NodeOutcome::Killed);
                    dynamic_node
                        .finished_at
                        .get_or_insert_with(now_rfc3339_like);
                }
            }
        }

        if pause_reason.is_none() {
            for group in &mut graph.groups {
                if group.status != DynamicGroupStatus::Closed {
                    group.status = DynamicGroupStatus::Failed;
                    group.updated_at = now_rfc3339_like();
                }
            }
        }

        match pause_reason {
            Some(reason) => {
                graph.run.status = DynamicRunStatus::Paused;
                graph.run.outcome = None;
                graph.run.pause_reason = Some(reason);
            }
            None => {
                graph.run.status = DynamicRunStatus::Completed;
                graph.run.outcome = Some(RunOutcome::Killed);
                graph.run.pause_reason = None;
            }
        }
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
            let _ = write_json(
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
    }

    fn kill_dynamic_descendants_best_effort(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) {
        self.update_dynamic_descendants_best_effort(
            task_id, run_id, round_id, node_id, attempt_id, None,
        );
    }

    pub fn run_pause(&self, task_id: &str, run_id: &str, reason: PauseReason) -> Result<RunState> {
        let mut run = self.run_status(task_id, run_id)?;
        if run.status != RunStatus::Running {
            return Ok(run);
        }
        let now = now_rfc3339_like();
        let current_round = run.current_round.clone();
        let current_node = run.current_node.clone();
        let current_attempt = run.current_attempt.clone();
        self.interrupt_run_descendants_best_effort(task_id, run_id, &run, reason);
        run.status = RunStatus::Paused;
        run.outcome = None;
        run.pause_reason = Some(reason);
        run.updated_at = now.clone();
        validate_run_state(&run)?;
        write_json(&self.paths.run_file(task_id, run_id), &run)?;

        if let Some(round_id) = current_round.as_deref() {
            let mut round: RoundState =
                read_json(&self.paths.round_file(task_id, run_id, round_id))?;
            round.status = RunStatus::Paused;
            round.outcome = None;
            validate_round_state(&round)?;
            write_json(&self.paths.round_file(task_id, run_id, round_id), &round)?;

            if let (Some(node_id), Some(attempt_id)) =
                (current_node.as_deref(), current_attempt.as_deref())
            {
                let node_path = self
                    .paths
                    .node_file(task_id, run_id, round_id, node_id, attempt_id);
                if node_path.exists() {
                    let mut node: NodeState = read_json(&node_path)?;
                    if node.status != RunStatus::Completed {
                        node.status = RunStatus::Paused;
                        node.outcome = None;
                        node.finished_at = Some(now.clone());
                        validate_node_state(&node)?;
                        write_json(&node_path, &node)?;
                    }
                    self.update_dynamic_descendants_best_effort(
                        task_id,
                        run_id,
                        round_id,
                        node_id,
                        attempt_id,
                        Some(reason),
                    );
                }
            }
        }

        Ok(run)
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
        let now = now_rfc3339_like();
        let run_path = self.paths.run_file(task_id, run_id);
        if run_path.exists() {
            let mut run: RunState = read_json(&run_path)?;
            if run.status == RunStatus::Running
                && run.current_round.as_deref() == Some(round_id)
                && run.current_node.as_deref() == Some(node_id)
                && run.current_attempt.as_deref() == Some(attempt_id)
            {
                run.status = RunStatus::Paused;
                run.outcome = None;
                run.pause_reason = Some(reason);
                run.updated_at = now.clone();
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

        let node_path = self
            .paths
            .node_file(task_id, run_id, round_id, node_id, attempt_id);
        if node_path.exists() {
            let mut node: NodeState = read_json(&node_path)?;
            if node.status != RunStatus::Completed {
                node.status = RunStatus::Paused;
                node.outcome = None;
                node.finished_at = Some(now);
                validate_node_state(&node)?;
                write_json(&node_path, &node)?;
            }
        }

        Ok(())
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
        let now = now_rfc3339_like();
        let graph_path = self.paths.dynamic_graph_file(
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
        );
        if graph_path.exists() {
            let mut graph: DynamicGraphState = read_json(&graph_path)?;
            let mut target_updated = false;
            if let Some(dynamic_node) = graph
                .nodes
                .iter_mut()
                .find(|candidate| candidate.id == node_id)
            {
                if dynamic_node.status != DynamicNodeStatus::Completed {
                    dynamic_node.status = DynamicNodeStatus::Paused;
                    dynamic_node.outcome = None;
                    dynamic_node.finished_at = Some(now.clone());
                    target_updated = true;
                }
            }
            refresh_dynamic_current_leaf_ids(&mut graph);
            let has_active_leaf = dynamic_graph_has_active_leaf(&graph);
            if !has_active_leaf && graph.run.status == DynamicRunStatus::Running {
                graph.run.status = DynamicRunStatus::Paused;
                graph.run.outcome = None;
                graph.run.pause_reason = Some(reason);
            }
            graph.run.updated_at = now.clone();
            write_json(&graph_path, &graph)?;
            write_json(
                &self.paths.dynamic_run_file(
                    task_id,
                    run_id,
                    round_id,
                    outer_node_id,
                    outer_attempt_id,
                ),
                &graph.run,
            )?;
            if target_updated {
                let dynamic_node_path = self.paths.dynamic_node_file(
                    task_id,
                    run_id,
                    round_id,
                    outer_node_id,
                    outer_attempt_id,
                    node_id,
                );
                if let Some(dynamic_node) =
                    graph.nodes.iter().find(|candidate| candidate.id == node_id)
                {
                    write_json(&dynamic_node_path, dynamic_node)?;
                }
            }
            if has_active_leaf {
                return Ok(());
            }
        }

        let run_path = self.paths.run_file(task_id, run_id);
        if run_path.exists() {
            let mut run: RunState = read_json(&run_path)?;
            if run.status == RunStatus::Running
                && run.current_round.as_deref() == Some(round_id)
                && run.current_node.as_deref() == Some(outer_node_id)
                && run.current_attempt.as_deref() == Some(outer_attempt_id)
            {
                run.status = RunStatus::Paused;
                run.outcome = None;
                run.pause_reason = Some(reason);
                run.updated_at = now.clone();
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

        let outer_node_path =
            self.paths
                .node_file(task_id, run_id, round_id, outer_node_id, outer_attempt_id);
        if outer_node_path.exists() {
            let mut outer_node: NodeState = read_json(&outer_node_path)?;
            if outer_node.status != RunStatus::Completed {
                outer_node.status = RunStatus::Paused;
                outer_node.outcome = None;
                outer_node.finished_at = Some(now);
                validate_node_state(&outer_node)?;
                write_json(&outer_node_path, &outer_node)?;
            }
        }

        Ok(())
    }

    pub fn cancel_attempt_dir_best_effort(&self, attempt_dir: &Utf8Path) {
        let _ = cancel_pending_permission_requests(attempt_dir, now_rfc3339_like());
    }

    pub fn request_attempt_prompt_cancel_best_effort(&self, attempt_dir: &Utf8Path) {
        let _ = acp_client::request_prompt_cancel(attempt_dir);
        let _ = acp_client::cancel_attempt_prompt(attempt_dir);
    }

    pub fn kill_provider_pid_file_best_effort(&self, pid_path: &Utf8Path) {
        let Ok(pid_text) = fs::read_to_string(pid_path.as_std_path()) else {
            return;
        };
        let Ok(pid) = pid_text.trim().parse::<u32>() else {
            return;
        };
        let _ = kill_process_tree(pid);
        let _ = fs::remove_file(pid_path.as_std_path());
    }

    pub fn persist_cancelled_session_snapshot_best_effort(&self, attempt_dir: &Utf8Path) {
        let _ = self.persist_cancelled_session_file(&attempt_dir.join("acp.snapshot.json"));
        let _ = self.persist_cancelled_session_file(&attempt_dir.join("acp.session.json"));
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
                "status": "cancelled",
                "restored": false,
                "createdAt": crate::acp::events::current_timestamp(),
            })
        };
        let now = crate::acp::events::current_timestamp();
        session["status"] = serde_json::json!("cancelled");
        session["stopReason"] = serde_json::json!("cancelled");
        session["updatedAt"] = serde_json::json!(now.clone());
        if session.get("updated_at").is_some() {
            session["updated_at"] = serde_json::json!(now);
        }
        ensure_parent_dir(path)?;
        write_json(path, &session)
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

    pub fn acp_prompt_bundle_for_attempt(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        prompt: String,
        prompt_id: Option<String>,
        continue_ref: Option<serde_json::Value>,
    ) -> Result<PromptBundle> {
        let workflow = self::state_access::load_run_workflow(self, task_id, run_id)?;
        let validated = validate_workflow(workflow)?;
        self.validate_workflow_agents(&validated)?;
        let round: RoundState = read_json(&self.paths.round_file(task_id, run_id, round_id))?;
        let node: NodeState = read_json(
            &self
                .paths
                .node_file(task_id, run_id, round_id, node_id, attempt_id),
        )?;
        validate_round_state(&round)?;
        validate_node_state(&node)?;
        let invocation = self::node_executor::build_worker_invocation(
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
            PromptVisibility::Visible,
        )?;
        render_prompt_bundle(&invocation)
    }

    pub fn dynamic_acp_prompt_bundle_for_attempt(
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
    ) -> Result<PromptBundle> {
        build_dynamic_prompt_bundle(
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
        orchestrator_run_continue(self, task_id, run_id, prompt_id, prompt)
    }

    pub fn run_continue_background(
        &self,
        task_id: &str,
        run_id: &str,
        prompt_id: Option<String>,
        prompt: Option<String>,
    ) -> Result<RunState> {
        orchestrator_run_continue_background(self, task_id, run_id, prompt_id, prompt)
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
        prompt: String,
        attachment_paths: Vec<String>,
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
            prompt,
            attachment_paths,
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

    pub fn submit_manual_check_background(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        outcome: NodeOutcome,
    ) -> Result<RunState> {
        orchestrator_submit_manual_check_background(
            self, task_id, run_id, round_id, node_id, attempt_id, outcome,
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

    pub fn validate_workflow_agents(&self, workflow: &ValidatedWorkflow) -> Result<()> {
        for node in workflow.nodes_by_id.values() {
            for provider in providers_for_node(node) {
                let (agent_type, _) = self.managed_agent(&provider)?;
                if !agent_type.is_supported() {
                    bail!("agent `{provider}` is not supported yet");
                }
            }
        }
        // Validate model references are not blank (capability-level validation
        // happens at runtime via provider_capabilities when the node executes).
        for node in workflow.nodes_by_id.values() {
            for (provider, model) in models_for_node(node) {
                if let Some(model) = model {
                    if model.trim().is_empty() {
                        bail!(WorkflowValidationError::AgentModelBlank {
                            provider: provider.clone(),
                        });
                    }
                }
            }
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
                let workflow = read_json::<WorkflowDsl>(&workflow_path)?;
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

        let workflow: WorkflowDsl = match read_json(&path) {
            Ok(workflow) => workflow,
            Err(err) => return Ok((Some(err.to_string()), None)),
        };

        let validated = match validate_workflow(workflow.clone()) {
            Ok(validated) => validated,
            Err(err) => {
                let validation_error = err.downcast_ref::<WorkflowValidationError>().cloned();
                return Ok((Some(err.to_string()), validation_error));
            }
        };

        if let Err(err) = self.validate_workflow_agents(&validated) {
            return Ok((Some(err.to_string()), None));
        }

        match resolve_workflow_profiles(&self.paths, &validated.raw, self.config.desktop_language) {
            Ok(_) => Ok((None, None)),
            Err(err) => Ok((Some(err.to_string()), None)),
        }
    }

    pub fn find_active_or_resumable_run_id(&self, task_id: &str) -> Result<Option<String>> {
        let runs = self.run_list(task_id)?;
        if let Some(run) = runs.iter().rev().find(|run| {
            run.status == RunStatus::Running
                && self.paths.run_progress_file(task_id, &run.id).exists()
        }) {
            return Ok(Some(run.id.clone()));
        }
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
    use super::{AcpLiveEventContext, App, RuntimeLifecycleEvent};
    use crate::config::{
        ConsoleThemeName, DesktopLanguage, DesktopThemePreference, DesktopUpdateBadgeState,
    };
    use crate::domain::{
        NodeOutcome, NodeType, PauseReason, RoundTrigger, RunStatus, SessionMode, VERSION,
    };
    use crate::dynamic::{
        DynamicGraphState, DynamicNodeKind, DynamicNodeState, DynamicNodeStatus, DynamicRunState,
        DynamicRunStatus, WorkspaceMode, WorkspacePolicy,
    };
    use crate::observability::touch_log_file_best_effort;
    use crate::runtime::{NodeState, RoundState, RunState};
    use crate::storage::{read_json, write_json};
    use camino::Utf8PathBuf;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        static ENV_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        ENV_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap()
    }

    fn sample_run_paused_event() -> RuntimeLifecycleEvent {
        RuntimeLifecycleEvent::RunPaused {
            event_id: "run-1:round-1:node-1:attempt-1:waiting-for-user-input".to_string(),
            occurred_at: "2026-01-01T00:00:00".to_string(),
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

    #[test]
    fn lifecycle_subscriber_invoked_and_propagated_to_background() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let gold_band_home = repo_root.join("gold-band-home");
        unsafe { std::env::set_var("GOLD_BAND_HOME", gold_band_home.as_str()) };
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_callback = seen.clone();
        let app = App::new(repo_root).with_inline_lifecycle_subscriber(Arc::new(move |event| {
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
    fn lifecycle_bus_silent_when_no_subscribers() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let gold_band_home = repo_root.join("gold-band-home");
        unsafe { std::env::set_var("GOLD_BAND_HOME", gold_band_home.as_str()) };
        let app = App::new(repo_root);
        app.emit_lifecycle_event(sample_run_paused_event());
    }

    #[test]
    fn emits_acp_session_update_context() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let gold_band_home = repo_root.join("gold-band-home");
        unsafe { std::env::set_var("GOLD_BAND_HOME", gold_band_home.as_str()) };
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_callback = seen.clone();
        let app = App::new(repo_root).with_acp_session_update(Arc::new(move |context| {
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
        let gold_band_home = repo_root.join("gold-band-home");
        unsafe { std::env::set_var("GOLD_BAND_HOME", gold_band_home.as_str()) };
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_callback = seen.clone();
        let app = App::new(repo_root).with_acp_session_update(Arc::new(move |context| {
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

    fn dynamic_pause_test_app(temp: &tempfile::TempDir) -> App {
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        let gold_band_home = Utf8PathBuf::from_path_buf(temp.path().join("home")).unwrap();
        std::fs::create_dir_all(repo_root.as_std_path()).unwrap();
        std::fs::create_dir_all(gold_band_home.as_std_path()).unwrap();
        unsafe { std::env::set_var("GOLD_BAND_HOME", gold_band_home.as_str()) };
        App::new(repo_root)
    }

    fn dynamic_pause_node(id: &str, status: DynamicNodeStatus) -> DynamicNodeState {
        DynamicNodeState {
            version: VERSION.to_string(),
            id: id.to_string(),
            dynamic_run_id: "dynamic-run-001".to_string(),
            kind: DynamicNodeKind::Worker,
            title: id.to_string(),
            task: id.to_string(),
            status,
            outcome: None,
            group_id: None,
            chain_id: id.to_string(),
            depth: 1,
            depends_on: Vec::new(),
            workspace: WorkspacePolicy {
                mode: WorkspaceMode::Worktree,
            },
            workspace_path: None,
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
        }
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
                resolved_config: Default::default(),
                uuid: None,
            },
        )
        .unwrap();
        let graph = DynamicGraphState {
            version: VERSION.to_string(),
            run: DynamicRunState {
                version: VERSION.to_string(),
                id: "dynamic-run-001".to_string(),
                parent_run_id: run_id.to_string(),
                parent_round_id: round_id.to_string(),
                parent_node_id: outer_node_id.to_string(),
                parent_attempt_id: outer_attempt_id.to_string(),
                status: DynamicRunStatus::Running,
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
                        "status": "completed"
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
                session.get("status").and_then(|value| value.as_str()),
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
                .get("status")
                .and_then(|value| value.as_str()),
            Some("cancelled")
        );
        assert_eq!(
            completed_session
                .get("status")
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
    }

    #[test]
    fn runtime_log_tail_reads_only_last_requested_lines() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let gold_band_home = repo_root.join("gold-band-home");
        unsafe { std::env::set_var("GOLD_BAND_HOME", gold_band_home.as_str()) };
        let app = App::new(repo_root);
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
        let gold_band_home = repo_root.join("gold-band-home");
        std::fs::create_dir_all(gold_band_home.as_std_path()).unwrap();
        unsafe { std::env::set_var("GOLD_BAND_HOME", gold_band_home.as_str()) };

        let app = App::new(repo_root);
        touch_log_file_best_effort(&app.paths);

        assert!(app.paths.runtime_log_file().as_std_path().exists());
    }

    #[test]
    fn user_console_theme_is_persisted_to_settings() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let gold_band_home = repo_root.join("gold-band-home");
        std::fs::create_dir_all(gold_band_home.as_std_path()).unwrap();
        unsafe { std::env::set_var("GOLD_BAND_HOME", gold_band_home.as_str()) };

        let app = App::new(repo_root.clone());
        app.set_user_console_theme(ConsoleThemeName::Nord).unwrap();

        let settings = app.load_settings().unwrap();
        assert_eq!(settings.console_theme, Some(ConsoleThemeName::Nord));
    }

    #[test]
    fn desktop_preferences_persisted_to_settings() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let gold_band_home = repo_root.join("gold-band-home");
        std::fs::create_dir_all(gold_band_home.as_std_path()).unwrap();
        unsafe { std::env::set_var("GOLD_BAND_HOME", gold_band_home.as_str()) };

        let app = App::new(repo_root.clone());
        app.set_user_desktop_preferences(
            DesktopThemePreference::Dark,
            DesktopLanguage::En,
            "Fira Code".to_string(),
        )
        .unwrap();
        app.set_user_use_local_claude(true).unwrap();

        let settings = app.load_settings().unwrap();
        assert_eq!(settings.desktop_theme, Some(DesktopThemePreference::Dark));
        assert_eq!(settings.desktop_language, Some(DesktopLanguage::En));
        assert_eq!(settings.desktop_font, Some("Fira Code".to_string()));
        assert_eq!(settings.use_local_claude, Some(true));

        let state = app.load_state().unwrap();
        assert!(state.desktop_updater_last_checked_at.is_none());
        assert!(state.desktop_available_update.is_none());
    }

    #[test]
    fn updater_state_persisted_to_state_json() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let gold_band_home = repo_root.join("gold-band-home");
        std::fs::create_dir_all(gold_band_home.as_std_path()).unwrap();
        unsafe { std::env::set_var("GOLD_BAND_HOME", gold_band_home.as_str()) };

        let app = App::new(repo_root.clone());
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
    fn workspace_persists_to_both_files() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let gold_band_home = repo_root.join("gold-band-home");
        std::fs::create_dir_all(gold_band_home.as_std_path()).unwrap();
        unsafe { std::env::set_var("GOLD_BAND_HOME", gold_band_home.as_str()) };

        let app = App::new(repo_root.clone());
        app.set_user_desktop_workspace("D:/Projects/MyRepo")
            .unwrap();

        let settings = app.load_settings().unwrap();
        assert_eq!(
            settings.desktop_workspace.as_deref(),
            Some("D:/Projects/MyRepo")
        );

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
        let gold_band_home = repo_root.join("gold-band-home");
        std::fs::create_dir_all(gold_band_home.as_std_path()).unwrap();
        unsafe { std::env::set_var("GOLD_BAND_HOME", gold_band_home.as_str()) };

        let app = App::new(repo_root.clone());
        app.set_user_desktop_workspace("D:/Projects/A").unwrap();
        app.set_user_desktop_workspace("D:/Projects/B").unwrap();
        app.set_user_desktop_workspace("D:/Projects/A").unwrap();

        let state = app.load_state().unwrap();
        // A should be at position 0 (most recent), B at position 1, no duplicates
        assert_eq!(state.recent_desktop_workspaces.len(), 2);
        assert_eq!(state.recent_desktop_workspaces[0], "D:/Projects/A");
        assert_eq!(state.recent_desktop_workspaces[1], "D:/Projects/B");
    }

    #[test]
    fn recent_workspaces_truncated_at_eight() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let gold_band_home = repo_root.join("gold-band-home");
        std::fs::create_dir_all(gold_band_home.as_std_path()).unwrap();
        unsafe { std::env::set_var("GOLD_BAND_HOME", gold_band_home.as_str()) };

        let app = App::new(repo_root.clone());
        for i in 0..10 {
            app.set_user_desktop_workspace(&format!("D:/Projects/Repo{i}"))
                .unwrap();
        }

        let state = app.load_state().unwrap();
        assert_eq!(state.recent_desktop_workspaces.len(), 8);
        assert_eq!(state.recent_desktop_workspaces[0], "D:/Projects/Repo9");
    }
}
