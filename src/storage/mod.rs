pub mod core_state;
pub mod sqlite;

use crate::config::{
    ManagedAgentId, ProjectIdentityConfig, SettingsConfig, project_identity_config,
};
use crate::domain::VERSION;
use anyhow::{Result, anyhow};
use atomic_write_file::AtomicWriteFile;
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct StoragePathConfig {
    pub app_key: &'static str,
    pub config_dir_name: &'static str,
    pub home_env_var: &'static str,
}

const DEFAULT_STORAGE_PATH_CONFIG: StoragePathConfig = StoragePathConfig {
    app_key: "gold-band",
    config_dir_name: ".gold-band",
    home_env_var: "GOLD_BAND_HOME",
};

static STORAGE_PATH_CONFIG: OnceLock<RwLock<StoragePathConfig>> = OnceLock::new();
static STORAGE_FILE_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();

pub fn configure_storage_paths(config: StoragePathConfig) {
    *storage_path_config_lock()
        .write()
        .expect("storage path config lock poisoned") = config;
}

pub fn active_storage_path_config() -> StoragePathConfig {
    *storage_path_config_lock()
        .read()
        .expect("storage path config lock poisoned")
}

fn storage_path_config_lock() -> &'static RwLock<StoragePathConfig> {
    STORAGE_PATH_CONFIG.get_or_init(|| RwLock::new(DEFAULT_STORAGE_PATH_CONFIG))
}

#[derive(Debug, Clone)]
pub struct GoldBandPaths {
    pub repo_root: Utf8PathBuf,
    pub repo_gold_band_root: Utf8PathBuf,
    pub user_gold_band_root: Utf8PathBuf,
    pub runtime_root: Utf8PathBuf,
    pub project_id: String,
    pub normalized_repo_root: String,
    path_config: StoragePathConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectManifest {
    pub version: String,
    pub project_id: String,
    pub repo_root: String,
    pub normalized_repo_root: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectManifestError {
    #[error("project manifest is missing for existing runtime directory {path}")]
    Missing { path: Utf8PathBuf },
    #[error("project manifest is invalid at {path}")]
    Invalid { path: Utf8PathBuf },
    #[error(
        "project manifest at {path} belongs to project {found_project_id} ({found_normalized_repo_root}), expected {expected_project_id} ({expected_normalized_repo_root})"
    )]
    Mismatch {
        path: Utf8PathBuf,
        found_project_id: String,
        found_normalized_repo_root: String,
        expected_project_id: String,
        expected_normalized_repo_root: String,
    },
}

