//! Conversation task creation authority.
//!
//! Turning a conversation request into a persisted task (workflow, authoring
//! metadata, AUTO config, prompt input, attachments) and a prepared run is one
//! domain operation. Desktop, scheduler, multica and the headless CLI all map
//! their own input DTOs onto [`CreateConversationTaskCommand`] instead of
//! re-implementing the persistence steps.

use std::fs;
use std::path::Path;

use anyhow::{Result, anyhow, ensure};
use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

use super::orchestrator::run_prepared_run_foreground;
use super::{App, CreateTaskInput, DEFAULT_WORKFLOW_TEMPLATE_ID, apply_optional_entry_preference};
use crate::config::{ConversationAutoConfig, ConversationDirectConfig, ConversationRunMode};
use crate::dsl::{WorkflowDsl, workflow_contains_ai_dynamic};
use crate::git::{GitRepositoryService, GitSourceControlService};
use crate::provider::{
    PromptWorkspaceFileRef, TaskPromptInput, UserPromptQuote, UserPromptRole,
    conversation_prompt_has_payload,
};
use crate::runtime::RunState;
use crate::storage::{read_json, write_json};
use crate::workflow_model_binding::{
    TaskAuthoringWorkflow, WorkflowModelBindings, migrate_authoring_workflow,
};

/// Persisted schema version of `authoring/conversation.json`.
pub const CONVERSATION_METADATA_VERSION: &str = "3";
/// Origin recorded for conversations created from the desktop UI.
pub const CONVERSATION_SOURCE_DESKTOP: &str = "conversation-ui";
/// Origin recorded for conversations created by the headless CLI.
pub const CONVERSATION_SOURCE_CLI: &str = "cli";
const CONVERSATION_DEFAULT_TITLE: &str = "New Task";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConversationWorkLocation {
    #[default]
    Main,
    Worktree,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationAgentIdentity {
    pub agent_type: String,
    pub display_name: String,
    pub icon_key: String,
}

/// Canonical conversation authoring metadata (`authoring/conversation.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMetadata {
    /// Direct background retention preference; independent of sidebar ordering.
    #[serde(default)]
    pub resident: bool,
    pub version: String,
    pub source: String,
    pub run_mode: String,
    pub workflow_template_id: Option<String>,
    pub include_optional_entry: Option<bool>,
    pub direct_config: Option<ConversationDirectConfig>,
    pub agent_identity: Option<ConversationAgentIdentity>,
    pub title_auto_generated: bool,
    pub initial_attachment_names: Option<Vec<String>>,
    #[serde(default)]
    pub initial_workspace_files: Vec<PromptWorkspaceFileRef>,
    pub created_at: String,
    pub last_activity_at: Option<String>,
    #[serde(default)]
    pub work_location: ConversationWorkLocation,
    #[serde(default)]
    pub scheduled_task_id: Option<String>,
    #[serde(default)]
    pub scheduled_content_fingerprint: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("{code}: {msg}")]
pub struct ConversationResidencyError {
    pub code: &'static str,
    pub msg: &'static str,
}

impl App {
    pub fn set_conversation_resident(&self, task_id: &str, resident: bool) -> Result<()> {
        self.task_show(task_id)?;
        let path = self
            .paths
            .task_dir(task_id)
            .join("authoring/conversation.json");
        crate::storage::with_file_lock(&path, || {
            let mut metadata: ConversationMetadata = read_json(&path)?;
            if metadata.run_mode != "direct" {
                return Err(ConversationResidencyError {
                    code: "conversation.residency-direct-only",
                    msg: "residency requires a Direct conversation",
                }
                .into());
            }
            metadata.resident = resident;
            write_json(&path, &metadata)?;
            crate::acp::client::set_direct_session_resident(
                &self.paths.project_id,
                task_id,
                resident,
            );
            Ok(())
        })
    }
}

