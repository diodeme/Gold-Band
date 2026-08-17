use crate::acp::{client, events::AcpUiEvent};
use crate::artifacts::{artifact_uses_json_output, json_artifact_text};
use crate::config::{
    AcpAdapterConfig, ManagedAgentConfig, ManagedAgentId, catalog_agent_default_config,
};
pub use crate::domain::SessionRef;
use crate::domain::{
    DEFAULT_PROVIDER, InvocationKind, SessionMode, TurnControlMode, TurnControlTransitionCause,
    VERSION,
};
use crate::prompts::{
    PromptExecutionSurface, RUNTIME_ARTIFACT_FINALIZE_EN, RUNTIME_ARTIFACT_FINALIZE_ZH_CN,
    RUNTIME_HIDDEN_CONTEXT_EN, RUNTIME_HIDDEN_CONTEXT_ZH_CN, RUNTIME_SYSTEM_EN,
    RUNTIME_SYSTEM_ZH_CN, RUNTIME_USER_EN, RUNTIME_USER_ZH_CN, profile_template_context,
    prompt_by_language, render as render_template,
};
use crate::runtime::WorkerRefState;
use crate::runtime_error::{
    RecoveryMode, RuntimeErrorDomain, RuntimeErrorInfo, blocked_runtime_error_info,
    normalize_provider_runtime_failure, runtime_error,
};
use crate::storage::{active_storage_path_config, read_json, write_json};
use anyhow::{Context, Result, bail, ensure};
use atomic_write_file::AtomicWriteFile;
use camino::{Utf8Path, Utf8PathBuf};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Write;
use std::str::FromStr;
use tracing::debug;

