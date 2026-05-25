mod ids;
mod node_executor;
mod orchestrator;
mod profile_resolver;
mod profiles;
mod state_access;
mod state_factory;
mod transition_context;

use crate::acp::client as acp_client;
use crate::acp::permission::{cancel_pending_permission_requests, request_cancel};
use crate::config::{
    ConsoleThemeName, DesktopFontPreference, DesktopLanguage, DesktopThemePreference,
    ManagedAgentConfig, ManagedAgentType, RuntimeConfig, UserConfig,
};
use crate::control::{ControlDecision, decide_next_step};
use crate::domain::{NodeOutcome, RunOutcome};
use crate::domain::{PauseReason, RunStatus, SessionMode, VERSION};
use crate::dsl::{
    END_NODE, EdgeDsl, EdgeOutcome, JsonConditionDsl, NEW_ROUND_NODE, NodeDsl, OutputContractDsl,
    OutputKind, ValidatedWorkflow, WorkerNode, WorkflowControl, WorkflowDsl,
    WorkflowValidationError, validate_workflow,
};
use crate::process::kill_process_tree;
use crate::provider::{
    DoctorResult, PromptBundle, PromptVisibility, ProviderAdapter, ProviderCapabilities,
    ProviderInfo, provider_capabilities, provider_from_agent, render_prompt_bundle,
};
use crate::runtime::{
    NodeState, RoundState, RunState, TaskState, WorkerRefState, validate_node_state,
    validate_round_state, validate_run_state, validate_task_state, validate_worker_ref_state,
};
use crate::storage::{GoldBandPaths, read_json, write_json};
use anyhow::{Context, Result, anyhow, bail};
use camino::{Utf8Path, Utf8PathBuf};
use serde::de::DeserializeOwned;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::str::FromStr;
use std::sync::Arc;

