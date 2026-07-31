use crate::acp::{client, events::AcpUiEvent};
use crate::artifacts::{artifact_uses_json_output, json_artifact_text_from_outputs};
use crate::config::{AcpAdapterConfig, ManagedAgentConfig, ManagedAgentId, managed_agent_preset};
pub use crate::domain::SessionRef;
use crate::domain::{DEFAULT_PROVIDER, InvocationKind, SessionMode};
use crate::prompts::{
    PromptExecutionSurface, RUNTIME_HIDDEN_CONTEXT_EN, RUNTIME_HIDDEN_CONTEXT_ZH_CN,
    RUNTIME_SYSTEM_EN, RUNTIME_SYSTEM_ZH_CN, RUNTIME_USER_EN, RUNTIME_USER_ZH_CN,
    profile_template_context, prompt_by_language, render as render_template,
};
use crate::runtime_error::{RuntimeErrorInfo, normalize_provider_runtime_failure};
use crate::storage::active_storage_path_config;
use anyhow::{Result, bail, ensure};
use camino::Utf8PathBuf;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::str::FromStr;
use tracing::debug;

use crate::acp::events::AttachmentMeta;

/// Content block types for ACP session/prompt requests.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AcpContentBlock {
    Image(AcpImageBlock),
    Resource(AcpResourceBlock),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpImageBlock {
    pub data: String,
    pub mime_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpResourceBlock {
    pub resource: AcpTextResourceContents,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpTextResourceContents {
    pub text: String,
    pub uri: String,
}

/// Resolved attachment ready to be sent to ACP.
#[derive(Debug, Clone)]
pub struct ResolvedAttachment {
    pub meta: AttachmentMeta,
    pub block: AcpContentBlock,
}

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
    RuntimeRepair,
    UserMessage,
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
    #[serde(default)]
    pub input_attachment_paths: Vec<String>,
    #[serde(default)]
    pub mcp_servers: Vec<serde_json::Value>,
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
    pub prompt_id: Option<String>,
    pub visibility: PromptVisibility,
    pub attachment_metas: Vec<AttachmentMeta>,
    pub content_blocks: Vec<AcpContentBlock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PromptVisibility {
    Visible,
    Hidden,
}

/// Resolve file paths into ResolvedAttachment structs.
/// For images: base64-encode and produce an AcpContentBlock::Image.
/// For text files: read as UTF-8 and produce an AcpContentBlock::Resource.
/// Other files are skipped.
pub fn resolve_attachments(
    paths: &[String],
    storage_prefix: &str,
) -> Result<Vec<ResolvedAttachment>> {
    let mut resolved = Vec::new();
    for path_str in paths {
        let std_path = std::path::Path::new(path_str);
        let name = std_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        let data = std::fs::read(std_path)?;
        let size = data.len() as u64;
        let ext = std_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let is_image = matches!(
            ext.as_str(),
            "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp"
        );
        let mime_type = mime_for_ext(&ext);

        if is_image {
            let b64 = base64_encode(&data);
            let path_for_storage = format!("{}/{}", storage_prefix, name);
            resolved.push(ResolvedAttachment {
                meta: AttachmentMeta {
                    name: name.clone(),
                    path: path_for_storage,
                    mime_type,
                    size,
                },
                block: AcpContentBlock::Image(AcpImageBlock {
                    data: b64,
                    mime_type: mime_for_ext(&ext),
                    uri: Some(format!("file://{}", path_str.replace('\\', "/"))),
                }),
            });
        } else if is_text_ext(&ext) {
            let text = String::from_utf8(data).unwrap_or_else(|_| "[binary file]".to_string());
            let path_for_storage = format!("{}/{}", storage_prefix, name);
            resolved.push(ResolvedAttachment {
                meta: AttachmentMeta {
                    name: name.clone(),
                    path: path_for_storage,
                    mime_type,
                    size,
                },
                block: AcpContentBlock::Resource(AcpResourceBlock {
                    resource: AcpTextResourceContents {
                        text,
                        uri: format!("file://{}", path_str.replace('\\', "/")),
                    },
                }),
            });
        }
        // Non-image, non-text files are skipped for now
    }
    Ok(resolved)
}

/// Returns the set of file extensions supported as attachments.
/// This is the single source of truth — the frontend queries it via Tauri command.
pub fn supported_attachment_extensions() -> Vec<&'static str> {
    vec![
        "png", "jpg", "jpeg", "webp", "gif", "bmp", "txt", "md", "markdown", "json", "jsonl",
        "csv", "html", "htm", "css", "js", "ts", "tsx", "jsx", "rs", "py", "go", "java", "c", "h",
        "cpp", "hpp", "yaml", "yml", "xml", "toml", "log", "sql", "sh", "bash", "zsh",
    ]
}

fn mime_for_ext(ext: &str) -> String {
    match ext {
        "png" => "image/png".to_string(),
        "jpg" | "jpeg" => "image/jpeg".to_string(),
        "webp" => "image/webp".to_string(),
        "gif" => "image/gif".to_string(),
        "bmp" => "image/bmp".to_string(),
        "txt" => "text/plain".to_string(),
        "md" | "markdown" => "text/markdown".to_string(),
        "json" => "application/json".to_string(),
        "csv" => "text/csv".to_string(),
        "html" | "htm" => "text/html".to_string(),
        "css" => "text/css".to_string(),
        "js" => "text/javascript".to_string(),
        "ts" => "text/typescript".to_string(),
        "tsx" => "text/typescript".to_string(),
        "jsx" => "text/javascript".to_string(),
        "rs" => "text/rust".to_string(),
        "py" => "text/python".to_string(),
        "go" => "text/go".to_string(),
        "java" => "text/java".to_string(),
        "c" | "h" => "text/c".to_string(),
        "cpp" | "hpp" => "text/cpp".to_string(),
        "yaml" | "yml" => "text/yaml".to_string(),
        "xml" => "text/xml".to_string(),
        "toml" => "text/toml".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

fn is_text_ext(ext: &str) -> bool {
    matches!(
        ext,
        "txt"
            | "md"
            | "markdown"
            | "json"
            | "csv"
            | "html"
            | "htm"
            | "css"
            | "js"
            | "ts"
            | "tsx"
            | "jsx"
            | "rs"
            | "py"
            | "go"
            | "java"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "yaml"
            | "yml"
            | "xml"
            | "toml"
            | "log"
            | "sql"
            | "sh"
            | "bash"
            | "zsh"
    )
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
    ) -> Result<ProviderRunResult> {
        self.run_worker_with_live_update(req, live_update)
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
        }
    }

    pub fn with_runtime_policy(mut self, runtime_policy: client::AcpRuntimePolicy) -> Self {
        self.runtime_policy = runtime_policy;
        self
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
                supports_system_prompt: self.provider_id == "claude-acp",
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
        match client::doctor(
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
        self.run_worker_with_callbacks(req, live_update, None)
    }

    fn run_worker_with_callbacks(
        &self,
        req: WorkerInvocation,
        live_update: Option<AcpLiveUpdate<'_>>,
        session_update: Option<AcpSessionUpdate<'_>>,
    ) -> Result<ProviderRunResult> {
        let prompt = render_prompt_bundle(&req)?;
        log_prompt_bundle(
            &prompt,
            req.invocation_kind,
            req.profile.as_deref(),
            req.output_contract
                .as_ref()
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
            }),
        )?;
        let terminal = classify_acp_prompt_run(&run);
        let result_payload = matches!(
            terminal.status,
            ProviderRunStatus::Success | ProviderRunStatus::Interrupted
        )
        .then(|| {
            req.output_contract.as_ref().and_then(|contract| {
                output_artifact_payload_from_run(contract, &run.final_outputs, &run.final_text)
            })
        })
        .flatten();
        Ok(ProviderRunResult {
            status: terminal.status,
            exit_code: None,
            result_payload,
            worker_ref_seed: None,
            stream_path: None,
            runtime_error: terminal.runtime_error,
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

fn classify_acp_prompt_run(run: &client::AcpPromptRun) -> ProviderTerminalOutcome {
    if let Some(failure) = &run.terminal_failure {
        return ProviderTerminalOutcome {
            status: ProviderRunStatus::Failure,
            runtime_error: Some(normalize_provider_runtime_failure(
                run.stop_reason.as_deref(),
                failure.diagnostic(),
                Some(serde_json::json!({
                    "adapterId": run.adapter_id,
                    "adapterDisplayName": run.adapter_display_name,
                    "stopReason": run.stop_reason,
                    "terminalFailure": failure,
                })),
            )),
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
                run.final_text.clone(),
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
    final_outputs: &[String],
    final_text: &str,
) -> Option<ProviderResultPayload> {
    let uses_json_output = contract.kind == "json" || artifact_uses_json_output(&contract.artifact);
    let content = if uses_json_output {
        json_artifact_text_from_outputs(final_outputs, final_text)
    } else {
        non_empty_artifact_text(final_text)
    }?;

    Some(ProviderResultPayload {
        output_artifact: Some(OutputArtifactPayload {
            name: contract.artifact.clone(),
            content,
        }),
    })
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
        | UserPromptRenderMode::RuntimeRepair
        | UserPromptRenderMode::UserMessage => String::new(),
    };

    let (system_prompt, user_prompt) = match req.prompt_envelope {
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
    let is_continue = matches!(req.session_mode, SessionMode::Continue);

    // Resolve task input attachments
    let mut attachment_metas = Vec::new();
    let mut content_blocks = Vec::new();
    if !req.input_attachment_paths.is_empty() {
        if let Ok(resolved) = resolve_attachments(&req.input_attachment_paths, "task-inputs") {
            for r in resolved {
                attachment_metas.push(r.meta);
                content_blocks.push(r.block);
            }
        }
    }

    Ok(PromptBundle {
        system_prompt,
        user_prompt,
        prompt_id: if is_continue {
            req.resume_prompt_id.clone()
        } else {
            None
        },
        visibility: if is_continue {
            req.resume_prompt_visibility
        } else {
            PromptVisibility::Visible
        },
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
        UserPromptRenderMode::UserMessage | UserPromptRenderMode::RuntimeRepair => req
            .resume_prompt
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_string(),
        UserPromptRenderMode::WorkflowResume | UserPromptRenderMode::RequirementTask => {
            let hidden_context = render_hidden_context(req);
            let continue_goal = matches!(req.user_prompt_render_mode, UserPromptRenderMode::WorkflowResume).then(|| {
                match req.runtime_context.language {
                    crate::config::DesktopLanguage::ZhCn => "根据最新反馈进行调整，确保后续节点能够成功；如果当前节点有输出格式要求，仍然严格按 system prompt 中的输出约束输出。".to_string(),
                    crate::config::DesktopLanguage::En => "Adjust according to the latest feedback and ensure downstream nodes can succeed. If this node has output format requirements, still strictly follow the output contract in the system prompt.".to_string(),
                }
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
    gold_band_hidden_block("Gold Band runtime context", &content)
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
}

fn runtime_system_context(req: &WorkerInvocation) -> Result<RuntimePromptTemplateContext> {
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
                    req.output_contract.is_some(),
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
        output_contract: req
            .output_contract
            .as_ref()
            .map(runtime_output_contract_context),
    })
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
    let preset = managed_agent_preset(agent_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported managed agent: {}", agent_id.as_str()))?;
    Ok(AcpProvider::new(
        agent_id.as_str(),
        preset.default_config().adapter,
        false,
        false,
        false,
        5 * 1024 * 1024,
        4 * 1024 * 1024,
    )
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
    managed_agent_preset(agent_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported managed agent: {}", agent_id.as_str()))?;
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
        ),
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
    let preset = managed_agent_preset(&agent_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported managed agent: {provider_id}"))?;
    let config = preset.default_config();
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
            final_text: String::new(),
            final_outputs: Vec::new(),
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
    fn fatal_session_error_overrides_end_turn_success_reason() {
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
        assert_eq!(error.recovery, RecoveryMode::Auto);
    }

    #[test]
    fn end_turn_without_fatal_signal_is_success() {
        let outcome = classify_acp_prompt_run(&acp_prompt_run(Some("end_turn"), None));

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
            resume_prompt_visibility: PromptVisibility::Visible,
            stream_mode: StreamMode::StreamJson,
            log_prompts: false,
            log_provider_command: false,
            attachments_dir: None,
            cold_artifacts: Vec::new(),
            cold_attachments: Vec::new(),
            input_attachment_paths: Vec::new(),
            mcp_servers: Vec::new(),
        };

        let prompt = render_prompt_bundle(&req).unwrap();
        assert!(!prompt.system_prompt.contains("Output contract"));

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
        };

        let payload = output_artifact_payload_from_run(&contract, &[], "");

        assert!(payload.is_none());
    }

    #[test]
    fn output_contract_with_json_final_output_creates_artifact_payload() {
        let contract = PromptOutputContract {
            artifact: "dynamic-node-completion".to_string(),
            kind: "json".to_string(),
            schema: None,
            schema_text: None,
            success_condition: None,
        };
        let outputs = vec![
            "planning text".to_string(),
            r#"{"kind":"dynamic-node-completion","status":"success"}"#.to_string(),
        ];

        let payload = output_artifact_payload_from_run(&contract, &outputs, "").unwrap();

        let artifact = payload.output_artifact.unwrap();
        assert_eq!(artifact.name, "dynamic-node-completion");
        assert_eq!(
            artifact.content,
            r#"{"kind":"dynamic-node-completion","status":"success"}"#
        );
    }

    #[test]
    fn json_output_contract_without_json_does_not_fallback_to_text_artifact() {
        let contract = PromptOutputContract {
            artifact: "accept-result".to_string(),
            kind: "json".to_string(),
            schema: None,
            schema_text: None,
            success_condition: None,
        };

        let payload = output_artifact_payload_from_run(
            &contract,
            &["I can see the requirement.".to_string()],
            "I can see the requirement.",
        );

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
