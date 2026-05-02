pub use crate::domain::SessionRef;
use crate::domain::{InvocationKind, SessionMode, DEFAULT_PROVIDER};
use crate::observability::append_raw_stream_best_effort;
use crate::storage::{append_jsonl, ensure_parent_dir};
use anyhow::{anyhow, bail, ensure, Result};
use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::thread;
use tracing::{debug, warn};

const WINDOWS_GIT_BASH_HINT: &str = "Claude Code on Windows requires Git Bash. Install Git for Windows or set CLAUDE_CODE_GIT_BASH_PATH to your bash.exe path.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub provider_id: String,
    pub display_name: String,
    pub capabilities: ProviderCapabilities,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub supports_open_session: bool,
    pub supports_continue_session: bool,
    pub supports_raw_stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorResult {
    pub available: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerInvocation {
    pub invocation_kind: InvocationKind,
    pub profile: Option<String>,
    pub requirement_path: Option<Utf8PathBuf>,
    pub requirement_text: Option<String>,
    pub workspace_dir: Utf8PathBuf,
    pub attempt_dir: Utf8PathBuf,
    pub primary_artifact: Option<String>,
    pub task_instruction: Option<String>,
    pub session_mode: SessionMode,
    pub continue_ref: Option<serde_json::Value>,
    pub stream_mode: StreamMode,
    #[serde(default)]
    pub log_prompts: bool,
    #[serde(default)]
    pub log_provider_command: bool,
    pub feedback_summary: Option<String>,
    pub verify_result_path: Option<Utf8PathBuf>,
    pub attachments_dir: Option<Utf8PathBuf>,
    pub cold_artifacts: Vec<ColdFileRef>,
    pub cold_attachments: Vec<ColdFileRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColdFileRef {
    pub name: Option<String>,
    pub path: Utf8PathBuf,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StreamMode {
    None,
    Raw,
    StreamJson,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRunResult {
    pub status: ProviderRunStatus,
    pub exit_code: Option<i32>,
    pub result_payload: Option<ProviderResultPayload>,
    pub worker_ref_seed: Option<SessionRef>,
    pub stream_path: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderRunStatus {
    Success,
    Failure,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResultPayload {
    pub primary_artifact: Option<PrimaryArtifactPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrimaryArtifactPayload {
    pub name: String,
    pub content: String,
}


#[derive(Debug, Clone)]
pub struct PromptBundle {
    pub system_prompt: String,
    pub user_prompt: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderInputProgressEvent<'a> {
    version: &'static str,
    #[serde(rename = "type")]
    event_type: &'static str,
    invocation_kind: &'a InvocationKind,
    session_mode: &'a SessionMode,
    stream_mode: &'a StreamMode,
    profile: Option<&'a str>,
    primary_artifact: Option<&'a str>,
    task_instruction: Option<&'a str>,
    requirement_path: Option<&'a str>,
    requirement_text: Option<&'a str>,
    verify_result_path: Option<&'a str>,
    attachments_dir: Option<&'a str>,
    cold_artifacts: &'a [ColdFileRef],
    cold_attachments: &'a [ColdFileRef],
    system_prompt: &'a str,
    user_prompt: &'a str,
}

pub trait ProviderAdapter: Send + Sync {
    fn describe_provider(&self) -> ProviderInfo;
    fn doctor(&self) -> DoctorResult;
    fn run_worker(&self, req: WorkerInvocation) -> Result<ProviderRunResult>;
    fn open_session(&self, worker_ref: &SessionRef) -> Result<()>;
    fn build_continue_command(&self, worker_ref: &SessionRef) -> Result<Option<String>>;
}

pub struct ClaudeCodeProvider;

impl ProviderAdapter for ClaudeCodeProvider {
    fn describe_provider(&self) -> ProviderInfo {
        ProviderInfo {
            provider_id: "claude-code".to_string(),
            display_name: "Claude Code".to_string(),
            capabilities: ProviderCapabilities {
                supports_open_session: true,
                supports_continue_session: true,
                supports_raw_stream: true,
            },
            is_default: true,
        }
    }

    fn doctor(&self) -> DoctorResult {
        let result = Command::new("claude").arg("--version").output();
        match result {
            Ok(output) if output.status.success() => DoctorResult {
                available: true,
                reason: None,
            },
            Ok(output) => DoctorResult {
                available: false,
                reason: Some(format!("claude --version failed with status {:?}", output.status.code())),
            },
            Err(err) => DoctorResult {
                available: false,
                reason: Some(err.to_string()),
            },
        }
    }

    fn run_worker(&self, req: WorkerInvocation) -> Result<ProviderRunResult> {
        let prompt = render_prompt_bundle(&req)?;
        let mut command = Command::new("claude");
        debug!(invocation_kind = ?req.invocation_kind, attempt_dir = %req.attempt_dir, session_mode = ?req.session_mode, stream_mode = ?req.stream_mode, "starting claude provider invocation");
        command.current_dir(req.workspace_dir.as_std_path());
        command.arg("--bare").arg("-p");
        command.arg(format!("{}\n\n{}", prompt.system_prompt, prompt.user_prompt));

        let raw_stream_path = matches!(req.stream_mode, StreamMode::Raw | StreamMode::StreamJson)
            .then(|| req.attempt_dir.join("raw.stream.jsonl"));

        match req.stream_mode {
            StreamMode::None | StreamMode::Raw => {
                command.arg("--output-format").arg("json");
            }
            StreamMode::StreamJson => {
                command.arg("--output-format").arg("stream-json");
                command.arg("--verbose");
                command.arg("--include-partial-messages");
            }
        }

        match req.session_mode {
            SessionMode::New => {}
            SessionMode::Continue => {
                let continue_ref = req
                    .continue_ref
                    .clone()
                    .ok_or_else(|| anyhow!("sessionMode=continue requires continueRef"))?;
                let session_id = continue_ref
                    .get("sessionId")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| anyhow!("continueRef is missing sessionId"))?;
                command.arg("--resume").arg(session_id);
            }
        }

        if req.log_provider_command {
            debug!(cwd = %req.workspace_dir, argv = ?provider_command_summary(&command), "provider command summary");
        }
        log_prompt_bundle(
            &prompt,
            req.invocation_kind,
            req.profile.as_deref(),
            req.primary_artifact.as_deref(),
            req.feedback_summary.is_some(),
            req.cold_artifacts.len(),
            req.cold_attachments.len(),
            req.log_prompts,
        );
        if matches!(req.stream_mode, StreamMode::StreamJson) {
            write_provider_input_progress_event(&req, &prompt)?;
        }
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        if let Some(path) = raw_stream_path.as_ref() {
            ensure_parent_dir(path)?;
            let _ = std::fs::File::options().create(true).append(true).open(path.as_std_path())?;
            debug!(path = %path, "prepared raw stream file");
        }
        let mut child = command.spawn().map_err(|err| provider_spawn_error(err))?;

        let stdout = child.stdout.take().ok_or_else(|| anyhow!("failed to capture claude stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| anyhow!("failed to capture claude stderr"))?;
        let stderr_path = raw_stream_path.clone();
        let stderr_handle = thread::spawn(move || read_stream(stderr, "stderr", stderr_path));
        let stream_path = raw_stream_path.clone();

        let run_result = match req.stream_mode {
            StreamMode::StreamJson => read_stream_json(stdout, &req, raw_stream_path.clone())?,
            StreamMode::None | StreamMode::Raw => {
                let stdout = read_stream(stdout, "stdout", raw_stream_path.clone());
                let stdout = stdout.trim().to_string();
                let response: ClaudeJsonResponse = serde_json::from_str(&stdout)
                    .map_err(|err| anyhow!("failed to parse Claude Code JSON output: {err}; stdout={stdout}"))?;
                let worker_ref_seed = response.session_id.as_ref().map(|session_id| SessionRef {
                    provider: "claude-code".to_string(),
                    mode: req.session_mode,
                    supports_open_session: true,
                    supports_continue_session: true,
                    continue_ref: Some(serde_json::json!({ "sessionId": session_id })),
                    open_command: Some(format!("claude -c {session_id}")),
                });
                let result_payload = req.primary_artifact.as_ref().map(|primary_artifact| ProviderResultPayload {
                    primary_artifact: Some(PrimaryArtifactPayload {
                        name: primary_artifact.clone(),
                        content: response.result,
                    }),
                });
                ProviderRunResult {
                    status: ProviderRunStatus::Success,
                    exit_code: None,
                    result_payload,
                    worker_ref_seed,
                    stream_path: stream_path.clone(),
                }
            }
        };

        let status = child.wait()?;
        let exit_code = status.code();
        let stderr_text = stderr_handle.join().map_err(|_| anyhow!("stderr reader thread panicked"))?;
        debug!(?exit_code, stderr_len = stderr_text.len(), "claude provider finished");

        if !status.success() {
            warn!(?exit_code, stderr_len = stderr_text.len(), "claude provider returned failure status");
            bail!(format_provider_failure(exit_code, &stderr_text));
        }

        Ok(ProviderRunResult {
            exit_code,
            stream_path,
            ..run_result
        })
    }

    fn open_session(&self, worker_ref: &SessionRef) -> Result<()> {
        if !worker_ref.supports_open_session {
            bail!("provider does not support open-session");
        }
        Ok(())
    }

    fn build_continue_command(&self, worker_ref: &SessionRef) -> Result<Option<String>> {
        Ok(worker_ref.open_command.clone())
    }
}

#[derive(Debug, Deserialize)]
struct ClaudeJsonResponse {
    result: String,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Default)]
struct StreamAccumulator {
    session_id: Option<String>,
    final_result: Option<String>,
}

fn current_timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("{secs}Z")
}

fn provider_spawn_error(err: std::io::Error) -> anyhow::Error {
    let mut message = format!("failed to start Claude Code provider: {err}");
    if cfg!(windows) {
        let lower = message.to_ascii_lowercase();
        if lower.contains("git-bash") || lower.contains("bash.exe") || lower.contains("claude_code_git_bash_path") {
            message.push_str(". ");
            message.push_str(WINDOWS_GIT_BASH_HINT);
        }
    }
    anyhow!(message)
}

fn format_provider_failure(exit_code: Option<i32>, stderr_text: &str) -> String {
    let mut message = format!("Claude Code provider exited with status {:?}", exit_code);
    let stderr_trimmed = stderr_text.trim();
    if !stderr_trimmed.is_empty() {
        message.push_str(": ");
        message.push_str(stderr_trimmed);
    }
    if cfg!(windows) {
        let lower = stderr_trimmed.to_ascii_lowercase();
        if lower.contains("git-bash") || lower.contains("bash.exe") || lower.contains("claude_code_git_bash_path") {
            message.push_str(". ");
            message.push_str(WINDOWS_GIT_BASH_HINT);
        }
    }
    message
}

fn read_stream<R: Read>(reader: R, stream: &'static str, path: Option<Utf8PathBuf>) -> String {
    let mut collected = String::new();
    let mut reader = BufReader::new(reader);
    let mut buffer = [0_u8; 4096];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read_len) => {
                let chunk = String::from_utf8_lossy(&buffer[..read_len]);
                if let Some(path) = path.as_ref() {
                    append_raw_stream_best_effort(path, &current_timestamp(), stream, &chunk);
                }
                collected.push_str(&chunk);
            }
            Err(err) => {
                warn!(stream, error = %err, "failed reading provider stream");
                break;
            }
        }
    }
    collected
}

fn read_stream_json<R: Read>(reader: R, req: &WorkerInvocation, path: Option<Utf8PathBuf>) -> Result<ProviderRunResult> {
    let mut accumulator = StreamAccumulator::default();
    sanitize_progress_events_file(req)?;
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    while reader.read_line(&mut line)? > 0 {
        let raw_line = line.trim_end_matches(['\r', '\n']).to_string();
        if !raw_line.is_empty() {
            let timestamp = current_timestamp();
            if let Some(path) = path.as_ref() {
                append_raw_stream_best_effort(path, &timestamp, "stdout", &raw_line);
            }
            collect_stream_json_line(&raw_line, &mut accumulator);
        }
        line.clear();
    }

    let worker_ref_seed = accumulator.session_id.as_ref().map(|session_id| SessionRef {
        provider: "claude-code".to_string(),
        mode: req.session_mode,
        supports_open_session: true,
        supports_continue_session: true,
        continue_ref: Some(serde_json::json!({ "sessionId": session_id })),
        open_command: Some(format!("claude -c {session_id}")),
    });

    let result_payload = req.primary_artifact.as_ref().map(|primary_artifact| ProviderResultPayload {
        primary_artifact: Some(PrimaryArtifactPayload {
            name: primary_artifact.clone(),
            content: accumulator.final_result.clone().unwrap_or_default(),
        }),
    });

    Ok(ProviderRunResult {
        status: ProviderRunStatus::Success,
        exit_code: None,
        result_payload,
        worker_ref_seed,
        stream_path: path,
    })
}

fn write_provider_input_progress_event(req: &WorkerInvocation, prompt: &PromptBundle) -> Result<()> {
    let path = progress_events_path(req);
    ensure_parent_dir(&path)?;
    let _ = std::fs::File::options().create(true).write(true).truncate(true).open(path.as_std_path())?;
    let event = ProviderInputProgressEvent {
        version: crate::domain::VERSION,
        event_type: "provider_input",
        invocation_kind: &req.invocation_kind,
        session_mode: &req.session_mode,
        stream_mode: &req.stream_mode,
        profile: req.profile.as_deref(),
        primary_artifact: req.primary_artifact.as_deref(),
        task_instruction: req.task_instruction.as_deref(),
        requirement_path: req.requirement_path.as_ref().map(|path| path.as_str()),
        requirement_text: req.requirement_text.as_deref(),
        verify_result_path: req.verify_result_path.as_ref().map(|path| path.as_str()),
        attachments_dir: req.attachments_dir.as_ref().map(|path| path.as_str()),
        cold_artifacts: &req.cold_artifacts,
        cold_attachments: &req.cold_attachments,
        system_prompt: &prompt.system_prompt,
        user_prompt: &prompt.user_prompt,
    };
    append_jsonl(&path, &event)?;
    Ok(())
}

fn progress_events_path(req: &WorkerInvocation) -> Utf8PathBuf {
    let paths = crate::storage::GoldBandPaths::new(req.workspace_dir.clone());
    paths.progress_events_file(
        task_id_from_attempt_dir(&req.attempt_dir),
        run_id_from_attempt_dir(&req.attempt_dir),
        round_id_from_attempt_dir(&req.attempt_dir),
        node_id_from_attempt_dir(&req.attempt_dir),
        attempt_id_from_attempt_dir(&req.attempt_dir),
    )
}

fn sanitize_progress_events_file(req: &WorkerInvocation) -> Result<()> {
    let path = progress_events_path(req);
    if !path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(path.as_std_path())?;
    let first_line = content.lines().find(|line| !line.trim().is_empty()).unwrap_or_default();
    if first_line.contains("\"type\":\"provider_input\"") {
        return Ok(());
    }
    let _ = std::fs::File::options().write(true).truncate(true).open(path.as_std_path())?;
    Ok(())
}

fn collect_stream_json_line(raw_line: &str, accumulator: &mut StreamAccumulator) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw_line) else {
        return;
    };
    let raw_event_type = value
        .get("type")
        .and_then(|value| value.as_str())
        .map(str::to_string);

    if accumulator.session_id.is_none() {
        accumulator.session_id = find_string_field(&value, &["session_id", "sessionId"]);
    }

    if let Some(text) = find_string_field(&value, &["delta", "text", "result"]) {
        accumulator.final_result = Some(match accumulator.final_result.take() {
            Some(existing) if raw_event_type.as_deref() != Some("result") => format!("{existing}{text}"),
            _ => text,
        });
    }
}

fn find_string_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(found) = value.get(*key).and_then(|item| item.as_str()) {
            return Some(found.to_string());
        }
    }
    for item in value.as_array().into_iter().flatten() {
        if let Some(found) = find_string_field(item, keys) {
            return Some(found);
        }
    }
    for item in value.as_object().into_iter().flat_map(|object| object.values()) {
        if let Some(found) = find_string_field(item, keys) {
            return Some(found);
        }
    }
    None
}