impl ProjectManifestError {
    pub fn code(&self) -> &'static str {
        "workspace.manifest-mismatch"
    }

    pub fn params(&self) -> serde_json::Value {
        match self {
            Self::Missing { path } | Self::Invalid { path } => {
                serde_json::json!({ "path": path })
            }
            Self::Mismatch {
                path,
                found_project_id,
                found_normalized_repo_root,
                expected_project_id,
                expected_normalized_repo_root,
            } => serde_json::json!({
                "path": path,
                "foundProjectId": found_project_id,
                "foundNormalizedWorkspacePath": found_normalized_repo_root,
                "expectedProjectId": expected_project_id,
                "expectedNormalizedWorkspacePath": expected_normalized_repo_root,
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectManifestProvision {
    Unchanged,
    Written,
}

impl GoldBandPaths {
    pub fn storage_path_config(&self) -> StoragePathConfig {
        self.path_config
    }

    pub fn new(repo_root: impl Into<Utf8PathBuf>) -> Self {
        Self::new_with_path_and_identity_config(
            repo_root,
            active_storage_path_config(),
            project_identity_config(),
        )
    }

    pub fn new_with_path_config(
        repo_root: impl Into<Utf8PathBuf>,
        path_config: StoragePathConfig,
    ) -> Self {
        Self::new_with_path_and_identity_config(repo_root, path_config, project_identity_config())
    }

    pub fn new_with_path_and_identity_config(
        repo_root: impl Into<Utf8PathBuf>,
        path_config: StoragePathConfig,
        identity_config: &ProjectIdentityConfig,
    ) -> Self {
        identity_config
            .validate()
            .expect("project identity config must be valid");
        let repo_root = repo_root.into();
        let normalized_repo_root = normalize_workspace_path(&repo_root);
        let project_id = project_id_from_normalized(&normalized_repo_root, identity_config);
        let repo_gold_band_root = repo_root.join(path_config.config_dir_name);
        let user_gold_band_root = user_gold_band_root(&repo_root, path_config);
        let runtime_root = user_gold_band_root.join("projects").join(&project_id);
        Self {
            repo_root,
            repo_gold_band_root,
            user_gold_band_root,
            runtime_root,
            project_id,
            normalized_repo_root,
            path_config,
        }
    }

    pub fn project_manifest_file(&self) -> Utf8PathBuf {
        self.runtime_root.join("project.json")
    }

    pub fn provision_project_manifest(&self) -> Result<ProjectManifestProvision> {
        let path = self.project_manifest_file();
        let expected = self.expected_project_manifest();
        if self.runtime_root.exists() {
            if path.is_file() {
                let found = self.read_validated_project_manifest()?;
                if found == expected {
                    return Ok(ProjectManifestProvision::Unchanged);
                }
            } else if std::fs::read_dir(self.runtime_root.as_std_path())
                .map(|mut entries| entries.next().is_some())
                .unwrap_or(true)
            {
                return Err(ProjectManifestError::Missing { path }.into());
            }
        }
        write_json(&path, &expected)?;
        Ok(ProjectManifestProvision::Written)
    }

    pub fn validate_project_manifest(&self) -> Result<()> {
        self.read_validated_project_manifest().map(|_| ())
    }

    pub fn replace_project_manifest_for_migration(&self) -> Result<()> {
        write_json(
            &self.project_manifest_file(),
            &self.expected_project_manifest(),
        )
    }

    fn expected_project_manifest(&self) -> ProjectManifest {
        ProjectManifest {
            version: VERSION.to_string(),
            project_id: self.project_id.clone(),
            repo_root: self.repo_root.to_string(),
            normalized_repo_root: self.normalized_repo_root.clone(),
        }
    }

    fn read_validated_project_manifest(&self) -> Result<ProjectManifest> {
        let path = self.project_manifest_file();
        if !path.is_file() {
            return Err(ProjectManifestError::Missing { path }.into());
        }
        let found = read_json::<ProjectManifest>(&path)
            .map_err(|_| ProjectManifestError::Invalid { path: path.clone() })?;
        let expected = self.expected_project_manifest();
        if found.project_id != expected.project_id
            || found.normalized_repo_root != expected.normalized_repo_root
        {
            return Err(ProjectManifestError::Mismatch {
                path,
                found_project_id: found.project_id,
                found_normalized_repo_root: found.normalized_repo_root,
                expected_project_id: expected.project_id,
                expected_normalized_repo_root: expected.normalized_repo_root,
            }
            .into());
        }
        Ok(found)
    }

    pub fn repo_presets_dir(&self) -> Utf8PathBuf {
        self.repo_gold_band_root.join("presets")
    }

    pub fn repo_profiles_dir(&self) -> Utf8PathBuf {
        self.repo_presets_dir().join("profiles")
    }

    pub fn repo_profile_file(&self, profile_name: &str) -> Utf8PathBuf {
        self.repo_profiles_dir().join(format!("{profile_name}.md"))
    }

    pub fn user_gold_band_dir(&self) -> Utf8PathBuf {
        self.user_gold_band_root.clone()
    }

    pub fn user_settings_file(&self) -> Utf8PathBuf {
        self.user_gold_band_dir().join("settings.json")
    }

    pub fn user_state_file(&self) -> Utf8PathBuf {
        if let Some(home) = std::env::var(self.path_config.home_env_var)
            .ok()
            .filter(|value| !value.trim().is_empty())
        {
            return Utf8PathBuf::from(home)
                .join(self.path_config.config_dir_name)
                .join("state.json");
        }
        let dir = dirs::data_local_dir()
            .and_then(|p| Utf8PathBuf::from_path_buf(p).ok())
            .unwrap_or_else(|| Utf8PathBuf::from("."));
        dir.join(self.path_config.app_key).join("state.json")
    }

    pub fn user_presets_dir(&self) -> Utf8PathBuf {
        self.user_gold_band_dir().join("presets")
    }

    pub fn user_profiles_dir(&self) -> Utf8PathBuf {
        self.user_presets_dir().join("profiles")
    }

    pub fn user_profile_file(&self, profile_name: &str) -> Utf8PathBuf {
        self.user_profiles_dir().join(format!("{profile_name}.md"))
    }

    pub fn user_context_dir(&self) -> Utf8PathBuf {
        self.user_gold_band_dir().join("context")
    }

    pub fn user_context_profiles_dir(&self) -> Utf8PathBuf {
        self.user_context_dir().join("profiles")
    }

    pub fn logs_dir(&self) -> Utf8PathBuf {
        self.user_gold_band_root.join("logs")
    }

    pub fn runtime_log_file(&self) -> Utf8PathBuf {
        self.logs_dir().join("runtime.log")
    }

    pub fn authoring_dir(&self) -> Utf8PathBuf {
        self.runtime_root.join("authoring")
    }

    pub fn workflow_templates_file(&self) -> Utf8PathBuf {
        self.user_context_dir().join("workflows.json")
    }

    pub fn legacy_project_workflow_templates_file(&self) -> Utf8PathBuf {
        self.authoring_dir().join("workflows.json")
    }

    pub fn auto_templates_file(&self) -> Utf8PathBuf {
        self.user_context_dir().join("auto-templates.json")
    }

    pub fn agent_diagnostics_file(&self) -> Utf8PathBuf {
        self.user_gold_band_root
            .join("desktop/agent-diagnostics.json")
    }

    pub fn agent_command_catalogs_file(&self) -> Utf8PathBuf {
        self.user_gold_band_root
            .join("desktop/agent-command-catalogs.json")
    }

    pub fn doctor_dir(&self) -> Utf8PathBuf {
        self.user_gold_band_root.join("doctor")
    }

    pub fn doctor_acp_root_dir(&self) -> Utf8PathBuf {
        self.doctor_dir().join("acp")
    }

    pub fn doctor_acp_dir(&self, agent_id: &ManagedAgentId) -> Utf8PathBuf {
        self.doctor_acp_root_dir().join(agent_id.as_str())
    }

    pub fn doctor_acp_provider_pid_file(&self, agent_id: &ManagedAgentId) -> Utf8PathBuf {
        self.doctor_acp_dir(agent_id).join("provider.pid")
    }

    pub fn sqlite_db_path(&self) -> Utf8PathBuf {
        self.user_gold_band_root.join("gold-band.db")
    }

    pub fn core_db_path(&self) -> Utf8PathBuf {
        self.user_gold_band_root.join("core.db")
    }

    pub fn scheduler_db_path(&self) -> Utf8PathBuf {
        self.core_db_path()
    }

    // ── SKILL paths ──

    pub fn global_skills_dir() -> Utf8PathBuf {
        let home = dirs::home_dir()
            .and_then(|p| Utf8PathBuf::from_path_buf(p).ok())
            .unwrap_or_else(|| Utf8PathBuf::from("."));
        home.join(crate::config::GOLD_BAND_DIR_NAME)
            .join(crate::config::SKILLS_DIR_NAME)
    }

    pub fn project_skills_dir(&self) -> Utf8PathBuf {
        self.repo_root
            .join(crate::config::GOLD_BAND_DIR_NAME)
            .join(crate::config::SKILLS_DIR_NAME)
    }

    pub fn projects_dir(&self) -> Utf8PathBuf {
        self.user_gold_band_root.join("projects")
    }

    pub fn tasks_dir(&self) -> Utf8PathBuf {
        self.runtime_root.join("tasks")
    }

    pub fn conversation_worktrees_dir(&self) -> Utf8PathBuf {
        self.runtime_root.join("worktrees")
    }

    pub fn scheduled_tasks_dir(&self) -> Utf8PathBuf {
        self.runtime_root.join("scheduled-tasks")
    }

    pub fn scheduled_task_dir(&self, scheduled_task_id: &str) -> Utf8PathBuf {
        self.scheduled_tasks_dir().join(scheduled_task_id)
    }

    pub fn scheduled_task_file(&self, scheduled_task_id: &str) -> Utf8PathBuf {
        self.scheduled_task_dir(scheduled_task_id)
            .join("scheduled-task.json")
    }

    pub fn scheduled_triggers_dir(&self, scheduled_task_id: &str) -> Utf8PathBuf {
        self.scheduled_task_dir(scheduled_task_id).join("triggers")
    }

    pub fn scheduled_trigger_file(&self, scheduled_task_id: &str, trigger_id: &str) -> Utf8PathBuf {
        self.scheduled_triggers_dir(scheduled_task_id)
            .join(format!("{trigger_id}.json"))
    }

    pub fn task_dir(&self, task_id: &str) -> Utf8PathBuf {
        self.tasks_dir().join(task_id)
    }

    pub fn task_file(&self, task_id: &str) -> Utf8PathBuf {
        self.task_dir(task_id).join("task.json")
    }

    pub fn conversation_attention_file(&self) -> Utf8PathBuf {
        self.runtime_root.join("conversation-attention.json")
    }

    pub fn requirement_file(&self, task_id: &str) -> Utf8PathBuf {
        self.task_dir(task_id).join("authoring/requirement.md")
    }

    pub fn workflow_file(&self, task_id: &str) -> Utf8PathBuf {
        self.task_dir(task_id).join("authoring/workflow.json")
    }

    pub fn task_workflow_resolved_file(&self, task_id: &str) -> Utf8PathBuf {
        self.task_dir(task_id)
            .join("authoring/workflow.resolved.json")
    }

    pub fn task_provenance_file(&self, task_id: &str) -> Utf8PathBuf {
        self.task_dir(task_id).join("authoring/provenance.json")
    }

    pub fn runs_dir(&self, task_id: &str) -> Utf8PathBuf {
        self.task_dir(task_id).join("runs")
    }

    pub fn run_dir(&self, task_id: &str, run_id: &str) -> Utf8PathBuf {
        self.runs_dir(task_id).join(run_id)
    }

    pub fn run_file(&self, task_id: &str, run_id: &str) -> Utf8PathBuf {
        self.run_dir(task_id, run_id).join("run.json")
    }

    pub fn workflow_snapshot_file(&self, task_id: &str, run_id: &str) -> Utf8PathBuf {
        self.run_dir(task_id, run_id).join("workflow.snapshot.json")
    }

    pub fn run_progress_file(&self, task_id: &str, run_id: &str) -> Utf8PathBuf {
        self.run_dir(task_id, run_id).join("run-progress.json")
    }

    pub fn run_events_file(&self, task_id: &str, run_id: &str) -> Utf8PathBuf {
        self.run_dir(task_id, run_id).join("events.jsonl")
    }

    pub fn round_dir(&self, task_id: &str, run_id: &str, round_id: &str) -> Utf8PathBuf {
        self.run_dir(task_id, run_id).join("rounds").join(round_id)
    }

    pub fn round_file(&self, task_id: &str, run_id: &str, round_id: &str) -> Utf8PathBuf {
        self.round_dir(task_id, run_id, round_id).join("round.json")
    }

    pub fn node_dir(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
    ) -> Utf8PathBuf {
        self.round_dir(task_id, run_id, round_id)
            .join("nodes")
            .join(node_id)
    }

    pub fn attempt_dir(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Utf8PathBuf {
        self.node_dir(task_id, run_id, round_id, node_id)
            .join(attempt_id)
    }

    pub fn node_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Utf8PathBuf {
        self.attempt_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("node.json")
    }

    pub fn worker_ref_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Utf8PathBuf {
        self.attempt_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("worker-ref.json")
    }

    pub fn provider_pid_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Utf8PathBuf {
        self.attempt_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("provider.pid")
    }

    pub fn artifacts_dir(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Utf8PathBuf {
        self.attempt_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("artifacts")
    }

    pub fn artifact_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        name: &str,
    ) -> Utf8PathBuf {
        self.artifacts_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join(format!("{name}.json"))
    }

    pub fn attachments_dir(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Utf8PathBuf {
        self.attempt_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("attachments")
    }

    pub fn progress_events_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Utf8PathBuf {
        self.attempt_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("progress.events.jsonl")
    }

    pub fn raw_stream_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Utf8PathBuf {
        self.attempt_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("raw.stream.jsonl")
    }

    pub fn acp_session_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Utf8PathBuf {
        self.attempt_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("acp.session.json")
    }

    pub fn acp_snapshot_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Utf8PathBuf {
        self.attempt_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("acp.snapshot.json")
    }

    pub fn acp_events_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Utf8PathBuf {
        self.attempt_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("acp.events.jsonl")
    }

    pub fn acp_timeline_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Utf8PathBuf {
        self.attempt_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("acp.timeline.jsonl")
    }

    pub fn acp_raw_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Utf8PathBuf {
        self.attempt_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("acp.raw.jsonl")
    }

    pub fn acp_diagnostics_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Utf8PathBuf {
        self.attempt_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("acp.diagnostics.jsonl")
    }

    pub fn dynamic_dir(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Utf8PathBuf {
        self.attempt_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("dynamic")
    }

    pub fn dynamic_run_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Utf8PathBuf {
        self.dynamic_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("dynamic-run.json")
    }

    pub fn dynamic_allowed_workflow_snapshots_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Utf8PathBuf {
        self.dynamic_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("allowed-workflow-snapshots.json")
    }

    pub fn dynamic_graph_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Utf8PathBuf {
        self.dynamic_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("graph.json")
    }

    pub fn dynamic_events_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Utf8PathBuf {
        self.dynamic_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("events.jsonl")
    }

    pub fn dynamic_group_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        group_id: &str,
    ) -> Utf8PathBuf {
        self.dynamic_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("groups")
            .join(format!("{group_id}.json"))
    }

    pub fn dynamic_workspace_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        workspace_id: &str,
    ) -> Utf8PathBuf {
        self.dynamic_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("workspaces")
            .join(format!("{workspace_id}.json"))
    }

    pub fn dynamic_node_dir(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        dynamic_node_id: &str,
    ) -> Utf8PathBuf {
        self.dynamic_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join("nodes")
            .join(dynamic_node_id)
    }

    pub fn dynamic_node_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        dynamic_node_id: &str,
    ) -> Utf8PathBuf {
        self.dynamic_node_dir(
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            dynamic_node_id,
        )
        .join("node.json")
    }

    pub fn dynamic_node_attempt_dir(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        dynamic_node_id: &str,
        dynamic_attempt_id: &str,
    ) -> Utf8PathBuf {
        self.dynamic_node_dir(
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            dynamic_node_id,
        )
        .join(dynamic_attempt_id)
    }

    pub fn dynamic_node_artifacts_dir(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        dynamic_node_id: &str,
        dynamic_attempt_id: &str,
    ) -> Utf8PathBuf {
        self.dynamic_node_attempt_dir(
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            dynamic_node_id,
            dynamic_attempt_id,
        )
        .join("artifacts")
    }

    pub fn dynamic_node_artifact_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        dynamic_node_id: &str,
        dynamic_attempt_id: &str,
        name: &str,
    ) -> Utf8PathBuf {
        self.dynamic_node_artifacts_dir(
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            dynamic_node_id,
            dynamic_attempt_id,
        )
        .join(format!("{name}.json"))
    }

    pub fn dynamic_node_attachments_dir(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        dynamic_node_id: &str,
        dynamic_attempt_id: &str,
    ) -> Utf8PathBuf {
        self.dynamic_node_attempt_dir(
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            dynamic_node_id,
            dynamic_attempt_id,
        )
        .join("attachments")
    }

    pub fn dynamic_node_worker_ref_file(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        dynamic_node_id: &str,
        dynamic_attempt_id: &str,
    ) -> Utf8PathBuf {
        self.dynamic_node_attempt_dir(
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            dynamic_node_id,
            dynamic_attempt_id,
        )
        .join("worker-ref.json")
    }
}

fn user_gold_band_root(repo_root: &Utf8Path, path_config: StoragePathConfig) -> Utf8PathBuf {
    if let Some(home) = std::env::var(path_config.home_env_var)
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return Utf8PathBuf::from(home).join(path_config.config_dir_name);
    }

    if is_under_system_temp(repo_root) {
        return repo_root
            .join(format!("{}-home", path_config.app_key))
            .join(path_config.config_dir_name);
    }

    let home = dirs::home_dir()
        .and_then(|p| Utf8PathBuf::from_path_buf(p).ok())
        .unwrap_or_else(|| Utf8PathBuf::from("."));
    home.join(path_config.config_dir_name)
}

fn is_under_system_temp(path: &Utf8Path) -> bool {
    let path = normalize_workspace_path(path);
    std::env::temp_dir()
        .to_str()
        .map(Utf8Path::new)
        .map(normalize_workspace_path)
        .is_some_and(|temp| path.starts_with(&temp))
}

pub fn normalize_workspace_path(repo_root: &Utf8Path) -> String {
    let canonical = std::fs::canonicalize(repo_root.as_std_path())
        .ok()
        .and_then(|path| Utf8PathBuf::from_path_buf(path).ok())
        .unwrap_or_else(|| repo_root.to_path_buf());
    let normalized = canonical
        .to_string()
        .replace('\\', "/")
        .trim_start_matches("//?/")
        .to_string();
    if cfg!(windows) {
        normalized.to_ascii_lowercase()
    } else {
        normalized
    }
}

pub fn project_id_for_workspace(repo_root: &Utf8Path) -> String {
    let normalized = normalize_workspace_path(repo_root);
    project_id_from_normalized(&normalized, project_identity_config())
}

pub fn legacy_project_id_for_workspace(repo_root: &Utf8Path) -> String {
    let canonical = std::fs::canonicalize(repo_root.as_std_path())
        .ok()
        .and_then(|path| Utf8PathBuf::from_path_buf(path).ok())
        .unwrap_or_else(|| repo_root.to_path_buf());
    let mut id = String::new();
    for character in canonical.to_string().replace('\\', "/").chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '.' | '_') {
            id.push(character);
        } else if matches!(character, ':' | '/') || !id.ends_with('-') {
            id.push('-');
        }
    }
    let trimmed = id.trim_matches('-');
    if trimmed.is_empty() {
        "root".to_string()
    } else {
        trimmed.to_string()
    }
}

