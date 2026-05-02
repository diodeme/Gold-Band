mod ids;
mod node_executor;
mod orchestrator;
mod profile_resolver;
mod state_access;
mod state_factory;
mod transition_context;

use crate::artifacts::{validate_exec_plan, validate_exec_result, validate_verify_result, ExecPlanArtifact, ExecResultArtifact, VerifyResultArtifact};
use crate::config::{RuntimeConfig, UserConfig, ConsoleThemeName};
use crate::control::{decide_next_step, ControlDecision};
use crate::domain::{PauseReason, RunStatus};
use crate::domain::{NodeOutcome, RunOutcome};
use crate::dsl::{validate_workflow, EdgeOutcome, WorkflowDsl};
use crate::provider::{provider_capabilities, provider_from_id, DoctorResult, ProviderAdapter, ProviderCapabilities, ProviderInfo};
use crate::runtime::{
    validate_node_state, validate_round_state, validate_run_state, validate_task_state, validate_worker_ref_state, NodeState,
    RoundState, RunState, TaskState, WorkerRefState,
};
use crate::storage::{read_json, write_json, GoldBandPaths};
use serde::de::DeserializeOwned;
use anyhow::{anyhow, bail, Result};
use camino::{Utf8Path, Utf8PathBuf};
use std::fs;
use std::io::{Read, Seek, SeekFrom};

use self::ids::now_rfc3339_like;
use self::orchestrator::{run_continue as orchestrator_run_continue, run_retry as orchestrator_run_retry, run_start as orchestrator_run_start};
use self::profile_resolver::resolve_workflow_profiles;

