use crate::acp::client;
use crate::artifacts::{artifact_uses_json_output, json_artifact_text_from_outputs};
use crate::config::{AcpAdapterConfig, ManagedAgentConfig, ManagedAgentType};
pub use crate::domain::SessionRef;
use crate::domain::{DEFAULT_PROVIDER, InvocationKind, SessionMode};
use anyhow::{Result, bail, ensure};
use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tracing::debug;

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
    pub profile_content: Option<String>,
    pub requirement_path: Option<Utf8PathBuf>,
    pub requirement_text: Option<String>,
    pub workspace_dir: Utf8PathBuf,
    pub attempt_dir: Utf8PathBuf,
    pub primary_artifact: Option<String>,
    pub task_instruction: Option<String>,
    pub session_mode: SessionMode,
    pub continue_ref: Option<serde_json::Value>,
    pub resume_prompt: Option<String>,
    pub resume_prompt_id: Option<String>,
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
    WaitingForUserInput,
    PermissionRequested,
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
    pub prompt_id: Option<String>,
}

pub trait ProviderAdapter: Send + Sync {
    fn describe_provider(&self) -> ProviderInfo;
    fn doctor(&self) -> DoctorResult;
    fn run_worker(&self, req: WorkerInvocation) -> Result<ProviderRunResult>;
    fn open_session(&self, worker_ref: &SessionRef) -> Result<()>;
    fn build_continue_command(&self, worker_ref: &SessionRef) -> Result<Option<String>>;
}

pub struct AcpProvider {
    adapter_config: AcpAdapterConfig,
}

impl AcpProvider {
    pub fn new(adapter_config: AcpAdapterConfig) -> Self {
        Self { adapter_config }
    }
}

impl ProviderAdapter for AcpProvider {
    fn describe_provider(&self) -> ProviderInfo {
        ProviderInfo {
            provider_id: DEFAULT_PROVIDER.to_string(),
            display_name: self.adapter_config.display_name.clone(),
            capabilities: ProviderCapabilities {
                supports_open_session: true,
                supports_continue_session: true,
                supports_raw_stream: false,
            },
            is_default: true,
        }
    }

    fn doctor(&self) -> DoctorResult {
        let cwd = std::env::current_dir()
            .ok()
            .and_then(|path| Utf8PathBuf::from_path_buf(path).ok())
            .unwrap_or_else(|| Utf8PathBuf::from("."));
        match client::doctor(&self.adapter_config, cwd) {
            Ok(()) => DoctorResult {
                available: true,
                reason: None,
            },
            Err(err) => DoctorResult {
                available: false,
                reason: Some(err.to_string()),
            },
        }
    }

    fn run_worker(&self, req: WorkerInvocation) -> Result<ProviderRunResult> {
        let prompt = render_prompt_bundle(&req)?;
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
        let run = client::run_prompt(
            &self.adapter_config,
            req.workspace_dir.clone(),
            req.attempt_dir.clone(),
            &prompt,
            req.session_mode,
            req.continue_ref.clone(),
        )?;
        let status = match run.stop_reason.as_deref() {
            Some("cancelled" | "interrupted" | "max_turn_requests") => {
                ProviderRunStatus::Interrupted
            }
            Some("waiting_for_user_input" | "user_input_required") => {
                ProviderRunStatus::WaitingForUserInput
            }
            Some("permission_requested") => ProviderRunStatus::PermissionRequested,
            Some("refusal" | "error") => ProviderRunStatus::Failure,
            _ => ProviderRunStatus::Success,
        };
        let result_payload = req.primary_artifact.as_ref().map(|primary_artifact| {
            let content = if artifact_uses_json_output(primary_artifact) {
                json_artifact_text_from_outputs(&run.final_outputs, &run.final_text)
                    .unwrap_or_else(|| run.final_text.clone())
            } else {
                run.final_text.clone()
            };
            ProviderResultPayload {
                primary_artifact: Some(PrimaryArtifactPayload {
                    name: primary_artifact.clone(),
                    content,
                }),
            }
        });
        Ok(ProviderRunResult {
            status,
            exit_code: None,
            result_payload,
            worker_ref_seed: None,
            stream_path: None,
        })
    }

    fn open_session(&self, worker_ref: &SessionRef) -> Result<()> {
        if !worker_ref.supports_open_session {
            bail!("provider does not support open-session");
        }
        Ok(())
    }

    fn build_continue_command(&self, _worker_ref: &SessionRef) -> Result<Option<String>> {
        Ok(None)
    }
}