/// Write command for creating a conversation task (and optionally its run).
#[derive(Debug, Clone)]
pub struct CreateConversationTaskCommand {
    pub source: &'static str,
    pub content: String,
    pub run_mode: ConversationRunMode,
    pub workflow_template_id: Option<String>,
    pub include_optional_entry: Option<bool>,
    pub direct_config: Option<ConversationDirectConfig>,
    pub auto_config: Option<ConversationAutoConfig>,
    pub workflow_authoring: Option<TaskAuthoringWorkflow>,
    pub attachment_paths: Vec<String>,
    pub work_location: ConversationWorkLocation,
    pub selected_branch: Option<String>,
    pub scheduled_task_id: Option<String>,
    pub scheduled_content_fingerprint: Option<String>,
    pub role: Option<UserPromptRole>,
    pub workspace_files: Vec<PromptWorkspaceFileRef>,
    pub quotes: Vec<UserPromptQuote>,
}

impl CreateConversationTaskCommand {
    pub fn new(source: &'static str, content: String, run_mode: ConversationRunMode) -> Self {
        Self {
            source,
            content,
            run_mode,
            workflow_template_id: None,
            include_optional_entry: None,
            direct_config: None,
            auto_config: None,
            workflow_authoring: None,
            attachment_paths: Vec::new(),
            work_location: ConversationWorkLocation::Main,
            selected_branch: None,
            scheduled_task_id: None,
            scheduled_content_fingerprint: None,
            role: None,
            workspace_files: Vec::new(),
            quotes: Vec::new(),
        }
    }
}

/// How a created conversation run is driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationRunLaunch {
    /// Return immediately; the run continues on a background thread.
    Background,
    /// Drive on the calling thread until the run completes or pauses.
    Foreground,
}

#[derive(Debug, Clone)]
pub struct CreatedConversationRun {
    pub task_id: String,
    pub task_uuid: Option<String>,
    pub title: String,
    pub run: RunState,
}

/// A created task that is rolled back (task directory removed) unless the
/// caller accepts it after the dependent run was successfully prepared.
pub struct PreparedConversationTask {
    task_id: String,
    task_uuid: Option<String>,
    title: String,
    task_dir: Utf8PathBuf,
    armed: bool,
}

impl PreparedConversationTask {
    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn task_uuid(&self) -> Option<&str> {
        self.task_uuid.as_deref()
    }

    pub fn accept(mut self) -> (String, Option<String>, String) {
        self.armed = false;
        (
            std::mem::take(&mut self.task_id),
            self.task_uuid.take(),
            std::mem::take(&mut self.title),
        )
    }
}

impl Drop for PreparedConversationTask {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_dir_all(self.task_dir.as_std_path());
        }
    }
}

pub fn conversation_auto_title(content: &str, max_chars: usize) -> String {
    if content.is_empty() {
        CONVERSATION_DEFAULT_TITLE.to_string()
    } else {
        content
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(max_chars.max(1))
            .collect()
    }
}

impl App {
    pub fn conversation_agent_identity(
        &self,
        agent_type: &str,
    ) -> Option<ConversationAgentIdentity> {
        let (_, config) = self.managed_agent(agent_type).ok()?;
        Some(ConversationAgentIdentity {
            agent_type: agent_type.to_string(),
            display_name: config.adapter.display_name.clone(),
            icon_key: config.icon.clone(),
        })
    }

