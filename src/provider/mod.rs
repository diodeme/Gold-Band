use crate::acp::{client, events::AcpUiEvent};
use crate::artifacts::{artifact_uses_json_output, json_artifact_text_from_outputs};
use crate::config::{AcpAdapterConfig, ManagedAgentConfig, ManagedAgentType};
pub use crate::domain::SessionRef;
use crate::domain::{DEFAULT_PROVIDER, InvocationKind, SessionMode};
use crate::prompts::{
    RUNTIME_SYSTEM_EN, RUNTIME_SYSTEM_ZH_CN, prompt_by_language, render as render_template,
};
use anyhow::{Result, bail, ensure};
use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    pub supports_raw_stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpModeOption {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerInvocation {
    pub invocation_kind: InvocationKind,
    pub profile: Option<String>,
    pub profile_content: Option<String>,
    pub requirement_path: Option<Utf8PathBuf>,
    pub requirement_text: Option<String>,
    pub workspace_dir: Utf8PathBuf,
    pub attempt_dir: Utf8PathBuf,
    pub output_contract: Option<PromptOutputContract>,
    pub runtime_context: PromptRuntimeContext,
    pub predecessors: Vec<PromptPredecessorContext>,
    #[serde(default)]
    pub extra_system_sections: Vec<String>,
    pub task_instruction: Option<String>,
    pub session_mode: SessionMode,
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptRuntimeContext {
    pub project_id: String,
    pub task_id: String,
    pub run_id: String,
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
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
    acp_session_title_refresh_enabled: bool,
    acp_raw_max_size_bytes: u64,
    acp_raw_target_size_bytes: u64,
}

impl AcpProvider {
    pub fn new(
        provider_id: impl Into<String>,
        adapter_config: AcpAdapterConfig,
        use_local_claude: bool,
        acp_session_title_refresh_enabled: bool,
        acp_raw_max_size_bytes: u64,
        acp_raw_target_size_bytes: u64,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            adapter_config,
            use_local_claude,
            acp_session_title_refresh_enabled,
            acp_raw_max_size_bytes,
            acp_raw_target_size_bytes,
        }
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
        match client::doctor(&self.adapter_config, cwd, self.use_local_claude) {
            Ok(capabilities) => DoctorResult {
                available: true,
                reason: None,
                capabilities: Some(capabilities),
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
            req.workspace_dir.clone(),
            req.attempt_dir.clone(),
            &prompt,
            req.session_mode,
            req.permission_mode.clone(),
            req.model.clone(),
            req.continue_ref.clone(),
            self.use_local_claude,
            self.acp_session_title_refresh_enabled,
            self.acp_raw_max_size_bytes,
            self.acp_raw_target_size_bytes,
            live_update,
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
        let result_payload = req.output_contract.as_ref().map(|contract| {
            let uses_json_output =
                contract.kind == "json" || artifact_uses_json_output(&contract.artifact);
            let content = if uses_json_output {
                json_artifact_text_from_outputs(&run.final_outputs, &run.final_text)
                    .unwrap_or_else(|| run.final_text.clone())
            } else {
                run.final_text.clone()
            };
            ProviderResultPayload {
                output_artifact: Some(OutputArtifactPayload {
                    name: contract.artifact.clone(),
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
                system_prompt: render_system_prompt(req),
                user_prompt: resume_prompt.clone(),
                prompt_id: req.resume_prompt_id.clone(),
                visibility: req.resume_prompt_visibility,
                attachment_metas: Vec::new(),
                content_blocks: Vec::new(),
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
        user_prompt: user_sections.join("\n\n"),
        prompt_id: None,
        visibility: PromptVisibility::Visible,
        attachment_metas,
        content_blocks,
    })
}

fn render_system_prompt(req: &WorkerInvocation) -> String {
    render_template(
        prompt_by_language(
            req.runtime_context.language,
            RUNTIME_SYSTEM_ZH_CN,
            RUNTIME_SYSTEM_EN,
        ),
        runtime_system_context(req),
    )
    .expect("prompt template renders")
}

#[derive(Serialize)]
struct RuntimePromptTemplateContext {
    project_id: String,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    attempt_dir: String,
    attachments_dir: String,
    run_dir: String,
    node_dir: String,
    predecessors: RuntimePredecessorTemplateContext,
    extra_system_sections: Option<String>,
    profile: RuntimeProfileTemplateContext,
    output_contract: Option<RuntimeOutputContractTemplateContext>,
}

#[derive(Serialize)]
struct RuntimePredecessorTemplateContext {
    is_empty: bool,
    chain: String,
    reason_lines: String,
    reason_lines_empty: bool,
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

fn runtime_system_context(req: &WorkerInvocation) -> RuntimePromptTemplateContext {
    RuntimePromptTemplateContext {
        project_id: req.runtime_context.project_id.clone(),
        task_id: req.runtime_context.task_id.clone(),
        run_id: req.runtime_context.run_id.clone(),
        round_id: req.runtime_context.round_id.clone(),
        node_id: req.runtime_context.node_id.clone(),
        attempt_id: req.runtime_context.attempt_id.clone(),
        attempt_dir: req.runtime_context.attempt_dir.to_string(),
        attachments_dir: req.runtime_context.attachments_dir.to_string(),
        run_dir: req.runtime_context.run_dir.to_string(),
        node_dir: req.runtime_context.node_dir.to_string(),
        predecessors: runtime_predecessor_context(&req.predecessors, &req.runtime_context),
        extra_system_sections: {
            let sections = req
                .extra_system_sections
                .iter()
                .filter(|section| !section.trim().is_empty())
                .cloned()
                .collect::<Vec<_>>();
            if sections.is_empty() {
                None
            } else {
                Some(sections.join("\n\n"))
            }
        },
        profile: RuntimeProfileTemplateContext {
            id: req.profile.clone(),
            content: req
                .profile_content
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        },
        output_contract: req
            .output_contract
            .as_ref()
            .map(runtime_output_contract_context),
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
    ctx: &PromptRuntimeContext,
) -> RuntimePredecessorTemplateContext {
    let reason_lines = predecessor_reason_lines(predecessors);
    RuntimePredecessorTemplateContext {
        is_empty: predecessors.is_empty(),
        chain: predecessor_chain_text(predecessors, ctx),
        reason_lines_empty: reason_lines.is_empty(),
        reason_lines,
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

fn predecessor_reason_lines(predecessors: &[PromptPredecessorContext]) -> String {
    predecessors
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
    let agent_type = ManagedAgentType::from_str(provider_id)?;
    provider_capabilities_for_type(agent_type)
}

pub fn provider_capabilities_for_type(
    agent_type: ManagedAgentType,
) -> Result<ProviderCapabilities> {
    if !agent_type.is_supported() {
        bail!("unsupported agent type: {}", agent_type.as_str());
    }
    Ok(AcpProvider::new(
        agent_type.as_str(),
        agent_type.default_adapter_config(),
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

pub fn provider_from_agent(
    agent_type: ManagedAgentType,
    config: &ManagedAgentConfig,
    use_local_claude: bool,
    acp_session_title_refresh_enabled: bool,
    acp_raw_max_size_bytes: u64,
    acp_raw_target_size_bytes: u64,
) -> Result<Box<dyn ProviderAdapter>> {
    if !agent_type.is_supported() {
        bail!("unsupported agent type: {}", agent_type.as_str());
    }
    Ok(Box::new(AcpProvider::new(
        agent_type.as_str(),
        config.adapter.clone(),
        use_local_claude,
        acp_session_title_refresh_enabled,
        acp_raw_max_size_bytes,
        acp_raw_target_size_bytes,
    )))
}

pub fn provider_from_id(
    provider_id: &str,
    use_local_claude: bool,
    acp_session_title_refresh_enabled: bool,
    acp_raw_max_size_bytes: u64,
    acp_raw_target_size_bytes: u64,
) -> Result<Box<dyn ProviderAdapter>> {
    let agent_type = ManagedAgentType::from_str(provider_id)?;
    let config = ManagedAgentConfig::new(agent_type.default_adapter_config());
    provider_from_agent(
        agent_type,
        &config,
        use_local_claude,
        acp_session_title_refresh_enabled,
        acp_raw_max_size_bytes,
        acp_raw_target_size_bytes,
    )
}

pub fn default_provider() -> Box<dyn ProviderAdapter> {
    provider_from_id(
        DEFAULT_PROVIDER,
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

    #[test]
    fn render_prompt_bundle_does_not_add_builtin_output_contracts() {
        let runtime_context = PromptRuntimeContext {
            project_id: "project-001".to_string(),
            task_id: "task-001".to_string(),
            run_id: "run-001".to_string(),
            round_id: "round-001".to_string(),
            node_id: "dev".to_string(),
            attempt_id: "attempt-001".to_string(),
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
        let req = WorkerInvocation {
            invocation_kind: InvocationKind::WorkerGeneric,
            profile: None,
            profile_content: None,
            requirement_path: None,
            requirement_text: Some("Need a structured result".to_string()),
            workspace_dir: Utf8PathBuf::from("/repo"),
            attempt_dir: runtime_context.attempt_dir.clone(),
            output_contract: None,
            runtime_context,
            predecessors: Vec::new(),
            extra_system_sections: Vec::new(),
            task_instruction: Some("Create a structured result".to_string()),
            session_mode: SessionMode::New,
            permission_mode: None,
            model: None,
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
        };

        let prompt = render_prompt_bundle(&req).unwrap();
        assert!(!prompt.system_prompt.contains("Output contract"));
    }

    #[test]
    fn default_provider_is_acp_only() {
        let info = default_provider().describe_provider();
        assert_eq!(info.provider_id, "claude-acp");
        assert!(info.capabilities.supports_continue_session);
        assert!(!info.capabilities.supports_raw_stream);
    }
}