fn project_id_from_normalized(
    normalized_repo_root: &str,
    identity_config: &ProjectIdentityConfig,
) -> String {
    let mut slug = String::new();
    for character in normalized_repo_root.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '.' | '_') {
            slug.push(character);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let mut slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        slug.push_str("root");
    }
    let slug_max_length = identity_config.slug_max_length();
    if slug.len() > slug_max_length {
        slug = slug[slug.len() - slug_max_length..]
            .trim_start_matches('-')
            .to_string();
    }
    if slug.is_empty() {
        slug.push_str("root");
    }

    let encoded = blake3::hash(normalized_repo_root.as_bytes()).to_hex();
    let hash = &encoded.as_str()[..identity_config.hash_hex_length];
    format!("{}{}{}", slug, identity_config.separator, hash)
}

pub fn ensure_parent_dir(path: &Utf8Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub fn atomic_write_file<T, E>(
    path: &Path,
    write: impl FnOnce(&mut AtomicWriteFile) -> std::result::Result<T, E>,
) -> std::result::Result<T, E>
where
    E: From<std::io::Error>,
{
    let mut file = AtomicWriteFile::open(path).map_err(E::from)?;
    let value = write(&mut file)?;
    file.commit().map_err(E::from)?;
    Ok(value)
}

pub fn write_json<T: Serialize>(path: &Utf8Path, value: &T) -> Result<()> {
    ensure_parent_dir(path)?;
    let content = serde_json::to_vec_pretty(value)?;
    atomic_write_file(path.as_std_path(), |file| -> Result<()> {
        file.write_all(&content)?;
        Ok(())
    })
}

pub fn read_json<T: serde::de::DeserializeOwned>(path: &Utf8Path) -> Result<T> {
    const MAX_ATTEMPTS: usize = 5;
    for attempt in 0..MAX_ATTEMPTS {
        let content = std::fs::read_to_string(path)?;
        match serde_json::from_str(&content) {
            Ok(value) => return Ok(value),
            Err(error)
                if attempt + 1 < MAX_ATTEMPTS && should_retry_json_read(&content, &error) =>
            {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error.into()),
        }
    }
    unreachable!("read_json should have returned within retry loop")
}

pub fn load_settings_file(path: &Utf8Path) -> Result<SettingsConfig> {
    if !path.exists() {
        return Ok(SettingsConfig::default());
    }
    let value: serde_json::Value = read_json(path)?;
    let (settings, migrated) = SettingsConfig::from_json_value_with_migration(value)?;
    if migrated {
        write_json(path, &settings)?;
    }
    Ok(settings)
}

fn should_retry_json_read(content: &str, error: &serde_json::Error) -> bool {
    content.trim().is_empty()
        || matches!(
            error.classify(),
            serde_json::error::Category::Eof | serde_json::error::Category::Syntax
        )
}

pub fn with_jsonl_file_lock<T>(
    path: &Utf8Path,
    operation: impl FnOnce() -> Result<T>,
) -> Result<T> {
    with_file_lock(path, operation)
}

pub fn with_file_lock<T>(path: &Utf8Path, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    let lock = storage_file_lock_for(path)?;
    let _guard = lock
        .lock()
        .map_err(|_| anyhow!("storage file lock poisoned"))?;
    operation()
}

fn storage_file_lock_for(path: &Utf8Path) -> Result<Arc<Mutex<()>>> {
    let key = storage_file_lock_key(path);
    let mut locks = STORAGE_FILE_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| anyhow!("storage file lock registry poisoned"))?;
    Ok(locks
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone())
}

