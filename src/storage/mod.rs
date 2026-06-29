pub mod sqlite;

use crate::domain::VERSION;
use anyhow::{Result, anyhow};
use atomic_write_file::AtomicWriteFile;
use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
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
static JSONL_FILE_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectManifest {
    pub version: String,
    pub project_id: String,
    pub repo_root: String,
    pub normalized_repo_root: String,
}

impl GoldBandPaths {
    pub fn new(repo_root: impl Into<Utf8PathBuf>) -> Self {
        Self::new_with_path_config(repo_root, active_storage_path_config())
    }

    pub fn new_with_path_config(
        repo_root: impl Into<Utf8PathBuf>,
        path_config: StoragePathConfig,
    ) -> Self {
        let repo_root = repo_root.into();
        let normalized_repo_root = normalized_repo_root(&repo_root);
        let project_id = project_id(&repo_root);
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

    pub fn write_project_manifest(&self) -> Result<()> {
        write_json(
            &self.project_manifest_file(),
            &ProjectManifest {
                version: VERSION.to_string(),
                project_id: self.project_id.clone(),
                repo_root: self.repo_root.to_string(),
                normalized_repo_root: self.normalized_repo_root.clone(),
            },
        )
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

    pub fn project_context_dir(&self) -> Utf8PathBuf {
        self.runtime_root.join("context")
    }

    pub fn project_context_profiles_dir(&self) -> Utf8PathBuf {
        self.project_context_dir().join("profiles")
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

    pub fn doctor_dir(&self) -> Utf8PathBuf {
        self.user_gold_band_root.join("doctor")
    }

    pub fn doctor_acp_dir(&self) -> Utf8PathBuf {
        self.doctor_dir().join("acp")
    }

    pub fn doctor_acp_provider_pid_file(&self) -> Utf8PathBuf {
        self.doctor_acp_dir().join("provider.pid")
    }

    pub fn sqlite_db_path(&self) -> Utf8PathBuf {
        self.user_gold_band_root.join("gold-band.db")
    }

    // ── SKILL paths ──

    pub fn global_skills_dir() -> Utf8PathBuf {
        let home = dirs::home_dir()
            .and_then(|p| Utf8PathBuf::from_path_buf(p).ok())
            .unwrap_or_else(|| Utf8PathBuf::from("."));
        home.join(crate::config::AGENTS_DIR_NAME)
            .join(crate::config::SKILLS_DIR_NAME)
    }

    pub fn project_skills_dir(&self) -> Utf8PathBuf {
        self.repo_root
            .join(crate::config::AGENTS_DIR_NAME)
            .join(crate::config::SKILLS_DIR_NAME)
    }

    pub fn projects_dir(&self) -> Utf8PathBuf {
        self.user_gold_band_root.join("projects")
    }

    pub fn tasks_dir(&self) -> Utf8PathBuf {
        self.runtime_root.join("tasks")
    }

    pub fn task_dir(&self, task_id: &str) -> Utf8PathBuf {
        self.tasks_dir().join(task_id)
    }

    pub fn task_file(&self, task_id: &str) -> Utf8PathBuf {
        self.task_dir(task_id).join("task.json")
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
    let path = normalized_repo_root(path);
    std::env::temp_dir()
        .to_str()
        .map(Utf8Path::new)
        .map(normalized_repo_root)
        .is_some_and(|temp| path.starts_with(&temp))
}

fn normalized_repo_root(repo_root: &Utf8Path) -> String {
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

fn project_id(repo_root: &Utf8Path) -> String {
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

pub fn ensure_parent_dir(path: &Utf8Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub fn write_json<T: Serialize>(path: &Utf8Path, value: &T) -> Result<()> {
    ensure_parent_dir(path)?;
    let content = serde_json::to_vec_pretty(value)?;
    let mut file = AtomicWriteFile::open(path.as_std_path())?;
    file.write_all(&content)?;
    file.commit()?;
    Ok(())
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
    let lock = jsonl_file_lock_for(path)?;
    let _guard = lock
        .lock()
        .map_err(|_| anyhow!("jsonl file lock poisoned"))?;
    operation()
}

fn jsonl_file_lock_for(path: &Utf8Path) -> Result<Arc<Mutex<()>>> {
    let key = jsonl_file_lock_key(path);
    let mut locks = JSONL_FILE_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| anyhow!("jsonl file lock registry poisoned"))?;
    Ok(locks
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone())
}

fn jsonl_file_lock_key(path: &Utf8Path) -> String {
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

pub fn append_jsonl_unlocked<T: Serialize>(path: &Utf8Path, value: &T) -> Result<()> {
    let line = serde_json::to_vec(value)?;
    append_jsonl_line_unlocked(path, &line)
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
                .contains("/.gold-band/projects/D--Projects-Example-App/")
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
                .contains("/.maling/projects/D--Projects-Example-App/")
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

        assert_eq!(paths.project_id, "root");
        assert!(
            paths
                .runtime_log_file()
                .to_string()
                .replace('\\', "/")
                .ends_with("/.gold-band-test-root/logs/runtime.log")
        );
    }
}