    /// Creates the task and all conversation authoring state. The returned
    /// guard removes the task again unless the caller accepts it.
    pub fn prepare_conversation_task(
        &self,
        command: &CreateConversationTaskCommand,
    ) -> Result<PreparedConversationTask> {
        ensure!(
            conversation_prompt_has_payload(
                &command.content,
                command.attachment_paths.len(),
                command.role.as_ref(),
                command.workspace_files.len(),
            ),
            "conversation payload cannot be empty"
        );
        let title = conversation_auto_title(
            &command.content,
            self.config.conversation_auto_title_max_chars,
        );

        let (mut workflow, mut model_bindings, effective_include_optional_entry) =
            self.conversation_workflow(command)?;
        migrate_authoring_workflow(&mut workflow, &mut model_bindings, None)?;

        // Git is an authoritative prerequisite for Auto and every workflow that
        // directly contains AI-DYNAMIC. Check before creating either the task or run.
        if workflow_contains_ai_dynamic(&workflow)
            || command.work_location == ConversationWorkLocation::Worktree
        {
            GitRepositoryService::default().require_worktree(&self.paths.repo_root)?;
        }

        let summary = self.create_conversation_task_from_payload_with_bindings(
            CreateTaskInput {
                title: Some(title.clone()),
                description: None,
                requirement_file_name: None,
                requirement_content: command.content.clone(),
                workflow: workflow.clone(),
                workflow_template_id: command.workflow_template_id.clone(),
            },
            workflow,
            model_bindings,
        )?;
        let task_id = summary.task.id.clone();
        let prepared = PreparedConversationTask {
            task_id: task_id.clone(),
            task_uuid: summary.task.uuid.clone().or_else(|| Some(task_id.clone())),
            title,
            task_dir: self.paths.task_dir(&task_id),
            armed: true,
        };

        let authoring_dir = self.paths.task_dir(&task_id).join("authoring");
        fs::create_dir_all(authoring_dir.as_std_path())?;
        let created_at = chrono::Utc::now().to_rfc3339();
        let metadata = ConversationMetadata {
            resident: false,
            version: CONVERSATION_METADATA_VERSION.to_string(),
            source: command.source.to_string(),
            run_mode: command.run_mode.as_str().to_string(),
            workflow_template_id: command.workflow_template_id.clone(),
            include_optional_entry: effective_include_optional_entry,
            direct_config: command.direct_config.clone(),
            agent_identity: command
                .direct_config
                .as_ref()
                .and_then(|config| self.conversation_agent_identity(&config.agent_type)),
            title_auto_generated: true,
            initial_attachment_names: Some(
                command
                    .attachment_paths
                    .iter()
                    .map(|path| {
                        Path::new(path)
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("unknown")
                            .to_string()
                    })
                    .collect(),
            ),
            initial_workspace_files: command.workspace_files.clone(),
            created_at: created_at.clone(),
            last_activity_at: Some(created_at),
            work_location: command.work_location,
            scheduled_task_id: command.scheduled_task_id.clone(),
            scheduled_content_fingerprint: command.scheduled_content_fingerprint.clone(),
        };
        write_json(&authoring_dir.join("conversation.json"), &metadata)?;
        if command.run_mode == ConversationRunMode::Auto
            && let Some(config) = command.auto_config.as_ref()
        {
            let mut config = config.clone();
            config.active_template_id = None;
            config.active_template_name = None;
            write_json(&self.paths.task_auto_config_file(&task_id), &config)?;
        }
        let task_prompt_input = TaskPromptInput {
            quotes: command.quotes.clone(),
            role: command.role.clone(),
            workspace_files: command.workspace_files.clone(),
        };
        if !task_prompt_input.is_empty() {
            write_json(
                &self.paths.task_prompt_input_file(&task_id),
                &task_prompt_input,
            )?;
        }
        if !command.attachment_paths.is_empty() {
            let attach_dir = authoring_dir.join("inputs");
            fs::create_dir_all(attach_dir.as_std_path())?;
            for source in &command.attachment_paths {
                let source = Path::new(source);
                if let Some(name) = source.file_name().and_then(|name| name.to_str()) {
                    fs::copy(source, attach_dir.join(name))?;
                }
            }
        }
        self.record_task_activity_index(&task_id, &metadata.created_at);
        Ok(prepared)
    }

    /// Creates a conversation task, prepares its run in the requested work
    /// location and launches it. The task is rolled back if the run cannot be
    /// prepared.
    pub fn create_conversation_run(
        &self,
        command: &CreateConversationTaskCommand,
        launch: ConversationRunLaunch,
    ) -> Result<CreatedConversationRun> {
        let fork_point = if command.work_location == ConversationWorkLocation::Worktree {
            Some(
                GitSourceControlService::default().resolve_branch_fork_point(
                    &self.paths.repo_root,
                    command.selected_branch.as_deref(),
                )?,
            )
        } else {
            None
        };
        let prepared_task = self.prepare_conversation_task(command)?;
        let task_id = prepared_task.task_id().to_string();
        let auto_config = match command.run_mode {
            ConversationRunMode::Auto => command.auto_config.clone(),
            _ => None,
        };
        let prepared_run = match (auto_config, fork_point) {
            (Some(config), Some(fork_point)) => {
                self.prepare_auto_run_in_worktree_at(&task_id, config, fork_point.head_oid)?
            }
            (Some(config), None) => self.prepare_auto_run(&task_id, config)?,
            (None, Some(fork_point)) => {
                self.prepare_run_in_worktree_at(&task_id, None, fork_point.head_oid)?
            }
            (None, None) => self.prepare_run(&task_id, None)?,
        };
        let accepted_run = prepared_run.accept();
        let (run, (task_id, task_uuid, title)) = match launch {
            ConversationRunLaunch::Background => {
                let run = self.launch_prepared_run_background(&task_id, accepted_run)?;
                (run, prepared_task.accept())
            }
            ConversationRunLaunch::Foreground => {
                // The run is durable from here on; a long foreground drive
                // must never roll back the task it is executing.
                let accepted_task = prepared_task.accept();
                let run = run_prepared_run_foreground(self, &task_id, accepted_run)?;
                (run, accepted_task)
            }
        };
        Ok(CreatedConversationRun {
            task_id,
            task_uuid,
            title,
            run,
        })
    }

