use crate::domain::VERSION;
use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{OnceLock, RwLock};

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
        if let Some(home) = std::env::var(active_storage_path_config().home_env_var)
            .ok()
            .filter(|value| !value.trim().is_empty())
        {
            return Utf8PathBuf::from(home)
                .join(active_storage_path_config().config_dir_name)
                .join("state.json");
        }
        let dir = dirs::data_local_dir()
            .and_then(|p| Utf8PathBuf::from_path_buf(p).ok())
            .unwrap_or_else(|| Utf8PathBuf::from("."));
        dir.join(active_storage_path_config().app_key)
            .join("state.json")
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
        self.runtime_root.join("logs")
    }

    pub fn runtime_log_file(&self) -> Utf8PathBuf {
        self.logs_dir().join("runtime.log")
    }

    pub fn authoring_dir(&self) -> Utf8PathBuf {
        self.runtime_root.join("authoring")
    }

    pub fn workflow_templates_file(&self) -> Utf8PathBuf {
        self.authoring_dir().join("workflows.json")
    }

    pub fn agent_diagnostics_file(&self) -> Utf8PathBuf {
        self.runtime_root.join("desktop/agent-diagnostics.json")
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
    id.trim_matches('-').to_string()
}

pub fn ensure_parent_dir(path: &Utf8Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub fn write_json<T: Serialize>(path: &Utf8Path, value: &T) -> Result<()> {
    ensure_parent_dir(path)?;
    let content = serde_json::to_string_pretty(value)?;
    std::fs::write(path, content)?;
    Ok(())
}

pub fn read_json<T: serde::de::DeserializeOwned>(path: &Utf8Path) -> Result<T> {
    let content = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

pub fn append_jsonl<T: Serialize>(path: &Utf8Path, value: &T) -> Result<()> {
    ensure_parent_dir(path)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path.as_std_path())?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
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
                .contains("/.gold-band/projects/D--Projects-Example-App/")
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
        assert!(settings.to_string().replace('\\', "/").ends_with("/.gold-band/settings.json"));
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
        assert!(normalized.ends_with("state.json"), "expected state.json path, got: {normalized}");
        assert!(normalized.contains("gold-band"), "expected gold-band in path, got: {normalized}");
    }

    #[test]
    fn state_file_under_home_env_when_set() {
        let temp = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("GOLD_BAND_HOME", temp.path().to_str().unwrap()) };
        let paths = GoldBandPaths::new_with_path_config(
            Utf8PathBuf::from("D:/Projects/Example App"),
            DEFAULT_STORAGE_PATH_CONFIG,
        );
        let state = paths.user_state_file();
        assert!(state.to_string().replace('\\', "/").ends_with("/.gold-band/state.json"));
        assert!(state.to_string().replace('\\', "/").contains("gold-band"));
    }
}