fn task_id_from_attempt_dir(path: &Utf8PathBuf) -> &str {
    path.components().nth_back(7).map(|part| part.as_str()).unwrap_or("")
}

fn run_id_from_attempt_dir(path: &Utf8PathBuf) -> &str {
    path.components().nth_back(5).map(|part| part.as_str()).unwrap_or("")
}

fn round_id_from_attempt_dir(path: &Utf8PathBuf) -> &str {
    path.components().nth_back(3).map(|part| part.as_str()).unwrap_or("")
}

fn node_id_from_attempt_dir(path: &Utf8PathBuf) -> &str {
    path.components().nth_back(1).map(|part| part.as_str()).unwrap_or("")
}

fn attempt_id_from_attempt_dir(path: &Utf8PathBuf) -> &str {
    path.file_name().unwrap_or("")
}

fn render_prompt_bundle(req: &WorkerInvocation) -> Result<PromptBundle> {
    ensure!(req.requirement_path.is_some() || req.requirement_text.is_some(), "worker invocation requires requirementPath or requirementText");

    let requirement_text = match (&req.requirement_text, &req.requirement_path) {
        (Some(text), _) => text.clone(),
        (None, Some(path)) => std::fs::read_to_string(path)?,
        (None, None) => unreachable!(),
    };

    let output_contract = req.primary_artifact.as_ref().map(|primary_artifact| {
        format!(
            "- Output contract: return exactly one valid `{}` artifact as the final answer content with no extra prose.{}{}\n",
            primary_artifact,
            if primary_artifact == "exec-plan" {
                " For `exec-plan`, output valid JSON with shape `{\"version\":\"0.1\",\"commands\":[{\"id\":string,\"run\":string,\"purpose\":string,\"cwd\"?:string,\"timeoutSec\"?:number}]}` and ensure `commands` is non-empty."
            } else {
                ""
            },
            if primary_artifact == "verify-result" {
                " For `verify-result`, output valid JSON with shape `{\"version\":\"0.1\",\"status\":\"success|failure\",\"summary\":string,\"unmet_requirements\":string[],\"validation_gaps\":string[]}`. `summary` must be non-empty. If `status` is `success`, both arrays must be empty. If `status` is `failure`, at least one of the arrays must be non-empty. A malformed `verify-result` blocks the run; a valid `verify-result` with `status=\"failure\"` allows the runtime acceptance policy to continue."
            } else {
                ""
            }
        )
    }).unwrap_or_default();

    let system_prompt = format!(
        "You are running inside Gold Band runtime.\n\nCurrent location:\n- Invocation kind: {:?}\n- Attempt directory: {}\n- Workspace directory: {}\n{}{}{}{}",
        req.invocation_kind,
        req.attempt_dir,
        req.workspace_dir,
        req.profile
            .as_ref()
            .map(|profile| format!("- Profile: {profile}\n"))
            .unwrap_or_default(),
        req.primary_artifact
            .as_ref()
            .map(|artifact| format!("- Required primary artifact: {artifact}\n"))
            .unwrap_or_default(),
        req.attachments_dir
            .as_ref()
            .map(|path| format!("- Free-form attachments may only be written under: {path}\n"))
            .unwrap_or_default(),
        output_contract,
    );

    let mut user_sections = vec![format!("# Requirement\n{}", requirement_text.trim())];

    if let Some(feedback_summary) = &req.feedback_summary {
        user_sections.push(format!("# Current Feedback\n{}", feedback_summary.trim()));
    }

    if let Some(task_instruction) = &req.task_instruction {
        user_sections.push(format!("# Task\n{}", task_instruction.trim()));
    }

    if !req.cold_artifacts.is_empty() {
        let index = req
            .cold_artifacts
            .iter()
            .map(|entry| match &entry.name {
                Some(name) => format!("- {name}: {}", entry.path),
                None => format!("- {}", entry.path),
            })
            .collect::<Vec<_>>()
            .join("\n");
        user_sections.push(format!("# Cold Artifact Index\n{}", index));
    }

    if !req.cold_attachments.is_empty() {
        let index = req
            .cold_attachments
            .iter()
            .map(|entry| format!("- {}", entry.path))
            .collect::<Vec<_>>()
            .join("\n");
        user_sections.push(format!("# Cold Attachment Index\n{}", index));
    }

    Ok(PromptBundle {
        system_prompt,
        user_prompt: user_sections.join("\n\n"),
    })
}