fn storage_file_lock_key(path: &Utf8Path) -> String {
    let normalized = std::fs::canonicalize(path.as_std_path())
        .ok()
        .and_then(|path| Utf8PathBuf::from_path_buf(path).ok())
        .unwrap_or_else(|| path.to_path_buf())
        .to_string()
        .replace('\\', "/")
        .trim_start_matches("//?/")
        .to_string();
    if cfg!(windows) {
        normalized.to_ascii_lowercase()
    } else {
        normalized
    }
}

pub fn append_jsonl<T: Serialize>(path: &Utf8Path, value: &T) -> Result<()> {
    let line = serde_json::to_vec(value)?;
    with_jsonl_file_lock(path, || append_jsonl_line_unlocked(path, &line))
}

/// Append one JSONL record and force its bytes to the operating-system storage
/// boundary before returning. Use this for small write-ahead journals whose
/// records must survive an application-process crash.
pub fn append_jsonl_durable<T: Serialize>(path: &Utf8Path, value: &T) -> Result<()> {
    with_jsonl_file_lock(path, || append_jsonl_durable_unlocked(path, value))
}

/// Durable JSONL append for callers that already hold this path's JSONL lock.
pub fn append_jsonl_durable_unlocked<T: Serialize>(path: &Utf8Path, value: &T) -> Result<()> {
    let line = serde_json::to_vec(value)?;
    ensure_parent_dir(path)?;
    repair_jsonl_tail_unlocked(path)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path.as_std_path())?;
    file.write_all(&line)?;
    file.write_all(b"\n")?;
    file.flush()?;
    file.sync_data()?;
    Ok(())
}

