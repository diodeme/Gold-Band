use crate::domain::VERSION;
use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;

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
        let repo_root = repo_root.into();
        let normalized_repo_root = normalized_repo_root(&repo_root);
        let project_id = project_id(&repo_root);
        let repo_gold_band_root = repo_root.join(".gold-band");
        let user_gold_band_root = user_gold_band_root(&repo_root);
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

    pub fn user_config_file(&self) -> Utf8PathBuf {
        self.user_gold_band_dir().join("config.json")
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

    pub fn logs_dir(&self) -> Utf8PathBuf {
        self.runtime_root.join("logs")
    }

    pub fn runtime_log_file(&self) -> Utf8PathBuf {
        self.logs_dir().join("runtime.log")
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

fn user_gold_band_root(repo_root: &Utf8Path) -> Utf8PathBuf {
    if let Some(home) = std::env::var("GOLD_BAND_HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return Utf8PathBuf::from(home).join(".gold-band");
    }

    if is_under_system_temp(repo_root) {
        return repo_root.join("gold-band-home/.gold-band");
    }

    let home = std::env::var("HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("USERPROFILE")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| ".".to_string());
    Utf8PathBuf::from(home).join(".gold-band")
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

    #[test]
    fn project_paths_split_repo_config_and_user_runtime() {
        let paths = GoldBandPaths::new(Utf8PathBuf::from("D:/Projects/Example App"));

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
        let first = GoldBandPaths::new(Utf8PathBuf::from("D:/Projects/Gold-Band"));
        let second = GoldBandPaths::new(Utf8PathBuf::from("D:/Projects/Gold-Band"));

        assert_eq!(first.project_id, second.project_id);
    }

    #[test]
    fn recognizes_system_temp_paths() {
        let repo_root =
            Utf8PathBuf::from_path_buf(std::env::temp_dir().join("gold-band-test-repo")).unwrap();

        assert!(is_under_system_temp(&repo_root));
    }
}