fn provider_command_summary(command: &Command) -> Vec<String> {
    let mut argv = Vec::new();
    argv.push(command.get_program().to_string_lossy().to_string());
    let mut skip_next_prompt = false;
    for arg in command.get_args() {
        let arg = arg.to_string_lossy().to_string();
        if skip_next_prompt {
            argv.push("<prompt-redacted>".to_string());
            skip_next_prompt = false;
            continue;
        }
        if arg == "-p" {
            argv.push(arg);
            skip_next_prompt = true;
            continue;
        }
        argv.push(arg);
    }
    argv
}

fn log_prompt_bundle(
    prompt: &PromptBundle,
    invocation_kind: InvocationKind,
    profile: Option<&str>,
    primary_artifact: Option<&str>,
    has_feedback: bool,
    cold_artifacts: usize,
    cold_attachments: usize,
    log_prompts: bool,
) {
    debug!(
        invocation_kind = ?invocation_kind,
        profile = ?profile,
        primary_artifact = ?primary_artifact,
        system_prompt_len = prompt.system_prompt.len(),
        user_prompt_len = prompt.user_prompt.len(),
        has_feedback,
        cold_artifacts,
        cold_attachments,
        "provider prompt bundle summary"
    );
    if log_prompts {
        debug!(system_prompt = %prompt.system_prompt, user_prompt = %prompt.user_prompt, "provider prompt bundle content");
    }
}