use self::ids::{next_task_id, now_rfc3339_like};
use self::orchestrator::{
    run_continue as orchestrator_run_continue,
    run_continue_background as orchestrator_run_continue_background,
    run_retry as orchestrator_run_retry, run_start as orchestrator_run_start,
    run_start_background as orchestrator_run_start_background,
    submit_manual_check as orchestrator_submit_manual_check,
    submit_manual_check_background as orchestrator_submit_manual_check_background,
};
use self::profile_resolver::resolve_workflow_profiles;
use self::profiles::{
    DefaultProfileIds, create_profile, ensure_default_user_profiles, list_profiles, show_profile,
    update_profile,
};
pub use self::profiles::{ProfileEntry, ProfileInput, ProfileList, ProfileScope};

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
    ) -> NodeDsl {
        let artifact = validation.then(|| format!("{id}-result"));
        NodeDsl::Worker(WorkerNode {
            id: id.to_string(),
            provider: Some(provider.to_string()),
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
            manual_check: None,
        })
    }

    WorkflowDsl {
        version: "0.1".to_string(),
        id: "task-workflow".to_string(),
        entry: "plan".to_string(),
        control: WorkflowControl::default(),
        nodes: vec![
            worker(
                provider,
                profiles,
                "plan",
                "plan",
                "Analyze the imported requirement and produce an implementation plan.",
                false,
            ),
            worker(
                provider,
                profiles,
                "dev",
                "dev",
                "Implement the requirement in the workspace.",
                false,
            ),
            worker(
                provider,
                profiles,
                "review",
                "review",
                "Review the implementation and return JSON with result and reason fields.",
                true,
            ),
            worker(
                provider,
                profiles,
                "test",
                "test",
                "Run or describe verification and return JSON with result and reason fields.",
                true,
            ),
            worker(
                provider,
                profiles,
                "accept",
                "accept",
                "Validate acceptance and return JSON with result and reason fields.",
                true,
            ),
            worker(
                provider,
                profiles,
                "cleanup",
                "cleanup",
                "Clean up resources, finalize handoff notes, and close the task after acceptance succeeds.",
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

#[derive(Debug, Clone)]
pub struct CreateTaskInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub requirement_file_name: String,
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

pub struct App {
    pub paths: GoldBandPaths,
    pub config: RuntimeConfig,
    provider_override: Option<Arc<dyn ProviderAdapter>>,
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

impl App {
    pub fn new(repo_root: Utf8PathBuf) -> Self {
        Self::with_config(repo_root, RuntimeConfig::default())
    }

    pub fn load_user_config(&self) -> Result<UserConfig> {
        let path = self.paths.user_config_file();
        if !path.exists() {
            return Ok(UserConfig::default());
        }
        read_json(&path)
    }

    pub fn save_user_config(&self, config: &UserConfig) -> Result<()> {
        write_json(&self.paths.user_config_file(), config)
    }

    pub fn set_user_console_theme(&self, theme: ConsoleThemeName) -> Result<UserConfig> {
        let mut config = self.load_user_config()?;
        config.console_theme = Some(theme);
        self.save_user_config(&config)?;
        Ok(config)
    }

    pub fn set_user_desktop_theme(&self, theme: DesktopThemePreference) -> Result<UserConfig> {
        let mut config = self.load_user_config()?;
        config.desktop_theme = Some(theme);
        self.save_user_config(&config)?;
        Ok(config)
    }

    pub fn set_user_desktop_language(&self, language: DesktopLanguage) -> Result<UserConfig> {
        let mut config = self.load_user_config()?;
        config.desktop_language = Some(language);
        self.save_user_config(&config)?;
        Ok(config)
    }

    pub fn set_user_desktop_preferences(
        &self,
        theme: DesktopThemePreference,
        language: DesktopLanguage,
        font: DesktopFontPreference,
    ) -> Result<UserConfig> {
        let mut config = self.load_user_config()?;
        config.desktop_theme = Some(theme);
        config.desktop_language = Some(language);
        config.desktop_font = Some(font);
        self.save_user_config(&config)?;
        Ok(config)
    }

    pub fn set_user_desktop_updater_url_override(
        &self,
        override_url: Option<String>,
    ) -> Result<UserConfig> {
        let mut config = self.load_user_config()?;
        config.desktop_updater_url_override = override_url;
        self.save_user_config(&config)?;
        Ok(config)
    }

    pub fn set_user_desktop_updater_last_checked_at(
        &self,
        checked_at: Option<String>,
    ) -> Result<UserConfig> {
        let mut config = self.load_user_config()?;
        config.desktop_updater_last_checked_at = checked_at;
        self.save_user_config(&config)?;
        Ok(config)
    }

    pub fn set_user_desktop_workspace(&self, workspace: &str) -> Result<UserConfig> {
        let mut config = self.load_user_config()?;
        config.desktop_workspace = Some(workspace.to_string());
        config
            .recent_desktop_workspaces
            .retain(|item| item != workspace);
        config
            .recent_desktop_workspaces
            .insert(0, workspace.to_string());
        config.recent_desktop_workspaces.truncate(8);
        self.save_user_config(&config)?;
        Ok(config)
    }

    pub fn set_user_agents(
        &self,
        agents: std::collections::BTreeMap<ManagedAgentType, ManagedAgentConfig>,
    ) -> Result<UserConfig> {
        let mut config = self.load_user_config()?;
        config.agents = Some(agents);
        self.save_user_config(&config)?;
        Ok(config)
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
    ) -> Result<UserConfig> {
        let mut agents = self.config.agents.clone();
        if !agent_type.is_supported() {
            bail!("agent `{}` is not supported yet", agent_type.as_str());
        }
        agents.insert(agent_type, config);
        self.set_user_agents(agents)
    }

    pub fn remove_managed_agent(&self, agent_type: ManagedAgentType) -> Result<UserConfig> {
        let mut agents = self.config.agents.clone();
        agents.remove(&agent_type);
        self.set_user_agents(agents)
    }

    pub fn workflow_templates(&self) -> Result<WorkflowTemplateStore> {
        self.load_workflow_template_store()
    }

    pub fn profiles(&self) -> Result<ProfileList> {
        list_profiles(&self.paths)
    }

    pub fn profile_show(&self, id: &str) -> Result<ProfileEntry> {
        show_profile(&self.paths, id)
    }

    pub fn create_profile(&self, input: ProfileInput) -> Result<ProfileEntry> {
        create_profile(&self.paths, input)
    }

    pub fn update_profile(&self, id: &str, input: ProfileInput) -> Result<ProfileEntry> {
        update_profile(&self.paths, id, input)
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
        let validated = validate_workflow(workflow)?;
        self.validate_workflow_agents(&validated)?;
        resolve_workflow_profiles(&self.paths, &validated.raw)?;

        let mut store = self.load_workflow_template_store()?;
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
        let validated = validate_workflow(workflow)?;
        self.validate_workflow_agents(&validated)?;
        resolve_workflow_profiles(&self.paths, &validated.raw)?;

        let mut store = self.load_workflow_template_store()?;
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

    fn load_workflow_template_store(&self) -> Result<WorkflowTemplateStore> {
        let default_profiles = ensure_default_user_profiles(&self.paths)?;
        let default_template = default_workflow_template(&default_profiles);
        let path = self.paths.workflow_templates_file();
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
        fs::create_dir_all(self.paths.authoring_dir().as_std_path())?;
        write_json(&self.paths.workflow_templates_file(), store)
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
        Ok(Arc::from(provider_from_agent(agent_type, config)?))
    }

    pub fn provider_info(&self, provider: &str) -> Result<ProviderInfo> {
        Ok(self.provider_for_id(provider)?.describe_provider())
    }

    pub fn provider_doctor(&self, provider: &str) -> Result<DoctorResult> {
        let (agent_type, config) = self.managed_agent(provider)?;
        if !agent_type.is_supported() {
            bail!("agent `{provider}` is not supported yet");
        }
        match acp_client::doctor(&config.adapter, self.paths.repo_root.clone()) {
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
        let extension = std::path::Path::new(&input.requirement_file_name)
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| anyhow!("requirement file must have .txt or .md extension"))?;
        if !matches!(extension.as_str(), "txt" | "md") {
            bail!("requirement file must be .txt or .md");
        }
        if input.requirement_content.trim().is_empty() {
            bail!("requirement content cannot be empty");
        }

        let validated = validate_workflow(input.workflow.clone())?;
        self.validate_workflow_agents(&validated)?;
        resolve_workflow_profiles(&self.paths, &validated.raw)?;

        let task_id = next_task_id(&self.paths.tasks_dir())?;
        let task = TaskState {
            version: VERSION.to_string(),
            id: task_id.clone(),
            title: input.title.filter(|value| !value.trim().is_empty()),
            description: input.description.filter(|value| !value.trim().is_empty()),
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
        resolve_workflow_profiles(&self.paths, &validated.raw)?;
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
        self.request_current_provider_cancel_best_effort(task_id, run_id, &run);
        self.kill_current_provider_process_best_effort(task_id, run_id, &run);
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
                }
            }
        }

        Ok(run)
    }

    pub fn stop_all_running_sessions(&self) -> Result<Vec<RunState>> {
        let mut stopped = Vec::new();
        for task in self.task_list()? {
            let Ok(runs) = self.run_list(&task.id) else {
                continue;
            };
            for run in runs {
                if run.status != RunStatus::Running {
                    continue;
                }
                if let Ok(killed) = self.run_kill(&task.id, &run.id) {
                    stopped.push(killed);
                }
            }
        }
        Ok(stopped)
    }

    fn request_current_provider_cancel_best_effort(
        &self,
        task_id: &str,
        run_id: &str,
        run: &RunState,
    ) {
        let (Some(round_id), Some(node_id), Some(attempt_id)) =
            (&run.current_round, &run.current_node, &run.current_attempt)
        else {
            return;
        };
        let attempt_dir = self
            .paths
            .attempt_dir(task_id, run_id, round_id, node_id, attempt_id);
        let _ = request_cancel(&attempt_dir, now_rfc3339_like());
        let _ = cancel_pending_permission_requests(&attempt_dir, now_rfc3339_like());
    }

    fn kill_current_provider_process_best_effort(
        &self,
        task_id: &str,
        run_id: &str,
        run: &RunState,
    ) {
        let (Some(round_id), Some(node_id), Some(attempt_id)) =
            (&run.current_round, &run.current_node, &run.current_attempt)
        else {
            return;
        };
        let pid_path = self
            .paths
            .provider_pid_file(task_id, run_id, round_id, node_id, attempt_id);
        let Ok(pid_text) = fs::read_to_string(pid_path.as_std_path()) else {
            return;
        };
        let Ok(pid) = pid_text.trim().parse::<u32>() else {
            return;
        };
        let _ = kill_process_tree(pid);
        let _ = fs::remove_file(pid_path.as_std_path());
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

    pub fn run_continue(
        &self,
        task_id: &str,
        run_id: &str,
        prompt_id: Option<String>,
    ) -> Result<RunState> {
        orchestrator_run_continue(self, task_id, run_id, prompt_id)
    }

    pub fn run_continue_background(
        &self,
        task_id: &str,
        run_id: &str,
        prompt_id: Option<String>,
    ) -> Result<RunState> {
        orchestrator_run_continue_background(self, task_id, run_id, prompt_id)
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
            if let Some(provider) = node.provider() {
                let (agent_type, _) = self.managed_agent(provider)?;
                if !agent_type.is_supported() {
                    bail!("agent `{provider}` is not supported yet");
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

        match resolve_workflow_profiles(&self.paths, &validated.raw) {
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
    use super::App;
    use crate::config::ConsoleThemeName;
    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        static ENV_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        ENV_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap()
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
    fn user_console_theme_is_persisted() {
        let _guard = env_guard();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let gold_band_home = repo_root.join("gold-band-home");
        std::fs::create_dir_all(gold_band_home.as_std_path()).unwrap();
        unsafe { std::env::set_var("GOLD_BAND_HOME", gold_band_home.as_str()) };

        let app = App::new(repo_root.clone());
        app.set_user_console_theme(ConsoleThemeName::Nord).unwrap();

        let config = app.load_user_config().unwrap();
        assert_eq!(config.console_theme, Some(ConsoleThemeName::Nord));
    }
}