pub fn render_prompt_bundle(req: &WorkerInvocation) -> Result<PromptBundle> {
    if matches!(req.session_mode, SessionMode::Continue) {
        if let Some(resume_prompt) = req.resume_prompt.as_ref() {
            return Ok(PromptBundle {
                system_prompt: String::new(),
                user_prompt: resume_prompt.clone(),
                prompt_id: req.resume_prompt_id.clone(),
            });
        }
    }

    ensure!(
        req.requirement_path.is_some() || req.requirement_text.is_some(),
        "worker invocation requires requirementPath or requirementText"
    );

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
            } else if artifact_uses_json_output(primary_artifact) {
                " Output a single valid JSON object. The workflow may evaluate specific JSON fields to decide the next edge."
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

    if let Some(profile_content) = req
        .profile_content
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        user_sections.push(format!("# Profile\n{}", profile_content));
    }

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
        prompt_id: None,
    })
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
    let agent_type = ManagedAgentType::from_str(provider_id)?;
    provider_capabilities_for_type(agent_type)
}

pub fn provider_capabilities_for_type(
    agent_type: ManagedAgentType,
) -> Result<ProviderCapabilities> {
    match agent_type {
        ManagedAgentType::ClaudeCode => Ok(AcpProvider::new(AcpAdapterConfig::default())
            .describe_provider()
            .capabilities),
        _ => bail!("unsupported agent type: {}", agent_type.as_str()),
    }
}

pub fn supports_continue_session(provider_id: &str) -> Result<bool> {
    Ok(provider_capabilities(provider_id)?.supports_continue_session)
}

pub fn provider_from_agent(
    agent_type: ManagedAgentType,
    config: &ManagedAgentConfig,
) -> Result<Box<dyn ProviderAdapter>> {
    match agent_type {
        ManagedAgentType::ClaudeCode => Ok(Box::new(AcpProvider::new(config.adapter.clone()))),
        _ => bail!("unsupported agent type: {}", agent_type.as_str()),
    }
}

pub fn provider_from_id(provider_id: &str) -> Result<Box<dyn ProviderAdapter>> {
    let agent_type = ManagedAgentType::from_str(provider_id)?;
    let config = match agent_type {
        ManagedAgentType::ClaudeCode => ManagedAgentConfig::new(AcpAdapterConfig::default()),
        _ => bail!("unsupported agent type: {}", agent_type.as_str()),
    };
    provider_from_agent(agent_type, &config)
}

pub fn default_provider() -> Box<dyn ProviderAdapter> {
    provider_from_id(DEFAULT_PROVIDER).expect("default provider must be supported")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::GoldBandPaths;
    use tempfile::tempdir;

    #[test]
    fn render_prompt_bundle_includes_verify_result_output_contract() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = GoldBandPaths::new(repo_root.clone());
        let req = WorkerInvocation {
            invocation_kind: InvocationKind::VerifyAcceptance,
            profile: Some("verifier".to_string()),
            profile_content: None,
            requirement_path: None,
            requirement_text: Some("Check whether hello-world exists".to_string()),
            workspace_dir: repo_root.clone(),
            attempt_dir: paths.attempt_dir("task-001", "run-001", "round-001", "accept", "attempt-001"),
            primary_artifact: Some("verify-result".to_string()),
            task_instruction: Some("Evaluate whether the requirement is satisfied based only on the provided evidence and produce a verify-result.".to_string()),
            session_mode: SessionMode::New,
            continue_ref: None,
            resume_prompt: None,
            resume_prompt_id: None,
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
        assert!(
            prompt
                .system_prompt
                .contains("Required primary artifact: verify-result")
        );
        assert!(prompt.system_prompt.contains("unmet_requirements"));
        assert!(prompt.system_prompt.contains("validation_gaps"));
        assert!(prompt.system_prompt.contains("status=\"failure\""));
    }

    #[test]
    fn render_prompt_bundle_includes_exec_plan_output_contract() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = GoldBandPaths::new(repo_root.clone());
        let req = WorkerInvocation {
            invocation_kind: InvocationKind::WorkerGeneric,
            profile: Some("developer".to_string()),
            profile_content: None,
            requirement_path: None,
            requirement_text: Some("Need an execution plan".to_string()),
            workspace_dir: repo_root.clone(),
            attempt_dir: paths.attempt_dir(
                "task-001",
                "run-001",
                "round-001",
                "dev",
                "attempt-001",
            ),
            primary_artifact: Some("exec-plan".to_string()),
            task_instruction: Some("Create an exec plan".to_string()),
            session_mode: SessionMode::New,
            continue_ref: None,
            resume_prompt: None,
            resume_prompt_id: None,
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
        assert!(
            prompt
                .system_prompt
                .contains("return exactly one valid `exec-plan` artifact")
        );
        assert!(prompt.system_prompt.contains("\"commands\""));
        assert!(prompt.system_prompt.contains("non-empty"));
    }

    #[test]
    fn default_provider_is_acp_only() {
        let info = default_provider().describe_provider();
        assert_eq!(info.provider_id, "claude-code");
        assert!(info.capabilities.supports_continue_session);
        assert!(!info.capabilities.supports_raw_stream);
    }
}