pub fn provider_capabilities(provider_id: &str) -> Result<ProviderCapabilities> {
    match provider_id {
        DEFAULT_PROVIDER => Ok(ClaudeCodeProvider.describe_provider().capabilities),
        _ => bail!("unsupported provider: {provider_id}"),
    }
}

pub fn supports_continue_session(provider_id: &str) -> Result<bool> {
    Ok(provider_capabilities(provider_id)?.supports_continue_session)
}

pub fn provider_from_id(provider_id: &str) -> Result<Box<dyn ProviderAdapter>> {
    match provider_id {
        DEFAULT_PROVIDER => Ok(Box::new(ClaudeCodeProvider)),
        _ => bail!("unsupported provider: {provider_id}"),
    }
}

pub fn default_provider() -> Box<dyn ProviderAdapter> {
    provider_from_id(DEFAULT_PROVIDER).expect("default provider must be supported")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn render_prompt_bundle_includes_verify_result_output_contract() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let req = WorkerInvocation {
            invocation_kind: InvocationKind::VerifyAcceptance,
            profile: Some("verifier".to_string()),
            requirement_path: None,
            requirement_text: Some("Check whether hello-world exists".to_string()),
            workspace_dir: repo_root.clone(),
            attempt_dir: repo_root.join(".gold-band/tasks/task-001/runs/run-001/rounds/round-001/nodes/accept/attempt-001"),
            primary_artifact: Some("verify-result".to_string()),
            task_instruction: Some("Evaluate whether the requirement is satisfied based only on the provided evidence and produce a verify-result.".to_string()),
            session_mode: SessionMode::New,
            continue_ref: None,
            stream_mode: StreamMode::StreamJson,
            log_prompts: false,
            log_provider_command: false,
            feedback_summary: None,
            verify_result_path: None,
            attachments_dir: None,
            cold_artifacts: Vec::new(),
            cold_attachments: Vec::new(),
        };

        let prompt = render_prompt_bundle(&req).unwrap();
        assert!(prompt.system_prompt.contains("Required primary artifact: verify-result"));
        assert!(prompt.system_prompt.contains("unmet_requirements"));
        assert!(prompt.system_prompt.contains("validation_gaps"));
        assert!(prompt.system_prompt.contains("status=\"failure\""));
    }

    #[test]
    fn render_prompt_bundle_includes_exec_plan_output_contract() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let req = WorkerInvocation {
            invocation_kind: InvocationKind::WorkerGeneric,
            profile: Some("developer".to_string()),
            requirement_path: None,
            requirement_text: Some("Need an execution plan".to_string()),
            workspace_dir: repo_root.clone(),
            attempt_dir: repo_root.join(".gold-band/tasks/task-001/runs/run-001/rounds/round-001/nodes/dev/attempt-001"),
            primary_artifact: Some("exec-plan".to_string()),
            task_instruction: Some("Create an exec plan".to_string()),
            session_mode: SessionMode::New,
            continue_ref: None,
            stream_mode: StreamMode::StreamJson,
            log_prompts: false,
            log_provider_command: false,
            feedback_summary: None,
            verify_result_path: None,
            attachments_dir: None,
            cold_artifacts: Vec::new(),
            cold_attachments: Vec::new(),
        };

        let prompt = render_prompt_bundle(&req).unwrap();
        assert!(prompt.system_prompt.contains("Output contract"));
        assert!(prompt.system_prompt.contains("return exactly one valid `exec-plan` artifact"));
        assert!(prompt.system_prompt.contains("\"commands\""));
        assert!(prompt.system_prompt.contains("non-empty"));
    }

    #[test]
    fn write_provider_input_progress_event_records_invocation_input() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let attempt_dir = repo_root.join(".gold-band/tasks/task-001/runs/run-001/rounds/round-001/nodes/dev/attempt-001");
        let req = WorkerInvocation {
            invocation_kind: InvocationKind::WorkerGeneric,
            profile: Some("developer".to_string()),
            requirement_path: Some(repo_root.join(".gold-band/tasks/task-001/authoring/requirement.md")),
            requirement_text: None,
            workspace_dir: repo_root.clone(),
            attempt_dir,
            primary_artifact: Some("exec-plan".to_string()),
            task_instruction: Some("Create an exec plan".to_string()),
            session_mode: SessionMode::New,
            continue_ref: None,
            stream_mode: StreamMode::StreamJson,
            log_prompts: false,
            log_provider_command: false,
            feedback_summary: None,
            verify_result_path: None,
            attachments_dir: Some(repo_root.join("attachments")),
            cold_artifacts: vec![ColdFileRef {
                name: Some("exec-result".to_string()),
                path: repo_root.join("artifacts/exec-result.json"),
            }],
            cold_attachments: vec![ColdFileRef {
                name: None,
                path: repo_root.join("attachments/report.md"),
            }],
        };
        let prompt = PromptBundle {
            system_prompt: "system prompt".to_string(),
            user_prompt: "user prompt".to_string(),
        };

        write_provider_input_progress_event(&req, &prompt).unwrap();

        let progress = std::fs::read_to_string(progress_events_path(&req).as_std_path()).unwrap();
        assert!(progress.contains("\"type\":\"provider_input\""));
        assert!(progress.contains("\"systemPrompt\":\"system prompt\""));
        assert!(progress.contains("\"userPrompt\":\"user prompt\""));
        assert!(progress.contains("\"primaryArtifact\":\"exec-plan\""));
    }
}
