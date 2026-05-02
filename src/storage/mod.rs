use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;

#[derive(Debug, Clone)]
pub struct GoldBandPaths {
    pub repo_root: Utf8PathBuf,
    pub runtime_root: Utf8PathBuf,
}

impl GoldBandPaths {
    pub fn new(repo_root: impl Into<Utf8PathBuf>) -> Self {
        let repo_root = repo_root.into();
        let runtime_root = repo_root.join(".gold-band");
        Self { repo_root, runtime_root }
    }

    pub fn repo_presets_dir(&self) -> Utf8PathBuf {
        self.runtime_root.join("presets")
    }

    pub fn repo_profiles_dir(&self) -> Utf8PathBuf {
        self.repo_presets_dir().join("profiles")
    }

    pub fn repo_profile_file(&self, profile_name: &str) -> Utf8PathBuf {
        self.repo_profiles_dir().join(format!("{profile_name}.md"))
    }

    pub fn user_gold_band_dir(&self) -> Utf8PathBuf {
        let home = std::env::var("HOME")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| std::env::var("USERPROFILE").ok().filter(|value| !value.trim().is_empty()))
            .unwrap_or_else(|| ".".to_string());
        Utf8PathBuf::from(home).join(".gold-band")
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
        self.task_dir(task_id).join("authoring/workflow.resolved.json")
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

    pub fn node_dir(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str) -> Utf8PathBuf {
        self.round_dir(task_id, run_id, round_id).join("nodes").join(node_id)
    }

    pub fn attempt_dir(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str) -> Utf8PathBuf {
        self.node_dir(task_id, run_id, round_id, node_id).join(attempt_id)
    }

    pub fn node_file(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str) -> Utf8PathBuf {
        self.attempt_dir(task_id, run_id, round_id, node_id, attempt_id).join("node.json")
    }

    pub fn worker_ref_file(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str) -> Utf8PathBuf {
        self.attempt_dir(task_id, run_id, round_id, node_id, attempt_id).join("worker-ref.json")
    }

    pub fn artifacts_dir(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str) -> Utf8PathBuf {
        self.attempt_dir(task_id, run_id, round_id, node_id, attempt_id).join("artifacts")
    }

    pub fn artifact_file(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str, name: &str) -> Utf8PathBuf {
        self.artifacts_dir(task_id, run_id, round_id, node_id, attempt_id)
            .join(format!("{name}.json"))
    }

    pub fn attachments_dir(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str) -> Utf8PathBuf {
        self.attempt_dir(task_id, run_id, round_id, node_id, attempt_id).join("attachments")
    }

    pub fn progress_events_file(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str) -> Utf8PathBuf {
        self.attempt_dir(task_id, run_id, round_id, node_id, attempt_id).join("progress.events.jsonl")
    }

    pub fn raw_stream_file(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str) -> Utf8PathBuf {
        self.attempt_dir(task_id, run_id, round_id, node_id, attempt_id).join("raw.stream.jsonl")
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
    let mut file = OpenOptions::new().create(true).append(true).open(path.as_std_path())?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    Ok(())
}