    fn conversation_workflow(
        &self,
        command: &CreateConversationTaskCommand,
    ) -> Result<(WorkflowDsl, WorkflowModelBindings, Option<bool>)> {
        Ok(match command.run_mode {
            ConversationRunMode::Direct => {
                let config = command
                    .direct_config
                    .as_ref()
                    .ok_or_else(|| anyhow!("direct config is required"))?;
                (
                    crate::dsl::presets::direct_workflow(
                        config.agent_type.clone(),
                        config.model_id.clone(),
                        config.permission_mode.clone(),
                        config.auto_accept,
                        config.config_options.clone(),
                    ),
                    WorkflowModelBindings::default(),
                    None,
                )
            }
            ConversationRunMode::Auto => (
                crate::execution_plan::compile_auto_workflow(command.auto_config.as_ref()),
                WorkflowModelBindings::default(),
                None,
            ),
            ConversationRunMode::Workflow => {
                if let Some(authoring) = command.workflow_authoring.as_ref() {
                    (
                        authoring.workflow.clone(),
                        authoring.model_bindings.clone(),
                        command.include_optional_entry,
                    )
                } else {
                    let store = self.workflow_templates()?;
                    let template_id = command
                        .workflow_template_id
                        .as_deref()
                        .unwrap_or(DEFAULT_WORKFLOW_TEMPLATE_ID);
                    let template = store
                        .templates
                        .iter()
                        .find(|template| template.id == template_id)
                        .ok_or_else(|| anyhow!("workflow template not found: {template_id}"))?;
                    let mut workflow = template.workflow.clone();
                    let include_optional_entry = apply_optional_entry_preference(
                        template,
                        command.include_optional_entry,
                        &mut workflow,
                    )?;
                    (
                        workflow,
                        template.model_bindings.clone(),
                        include_optional_entry,
                    )
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;
    use serde_json::json;
    use tempfile::tempdir;

    use super::{
        CONVERSATION_SOURCE_CLI, ConversationMetadata, ConversationRunLaunch,
        CreateConversationTaskCommand,
    };
    use crate::app::App;
    use crate::config::{ConversationAutoConfig, ConversationRunMode, RuntimeConfig};
    use crate::process::background_command;
    use crate::storage::read_json;

    fn git(repo: &Utf8PathBuf, args: &[&str]) {
        let status = background_command("git")
            .args(args)
            .current_dir(repo.as_std_path())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn git_repo() -> (tempfile::TempDir, Utf8PathBuf) {
        let temp = tempdir().unwrap();
        let repo = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        std::fs::create_dir_all(repo.as_std_path()).unwrap();
        git(&repo, &["init"]);
        std::fs::write(repo.join(".git/info/exclude"), "gold-band-home/\n").unwrap();
        git(&repo, &["config", "user.email", "test@example.com"]);
        git(&repo, &["config", "user.name", "Test User"]);
        std::fs::write(repo.join("README.md"), "hello\n").unwrap();
        git(&repo, &["add", "README.md"]);
        git(&repo, &["commit", "-m", "init"]);
        (temp, repo)
    }

    fn auto_command() -> CreateConversationTaskCommand {
        let auto_config: ConversationAutoConfig = serde_json::from_value(json!({
            "agentType": "claude-acp",
            "configOptions": { "effort": "max" },
            "allowedProfiles": ["dev"],
            "activeTemplateId": "template-1",
            "activeTemplateName": "Template"
        }))
        .unwrap();
        let mut command = CreateConversationTaskCommand::new(
            CONVERSATION_SOURCE_CLI,
            "Implement the feature\nwith details".to_string(),
            ConversationRunMode::Auto,
        );
        command.auto_config = Some(auto_config);
        command
    }

    #[test]
    fn auto_conversation_task_persists_metadata_and_auto_config() {
        let (_temp, repo) = git_repo();
        let app = App::with_config(repo, RuntimeConfig::default());

        let prepared = app.prepare_conversation_task(&auto_command()).unwrap();
        let (task_id, _, title) = prepared.accept();

        assert_eq!(title, "Implement the feature");
        let authoring = app.paths.task_dir(&task_id).join("authoring");
        let metadata: ConversationMetadata =
            read_json(&authoring.join("conversation.json")).unwrap();
        assert_eq!(metadata.run_mode, "auto");
        assert_eq!(metadata.source, CONVERSATION_SOURCE_CLI);
        let auto: ConversationAutoConfig =
            read_json(&app.paths.task_auto_config_file(&task_id)).unwrap();
        assert_eq!(auto.agent_type, "claude-acp");
        assert_eq!(
            auto.config_options.get("effort").map(String::as_str),
            Some("max")
        );
        assert_eq!(auto.allowed_profiles, Some(vec!["dev".to_string()]));
        // Template identity belongs to the authoring surface, not the task.
        assert!(auto.active_template_id.is_none());
        assert!(auto.active_template_name.is_none());
    }

    #[test]
    fn residency_is_durable_direct_only_and_does_not_start_a_run() {
        let (_temp, repo) = git_repo();
        let app = App::with_config(repo, RuntimeConfig::default());
        let prepared = app.prepare_conversation_task(&auto_command()).unwrap();
        let (task_id, _, _) = prepared.accept();
        assert!(
            app.set_conversation_resident(&task_id, true)
                .unwrap_err()
                .downcast_ref::<super::ConversationResidencyError>()
                .is_some()
        );
        let path = app
            .paths
            .task_dir(&task_id)
            .join("authoring/conversation.json");
        let mut metadata: ConversationMetadata = read_json(&path).unwrap();
        metadata.run_mode = "direct".into();
        crate::storage::write_json(&path, &metadata).unwrap();
        app.set_conversation_resident(&task_id, true).unwrap();
        let saved: ConversationMetadata = read_json(&path).unwrap();
        assert!(saved.resident);
        assert_eq!(saved.created_at, metadata.created_at);
        assert!(app.run_list(&task_id).unwrap().is_empty());
        let reloaded = App::with_config(app.paths.repo_root.clone(), RuntimeConfig::default());
        reloaded.set_conversation_resident(&task_id, false).unwrap();
        assert!(!read_json::<ConversationMetadata>(&path).unwrap().resident);
    }

    #[test]
    fn unaccepted_conversation_task_is_rolled_back() {
        let (_temp, repo) = git_repo();
        let app = App::with_config(repo, RuntimeConfig::default());

        let task_dir = {
            let prepared = app.prepare_conversation_task(&auto_command()).unwrap();
            let task_dir = app.paths.task_dir(prepared.task_id());
            assert!(task_dir.exists());
            task_dir
        };

        assert!(!task_dir.exists());
    }

    #[test]
    fn auto_conversation_requires_git_before_creating_task() {
        let temp = tempdir().unwrap();
        let repo = Utf8PathBuf::from_path_buf(temp.path().join("plain")).unwrap();
        std::fs::create_dir_all(repo.as_std_path()).unwrap();
        let app = App::with_config(repo, RuntimeConfig::default());

        let result =
            app.create_conversation_run(&auto_command(), ConversationRunLaunch::Foreground);

        assert!(result.is_err());
        assert!(app.task_summaries().unwrap_or_default().is_empty());
    }

    #[test]
    fn empty_conversation_payload_is_rejected() {
        let (_temp, repo) = git_repo();
        let app = App::with_config(repo, RuntimeConfig::default());
        let mut command = auto_command();
        command.content.clear();

        assert!(app.prepare_conversation_task(&command).is_err());
        assert!(app.task_summaries().unwrap_or_default().is_empty());
    }
}
