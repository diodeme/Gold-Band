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
    pub output_contract: Option<PromptOutputContract>,
    pub runtime_context: PromptRuntimeContext,
    pub predecessors: Vec<PromptPredecessorContext>,
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
    pub attachments_dir: Option<Utf8PathBuf>,
    pub cold_artifacts: Vec<ColdFileRef>,
    pub cold_attachments: Vec<ColdFileRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptRuntimeContext {
    pub project_id: String,
    pub task_id: String,
    pub run_id: String,
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
    pub run_dir: Utf8PathBuf,
    pub round_dir: Utf8PathBuf,
    pub node_dir: Utf8PathBuf,
    pub attempt_dir: Utf8PathBuf,
    pub attachments_dir: Utf8PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptPredecessorContext {
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
    pub node_type: String,
    pub branch_kind: String,
    pub outcome: Option<String>,
    pub branch_direction: Option<String>,
    pub output_artifact: Option<PromptArtifactRef>,
    pub branch_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptArtifactRef {
    pub name: String,
    pub path: Utf8PathBuf,
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptOutputContract {
    pub artifact: String,
    pub kind: String,
    pub schema: Option<serde_json::Value>,
    pub success_condition: Option<String>,
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
            let uses_json_output = req
                .output_contract
                .as_ref()
                .is_some_and(|contract| contract.kind == "json")
                || artifact_uses_json_output(primary_artifact);
            let content = if uses_json_output {
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

    let system_prompt = render_system_prompt(req);
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
        prompt_id: None,
    })
}

fn render_system_prompt(req: &WorkerInvocation) -> String {
    [
        render_current_location(&req.runtime_context),
        render_predecessor_chain(&req.predecessors),
        render_predecessor_reasons(&req.predecessors),
        render_directory_rules(&req.runtime_context),
        render_role_section(req.profile.as_deref(), req.profile_content.as_deref()),
        render_output_constraints(req.output_contract.as_ref()),
    ]
    .into_iter()
    .filter(|section| !section.trim().is_empty())
    .collect::<Vec<_>>()
    .join("\n\n")
}

fn render_current_location(ctx: &PromptRuntimeContext) -> String {
    format!(
        "你正在 Gold Band runtime 中执行一个工作流节点。\n\n当前是：\n- Project: {}\n- Task: {}\n- Run: {}\n- Round: {}\n- Node: {}\n- Attempt: {}",
        ctx.project_id, ctx.task_id, ctx.run_id, ctx.round_id, ctx.node_id, ctx.attempt_id
    )
}

fn render_predecessor_chain(predecessors: &[PromptPredecessorContext]) -> String {
    if predecessors.is_empty() {
        return "当前节点的前序运行节点：无，当前节点是本轮入口节点。".to_string();
    }

    let mut chain = String::from("当前节点的前序运行节点：\n");
    for predecessor in predecessors {
        chain.push_str(&format!("{}/{} ", predecessor.node_id, predecessor.attempt_id));
        if let Some(direction) = predecessor.branch_direction.as_deref() {
            chain.push_str(&format!("-{direction}-> "));
        } else {
            chain.push_str("-> ");
        }
    }
    chain.push_str("当前节点");
    chain
}

fn render_predecessor_reasons(predecessors: &[PromptPredecessorContext]) -> String {
    if predecessors.is_empty() {
        return "当前节点前序节点的分支执行原因：无。".to_string();
    }

    let lines = predecessors
        .iter()
        .filter_map(|predecessor| {
            let is_ordinary = predecessor.branch_kind == "普通"
                && predecessor.branch_reason.is_none()
                && predecessor.output_artifact.is_none();
            if is_ordinary {
                return None;
            }

            let mut parts = vec![format!(
                "{}；节点类型={}；结果={}；分支方向={}",
                predecessor.branch_kind,
                predecessor.node_type,
                predecessor.outcome.as_deref().unwrap_or("unknown"),
                predecessor.branch_direction.as_deref().unwrap_or("unknown")
            )];
            if let Some(reason) = predecessor.branch_reason.as_deref() {
                parts.push(reason.to_string());
            }
            if let Some(artifact) = &predecessor.output_artifact {
                parts.push(format!("输出 artifact={}: {}", artifact.name, artifact.path));
                if let Some(preview) = artifact.preview.as_deref() {
                    parts.push(format!("输出预览={}{}", preview.trim(), if preview.ends_with('\n') { "" } else { "" }));
                }
            }
            Some(format!(
                "- {}/{}：{}。",
                predecessor.node_id,
                predecessor.attempt_id,
                parts.join("；")
            ))
        })
        .collect::<Vec<_>>();

    if lines.is_empty() {
        "当前节点前序节点的分支执行原因：前序节点均为普通节点，按节点结果进入当前分支。".to_string()
    } else {
        format!("当前节点前序节点的分支执行原因：\n{}", lines.join("\n"))
    }
}

fn render_directory_rules(ctx: &PromptRuntimeContext) -> String {
    format!(
        "Gold Band 文件规则：\n- 所有节点运行产物都位于：{}\n- 本次节点运行中，你创建的自由文件必须写入：{}\n- 不要把自由文件写到 attachments 之外。\n- 当前 run 目录可读取：{}\n- 当前 node 目录可写入：{}\n- runtime/ACP 可能会在 node 目录下写入状态文件；你的附加文件仍只能写入 attachments。",
        ctx.attempt_dir, ctx.attachments_dir, ctx.run_dir, ctx.node_dir
    )
}

fn render_role_section(profile: Option<&str>, profile_content: Option<&str>) -> String {
    let Some(profile) = profile else {
        return "当前节点角色：\n- 未配置 profile。".to_string();
    };
    let content = profile_content.map(str::trim).filter(|value| !value.is_empty());
    match content {
        Some(content) => format!("当前节点角色：\n- Profile ID: {profile}\n\n{content}"),
        None => format!("当前节点角色：\n- Profile ID: {profile}\n- 未找到 profile 正文。"),
    }
}

fn render_output_constraints(contract: Option<&PromptOutputContract>) -> String {
    let Some(contract) = contract else {
        return String::new();
    };

    let mut section = format!(
        "当前节点输出约束：\n- 输出 artifact: {}\n- 输出类型: {}\n\n你必须在最后一步按照以下格式输出你的结果：",
        contract.artifact, contract.kind
    );
    if let Some(schema) = &contract.schema {
        section.push_str(&format!(
            "\n{}",
            serde_json::to_string_pretty(schema).expect("serialize output schema")
        ));
    } else {
        section.push_str("\n当前节点未声明结构化 schema。");
    }
    if let Some(condition) = contract.success_condition.as_deref() {
        section.push_str(&format!("\n\nruntime 将使用以下条件判断节点结果：\n{condition}"));
    }
    section
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

    #[test]
    fn render_prompt_bundle_does_not_add_builtin_verify_or_exec_contracts() {
        let runtime_context = PromptRuntimeContext {
            project_id: "project-001".to_string(),
            task_id: "task-001".to_string(),
            run_id: "run-001".to_string(),
            round_id: "round-001".to_string(),
            node_id: "dev".to_string(),
            attempt_id: "attempt-001".to_string(),
            run_dir: Utf8PathBuf::from("/run"),
            round_dir: Utf8PathBuf::from("/run/rounds/round-001"),
            node_dir: Utf8PathBuf::from("/run/rounds/round-001/nodes/dev"),
            attempt_dir: Utf8PathBuf::from("/run/rounds/round-001/nodes/dev/attempt-001"),
            attachments_dir: Utf8PathBuf::from("/run/rounds/round-001/nodes/dev/attempt-001/attachments"),
        };
        let req = WorkerInvocation {
            invocation_kind: InvocationKind::WorkerGeneric,
            profile: None,
            profile_content: None,
            requirement_path: None,
            requirement_text: Some("Need an execution plan".to_string()),
            workspace_dir: Utf8PathBuf::from("/repo"),
            attempt_dir: runtime_context.attempt_dir.clone(),
            primary_artifact: Some("exec-plan".to_string()),
            output_contract: None,
            runtime_context,
            predecessors: Vec::new(),
            task_instruction: Some("Create an exec plan".to_string()),
            session_mode: SessionMode::New,
            continue_ref: None,
            resume_prompt: None,
            resume_prompt_id: None,
            stream_mode: StreamMode::StreamJson,
            log_prompts: false,
            log_provider_command: false,
            feedback_summary: None,
            attachments_dir: None,
            cold_artifacts: Vec::new(),
            cold_attachments: Vec::new(),
        };

        let prompt = render_prompt_bundle(&req).unwrap();
        assert!(!prompt.system_prompt.contains("Output contract"));
        assert!(!prompt.system_prompt.contains("verify-result"));
        assert!(!prompt.system_prompt.contains("exec-plan"));
    }

    #[test]
    fn default_provider_is_acp_only() {
        let info = default_provider().describe_provider();
        assert_eq!(info.provider_id, "claude-code");
        assert!(info.capabilities.supports_continue_session);
        assert!(!info.capabilities.supports_raw_stream);
    }
}
