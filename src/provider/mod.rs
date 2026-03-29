pub use crate::domain::SessionRef;
use crate::domain::{InvocationKind, SessionMode};
use anyhow::{anyhow, bail, ensure, Result};
use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use std::process::Command;

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
        command.current_dir(req.workspace_dir.as_std_path());
        command.arg("--bare").arg("-p");
        command.arg(format!("{}\n\n{}", prompt.system_prompt, prompt.user_prompt));
        command.arg("--output-format").arg("json");

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

        let output = command.output()?;
        let exit_code = output.status.code();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

        if !output.status.success() {
            return Ok(ProviderRunResult {
                status: ProviderRunStatus::Failure,
                exit_code,
                result_payload: None,
                worker_ref_seed: None,
                stream_path: None,
            });
        }

        let response: ClaudeJsonResponse = serde_json::from_str(&stdout)
            .map_err(|err| anyhow!("failed to parse Claude Code JSON output: {err}; stdout={stdout}; stderr={stderr}"))?;

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

        Ok(ProviderRunResult {
            status: ProviderRunStatus::Success,
            exit_code,
            result_payload,
            worker_ref_seed,
            stream_path: None,
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

fn render_prompt_bundle(req: &WorkerInvocation) -> Result<PromptBundle> {
    ensure!(req.requirement_path.is_some() || req.requirement_text.is_some(), "worker invocation requires requirementPath or requirementText");

    let requirement_text = match (&req.requirement_text, &req.requirement_path) {
        (Some(text), _) => text.clone(),
        (None, Some(path)) => std::fs::read_to_string(path)?,
        (None, None) => unreachable!(),
    };

    let system_prompt = format!(
        "You are running inside Gold Band runtime.\n\nCurrent location:\n- Invocation kind: {:?}\n- Attempt directory: {}\n- Workspace directory: {}\n{}{}{}\n- Return only the final answer content for the declared primary artifact when one is required.",
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

pub fn default_provider() -> Box<dyn ProviderAdapter> {
    Box::new(ClaudeCodeProvider)
}