use crate::acp::events::AttachmentMeta;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPromptQuote {
    pub id: String,
    pub source_message_key: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPromptInput {
    pub display_text: String,
    #[serde(default)]
    pub quotes: Vec<UserPromptQuote>,
}

impl From<String> for ConversationPromptInput {
    fn from(prompt: String) -> Self {
        Self {
            display_text: prompt.clone(),
            quotes: Vec::new(),
        }
    }
}

pub const MAX_USER_PROMPT_QUOTE_CHARS: usize = 12_000;
pub const MAX_USER_PROMPT_QUOTES: usize = 64;
pub const MAX_USER_PROMPT_QUOTE_ID_BYTES: usize = 128;
pub const MAX_USER_PROMPT_QUOTE_SOURCE_KEY_BYTES: usize = 512;

pub fn conversation_prompt_text(display_text: &str, quotes: &[UserPromptQuote]) -> String {
    let display_text = display_text.trim();
    if quotes.is_empty() {
        return display_text.to_string();
    }
    let quote_blocks = quotes
        .iter()
        .map(|quote| {
            quote
                .text
                .lines()
                .map(|line| format!("> {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("{quote_blocks}\n\n{display_text}")
}

/// Attachment content awaiting projection into an ACP session/prompt content block.
///
/// The live ACP connection capabilities decide whether this becomes visual/embedded
/// content or the protocol-baseline resource link at the outbound request boundary.
#[derive(Debug, Clone)]
pub enum AcpContentBlock {
    Image(AcpImageBlock),
    Resource(AcpResourceBlock),
}

#[derive(Debug, Clone)]
pub struct AcpImageBlock {
    pub data: String,
    pub mime_type: String,
    pub link: AcpResourceLinkBlock,
}

#[derive(Debug, Clone)]
pub struct AcpResourceBlock {
    pub resource: AcpTextResourceContents,
    pub link: AcpResourceLinkBlock,
}

#[derive(Debug, Clone)]
pub struct AcpTextResourceContents {
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct AcpResourceLinkBlock {
    pub name: String,
    pub uri: String,
    pub mime_type: String,
    pub size: u64,
}

/// Resolved attachment ready to be sent to ACP.
#[derive(Debug, Clone)]
pub struct ResolvedAttachment {
    pub meta: AttachmentMeta,
    pub block: AcpContentBlock,
}

pub const TASK_INPUT_ATTACHMENT_PREFIX: &str = "task-inputs";
pub const USER_INPUT_ATTACHMENT_PREFIX: &str = "user-inputs";

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
    pub supports_system_prompt: bool,
    pub supports_raw_stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpModeOption {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AcpSelectConfigValue {
    pub value: String,
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AcpSelectConfigOption {
    pub id: String,
    pub category: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub current_value: Option<String>,
    pub options: Vec<AcpSelectConfigValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorResult {
    pub available: bool,
    pub reason: Option<String>,
    pub capabilities: Option<Value>,
}

impl DoctorResult {
    pub fn supported_modes(&self) -> Vec<AcpModeOption> {
        supported_modes_from_capabilities(self.capabilities.as_ref())
    }

    pub fn supported_models(&self) -> Vec<AcpModeOption> {
        supported_models_from_capabilities(self.capabilities.as_ref())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UserPromptRenderMode {
    RequirementTask,
    WorkflowResume,
    RuntimeResume,
    RuntimeFinalize,
    RuntimeRepair,
    UserMessage,
}

/// Describes whether accepting the prompt changes Runtime control ownership.
/// This is intentionally independent from `SessionMode`: continuing an ACP
/// session does not by itself imply either a manual follow-up or a Runtime
/// resume transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeControlIntent {
    #[default]
    Unchanged,
    ManualFollowUp,
    Resume,
}

impl Default for UserPromptRenderMode {
    fn default() -> Self {
        Self::RequirementTask
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerInvocation {
    pub invocation_kind: InvocationKind,
    #[serde(default)]
    pub turn_control_mode: TurnControlMode,
    #[serde(default)]
    pub runtime_control_intent: RuntimeControlIntent,
    #[serde(default)]
    pub prompt_envelope: crate::dsl::PromptEnvelopeMode,
    pub execution_surface: PromptExecutionSurface,
    pub profile: Option<String>,
    pub profile_content: Option<String>,
    #[serde(default)]
    pub profile_dynamic_template: bool,
    pub requirement_path: Option<Utf8PathBuf>,
    pub requirement_text: Option<String>,
    pub adapter_workspace_dir: Utf8PathBuf,
    pub workspace_dir: Utf8PathBuf,
    pub attempt_dir: Utf8PathBuf,
    pub output_contract: Option<PromptOutputContract>,
    pub runtime_context: PromptRuntimeContext,
    pub predecessors: Vec<PromptPredecessorContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_round_trigger: Option<PromptPredecessorContext>,
    #[serde(default)]
    pub extra_system_sections: Vec<String>,
    #[serde(default)]
    pub extra_hidden_sections: Vec<PromptHiddenSection>,
    pub task_instruction: Option<String>,
    #[serde(default)]
    pub user_tips_instruction: Option<String>,
    #[serde(default)]
    pub resume_task_instruction: Option<String>,
    pub session_mode: SessionMode,
    #[serde(default)]
    pub user_prompt_render_mode: UserPromptRenderMode,
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config_options: BTreeMap<String, String>,
    pub continue_ref: Option<serde_json::Value>,
    pub resume_prompt: Option<String>,
    pub resume_prompt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_display: Option<ConversationPromptInput>,
    #[serde(default)]
    pub resume_prompt_visibility: PromptVisibility,
    pub stream_mode: StreamMode,
    #[serde(default)]
    pub log_prompts: bool,
    #[serde(default)]
    pub log_provider_command: bool,
    pub attachments_dir: Option<Utf8PathBuf>,
    pub cold_artifacts: Vec<ColdFileRef>,
    pub cold_attachments: Vec<ColdFileRef>,
    /// Task-owned requirement inputs remain canonical under authoring/inputs
    /// and are referenced from the first user message as task-inputs/*.
    #[serde(default)]
    pub task_input_attachment_paths: Vec<String>,
    /// Attachments explicitly added by a later user turn belong to this
    /// attempt and are materialized under user-inputs/* before prompting.
    #[serde(default)]
    pub user_input_attachment_paths: Vec<String>,
    #[serde(default)]
    pub mcp_servers: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduled_context: Option<ScheduledTaskContextInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTaskContextInfo {
    pub title: String,
    pub mode: String,
    pub session_policy: String,
    pub trigger_kind: String,
    pub triggered_at: String,
    pub instruction: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptRuntimeContext {
    pub project_id: String,
    pub task_id: String,
    pub run_id: String,
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
    #[serde(default)]
    pub runtime_node_id: Option<String>,
    #[serde(default)]
    pub runtime_attempt_id: Option<String>,
    #[serde(default)]
    pub attempt_state_file: Option<Utf8PathBuf>,
    pub language: crate::config::DesktopLanguage,
    pub run_dir: Utf8PathBuf,
    pub round_dir: Utf8PathBuf,
    pub node_dir: Utf8PathBuf,
    pub attempt_dir: Utf8PathBuf,
    pub attachments_dir: Utf8PathBuf,
    #[serde(default)]
    pub task_inputs_dir: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptHiddenSection {
    pub title: String,
    pub content: String,
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
    pub attachments: Vec<PromptAttachmentRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptArtifactRef {
    pub name: String,
    pub path: Utf8PathBuf,
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptAttachmentRef {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptOutputContract {
    pub artifact: String,
    pub kind: String,
    pub schema: Option<serde_json::Value>,
    pub schema_text: Option<String>,
    pub success_condition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finalize_context: Option<String>,
    #[serde(default)]
    pub emission_mode: OutputEmissionMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum OutputEmissionMode {
    #[default]
    PostTurnProjection,
    InlineControl,
}

const ARTIFACT_EMISSION_STATE_FILE: &str = "artifact-emission.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ArtifactEmissionPhase {
    BusinessTurn,
    Finalizing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArtifactEmissionState {
    version: String,
    phase: ArtifactEmissionPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PostTurnProjectionEntry {
    RunBusinessTurn,
    ResumeFinalization,
}

fn artifact_emission_checkpoint(attempt_dir: &Utf8Path) -> Result<Option<ArtifactEmissionState>> {
    let state_path = attempt_dir.join(ARTIFACT_EMISSION_STATE_FILE);
    if !state_path.exists() {
        return Ok(None);
    }
    let state: ArtifactEmissionState = read_json(&state_path).map_err(|error| {
        runtime_error(blocked_runtime_error_info(
            RuntimeErrorDomain::Internal,
            "runtime.artifact-emission-state-invalid",
            format!("failed to read artifact emission state `{state_path}`: {error:#}"),
            serde_json::json!({ "path": state_path }),
        ))
    })?;
    if state.version != VERSION {
        return Err(runtime_error(blocked_runtime_error_info(
            RuntimeErrorDomain::Internal,
            "runtime.artifact-emission-state-version-unsupported",
            format!(
                "unsupported artifact emission state version `{}`",
                state.version
            ),
            serde_json::json!({
                "path": state_path,
                "actualVersion": state.version,
                "expectedVersion": VERSION,
            }),
        )));
    }
    Ok(Some(state))
}

fn write_artifact_emission_phase(
    attempt_dir: &Utf8Path,
    phase: ArtifactEmissionPhase,
) -> Result<()> {
    write_json(
        &attempt_dir.join(ARTIFACT_EMISSION_STATE_FILE),
        &ArtifactEmissionState {
            version: VERSION.to_string(),
            phase,
        },
    )
}

fn prepare_post_turn_projection(req: &WorkerInvocation) -> Result<PostTurnProjectionEntry> {
    if matches!(
        req.user_prompt_render_mode,
        UserPromptRenderMode::RuntimeFinalize | UserPromptRenderMode::RuntimeRepair
    ) {
        return Ok(PostTurnProjectionEntry::ResumeFinalization);
    }

    match artifact_emission_checkpoint(&req.attempt_dir)?.map(|state| state.phase) {
        Some(ArtifactEmissionPhase::Finalizing)
            if req.user_prompt_render_mode == UserPromptRenderMode::UserMessage =>
        {
            // A continue-with-message at the finalize boundary opens a new durable
            // business turn. If that turn is interrupted, the next continue must
            // resume business work instead of skipping directly back to artifact
            // finalization.
            write_artifact_emission_phase(&req.attempt_dir, ArtifactEmissionPhase::BusinessTurn)?;
            Ok(PostTurnProjectionEntry::RunBusinessTurn)
        }
        Some(ArtifactEmissionPhase::Finalizing) => Ok(PostTurnProjectionEntry::ResumeFinalization),
        Some(ArtifactEmissionPhase::BusinessTurn) | None => {
            Ok(PostTurnProjectionEntry::RunBusinessTurn)
        }
    }
}

pub(crate) fn post_turn_projection_checkpoint_is_finalizing(
    attempt_dir: &Utf8Path,
) -> Result<bool> {
    Ok(artifact_emission_checkpoint(attempt_dir)?
        .is_some_and(|state| state.phase == ArtifactEmissionPhase::Finalizing))
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_error: Option<RuntimeErrorInfo>,
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

#[derive(Debug)]
struct ProviderTerminalOutcome {
    status: ProviderRunStatus,
    runtime_error: Option<RuntimeErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResultPayload {
    pub output_artifact: Option<OutputArtifactPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputArtifactPayload {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct PromptBundle {
    pub system_prompt: String,
    pub user_prompt: String,
    pub display_text: Option<String>,
    pub quotes: Vec<UserPromptQuote>,
    pub prompt_id: Option<String>,
    pub visibility: PromptVisibility,
    pub hidden_reason: Option<String>,
    pub turn_control_mode: TurnControlMode,
    pub runtime_control_intent: RuntimeControlIntent,
    pub runtime_control_transition_id: Option<String>,
    pub runtime_control_source_transition_id: Option<String>,
    pub runtime_control_transition_cause: Option<TurnControlTransitionCause>,
    pub attachment_metas: Vec<AttachmentMeta>,
    pub content_blocks: Vec<AcpContentBlock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttachmentContentKind {
    Image,
    Text,
}

#[derive(Debug, Clone, Copy)]
struct AttachmentFormat {
    extensions: &'static [&'static str],
    mime_type: &'static str,
    content_kind: AttachmentContentKind,
}

const ATTACHMENT_FORMATS: &[AttachmentFormat] = &[
    AttachmentFormat {
        extensions: &["png"],
        mime_type: "image/png",
        content_kind: AttachmentContentKind::Image,
    },
    AttachmentFormat {
        extensions: &["jpg", "jpeg"],
        mime_type: "image/jpeg",
        content_kind: AttachmentContentKind::Image,
    },
    AttachmentFormat {
        extensions: &["webp"],
        mime_type: "image/webp",
        content_kind: AttachmentContentKind::Image,
    },
    AttachmentFormat {
        extensions: &["gif"],
        mime_type: "image/gif",
        content_kind: AttachmentContentKind::Image,
    },
    AttachmentFormat {
        extensions: &["bmp"],
        mime_type: "image/bmp",
        content_kind: AttachmentContentKind::Image,
    },
    AttachmentFormat {
        extensions: &["txt"],
        mime_type: "text/plain",
        content_kind: AttachmentContentKind::Text,
    },
    AttachmentFormat {
        extensions: &["md", "markdown"],
        mime_type: "text/markdown",
        content_kind: AttachmentContentKind::Text,
    },
    AttachmentFormat {
        extensions: &["json", "jsonl"],
        mime_type: "application/json",
        content_kind: AttachmentContentKind::Text,
    },
    AttachmentFormat {
        extensions: &["csv"],
        mime_type: "text/csv",
        content_kind: AttachmentContentKind::Text,
    },
    AttachmentFormat {
        extensions: &["html", "htm"],
        mime_type: "text/html",
        content_kind: AttachmentContentKind::Text,
    },
    AttachmentFormat {
        extensions: &["css"],
        mime_type: "text/css",
        content_kind: AttachmentContentKind::Text,
    },
    AttachmentFormat {
        extensions: &["js", "jsx"],
        mime_type: "text/javascript",
        content_kind: AttachmentContentKind::Text,
    },
    AttachmentFormat {
        extensions: &["ts", "tsx"],
        mime_type: "text/typescript",
        content_kind: AttachmentContentKind::Text,
    },
    AttachmentFormat {
        extensions: &["rs"],
        mime_type: "text/rust",
        content_kind: AttachmentContentKind::Text,
    },
    AttachmentFormat {
        extensions: &["py"],
        mime_type: "text/python",
        content_kind: AttachmentContentKind::Text,
    },
    AttachmentFormat {
        extensions: &["go"],
        mime_type: "text/go",
        content_kind: AttachmentContentKind::Text,
    },
    AttachmentFormat {
        extensions: &["java"],
        mime_type: "text/java",
        content_kind: AttachmentContentKind::Text,
    },
    AttachmentFormat {
        extensions: &["c", "h"],
        mime_type: "text/c",
        content_kind: AttachmentContentKind::Text,
    },
    AttachmentFormat {
        extensions: &["cpp", "hpp"],
        mime_type: "text/cpp",
        content_kind: AttachmentContentKind::Text,
    },
    AttachmentFormat {
        extensions: &["yaml", "yml"],
        mime_type: "text/yaml",
        content_kind: AttachmentContentKind::Text,
    },
    AttachmentFormat {
        extensions: &["xml"],
        mime_type: "text/xml",
        content_kind: AttachmentContentKind::Text,
    },
    AttachmentFormat {
        extensions: &["toml"],
        mime_type: "text/toml",
        content_kind: AttachmentContentKind::Text,
    },
    AttachmentFormat {
        extensions: &["log"],
        mime_type: "text/plain",
        content_kind: AttachmentContentKind::Text,
    },
    AttachmentFormat {
        extensions: &["sql"],
        mime_type: "text/plain",
        content_kind: AttachmentContentKind::Text,
    },
    AttachmentFormat {
        extensions: &["sh", "bash", "zsh"],
        mime_type: "text/plain",
        content_kind: AttachmentContentKind::Text,
    },
];

fn attachment_format(extension: &str) -> Option<&'static AttachmentFormat> {
    ATTACHMENT_FORMATS
        .iter()
        .find(|format| format.extensions.contains(&extension))
}

pub fn attachment_meta_for_path(
    path: &std::path::Path,
    storage_prefix: &str,
) -> Result<Option<AttachmentMeta>> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let Some(format) = attachment_format(&extension) else {
        return Ok(None);
    };
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string();
    Ok(Some(AttachmentMeta {
        path: format!("{storage_prefix}/{name}"),
        mime_type: format.mime_type.to_string(),
        size: path.metadata()?.len(),
        name,
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PromptVisibility {
    Visible,
    Hidden,
}

/// Resolve file paths into ResolvedAttachment structs.
/// For images: base64-encode and retain both visual content and resource-link metadata.
/// For text files: read as UTF-8 and retain both embedded content and resource-link metadata.
/// The current ACP connection capabilities choose the final protocol shape when prompting.
/// Other files are skipped.
pub fn resolve_attachments(
    paths: &[String],
    storage_prefix: &str,
) -> Result<Vec<ResolvedAttachment>> {
    let mut resolved = Vec::new();
    for path_str in paths {
        let std_path = std::path::Path::new(path_str);
        let extension = std_path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if attachment_format(&extension).is_none() {
            continue;
        }
        let data = std::fs::read(std_path)?;
        if let Some(attachment) = resolved_attachment(std_path, storage_prefix, data)? {
            resolved.push(attachment);
        }
    }
    Ok(resolved)
}

fn resolved_attachment(
    path: &std::path::Path,
    storage_prefix: &str,
    data: Vec<u8>,
) -> Result<Option<ResolvedAttachment>> {
    let Some(meta) = attachment_meta_for_path(path, storage_prefix)? else {
        return Ok(None);
    };
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let format = attachment_format(&extension)
        .expect("attachment metadata is only created for a supported format");
    let uri = format!("file://{}", path.to_string_lossy().replace('\\', "/"));
    let link = AcpResourceLinkBlock {
        name: meta.name.clone(),
        uri,
        mime_type: meta.mime_type.clone(),
        size: meta.size,
    };
    let block = match format.content_kind {
        AttachmentContentKind::Image => AcpContentBlock::Image(AcpImageBlock {
            data: base64_encode(&data),
            mime_type: format.mime_type.to_string(),
            link,
        }),
        AttachmentContentKind::Text => AcpContentBlock::Resource(AcpResourceBlock {
            resource: AcpTextResourceContents {
                text: String::from_utf8(data).unwrap_or_else(|_| "[binary file]".to_string()),
            },
            link,
        }),
    };
    Ok(Some(ResolvedAttachment { meta, block }))
}

/// Persists attachments added by a user turn into the owning attempt before
/// projecting them into ACP content blocks and timeline metadata.
pub fn resolve_user_input_attachments(
    paths: &[String],
    attempt_dir: &Utf8Path,
) -> Result<Vec<ResolvedAttachment>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    let user_inputs_dir = attempt_dir.join(USER_INPUT_ATTACHMENT_PREFIX);
    std::fs::create_dir_all(user_inputs_dir.as_std_path())?;
    let mut resolved = Vec::with_capacity(paths.len());
    for path in paths {
        let source = std::path::Path::new(path);
        let extension = source
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if attachment_format(&extension).is_none() {
            continue;
        }
        let name = source
            .file_name()
            .and_then(|name| name.to_str())
            .context("user input attachment requires a UTF-8 file name")?;
        let destination = user_inputs_dir.join(name);
        let same_file = destination.exists()
            && std::fs::canonicalize(source)
                .ok()
                .zip(std::fs::canonicalize(destination.as_std_path()).ok())
                .is_some_and(|(source, destination)| source == destination);
        let bytes = std::fs::read(source)?;
        if !same_file {
            let mut file = AtomicWriteFile::open(destination.as_std_path())?;
            file.write_all(&bytes)?;
            file.commit()?;
        }
        if let Some(attachment) = resolved_attachment(
            destination.as_std_path(),
            USER_INPUT_ATTACHMENT_PREFIX,
            bytes,
        )? {
            resolved.push(attachment);
        }
    }
    Ok(resolved)
}

/// Returns the set of file extensions supported as attachments.
/// This is the single source of truth — the frontend queries it via Tauri command.
pub fn supported_attachment_extensions() -> Vec<&'static str> {
    ATTACHMENT_FORMATS
        .iter()
        .flat_map(|format| format.extensions.iter().copied())
        .collect()
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(((bytes.len() + 2) / 3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 0x3F) as usize] as char
        } else {
            b'=' as char
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 0x3F) as usize] as char
        } else {
            b'=' as char
        });
    }
    out
}

impl Default for PromptVisibility {
    fn default() -> Self {
        Self::Visible
    }
}

pub type AcpLiveUpdate<'a> = &'a dyn Fn(&AcpUiEvent) -> Result<()>;
pub type AcpSessionUpdate<'a> = &'a dyn Fn() -> Result<()>;
pub type AcpPromptAccepted<'a> = &'a dyn Fn(&str) -> Result<()>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderRuntimePhase {
    FinalizingArtifact,
}

pub type ProviderRuntimePhaseUpdate<'a> = &'a dyn Fn(ProviderRuntimePhase) -> Result<()>;

pub trait ProviderAdapter: Send + Sync {
    fn describe_provider(&self) -> ProviderInfo;
    fn doctor(&self) -> DoctorResult;
    fn run_worker(&self, req: WorkerInvocation) -> Result<ProviderRunResult>;
    fn run_worker_with_live_update(
        &self,
        req: WorkerInvocation,
        _live_update: Option<AcpLiveUpdate<'_>>,
    ) -> Result<ProviderRunResult> {
        self.run_worker(req)
    }
    fn run_worker_with_callbacks(
        &self,
        req: WorkerInvocation,
        live_update: Option<AcpLiveUpdate<'_>>,
        _session_update: Option<AcpSessionUpdate<'_>>,
        _prompt_accepted: Option<AcpPromptAccepted<'_>>,
    ) -> Result<ProviderRunResult> {
        self.run_worker_with_live_update(req, live_update)
    }
    fn run_worker_with_runtime_callbacks(
        &self,
        req: WorkerInvocation,
        live_update: Option<AcpLiveUpdate<'_>>,
        session_update: Option<AcpSessionUpdate<'_>>,
        prompt_accepted: Option<AcpPromptAccepted<'_>>,
        _runtime_phase_update: Option<ProviderRuntimePhaseUpdate<'_>>,
    ) -> Result<ProviderRunResult> {
        self.run_worker_with_callbacks(req, live_update, session_update, prompt_accepted)
    }
    fn open_session(&self, worker_ref: &SessionRef) -> Result<()>;
    fn build_continue_command(&self, worker_ref: &SessionRef) -> Result<Option<String>>;
}

fn option_str(option: &Value, key: &str) -> Option<String> {
    option
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub fn supported_modes_from_capabilities(capabilities: Option<&Value>) -> Vec<AcpModeOption> {
    if let Some(options) = capabilities
        .and_then(find_mode_config_option)
        .and_then(|option| option.get("options"))
        .and_then(Value::as_array)
    {
        return options
            .iter()
            .filter_map(|option| {
                let id = option.get("value").and_then(Value::as_str)?.trim();
                if id.is_empty() {
                    return None;
                }
                Some(AcpModeOption {
                    id: id.to_string(),
                    name: option_str(option, "name"),
                    description: option_str(option, "description"),
                })
            })
            .collect();
    }

    capabilities
        .and_then(|value| value.get("modes"))
        .and_then(|value| value.get("availableModes"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|mode| {
            let id = mode.get("id").and_then(Value::as_str)?.trim();
            if id.is_empty() {
                return None;
            }
            Some(AcpModeOption {
                id: id.to_string(),
                name: mode
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
                description: None,
            })
        })
        .collect()
}

/// Extracts available AI models from agent capabilities.
/// Reads from `configOptions[?category="model"].options` (not configOptions[?id="mode"]
/// which carries permission-mode values, and not `modes.availableModes` which also
/// carries permission modes).
pub fn supported_models_from_capabilities(capabilities: Option<&Value>) -> Vec<AcpModeOption> {
    if let Some(options) = capabilities
        .and_then(find_model_config_option)
        .and_then(|option| option.get("options"))
        .and_then(Value::as_array)
    {
        return options
            .iter()
            .filter_map(|option| {
                let id = option.get("value").and_then(Value::as_str)?.trim();
                if id.is_empty() {
                    return None;
                }
                Some(AcpModeOption {
                    id: id.to_string(),
                    name: option
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                    description: option
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                })
            })
            .collect();
    }
    Vec::new()
}

/// Agent 对 MCP 远程传输的支持情况（stdio 由 ACP 规范保证必支持，不在此列）。
/// 读取 `agentCapabilities.mcpCapabilities.{http,sse}`（ACP schema `McpCapabilities`，camelCase）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct McpCapabilitiesSummary {
    /// 是否支持 streamable HTTP 传输
    pub http: bool,
    /// 是否支持旧式 SSE 传输
    pub sse: bool,
}

/// 从 agent capabilities 提取 MCP 远程传输支持。
/// 返回 `None` 表示 capabilities 缺失或无 `mcpCapabilities` 字段 —— agent 尚未诊断或未声明。
pub fn mcp_capabilities_from_capabilities(
    capabilities: Option<&Value>,
) -> Option<McpCapabilitiesSummary> {
    let mcp = capabilities?.get("mcpCapabilities")?;
    Some(McpCapabilitiesSummary {
        http: mcp.get("http").and_then(Value::as_bool).unwrap_or(false),
        sse: mcp.get("sse").and_then(Value::as_bool).unwrap_or(false),
    })
}

pub const ACP_MCP_TRANSPORT_UNSUPPORTED_CODE: &str = "acp.mcp-transport-unsupported";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AcpMcpTransport {
    Stdio,
    Http,
    Sse,
}

impl AcpMcpTransport {
    fn from_server(server: &Value) -> Self {
        match server.get("type").and_then(Value::as_str) {
            Some("http") => Self::Http,
            Some("sse") => Self::Sse,
            _ => Self::Stdio,
        }
    }

    fn capability_path(self) -> Option<&'static str> {
        match self {
            Self::Stdio => None,
            Self::Http => Some("mcpCapabilities.http"),
            Self::Sse => Some("mcpCapabilities.sse"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedAcpMcpServer {
    pub name: String,
    pub transport: AcpMcpTransport,
    pub capability: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AcpMcpPreparationResult {
    pub accepted: Vec<Value>,
    pub skipped: Vec<SkippedAcpMcpServer>,
}

/// Applies the current Agent's ACP initialize capabilities to the MCP list.
/// Missing `mcpCapabilities` means the Agent did not declare remote transport
/// support, so the existing list is preserved instead of guessing.
pub fn prepare_acp_mcp_servers(
    servers: &[Value],
    agent_capabilities: Option<&Value>,
) -> AcpMcpPreparationResult {
    let Some(mcp_capabilities) =
        agent_capabilities.and_then(|capabilities| capabilities.get("mcpCapabilities"))
    else {
        return AcpMcpPreparationResult {
            accepted: servers.to_vec(),
            skipped: Vec::new(),
        };
    };

    let mut accepted = Vec::with_capacity(servers.len());
    let mut skipped = Vec::new();
    for server in servers {
        let transport = AcpMcpTransport::from_server(server);
        let supported = match transport {
            AcpMcpTransport::Stdio => true,
            AcpMcpTransport::Http => {
                mcp_capabilities.get("http").and_then(Value::as_bool) != Some(false)
            }
            AcpMcpTransport::Sse => {
                mcp_capabilities.get("sse").and_then(Value::as_bool) != Some(false)
            }
        };
        if supported {
            accepted.push(server.clone());
            continue;
        }
        skipped.push(SkippedAcpMcpServer {
            name: server
                .get("name")
                .or_else(|| server.get("id"))
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .unwrap_or("unnamed")
                .to_string(),
            transport,
            capability: transport
                .capability_path()
                .expect("stdio MCP transport is always accepted")
                .to_string(),
        });
    }
    AcpMcpPreparationResult { accepted, skipped }
}

/// Extracts generic ACP select configuration options without assuming adapter-specific IDs.
pub fn select_config_options_from_capabilities(
    capabilities: Option<&Value>,
) -> Vec<AcpSelectConfigOption> {
    capabilities
        .and_then(|value| value.get("configOptions"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| {
            let id = option.get("id").and_then(Value::as_str)?.trim();
            if id.is_empty() || option.get("type").and_then(Value::as_str) != Some("select") {
                return None;
            }
            let values = option
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|value| {
                    let raw_value = value.get("value").and_then(Value::as_str)?.trim();
                    if raw_value.is_empty() {
                        return None;
                    }
                    Some(AcpSelectConfigValue {
                        value: raw_value.to_string(),
                        name: optional_trimmed_string(value.get("name")),
                        description: optional_trimmed_string(value.get("description")),
                    })
                })
                .collect::<Vec<_>>();
            (!values.is_empty()).then(|| AcpSelectConfigOption {
                id: id.to_string(),
                category: optional_trimmed_string(option.get("category")),
                name: optional_trimmed_string(option.get("name")),
                description: optional_trimmed_string(option.get("description")),
                current_value: optional_trimmed_string(option.get("currentValue")),
                options: values,
            })
        })
        .collect()
}

fn optional_trimmed_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Finds the config option with `category == "model"` (AI model selector).
fn find_model_config_option(capabilities: &Value) -> Option<&Value> {
    capabilities
        .get("configOptions")
        .and_then(Value::as_array)
        .and_then(|options| {
            options
                .iter()
                .find(|option| option.get("category").and_then(Value::as_str) == Some("model"))
        })
}

fn find_mode_config_option(capabilities: &Value) -> Option<&Value> {
    capabilities
        .get("configOptions")
        .and_then(Value::as_array)
        .and_then(|options| {
            options.iter().find(|option| {
                option.get("id").and_then(Value::as_str) == Some("mode")
                    || option.get("category").and_then(Value::as_str) == Some("mode")
            })
        })
}

pub struct AcpProvider {
    provider_id: String,
    adapter_config: AcpAdapterConfig,
    use_local_claude: bool,
    require_local_claude_executable: bool,
    acp_session_title_refresh_enabled: bool,
    acp_raw_max_size_bytes: u64,
    acp_raw_target_size_bytes: u64,
    runtime_policy: client::AcpRuntimePolicy,
    supports_system_prompt: bool,
}

impl AcpProvider {
    pub fn new(
        provider_id: impl Into<String>,
        adapter_config: AcpAdapterConfig,
        use_local_claude: bool,
        require_local_claude_executable: bool,
        acp_session_title_refresh_enabled: bool,
        acp_raw_max_size_bytes: u64,
        acp_raw_target_size_bytes: u64,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            adapter_config,
            use_local_claude,
            require_local_claude_executable,
            acp_session_title_refresh_enabled,
            acp_raw_max_size_bytes,
            acp_raw_target_size_bytes,
            runtime_policy: client::AcpRuntimePolicy::default(),
            supports_system_prompt: false,
        }
    }

    pub fn with_runtime_policy(mut self, runtime_policy: client::AcpRuntimePolicy) -> Self {
        self.runtime_policy = runtime_policy;
        self
    }

    pub fn with_system_prompt_support(mut self, supported: bool) -> Self {
        self.supports_system_prompt = supported;
        self.runtime_policy.supports_system_prompt = supported;
        self
    }

    fn run_worker_once_with_callbacks(
        &self,
        req: WorkerInvocation,
        live_update: Option<AcpLiveUpdate<'_>>,
        session_update: Option<AcpSessionUpdate<'_>>,
        prompt_accepted: Option<AcpPromptAccepted<'_>>,
    ) -> Result<ProviderRunResult> {
        let prompt = render_prompt_bundle(&req)?;
        log_prompt_bundle(
            &prompt,
            req.invocation_kind,
            req.profile.as_deref(),
            req.output_contract
                .as_ref()
                .filter(|contract| contract.emission_mode == OutputEmissionMode::InlineControl)
                .map(|contract| contract.artifact.as_str()),
            req.cold_artifacts.len(),
            req.cold_attachments.len(),
            req.log_prompts,
        );
        let run = client::run_prompt(
            &self.provider_id,
            &self.adapter_config,
            req.adapter_workspace_dir.clone(),
            req.workspace_dir.clone(),
            req.attempt_dir.clone(),
            &prompt,
            req.session_mode,
            req.permission_mode.clone(),
            req.model.clone(),
            req.config_options.clone(),
            req.continue_ref.clone(),
            self.use_local_claude,
            self.require_local_claude_executable,
            self.acp_session_title_refresh_enabled,
            self.acp_raw_max_size_bytes,
            self.acp_raw_target_size_bytes,
            self.runtime_policy,
            live_update,
            &req.mcp_servers,
            session_update,
            prompt_accepted,
            Some(client::RuntimeStopProbe {
                run_file: req.runtime_context.run_dir.join("run.json"),
                round_id: req.runtime_context.round_id.clone(),
                node_id: req
                    .runtime_context
                    .runtime_node_id
                    .clone()
                    .unwrap_or_else(|| req.runtime_context.node_id.clone()),
                attempt_id: req
                    .runtime_context
                    .runtime_attempt_id
                    .clone()
                    .unwrap_or_else(|| req.runtime_context.attempt_id.clone()),
                attempt_state_file: req.runtime_context.attempt_state_file.clone(),
                turn_control_mode: req.turn_control_mode,
            }),
        )?;
        let mut terminal = classify_acp_prompt_run(&run);
        let artifact_result = if req.turn_control_mode == TurnControlMode::RuntimeControlled
            && matches!(
                terminal.status,
                ProviderRunStatus::Success | ProviderRunStatus::Interrupted
            ) {
            req.output_contract
                .as_ref()
                .filter(|contract| contract.emission_mode == OutputEmissionMode::InlineControl)
                .map(|contract| output_artifact_payload_from_run(contract, &run.output))
                .unwrap_or(Ok(None))
        } else {
            Ok(None)
        };
        let result_payload = match artifact_result {
            Ok(payload) => payload,
            Err(error) => {
                terminal.status = ProviderRunStatus::Failure;
                terminal.runtime_error = Some(error);
                None
            }
        };
        Ok(ProviderRunResult {
            status: terminal.status,
            exit_code: None,
            result_payload,
            worker_ref_seed: None,
            stream_path: None,
            runtime_error: terminal.runtime_error,
        })
    }

    fn run_post_turn_projection_with_callbacks(
        &self,
        req: WorkerInvocation,
        live_update: Option<AcpLiveUpdate<'_>>,
        session_update: Option<AcpSessionUpdate<'_>>,
        prompt_accepted: Option<AcpPromptAccepted<'_>>,
        runtime_phase_update: Option<ProviderRuntimePhaseUpdate<'_>>,
    ) -> Result<ProviderRunResult> {
        let mut contract = req
            .output_contract
            .clone()
            .expect("post-turn projection requires output contract");
        let resumed_control_turn = matches!(
            req.user_prompt_render_mode,
            UserPromptRenderMode::RuntimeFinalize | UserPromptRenderMode::RuntimeRepair
        );
        let entry = prepare_post_turn_projection(&req)?;

        if entry == PostTurnProjectionEntry::RunBusinessTurn {
            let work_result = self.run_worker_once_with_callbacks(
                req.clone(),
                live_update,
                session_update,
                prompt_accepted,
            )?;
            if work_result.runtime_error.is_some()
                || work_result.status != ProviderRunStatus::Success
            {
                return Ok(work_result);
            }
        }

        write_artifact_emission_phase(&req.attempt_dir, ArtifactEmissionPhase::Finalizing)?;
        if let Some(runtime_phase_update) = runtime_phase_update {
            runtime_phase_update(ProviderRuntimePhase::FinalizingArtifact)?;
        }
        let worker_ref: WorkerRefState = read_json(&req.attempt_dir.join("worker-ref.json"))
            .context("post-turn artifact finalization requires durable worker-ref")?;
        ensure!(
            worker_ref.supports_continue_session,
            "post-turn artifact finalization requires a continuable provider session"
        );
        let continue_ref = worker_ref
            .continue_ref
            .context("post-turn artifact finalization requires provider continue reference")?;

        let preserve_control_prompt = resumed_control_turn
            && req
                .resume_prompt
                .as_deref()
                .is_some_and(|prompt| !prompt.trim().is_empty());
        let mut finalize_req = req;
        contract.emission_mode = OutputEmissionMode::InlineControl;
        finalize_req.output_contract = Some(contract.clone());
        finalize_req.session_mode = SessionMode::Continue;
        finalize_req.continue_ref = Some(continue_ref);
        finalize_req.resume_prompt_visibility = PromptVisibility::Hidden;
        finalize_req.task_input_attachment_paths.clear();
        finalize_req.user_input_attachment_paths.clear();
        if !preserve_control_prompt {
            finalize_req.resume_prompt = Some(render_artifact_finalize_prompt(
                finalize_req.runtime_context.language,
                &contract,
            )?);
            finalize_req.resume_prompt_id = Some(format!(
                "artifact-finalize-{}",
                finalize_req.runtime_context.attempt_id
            ));
            finalize_req.user_prompt_render_mode = UserPromptRenderMode::RuntimeFinalize;
        }

        self.run_worker_once_with_callbacks(
            finalize_req,
            live_update,
            session_update,
            prompt_accepted,
        )
    }
}

impl ProviderAdapter for AcpProvider {
    fn describe_provider(&self) -> ProviderInfo {
        ProviderInfo {
            provider_id: self.provider_id.clone(),
            display_name: self.adapter_config.display_name.clone(),
            capabilities: ProviderCapabilities {
                supports_open_session: true,
                supports_continue_session: true,
                supports_system_prompt: self.supports_system_prompt,
                supports_raw_stream: false,
            },
            is_default: self.provider_id == DEFAULT_PROVIDER,
        }
    }

    fn doctor(&self) -> DoctorResult {
        let cwd = std::env::current_dir()
            .ok()
            .and_then(|path| Utf8PathBuf::from_path_buf(path).ok())
            .unwrap_or_else(|| Utf8PathBuf::from("."));
        let agent_id = match ManagedAgentId::from_str(&self.provider_id) {
            Ok(agent_id) => agent_id,
            Err(err) => {
                return DoctorResult {
                    available: false,
                    reason: Some(err.to_string()),
                    capabilities: None,
                };
            }
        };
        match client::doctor(
            &agent_id,
            &self.adapter_config,
            cwd,
            self.use_local_claude,
            self.require_local_claude_executable,
        ) {
            Ok(probe) => DoctorResult {
                available: true,
                reason: None,
                capabilities: Some(probe.capabilities),
            },
            Err(err) => DoctorResult {
                available: false,
                reason: Some(err.to_string()),
                capabilities: None,
            },
        }
    }

    fn run_worker(&self, req: WorkerInvocation) -> Result<ProviderRunResult> {
        self.run_worker_with_live_update(req, None)
    }

    fn run_worker_with_live_update(
        &self,
        req: WorkerInvocation,
        live_update: Option<AcpLiveUpdate<'_>>,
    ) -> Result<ProviderRunResult> {
        self.run_worker_with_callbacks(req, live_update, None, None)
    }

    fn run_worker_with_callbacks(
        &self,
        req: WorkerInvocation,
        live_update: Option<AcpLiveUpdate<'_>>,
        session_update: Option<AcpSessionUpdate<'_>>,
        prompt_accepted: Option<AcpPromptAccepted<'_>>,
    ) -> Result<ProviderRunResult> {
        self.run_worker_with_runtime_callbacks(
            req,
            live_update,
            session_update,
            prompt_accepted,
            None,
        )
    }

    fn run_worker_with_runtime_callbacks(
        &self,
        req: WorkerInvocation,
        live_update: Option<AcpLiveUpdate<'_>>,
        session_update: Option<AcpSessionUpdate<'_>>,
        prompt_accepted: Option<AcpPromptAccepted<'_>>,
        runtime_phase_update: Option<ProviderRuntimePhaseUpdate<'_>>,
    ) -> Result<ProviderRunResult> {
        if req.turn_control_mode == TurnControlMode::RuntimeControlled
            && req.output_contract.as_ref().is_some_and(|contract| {
                contract.emission_mode == OutputEmissionMode::PostTurnProjection
            })
        {
            self.run_post_turn_projection_with_callbacks(
                req,
                live_update,
                session_update,
                prompt_accepted,
                runtime_phase_update,
            )
        } else {
            self.run_worker_once_with_callbacks(req, live_update, session_update, prompt_accepted)
        }
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

fn classify_acp_prompt_run(run: &client::AcpPromptRun) -> ProviderTerminalOutcome {
    if let Some(failure) = &run.terminal_failure {
        let raw = Some(serde_json::json!({
            "adapterId": run.adapter_id,
            "adapterDisplayName": run.adapter_display_name,
            "stopReason": run.stop_reason,
            "terminalFailure": failure,
        }));
        let mut runtime_error = normalize_provider_runtime_failure(
            run.stop_reason.as_deref(),
            failure.diagnostic(),
            raw,
        );
        // A structured terminal failure proves that the current prompt has
        // ended abnormally. Replaying a business prompt could duplicate
        // partial side effects, so recovery is always an explicit user action.
        runtime_error.recovery = RecoveryMode::Manual;
        runtime_error.retry_policy = None;
        return ProviderTerminalOutcome {
            status: ProviderRunStatus::Failure,
            runtime_error: Some(runtime_error),
        };
    }

    let normalized_reason = run
        .stop_reason
        .as_deref()
        .map(|reason| reason.trim().to_ascii_lowercase().replace('_', "-"));
    match normalized_reason.as_deref() {
        Some("end-turn") => ProviderTerminalOutcome {
            status: ProviderRunStatus::Success,
            runtime_error: None,
        },
        Some("cancelled" | "canceled" | "interrupted" | "max-turn-requests" | "max-tokens") => {
            ProviderTerminalOutcome {
                status: ProviderRunStatus::Interrupted,
                runtime_error: None,
            }
        }
        Some("waiting-for-user-input" | "user-input-required") => ProviderTerminalOutcome {
            status: ProviderRunStatus::WaitingForUserInput,
            runtime_error: None,
        },
        Some("permission-requested") => ProviderTerminalOutcome {
            status: ProviderRunStatus::PermissionRequested,
            runtime_error: None,
        },
        Some("refusal") => ProviderTerminalOutcome {
            status: ProviderRunStatus::Failure,
            runtime_error: None,
        },
        Some("error" | "failure") => ProviderTerminalOutcome {
            status: ProviderRunStatus::Failure,
            runtime_error: Some(normalize_provider_runtime_failure(
                run.stop_reason.as_deref(),
                run.output.visible_text.clone(),
                Some(serde_json::json!({
                    "adapterId": run.adapter_id,
                    "adapterDisplayName": run.adapter_display_name,
                    "stopReason": run.stop_reason,
                })),
            )),
        },
        unknown => {
            let diagnostic = match unknown {
                Some(reason) => format!("ACP returned unknown prompt stop reason `{reason}`"),
                None => "ACP prompt response did not include a stop reason".to_string(),
            };
            ProviderTerminalOutcome {
                status: ProviderRunStatus::Failure,
                runtime_error: Some(normalize_provider_runtime_failure(
                    run.stop_reason.as_deref(),
                    diagnostic,
                    Some(serde_json::json!({
                        "adapterId": run.adapter_id,
                        "adapterDisplayName": run.adapter_display_name,
                        "stopReason": run.stop_reason,
                    })),
                )),
            }
        }
    }
}

fn output_artifact_payload_from_run(
    contract: &PromptOutputContract,
    output: &client::AcpPromptOutput,
) -> std::result::Result<Option<ProviderResultPayload>, RuntimeErrorInfo> {
    let Some(terminal_message) = output.recent_messages.last() else {
        return Ok(None);
    };
    if !terminal_message.has_stable_id && output.observed_stable_message {
        return Err(crate::runtime_error::manual_runtime_error_info(
            RuntimeErrorDomain::Provider,
            "provider.acp-terminal-message-unidentified",
            "ACP prompt ended with anonymous Agent text after producing a stable Agent message",
            serde_json::json!({
                "observedStableMessage": true,
                "terminalMessageHasStableId": false,
            }),
        ));
    }

    let uses_json_output = contract.kind == "json" || artifact_uses_json_output(&contract.artifact);
    let content = if uses_json_output {
        if terminal_message.has_stable_id {
            output
                .recent_messages
                .iter()
                .rev()
                .take(3)
                .find_map(|message| json_artifact_text(&message.text))
        } else {
            json_artifact_text(&terminal_message.text)
        }
    } else {
        non_empty_artifact_text(&terminal_message.text)
    };

    Ok(content.map(|content| ProviderResultPayload {
        output_artifact: Some(OutputArtifactPayload {
            name: contract.artifact.clone(),
            content,
        }),
    }))
}

fn non_empty_artifact_text(value: &str) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.to_string())
}

pub fn render_prompt_bundle(req: &WorkerInvocation) -> Result<PromptBundle> {
    let requirement_text = match req.user_prompt_render_mode {
        UserPromptRenderMode::RequirementTask => {
            ensure!(
                req.requirement_path.is_some() || req.requirement_text.is_some(),
                "worker invocation requires requirementPath or requirementText"
            );
            match (&req.requirement_text, &req.requirement_path) {
                (Some(text), _) => text.clone(),
                (None, Some(path)) => std::fs::read_to_string(path)?,
                (None, None) => unreachable!(),
            }
        }
        UserPromptRenderMode::WorkflowResume
        | UserPromptRenderMode::RuntimeResume
        | UserPromptRenderMode::RuntimeFinalize
        | UserPromptRenderMode::RuntimeRepair
        | UserPromptRenderMode::UserMessage => String::new(),
    };

    let (system_prompt, mut user_prompt) = match req.prompt_envelope {
        crate::dsl::PromptEnvelopeMode::RuntimeManaged => (
            render_system_prompt(req)?,
            render_user_prompt(req, &requirement_text),
        ),
        crate::dsl::PromptEnvelopeMode::RawAgent => (
            String::new(),
            if matches!(req.session_mode, SessionMode::Continue) {
                req.resume_prompt.clone().unwrap_or_default()
            } else {
                requirement_text.clone()
            },
        ),
    };
    if req.turn_control_mode == TurnControlMode::NonRuntimeControlled
        && req.user_prompt_render_mode == UserPromptRenderMode::UserMessage
        && !req.extra_hidden_sections.is_empty()
    {
        user_prompt = append_extra_hidden_sections(&user_prompt, &req.extra_hidden_sections);
    }
    let is_continue = matches!(req.session_mode, SessionMode::Continue);

    let mut attachment_metas = Vec::new();
    let mut content_blocks = Vec::new();
    let task_inputs = resolve_attachments(
        &req.task_input_attachment_paths,
        TASK_INPUT_ATTACHMENT_PREFIX,
    )?;
    let user_inputs =
        resolve_user_input_attachments(&req.user_input_attachment_paths, &req.attempt_dir)?;
    for resolved in task_inputs.into_iter().chain(user_inputs) {
        attachment_metas.push(resolved.meta);
        content_blocks.push(resolved.block);
    }

    Ok(PromptBundle {
        system_prompt,
        user_prompt,
        display_text: req
            .prompt_display
            .as_ref()
            .map(|input| input.display_text.clone()),
        quotes: req
            .prompt_display
            .as_ref()
            .map(|input| input.quotes.clone())
            .unwrap_or_default(),
        // Prompt identity is an orchestration concern, independent of ACP
        // session mode.  In particular, an automatic retry may start a new
        // ACP session while remaining the same visible user turn.
        prompt_id: req.resume_prompt_id.clone(),
        visibility: if is_continue {
            req.resume_prompt_visibility
        } else {
            PromptVisibility::Visible
        },
        hidden_reason: match req.user_prompt_render_mode {
            UserPromptRenderMode::RuntimeResume => Some("runtimeControlResume".to_string()),
            UserPromptRenderMode::RuntimeFinalize => Some("artifactFinalize".to_string()),
            UserPromptRenderMode::RuntimeRepair => Some("invalidOutputRepair".to_string()),
            _ => None,
        },
        turn_control_mode: req.turn_control_mode,
        runtime_control_intent: req.runtime_control_intent,
        runtime_control_transition_id: None,
        runtime_control_source_transition_id: None,
        runtime_control_transition_cause: None,
        attachment_metas,
        content_blocks,
    })
}

fn render_system_prompt(req: &WorkerInvocation) -> Result<String> {
    render_template(
        prompt_by_language(
            req.runtime_context.language,
            RUNTIME_SYSTEM_ZH_CN,
            RUNTIME_SYSTEM_EN,
        ),
        runtime_system_context(req)?,
    )
}

fn render_user_prompt(req: &WorkerInvocation, requirement_text: &str) -> String {
    match req.user_prompt_render_mode {
        UserPromptRenderMode::UserMessage
        | UserPromptRenderMode::RuntimeResume
        | UserPromptRenderMode::RuntimeFinalize
        | UserPromptRenderMode::RuntimeRepair => req
            .resume_prompt
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_string(),
        UserPromptRenderMode::WorkflowResume | UserPromptRenderMode::RequirementTask => {
            let hidden_context = render_hidden_context(req);
            let continue_goal = matches!(
                req.user_prompt_render_mode,
                UserPromptRenderMode::WorkflowResume
            )
            .then(|| {
                prompt_by_language(
                    req.runtime_context.language,
                    crate::prompts::RUNTIME_WORKFLOW_RESUME_ZH_CN,
                    crate::prompts::RUNTIME_WORKFLOW_RESUME_EN,
                )
                .trim()
                .to_string()
            });

            let content = render_template(
                prompt_by_language(
                    req.runtime_context.language,
                    RUNTIME_USER_ZH_CN,
                    RUNTIME_USER_EN,
                ),
                RuntimeUserTemplateContext {
                    hidden_context,
                    requirement: requirement_text.trim().to_string(),
                    task: req
                        .task_instruction
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                    user_tips: req
                        .user_tips_instruction
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                    resume_task: req
                        .resume_task_instruction
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                    continue_goal,
                },
            )
            .expect("prompt template renders");
            compact_hidden_context_spacing(&content)
        }
    }
}

fn append_extra_hidden_sections(prompt: &str, sections: &[PromptHiddenSection]) -> String {
    let content = sections
        .iter()
        .map(|section| section.content.trim())
        .filter(|content| !content.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    if content.is_empty() {
        return prompt.to_string();
    }
    format!(
        "{}\n\n{}",
        prompt.trim(),
        gold_band_hidden_block("Gold Band runtime context", &content)
    )
}

fn render_hidden_context(req: &WorkerInvocation) -> String {
    let extra_sections = req
        .extra_hidden_sections
        .iter()
        .filter(|section| !section.content.trim().is_empty())
        .collect::<Vec<_>>();
    let suppress_base_context = extra_sections
        .iter()
        .any(|section| section.title == "Gold Band AI-DYNAMIC runtime context");
    let mut content = if suppress_base_context {
        String::new()
    } else {
        render_template(
            prompt_by_language(
                req.runtime_context.language,
                RUNTIME_HIDDEN_CONTEXT_ZH_CN,
                RUNTIME_HIDDEN_CONTEXT_EN,
            ),
            runtime_hidden_context(req),
        )
        .expect("prompt template renders")
    };
    for section in extra_sections {
        content.push_str("\n\n");
        content.push_str(section.content.trim());
    }
    let content = compact_hidden_context_spacing(&content);
    let content = if let Some(scheduled) = render_scheduled_context(req) {
        format!("{content}\n\n{scheduled}")
    } else {
        content
    };
    gold_band_hidden_block("Gold Band runtime context", &content)
}

fn render_scheduled_context(req: &WorkerInvocation) -> Option<String> {
    let ctx = req.scheduled_context.as_ref()?;
    let template = prompt_by_language(
        req.runtime_context.language,
        crate::prompts::RUNTIME_SCHEDULED_TASK_CONTEXT_ZH_CN,
        crate::prompts::RUNTIME_SCHEDULED_TASK_CONTEXT_EN,
    );
    let rendered = crate::prompts::render(
        template,
        &ScheduledTaskContextTemplateContext {
            scheduled_title: &ctx.title,
            scheduled_mode: &ctx.mode,
            scheduled_session_policy: &ctx.session_policy,
            scheduled_trigger_kind: &ctx.trigger_kind,
            scheduled_triggered_at: &ctx.triggered_at,
            scheduled_instruction: ctx.instruction.as_deref(),
        },
    )
    .ok()?;
    Some(gold_band_hidden_block(
        "Gold Band scheduled task context",
        &rendered,
    ))
}

#[derive(Serialize)]
struct ScheduledTaskContextTemplateContext<'a> {
    scheduled_title: &'a str,
    scheduled_mode: &'a str,
    scheduled_session_policy: &'a str,
    scheduled_trigger_kind: &'a str,
    scheduled_triggered_at: &'a str,
    scheduled_instruction: Option<&'a str>,
}

fn compact_hidden_context_spacing(content: &str) -> String {
    let mut compacted = content.replace("\r\n", "\n");
    while compacted.contains("\n\n\n") {
        compacted = compacted.replace("\n\n\n", "\n\n");
    }
    compacted.trim().to_string()
}

pub(crate) fn gold_band_hidden_block(title: &str, content: &str) -> String {
    let escaped = content.replace("</hidden>", "<\\/hidden>");
    format!(
        "<hidden data-gold-band-hidden=\"true\" title=\"{}\">\n{}\n</hidden>",
        title,
        escaped.trim()
    )
}

#[derive(Serialize)]
struct RuntimePromptTemplateContext {
    project_id: String,
    task_id: String,
    run_id: String,
    node_id: String,
    run_dir: String,
    node_dir: String,
    config_dir_name: String,
    extra_system_sections: Option<String>,
    profile: RuntimeProfileTemplateContext,
    output_contract: Option<RuntimeOutputContractTemplateContext>,
    output_deferred: bool,
}

#[derive(Serialize)]
struct RuntimeHiddenContextTemplateContext {
    session_mode: String,
    round_id: String,
    attempt_id: String,
    attempt_dir: String,
    attachments_dir: String,
    invocation_reason: Option<String>,
    predecessors: RuntimePredecessorTemplateContext,
}

#[derive(Serialize)]
struct RuntimeUserTemplateContext {
    hidden_context: String,
    requirement: String,
    task: Option<String>,
    user_tips: Option<String>,
    resume_task: Option<String>,
    continue_goal: Option<String>,
}

#[derive(Serialize)]
struct RuntimePredecessorTemplateContext {
    is_empty: bool,
    chain: String,
    reason_lines: String,
    reason_lines_empty: bool,
    attachment_lines: String,
    attachment_lines_empty: bool,
}

#[derive(Serialize)]
struct RuntimeProfileTemplateContext {
    id: Option<String>,
    content: Option<String>,
}

#[derive(Serialize)]
struct RuntimeOutputContractTemplateContext {
    artifact: String,
    kind: String,
    schema: String,
    success_condition: Option<String>,
    finalize_context: Option<String>,
}

fn runtime_system_context(req: &WorkerInvocation) -> Result<RuntimePromptTemplateContext> {
    let inline_output_contract = req
        .output_contract
        .as_ref()
        .filter(|contract| contract.emission_mode == OutputEmissionMode::InlineControl);
    let profile_content = match req
        .profile_content
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(content) if req.profile_dynamic_template => {
            let session_mode = match req.session_mode {
                SessionMode::New => "new",
                SessionMode::Continue => "continue",
            };
            Some(render_template(
                content,
                profile_template_context(
                    req.execution_surface,
                    inline_output_contract.is_some(),
                    session_mode,
                ),
            )?)
        }
        Some(content) => Some(content.to_string()),
        None => None,
    };
    Ok(RuntimePromptTemplateContext {
        project_id: req.runtime_context.project_id.clone(),
        task_id: req.runtime_context.task_id.clone(),
        run_id: req.runtime_context.run_id.clone(),
        node_id: req.runtime_context.node_id.clone(),
        run_dir: req.runtime_context.run_dir.to_string(),
        node_dir: req.runtime_context.node_dir.to_string(),
        config_dir_name: active_storage_path_config().config_dir_name.to_string(),
        extra_system_sections: joined_extra_system_sections(req),
        profile: RuntimeProfileTemplateContext {
            id: req.profile.clone(),
            content: profile_content,
        },
        output_contract: inline_output_contract.map(runtime_output_contract_context),
        output_deferred: req.output_contract.as_ref().is_some_and(|contract| {
            contract.emission_mode == OutputEmissionMode::PostTurnProjection
        }),
    })
}

fn render_artifact_finalize_prompt(
    language: crate::config::DesktopLanguage,
    contract: &PromptOutputContract,
) -> Result<String> {
    let context = runtime_output_contract_context(contract);
    render_template(
        prompt_by_language(
            language,
            RUNTIME_ARTIFACT_FINALIZE_ZH_CN,
            RUNTIME_ARTIFACT_FINALIZE_EN,
        ),
        context,
    )
}

fn runtime_hidden_context(req: &WorkerInvocation) -> RuntimeHiddenContextTemplateContext {
    RuntimeHiddenContextTemplateContext {
        session_mode: match req.session_mode {
            SessionMode::New => "new".to_string(),
            SessionMode::Continue => "continue".to_string(),
        },
        round_id: req.runtime_context.round_id.clone(),
        attempt_id: req.runtime_context.attempt_id.clone(),
        attempt_dir: req.runtime_context.attempt_dir.to_string(),
        attachments_dir: req.runtime_context.attachments_dir.to_string(),
        invocation_reason: runtime_invocation_reason(req),
        predecessors: runtime_predecessor_context(
            &req.predecessors,
            req.new_round_trigger.as_ref(),
            &req.runtime_context,
        ),
    }
}

fn joined_extra_system_sections(req: &WorkerInvocation) -> Option<String> {
    let sections = req
        .extra_system_sections
        .iter()
        .filter_map(|section| {
            let trimmed = section.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .collect::<Vec<_>>();
    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

fn runtime_invocation_reason(req: &WorkerInvocation) -> Option<String> {
    let mut parts = Vec::new();
    if matches!(req.session_mode, SessionMode::Continue) {
        parts.push(match req.runtime_context.language {
            crate::config::DesktopLanguage::ZhCn => "继续已有 ACP session".to_string(),
            crate::config::DesktopLanguage::En => "Continue an existing ACP session".to_string(),
        });
    }
    if let Some(resume_prompt) = req
        .resume_prompt
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(resume_prompt.to_string());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

fn predecessor_ref(predecessor: &PromptPredecessorContext) -> String {
    format!(
        "{}/{}/{}",
        predecessor.round_id, predecessor.node_id, predecessor.attempt_id
    )
}

fn runtime_predecessor_context(
    predecessors: &[PromptPredecessorContext],
    new_round_trigger: Option<&PromptPredecessorContext>,
    ctx: &PromptRuntimeContext,
) -> RuntimePredecessorTemplateContext {
    let reason_lines = predecessor_reason_lines(predecessors, new_round_trigger);
    let attachment_lines = predecessor_attachment_lines(predecessors);
    RuntimePredecessorTemplateContext {
        is_empty: predecessors.is_empty(),
        chain: predecessor_chain_text(predecessors, ctx),
        reason_lines_empty: reason_lines.is_empty(),
        reason_lines,
        attachment_lines_empty: attachment_lines.is_empty(),
        attachment_lines,
    }
}

fn predecessor_chain_text(
    predecessors: &[PromptPredecessorContext],
    ctx: &PromptRuntimeContext,
) -> String {
    if predecessors.is_empty() {
        return String::new();
    }

    let mut chain = String::new();
    for (index, predecessor) in predecessors.iter().enumerate() {
        chain.push_str(&format!("{} ", predecessor_ref(predecessor)));
        let next_round = predecessors
            .get(index + 1)
            .map(|next| next.round_id.as_str())
            .unwrap_or(ctx.round_id.as_str());
        if predecessor.round_id != next_round {
            chain.push_str("-$new-round-> ");
        } else if let Some(direction) = predecessor.branch_direction.as_deref() {
            chain.push_str(&format!("-{direction}-> "));
        } else {
            chain.push_str("-> ");
        }
    }
    chain.push_str(&format!(
        "当前节点({}/{}/{})",
        ctx.round_id, ctx.node_id, ctx.attempt_id
    ));
    chain
}

fn predecessor_reason_lines(
    predecessors: &[PromptPredecessorContext],
    new_round_trigger: Option<&PromptPredecessorContext>,
) -> String {
    let mut lines = predecessors
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
                parts.push(format!(
                    "输出 artifact={}: {}",
                    artifact.name, artifact.path
                ));
                if let Some(preview) = artifact.preview.as_deref() {
                    parts.push(format!("输出预览={}", preview.trim()));
                }
            }
            Some(format!(
                "- {}：{}。",
                predecessor_ref(predecessor),
                parts.join("；")
            ))
        })
        .collect::<Vec<_>>()
        .join("\n");
    if let Some(trigger) = new_round_trigger {
        if !lines.is_empty() {
            lines.push('\n');
        }
        lines.push_str(&new_round_trigger_reason_line(trigger));
    }
    lines
}

fn new_round_trigger_reason_line(trigger: &PromptPredecessorContext) -> String {
    let mut parts = vec![format!(
        "$new-round 由该节点触发；节点类型={}；结果={}",
        trigger.node_type,
        trigger.outcome.as_deref().unwrap_or("unknown")
    )];
    if let Some(reason) = trigger.branch_reason.as_deref() {
        parts.push(reason.to_string());
    }
    if let Some(artifact) = &trigger.output_artifact {
        parts.push(format!(
            "输出 artifact={}: {}",
            artifact.name, artifact.path
        ));
        if let Some(preview) = artifact.preview.as_deref() {
            parts.push(format!("输出预览={}", preview.trim()));
        }
    }
    if !trigger.attachments.is_empty() {
        let files = trigger
            .attachments
            .iter()
            .map(|attachment| format!("attachments/{}", attachment.name))
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!("附件={}", files));
    }
    format!("- {}：{}。", predecessor_ref(trigger), parts.join("；"))
}

fn predecessor_attachment_lines(predecessors: &[PromptPredecessorContext]) -> String {
    let mut seen = IndexMap::<String, Vec<String>>::new();
    for p in predecessors {
        if p.attachments.is_empty() {
            continue;
        }
        let entry = seen
            .entry(format!("{}/{}/{}", p.round_id, p.node_id, p.attempt_id))
            .or_insert_with(Vec::new);
        for a in &p.attachments {
            entry.push(format!("attachments/{}", a.name));
        }
    }
    seen.iter()
        .map(|(locator, files)| format!("- {}: {}", locator, files.join(", ")))
        .collect::<Vec<_>>()
        .join("\n")
}

fn runtime_output_contract_context(
    contract: &PromptOutputContract,
) -> RuntimeOutputContractTemplateContext {
    RuntimeOutputContractTemplateContext {
        artifact: contract.artifact.clone(),
        kind: contract.kind.clone(),
        schema: contract
            .schema_text
            .clone()
            .or_else(|| {
                contract.schema.as_ref().map(|schema| {
                    serde_json::to_string_pretty(schema).expect("serialize output schema")
                })
            })
            .unwrap_or_else(|| "当前节点未声明结构化 schema。".to_string()),
        success_condition: contract.success_condition.clone(),
        finalize_context: contract.finalize_context.clone(),
    }
}

fn log_prompt_bundle(
    prompt: &PromptBundle,
    invocation_kind: InvocationKind,
    profile: Option<&str>,
    output_artifact: Option<&str>,
    cold_artifacts: usize,
    cold_attachments: usize,
    log_prompts: bool,
) {
    debug!(
        invocation_kind = ?invocation_kind,
        profile = ?profile,
        output_artifact = ?output_artifact,
        system_prompt_len = prompt.system_prompt.len(),
        user_prompt_len = prompt.user_prompt.len(),
        cold_artifacts,
        cold_attachments,
        "provider prompt bundle summary"
    );
    if log_prompts {
        debug!(system_prompt = %prompt.system_prompt, user_prompt = %prompt.user_prompt, "provider prompt bundle content");
    }
}

pub fn provider_capabilities(provider_id: &str) -> Result<ProviderCapabilities> {
    let agent_id = ManagedAgentId::from_str(provider_id)?;
    provider_capabilities_for_id(&agent_id)
}

pub fn provider_capabilities_for_id(agent_id: &ManagedAgentId) -> Result<ProviderCapabilities> {
    let config = catalog_agent_default_config(agent_id.as_str()).unwrap_or_else(|| {
        ManagedAgentConfig::new(
            AcpAdapterConfig {
                command: String::new(),
                args: Vec::new(),
                display_name: agent_id.as_str().to_string(),
                env: BTreeMap::new(),
            },
            ".agent",
            Vec::new(),
        )
    });
    let supports_system_prompt = config.supports_system_prompt();
    Ok(AcpProvider::new(
        agent_id.as_str(),
        config.adapter,
        false,
        false,
        false,
        5 * 1024 * 1024,
        4 * 1024 * 1024,
    )
    .with_system_prompt_support(supports_system_prompt)
    .describe_provider()
    .capabilities)
}

pub fn supports_continue_session(provider_id: &str) -> Result<bool> {
    Ok(provider_capabilities(provider_id)?.supports_continue_session)
}

pub fn supports_system_prompt(provider_id: &str) -> Result<bool> {
    Ok(provider_capabilities(provider_id)?.supports_system_prompt)
}

pub fn provider_from_agent(
    agent_id: &ManagedAgentId,
    config: &ManagedAgentConfig,
    use_local_claude: bool,
    require_local_claude_executable: bool,
    acp_session_title_refresh_enabled: bool,
    acp_raw_max_size_bytes: u64,
    acp_raw_target_size_bytes: u64,
    runtime_policy: client::AcpRuntimePolicy,
) -> Result<Box<dyn ProviderAdapter>> {
    Ok(Box::new(
        AcpProvider::new(
            agent_id.as_str(),
            config.adapter.clone(),
            use_local_claude,
            require_local_claude_executable,
            acp_session_title_refresh_enabled,
            acp_raw_max_size_bytes,
            acp_raw_target_size_bytes,
        )
        .with_runtime_policy(
            runtime_policy.with_external_session_sync_enabled(config.external_session_sync_enabled),
        )
        .with_system_prompt_support(config.supports_system_prompt()),
    ))
}

pub fn provider_from_id(
    provider_id: &str,
    use_local_claude: bool,
    require_local_claude_executable: bool,
    acp_session_title_refresh_enabled: bool,
    acp_raw_max_size_bytes: u64,
    acp_raw_target_size_bytes: u64,
) -> Result<Box<dyn ProviderAdapter>> {
    let agent_id = ManagedAgentId::from_str(provider_id)?;
    let config = catalog_agent_default_config(agent_id.as_str())
        .ok_or_else(|| anyhow::anyhow!("Agent `{provider_id}` has no built-in default config"))?;
    provider_from_agent(
        &agent_id,
        &config,
        use_local_claude,
        require_local_claude_executable,
        acp_session_title_refresh_enabled,
        acp_raw_max_size_bytes,
        acp_raw_target_size_bytes,
        client::AcpRuntimePolicy::default(),
    )
}

pub fn default_provider() -> Box<dyn ProviderAdapter> {
    provider_from_id(
        DEFAULT_PROVIDER,
        false,
        false,
        false,
        5 * 1024 * 1024,
        4 * 1024 * 1024,
    )
    .expect("default provider must be supported")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::client::AcpPromptFailure;
    use crate::runtime_error::RecoveryMode;

    fn test_worker_invocation(attempt_dir: Utf8PathBuf) -> WorkerInvocation {
        let runtime_context = PromptRuntimeContext {
            project_id: "project-001".to_string(),
            task_id: "task-001".to_string(),
            run_id: "run-001".to_string(),
            round_id: "round-001".to_string(),
            node_id: "dev".to_string(),
            attempt_id: "attempt-001".to_string(),
            runtime_node_id: None,
            runtime_attempt_id: None,
            attempt_state_file: None,
            language: crate::config::DesktopLanguage::ZhCn,
            run_dir: attempt_dir.join("../../.."),
            round_dir: attempt_dir.join("../.."),
            node_dir: attempt_dir.join(".."),
            attempt_dir: attempt_dir.clone(),
            attachments_dir: attempt_dir.join("attachments"),
            task_inputs_dir: None,
        };
        WorkerInvocation {
            invocation_kind: InvocationKind::WorkerGeneric,
            turn_control_mode: TurnControlMode::RuntimeControlled,
            runtime_control_intent: RuntimeControlIntent::Unchanged,
            prompt_envelope: crate::dsl::PromptEnvelopeMode::RuntimeManaged,
            execution_surface: PromptExecutionSurface::Workflow,
            profile: None,
            profile_content: None,
            profile_dynamic_template: false,
            requirement_path: None,
            requirement_text: Some("Need a structured result".to_string()),
            adapter_workspace_dir: Utf8PathBuf::from("/repo"),
            workspace_dir: Utf8PathBuf::from("/repo"),
            attempt_dir,
            output_contract: None,
            runtime_context,
            predecessors: Vec::new(),
            new_round_trigger: None,
            extra_system_sections: Vec::new(),
            extra_hidden_sections: Vec::new(),
            task_instruction: Some("Create a structured result".to_string()),
            user_tips_instruction: None,
            resume_task_instruction: None,
            session_mode: SessionMode::New,
            user_prompt_render_mode: UserPromptRenderMode::RequirementTask,
            permission_mode: None,
            model: None,
            config_options: Default::default(),
            continue_ref: None,
            resume_prompt: None,
            resume_prompt_id: None,
            prompt_display: None,
            resume_prompt_visibility: PromptVisibility::Visible,
            stream_mode: StreamMode::StreamJson,
            log_prompts: false,
            log_provider_command: false,
            attachments_dir: None,
            cold_artifacts: Vec::new(),
            cold_attachments: Vec::new(),
            task_input_attachment_paths: Vec::new(),
            user_input_attachment_paths: Vec::new(),
            mcp_servers: Vec::new(),
            scheduled_context: None,
        }
    }

    fn test_output_contract(emission_mode: OutputEmissionMode) -> PromptOutputContract {
        PromptOutputContract {
            artifact: "dynamic-node-completion".to_string(),
            kind: "json".to_string(),
            schema: Some(serde_json::json!({
                "type": "object",
                "required": ["status"]
            })),
            schema_text: Some("Return JSON with a required status field.".to_string()),
            success_condition: None,
            finalize_context: None,
            emission_mode,
        }
    }

    #[test]
    fn every_advertised_attachment_extension_is_resolvable() {
        let dir = tempfile::tempdir().unwrap();
        let paths = supported_attachment_extensions()
            .into_iter()
            .map(|extension| {
                let path = dir.path().join(format!("sample.{extension}"));
                std::fs::write(&path, b"{}\n").unwrap();
                path.to_string_lossy().to_string()
            })
            .collect::<Vec<_>>();

        let resolved = resolve_attachments(&paths, "task-inputs").unwrap();

        assert_eq!(resolved.len(), paths.len());
        assert!(resolved.iter().all(|attachment| {
            attachment.meta.path.starts_with("task-inputs/")
                && attachment.meta.mime_type != "application/octet-stream"
        }));
    }

    #[test]
    fn jsonl_attachment_is_projected_as_text_resource_and_message_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("acp.raw.jsonl");
        std::fs::write(&path, b"{\"event\":1}\n{\"event\":2}\n").unwrap();

        let resolved =
            resolve_attachments(&[path.to_string_lossy().to_string()], "task-inputs").unwrap();

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].meta.name, "acp.raw.jsonl");
        assert_eq!(resolved[0].meta.path, "task-inputs/acp.raw.jsonl");
        assert_eq!(resolved[0].meta.mime_type, "application/json");
        assert!(matches!(
            &resolved[0].block,
            AcpContentBlock::Resource(resource)
                if resource.resource.text.contains("{\"event\":2}")
        ));
    }

    #[test]
    fn prompt_bundle_preserves_task_inputs_and_persists_user_inputs_by_scope() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let task_image = root.join("task-image.png");
        let follow_up_image = root.join("follow-up.png");
        let follow_up_text = root.join("notes.txt");
        std::fs::write(task_image.as_std_path(), [1_u8, 2, 3]).unwrap();
        std::fs::write(follow_up_image.as_std_path(), [4_u8, 5, 6]).unwrap();
        std::fs::write(follow_up_text.as_std_path(), "runtime notes").unwrap();
        let attempt_dir = root.join("attempt-001");
        let mut invocation = test_worker_invocation(attempt_dir.clone());
        invocation.task_input_attachment_paths = vec![task_image.to_string()];
        invocation.user_input_attachment_paths =
            vec![follow_up_image.to_string(), follow_up_text.to_string()];

        let prompt = render_prompt_bundle(&invocation).unwrap();

        let paths = prompt
            .attachment_metas
            .iter()
            .map(|attachment| attachment.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                "task-inputs/task-image.png",
                "user-inputs/follow-up.png",
                "user-inputs/notes.txt",
            ]
        );
        assert!(!attempt_dir.join("user-inputs/task-image.png").exists());
        assert_eq!(
            std::fs::read(attempt_dir.join("user-inputs/follow-up.png").as_std_path()).unwrap(),
            [4_u8, 5, 6]
        );
        assert_eq!(
            std::fs::read_to_string(attempt_dir.join("user-inputs/notes.txt").as_std_path())
                .unwrap(),
            "runtime notes"
        );
        assert!(matches!(
            &prompt.content_blocks[1],
            AcpContentBlock::Image(image)
                if image.link.uri.contains("/attempt-001/user-inputs/follow-up.png")
        ));
        assert!(matches!(
            &prompt.content_blocks[2],
            AcpContentBlock::Resource(resource)
                if resource.resource.text == "runtime notes"
                    && resource.link.uri.contains("/attempt-001/user-inputs/notes.txt")
        ));
    }

    #[test]
    fn prepare_acp_mcp_servers_filters_only_explicitly_unsupported_transports() {
        let servers = vec![
            serde_json::json!({"name": "local", "command": "node"}),
            serde_json::json!({"type": "http", "name": "docs", "url": "https://example.com/mcp"}),
            serde_json::json!({"type": "sse", "name": "legacy", "url": "https://example.com/sse"}),
        ];
        let capabilities = serde_json::json!({
            "mcpCapabilities": {
                "http": true,
                "sse": false
            }
        });

        let prepared = prepare_acp_mcp_servers(&servers, Some(&capabilities));

        assert_eq!(prepared.accepted.len(), 2);
        assert_eq!(prepared.skipped.len(), 1);
        assert_eq!(prepared.skipped[0].name, "legacy");
        assert_eq!(prepared.skipped[0].transport, AcpMcpTransport::Sse);
        assert_eq!(prepared.skipped[0].capability, "mcpCapabilities.sse");
    }

    #[test]
    fn prepare_acp_mcp_servers_preserves_servers_when_agent_does_not_declare_capabilities() {
        let servers = vec![serde_json::json!({
            "type": "sse",
            "name": "legacy",
            "url": "https://example.com/sse"
        })];

        let prepared = prepare_acp_mcp_servers(&servers, Some(&serde_json::json!({})));

        assert_eq!(prepared.accepted, servers);
        assert!(prepared.skipped.is_empty());
    }

    #[test]
    fn prepare_acp_mcp_servers_preserves_transport_when_specific_flag_is_not_declared() {
        let servers = vec![serde_json::json!({
            "type": "sse",
            "name": "legacy",
            "url": "https://example.com/sse"
        })];
        let capabilities = serde_json::json!({
            "mcpCapabilities": {
                "http": true
            }
        });

        let prepared = prepare_acp_mcp_servers(&servers, Some(&capabilities));

        assert_eq!(prepared.accepted, servers);
        assert!(prepared.skipped.is_empty());
    }

    #[test]
    fn prepare_acp_mcp_servers_uses_capabilities_instead_of_agent_identity() {
        let servers = vec![
            serde_json::json!({"type": "http", "name": "docs", "url": "https://example.com/mcp"}),
            serde_json::json!({"type": "sse", "name": "legacy", "url": "https://example.com/sse"}),
        ];
        let capabilities = serde_json::json!({
            "mcpCapabilities": {
                "http": false,
                "sse": true
            }
        });

        let prepared = prepare_acp_mcp_servers(&servers, Some(&capabilities));

        assert_eq!(prepared.accepted, vec![servers[1].clone()]);
        assert_eq!(prepared.skipped[0].transport, AcpMcpTransport::Http);
    }

    fn acp_prompt_run(
        stop_reason: Option<&str>,
        terminal_failure: Option<AcpPromptFailure>,
    ) -> client::AcpPromptRun {
        client::AcpPromptRun {
            session_id: "session-1".to_string(),
            adapter_id: "codex-acp".to_string(),
            adapter_display_name: "Codex".to_string(),
            stop_reason: stop_reason.map(str::to_string),
            terminal_failure,
            output: client::AcpPromptOutput::default(),
            restored: false,
            used_tokens: None,
            context_window_size: None,
            total_cost_usd: None,
            input_tokens: None,
            output_tokens: None,
            cached_read_tokens: None,
            cached_write_tokens: None,
            total_tokens: None,
        }
    }

    #[test]
    fn terminal_session_error_overrides_end_turn_without_auto_retry() {
        let run = acp_prompt_run(
            Some("end_turn"),
            Some(AcpPromptFailure {
                code: "acp.session-system-error".to_string(),
                message:
                    "We're currently experiencing high demand, which may cause temporary errors."
                        .to_string(),
                details: Some("Reconnecting... 5/5".to_string()),
                raw: serde_json::json!({ "threadStatus": { "type": "systemError" } }),
            }),
        );

        let outcome = classify_acp_prompt_run(&run);

        assert_eq!(outcome.status, ProviderRunStatus::Failure);
        let error = outcome
            .runtime_error
            .expect("fatal error must be preserved");
        assert_eq!(error.code_str(), "provider.server-unavailable");
        assert_eq!(error.recovery, RecoveryMode::Manual);
        assert!(error.retry_policy.is_none());
    }

    #[test]
    fn end_turn_without_fatal_signal_is_success() {
        let outcome = classify_acp_prompt_run(&acp_prompt_run(Some("end_turn"), None));

        assert_eq!(outcome.status, ProviderRunStatus::Success);
        assert!(outcome.runtime_error.is_none());
    }

    #[test]
    fn post_turn_projection_hides_contract_until_hidden_finalize_turn() {
        let mut req = test_worker_invocation(Utf8PathBuf::from("/run/attempt-001"));
        req.output_contract = Some(test_output_contract(OutputEmissionMode::PostTurnProjection));

        let business_prompt = render_prompt_bundle(&req).unwrap();
        assert_eq!(business_prompt.visibility, PromptVisibility::Visible);
        assert_eq!(business_prompt.hidden_reason, None);
        assert!(business_prompt.system_prompt.contains("隐藏 finalize turn"));
        assert!(
            !business_prompt
                .system_prompt
                .contains("required status field")
        );
        assert!(
            !business_prompt
                .system_prompt
                .contains("dynamic-node-completion")
        );

        let contract = req.output_contract.as_mut().unwrap();
        contract.emission_mode = OutputEmissionMode::InlineControl;
        contract.finalize_context = Some("remaining nodes: 3".to_string());
        let finalize_prompt =
            render_artifact_finalize_prompt(req.runtime_context.language, contract).unwrap();
        req.session_mode = SessionMode::Continue;
        req.user_prompt_render_mode = UserPromptRenderMode::RuntimeFinalize;
        req.resume_prompt_visibility = PromptVisibility::Hidden;
        req.resume_prompt = Some(finalize_prompt);

        let prompt = render_prompt_bundle(&req).unwrap();
        assert_eq!(prompt.visibility, PromptVisibility::Hidden);
        assert_eq!(prompt.hidden_reason.as_deref(), Some("artifactFinalize"));
        assert!(prompt.user_prompt.contains("dynamic-node-completion"));
        assert!(prompt.user_prompt.contains("required status field"));
        assert!(prompt.user_prompt.contains("remaining nodes: 3"));
        assert!(prompt.user_prompt.contains("不要继续执行任务"));

        req.user_prompt_render_mode = UserPromptRenderMode::RuntimeRepair;
        assert_eq!(
            render_prompt_bundle(&req).unwrap().hidden_reason.as_deref(),
            Some("invalidOutputRepair")
        );
    }

    #[test]
    fn runtime_system_exempts_interrupted_free_conversation_from_artifact_semantics() {
        let mut req = test_worker_invocation(Utf8PathBuf::from("/run/attempt-001"));
        req.output_contract = Some(test_output_contract(OutputEmissionMode::InlineControl));

        let zh = render_prompt_bundle(&req).unwrap();
        assert!(zh.system_prompt.contains("用户主动打断当前工作"));
        assert!(
            zh.system_prompt
                .contains("无需遵守本节的 artifact 输出语义")
        );
        assert!(zh.system_prompt.contains("角色预设的执行流程"));
        assert!(
            zh.system_prompt
                .contains("用户指引不得覆盖下方 artifact 输出契约")
        );

        req.runtime_context.language = crate::config::DesktopLanguage::En;
        let en = render_prompt_bundle(&req).unwrap();
        assert!(
            en.system_prompt
                .contains("If the user interrupts the current work")
        );
        assert!(
            en.system_prompt
                .contains("do not need to follow the artifact-output semantics")
        );
        assert!(
            en.system_prompt
                .contains("execution process prescribed by the role")
        );
        assert!(
            en.system_prompt
                .contains("cannot override the artifact output contract")
        );
    }

    #[test]
    fn runtime_resume_remains_a_hidden_control_prompt() {
        let mut req = test_worker_invocation(Utf8PathBuf::from("/run/attempt-001"));
        req.session_mode = SessionMode::Continue;
        req.user_prompt_render_mode = UserPromptRenderMode::RuntimeResume;
        req.resume_prompt_visibility = PromptVisibility::Hidden;
        req.resume_prompt =
            Some("resume runtime control with the user's latest task instructions".to_string());

        let prompt = render_prompt_bundle(&req).unwrap();

        assert_eq!(prompt.visibility, PromptVisibility::Hidden);
        assert_eq!(
            prompt.hidden_reason.as_deref(),
            Some("runtimeControlResume")
        );
        assert_eq!(
            prompt.user_prompt,
            "resume runtime control with the user's latest task instructions"
        );
    }

    #[test]
    fn durable_finalizing_state_routes_resume_by_prompt_semantics() {
        let temp = tempfile::tempdir().unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let mut req = test_worker_invocation(attempt_dir.clone());

        assert_eq!(
            prepare_post_turn_projection(&req).unwrap(),
            PostTurnProjectionEntry::RunBusinessTurn
        );
        write_artifact_emission_phase(&attempt_dir, ArtifactEmissionPhase::Finalizing).unwrap();
        req.user_prompt_render_mode = UserPromptRenderMode::RuntimeResume;
        assert_eq!(
            prepare_post_turn_projection(&req).unwrap(),
            PostTurnProjectionEntry::ResumeFinalization
        );
        assert!(post_turn_projection_checkpoint_is_finalizing(&attempt_dir).unwrap());

        let mut repair_req = test_worker_invocation(attempt_dir.join("repair"));
        repair_req.user_prompt_render_mode = UserPromptRenderMode::RuntimeRepair;
        assert_eq!(
            prepare_post_turn_projection(&repair_req).unwrap(),
            PostTurnProjectionEntry::ResumeFinalization
        );
    }

    #[test]
    fn continue_with_message_at_finalize_boundary_reopens_a_durable_business_turn() {
        let temp = tempfile::tempdir().unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        write_artifact_emission_phase(&attempt_dir, ArtifactEmissionPhase::Finalizing).unwrap();

        let mut user_message_req = test_worker_invocation(attempt_dir.clone());
        user_message_req.session_mode = SessionMode::Continue;
        user_message_req.user_prompt_render_mode = UserPromptRenderMode::UserMessage;
        user_message_req.resume_prompt = Some("user message\n<hidden>resume</hidden>".to_string());

        assert_eq!(
            prepare_post_turn_projection(&user_message_req).unwrap(),
            PostTurnProjectionEntry::RunBusinessTurn
        );
        assert!(!post_turn_projection_checkpoint_is_finalizing(&attempt_dir).unwrap());
        let state = artifact_emission_checkpoint(&attempt_dir)
            .unwrap()
            .expect("continue-with-message persists the business checkpoint");
        assert_eq!(state.phase, ArtifactEmissionPhase::BusinessTurn);

        let mut pure_resume_req = test_worker_invocation(attempt_dir.clone());
        pure_resume_req.session_mode = SessionMode::Continue;
        pure_resume_req.user_prompt_render_mode = UserPromptRenderMode::RuntimeResume;
        pure_resume_req.resume_prompt = Some("resume runtime control".to_string());
        assert_eq!(
            prepare_post_turn_projection(&pure_resume_req).unwrap(),
            PostTurnProjectionEntry::RunBusinessTurn
        );

        write_artifact_emission_phase(&attempt_dir, ArtifactEmissionPhase::Finalizing).unwrap();
        assert!(post_turn_projection_checkpoint_is_finalizing(&attempt_dir).unwrap());
    }

    #[test]
    fn invalid_artifact_emission_state_never_replays_business_work() {
        let temp = tempfile::tempdir().unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        std::fs::write(
            attempt_dir.join(ARTIFACT_EMISSION_STATE_FILE),
            "{ invalid state",
        )
        .unwrap();
        let req = test_worker_invocation(attempt_dir);

        let error = prepare_post_turn_projection(&req).unwrap_err();
        let info = crate::runtime_error::normalize_runtime_error(&error);

        assert!(error.to_string().contains("artifact emission state"));
        assert_eq!(info.code_str(), "runtime.artifact-emission-state-invalid");
        assert_eq!(info.recovery, RecoveryMode::Blocked);
    }

    #[test]
    fn anonymous_only_end_turn_remains_provider_success() {
        let mut run = acp_prompt_run(Some("end_turn"), None);
        run.output.visible_text = "unexpected status 502 Bad Gateway".to_string();
        run.output.recent_messages = vec![client::AcpPromptMessageOutput {
            text: run.output.visible_text.clone(),
            has_stable_id: false,
        }];

        let outcome = classify_acp_prompt_run(&run);

        assert_eq!(outcome.status, ProviderRunStatus::Success);
        assert!(outcome.runtime_error.is_none());
    }

    #[test]
    fn unknown_or_missing_stop_reason_is_protocol_failure() {
        for stop_reason in [Some("future_reason"), None] {
            let outcome = classify_acp_prompt_run(&acp_prompt_run(stop_reason, None));

            assert_eq!(outcome.status, ProviderRunStatus::Failure);
            let error = outcome
                .runtime_error
                .expect("unknown terminal state must not become success");
            assert_eq!(error.recovery, RecoveryMode::Manual);
            assert_eq!(error.code_str(), "provider.acp-error");
        }
    }

    #[test]
    fn incomplete_and_interactive_stop_reasons_are_not_success() {
        assert_eq!(
            classify_acp_prompt_run(&acp_prompt_run(Some("max_tokens"), None)).status,
            ProviderRunStatus::Interrupted
        );
        assert_eq!(
            classify_acp_prompt_run(&acp_prompt_run(Some("permission_requested"), None)).status,
            ProviderRunStatus::PermissionRequested
        );
    }

    #[test]
    fn extracts_generic_thought_level_select_option_without_hardcoded_id() {
        let capabilities = serde_json::json!({
            "configOptions": [{
                "id": "reasoning_effort",
                "category": "thought_level",
                "type": "select",
                "currentValue": "high",
                "options": [
                    { "value": "low", "name": "Low" },
                    { "value": "high", "name": "High", "description": "More reasoning" }
                ]
            }]
        });

        let options = select_config_options_from_capabilities(Some(&capabilities));
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].id, "reasoning_effort");
        assert_eq!(options[0].category.as_deref(), Some("thought_level"));
        assert_eq!(options[0].current_value.as_deref(), Some("high"));
        assert_eq!(options[0].options[1].value, "high");
    }

    #[test]
    fn render_prompt_bundle_does_not_add_builtin_output_contracts() {
        let runtime_context = PromptRuntimeContext {
            project_id: "project-001".to_string(),
            task_id: "task-001".to_string(),
            run_id: "run-001".to_string(),
            round_id: "round-001".to_string(),
            node_id: "dev".to_string(),
            attempt_id: "attempt-001".to_string(),
            runtime_node_id: None,
            runtime_attempt_id: None,
            attempt_state_file: None,
            language: crate::config::DesktopLanguage::ZhCn,
            run_dir: Utf8PathBuf::from("/run"),
            round_dir: Utf8PathBuf::from("/run/rounds/round-001"),
            node_dir: Utf8PathBuf::from("/run/rounds/round-001/nodes/dev"),
            attempt_dir: Utf8PathBuf::from("/run/rounds/round-001/nodes/dev/attempt-001"),
            attachments_dir: Utf8PathBuf::from(
                "/run/rounds/round-001/nodes/dev/attempt-001/attachments",
            ),
            task_inputs_dir: None,
        };
        let mut req = WorkerInvocation {
            invocation_kind: InvocationKind::WorkerGeneric,
            turn_control_mode: TurnControlMode::RuntimeControlled,
            runtime_control_intent: RuntimeControlIntent::Unchanged,
            prompt_envelope: crate::dsl::PromptEnvelopeMode::RuntimeManaged,
            execution_surface: PromptExecutionSurface::Workflow,
            profile: None,
            profile_content: None,
            profile_dynamic_template: false,
            requirement_path: None,
            requirement_text: Some("Need a structured result".to_string()),
            adapter_workspace_dir: Utf8PathBuf::from("/repo"),
            workspace_dir: Utf8PathBuf::from("/repo"),
            attempt_dir: runtime_context.attempt_dir.clone(),
            output_contract: None,
            runtime_context,
            predecessors: Vec::new(),
            new_round_trigger: None,
            extra_system_sections: Vec::new(),
            extra_hidden_sections: Vec::new(),
            task_instruction: Some("Create a structured result".to_string()),
            user_tips_instruction: None,
            resume_task_instruction: None,
            session_mode: SessionMode::New,
            user_prompt_render_mode: UserPromptRenderMode::RequirementTask,
            permission_mode: None,
            model: None,
            config_options: Default::default(),
            continue_ref: None,
            resume_prompt: None,
            resume_prompt_id: None,
            prompt_display: None,
            resume_prompt_visibility: PromptVisibility::Visible,
            stream_mode: StreamMode::StreamJson,
            log_prompts: false,
            log_provider_command: false,
            attachments_dir: None,
            cold_artifacts: Vec::new(),
            cold_attachments: Vec::new(),
            task_input_attachment_paths: Vec::new(),
            user_input_attachment_paths: Vec::new(),
            mcp_servers: Vec::new(),
            scheduled_context: None,
        };

        let prompt = render_prompt_bundle(&req).unwrap();
        assert!(!prompt.system_prompt.contains("Output contract"));
        assert_eq!(prompt.prompt_id, None);

        req.resume_prompt_id = Some("logical-turn-001".to_string());
        let prompt = render_prompt_bundle(&req).unwrap();
        assert_eq!(prompt.prompt_id.as_deref(), Some("logical-turn-001"));

        req.prompt_envelope = crate::dsl::PromptEnvelopeMode::RawAgent;
        req.requirement_text = Some("  original direct prompt\n".to_string());
        req.profile_content = Some("PROFILE MUST NOT LEAK".to_string());
        req.task_instruction = Some("RUNTIME MUST NOT LEAK".to_string());
        req.extra_system_sections = vec!["SYSTEM MUST NOT LEAK".to_string()];
        req.extra_hidden_sections = vec![PromptHiddenSection {
            title: "hidden".to_string(),
            content: "HIDDEN MUST NOT LEAK".to_string(),
        }];
        let prompt = render_prompt_bundle(&req).unwrap();
        assert_eq!(prompt.system_prompt, "");
        assert_eq!(prompt.user_prompt, "  original direct prompt\n");

        req.session_mode = SessionMode::Continue;
        req.user_prompt_render_mode = UserPromptRenderMode::UserMessage;
        req.resume_prompt = Some("  follow-up direct prompt\n".to_string());
        let prompt = render_prompt_bundle(&req).unwrap();
        assert_eq!(prompt.system_prompt, "");
        assert_eq!(prompt.user_prompt, "  follow-up direct prompt\n");
        assert!(!prompt.user_prompt.contains("MUST NOT LEAK"));
    }

    #[test]
    fn output_contract_without_final_content_does_not_create_empty_artifact_payload() {
        let contract = PromptOutputContract {
            artifact: "dynamic-node-completion".to_string(),
            kind: "json".to_string(),
            schema: None,
            schema_text: None,
            success_condition: None,
            finalize_context: None,
            emission_mode: OutputEmissionMode::InlineControl,
        };

        let payload =
            output_artifact_payload_from_run(&contract, &client::AcpPromptOutput::default());

        assert!(payload.unwrap().is_none());
    }

    #[test]
    fn output_contract_uses_stable_terminal_message() {
        let contract = PromptOutputContract {
            artifact: "dynamic-node-completion".to_string(),
            kind: "json".to_string(),
            schema: None,
            schema_text: None,
            success_condition: None,
            finalize_context: None,
            emission_mode: OutputEmissionMode::InlineControl,
        };
        let json = r#"{"kind":"dynamic-node-completion","status":"success"}"#;
        let output = client::AcpPromptOutput {
            visible_text: format!("planning text{json}"),
            recent_messages: vec![client::AcpPromptMessageOutput {
                text: json.to_string(),
                has_stable_id: true,
            }],
            observed_stable_message: true,
        };

        let payload = output_artifact_payload_from_run(&contract, &output)
            .unwrap()
            .unwrap();

        let artifact = payload.output_artifact.unwrap();
        assert_eq!(artifact.name, "dynamic-node-completion");
        assert_eq!(
            artifact.content,
            r#"{"kind":"dynamic-node-completion","status":"success"}"#
        );
    }

    #[test]
    fn output_contract_without_message_id_uses_visible_json_candidate() {
        let contract = PromptOutputContract {
            artifact: "accept-result".to_string(),
            kind: "json".to_string(),
            schema: None,
            schema_text: None,
            success_condition: None,
            finalize_context: None,
            emission_mode: OutputEmissionMode::InlineControl,
        };
        let json = r#"{"reason":"验收通过","result":true}"#;
        let output = client::AcpPromptOutput {
            visible_text: json.to_string(),
            recent_messages: vec![client::AcpPromptMessageOutput {
                text: json.to_string(),
                has_stable_id: false,
            }],
            ..Default::default()
        };

        let payload = output_artifact_payload_from_run(&contract, &output)
            .expect("anonymous ACP text is delegated to Runtime artifact validation")
            .expect("valid anonymous JSON creates an artifact candidate");

        assert_eq!(payload.output_artifact.unwrap().content, json);
    }

    #[test]
    fn anonymous_terminal_message_after_stable_output_is_manual_runtime_error() {
        let contract = test_output_contract(OutputEmissionMode::InlineControl);
        let output = client::AcpPromptOutput {
            visible_text: r#"{"status":"success"}unexpected status 502 Bad Gateway"#.to_string(),
            recent_messages: vec![
                client::AcpPromptMessageOutput {
                    text: r#"{"status":"success"}"#.to_string(),
                    has_stable_id: false,
                },
                client::AcpPromptMessageOutput {
                    text: "unexpected status 502 Bad Gateway".to_string(),
                    has_stable_id: false,
                },
            ],
            observed_stable_message: true,
        };

        let error = output_artifact_payload_from_run(&contract, &output)
            .expect_err("anonymous terminal message must not trigger artifact repair");

        assert_eq!(
            error.code_str(),
            "provider.acp-terminal-message-unidentified"
        );
        assert_eq!(error.recovery, RecoveryMode::Manual);
        assert!(error.retry_policy.is_none());
    }

    #[test]
    fn stable_terminal_message_scans_backward_into_anonymous_message() {
        let contract = test_output_contract(OutputEmissionMode::InlineControl);
        let output = client::AcpPromptOutput {
            visible_text: r#"{"status":"success"}final explanation without JSON"#.to_string(),
            recent_messages: vec![
                client::AcpPromptMessageOutput {
                    text: r#"{"status":"success"}"#.to_string(),
                    has_stable_id: true,
                },
                client::AcpPromptMessageOutput {
                    text: "final explanation without JSON".to_string(),
                    has_stable_id: true,
                },
            ],
            observed_stable_message: true,
        };

        let payload = output_artifact_payload_from_run(&contract, &output)
            .unwrap()
            .expect("earlier stable message is inside the three-message search window");

        assert_eq!(
            payload.output_artifact.unwrap().content,
            r#"{"status":"success"}"#
        );
    }

    #[test]
    fn stable_terminal_message_scans_at_most_three_messages() {
        let contract = test_output_contract(OutputEmissionMode::InlineControl);
        let output = client::AcpPromptOutput {
            visible_text: String::new(),
            recent_messages: vec![
                client::AcpPromptMessageOutput {
                    text: r#"{"status":"success"}"#.to_string(),
                    has_stable_id: true,
                },
                client::AcpPromptMessageOutput {
                    text: "one".to_string(),
                    has_stable_id: true,
                },
                client::AcpPromptMessageOutput {
                    text: "two".to_string(),
                    has_stable_id: true,
                },
                client::AcpPromptMessageOutput {
                    text: "three".to_string(),
                    has_stable_id: true,
                },
            ],
            observed_stable_message: true,
        };

        let payload = output_artifact_payload_from_run(&contract, &output).unwrap();

        assert!(payload.is_none());
    }

    #[test]
    fn json_output_contract_without_json_does_not_promote_text_artifact() {
        let contract = PromptOutputContract {
            artifact: "accept-result".to_string(),
            kind: "json".to_string(),
            schema: None,
            schema_text: None,
            success_condition: None,
            finalize_context: None,
            emission_mode: OutputEmissionMode::InlineControl,
        };

        let text = "I can see the requirement.";
        let output = client::AcpPromptOutput {
            visible_text: text.to_string(),
            recent_messages: vec![client::AcpPromptMessageOutput {
                text: text.to_string(),
                has_stable_id: false,
            }],
            ..Default::default()
        };
        let payload = output_artifact_payload_from_run(&contract, &output).unwrap();

        assert!(payload.is_none());
    }

    #[test]
    fn default_provider_is_acp_only() {
        let info = default_provider().describe_provider();
        assert_eq!(info.provider_id, "claude-acp");
        assert!(info.capabilities.supports_continue_session);
        assert!(info.capabilities.supports_system_prompt);
        assert!(!info.capabilities.supports_raw_stream);
    }

    #[test]
    fn codex_provider_does_not_support_system_prompt() {
        let capabilities = provider_capabilities("codex-acp").unwrap();
        assert!(capabilities.supports_continue_session);
        assert!(!capabilities.supports_system_prompt);
    }
}