pub fn append_jsonl_unlocked<T: Serialize>(path: &Utf8Path, value: &T) -> Result<()> {
    let line = serde_json::to_vec(value)?;
    append_jsonl_line_unlocked(path, &line)
}

/// Append one JSONL record and flush it to the operating-system file boundary.
/// This is the appropriate commit point for high-frequency append logs that
/// need process-crash recovery without forcing a physical disk sync per frame.
pub fn append_jsonl_flushed_unlocked<T: Serialize>(path: &Utf8Path, value: &T) -> Result<()> {
    let line = serde_json::to_vec(value)?;
    ensure_parent_dir(path)?;
    repair_jsonl_tail_unlocked(path)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path.as_std_path())?;
    file.write_all(&line)?;
    file.write_all(b"\n")?;
    file.flush()?;
    Ok(())
}

fn append_jsonl_line_unlocked(path: &Utf8Path, line: &[u8]) -> Result<()> {
    ensure_parent_dir(path)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path.as_std_path())?;
    file.write_all(line)?;
    file.write_all(b"\n")?;
    Ok(())
}

fn repair_jsonl_tail_unlocked(path: &Utf8Path) -> Result<()> {
    let Ok(mut file) = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path.as_std_path())
    else {
        return Ok(());
    };
    let len = file.metadata()?.len();
    if len == 0 {
        return Ok(());
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0u8; 1];
    file.read_exact(&mut last)?;
    if last[0] == b'\n' {
        return Ok(());
    }

    // A missing newline is exceptional (usually an interrupted append). Only
    // pay for reading the file on that recovery path; healthy appends inspect
    // one byte instead of re-reading the complete, ever-growing journal.
    file.seek(SeekFrom::Start(0))?;
    let mut content = Vec::with_capacity(len as usize);
    file.read_to_end(&mut content)?;

    let tail_start = content
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index.saturating_add(1));
    let tail = &content[tail_start..];
    if serde_json::from_slice::<serde_json::Value>(tail).is_ok() {
        file.seek(SeekFrom::End(0))?;
        file.write_all(b"\n")?;
        return Ok(());
    }

    file.set_len(tail_start as u64)?;
    Ok(())
}

/// Trim a JSONL file from the beginning when it exceeds `max_size`,
/// keeping the most recent lines that fit within `target_size`.
pub fn roll_jsonl(path: &Utf8Path, max_size: u64, target_size: u64) -> Result<()> {
    with_jsonl_file_lock(path, || roll_jsonl_unlocked(path, max_size, target_size))
}