fn tail_text(text: &str, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }
    let normalized = text.strip_suffix('\n').unwrap_or(text);
    let lines = normalized.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(limit);
    lines[start..].join("\n")
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskSummary {
    pub task: TaskState,
    pub workflow_exists: bool,
    pub workflow_valid: bool,
    pub workflow_error: Option<String>,
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
    provider: Box<dyn ProviderAdapter>,
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

    pub fn provider_info(&self) -> ProviderInfo {
        self.provider.describe_provider()
    }

    pub fn provider_doctor(&self) -> DoctorResult {
        self.provider.doctor()
    }

    pub fn provider_capabilities(&self) -> Result<ProviderCapabilities> {
        provider_capabilities(&self.config.default_provider)
    }

    pub fn with_config(repo_root: Utf8PathBuf, config: RuntimeConfig) -> Self {
        let provider = provider_from_id(&config.default_provider).expect("configured default provider must be supported");
        Self {
            paths: GoldBandPaths::new(repo_root),
            config,
            provider,
        }
    }

    pub fn with_provider(repo_root: Utf8PathBuf, provider: Box<dyn ProviderAdapter>) -> Self {
        Self::with_provider_config(repo_root, RuntimeConfig::default(), provider)
    }

    pub fn with_provider_config(repo_root: Utf8PathBuf, config: RuntimeConfig, provider: Box<dyn ProviderAdapter>) -> Self {
        Self {
            paths: GoldBandPaths::new(repo_root),
            config,
            provider,
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
        tasks.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(tasks)
    }

    pub fn task_summaries(&self) -> Result<Vec<TaskSummary>> {
        let mut summaries = self
            .task_list()?
            .into_iter()
            .map(|task| self.task_summary(&task.id))
            .collect::<Result<Vec<_>>>()?;
        summaries.sort_by(|left, right| left.task.id.cmp(&right.task.id));
        Ok(summaries)
    }

    pub fn task_summary(&self, task_id: &str) -> Result<TaskSummary> {
        let task = self.task_show(task_id)?;
        let workflow_exists = self.paths.workflow_file(task_id).exists();
        let workflow_error = self.workflow_validation_error(task_id)?;
        let workflow_valid = workflow_exists && workflow_error.is_none();
        let latest_run = self.latest_run(task_id)?;
        let resumable_run_id = self.find_resumable_run_id(task_id)?;
        let suggested_run_id = self.find_active_or_resumable_run_id(task_id)?;
        Ok(TaskSummary {
            task,
            workflow_exists,
            workflow_valid,
            workflow_error,
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
        self.read_json_dir_sorted_by_file(&self.paths.run_dir(task_id, run_id).join("rounds"), "round.json")
    }

    pub fn node_list(&self, task_id: &str, run_id: &str, round_id: &str) -> Result<Vec<NodeState>> {
        let nodes_dir = self.paths.round_dir(task_id, run_id, round_id).join("nodes");
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
            if let Some(first_attempt_dir) = attempt_dirs.into_iter().find(|path| path.is_dir()) {
                let node_file = first_attempt_dir.join("node.json");
                if node_file.exists() {
                    let utf8 = Utf8PathBuf::from_path_buf(node_file).map_err(|_| anyhow!("path is not valid UTF-8"))?;
                    let node: NodeState = read_json(&utf8)?;
                    validate_node_state(&node)?;
                    nodes.push(node);
                }
            }
        }
        Ok(nodes)
    }

    pub fn attempt_list(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str) -> Result<Vec<NodeState>> {
        let mut attempts: Vec<NodeState> = self.read_json_dir_sorted_by_file(&self.paths.node_dir(task_id, run_id, round_id, node_id), "node.json")?;
        for attempt in &attempts {
            validate_node_state(attempt)?;
        }
        attempts.sort_by(|left, right| left.attempt_id.cmp(&right.attempt_id));
        Ok(attempts)
    }

    pub fn attachment_list(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str) -> Result<Vec<String>> {
        let dir = self.paths.attachments_dir(task_id, run_id, round_id, node_id, attempt_id);
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

    pub fn attachment_show(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str, name: &str) -> Result<String> {
        let path = self.paths.attachments_dir(task_id, run_id, round_id, node_id, attempt_id).join(name);
        self.artifact_show_path(path.as_path())
    }

    pub fn run_progress(&self, task_id: &str, run_id: &str) -> Result<Option<serde_json::Value>> {
        self.read_optional_json_value(&self.paths.run_progress_file(task_id, run_id))
    }

    pub fn run_events(&self, task_id: &str, run_id: &str) -> Result<Option<String>> {
        self.read_optional_text(&self.paths.run_events_file(task_id, run_id))
    }

    pub fn attempt_progress_events(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str) -> Result<Option<String>> {
        self.read_optional_text(&self.paths.progress_events_file(task_id, run_id, round_id, node_id, attempt_id))
    }

    pub fn attempt_raw_stream(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str) -> Result<Option<String>> {
        self.read_optional_text(&self.paths.raw_stream_file(task_id, run_id, round_id, node_id, attempt_id))
    }

    pub fn workflow_snapshot_show(&self, task_id: &str, run_id: &str) -> Result<Option<String>> {
        self.read_optional_text(&self.paths.workflow_snapshot_file(task_id, run_id))
    }

    pub fn worker_ref_show(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str) -> Result<Option<String>> {
        let path = self.paths.worker_ref_file(task_id, run_id, round_id, node_id, attempt_id);
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
            newline_count += buffer[..read_len].iter().filter(|&&byte| byte == b'\n').count();
            chunks.push(buffer[..read_len].to_vec());
        }

        chunks.reverse();
        let text = String::from_utf8(chunks.concat())?;
        let normalized = text.strip_suffix('\n').unwrap_or(&text);
        let lines = normalized.lines().collect::<Vec<_>>();
        let start = lines.len().saturating_sub(limit);
        Ok(Some(lines[start..].join("\n")))
    }

    pub fn attempt_log(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str, source: LogSource) -> Result<Option<String>> {
        match source {
            LogSource::ProgressEvents => self.attempt_progress_events(task_id, run_id, round_id, node_id, attempt_id),
            LogSource::RawStream => self.attempt_raw_stream(task_id, run_id, round_id, node_id, attempt_id),
        }
    }

    pub fn attempt_log_exists(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str, source: LogSource) -> bool {
        match source {
            LogSource::ProgressEvents => self.paths.progress_events_file(task_id, run_id, round_id, node_id, attempt_id).exists(),
            LogSource::RawStream => self.paths.raw_stream_file(task_id, run_id, round_id, node_id, attempt_id).exists(),
        }
    }

    pub fn attempt_log_tail(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str, source: LogSource, limit: usize) -> Result<Option<String>> {
        Ok(self
            .attempt_log(task_id, run_id, round_id, node_id, attempt_id, source)?
            .map(|content| tail_text(&content, limit)))
    }

    pub fn provider_output(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str) -> Result<Option<String>> {
        if let Some(progress) = self.attempt_log(task_id, run_id, round_id, node_id, attempt_id, LogSource::ProgressEvents)? {
            return Ok(Some(progress));
        }
        self.attempt_log(task_id, run_id, round_id, node_id, attempt_id, LogSource::RawStream)
    }

    pub fn current_attempt_selection(&self, task_id: &str, run_id: &str) -> Result<Option<(String, String, String)>> {
        let run = self.run_status(task_id, run_id)?;
        match (run.current_round, run.current_node, run.current_attempt) {
            (Some(round_id), Some(node_id), Some(attempt_id)) => Ok(Some((round_id, node_id, attempt_id))),
            _ => Ok(None),
        }
    }

    pub fn node_runtime_summary(&self, task_id: &str, run_id: &str, round_id: &str, workflow: &WorkflowDsl, node_id: &str) -> Result<NodeRuntimeSummary> {
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

    pub fn artifact_show(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str, name: &str) -> Result<String> {
        let path = self.paths.artifact_file(task_id, run_id, round_id, node_id, attempt_id, name);
        self.artifact_show_path(&path)
    }

    pub fn artifact_list(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str) -> Result<Vec<String>> {
        let dir = self.paths.artifacts_dir(task_id, run_id, round_id, node_id, attempt_id);
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

    pub fn run_status(&self, task_id: &str, run_id: &str) -> Result<RunState> {
        let run: RunState = read_json(&self.paths.run_file(task_id, run_id))?;
        validate_run_state(&run)?;
        Ok(run)
    }

    pub fn run_kill(&self, task_id: &str, run_id: &str) -> Result<RunState> {
        let mut run = self.run_status(task_id, run_id)?;
        run.status = RunStatus::Completed;
        run.outcome = Some(RunOutcome::Killed);
        run.pause_reason = None;
        run.updated_at = now_rfc3339_like();
        validate_run_state(&run)?;
        write_json(&self.paths.run_file(task_id, run_id), &run)?;

        if let Some(round_id) = &run.current_round {
            let mut round: RoundState = read_json(&self.paths.round_file(task_id, run_id, round_id))?;
            round.status = RunStatus::Completed;
            round.outcome = Some(RunOutcome::Killed);
            validate_round_state(&round)?;
            write_json(&self.paths.round_file(task_id, run_id, round_id), &round)?;

            if let (Some(node_id), Some(attempt_id)) = (&run.current_node, &run.current_attempt) {
                let node_path = self.paths.node_file(task_id, run_id, round_id, node_id, attempt_id);
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

    pub fn run_open_session(&self, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str) -> Result<String> {
        let worker_ref: WorkerRefState = read_json(&self.paths.worker_ref_file(task_id, run_id, round_id, node_id, attempt_id))?;
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
        self.provider
            .build_continue_command(&session_ref)?
            .ok_or_else(|| anyhow!("provider did not return an open-session command"))
    }

    pub fn run_continue(&self, task_id: &str, run_id: &str) -> Result<RunState> {
        orchestrator_run_continue(self, task_id, run_id)
    }

    pub fn run_retry(&self, task_id: &str, run_id: &str) -> Result<RunState> {
        orchestrator_run_retry(self, task_id, run_id)
    }

    pub fn run_start(&self, task_id: &str, workflow_override: Option<&Utf8Path>) -> Result<RunState> {
        orchestrator_run_start(self, task_id, workflow_override)
    }

    pub fn decide(&self, workflow: WorkflowDsl, run: &RunState, round: &RoundState, node: &NodeState) -> Result<ControlDecision> {
        let validated = validate_workflow(workflow)?;
        Ok(decide_next_step(&validated, run, round, node))
    }

    pub fn validate_artifact_samples(
        &self,
        exec_plan: &ExecPlanArtifact,
        exec_result: &ExecResultArtifact,
        verify_result: &VerifyResultArtifact,
    ) -> Result<()> {
        validate_exec_plan(exec_plan)?;
        validate_exec_result(exec_result)?;
        validate_verify_result(verify_result)?;
        Ok(())
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
                    let utf8 = Utf8PathBuf::from_path_buf(file).map_err(|_| anyhow!("path is not valid UTF-8"))?;
                    items.push(read_json(&utf8)?);
                } else if run_file.exists() {
                    let utf8 = Utf8PathBuf::from_path_buf(run_file).map_err(|_| anyhow!("path is not valid UTF-8"))?;
                    items.push(read_json(&utf8)?);
                }
            }
        }
        Ok(items)
    }

    fn read_json_dir_sorted_by_file<T: DeserializeOwned>(&self, dir: &Utf8Path, file_name: &str) -> Result<Vec<T>> {
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
                    let utf8 = Utf8PathBuf::from_path_buf(file).map_err(|_| anyhow!("path is not valid UTF-8"))?;
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

    fn workflow_validation_error(&self, task_id: &str) -> Result<Option<String>> {
        let path = self.paths.workflow_file(task_id);
        if !path.exists() {
            return Ok(Some("missing authoring/workflow.json".to_string()));
        }

        let workflow: WorkflowDsl = match read_json(&path) {
            Ok(workflow) => workflow,
            Err(err) => return Ok(Some(err.to_string())),
        };

        let validated = match validate_workflow(workflow.clone()) {
            Ok(validated) => validated,
            Err(err) => return Ok(Some(err.to_string())),
        };

        match resolve_workflow_profiles(&self.paths, &validated.raw) {
            Ok(_) => Ok(None),
            Err(err) => Ok(Some(err.to_string())),
        }
    }

    pub fn find_active_or_resumable_run_id(&self, task_id: &str) -> Result<Option<String>> {
        let runs = self.run_list(task_id)?;
        if let Some(run) = runs
            .iter()
            .rev()
            .find(|run| run.status == RunStatus::Running && self.paths.run_progress_file(task_id, &run.id).exists())
        {
            return Ok(Some(run.id.clone()));
        }
        if let Some(run) = runs.iter().rev().find(|run| run.status == RunStatus::Running) {
            return Ok(Some(run.id.clone()));
        }
        if let Some(run) = runs
            .iter()
            .rev()
            .find(|run| run.status == RunStatus::Paused && matches!(run.pause_reason, Some(PauseReason::ProcessInterrupted)))
        {
            return Ok(Some(run.id.clone()));
        }
        Ok(runs.into_iter().last().map(|run| run.id))
    }

    fn find_resumable_run_id(&self, task_id: &str) -> Result<Option<String>> {
        for run in self.run_list(task_id)?.into_iter().rev() {
            if run.status == RunStatus::Paused && matches!(run.pause_reason, Some(PauseReason::ProcessInterrupted)) {
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

    #[test]
    fn runtime_log_tail_reads_only_last_requested_lines() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        std::fs::create_dir_all(repo_root.join(".gold-band/logs").as_std_path()).unwrap();
        std::fs::write(
            repo_root.join(".gold-band/logs/runtime.log").as_std_path(),
            (1..=1000).map(|n| format!("line-{n}")).collect::<Vec<_>>().join("\n"),
        )
        .unwrap();

        let app = App::new(repo_root);
        let tail = app.runtime_log_tail_show(3).unwrap().unwrap();
        assert_eq!(tail, "line-998\nline-999\nline-1000");
    }

    #[test]
    fn user_console_theme_is_persisted() {
        static HOME_ENV_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        let _home_guard = HOME_ENV_LOCK.get_or_init(|| std::sync::Mutex::new(())).lock().unwrap();
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let home_dir = repo_root.join("fake-home");
        std::fs::create_dir_all(home_dir.as_std_path()).unwrap();
        unsafe { std::env::set_var("HOME", home_dir.as_str()) };

        let app = App::new(repo_root.clone());
        app.set_user_console_theme(ConsoleThemeName::Nord).unwrap();

        let config = app.load_user_config().unwrap();
        assert_eq!(config.console_theme, Some(ConsoleThemeName::Nord));
    }
}