pub fn roll_jsonl_unlocked(path: &Utf8Path, max_size: u64, target_size: u64) -> Result<()> {
    let meta = match std::fs::metadata(path.as_std_path()) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };
    if meta.len() <= max_size {
        return Ok(());
    }
    let content = std::fs::read(path.as_std_path())?;
    let total = content.len() as u64;
    if total <= target_size {
        return Ok(());
    }
    let excess = total.saturating_sub(target_size);
    let mut cumulative = 0u64;
    let mut drop_bytes = 0usize;
    for line in content.split_inclusive(|byte| *byte == b'\n') {
        if cumulative >= excess {
            break;
        }
        cumulative += line.len() as u64;
        drop_bytes += line.len();
    }
    let drop_bytes = drop_bytes.min(content.len());
    let keep = &content[drop_bytes..];
    std::fs::write(path.as_std_path(), keep)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        CURRENT_SETTINGS_SCHEMA_VERSION, ManagedAgentId, catalog_agent_default_config,
    };
    use std::str::FromStr;
    use tempfile;

    #[test]
    fn project_paths_split_repo_config_and_user_runtime() {
        let paths = GoldBandPaths::new_with_path_config(
            Utf8PathBuf::from("D:/Projects/Example App"),
            DEFAULT_STORAGE_PATH_CONFIG,
        );

        assert_eq!(
            paths.repo_presets_dir(),
            Utf8PathBuf::from("D:/Projects/Example App/.gold-band/presets")
        );
        assert!(
            paths
                .task_file("task-001")
                .to_string()
                .replace('\\', "/")
                .contains(&format!("/.gold-band/projects/{}/", paths.project_id))
        );
        assert!(
            paths
                .runtime_log_file()
                .to_string()
                .replace('\\', "/")
                .ends_with("/.gold-band/logs/runtime.log")
        );
    }

    #[test]
    fn project_id_is_stable_for_same_input() {
        let first = GoldBandPaths::new_with_path_config(
            Utf8PathBuf::from("D:/Projects/Gold-Band"),
            DEFAULT_STORAGE_PATH_CONFIG,
        );
        let second = GoldBandPaths::new_with_path_config(
            Utf8PathBuf::from("D:/Projects/Gold-Band"),
            DEFAULT_STORAGE_PATH_CONFIG,
        );

        assert_eq!(first.project_id, second.project_id);
        assert_eq!(first.project_id.len(), first.project_id.as_bytes().len());
        assert!(first.project_id.ends_with(&format!(
            "--{}",
            &blake3::hash(first.normalized_repo_root.as_bytes()).to_hex().as_str()[..8]
        )));
        assert!(first.project_id.len() <= 80);
    }

    #[test]
    fn project_id_uses_configured_length_and_preserves_slug_tail() {
        let identity = ProjectIdentityConfig {
            max_length: 24,
            hash_hex_length: 8,
            separator: "--".to_string(),
        };
        let id = project_id_from_normalized("d:/very/long/parent/path/final-repository", &identity);

        assert_eq!(id.len(), 24);
        assert!(id.starts_with("nal-repository"));
        assert_eq!(id.matches("--").count(), 1);
    }

    #[test]
    fn project_id_blake3_contract_uses_a_fixed_vector() {
        assert_eq!(
            project_id_from_normalized(
                "d:/projects/example-app",
                &ProjectIdentityConfig::default(),
            ),
            "d-projects-example-app--3d4964d2"
        );
    }

    #[cfg(windows)]
    #[test]
    fn project_id_normalizes_windows_case_and_separators() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Utf8PathBuf::from_path_buf(directory.path().join("Mixed-Case")).unwrap();
        std::fs::create_dir_all(workspace.as_std_path()).unwrap();
        let uppercase_backslashes =
            Utf8PathBuf::from(workspace.as_str().replace('/', "\\").to_ascii_uppercase());

        assert_eq!(
            project_id_for_workspace(&workspace),
            project_id_for_workspace(&uppercase_backslashes)
        );
    }

    #[test]
    fn supports_custom_config_directory_names() {
        let paths = GoldBandPaths::new_with_path_config(
            Utf8PathBuf::from("D:/Projects/Example App"),
            StoragePathConfig {
                app_key: "maling",
                config_dir_name: ".maling",
                home_env_var: "MALING_HOME",
            },
        );

        assert_eq!(
            paths.repo_presets_dir(),
            Utf8PathBuf::from("D:/Projects/Example App/.maling/presets")
        );
        assert!(
            paths
                .task_file("task-001")
                .to_string()
                .replace('\\', "/")
                .contains(&format!("/.maling/projects/{}/", paths.project_id))
        );
    }

    #[test]
    fn recognizes_system_temp_paths() {
        let repo_root =
            Utf8PathBuf::from_path_buf(std::env::temp_dir().join("gold-band-test-repo")).unwrap();

        assert!(is_under_system_temp(&repo_root));
    }

    #[test]
    fn settings_file_in_user_gold_band_dir() {
        let paths = GoldBandPaths::new_with_path_config(
            Utf8PathBuf::from("D:/Projects/Example App"),
            DEFAULT_STORAGE_PATH_CONFIG,
        );
        let settings = paths.user_settings_file();
        assert!(
            settings
                .to_string()
                .replace('\\', "/")
                .ends_with("/.gold-band/settings.json")
        );
    }

    #[test]
    fn state_file_in_data_local_dir_by_default() {
        unsafe { std::env::remove_var("GOLD_BAND_HOME") };
        let paths = GoldBandPaths::new_with_path_config(
            Utf8PathBuf::from("D:/Projects/Example App"),
            DEFAULT_STORAGE_PATH_CONFIG,
        );
        let state = paths.user_state_file();
        let normalized = state.to_string().replace('\\', "/");
        assert!(
            normalized.ends_with("state.json"),
            "expected state.json path, got: {normalized}"
        );
        assert!(
            normalized.contains("gold-band"),
            "expected gold-band in path, got: {normalized}"
        );
    }

    #[test]
    fn state_file_under_home_env_when_set() {
        let temp = tempfile::tempdir().unwrap();
        let path_config = StoragePathConfig {
            app_key: "gold-band-test",
            config_dir_name: ".gold-band-test",
            home_env_var: "GOLD_BAND_TEST_HOME",
        };
        unsafe { std::env::set_var(path_config.home_env_var, temp.path().to_str().unwrap()) };
        let paths = GoldBandPaths::new_with_path_config(
            Utf8PathBuf::from("D:/Projects/Example App"),
            path_config,
        );
        let state = paths.user_state_file();
        unsafe { std::env::remove_var(path_config.home_env_var) };
        assert!(
            state
                .to_string()
                .replace('\\', "/")
                .ends_with("/.gold-band-test/state.json")
        );
        assert!(
            state
                .to_string()
                .replace('\\', "/")
                .contains("gold-band-test")
        );
    }

    #[test]
    fn project_manifest_provision_writes_only_when_content_changes() {
        let dir = tempfile::tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(dir.path().join("workspace")).unwrap();
        std::fs::create_dir_all(repo_root.as_std_path()).unwrap();
        let paths = GoldBandPaths::new_with_path_config(repo_root, DEFAULT_STORAGE_PATH_CONFIG);

        assert_eq!(
            paths.provision_project_manifest().unwrap(),
            ProjectManifestProvision::Written
        );
        let manifest_path = paths.project_manifest_file();
        let first_modified = std::fs::metadata(manifest_path.as_std_path())
            .unwrap()
            .modified()
            .unwrap();
        std::thread::sleep(Duration::from_millis(20));

        assert_eq!(
            paths.provision_project_manifest().unwrap(),
            ProjectManifestProvision::Unchanged
        );
        assert_eq!(
            std::fs::metadata(manifest_path.as_std_path())
                .unwrap()
                .modified()
                .unwrap(),
            first_modified
        );

        let mut stale: ProjectManifest = read_json(&manifest_path).unwrap();
        stale.version = "0.12.4".to_string();
        stale.repo_root = "D:/moved-workspace".to_string();
        write_json(&manifest_path, &stale).unwrap();
        paths.validate_project_manifest().unwrap();
        assert_eq!(read_json::<ProjectManifest>(&manifest_path).unwrap(), stale);
        assert_eq!(
            paths.provision_project_manifest().unwrap(),
            ProjectManifestProvision::Written
        );
        let current: ProjectManifest = read_json(&manifest_path).unwrap();
        assert_eq!(current.version, VERSION);
        assert_eq!(current.repo_root, paths.repo_root.as_str());
    }

    #[test]
    fn project_manifest_rejects_a_different_workspace_owner() {
        let dir = tempfile::tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(dir.path().join("workspace")).unwrap();
        std::fs::create_dir_all(repo_root.as_std_path()).unwrap();
        let paths = GoldBandPaths::new_with_path_config(repo_root, DEFAULT_STORAGE_PATH_CONFIG);
        paths.provision_project_manifest().unwrap();
        let manifest_path = paths.project_manifest_file();
        let mut foreign: ProjectManifest = read_json(&manifest_path).unwrap();
        foreign.project_id = "foreign-project--00000000".to_string();
        foreign.normalized_repo_root = "d:/foreign/workspace".to_string();
        write_json(&manifest_path, &foreign).unwrap();

        let error = paths.provision_project_manifest().unwrap_err();

        assert!(error.downcast_ref::<ProjectManifestError>().is_some());
        assert_eq!(
            read_json::<ProjectManifest>(&manifest_path).unwrap(),
            foreign
        );
    }

    #[test]
    fn atomic_write_file_returns_open_errors() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("not-a-directory");
        std::fs::write(&parent, "blocking file").unwrap();
        let target = parent.join("state.json");

        let result: std::io::Result<()> = atomic_write_file(&target, |_file| Ok(()));

        assert!(result.is_err());
    }

    #[test]
    fn write_json_replaces_longer_existing_file_without_trailing_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("state.json")).unwrap();
        std::fs::write(path.as_std_path(), r#"{"items":[1,2,3],"stale":true}"#).unwrap();

        write_json(&path, &serde_json::json!({"ok": true})).unwrap();

        let contents = std::fs::read_to_string(path.as_std_path()).unwrap();
        assert_eq!(
            contents,
            r#"{
  "ok": true
}"#
        );
        assert_eq!(
            read_json::<serde_json::Value>(&path).unwrap(),
            serde_json::json!({"ok": true})
        );
        assert!(!contents.contains("stale"));
    }

    #[test]
    fn write_json_does_not_leave_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("state.json")).unwrap();

        write_json(&path, &serde_json::json!({"ok": true})).unwrap();

        let files = std::fs::read_dir(dir.path())
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name().to_string_lossy(), "state.json");
    }

    #[test]
    fn load_settings_file_migrates_and_persists_legacy_agent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("settings.json")).unwrap();
        let legacy = serde_json::json!({
            "agents": {
                "codex-cli": {
                    "adapter": catalog_agent_default_config("codex-acp").unwrap().adapter,
                    "skillsDirOverride": ".custom-codex",
                    "externalSessionSyncEnabled": false
                }
            }
        });
        write_json(&path, &legacy).unwrap();

        let settings = load_settings_file(&path).unwrap();

        let codex_id = ManagedAgentId::from_str("codex-acp").unwrap();
        let codex = &settings.agents.unwrap()[&codex_id];
        assert_eq!(codex.primary_agent_dir.as_deref(), Some(".custom-codex"));
        assert_eq!(codex.compatible_agent_dirs, vec![".agents"]);

        let persisted: serde_json::Value = read_json(&path).unwrap();
        assert_eq!(
            persisted["settingsSchemaVersion"],
            serde_json::json!(CURRENT_SETTINGS_SCHEMA_VERSION)
        );
        assert_eq!(
            persisted["agents"]["codex-acp"]["primaryAgentDir"],
            ".custom-codex"
        );
        assert_eq!(
            persisted["agents"]["codex-acp"]["compatibleAgentDirs"],
            serde_json::json!([".agents"])
        );
        assert!(persisted["agents"].get("codex-cli").is_none());
        assert!(
            persisted["agents"]["codex-acp"]
                .get("skillsDirOverride")
                .is_none()
        );
    }

    #[test]
    fn load_settings_file_migrates_and_persists_legacy_codex_package() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("settings.json")).unwrap();
        let legacy = serde_json::json!({
            "settingsSchemaVersion": 1,
            "agents": {
                "codex-acp": {
                    "adapter": {
                        "command": "npx",
                        "args": ["-y", "@zed-industries/codex-acp@latest"],
                        "displayName": "Codex",
                        "env": {}
                    },
                    "primaryAgentDir": ".codex",
                    "compatibleAgentDirs": [".agents"],
                    "externalSessionSyncEnabled": false
                }
            }
        });
        write_json(&path, &legacy).unwrap();

        let settings = load_settings_file(&path).unwrap();

        let codex_id = ManagedAgentId::from_str("codex-acp").unwrap();
        let codex = &settings.agents.unwrap()[&codex_id];
        assert_eq!(
            codex.adapter.args,
            vec!["-y", "@agentclientprotocol/codex-acp@latest"]
        );

        let persisted: serde_json::Value = read_json(&path).unwrap();
        assert_eq!(
            persisted["settingsSchemaVersion"],
            serde_json::json!(CURRENT_SETTINGS_SCHEMA_VERSION)
        );
        assert_eq!(
            persisted["agents"]["codex-acp"]["adapter"]["args"],
            serde_json::json!(["-y", "@agentclientprotocol/codex-acp@latest"])
        );
    }

    #[test]
    fn roll_jsonl_trims_oldest_lines_when_over_max() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("test.jsonl")).unwrap();

        // Write 3 lines totaling ~60+ bytes
        append_jsonl(&path, &"line-one-is-longer").unwrap();
        append_jsonl(&path, &"line-two").unwrap();
        append_jsonl(&path, &"line-three-even-longer").unwrap();

        let original = std::fs::read_to_string(path.as_std_path()).unwrap();
        assert_eq!(original.lines().count(), 3);

        // Set max so we need to drop first line
        let meta = std::fs::metadata(path.as_std_path()).unwrap();
        let target = meta.len() / 2; // keep roughly half
        roll_jsonl(&path, target.saturating_sub(1), target).unwrap();

        let after = std::fs::read_to_string(path.as_std_path()).unwrap();
        let lines: Vec<&str> = after.lines().collect();
        assert!(lines.len() < 3, "should have dropped some lines");
        assert!(
            after.len() as u64 <= target + 10,
            "should be roughly under target"
        );
    }

    #[test]
    fn roll_jsonl_noop_when_under_max() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("test.jsonl")).unwrap();

        append_jsonl(&path, &"hello").unwrap();
        let before = std::fs::read_to_string(path.as_std_path()).unwrap();

        // max far above current size
        roll_jsonl(&path, 1024 * 1024, 512 * 1024).unwrap();

        let after = std::fs::read_to_string(path.as_std_path()).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn append_jsonl_serializes_concurrent_same_path_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("concurrent.jsonl")).unwrap();
        let thread_count = 16;
        let writes_per_thread = 32;
        let payload = "x".repeat(16 * 1024);
        let mut handles = Vec::new();

        for thread_index in 0..thread_count {
            let path = path.clone();
            let payload = payload.clone();
            handles.push(std::thread::spawn(move || {
                for write_index in 0..writes_per_thread {
                    append_jsonl(
                        &path,
                        &serde_json::json!({
                            "thread": thread_index,
                            "write": write_index,
                            "payload": payload,
                        }),
                    )
                    .unwrap();
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let contents = std::fs::read_to_string(path.as_std_path()).unwrap();
        let mut seen = std::collections::HashSet::new();
        let mut line_count = 0;
        for line in contents.lines() {
            let value = serde_json::from_str::<serde_json::Value>(line).unwrap();
            let thread = value
                .get("thread")
                .and_then(|value| value.as_u64())
                .unwrap();
            let write = value.get("write").and_then(|value| value.as_u64()).unwrap();
            assert_eq!(
                value
                    .get("payload")
                    .and_then(|value| value.as_str())
                    .unwrap()
                    .len(),
                payload.len()
            );
            assert!(seen.insert((thread, write)));
            line_count += 1;
        }
        assert_eq!(line_count, thread_count * writes_per_thread);
    }

    #[test]
    fn durable_jsonl_append_repairs_torn_tail_before_writing_next_record() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("journal.jsonl")).unwrap();
        std::fs::write(path.as_std_path(), b"{\"kind\":\"complete\"}\n{\"kind\":").unwrap();

        append_jsonl_durable(&path, &serde_json::json!({ "kind": "next" })).unwrap();

        let records = std::fs::read_to_string(path.as_std_path())
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["kind"], "complete");
        assert_eq!(records[1]["kind"], "next");
    }

    #[test]
    fn durable_jsonl_append_preserves_complete_tail_without_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("journal.jsonl")).unwrap();
        std::fs::write(path.as_std_path(), b"{\"kind\":\"complete\"}").unwrap();

        append_jsonl_durable(&path, &serde_json::json!({ "kind": "next" })).unwrap();

        let records = std::fs::read_to_string(path.as_std_path())
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["kind"], "complete");
        assert_eq!(records[1]["kind"], "next");
    }

    #[test]
    fn roll_jsonl_trims_unicode_file_without_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("unicode.jsonl")).unwrap();
        let first = r#"{"content":"本次任务包含中文内容一"}"#;
        let second = r#"{"content":"本次任务包含中文内容二"}"#;
        std::fs::write(path.as_std_path(), format!("{first}\n{second}")).unwrap();

        roll_jsonl(&path, 1, second.len() as u64).unwrap();

        let after = std::fs::read_to_string(path.as_std_path()).unwrap();
        assert_eq!(after, second);
    }

    #[test]
    fn doctor_attempt_directories_are_isolated_by_agent_id() {
        let root = tempfile::tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
        let paths = GoldBandPaths::new(repo_root);
        let cursor = "cursor".parse::<ManagedAgentId>().unwrap();
        let opencode = "opencode".parse::<ManagedAgentId>().unwrap();

        assert_ne!(
            paths.doctor_acp_dir(&cursor),
            paths.doctor_acp_dir(&opencode)
        );
        assert!(paths.doctor_acp_dir(&cursor).ends_with("doctor/acp/cursor"));
        assert!(
            paths
                .doctor_acp_dir(&opencode)
                .ends_with("doctor/acp/opencode")
        );
    }

    #[cfg(unix)]
    #[test]
    fn root_workspace_uses_stable_non_empty_project_id() {
        let temp = tempfile::tempdir().unwrap();
        let path_config = StoragePathConfig {
            app_key: "gold-band-test-root",
            config_dir_name: ".gold-band-test-root",
            home_env_var: "GOLD_BAND_TEST_ROOT_HOME",
        };
        unsafe { std::env::set_var(path_config.home_env_var, temp.path().to_str().unwrap()) };
        let paths = GoldBandPaths::new_with_path_config(Utf8PathBuf::from("/"), path_config);
        unsafe { std::env::remove_var(path_config.home_env_var) };

        assert!(paths.project_id.starts_with("root--"));
        assert_eq!(paths.project_id.len(), 14);
        assert!(
            paths
                .runtime_log_file()
                .to_string()
                .replace('\\', "/")
                .ends_with("/.gold-band-test-root/logs/runtime.log")
        );
    }

    #[test]
    fn scheduler_db_path_reuses_global_core_state() {
        let temp = tempfile::tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        std::fs::create_dir_all(repo_root.as_std_path()).unwrap();
        let paths = GoldBandPaths::new(repo_root);

        assert_ne!(paths.scheduler_db_path(), paths.sqlite_db_path());
        assert_eq!(paths.scheduler_db_path(), paths.core_db_path());
        assert_eq!(paths.scheduler_db_path().file_name(), Some("core.db"));
    }
}
