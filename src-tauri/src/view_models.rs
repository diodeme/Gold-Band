use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    str::FromStr,
    time::Instant,
};

use anyhow::Result;
use gold_band::acp::client::PromptActivity;
use gold_band::app::{App, LogSource, TaskSummary, is_run_continuable};
use gold_band::config::{
    AppearancePreference, DesktopAvailableUpdate, DesktopLanguage, DesktopUpdateBadgeState,
    ManagedAgentConfig, ManagedAgentId, McpServerState, PersonalizationPreference, RuntimeConfig,
    RuntimeLogLevel,
};
use gold_band::domain::{NodeType, RunOutcome, RunStatus, SessionMode};
use gold_band::dsl::{NodeDsl, WorkflowDsl, WorkflowValidationError};
use gold_band::dynamic::{DynamicGraphState, WorkspaceKind};
use gold_band::dynamic_store::load_dynamic_graph;
use gold_band::provider::{
    attachment_meta_for_path, mcp_capabilities_from_capabilities,
    select_config_options_from_capabilities, supported_models_from_capabilities,
    supported_modes_from_capabilities,
};
use gold_band::runtime::{NodeState, RoundState, RoundTraceStep, RunState, WorkerRefState};

use crate::channel::current_channel_config;
use crate::i18n::Translator;
use crate::metrics::{MetricsSettingsVm, metrics_settings};
use crate::state::AgentDiagnosticState;
use crate::updater::{UpdateInfoVm, UpdateStatusVm, UpdaterSettingsVm, updater_settings};
use crate::window_chrome::{DesktopWindowChromeVm, desktop_window_chrome_vm};
use gold_band::storage::read_json;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::avatar::{AvatarPreferencesVm, load_resolved_avatar_preferences};
use crate::wallpaper::{WallpaperPreferencesVm, load_resolved_wallpaper_preferences};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferencesVm {
    pub appearance: AppearancePreference,
    pub personalization: PersonalizationPreference,
    pub language: DesktopLanguage,
    pub use_local_claude: bool,
    pub verbose_logging: bool,
    pub avatars: AvatarPreferencesVm,
    pub wallpapers: WallpaperPreferencesVm,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalClaudeStatusVm {
    pub found: bool,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBadgeStateVm {
    pub settings_entry_seen_version: Option<String>,
    pub settings_advanced_seen_version: Option<String>,
    pub announcement_closed_version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBootstrapVm {
    pub repo_root: String,
    pub recent_workspaces: Vec<String>,
    pub preferences: PreferencesVm,
    pub updater_settings: UpdaterSettingsVm,
    pub metrics_settings: MetricsSettingsVm,
    pub update_status: UpdateStatusVm,
    pub update_badges: UpdateBadgeStateVm,
    pub persisted_available_update: Option<UpdateInfoVm>,
    pub client_version: String,
    pub platform: String,
    pub window_chrome: DesktopWindowChromeVm,
    pub app_info: AppInfoVm,
    pub app_config: AppConfigVm,
    pub needs_workspace: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfigVm {
    pub acp_session_title_refresh_enabled: bool,
    pub acp_chat_event_page_size: usize,
    pub conversation_inline_content_max_bytes: u64,
    pub conversation_inline_image_max_bytes: u64,
    pub conversation_inline_image_max_dimension: u32,
    pub turn_files: TurnFilesVm,
    pub workspace_layout: WorkspaceLayoutVm,
    pub workspace_files: WorkspaceFilesVm,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnFilesVm {
    pub card_preview_limit: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFilesVm {
    pub auto_save_delay_ms: u64,
    pub search_debounce_ms: u64,
    pub search_result_limit: usize,
    pub text_editable_max_bytes: u64,
    pub text_highlight_max_chars: usize,
    pub text_read_only_max_bytes: u64,
    pub image_preview_max_bytes: u64,
    pub image_preview_max_pixels: u64,
    pub content_cache_entries: usize,
    pub content_cache_max_bytes: u64,
    pub watch_debounce_ms: u64,
    pub external_access_grant_ttl_seconds: u64,
    pub markdown_live_preview_max_chars: usize,
    pub markdown_embedded_image_limit: usize,
    pub markdown_embedded_image_max_concurrent: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceLayoutVm {
    pub shell_min_width: u32,
    pub shell_min_height: u32,
    pub right_workspace: RightWorkspaceLayoutVm,
    pub conversation: WorkspaceLayoutProfileVm,
    pub context_cards: WorkspaceLayoutProfileVm,
    pub workflow_canvas: WorkspaceLayoutProfileVm,
    pub settings: WorkspaceLayoutProfileVm,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RightWorkspaceLayoutVm {
    pub min_width: u32,
    pub default_width: u32,
    pub max_width: u32,
    pub file: FileWorkspaceLayoutVm,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileWorkspaceLayoutVm {
    pub split_min_width: u32,
    pub tree_default_width: u32,
    pub tree_min_width: u32,
    pub tree_max_width: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceLayoutProfileVm {
    pub center_min_width: u32,
    pub center_auto_collapse_width: u32,
    pub window_min_width: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoVm {
    pub channel: String,
    pub feedback_enabled: bool,
    pub app_name: String,
    pub app_key: String,
    pub config_dir_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRegistryVm {
    pub agents: Vec<ManagedAgentVm>,
    pub catalog: Vec<AgentCatalogEntryVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedAgentVm {
    pub agent_type: String,
    pub display_name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<AgentEnvEntryVm>,
    pub icon_key: String,
    pub primary_agent_dir: String,
    pub project_primary_agent_dir: Option<String>,
    pub compatible_agent_dirs: Vec<String>,
    pub supports_system_prompt: bool,
    pub external_session_sync_supported: bool,
    pub external_session_sync_enabled: bool,
    pub diagnostic: Option<ManagedAgentDiagnosticVm>,
    pub supported_modes: Option<Vec<AcpModeVm>>,
    pub supported_models: Option<Vec<AcpModeVm>>,
    pub config_options: Option<Vec<AcpSelectConfigOptionVm>>,
    /// 是否支持 streamable HTTP MCP 传输（None=未诊断/未知）
    pub mcp_http_supported: Option<bool>,
    /// 是否支持 SSE MCP 传输（None=未诊断/未知）
    pub mcp_sse_supported: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpModeVm {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpSelectConfigOptionVm {
    pub id: String,
    pub category: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub current_value: Option<String>,
    pub options: Vec<AcpSelectConfigValueVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpSelectConfigValueVm {
    pub value: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEnvEntryVm {
    pub key: String,
    pub value: String,
}

// ── MCP ViewModels ──

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerVm {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub transport: String,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<Vec<AgentEnvEntryVm>>,
    pub url: Option<String>,
    pub headers: Option<Vec<AgentEnvEntryVm>>,
    pub managed: bool,
    pub help_message: Option<String>,
    pub health_status: Option<String>, // "healthy" | "unhealthy" | "unknown"
    pub health_message: Option<String>,
}

// ── SKILL ViewModels ──

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMetaVm {
    pub name: String,
    pub description: String,
    pub source: String,
    pub directory_path: String,
    pub agent_source: String,
    pub load_warnings: Vec<String>,
    pub synced_agent_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillListVm {
    pub global: Vec<SkillMetaVm>,
    pub project: Vec<SkillMetaVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillContentVm {
    pub meta: SkillMetaVm,
    pub description_source: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedAgentDiagnosticVm {
    pub status: String,
    pub available: bool,
    pub reason: Option<String>,
    pub checked_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogEntryVm {
    pub agent_type: String,
    pub label: String,
    pub icon_key: String,
    pub version: String,
    pub description: String,
    pub repository: Option<String>,
    pub website: Option<String>,
    pub primary_agent_dir: String,
    pub project_primary_agent_dir: Option<String>,
    pub compatible_agent_dirs: Vec<String>,
    pub configured: bool,
    pub supports_system_prompt: bool,
    pub supports_external_session_sync: bool,
    pub default_display_name: String,
    pub default_command: String,
    pub default_args: Vec<String>,
    pub default_env: Vec<AgentEnvEntryVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryCardVm {
    pub key: String,
    pub label: String,
    pub value: usize,
    pub tone: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskListVm {
    pub cards: Vec<SummaryCardVm>,
    pub tasks: Vec<TaskRowVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRowVm {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub requirement: String,
    pub requirement_preview: String,
    pub display_status: String,
    pub workflow_exists: bool,
    pub workflow_valid: bool,
    pub workflow_error: Option<WorkflowErrorVm>,
    pub latest_run: Option<RunSummaryVm>,
    pub resumable_run_id: Option<String>,
    pub artifact_count: usize,
    pub attachment_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDetailVm {
    pub task: TaskRowVm,
    pub requirement: String,
    pub runs: Vec<RunSummaryVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowVm {
    pub task: TaskRowVm,
    pub graph: GraphVm,
    pub runs: Vec<RunGroupVm>,
    pub control: Option<WorkflowControlVm>,
    pub workflow_json: Option<String>,
    pub model_bindings: gold_band::workflow_model_binding::WorkflowModelBindings,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowErrorVm {
    pub code: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowControlVm {
    pub max_attempts: Option<u32>,
    pub max_rounds: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlFailureVm {
    pub reason_kind: String,
    pub title: String,
    pub message: String,
    pub from_node_id: Option<String>,
    pub to_node_id: Option<String>,
    pub target: Option<String>,
    pub edge_outcome: Option<String>,
    pub proposed_count: Option<u32>,
    pub limit: Option<u32>,
    pub timestamp: Option<String>,
    pub round_id: Option<String>,
    pub node_id: Option<String>,
    pub attempt_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunDetailVm {
    pub run: RunSummaryVm,
    pub rounds: Vec<RoundSummaryVm>,
    pub events: Option<String>,
    pub progress: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundDetailVm {
    pub run: RunSummaryVm,
    pub round: RoundSummaryVm,
    pub graph: GraphVm,
    pub control: Option<WorkflowControlVm>,
    pub control_failure: Option<ControlFailureVm>,
    pub requirement: String,
    pub selected_node_detail: Option<NodeDetailVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunGroupVm {
    pub run: RunSummaryVm,
    pub rounds: Vec<RoundSummaryVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummaryVm {
    pub id: String,
    pub task_id: String,
    pub status: String,
    pub outcome: Option<String>,
    pub started_at: String,
    pub updated_at: String,
    pub current_round: Option<String>,
    pub current_node: Option<String>,
    pub current_attempt: Option<String>,
    pub resumable: bool,
    pub pause_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundSummaryVm {
    pub id: String,
    pub run_id: String,
    pub index: u32,
    pub status: String,
    pub outcome: Option<String>,
    pub trigger: String,
    pub started_at: String,
    pub current_node: Option<String>,
    pub artifact_count: usize,
    pub attachment_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphVm {
    pub nodes: Vec<GraphNodeVm>,
    pub edges: Vec<GraphEdgeVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDisplayVm {
    pub code: String,
    pub tone: String,
    pub icon: String,
    pub terminal: bool,
    pub resumable: bool,
    pub reason_code: Option<String>,
    pub blocking_error: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNodeVm {
    pub id: String,
    pub node_id: Option<String>,
    pub sequence: Option<u32>,
    pub label: String,
    pub node_type: String,
    pub status: Option<String>,
    pub outcome: Option<String>,
    pub runtime_display: RuntimeDisplayVm,
    pub attempt_id: Option<String>,
    pub outer_node_id: Option<String>,
    pub outer_attempt_id: Option<String>,
    pub attempt_count: usize,
    pub attempts: Vec<GraphAttemptVm>,
    pub artifact_count: usize,
    pub attachment_count: usize,
    pub current: bool,
    pub icon_key: Option<String>,
    pub session_mode: Option<String>,
    pub continue_from_node_id: Option<String>,
    pub dynamic_summary: Option<DynamicSummaryVm>,
    pub dynamic_group_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicSummaryVm {
    pub status: String,
    pub outcome: Option<String>,
    pub internal_node_count: usize,
    pub group_count: usize,
    pub proposal_count: usize,
    pub current_node_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphAttemptVm {
    pub attempt_id: String,
    pub sequence: Option<u32>,
    pub status: String,
    pub outcome: Option<String>,
    pub runtime_display: RuntimeDisplayVm,
    pub session_mode: Option<String>,
    pub acp_session_id: Option<String>,
    pub current: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdgeVm {
    pub from: String,
    pub to: String,
    pub label: String,
    pub traversal_count: usize,
    pub last_outcome: Option<String>,
    pub blocked_reason: Option<ControlFailureVm>,
}

pub fn runtime_display_vm(
    status: Option<&str>,
    outcome: Option<&str>,
    current: bool,
    pause_reason: Option<&str>,
    resumable: bool,
) -> RuntimeDisplayVm {
    let status = status.map(normalize_status_code);
    let outcome = outcome.map(normalize_status_code);
    let reason_code = pause_reason.map(normalize_status_code);

    let (code, tone, icon, terminal) = match outcome.as_deref() {
        Some("success") => ("success", "success", "check", true),
        Some("failure") | Some("failed") | Some("invalid") => ("failure", "danger", "error", true),
        Some("killed") | Some("cancelled") | Some("canceled") => {
            ("killed", "danger", "error", true)
        }
        _ => match status.as_deref() {
            Some("running") | Some("in-progress") | Some("in_progress") | Some("active")
            | Some("starting") | Some("sending") => ("running", "running", "dot", false),
            Some("paused") if current && reason_code.as_deref() == Some("error-blocked") => {
                ("error-blocked", "danger", "error", false)
            }
            Some("paused") if current && reason_code.as_deref() == Some("runtime-abnormal") => {
                ("runtime-abnormal", "danger", "error", false)
            }
            Some("paused") => ("paused", "warning", "pause", false),
            Some("cancelling") | Some("cancel-requested") => ("paused", "warning", "pause", false),
            Some("pending") | Some("ready") => ("pending", "neutral", "dot", false),
            Some("completed") | Some("complete") => ("completed", "neutral", "dot", true),
            Some("failed") | Some("failure") | Some("error") => {
                ("failure", "danger", "error", true)
            }
            Some("killed") | Some("cancelled") | Some("canceled") => {
                ("killed", "danger", "error", true)
            }
            Some(other) => (other, "neutral", "dot", false),
            None => ("pending", "neutral", "dot", false),
        },
    };

    let blocking_error = match outcome.as_deref() {
        Some("failure") | Some("failed") | Some("invalid") | Some("success") => false,
        Some("killed") | Some("cancelled") | Some("canceled") => true,
        _ => matches!(code, "error-blocked" | "failure" | "killed"),
    };

    RuntimeDisplayVm {
        code: code.to_string(),
        tone: tone.to_string(),
        icon: icon.to_string(),
        terminal,
        resumable: matches!(code.as_ref(), "paused" | "runtime-abnormal") && resumable,
        reason_code,
        blocking_error,
    }
}

fn normalize_status_code(value: &str) -> String {
    match value.trim().to_ascii_lowercase().replace('_', "-").as_str() {
        "errorblocked" => "error-blocked".to_string(),
        "processinterrupted" => "process-interrupted".to_string(),
        "runtimeabnormal" => "runtime-abnormal".to_string(),
        "waitingforuserinput" => "waiting-for-user-input".to_string(),
        "permissionrequested" => "permission-requested".to_string(),
        other => other.to_string(),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDetailVm {
    pub id: String,
    pub node_id: String,
    pub sequence: Option<u32>,
    pub label: String,
    pub node_type: String,
    pub provider: Option<String>,
    pub provider_display_name: Option<String>,
    pub status: String,
    pub outcome: Option<String>,
    pub attempt_id: String,
    pub outer_node_id: Option<String>,
    pub outer_attempt_id: Option<String>,
    pub current: bool,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub artifact_count: usize,
    pub attachment_count: usize,
    pub artifacts: Vec<AssetItemVm>,
    pub attachments: Vec<AssetItemVm>,
    pub has_progress_events: bool,
    pub has_raw_stream: bool,
    pub has_worker_ref: bool,
    pub manual_check_enabled: bool,
    pub manual_check_pending: bool,
    pub session_mode: Option<String>,
    pub continue_from_node_id: Option<String>,
    pub acp_session: Option<AcpSessionVm>,
    pub acp_conversations: Vec<AcpConversationVm>,
    pub selected_conversation_key: Option<String>,
    pub dynamic: Option<DynamicDetailVm>,
    pub dynamic_group_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicDetailVm {
    pub summary: DynamicSummaryVm,
    pub graph: GraphVm,
    pub groups: Vec<DynamicGroupVm>,
    pub proposals: Vec<DynamicProposalVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicGroupVm {
    pub id: String,
    pub status: String,
    pub depth: u32,
    pub parent_group_id: Option<String>,
    pub root_node_ids: Vec<String>,
    pub terminal_node_ids: Vec<String>,
    pub merge_node_id: Option<String>,
    pub acceptance_node_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicProposalValidationErrorVm {
    pub code: String,
    pub message: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicProposalVm {
    pub id: String,
    pub source_node_id: String,
    pub validation_status: String,
    pub validation_errors: Vec<DynamicProposalValidationErrorVm>,
    pub artifact_path: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpConversationVm {
    pub key: String,
    pub label: String,
    pub session_id: Option<String>,
    pub session_mode: String,
    pub active_attempt_id: String,
    pub attempts: Vec<AcpAttemptSessionVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpAttemptSessionVm {
    pub node_id: String,
    pub attempt_id: String,
    pub sequence: Option<u32>,
    pub status: String,
    pub outcome: Option<String>,
    pub current: bool,
    pub session_mode: Option<String>,
    pub acp_session_id: Option<String>,
    pub acp_session: Option<AcpSessionVm>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AcpUsageVm {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_amount_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_read_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_write_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpSessionVm {
    pub branch_id: String,
    pub parent_branch_id: Option<String>,
    pub read_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_execution: Option<AcpAgentExecutionVm>,
    pub session_id: Option<String>,
    pub title: Option<String>,
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outer_node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outer_attempt_id: Option<String>,
    pub provider: String,
    pub adapter_id: Option<String>,
    pub adapter_display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter_icon_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_branch: Option<String>,
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_cwd: Option<String>,
    pub status: String,
    pub session_started_at: Option<String>,
    pub session_updated_at: Option<String>,
    pub session_elapsed_seconds: Option<u64>,
    pub timing: Option<AcpSessionTimingVm>,
    pub restored: bool,
    pub stop_reason: Option<String>,
    pub system_prompt_append: Option<String>,
    pub config: Option<AcpSessionConfigVm>,
    pub events: Vec<AcpUiEventVm>,
    pub event_page: AcpEventPageVm,
    pub timeline_projection: AcpTimelineProjectionVm,
    pub pending_interactions: Vec<AcpPromptInteractionVm>,
    pub available_commands: Option<Vec<serde_json::Value>>,
    pub usage: Option<AcpUsageVm>,
    pub diagnostics: AcpDiagnosticsVm,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpTimelineProjectionVm {
    pub agents: Vec<AcpAgentExecutionVm>,
    pub todo_entries: Vec<serde_json::Value>,
    #[serde(skip)]
    todo_ownership: Option<gold_band::acp::branches::ConversationPlanOwnership>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpAgentExecutionVm {
    pub agent_execution_id: String,
    pub parent_agent_execution_id: Option<String>,
    pub execution_status: String,
    pub event_count: usize,
    pub tool_call_count: usize,
    pub read_file_count: usize,
    pub written_file_count: usize,
    pub has_attention: bool,
    pub title: Option<String>,
    pub description: Option<String>,
    pub todo_entries: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpSessionQueryInput {
    pub trace_id: Option<String>,
    pub branch_id: Option<String>,
    pub before_seq: Option<u64>,
    pub after_seq: Option<u64>,
    pub after_revision: Option<u64>,
    pub before_cursor: Option<String>,
    pub after_cursor: Option<String>,
    pub event_limit: Option<usize>,
    pub page_size: Option<usize>,
}

struct AcpSessionQueryTrace {
    trace_id: String,
    branch_id: String,
    started_at: Instant,
    stage_started_at: Instant,
}

impl AcpSessionQueryTrace {
    fn from_query(query: Option<&AcpSessionQueryInput>, branch_id: &str) -> Option<Self> {
        let trace_id = query?.trace_id.as_deref()?.trim();
        if trace_id.is_empty() {
            return None;
        }
        let now = Instant::now();
        Some(Self {
            trace_id: trace_id.to_string(),
            branch_id: branch_id.to_string(),
            started_at: now,
            stage_started_at: now,
        })
    }

    fn stage(&mut self, stage: &'static str, details: serde_json::Value) {
        let now = Instant::now();
        let stage_ms = now.duration_since(self.stage_started_at).as_millis() as u64;
        let total_ms = now.duration_since(self.started_at).as_millis() as u64;
        self.stage_started_at = now;
        tracing::info!(
            target: "gold_band_desktop::acp_session_query",
            trace_id = %self.trace_id,
            branch_id = %self.branch_id,
            stage,
            stage_ms,
            total_ms,
            details = %details,
            "ACP session query stage"
        );
        #[cfg(debug_assertions)]
        eprintln!(
            "[acp-session-query] trace={} branch={} stage={} stage_ms={} total_ms={} details={}",
            self.trace_id, self.branch_id, stage, stage_ms, total_ms, details
        );
    }
}

fn trace_acp_session_query(
    trace: &mut Option<AcpSessionQueryTrace>,
    stage: &'static str,
    details: serde_json::Value,
) {
    if let Some(trace) = trace.as_mut() {
        trace.stage(stage, details);
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpEventPageVm {
    pub generation: u64,
    pub covered_revision: u64,
    pub newest_revision: Option<u64>,
    pub loaded_count: usize,
    pub total: usize,
    pub oldest_seq: Option<u64>,
    pub newest_seq: Option<u64>,
    pub has_older: bool,
    pub has_newer: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oldest_cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub newest_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpSessionConfigVm {
    pub catalog_observed_at: Option<String>,
    pub model_override_id: Option<String>,
    pub permission_mode_override_id: Option<String>,
    pub config_option_overrides: std::collections::BTreeMap<String, String>,
    pub current_model_id: Option<String>,
    pub current_model_name: Option<String>,
    pub current_mode_id: Option<String>,
    pub current_mode_name: Option<String>,
    pub models: Option<serde_json::Value>,
    pub modes: Option<serde_json::Value>,
    pub config_options: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpUiEventVm {
    pub id: String,
    pub seq: u64,
    pub timestamp: String,
    pub kind: String,
    pub session_id: Option<String>,
    pub content: Option<String>,
    pub title: Option<String>,
    pub tool_call_id: Option<String>,
    pub status: Option<String>,
    pub started_seq: Option<u64>,
    pub ended_seq: Option<u64>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub timing: Option<AcpTimingPatchVm>,
    pub raw: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AcpTimingPatchVm {
    pub session_elapsed_seconds: u64,
    pub revision: Option<u64>,
    pub observed_at: Option<String>,
    pub active_turn_started_at: Option<String>,
    pub active_turn_last_activity_at: Option<String>,
    pub permission_wait_started_at: Option<String>,
    pub user_wait_started_at: Option<String>,
    pub wait_reason: Option<String>,
    pub paused: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AcpSessionTimingVm {
    pub session_elapsed_seconds: u64,
    pub revision: Option<u64>,
    pub observed_at: Option<String>,
    pub active_turn_started_at: Option<String>,
    pub active_turn_last_activity_at: Option<String>,
    pub permission_wait_started_at: Option<String>,
    pub user_wait_started_at: Option<String>,
    pub wait_reason: Option<String>,
    pub paused: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AcpPromptInteractionVm {
    Permission {
        interaction_id: String,
        turn_id: Option<String>,
        prompt_event_id: Option<String>,
        title: String,
        tool_call_id: Option<String>,
        options: Vec<AcpPermissionOptionVm>,
        raw: serde_json::Value,
    },
    Elicitation {
        interaction_id: String,
        turn_id: Option<String>,
        prompt_event_id: Option<String>,
        message: String,
        tool_call_id: Option<String>,
        requested_schema: serde_json::Value,
        raw: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpPermissionOptionVm {
    pub option_id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpDiagnosticsVm {
    pub raw_frame_count: usize,
    pub event_count: usize,
    pub error_count: usize,
    pub last_error: Option<String>,
    pub last_error_timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetItemVm {
    pub kind: String,
    pub name: String,
    pub title: String,
    pub tone: String,
    pub preview: String,
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntryVm {
    pub id: String,
    pub timestamp: String,
    pub entry_type: String,
    pub level: Option<String>,
    pub node_id: Option<String>,
    pub attempt_id: Option<String>,
    pub stage: Option<String>,
    pub summary: String,
    pub source: String,
    pub raw: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogPageVm {
    pub items: Vec<LogEntryVm>,
    pub page: usize,
    pub page_size: usize,
    pub total: usize,
    pub has_previous: bool,
    pub has_next: bool,
    pub tier: String,
    pub hot_limit: usize,
    pub archive_retention_days: u64,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AcpRawFrameOrder {
    Asc,
    #[default]
    Desc,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpRawFrameQueryInput {
    pub page: Option<usize>,
    pub page_size: Option<usize>,
    pub search: Option<String>,
    pub kind: Option<String>,
    pub direction: Option<String>,
    pub order: Option<AcpRawFrameOrder>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpRawFrameVm {
    pub id: String,
    pub line_number: usize,
    pub timestamp: Option<String>,
    pub direction: Option<String>,
    pub kind: String,
    pub content: String,
    pub content_truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpRawFramePageVm {
    pub items: Vec<AcpRawFrameVm>,
    pub page: usize,
    pub page_size: usize,
    pub total: usize,
    pub has_previous: bool,
    pub has_next: bool,
    pub order: AcpRawFrameOrder,
    pub search: Option<String>,
    pub kind: Option<String>,
    pub direction: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpActivityDetailQueryInput {
    pub branch_id: String,
    pub activity_start_seq: u64,
    pub activity_end_seq: u64,
    pub earlier_cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpActivityDetailVm {
    pub items: Vec<AcpUiEventVm>,
    pub has_more_earlier: bool,
    pub earlier_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpToolDetailQueryInput {
    pub branch_id: String,
    pub event_id: String,
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpToolDetailVm {
    pub event: Option<AcpUiEventVm>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogScopeInput {
    pub task_id: String,
    pub run_id: String,
    pub round_id: Option<String>,
    pub node_id: Option<String>,
    pub attempt_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogQueryInput {
    pub scope: LogScopeInput,
    pub source: Option<String>,
    pub page: Option<usize>,
    pub page_size: Option<usize>,
    pub hot_limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentVm {
    pub title: String,
    pub kind: String,
    pub content: String,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RoundSelectionInput {
    Round {
        context_node_id: Option<String>,
    },
    Requirement {
        context_node_id: Option<String>,
    },
    Node {
        node_id: String,
        attempt_id: Option<String>,
        outer_node_id: Option<String>,
        outer_attempt_id: Option<String>,
    },
    Artifact {
        node_id: String,
        attempt_id: Option<String>,
    },
    Attachment {
        node_id: String,
        attempt_id: Option<String>,
    },
    WorkerRef {
        node_id: String,
        attempt_id: Option<String>,
    },
    Event {
        node_id: Option<String>,
        attempt_id: Option<String>,
        context_node_id: Option<String>,
    },
    Log {
        node_id: Option<String>,
        attempt_id: Option<String>,
        context_node_id: Option<String>,
    },
}

pub fn preferences_vm(
    appearance: AppearancePreference,
    personalization: PersonalizationPreference,
    language: DesktopLanguage,
    use_local_claude: bool,
    log_level: RuntimeLogLevel,
    avatars: AvatarPreferencesVm,
    wallpapers: WallpaperPreferencesVm,
) -> PreferencesVm {
    PreferencesVm {
        appearance,
        personalization,
        language,
        use_local_claude,
        verbose_logging: matches!(log_level, RuntimeLogLevel::Debug | RuntimeLogLevel::Trace),
        avatars,
        wallpapers,
    }
}

fn update_badge_state_vm(state: &DesktopUpdateBadgeState) -> UpdateBadgeStateVm {
    UpdateBadgeStateVm {
        settings_entry_seen_version: state.settings_entry_seen_version.clone(),
        settings_advanced_seen_version: state.settings_advanced_seen_version.clone(),
        announcement_closed_version: state.announcement_closed_version.clone(),
    }
}

fn persisted_available_update_vm(
    update: Option<&DesktopAvailableUpdate>,
    current_version: &str,
) -> Option<UpdateInfoVm> {
    let update = update?;
    // 退出安装后 current_version 会变为新版本号，此时应清除旧的 available 记录
    if update.current_version != current_version {
        return None;
    }
    Some(UpdateInfoVm {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        notes: update.notes.clone(),
        pub_date: update.pub_date.clone(),
    })
}

fn app_config_vm(config: &RuntimeConfig) -> AppConfigVm {
    let profile_vm =
        |profile: &gold_band::config::WorkspaceLayoutProfileConfig| WorkspaceLayoutProfileVm {
            center_min_width: profile.center_min_width,
            center_auto_collapse_width: profile.center_auto_collapse_width,
            window_min_width: profile.window_min_width,
        };
    let right_workspace = &config.workspace_layout.right_workspace;
    AppConfigVm {
        acp_session_title_refresh_enabled: config.acp_session_title_refresh_enabled,
        acp_chat_event_page_size: config.acp_chat_event_page_size,
        conversation_inline_content_max_bytes: config.conversation_inline_content_max_bytes,
        conversation_inline_image_max_bytes: config.conversation_inline_image_max_bytes,
        conversation_inline_image_max_dimension: config.conversation_inline_image_max_dimension,
        turn_files: TurnFilesVm {
            card_preview_limit: config.turn_files.card_preview_limit,
        },
        workspace_layout: WorkspaceLayoutVm {
            shell_min_width: config.workspace_layout.shell_min_width,
            shell_min_height: config.workspace_layout.shell_min_height,
            right_workspace: RightWorkspaceLayoutVm {
                min_width: right_workspace.min_width,
                default_width: right_workspace.default_width,
                max_width: right_workspace.max_width,
                file: FileWorkspaceLayoutVm {
                    split_min_width: right_workspace.file.split_min_width,
                    tree_default_width: right_workspace.file.tree_default_width,
                    tree_min_width: right_workspace.file.tree_min_width,
                    tree_max_width: right_workspace.file.tree_max_width,
                },
            },
            conversation: profile_vm(&config.workspace_layout.conversation),
            context_cards: profile_vm(&config.workspace_layout.context_cards),
            workflow_canvas: profile_vm(&config.workspace_layout.workflow_canvas),
            settings: profile_vm(&config.workspace_layout.settings),
        },
        workspace_files: WorkspaceFilesVm {
            auto_save_delay_ms: config.workspace_files.auto_save_delay_ms,
            search_debounce_ms: config.workspace_files.search_debounce_ms,
            search_result_limit: config.workspace_files.search_result_limit,
            text_editable_max_bytes: config.workspace_files.text_editable_max_bytes,
            text_highlight_max_chars: config.workspace_files.text_highlight_max_chars,
            text_read_only_max_bytes: config.workspace_files.text_read_only_max_bytes,
            image_preview_max_bytes: config.workspace_files.image_preview_max_bytes,
            image_preview_max_pixels: config.workspace_files.image_preview_max_pixels,
            content_cache_entries: config.workspace_files.content_cache_entries,
            content_cache_max_bytes: config.workspace_files.content_cache_max_bytes,
            watch_debounce_ms: config.workspace_files.watch_debounce_ms,
            external_access_grant_ttl_seconds: config
                .workspace_files
                .external_access_grant_ttl_seconds,
            markdown_live_preview_max_chars: config.workspace_files.markdown_live_preview_max_chars,
            markdown_embedded_image_limit: config.workspace_files.markdown_embedded_image_limit,
            markdown_embedded_image_max_concurrent: config
                .workspace_files
                .markdown_embedded_image_max_concurrent,
        },
    }
}

#[cfg(target_os = "macos")]
const DESKTOP_PLATFORM: &str = "macos";
#[cfg(target_os = "windows")]
const DESKTOP_PLATFORM: &str = "windows";
#[cfg(target_os = "linux")]
const DESKTOP_PLATFORM: &str = "linux";
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
const DESKTOP_PLATFORM: &str = "unknown";

pub fn bootstrap_vm(
    app: &App,
    recent_workspaces: Vec<String>,
    update_status: UpdateStatusVm,
    client_version: impl Into<String>,
    needs_workspace: bool,
) -> AppBootstrapVm {
    let client_version_string: String = client_version.into();
    let channel_config = current_channel_config();
    AppBootstrapVm {
        repo_root: app.paths.repo_root.to_string(),
        recent_workspaces,
        preferences: preferences_vm(
            app.config.appearance.clone(),
            app.config.personalization.clone(),
            app.config.desktop_language,
            app.config.use_local_claude,
            app.config.log_level,
            load_resolved_avatar_preferences(
                &app.paths.user_gold_band_dir(),
                &app.config.personalization,
            )
            .unwrap_or_default(),
            load_resolved_wallpaper_preferences(&app.paths.user_gold_band_dir())
                .unwrap_or_default(),
        ),
        updater_settings: updater_settings(&app.config),
        metrics_settings: metrics_settings(&app.config),
        update_status,
        update_badges: update_badge_state_vm(&app.config.desktop_update_badges),
        persisted_available_update: persisted_available_update_vm(
            app.config.desktop_available_update.as_ref(),
            &client_version_string,
        ),
        client_version: client_version_string,
        platform: DESKTOP_PLATFORM.to_string(),
        window_chrome: desktop_window_chrome_vm(),
        app_info: AppInfoVm {
            channel: channel_config.channel.to_string(),
            feedback_enabled: channel_config.feedback_enabled,
            app_name: channel_config.app_name.to_string(),
            app_key: channel_config.app_key.to_string(),
            config_dir_name: channel_config.config_dir_name.to_string(),
        },
        app_config: app_config_vm(&app.config),
        needs_workspace,
    }
}

pub fn agent_registry_vm(
    app: &App,
    diagnostics: &std::collections::BTreeMap<ManagedAgentId, AgentDiagnosticState>,
) -> AgentRegistryVm {
    let agents = app
        .managed_agents()
        .iter()
        .map(|(agent_type, config)| {
            managed_agent_vm(agent_type, config, diagnostics.get(agent_type))
        })
        .collect::<Vec<_>>();
    let catalog = gold_band::agent_catalog::builtin_agent_catalog()
        .agents
        .iter()
        .map(|entry| {
            let agent_id =
                ManagedAgentId::from_str(&entry.id).expect("built-in Agent catalog ids are valid");
            let default_config = ManagedAgentConfig::from_catalog(entry);
            AgentCatalogEntryVm {
                agent_type: entry.id.clone(),
                label: entry.label.clone(),
                icon_key: entry.icon_key.clone(),
                version: entry.version.clone(),
                description: entry.description.clone(),
                repository: entry.repository.clone(),
                website: entry.website.clone(),
                primary_agent_dir: default_config.primary_agent_dir.unwrap_or_default(),
                project_primary_agent_dir: default_config.project_primary_agent_dir.clone(),
                compatible_agent_dirs: default_config.compatible_agent_dirs.clone(),
                configured: app.managed_agents().contains_key(&agent_id),
                supports_system_prompt: entry.supports_system_prompt,
                supports_external_session_sync: entry.supports_external_session_sync,
                default_display_name: default_config.adapter.display_name,
                default_command: default_config.adapter.command,
                default_args: default_config.adapter.args,
                default_env: default_config
                    .adapter
                    .env
                    .into_iter()
                    .map(|(key, value)| AgentEnvEntryVm { key, value })
                    .collect(),
            }
        })
        .collect();
    AgentRegistryVm { agents, catalog }
}

fn managed_agent_vm(
    agent_id: &ManagedAgentId,
    config: &ManagedAgentConfig,
    diagnostic: Option<&AgentDiagnosticState>,
) -> ManagedAgentVm {
    ManagedAgentVm {
        agent_type: agent_id.as_str().to_string(),
        display_name: config.adapter.display_name.clone(),
        command: config.adapter.command.clone(),
        args: config.adapter.args.clone(),
        env: config
            .adapter
            .env
            .iter()
            .map(|(key, value)| AgentEnvEntryVm {
                key: key.clone(),
                value: value.clone(),
            })
            .collect(),
        icon_key: config.icon.clone(),
        primary_agent_dir: config.primary_agent_dir.clone().unwrap_or_default(),
        project_primary_agent_dir: config.project_primary_agent_dir.clone(),
        compatible_agent_dirs: config.compatible_agent_dirs.clone(),
        supports_system_prompt: config.supports_system_prompt(),
        external_session_sync_supported: config.external_session_sync_supported,
        external_session_sync_enabled: config.external_session_sync_enabled,
        diagnostic: diagnostic.map(|diagnostic| ManagedAgentDiagnosticVm {
            status: if diagnostic.available {
                "healthy"
            } else {
                "unhealthy"
            }
            .to_string(),
            available: diagnostic.available,
            reason: diagnostic.reason.clone(),
            checked_at: diagnostic.checked_at.clone(),
        }),
        supported_modes: diagnostic.and_then(|diagnostic| {
            let modes = supported_modes_from_capabilities(diagnostic.capabilities.as_ref())
                .into_iter()
                .map(|mode| AcpModeVm {
                    id: mode.id.clone(),
                    name: mode.name.unwrap_or_else(|| mode.id.clone()),
                    description: mode.description.clone(),
                })
                .collect::<Vec<_>>();
            (!modes.is_empty()).then_some(modes)
        }),
        supported_models: diagnostic.and_then(|diagnostic| {
            let models = supported_models_from_capabilities(diagnostic.capabilities.as_ref())
                .into_iter()
                .map(|model| AcpModeVm {
                    id: model.id.clone(),
                    name: model.name.unwrap_or_else(|| model.id.clone()),
                    description: model.description.clone(),
                })
                .collect::<Vec<_>>();
            (!models.is_empty()).then_some(models)
        }),
        config_options: diagnostic.and_then(|diagnostic| {
            let options = select_config_options_from_capabilities(diagnostic.capabilities.as_ref())
                .into_iter()
                .map(|option| AcpSelectConfigOptionVm {
                    id: option.id,
                    category: option.category,
                    name: option.name,
                    description: option.description,
                    current_value: option.current_value,
                    options: option
                        .options
                        .into_iter()
                        .map(|value| AcpSelectConfigValueVm {
                            name: value.name.unwrap_or_else(|| value.value.clone()),
                            value: value.value,
                            description: value.description,
                        })
                        .collect(),
                })
                .collect::<Vec<_>>();
            (!options.is_empty()).then_some(options)
        }),
        mcp_http_supported: diagnostic.and_then(|d| {
            mcp_capabilities_from_capabilities(d.capabilities.as_ref()).map(|m| m.http)
        }),
        mcp_sse_supported: diagnostic.and_then(|d| {
            mcp_capabilities_from_capabilities(d.capabilities.as_ref()).map(|m| m.sse)
        }),
    }
}

fn provider_icon_key(app: &App, provider: &str) -> Option<String> {
    app.managed_agent(provider)
        .ok()
        .map(|(_, config)| config.icon.clone())
}

pub fn task_list_vm(app: &App) -> Result<TaskListVm> {
    let labels = Translator::new(app.config.desktop_language);
    let summaries = app.task_summaries()?;
    let mut tasks = Vec::new();
    let mut running = 0usize;
    let mut resumable = 0usize;
    let mut failed = 0usize;
    let mut invalid = 0usize;

    for summary in summaries {
        let row = task_row_vm(app, &summary)?;
        match row.display_status.as_str() {
            "running" => running += 1,
            "resumable" => resumable += 1,
            "failed" => failed += 1,
            "invalid" | "missing-workflow" => invalid += 1,
            _ => {}
        }
        tasks.push(row);
    }

    Ok(TaskListVm {
        cards: vec![
            summary_card_vm(&labels, "all", tasks.len(), "neutral"),
            summary_card_vm(&labels, "running", running, "accent"),
            summary_card_vm(&labels, "resumable", resumable, "warning"),
            summary_card_vm(&labels, "failed", failed, "danger"),
            summary_card_vm(&labels, "invalid", invalid, "muted"),
        ],
        tasks,
    })
}

fn summary_card_vm(labels: &Translator, key: &str, value: usize, tone: &str) -> SummaryCardVm {
    SummaryCardVm {
        key: key.to_string(),
        label: labels.tr(&format!("summary.{key}")),
        value,
        tone: tone.to_string(),
    }
}

pub fn task_detail_vm(app: &App, task_id: &str) -> Result<TaskDetailVm> {
    let labels = Translator::new(app.config.desktop_language);
    let summary = app.task_summary(task_id)?;
    let task = task_row_vm(app, &summary)?;
    let requirement = read_optional_text(&app.paths.requirement_file(task_id))?
        .unwrap_or_else(|| labels.tr("fallback.missingRequirement"));
    let runs = newest_first(app.run_list(task_id)?)
        .into_iter()
        .map(run_summary_vm)
        .collect::<Vec<_>>();
    Ok(TaskDetailVm {
        task,
        requirement,
        runs,
    })
}

pub fn workflow_vm(app: &App, task_id: &str) -> Result<WorkflowVm> {
    let summary = app.task_summary(task_id)?;
    let task = task_row_vm(app, &summary)?;
    let authoring = app.task_authoring_workflow(task_id).ok();
    let workflow = authoring
        .as_ref()
        .map(|authoring| authoring.workflow.clone());
    let workflow_json = workflow
        .as_ref()
        .and_then(|workflow| serde_json::to_string_pretty(workflow).ok());
    let graph = workflow
        .as_ref()
        .map(|workflow| workflow_graph_vm(app, workflow))
        .unwrap_or_else(empty_graph);
    let control = workflow.as_ref().map(workflow_control_vm);
    let runs = newest_first(app.run_list(task_id)?)
        .into_iter()
        .map(|run| run_group_vm(app, task_id, run))
        .collect::<Result<Vec<_>>>()?;
    Ok(WorkflowVm {
        task,
        graph,
        runs,
        control,
        workflow_json,
        model_bindings: authoring
            .map(|authoring| authoring.model_bindings)
            .unwrap_or_default(),
    })
}

pub fn run_detail_vm(app: &App, task_id: &str, run_id: &str) -> Result<RunDetailVm> {
    let run = app.run_status(task_id, run_id)?;
    let progress = app.run_progress(task_id, run_id)?.filter(|progress| {
        progress
            .get("runtimeRevision")
            .and_then(serde_json::Value::as_u64)
            == Some(run.execution.revision)
    });
    let rounds = app
        .round_list(task_id, run_id)?
        .into_iter()
        .map(|round| round_summary_vm(app, task_id, &run, round))
        .collect::<Result<Vec<_>>>()?;
    Ok(RunDetailVm {
        run: run_summary_vm(run),
        rounds,
        events: app.run_events(task_id, run_id)?,
        progress,
    })
}

pub fn round_detail_vm(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    selection: Option<RoundSelectionInput>,
) -> Result<RoundDetailVm> {
    let run = app.run_status(task_id, run_id)?;
    let round = app
        .round_list(task_id, run_id)?
        .into_iter()
        .find(|round| round.id == round_id)
        .ok_or_else(|| anyhow::anyhow!("round not found: {round_id}"))?;
    let nodes = round_attempt_nodes(app, task_id, run_id, &round)?;
    let control_failure = latest_control_failure_vm(app, task_id, run_id)?;
    let graph = round_graph_vm(app, task_id, &run, &round, &nodes, control_failure.as_ref())?;
    let selection = selection.unwrap_or(RoundSelectionInput::Round {
        context_node_id: None,
    });
    let requirement = read_optional_text(&app.paths.requirement_file(task_id))?.unwrap_or_default();
    let selected_node_detail = selected_node_detail_vm(
        app, task_id, run_id, round_id, &run, &round, &nodes, &graph, &selection,
    )?;
    let control = read_json::<WorkflowDsl>(&app.paths.workflow_snapshot_file(task_id, run_id))
        .ok()
        .map(|workflow| workflow_control_vm(&workflow));

    Ok(RoundDetailVm {
        run: run_summary_vm(run.clone()),
        round: round_summary_vm(app, task_id, &run, round)?,
        graph,
        control,
        control_failure,
        requirement,
        selected_node_detail,
    })
}

pub fn run_summary_vm(run: RunState) -> RunSummaryVm {
    let resumable = is_run_continuable(&run);
    RunSummaryVm {
        id: run.id,
        task_id: run.task_id,
        status: enum_label(&run.status),
        outcome: run.outcome.map(|outcome| enum_label(&outcome)),
        started_at: run.started_at,
        updated_at: run.updated_at,
        current_round: run.current_round,
        current_node: run.current_node,
        current_attempt: run.current_attempt,
        resumable,
        pause_reason: run.pause_reason.map(|reason| enum_label(&reason)),
    }
}

fn task_row_vm(app: &App, summary: &TaskSummary) -> Result<TaskRowVm> {
    let requirement =
        read_optional_text(&app.paths.requirement_file(&summary.task.id))?.unwrap_or_default();
    let requirement_preview = preview_text(&requirement, 120);
    let (artifact_count, attachment_count) = count_task_outputs(app, &summary.task.id)?;
    Ok(TaskRowVm {
        id: summary.task.id.clone(),
        title: summary
            .task
            .title
            .clone()
            .unwrap_or_else(|| summary.task.id.clone()),
        description: summary.task.description.clone(),
        requirement,
        requirement_preview,
        display_status: display_status(summary),
        workflow_exists: summary.workflow_exists,
        workflow_valid: summary.workflow_valid,
        workflow_error: workflow_error_vm(summary),
        latest_run: summary.latest_run.clone().map(run_summary_vm),
        resumable_run_id: summary.resumable_run_id.clone(),
        artifact_count,
        attachment_count,
    })
}

fn workflow_error_vm(summary: &TaskSummary) -> Option<WorkflowErrorVm> {
    match &summary.workflow_validation_error {
        Some(WorkflowValidationError::MissingEndNode) => Some(WorkflowErrorVm {
            code: "workflow.missing-end-node".to_string(),
            params: serde_json::json!({}),
        }),
        Some(WorkflowValidationError::UnreachableNode { node_id }) => Some(WorkflowErrorVm {
            code: "workflow.unreachable-node".to_string(),
            params: serde_json::json!({ "nodeId": node_id }),
        }),
        Some(WorkflowValidationError::SuccessNewRoundTarget { from }) => Some(WorkflowErrorVm {
            code: "workflow.success-new-round-target".to_string(),
            params: serde_json::json!({ "from": from }),
        }),
        Some(WorkflowValidationError::MissingNewRoundEntry { from }) => Some(WorkflowErrorVm {
            code: "workflow.missing-new-round-entry".to_string(),
            params: serde_json::json!({ "from": from }),
        }),
        Some(WorkflowValidationError::InvalidNewRoundEntry { from, entry }) => {
            Some(WorkflowErrorVm {
                code: "workflow.invalid-new-round-entry".to_string(),
                params: serde_json::json!({ "from": from, "entry": entry }),
            })
        }
        Some(WorkflowValidationError::DuplicateWorkflowId {
            workflow_name,
            workflow_id,
            conflicts,
        }) => Some(WorkflowErrorVm {
            code: "workflow.duplicate-id".to_string(),
            params: serde_json::json!({
                "workflowName": workflow_name,
                "workflowId": workflow_id,
                "conflicts": conflicts,
            }),
        }),
        Some(WorkflowValidationError::AiDynamicInvalidWorkflow {
            node_id,
            workflow_name,
            reason,
        }) => Some(WorkflowErrorVm {
            code: "workflow.ai-dynamic-invalid-workflow".to_string(),
            params: serde_json::json!({
                "nodeId": node_id,
                "workflowName": workflow_name,
                "reason": reason,
            }),
        }),
        Some(WorkflowValidationError::WorkerModelBlank { node_id, provider }) => {
            Some(WorkflowErrorVm {
                code: "workflow.model-blank".to_string(),
                params: serde_json::json!({ "nodeId": node_id, "provider": provider }),
            })
        }
        Some(WorkflowValidationError::DynamicFixedModelBlank { node_id }) => {
            Some(WorkflowErrorVm {
                code: "workflow.dynamic-fixed-model-blank".to_string(),
                params: serde_json::json!({ "nodeId": node_id }),
            })
        }
        Some(WorkflowValidationError::DynamicAgentsEmpty { node_id }) => Some(WorkflowErrorVm {
            code: "workflow.dynamic-agents-empty".to_string(),
            params: serde_json::json!({ "nodeId": node_id }),
        }),
        Some(WorkflowValidationError::DynamicAgentDuplicate { node_id, provider }) => {
            Some(WorkflowErrorVm {
                code: "workflow.dynamic-agent-duplicate".to_string(),
                params: serde_json::json!({ "nodeId": node_id, "provider": provider }),
            })
        }
        Some(WorkflowValidationError::DynamicAgentModelBlank { node_id, provider }) => {
            Some(WorkflowErrorVm {
                code: "workflow.dynamic-agent-model-blank".to_string(),
                params: serde_json::json!({ "nodeId": node_id, "provider": provider }),
            })
        }
        Some(WorkflowValidationError::AgentModelBlank { provider }) => Some(WorkflowErrorVm {
            code: "workflow.agent-model-blank".to_string(),
            params: serde_json::json!({ "provider": provider }),
        }),
        None if summary.workflow_error.is_some() => Some(WorkflowErrorVm {
            code: "workflow.invalid".to_string(),
            params: serde_json::json!({}),
        }),
        None => None,
    }
}

fn display_status(summary: &TaskSummary) -> String {
    if !summary.workflow_exists {
        return "missing-workflow".to_string();
    }
    if !summary.workflow_valid {
        return "invalid".to_string();
    }
    match &summary.latest_run {
        Some(run) if run.status == RunStatus::Running => "running".to_string(),
        Some(run) if run.status == RunStatus::Paused => "resumable".to_string(),
        Some(run) if run.outcome == Some(RunOutcome::Failure) => "failed".to_string(),
        Some(run) if run.outcome == Some(RunOutcome::Killed) => "killed".to_string(),
        Some(run) if run.outcome == Some(RunOutcome::Success) => "completed".to_string(),
        _ => "ready".to_string(),
    }
}

fn run_group_vm(app: &App, task_id: &str, run: RunState) -> Result<RunGroupVm> {
    let rounds = app
        .round_list(task_id, &run.id)?
        .into_iter()
        .map(|round| round_summary_vm(app, task_id, &run, round))
        .collect::<Result<Vec<_>>>()?;
    Ok(RunGroupVm {
        run: run_summary_vm(run),
        rounds,
    })
}

fn round_summary_vm(
    app: &App,
    task_id: &str,
    run: &RunState,
    round: RoundState,
) -> Result<RoundSummaryVm> {
    let (artifact_count, attachment_count) =
        count_round_outputs(app, task_id, &round.run_id, &round.id)?;
    Ok(RoundSummaryVm {
        id: round.id.clone(),
        run_id: round.run_id,
        index: round.index,
        status: enum_label(&round.status),
        outcome: round.outcome.map(|outcome| enum_label(&outcome)),
        trigger: enum_label(&round.trigger),
        started_at: round.started_at,
        current_node: if run.current_round.as_deref() == Some(&round.id) {
            run.current_node.clone()
        } else {
            None
        },
        artifact_count,
        attachment_count,
    })
}

fn workflow_control_vm(workflow: &WorkflowDsl) -> WorkflowControlVm {
    WorkflowControlVm {
        max_attempts: workflow.control.max_attempts,
        max_rounds: workflow.control.max_rounds,
    }
}

pub(crate) fn latest_control_failure_vm(
    app: &App,
    task_id: &str,
    run_id: &str,
) -> Result<Option<ControlFailureVm>> {
    let mut latest = None;
    let events = app.run_events(task_id, run_id)?.unwrap_or_default();
    for line in events.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if event.get("type").and_then(|value| value.as_str())
            != Some("workflow_control_limit_exceeded")
        {
            continue;
        }
        let data = event.get("data").unwrap_or(&serde_json::Value::Null);
        let summary = data.get("summary").and_then(|value| value.as_str());
        latest = data
            .get("controlFailure")
            .or_else(|| data.get("control_failure"))
            .map(|failure| control_failure_from_value(failure, data, &event, summary))
            .or_else(|| {
                summary.and_then(|summary| control_failure_from_summary(summary, data, &event))
            });
    }
    Ok(latest)
}

fn control_failure_from_value(
    failure: &serde_json::Value,
    data: &serde_json::Value,
    event: &serde_json::Value,
    summary: Option<&str>,
) -> ControlFailureVm {
    let reason_kind = failure
        .get("reasonKind")
        .and_then(|value| value.as_str())
        .unwrap_or("workflow_control_limit_exceeded")
        .to_string();
    let message = failure
        .get("message")
        .and_then(|value| value.as_str())
        .or(summary)
        .unwrap_or("workflow control limit exceeded")
        .to_string();
    ControlFailureVm {
        title: control_failure_title(&reason_kind),
        reason_kind,
        message,
        from_node_id: failure
            .get("fromNodeId")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        to_node_id: failure
            .get("toNodeId")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        target: failure
            .get("target")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        edge_outcome: failure
            .get("edgeOutcome")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        proposed_count: failure
            .get("proposedCount")
            .and_then(|value| value.as_u64())
            .map(|value| value as u32),
        limit: failure
            .get("limit")
            .and_then(|value| value.as_u64())
            .map(|value| value as u32),
        timestamp: event
            .get("timestamp")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        round_id: data
            .get("roundId")
            .or_else(|| data.get("currentRoundId"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
        node_id: data
            .get("nodeId")
            .or_else(|| data.get("currentNodeId"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
        attempt_id: data
            .get("attemptId")
            .or_else(|| data.get("currentAttemptId"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
    }
}

fn control_failure_from_summary(
    summary: &str,
    data: &serde_json::Value,
    event: &serde_json::Value,
) -> Option<ControlFailureVm> {
    let (reason_kind, rest) = summary
        .strip_prefix("max repair attempts exceeded for ")
        .map(|rest| ("max_repair_attempts_exceeded", rest))
        .or_else(|| {
            summary
                .strip_prefix("max attempts exceeded for ")
                .map(|rest| ("max_repair_attempts_exceeded", rest))
        })
        .or_else(|| {
            summary
                .strip_prefix("max rounds exceeded for ")
                .map(|rest| ("max_rounds_exceeded", rest))
        })?;
    let (transition, counts) = rest.split_once(": ").unwrap_or((rest, ""));
    let (from_node_id, to_node_id, target) = if reason_kind == "max_rounds_exceeded" {
        (None, None, Some(transition.to_string()))
    } else {
        let (from, to) = transition.split_once(" -> ").unwrap_or((transition, ""));
        (
            Some(from.to_string()),
            Some(to.to_string()),
            Some(to.to_string()),
        )
    };
    let (proposed_count, limit) = counts
        .split_once(" > ")
        .map(|(left, right)| (left.parse::<u32>().ok(), right.parse::<u32>().ok()))
        .unwrap_or((None, None));
    Some(ControlFailureVm {
        title: control_failure_title(reason_kind),
        reason_kind: reason_kind.to_string(),
        message: summary.to_string(),
        from_node_id,
        to_node_id,
        target,
        edge_outcome: None,
        proposed_count,
        limit,
        timestamp: event
            .get("timestamp")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        round_id: data
            .get("roundId")
            .or_else(|| data.get("currentRoundId"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
        node_id: data
            .get("nodeId")
            .or_else(|| data.get("currentNodeId"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
        attempt_id: data
            .get("attemptId")
            .or_else(|| data.get("currentAttemptId"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
    })
}

fn control_failure_title(reason_kind: &str) -> String {
    match reason_kind {
        "max_repair_attempts_exceeded" => "修复次数已达上限".to_string(),
        "max_rounds_exceeded" => "Round 数已达上限".to_string(),
        _ => "工作流已停止".to_string(),
    }
}

fn round_attempt_nodes(
    app: &App,
    task_id: &str,
    run_id: &str,
    round: &RoundState,
) -> Result<Vec<NodeState>> {
    if round.trace.is_empty() {
        return app.node_list(task_id, run_id, &round.id);
    }

    let mut node_ids = Vec::<String>::new();
    for step in &round.trace {
        if !node_ids.iter().any(|node_id| node_id == &step.node_id) {
            node_ids.push(step.node_id.clone());
        }
    }

    let mut nodes = Vec::new();
    for node_id in node_ids {
        nodes.extend(app.attempt_list(task_id, run_id, &round.id, &node_id)?);
    }
    Ok(nodes)
}

pub fn workflow_graph_vm(app: &App, workflow: &WorkflowDsl) -> GraphVm {
    GraphVm {
        nodes: workflow
            .nodes
            .iter()
            .map(|node| GraphNodeVm {
                id: node.id().to_string(),
                node_id: Some(node.id().to_string()),
                sequence: None,
                label: node_label(node),
                node_type: enum_label(&node.node_type()),
                status: None,
                outcome: None,
                runtime_display: runtime_display_vm(None, None, false, None, false),
                attempt_id: None,
                outer_node_id: None,
                outer_attempt_id: None,
                attempt_count: 0,
                attempts: Vec::new(),
                artifact_count: 0,
                attachment_count: 0,
                current: false,
                icon_key: node
                    .provider()
                    .and_then(|provider| provider_icon_key(app, provider)),
                session_mode: None,
                continue_from_node_id: None,
                dynamic_summary: None,
                dynamic_group_id: None,
            })
            .collect(),
        edges: workflow
            .edges
            .iter()
            .map(|edge| GraphEdgeVm {
                from: edge.from.clone(),
                to: edge.to.clone(),
                label: enum_label(&edge.on),
                traversal_count: 0,
                last_outcome: None,
                blocked_reason: None,
            })
            .collect(),
    }
}

fn round_graph_vm(
    app: &App,
    task_id: &str,
    run: &RunState,
    round: &RoundState,
    nodes: &[NodeState],
    control_failure: Option<&ControlFailureVm>,
) -> Result<GraphVm> {
    let node_labels = workflow_node_labels(app, task_id, &run.id);
    if !round.trace.is_empty() {
        return round_trace_graph_vm(
            app,
            task_id,
            run,
            round,
            nodes,
            &node_labels,
            control_failure,
        );
    }

    let mut ordered_nodes = nodes.to_vec();
    ordered_nodes.sort_by(|left, right| {
        left.started_at
            .cmp(&right.started_at)
            .then_with(|| left.attempt_id.cmp(&right.attempt_id))
    });
    let graph_nodes = ordered_nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            round_node_graph_vm(
                app,
                task_id,
                run,
                round,
                node,
                index as u32 + 1,
                &node_labels,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let edges = graph_nodes
        .windows(2)
        .map(|pair| GraphEdgeVm {
            from: pair[0].id.clone(),
            to: pair[1].id.clone(),
            label: "observed".to_string(),
            traversal_count: 1,
            last_outcome: None,
            blocked_reason: None,
        })
        .collect();

    Ok(GraphVm {
        nodes: graph_nodes,
        edges,
    })
}

fn round_trace_graph_vm(
    app: &App,
    task_id: &str,
    run: &RunState,
    round: &RoundState,
    nodes: &[NodeState],
    node_labels: &HashMap<String, String>,
    control_failure: Option<&ControlFailureVm>,
) -> Result<GraphVm> {
    const TRACE_SEQUENCE_RANK_WIDTH: u32 = 100;
    fn trace_rank_sequence(sequence: u32) -> u32 {
        sequence.saturating_mul(TRACE_SEQUENCE_RANK_WIDTH)
    }

    let mut steps = round.trace.clone();
    steps.sort_by_key(|step| step.sequence);

    let mut graph_nodes = Vec::<GraphNodeVm>::new();
    let mut graph_edges = Vec::<GraphEdgeVm>::new();
    let mut added_ids = HashSet::<String>::new();
    let mut ai_dynamic_entry_map = HashMap::<String, String>::new();
    let mut ai_dynamic_terminal_map = HashMap::<String, Vec<String>>::new();

    for step in &steps {
        let Some(node) = nodes
            .iter()
            .find(|node| node.node_id == step.node_id && node.attempt_id == step.attempt_id)
        else {
            continue;
        };

        if node.node_type == NodeType::AiDynamic {
            if let Some(dynamic_graph) = dynamic_graph_state_optional(
                app,
                task_id,
                &run.id,
                &round.id,
                &node.node_id,
                &node.attempt_id,
            ) {
                let base_sequence = trace_rank_sequence(step.sequence);
                let pause_reason = run.pause_reason.as_ref().map(enum_label);
                let run_resumable = is_run_continuable(run);
                let mut internal_nodes = dynamic_graph
                    .nodes
                    .iter()
                    .enumerate()
                    .map(|(index, dynamic_node)| {
                        let current = run.current_round.as_deref() == Some(&round.id)
                            && run.current_node.as_deref() == Some(&node.node_id)
                            && dynamic_graph
                                .run
                                .current_node_ids
                                .iter()
                                .any(|id| id == &dynamic_node.id);
                        dynamic_node_graph_vm(
                            app,
                            task_id,
                            &run.id,
                            &round.id,
                            &node.node_id,
                            &node.attempt_id,
                            dynamic_node,
                            index as u32 + 1,
                            Some(base_sequence + index as u32 + 1),
                            current,
                            pause_reason.as_deref(),
                            run_resumable,
                        )
                    })
                    .collect::<Vec<_>>();

                if let Some(first) = internal_nodes.first() {
                    ai_dynamic_entry_map.insert(node.node_id.clone(), first.id.clone());
                }
                ai_dynamic_terminal_map.insert(
                    node.node_id.clone(),
                    dynamic_external_exit_graph_node_ids(
                        &node.node_id,
                        &node.attempt_id,
                        &dynamic_graph,
                    ),
                );

                for vm in internal_nodes.drain(..) {
                    if added_ids.insert(vm.id.clone()) {
                        graph_nodes.push(vm);
                    }
                }

                let internal_graph = dynamic_internal_graph_vm(
                    app,
                    task_id,
                    &run.id,
                    &round.id,
                    &node.node_id,
                    &node.attempt_id,
                    &dynamic_graph,
                );
                for edge in internal_graph.edges {
                    if let Some(existing) = graph_edges.iter_mut().find(|item| {
                        item.from == edge.from && item.to == edge.to && item.label == edge.label
                    }) {
                        existing.traversal_count += edge.traversal_count;
                        existing.last_outcome =
                            edge.last_outcome.clone().or(existing.last_outcome.clone());
                    } else {
                        graph_edges.push(edge);
                    }
                }
                continue;
            }
        }

        if added_ids.contains(&node.node_id) {
            continue;
        }
        let node_steps = steps
            .iter()
            .filter(|candidate| candidate.node_id == step.node_id)
            .collect::<Vec<_>>();
        let latest_step = node_steps
            .last()
            .expect("node_steps is non-empty because it is built from current node_id");
        let latest_node = nodes.iter().find(|candidate| {
            candidate.node_id == latest_step.node_id
                && candidate.attempt_id == latest_step.attempt_id
        });
        let first_sequence = node_steps
            .first()
            .map(|candidate| trace_rank_sequence(candidate.sequence));
        let mut attempts = Vec::new();
        for node_step in &node_steps {
            if let Some(node_attempt) = nodes.iter().find(|candidate| {
                candidate.node_id == node_step.node_id
                    && candidate.attempt_id == node_step.attempt_id
            }) {
                attempts.push(graph_attempt_vm(
                    app,
                    task_id,
                    run,
                    round,
                    node_step,
                    node_attempt,
                )?);
            }
        }
        let artifacts = app
            .artifact_list(
                task_id,
                &run.id,
                &round.id,
                &latest_step.node_id,
                &latest_step.attempt_id,
            )?
            .len();
        let attachments = app
            .attachment_list(
                task_id,
                &run.id,
                &round.id,
                &latest_step.node_id,
                &latest_step.attempt_id,
            )?
            .len();
        let latest_status = latest_node.map(|node| enum_label(&node.status));
        let latest_outcome =
            latest_node.and_then(|node| node.outcome.map(|outcome| enum_label(&outcome)));
        let current = run.current_round.as_deref() == Some(&round.id)
            && run.current_node.as_deref() == Some(&latest_step.node_id);
        let pause_reason = run.pause_reason.as_ref().map(enum_label);
        let runtime_display = runtime_display_vm(
            latest_status.as_deref(),
            latest_outcome.as_deref(),
            current,
            pause_reason.as_deref(),
            is_run_continuable(run),
        );
        graph_nodes.push(GraphNodeVm {
            id: latest_step.node_id.clone(),
            node_id: Some(latest_step.node_id.clone()),
            sequence: first_sequence,
            label: node_labels
                .get(&latest_step.node_id)
                .cloned()
                .unwrap_or_else(|| latest_step.node_id.clone()),
            node_type: latest_node
                .map(|node| enum_label(&node.node_type))
                .unwrap_or_else(|| "unknown".to_string()),
            status: latest_status,
            outcome: latest_outcome,
            runtime_display,
            attempt_id: Some(latest_step.attempt_id.clone()),
            outer_node_id: None,
            outer_attempt_id: None,
            attempt_count: attempts.len(),
            attempts,
            artifact_count: artifacts,
            attachment_count: attachments,
            current,
            icon_key: latest_node.and_then(|n| {
                n.resolved_config
                    .get("provider")
                    .and_then(|v| v.as_str())
                    .and_then(|provider| provider_icon_key(app, provider))
            }),
            session_mode: None,
            continue_from_node_id: None,
            dynamic_summary: latest_node
                .filter(|candidate| candidate.node_type == NodeType::AiDynamic)
                .and_then(|candidate| {
                    dynamic_graph_state_optional(
                        app,
                        task_id,
                        &run.id,
                        &round.id,
                        &candidate.node_id,
                        &candidate.attempt_id,
                    )
                    .map(|graph| dynamic_summary_vm(&graph))
                }),
            dynamic_group_id: None,
        });
        added_ids.insert(node.node_id.clone());
    }

    for pair in steps.windows(2) {
        let mut from_ids = if let Some(terminals) = ai_dynamic_terminal_map.get(&pair[0].node_id) {
            terminals.clone()
        } else {
            vec![pair[0].node_id.clone()]
        };
        if from_ids.is_empty() {
            from_ids.push(pair[0].node_id.clone());
        }
        let to_id = ai_dynamic_entry_map
            .get(&pair[1].node_id)
            .cloned()
            .unwrap_or_else(|| pair[1].node_id.clone());
        let label = pair[1].edge_outcome.clone().unwrap_or_default();
        for from in from_ids {
            if let Some(edge) = graph_edges
                .iter_mut()
                .find(|edge| edge.from == from && edge.to == to_id && edge.label == label)
            {
                edge.traversal_count += 1;
                edge.last_outcome = Some(label.clone());
                continue;
            }
            let blocked_reason = control_failure.and_then(|failure| {
                let from_match = failure.from_node_id.as_deref() == Some(pair[0].node_id.as_str());
                let to_match = failure.to_node_id.as_deref() == Some(pair[1].node_id.as_str())
                    || failure.target.as_deref() == Some(pair[1].node_id.as_str());
                let outcome_match = failure
                    .edge_outcome
                    .as_deref()
                    .map_or(true, |outcome| outcome == label);
                (from_match && to_match && outcome_match).then(|| failure.clone())
            });
            graph_edges.push(GraphEdgeVm {
                from,
                to: to_id.clone(),
                label: label.clone(),
                traversal_count: 1,
                last_outcome: Some(label.clone()),
                blocked_reason,
            });
        }
    }

    graph_nodes.sort_by(|left, right| {
        left.sequence
            .unwrap_or_default()
            .cmp(&right.sequence.unwrap_or_default())
            .then_with(|| left.id.cmp(&right.id))
    });

    Ok(GraphVm {
        nodes: graph_nodes,
        edges: graph_edges,
    })
}

fn read_worker_ref_optional(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
) -> Option<WorkerRefState> {
    let path = app
        .paths
        .worker_ref_file(task_id, run_id, round_id, node_id, attempt_id);
    path.exists()
        .then(|| read_json::<WorkerRefState>(&path).ok())
        .flatten()
}

fn worker_ref_session_mode(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
) -> Option<String> {
    read_worker_ref_optional(app, task_id, run_id, round_id, node_id, attempt_id)
        .map(|worker_ref| enum_label(&worker_ref.mode))
}

fn worker_ref_acp_session_id(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
) -> Option<String> {
    read_worker_ref_optional(app, task_id, run_id, round_id, node_id, attempt_id)
        .and_then(|worker_ref| worker_ref.continue_ref)
        .and_then(|value| {
            value
                .get("acpSessionId")
                .or_else(|| value.get("sessionId"))
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
}

fn graph_attempt_vm(
    app: &App,
    task_id: &str,
    run: &RunState,
    round: &RoundState,
    step: &RoundTraceStep,
    node: &NodeState,
) -> Result<GraphAttemptVm> {
    let status = enum_label(&node.status);
    let outcome = node.outcome.map(|outcome| enum_label(&outcome));
    let current = run.current_round.as_deref() == Some(&round.id)
        && run.current_node.as_deref() == Some(&node.node_id)
        && run.current_attempt.as_deref() == Some(&node.attempt_id);
    let pause_reason = run.pause_reason.as_ref().map(enum_label);
    let runtime_display = runtime_display_vm(
        Some(&status),
        outcome.as_deref(),
        current,
        pause_reason.as_deref(),
        is_run_continuable(run),
    );
    Ok(GraphAttemptVm {
        attempt_id: step.attempt_id.clone(),
        sequence: Some(step.sequence),
        status,
        outcome,
        runtime_display,
        session_mode: worker_ref_session_mode(
            app,
            task_id,
            &run.id,
            &round.id,
            &node.node_id,
            &node.attempt_id,
        ),
        acp_session_id: worker_ref_acp_session_id(
            app,
            task_id,
            &run.id,
            &round.id,
            &node.node_id,
            &node.attempt_id,
        ),
        current,
    })
}

fn dynamic_graph_state_optional(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
) -> Option<DynamicGraphState> {
    let path = app
        .paths
        .dynamic_graph_file(task_id, run_id, round_id, node_id, attempt_id);
    path.exists()
        .then(|| load_dynamic_graph(&path, &app.paths.repo_root).ok())
        .flatten()
}

fn dynamic_summary_vm(graph: &DynamicGraphState) -> DynamicSummaryVm {
    DynamicSummaryVm {
        status: enum_label(&graph.run.status),
        outcome: graph.run.outcome.map(|outcome| enum_label(&outcome)),
        internal_node_count: graph.nodes.len(),
        group_count: graph.groups.len(),
        proposal_count: graph.proposals.len(),
        current_node_ids: graph.run.current_node_ids.clone(),
    }
}

fn count_dir_entries(path: &camino::Utf8Path) -> usize {
    fs::read_dir(path)
        .map(|entries| entries.filter_map(|entry| entry.ok()).count())
        .unwrap_or(0)
}

fn latest_dynamic_attempt_id(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    node_id: &str,
) -> String {
    let node_dir = app.paths.dynamic_node_dir(
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        node_id,
    );
    let mut attempts = fs::read_dir(node_dir.as_std_path())
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    attempts.sort();
    attempts.pop().unwrap_or_else(|| "attempt-001".to_string())
}

fn dynamic_node_graph_vm(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    node: &gold_band::dynamic::DynamicNodeState,
    sequence: u32,
    sequence_hint: Option<u32>,
    current: bool,
    pause_reason: Option<&str>,
    resumable: bool,
) -> GraphNodeVm {
    let attempt_id = latest_dynamic_attempt_id(
        app,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        &node.id,
    );
    let artifact_count = count_dir_entries(&app.paths.dynamic_node_artifacts_dir(
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        &node.id,
        &attempt_id,
    ));
    let attachment_count = count_dir_entries(&app.paths.dynamic_node_attachments_dir(
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        &node.id,
        &attempt_id,
    ));
    let acp_session_id = read_json::<WorkerRefState>(&app.paths.dynamic_node_worker_ref_file(
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        &node.id,
        &attempt_id,
    ))
    .ok()
    .and_then(|worker_ref| worker_ref.continue_ref)
    .and_then(|value| {
        value
            .get("acpSessionId")
            .or_else(|| value.get("sessionId"))
            .and_then(|value| value.as_str())
            .map(str::to_string)
    });
    let status = enum_label(&node.status);
    let outcome = node.outcome.map(|outcome| enum_label(&outcome));
    let runtime_display = runtime_display_vm(
        Some(&status),
        outcome.as_deref(),
        current,
        pause_reason,
        resumable,
    );
    GraphNodeVm {
        id: dynamic_graph_node_vm_id(outer_node_id, outer_attempt_id, &node.id),
        node_id: Some(node.id.clone()),
        sequence: Some(sequence_hint.unwrap_or(sequence)),
        label: node.title.clone(),
        node_type: format!("dynamic-{}", enum_label(&node.kind)),
        status: Some(status.clone()),
        outcome: outcome.clone(),
        runtime_display: runtime_display.clone(),
        attempt_id: Some(attempt_id.clone()),
        outer_node_id: Some(outer_node_id.to_string()),
        outer_attempt_id: Some(outer_attempt_id.to_string()),
        attempt_count: 1,
        attempts: vec![GraphAttemptVm {
            attempt_id,
            sequence: Some(sequence_hint.unwrap_or(sequence)),
            status,
            outcome,
            runtime_display,
            session_mode: Some(enum_label(&node.session_mode)),
            acp_session_id,
            current,
        }],
        artifact_count,
        attachment_count,
        current,
        icon_key: node
            .provider
            .as_deref()
            .and_then(|provider| provider_icon_key(app, provider)),
        session_mode: Some(enum_label(&node.session_mode)),
        continue_from_node_id: node.continue_from_node_id.clone(),
        dynamic_summary: None,
        dynamic_group_id: node.group_id.clone(),
    }
}

fn dynamic_internal_graph_vm(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    graph: &DynamicGraphState,
) -> GraphVm {
    let nodes = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let current = graph.run.current_node_ids.iter().any(|id| id == &node.id);
            let pause_reason = node
                .pause_reason
                .as_ref()
                .or(graph.run.pause_reason.as_ref())
                .map(enum_label);
            let run_status = enum_label(&graph.run.status);
            dynamic_node_graph_vm(
                app,
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
                node,
                index as u32 + 1,
                Some(index as u32 + 1),
                current,
                pause_reason.as_deref(),
                run_status == "paused",
            )
        })
        .collect::<Vec<_>>();

    let mut edges = Vec::new();
    for node in &graph.nodes {
        let to = dynamic_graph_node_vm_id(outer_node_id, outer_attempt_id, &node.id);
        let mut has_dependency = false;
        for dependency in &node.depends_on {
            has_dependency = true;
            edges.push(GraphEdgeVm {
                from: dynamic_graph_node_vm_id(outer_node_id, outer_attempt_id, dependency),
                to: to.clone(),
                label: "depends-on".to_string(),
                traversal_count: 1,
                last_outcome: None,
                blocked_reason: None,
            });
        }
        if !has_dependency {
            let upstream = dynamic_implicit_upstream_node(graph, node);
            if let Some(upstream) = upstream {
                edges.push(GraphEdgeVm {
                    from: dynamic_graph_node_vm_id(outer_node_id, outer_attempt_id, &upstream.id),
                    to: to.clone(),
                    label: "success".to_string(),
                    traversal_count: 1,
                    last_outcome: Some("success".to_string()),
                    blocked_reason: None,
                });
            }
        }
        if node.session_mode == SessionMode::Continue {
            if let Some(continue_from_node_id) = &node.continue_from_node_id {
                edges.push(GraphEdgeVm {
                    from: dynamic_graph_node_vm_id(
                        outer_node_id,
                        outer_attempt_id,
                        continue_from_node_id,
                    ),
                    to: to.clone(),
                    label: "continue".to_string(),
                    traversal_count: 1,
                    last_outcome: None,
                    blocked_reason: None,
                });
            }
        }
    }

    GraphVm { nodes, edges }
}

fn dynamic_graph_node_vm_id(outer_node_id: &str, outer_attempt_id: &str, node_id: &str) -> String {
    format!("{outer_node_id}::{outer_attempt_id}::{node_id}")
}

fn dynamic_external_exit_graph_node_ids(
    outer_node_id: &str,
    outer_attempt_id: &str,
    graph: &DynamicGraphState,
) -> Vec<String> {
    let mut non_exit_node_ids = HashSet::<String>::new();
    for node in &graph.nodes {
        for dependency in &node.depends_on {
            non_exit_node_ids.insert(dependency.clone());
        }
        if let Some(upstream) = dynamic_implicit_upstream_node(graph, node) {
            non_exit_node_ids.insert(upstream.id.clone());
        }
        if node.session_mode == SessionMode::Continue {
            if let Some(continue_from_node_id) = &node.continue_from_node_id {
                non_exit_node_ids.insert(continue_from_node_id.clone());
            }
        }
    }

    graph
        .nodes
        .iter()
        .filter(|node| !non_exit_node_ids.contains(&node.id))
        .map(|node| dynamic_graph_node_vm_id(outer_node_id, outer_attempt_id, &node.id))
        .collect()
}

fn dynamic_implicit_upstream_node<'a>(
    graph: &'a DynamicGraphState,
    node: &gold_band::dynamic::DynamicNodeState,
) -> Option<&'a gold_band::dynamic::DynamicNodeState> {
    if !node.depends_on.is_empty() || node.depth == 0 {
        return None;
    }
    graph
        .nodes
        .iter()
        .find(|candidate| candidate.chain_id == node.chain_id && candidate.depth + 1 == node.depth)
        .or_else(|| {
            node.group_id.as_deref().and_then(|group_id| {
                graph
                    .groups
                    .iter()
                    .find(|group| {
                        group.id == group_id && group.root_node_ids.iter().any(|id| id == &node.id)
                    })
                    .map(|group| &group.created_by_node_id)
                    .and_then(|source_id| {
                        graph
                            .nodes
                            .iter()
                            .find(|candidate| candidate.id == *source_id)
                    })
            })
        })
}

pub fn dynamic_runtime_graph_vm(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
) -> Option<GraphVm> {
    dynamic_graph_state_optional(
        app,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
    )
    .map(|graph| {
        dynamic_internal_graph_vm(
            app,
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            &graph,
        )
    })
}

fn dynamic_detail_vm(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    graph: &DynamicGraphState,
) -> DynamicDetailVm {
    DynamicDetailVm {
        summary: dynamic_summary_vm(graph),
        graph: dynamic_internal_graph_vm(
            app,
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            graph,
        ),
        groups: graph
            .groups
            .iter()
            .map(|group| DynamicGroupVm {
                id: group.id.clone(),
                status: enum_label(&group.status),
                depth: group.depth,
                parent_group_id: group.parent_group_id.clone(),
                root_node_ids: group.root_node_ids.clone(),
                terminal_node_ids: group.terminal_node_ids.clone(),
                merge_node_id: group.merge_node_id.clone(),
                acceptance_node_id: group.acceptance_node_id.clone(),
            })
            .collect(),
        proposals: graph
            .proposals
            .iter()
            .map(|proposal| DynamicProposalVm {
                id: proposal.id.clone(),
                source_node_id: proposal.source_node_id.clone(),
                validation_status: enum_label(&proposal.validation_status),
                validation_errors: proposal
                    .validation_errors
                    .iter()
                    .map(|error| DynamicProposalValidationErrorVm {
                        code: error.code.clone(),
                        message: error.message.clone(),
                        params: error.params.clone(),
                    })
                    .collect(),
                artifact_path: proposal.artifact_path.to_string(),
                created_at: proposal.created_at.clone(),
            })
            .collect(),
    }
}

fn round_node_graph_vm(
    app: &App,
    task_id: &str,
    run: &RunState,
    round: &RoundState,
    node: &NodeState,
    sequence: u32,
    node_labels: &HashMap<String, String>,
) -> Result<GraphNodeVm> {
    let artifacts = app
        .artifact_list(task_id, &run.id, &round.id, &node.node_id, &node.attempt_id)?
        .len();
    let attachments = app
        .attachment_list(task_id, &run.id, &round.id, &node.node_id, &node.attempt_id)?
        .len();
    let dynamic_summary = (node.node_type == NodeType::AiDynamic)
        .then(|| {
            dynamic_graph_state_optional(
                app,
                task_id,
                &run.id,
                &round.id,
                &node.node_id,
                &node.attempt_id,
            )
            .map(|graph| dynamic_summary_vm(&graph))
        })
        .flatten();
    let status = enum_label(&node.status);
    let outcome = node.outcome.map(|outcome| enum_label(&outcome));
    let node_current = run.current_round.as_deref() == Some(&round.id)
        && run.current_node.as_deref() == Some(&node.node_id);
    let attempt_current = node_current && run.current_attempt.as_deref() == Some(&node.attempt_id);
    let pause_reason = run.pause_reason.as_ref().map(enum_label);
    let run_resumable = is_run_continuable(run);
    let runtime_display = runtime_display_vm(
        Some(&status),
        outcome.as_deref(),
        node_current,
        pause_reason.as_deref(),
        run_resumable,
    );
    let attempt_runtime_display = runtime_display_vm(
        Some(&status),
        outcome.as_deref(),
        attempt_current,
        pause_reason.as_deref(),
        run_resumable,
    );
    Ok(GraphNodeVm {
        id: format!("{}:{}:{}", sequence, node.node_id, node.attempt_id),
        node_id: Some(node.node_id.clone()),
        sequence: Some(sequence),
        label: node_labels
            .get(&node.node_id)
            .cloned()
            .unwrap_or_else(|| node.node_id.clone()),
        node_type: enum_label(&node.node_type),
        status: Some(status.clone()),
        outcome: outcome.clone(),
        runtime_display: runtime_display.clone(),
        attempt_id: Some(node.attempt_id.clone()),
        outer_node_id: None,
        outer_attempt_id: None,
        attempt_count: 1,
        attempts: vec![GraphAttemptVm {
            attempt_id: node.attempt_id.clone(),
            sequence: Some(sequence),
            status,
            outcome,
            runtime_display: attempt_runtime_display,
            session_mode: worker_ref_session_mode(
                app,
                task_id,
                &run.id,
                &round.id,
                &node.node_id,
                &node.attempt_id,
            ),
            acp_session_id: worker_ref_acp_session_id(
                app,
                task_id,
                &run.id,
                &round.id,
                &node.node_id,
                &node.attempt_id,
            ),
            current: attempt_current,
        }],
        artifact_count: artifacts,
        attachment_count: attachments,
        current: node_current,
        icon_key: node
            .resolved_config
            .get("provider")
            .and_then(|v| v.as_str())
            .and_then(|provider| provider_icon_key(app, provider)),
        session_mode: None,
        continue_from_node_id: None,
        dynamic_summary,
        dynamic_group_id: None,
    })
}

fn selected_node_id(selection: &RoundSelectionInput) -> Option<&str> {
    match selection {
        RoundSelectionInput::Node { node_id, .. }
        | RoundSelectionInput::Artifact { node_id, .. }
        | RoundSelectionInput::Attachment { node_id, .. }
        | RoundSelectionInput::WorkerRef { node_id, .. } => Some(node_id),
        RoundSelectionInput::Log {
            node_id: Some(node_id),
            ..
        } => Some(node_id),
        RoundSelectionInput::Event {
            node_id: Some(node_id),
            ..
        } => Some(node_id),
        RoundSelectionInput::Round { context_node_id }
        | RoundSelectionInput::Requirement { context_node_id }
        | RoundSelectionInput::Event {
            context_node_id, ..
        }
        | RoundSelectionInput::Log {
            context_node_id, ..
        } => context_node_id.as_deref(),
    }
}

fn selected_attempt_id(selection: &RoundSelectionInput) -> Option<&str> {
    match selection {
        RoundSelectionInput::Node { attempt_id, .. }
        | RoundSelectionInput::Artifact { attempt_id, .. }
        | RoundSelectionInput::Attachment { attempt_id, .. }
        | RoundSelectionInput::WorkerRef { attempt_id, .. }
        | RoundSelectionInput::Event { attempt_id, .. }
        | RoundSelectionInput::Log { attempt_id, .. } => attempt_id.as_deref(),
        RoundSelectionInput::Round { .. } | RoundSelectionInput::Requirement { .. } => None,
    }
}

fn selected_outer_locator(selection: &RoundSelectionInput) -> (Option<&str>, Option<&str>) {
    match selection {
        RoundSelectionInput::Node {
            outer_node_id,
            outer_attempt_id,
            ..
        } => (outer_node_id.as_deref(), outer_attempt_id.as_deref()),
        _ => (None, None),
    }
}

fn selected_node_detail_vm(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    run: &RunState,
    round: &RoundState,
    nodes: &[NodeState],
    graph: &GraphVm,
    selection: &RoundSelectionInput,
) -> Result<Option<NodeDetailVm>> {
    let Some(node_id) = selected_node_id(selection) else {
        return Ok(None);
    };
    let (outer_node_id, outer_attempt_id) = selected_outer_locator(selection);
    if let (Some(outer_node_id), Some(outer_attempt_id)) = (outer_node_id, outer_attempt_id) {
        return selected_dynamic_node_detail_vm(
            app,
            task_id,
            run_id,
            round_id,
            run,
            round,
            graph,
            node_id,
            selected_attempt_id(selection),
            outer_node_id,
            outer_attempt_id,
        );
    }
    let node_attempts = nodes
        .iter()
        .filter(|node| node.node_id == node_id)
        .collect::<Vec<_>>();
    let Some(node) = selected_attempt_id(selection)
        .and_then(|attempt_id| {
            node_attempts
                .iter()
                .copied()
                .find(|node| node.attempt_id == attempt_id)
        })
        .or_else(|| {
            node_attempts.iter().copied().find(|node| {
                run.current_round.as_deref() == Some(&round.id)
                    && run.current_node.as_deref() == Some(node_id)
                    && run.current_attempt.as_deref() == Some(&node.attempt_id)
            })
        })
        .or_else(|| {
            node_attempts
                .iter()
                .copied()
                .max_by(|left, right| left.attempt_id.cmp(&right.attempt_id))
        })
    else {
        return Ok(None);
    };
    let graph_node = graph
        .nodes
        .iter()
        .find(|item| item.node_id.as_deref() == Some(node_id) || item.id == node_id);
    let provider = node
        .resolved_config
        .get("provider")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);
    let provider_display_name = provider
        .as_deref()
        .and_then(|provider| app.managed_agent(provider).ok())
        .map(|(_, agent)| agent.adapter.display_name.clone());
    let artifacts = app
        .artifact_list(task_id, run_id, round_id, node_id, &node.attempt_id)?
        .into_iter()
        .map(|name| asset_item_vm("artifact", round_id, node_id, &node.attempt_id, name))
        .collect::<Vec<_>>();
    let attachments = app
        .attachment_list(task_id, run_id, round_id, node_id, &node.attempt_id)?
        .into_iter()
        .map(|name| asset_item_vm("attachment", round_id, node_id, &node.attempt_id, name))
        .collect::<Vec<_>>();
    let worker_ref_exists = app
        .paths
        .worker_ref_file(task_id, run_id, round_id, node_id, &node.attempt_id)
        .exists();
    let manual_check_enabled = node
        .resolved_config
        .get("manualCheck")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let acp_session = acp_session_vm(
        app,
        task_id,
        run_id,
        round_id,
        node_id,
        &node.attempt_id,
        None,
        None,
    )?;
    let acp_conversations = acp_conversations_vm(app, task_id, run_id, round, node_id, nodes)?;
    let selected_conversation_key = acp_conversations
        .iter()
        .find(|conversation| {
            conversation
                .attempts
                .iter()
                .any(|attempt| attempt.attempt_id == node.attempt_id)
        })
        .map(|conversation| conversation.key.clone());
    let dynamic = if node.node_type == NodeType::AiDynamic {
        dynamic_graph_state_optional(app, task_id, run_id, round_id, node_id, &node.attempt_id).map(
            |graph| {
                dynamic_detail_vm(
                    app,
                    task_id,
                    run_id,
                    round_id,
                    node_id,
                    &node.attempt_id,
                    &graph,
                )
            },
        )
    } else {
        None
    };

    Ok(Some(NodeDetailVm {
        id: graph_node
            .map(|node| node.id.clone())
            .unwrap_or_else(|| node_id.to_string()),
        node_id: node_id.to_string(),
        sequence: graph_node.and_then(|node| node.sequence),
        label: graph_node
            .map(|node| node.label.clone())
            .unwrap_or_else(|| node_id.to_string()),
        node_type: enum_label(&node.node_type),
        provider,
        provider_display_name,
        status: enum_label(&node.status),
        outcome: node.outcome.map(|outcome| enum_label(&outcome)),
        attempt_id: node.attempt_id.clone(),
        outer_node_id: None,
        outer_attempt_id: None,
        current: run.current_round.as_deref() == Some(&round.id)
            && run.current_node.as_deref() == Some(node_id)
            && run.current_attempt.as_deref() == Some(&node.attempt_id),
        started_at: node.started_at.clone(),
        finished_at: node.finished_at.clone(),
        artifact_count: artifacts.len(),
        attachment_count: attachments.len(),
        artifacts,
        attachments,
        has_progress_events: app.attempt_log_exists(
            task_id,
            run_id,
            round_id,
            node_id,
            &node.attempt_id,
            LogSource::ProgressEvents,
        ),
        has_raw_stream: app.attempt_log_exists(
            task_id,
            run_id,
            round_id,
            node_id,
            &node.attempt_id,
            LogSource::RawStream,
        ),
        has_worker_ref: worker_ref_exists,
        manual_check_enabled,
        manual_check_pending: node.manual_check_pending,
        session_mode: None,
        continue_from_node_id: None,
        acp_session,
        acp_conversations,
        selected_conversation_key,
        dynamic,
        dynamic_group_id: None,
    }))
}

fn trace_sequence_for_attempt(round: &RoundState, node_id: &str, attempt_id: &str) -> Option<u32> {
    round
        .trace
        .iter()
        .find(|step| step.node_id == node_id && step.attempt_id == attempt_id)
        .map(|step| step.sequence)
}

fn selected_dynamic_node_detail_vm(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    run: &RunState,
    round: &RoundState,
    graph: &GraphVm,
    node_id: &str,
    attempt_id: Option<&str>,
    outer_node_id: &str,
    outer_attempt_id: &str,
) -> Result<Option<NodeDetailVm>> {
    let Some(dynamic_graph) = dynamic_graph_state_optional(
        app,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
    ) else {
        return Ok(None);
    };
    let dynamic_node = dynamic_graph
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .cloned();
    let Some(node) = dynamic_node else {
        return Ok(None);
    };
    let dynamic_attempt_id = attempt_id.map(str::to_string).unwrap_or_else(|| {
        latest_dynamic_attempt_id(
            app,
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            &node.id,
        )
    });
    let graph_node = graph.nodes.iter().find(|item| {
        item.node_id.as_deref() == Some(node_id)
            && item.outer_node_id.as_deref() == Some(outer_node_id)
            && item.outer_attempt_id.as_deref() == Some(outer_attempt_id)
    });
    let provider = node.provider.clone();
    let provider_display_name = provider
        .as_deref()
        .and_then(|provider| app.managed_agent(provider).ok())
        .map(|(_, agent)| agent.adapter.display_name.clone());
    let artifacts_dir = app.paths.dynamic_node_artifacts_dir(
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        node_id,
        &dynamic_attempt_id,
    );
    let attachments_dir = app.paths.dynamic_node_attachments_dir(
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        node_id,
        &dynamic_attempt_id,
    );
    let artifacts = std::fs::read_dir(artifacts_dir.as_std_path())
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .map(|name| {
                    asset_item_vm(
                        "artifact",
                        round_id,
                        node_id,
                        &dynamic_attempt_id,
                        name.strip_suffix(".json").unwrap_or(&name).to_string(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let attachments = std::fs::read_dir(attachments_dir.as_std_path())
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .map(|name| {
                    asset_item_vm("attachment", round_id, node_id, &dynamic_attempt_id, name)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let acp_session = dynamic_acp_session_vm(
        app,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        node_id,
        &dynamic_attempt_id,
        None,
        None,
    )?;
    Ok(Some(NodeDetailVm {
        id: graph_node
            .map(|node| node.id.clone())
            .unwrap_or_else(|| node_id.to_string()),
        node_id: node_id.to_string(),
        sequence: graph_node.and_then(|node| node.sequence),
        label: graph_node
            .map(|node| node.label.clone())
            .unwrap_or_else(|| node.title.clone()),
        node_type: enum_label(&node.kind),
        provider,
        provider_display_name,
        status: enum_label(&node.status),
        outcome: node.outcome.map(|outcome| enum_label(&outcome)),
        attempt_id: dynamic_attempt_id.clone(),
        outer_node_id: Some(outer_node_id.to_string()),
        outer_attempt_id: Some(outer_attempt_id.to_string()),
        current: run.current_round.as_deref() == Some(&round.id)
            && dynamic_graph
                .run
                .current_node_ids
                .iter()
                .any(|id| id == node_id),
        started_at: node.started_at.unwrap_or_else(|| round.started_at.clone()),
        finished_at: node.finished_at,
        artifact_count: artifacts.len(),
        attachment_count: attachments.len(),
        artifacts,
        attachments,
        has_progress_events: app
            .paths
            .dynamic_node_attempt_dir(
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
                node_id,
                &dynamic_attempt_id,
            )
            .join("progress.events.jsonl")
            .exists(),
        has_raw_stream: app
            .paths
            .dynamic_node_attempt_dir(
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
                node_id,
                &dynamic_attempt_id,
            )
            .join("raw.stream.jsonl")
            .exists(),
        has_worker_ref: app
            .paths
            .dynamic_node_worker_ref_file(
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
                node_id,
                &dynamic_attempt_id,
            )
            .exists(),
        manual_check_enabled: false,
        manual_check_pending: false,
        session_mode: Some(enum_label(&node.session_mode)),
        continue_from_node_id: node.continue_from_node_id.clone(),
        acp_session,
        acp_conversations: Vec::new(),
        selected_conversation_key: None,
        dynamic: None,
        dynamic_group_id: node.group_id.clone(),
    }))
}

fn acp_conversations_vm(
    app: &App,
    task_id: &str,
    run_id: &str,
    round: &RoundState,
    node_id: &str,
    nodes: &[NodeState],
) -> Result<Vec<AcpConversationVm>> {
    let mut attempts = nodes
        .iter()
        .filter(|node| node.node_id == node_id)
        .collect::<Vec<_>>();
    attempts.sort_by(|left, right| {
        trace_sequence_for_attempt(round, node_id, &left.attempt_id)
            .cmp(&trace_sequence_for_attempt(
                round,
                node_id,
                &right.attempt_id,
            ))
            .then_with(|| left.attempt_id.cmp(&right.attempt_id))
    });

    let mut conversations = Vec::<AcpConversationVm>::new();
    let mut session_conversation_keys = HashMap::<String, String>::new();
    for node in attempts {
        let sequence = trace_sequence_for_attempt(round, node_id, &node.attempt_id);
        let session_mode =
            worker_ref_session_mode(app, task_id, run_id, &round.id, node_id, &node.attempt_id);
        let worker_acp_session_id =
            worker_ref_acp_session_id(app, task_id, run_id, &round.id, node_id, &node.attempt_id);
        let acp_session = acp_session_vm(
            app,
            task_id,
            run_id,
            &round.id,
            node_id,
            &node.attempt_id,
            None,
            None,
        )?;
        let acp_session_id = worker_acp_session_id.or_else(|| {
            acp_session
                .as_ref()
                .and_then(|session| session.session_id.clone())
        });
        let attempt = AcpAttemptSessionVm {
            node_id: node_id.to_string(),
            attempt_id: node.attempt_id.clone(),
            sequence,
            status: enum_label(&node.status),
            outcome: node.outcome.map(|outcome| enum_label(&outcome)),
            current: false,
            session_mode: session_mode.clone(),
            acp_session_id: acp_session_id.clone(),
            acp_session,
        };
        let key = match (session_mode.as_deref(), acp_session_id.as_deref()) {
            (Some("continue"), Some(session_id)) => session_conversation_keys
                .get(session_id)
                .cloned()
                .unwrap_or_else(|| format!("session:{session_id}")),
            (Some("new"), _) => format!("attempt:{}", node.attempt_id),
            (_, Some(session_id)) => session_conversation_keys
                .get(session_id)
                .cloned()
                .unwrap_or_else(|| format!("session:{session_id}")),
            _ => format!("attempt:{}", node.attempt_id),
        };
        if let Some(session_id) = acp_session_id.as_deref() {
            session_conversation_keys.insert(session_id.to_string(), key.clone());
        }
        if let Some(conversation) = conversations.iter_mut().find(|item| item.key == key) {
            conversation.active_attempt_id = node.attempt_id.clone();
            if session_mode.as_deref() == Some("continue") {
                conversation.session_mode = "continue".to_string();
                conversation.label = conversation_label(
                    &key,
                    Some("continue"),
                    conversation.session_id.as_deref(),
                    &node.attempt_id,
                );
            }
            conversation.attempts.push(attempt);
        } else {
            conversations.push(AcpConversationVm {
                key: key.clone(),
                label: conversation_label(
                    &key,
                    session_mode.as_deref(),
                    acp_session_id.as_deref(),
                    &node.attempt_id,
                ),
                session_id: acp_session_id,
                session_mode: session_mode.unwrap_or_else(|| "unknown".to_string()),
                active_attempt_id: node.attempt_id.clone(),
                attempts: vec![attempt],
            });
        }
    }

    for conversation in &mut conversations {
        if let Some(active_attempt) = conversation.attempts.last() {
            conversation.active_attempt_id = active_attempt.attempt_id.clone();
        }
    }
    Ok(conversations)
}

fn conversation_label(
    key: &str,
    session_mode: Option<&str>,
    acp_session_id: Option<&str>,
    attempt_id: &str,
) -> String {
    match session_mode {
        Some("continue") => acp_session_id
            .map(|session_id| format!("continued session {session_id}"))
            .unwrap_or_else(|| format!("continued {attempt_id}")),
        Some("new") => format!("{attempt_id} · new session"),
        _ if key.starts_with("session:") => acp_session_id
            .map(|session_id| format!("session {session_id}"))
            .unwrap_or_else(|| attempt_id.to_string()),
        _ => attempt_id.to_string(),
    }
}

pub fn dynamic_acp_session_vm(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    node_id: &str,
    attempt_id: &str,
    query: Option<AcpSessionQueryInput>,
    preloaded_session_json: Option<serde_json::Value>,
) -> Result<Option<AcpSessionVm>> {
    let attempt_dir = app.paths.dynamic_node_attempt_dir(
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        node_id,
        attempt_id,
    );
    let snapshot_path = attempt_dir.join("acp.snapshot.json");
    let session_path = attempt_dir.join("acp.session.json");
    let timeline_path = attempt_dir.join("acp.timeline.jsonl");
    let events_path = attempt_dir.join("acp.events.jsonl");
    let raw_path = attempt_dir.join("acp.raw.jsonl");
    let diagnostics_path = attempt_dir.join("acp.diagnostics.jsonl");
    let has_preloaded = preloaded_session_json.is_some();
    if !has_preloaded
        && !snapshot_path.exists()
        && !session_path.exists()
        && !timeline_path.exists()
        && !events_path.exists()
        && !raw_path.exists()
        && !diagnostics_path.exists()
    {
        return Ok(None);
    }
    let session = if let Some(json) = preloaded_session_json {
        normalize_preloaded_session_metadata(json)
    } else if snapshot_path.exists() {
        load_session_metadata_value(&snapshot_path).unwrap_or_else(|| serde_json::json!({}))
    } else if session_path.exists() {
        load_session_metadata_value(&session_path).unwrap_or_else(|| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    let worker_ref_path = app.paths.dynamic_node_worker_ref_file(
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        node_id,
        attempt_id,
    );
    let worker_ref = if worker_ref_path.exists() {
        read_json::<WorkerRefState>(&worker_ref_path).ok()
    } else {
        None
    };
    let continue_ref = worker_ref
        .as_ref()
        .and_then(|state| state.continue_ref.as_ref());
    let diagnostics = AcpDiagnosticsScan {
        error_count: 0,
        last_error: None,
        last_error_timestamp: None,
    };
    let system_prompt_append = session
        .get("systemPromptAppend")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let config = acp_session_config_vm(&session);
    let metadata_status = session_metadata_status(&session);
    let root_status = effective_acp_session_status(
        metadata_status,
        gold_band::acp::client::prompt_activity(&attempt_dir),
    );
    let default_event_limit = app.config.acp_chat_event_page_size;
    let branch_id = query
        .as_ref()
        .and_then(|query| query.branch_id.clone())
        .unwrap_or_else(|| gold_band::acp::branches::ROOT_BRANCH_ID.to_string());
    gold_band::acp::branches::validate_conversation_branch_id(&branch_id)?;
    gold_band::acp::branches::prepare_agent_timeline_storage(&attempt_dir)?;
    let agent_index = if gold_band::acp::timeline::timeline_has_agent_launches(&timeline_path)? {
        gold_band::acp::branches::indexed_agent_index(&attempt_dir, &root_status)?
    } else {
        Vec::new()
    };
    let branch_record = conversation_branch_record(&agent_index, &branch_id);
    let status = conversation_branch_status(&root_status, &branch_id, branch_record);
    let active_status = is_acp_session_active_status(&status);
    let branch_timeline_path =
        gold_band::acp::branches::branch_timeline_path(&attempt_dir, &branch_id);
    let mut event_scan = scan_acp_timeline(
        &branch_timeline_path,
        query,
        active_status,
        default_event_limit,
    )?;
    restore_initial_task_attachments(
        app,
        task_id,
        worker_ref.as_ref().map(|worker| worker.mode),
        &branch_id,
        &mut event_scan.events,
    );
    apply_agent_index_projection(
        &mut event_scan.timeline_projection,
        &agent_index,
        &branch_id,
    );
    let parent_branch_id =
        branch_record.and_then(|record| record.parent_agent_execution_id.clone());
    let pending_interactions = event_scan
        .latest_permission_events
        .into_values()
        .filter(|event| event.status.as_deref() == Some("pending"))
        .map(|event| permission_vm_from_event(&event))
        .chain(event_scan.pending_elicitations.clone())
        .collect::<Vec<_>>();
    let provider = worker_ref
        .as_ref()
        .map(|state| state.provider.clone())
        .unwrap_or_else(|| gold_band::domain::DEFAULT_PROVIDER.to_string());
    let adapter_display_name = continue_ref
        .and_then(|value| value.get("adapterDisplayName"))
        .and_then(|value| value.as_str())
        .or_else(|| {
            session
                .get("adapterDisplayName")
                .and_then(|value| value.as_str())
        })
        .map(str::to_string)
        .or_else(|| {
            app.managed_agent(&provider)
                .ok()
                .map(|(_, agent)| agent.adapter.display_name.clone())
        });
    let adapter_icon_key = provider_icon_key(app, &provider);
    let session_elapsed_seconds = conversation_branch_elapsed_seconds(
        &branch_id,
        branch_record,
        event_scan.session_elapsed_seconds,
    );
    let session_timing = if branch_id == gold_band::acp::branches::ROOT_BRANCH_ID {
        resolve_acp_session_timing(
            &status,
            acp_session_timing_from_snapshot(&session),
            event_scan.session_timing.clone(),
            session_elapsed_seconds,
        )
    } else {
        legacy_session_timing(session_elapsed_seconds)
    };
    let provider_cwd = continue_ref
        .and_then(|value| value.get("cwd"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let cwd = session
        .get("cwd")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .or_else(|| Some(attempt_dir.to_string()));
    let run_worktree = run_worktree_state_optional(app, task_id, run_id)?;
    let dynamic_graph = dynamic_graph_state_optional(
        app,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
    );
    let session_worktree =
        session_worktree_projection(run_worktree.as_ref(), dynamic_graph.as_ref(), Some(node_id));
    let result = AcpSessionVm {
        branch_id: branch_id.clone(),
        parent_branch_id,
        read_only: branch_id != gold_band::acp::branches::ROOT_BRANCH_ID,
        branch_execution: branch_record.map(agent_execution_vm),
        session_id: continue_ref
            .and_then(|value| value.get("acpSessionId").or_else(|| value.get("sessionId")))
            .and_then(|value| value.as_str())
            .or_else(|| {
                session
                    .get("acpSessionId")
                    .or_else(|| session.get("sessionId"))
                    .and_then(|value| value.as_str())
            })
            .map(str::to_string),
        title: session
            .get("title")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        round_id: round_id.to_string(),
        node_id: node_id.to_string(),
        attempt_id: attempt_id.to_string(),
        outer_node_id: Some(outer_node_id.to_string()),
        outer_attempt_id: Some(outer_attempt_id.to_string()),
        provider,
        adapter_id: continue_ref
            .and_then(|value| value.get("adapterId"))
            .and_then(|value| value.as_str())
            .or_else(|| session.get("adapterId").and_then(|value| value.as_str()))
            .map(str::to_string),
        adapter_display_name,
        adapter_icon_key,
        worktree_path: session_worktree
            .as_ref()
            .map(|workspace| workspace.path.clone()),
        worktree_branch: session_worktree.and_then(|workspace| workspace.branch),
        cwd,
        provider_cwd,
        status,
        session_started_at: branch_record
            .map(|record| record.started_at.clone())
            .or_else(|| {
                session
                    .get("createdAt")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            }),
        session_updated_at: branch_record
            .map(|record| record.updated_at.clone())
            .or_else(|| {
                session
                    .get("updatedAt")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            }),
        session_elapsed_seconds,
        timing: session_timing,
        restored: session
            .get("restored")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        stop_reason: session
            .get("stopReason")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        system_prompt_append,
        config,
        events: event_scan.events,
        event_page: event_scan.event_page,
        timeline_projection: event_scan.timeline_projection,
        pending_interactions,
        available_commands: event_scan.available_commands,
        usage: if branch_id != gold_band::acp::branches::ROOT_BRANCH_ID {
            None
        } else {
            let mut u = event_scan.usage.unwrap_or_default();
            if u.used.is_none() {
                u.used = session
                    .get("usedTokens")
                    .and_then(|v| v.as_u64())
                    .filter(|used| *used > 0);
            }
            if u.size.is_none() {
                u.size = session.get("contextWindowSize").and_then(|v| v.as_u64());
            }
            if u.cost_amount_usd.is_none() {
                u.cost_amount_usd = session.get("totalCostUsd").and_then(|v| v.as_f64());
            }
            apply_persisted_attempt_token_totals(&mut u, &session);
            Some(u)
        },
        diagnostics: AcpDiagnosticsVm {
            raw_frame_count: 0,
            event_count: event_scan.event_count,
            error_count: diagnostics.error_count,
            last_error: diagnostics.last_error,
            last_error_timestamp: diagnostics.last_error_timestamp,
        },
    };
    Ok(Some(result))
}

pub fn dynamic_acp_session_status(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    node_id: &str,
    attempt_id: &str,
) -> Result<Option<String>> {
    let attempt_dir = app.paths.dynamic_node_attempt_dir(
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        node_id,
        attempt_id,
    );
    let Some(mut session) = session_metadata_from_attempt_dir(&attempt_dir) else {
        return Ok(None);
    };
    let node_path = app.paths.dynamic_node_file(
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        node_id,
    );
    apply_stale_session_completion_fuse_dynamic(&attempt_dir, &node_path, &mut session)?;
    Ok(Some(effective_acp_session_status(
        session_metadata_status(&session),
        gold_band::acp::client::prompt_activity(&attempt_dir),
    )))
}

pub fn acp_session_vm(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    query: Option<AcpSessionQueryInput>,
    preloaded_session_json: Option<serde_json::Value>,
) -> Result<Option<AcpSessionVm>> {
    let attempt_dir = app
        .paths
        .attempt_dir(task_id, run_id, round_id, node_id, attempt_id);
    let branch_id = query
        .as_ref()
        .and_then(|query| query.branch_id.clone())
        .unwrap_or_else(|| gold_band::acp::branches::ROOT_BRANCH_ID.to_string());
    let mut query_trace = AcpSessionQueryTrace::from_query(query.as_ref(), &branch_id);
    let snapshot_path = app
        .paths
        .acp_snapshot_file(task_id, run_id, round_id, node_id, attempt_id);
    let session_path = app
        .paths
        .acp_session_file(task_id, run_id, round_id, node_id, attempt_id);
    let timeline_path = app
        .paths
        .acp_timeline_file(task_id, run_id, round_id, node_id, attempt_id);
    let events_path = app
        .paths
        .acp_events_file(task_id, run_id, round_id, node_id, attempt_id);
    let raw_path = app
        .paths
        .acp_raw_file(task_id, run_id, round_id, node_id, attempt_id);
    let diagnostics_path = app
        .paths
        .acp_diagnostics_file(task_id, run_id, round_id, node_id, attempt_id);
    trace_acp_session_query(
        &mut query_trace,
        "view-model-start",
        serde_json::json!({
            "attemptDir": attempt_dir,
            "timelineBytes": timeline_path.metadata().ok().map(|metadata| metadata.len()),
            "rawBytes": raw_path.metadata().ok().map(|metadata| metadata.len()),
        }),
    );
    let has_preloaded = preloaded_session_json.is_some();
    if !has_preloaded
        && !snapshot_path.exists()
        && !session_path.exists()
        && !timeline_path.exists()
        && !events_path.exists()
        && !raw_path.exists()
        && !diagnostics_path.exists()
    {
        return Ok(None);
    }

    let session = if let Some(json) = preloaded_session_json {
        normalize_preloaded_session_metadata(json)
    } else if snapshot_path.exists() {
        load_session_metadata_value(&snapshot_path).unwrap_or_else(|| serde_json::json!({}))
    } else if session_path.exists() {
        load_session_metadata_value(&session_path).unwrap_or_else(|| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    trace_acp_session_query(
        &mut query_trace,
        "session-metadata",
        serde_json::json!({ "snapshotBytes": snapshot_path.metadata().ok().map(|metadata| metadata.len()) }),
    );
    let worker_ref_path = app
        .paths
        .worker_ref_file(task_id, run_id, round_id, node_id, attempt_id);
    let worker_ref = if worker_ref_path.exists() {
        read_json::<WorkerRefState>(&worker_ref_path).ok()
    } else {
        None
    };
    let node_provider = if worker_ref.is_none() {
        let node_path = app
            .paths
            .node_file(task_id, run_id, round_id, node_id, attempt_id);
        if node_path.exists() {
            read_json::<NodeState>(&node_path).ok().and_then(|node| {
                node.resolved_config
                    .get("provider")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            })
        } else {
            None
        }
    } else {
        None
    };
    trace_acp_session_query(
        &mut query_trace,
        "worker-and-node-metadata",
        serde_json::json!({ "hasWorkerRef": worker_ref.is_some(), "hasNodeProvider": node_provider.is_some() }),
    );
    let continue_ref = worker_ref
        .as_ref()
        .and_then(|state| state.continue_ref.as_ref());
    let diagnostics = AcpDiagnosticsScan {
        error_count: 0,
        last_error: None,
        last_error_timestamp: None,
    };
    trace_acp_session_query(
        &mut query_trace,
        "diagnostics",
        serde_json::json!({ "diagnosticBytes": diagnostics_path.metadata().ok().map(|metadata| metadata.len()) }),
    );
    let system_prompt_append = session
        .get("systemPromptAppend")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    trace_acp_session_query(
        &mut query_trace,
        "system-prompt",
        serde_json::json!({ "found": system_prompt_append.is_some() }),
    );
    trace_acp_session_query(
        &mut query_trace,
        "lifecycle-fuse",
        serde_json::json!({
            "availability": session.get("availability").and_then(Value::as_str),
            "latestTurnStatus": session.get("latestTurnStatus").and_then(Value::as_str),
        }),
    );
    let config = acp_session_config_vm(&session);
    let metadata_status = session_metadata_status(&session);
    let root_status = effective_acp_session_status(
        metadata_status,
        gold_band::acp::client::prompt_activity(&attempt_dir),
    );
    let default_event_limit = app.config.acp_chat_event_page_size;
    gold_band::acp::branches::validate_conversation_branch_id(&branch_id)?;
    gold_band::acp::branches::prepare_agent_timeline_storage(&attempt_dir)?;
    let agent_index = if gold_band::acp::timeline::timeline_has_agent_launches(&timeline_path)? {
        gold_band::acp::branches::indexed_agent_index(&attempt_dir, &root_status)?
    } else {
        Vec::new()
    };
    trace_acp_session_query(
        &mut query_trace,
        "agent-index",
        serde_json::json!({ "agentCount": agent_index.len() }),
    );
    let branch_record = conversation_branch_record(&agent_index, &branch_id);
    let status = conversation_branch_status(&root_status, &branch_id, branch_record);
    let active_status = is_acp_session_active_status(&status);
    let branch_timeline_path =
        gold_band::acp::branches::branch_timeline_path(&attempt_dir, &branch_id);
    let mut event_scan = scan_acp_timeline(
        &branch_timeline_path,
        query,
        active_status,
        default_event_limit,
    )?;
    restore_initial_task_attachments(
        app,
        task_id,
        worker_ref.as_ref().map(|worker| worker.mode),
        &branch_id,
        &mut event_scan.events,
    );
    trace_acp_session_query(
        &mut query_trace,
        "branch-timeline",
        serde_json::json!({
            "branchTimelineBytes": branch_timeline_path.metadata().ok().map(|metadata| metadata.len()),
            "eventCount": event_scan.event_count,
            "returnedEventCount": event_scan.events.len(),
            "semanticTotal": event_scan.event_page.total,
        }),
    );
    let session_id = continue_ref
        .and_then(|value| value.get("acpSessionId").or_else(|| value.get("sessionId")))
        .and_then(|value| value.as_str())
        .or_else(|| {
            session
                .get("acpSessionId")
                .or_else(|| session.get("sessionId"))
                .and_then(|value| value.as_str())
        })
        .map(str::to_string);
    let has_displayable_timeline_event = event_scan
        .events
        .iter()
        .any(|event| !is_hidden_from_chat(event) && is_session_timeline_event(event));
    if session.get("availability").and_then(Value::as_str) == Some("unavailable")
        && session_id.is_none()
        && !has_displayable_timeline_event
        && branch_record.is_none()
    {
        trace_acp_session_query(
            &mut query_trace,
            "session-not-materialized",
            serde_json::json!({ "availability": "unavailable" }),
        );
        return Ok(None);
    }
    apply_agent_index_projection(
        &mut event_scan.timeline_projection,
        &agent_index,
        &branch_id,
    );
    let parent_branch_id =
        branch_record.and_then(|record| record.parent_agent_execution_id.clone());
    let pending_interactions = event_scan
        .latest_permission_events
        .into_values()
        .filter(|event| event.status.as_deref() == Some("pending"))
        .map(|event| permission_vm_from_event(&event))
        .chain(event_scan.pending_elicitations.clone())
        .collect::<Vec<_>>();

    let provider = worker_ref
        .as_ref()
        .map(|state| state.provider.clone())
        .or(node_provider)
        .unwrap_or_else(|| gold_band::domain::DEFAULT_PROVIDER.to_string());
    let adapter_display_name = continue_ref
        .and_then(|value| value.get("adapterDisplayName"))
        .and_then(|value| value.as_str())
        .or_else(|| {
            session
                .get("adapterDisplayName")
                .and_then(|value| value.as_str())
        })
        .map(str::to_string)
        .or_else(|| {
            app.managed_agent(&provider)
                .ok()
                .map(|(_, agent)| agent.adapter.display_name.clone())
        });
    let adapter_icon_key = provider_icon_key(app, &provider);
    let session_elapsed_seconds = conversation_branch_elapsed_seconds(
        &branch_id,
        branch_record,
        event_scan.session_elapsed_seconds,
    );
    let session_timing = if branch_id == gold_band::acp::branches::ROOT_BRANCH_ID {
        resolve_acp_session_timing(
            &status,
            acp_session_timing_from_snapshot(&session),
            event_scan.session_timing.clone(),
            session_elapsed_seconds,
        )
    } else {
        legacy_session_timing(session_elapsed_seconds)
    };
    let provider_cwd = continue_ref
        .and_then(|value| value.get("cwd"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let cwd = session
        .get("cwd")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .or_else(|| snapshot_path.parent().map(|path| path.to_string()));
    let run_worktree = run_worktree_state_optional(app, task_id, run_id)?;
    let session_worktree = session_worktree_projection(run_worktree.as_ref(), None, None);

    let result = AcpSessionVm {
        branch_id: branch_id.clone(),
        parent_branch_id,
        read_only: branch_id != gold_band::acp::branches::ROOT_BRANCH_ID,
        branch_execution: branch_record.map(agent_execution_vm),
        session_id,
        title: session
            .get("title")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        round_id: round_id.to_string(),
        node_id: node_id.to_string(),
        attempt_id: attempt_id.to_string(),
        outer_node_id: None,
        outer_attempt_id: None,
        provider,
        adapter_id: continue_ref
            .and_then(|value| value.get("adapterId"))
            .and_then(|value| value.as_str())
            .or_else(|| session.get("adapterId").and_then(|value| value.as_str()))
            .map(str::to_string),
        adapter_display_name,
        adapter_icon_key,
        worktree_path: session_worktree
            .as_ref()
            .map(|workspace| workspace.path.clone()),
        worktree_branch: session_worktree.and_then(|workspace| workspace.branch),
        cwd,
        provider_cwd,
        status,
        session_started_at: branch_record
            .map(|record| record.started_at.clone())
            .or_else(|| {
                session
                    .get("createdAt")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            }),
        session_updated_at: branch_record
            .map(|record| record.updated_at.clone())
            .or_else(|| {
                session
                    .get("updatedAt")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            }),
        session_elapsed_seconds,
        timing: session_timing,
        restored: session
            .get("restored")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        stop_reason: session
            .get("stopReason")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        system_prompt_append,
        config,
        available_commands: event_scan.available_commands,
        usage: if branch_id != gold_band::acp::branches::ROOT_BRANCH_ID {
            None
        } else {
            let mut u = event_scan.usage.unwrap_or_default();
            // Merge persisted session usage as fallback for restored sessions
            // where events may not contain a usage_update yet.
            if u.used.is_none() {
                u.used = session
                    .get("usedTokens")
                    .and_then(|v| v.as_u64())
                    .filter(|used| *used > 0);
            }
            if u.size.is_none() {
                u.size = session.get("contextWindowSize").and_then(|v| v.as_u64());
            }
            if u.cost_amount_usd.is_none() {
                u.cost_amount_usd = session.get("totalCostUsd").and_then(|v| v.as_f64());
            }
            // Token breakdown shown in conversation UI is always the cumulative
            // ACP-attempt total, never the latest prompt response or timeline sample.
            apply_persisted_attempt_token_totals(&mut u, &session);
            Some(u)
        },
        diagnostics: AcpDiagnosticsVm {
            raw_frame_count: 0,
            event_count: event_scan.event_count,
            error_count: diagnostics.error_count,
            last_error: diagnostics.last_error,
            last_error_timestamp: diagnostics.last_error_timestamp,
        },
        events: event_scan.events,
        event_page: event_scan.event_page,
        timeline_projection: event_scan.timeline_projection,
        pending_interactions,
    };
    trace_acp_session_query(
        &mut query_trace,
        "view-model-complete",
        serde_json::json!({
            "returnedEventCount": result.events.len(),
            "projectedAgentCount": result.timeline_projection.agents.len(),
            "todoCount": result.timeline_projection.todo_entries.len(),
        }),
    );
    Ok(Some(result))
}

pub fn acp_session_status(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
) -> Result<Option<String>> {
    let attempt_dir = app
        .paths
        .attempt_dir(task_id, run_id, round_id, node_id, attempt_id);
    let Some(mut session) = session_metadata_from_attempt_dir(&attempt_dir) else {
        return Ok(None);
    };
    let node_path = app
        .paths
        .node_file(task_id, run_id, round_id, node_id, attempt_id);
    apply_stale_session_completion_fuse(
        app,
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        &node_path,
        &mut session,
    )?;
    Ok(Some(effective_acp_session_status(
        session_metadata_status(&session),
        gold_band::acp::client::prompt_activity(&attempt_dir),
    )))
}

fn session_metadata_from_attempt_dir(attempt_dir: &camino::Utf8Path) -> Option<serde_json::Value> {
    let snapshot_path = attempt_dir.join("acp.snapshot.json");
    let session_path = attempt_dir.join("acp.session.json");
    let has_acp_artifact = snapshot_path.exists()
        || session_path.exists()
        || [
            "acp.timeline.jsonl",
            "acp.events.jsonl",
            "acp.raw.jsonl",
            "acp.diagnostics.jsonl",
        ]
        .into_iter()
        .any(|file_name| attempt_dir.join(file_name).exists());
    if !has_acp_artifact {
        return None;
    }
    if snapshot_path.exists() {
        return Some(load_session_metadata_value(&snapshot_path).unwrap_or_default());
    }
    if session_path.exists() {
        return Some(load_session_metadata_value(&session_path).unwrap_or_default());
    }
    Some(serde_json::json!({}))
}

struct AcpEventScan {
    events: Vec<AcpUiEventVm>,
    event_page: AcpEventPageVm,
    timeline_projection: AcpTimelineProjectionVm,
    event_count: usize,
    session_elapsed_seconds: Option<u64>,
    session_timing: Option<AcpSessionTimingVm>,
    latest_permission_events: HashMap<String, AcpUiEventVm>,
    pending_elicitations: Vec<AcpPromptInteractionVm>,
    available_commands: Option<Vec<serde_json::Value>>,
    usage: Option<AcpUsageVm>,
}

struct AcpDiagnosticsScan {
    error_count: usize,
    last_error: Option<String>,
    last_error_timestamp: Option<String>,
}

fn restore_initial_task_attachments(
    app: &App,
    task_id: &str,
    session_mode: Option<SessionMode>,
    branch_id: &str,
    events: &mut [AcpUiEventVm],
) {
    if session_mode != Some(SessionMode::New)
        || branch_id != gold_band::acp::branches::ROOT_BRANCH_ID
    {
        return;
    }

    let input_dir = app.paths.task_dir(task_id).join("authoring").join("inputs");
    let mut input_paths = std::fs::read_dir(input_dir.as_std_path())
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
                .map(|entry| entry.path())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    input_paths.sort();
    let attachment_values = input_paths
        .iter()
        .filter_map(|path| attachment_meta_for_path(path, "task-inputs").ok().flatten())
        .filter_map(|attachment| serde_json::to_value(attachment).ok())
        .collect::<Vec<_>>();
    if attachment_values.is_empty() {
        return;
    }

    merge_initial_task_attachment_values(events, attachment_values);
}

fn merge_initial_task_attachment_values(
    events: &mut [AcpUiEventVm],
    attachment_values: Vec<Value>,
) {
    let Some(initial_prompt) = events.iter_mut().find(|event| {
        is_gold_band_user_prompt_event(event)
            && event
                .raw
                .as_ref()
                .is_none_or(|raw| raw.get("promptId").is_none())
    }) else {
        return;
    };
    let raw = initial_prompt
        .raw
        .get_or_insert_with(|| serde_json::json!({}));
    let Some(raw) = raw.as_object_mut() else {
        return;
    };
    let attachments = raw
        .entry("attachments")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    let Some(attachments) = attachments.as_array_mut() else {
        return;
    };
    let mut known_paths = attachments
        .iter()
        .filter_map(|attachment| attachment.get("path").and_then(Value::as_str))
        .map(str::to_string)
        .collect::<HashSet<_>>();
    for attachment in attachment_values {
        let Some(path) = attachment.get("path").and_then(Value::as_str) else {
            continue;
        };
        if known_paths.insert(path.to_string()) {
            attachments.push(attachment);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcpTimelineItemVm {
    item: AcpUiEventVm,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcpTimelinePatchVm {
    patch_type: String,
    item_id: String,
    revision: u64,
    op: String,
    item: AcpUiEventVm,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcpTimelineEntryHeaderVm {
    #[serde(default)]
    patch_type: Option<String>,
    #[serde(default)]
    item_id: Option<String>,
    #[serde(default)]
    revision: Option<u64>,
    #[serde(default)]
    op: Option<String>,
    item: AcpTimelineEventHeaderVm,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcpTimelineEventHeaderVm {
    id: String,
    seq: u64,
    kind: String,
    #[serde(default)]
    started_seq: Option<u64>,
    #[serde(default)]
    raw: Option<AcpTimelineEventRawHeaderVm>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcpTimelineEventRawHeaderVm {
    #[serde(default)]
    hidden_from_chat: bool,
}

#[derive(Debug, Clone)]
struct AcpActivityDetailCandidateVm {
    revision: u64,
    started_seq: u64,
}

fn scan_acp_timeline(
    path: &camino::Utf8Path,
    query: Option<AcpSessionQueryInput>,
    session_active: bool,
    default_event_limit: usize,
) -> Result<AcpEventScan> {
    const MIN_EVENT_LIMIT: usize = 1;
    const MAX_EVENT_LIMIT: usize = 1000;

    let query = query.unwrap_or(AcpSessionQueryInput {
        trace_id: None,
        branch_id: None,
        before_seq: None,
        after_seq: None,
        after_revision: None,
        before_cursor: None,
        after_cursor: None,
        event_limit: None,
        page_size: None,
    });
    let limit = query
        .page_size
        .or(query.event_limit)
        .unwrap_or(default_event_limit)
        .clamp(MIN_EVENT_LIMIT, MAX_EVENT_LIMIT);
    let before_seq = query
        .before_cursor
        .as_deref()
        .and_then(parse_timeline_cursor)
        .or(query.before_seq);
    let after_seq = query
        .after_cursor
        .as_deref()
        .and_then(parse_timeline_cursor)
        .or(query.after_seq);
    let indexed = gold_band::acp::timeline::read_indexed_timeline_page(
        path,
        before_seq,
        after_seq,
        query.after_revision,
        limit,
    )?;
    indexed_timeline_page_to_scan(path, indexed, session_active)
}

fn indexed_timeline_page_to_scan(
    timeline_path: &camino::Utf8Path,
    indexed: gold_band::acp::timeline::TimelineIndexedPage,
    session_active: bool,
) -> Result<AcpEventScan> {
    let mut events = indexed
        .events
        .into_iter()
        .map(|event| serde_json::from_value::<AcpUiEventVm>(serde_json::to_value(event)?))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for event in &mut events {
        normalize_turn_file_change_set_position(event);
        if event.kind != "permissionRequest" && event.kind != "activitySummary" {
            *event = compact_event_for_session(event.clone());
        }
    }
    let latest_permission_events = indexed
        .pending_permissions
        .into_iter()
        .map(|event| {
            let event = serde_json::from_value::<AcpUiEventVm>(serde_json::to_value(event)?)?;
            Ok((permission_request_id_from_event(&event), event))
        })
        .collect::<Result<HashMap<_, _>>>()?;
    include_latest_permission_events(&mut events, &latest_permission_events);
    order_provider_history_by_prompt_anchors_vm(&mut events);
    hydrate_timeline_events(timeline_path, &mut events)?;

    let pending_elicitations = indexed
        .pending_elicitations
        .into_iter()
        .map(|event| {
            serde_json::from_value::<AcpUiEventVm>(serde_json::to_value(event)?)
                .map(|event| elicitation_vm_from_event(&event))
                .map_err(Into::into)
        })
        .collect::<Result<Vec<_>>>()?;
    let projection_events = indexed
        .latest_plan
        .into_iter()
        .map(|event| serde_json::from_value::<AcpUiEventVm>(serde_json::to_value(event)?))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let timeline_projection = build_acp_timeline_projection(
        &projection_events,
        &latest_permission_events,
        session_active,
    );
    let session_timing = indexed
        .timing
        .map(|timing| serde_json::from_value::<AcpSessionTimingVm>(serde_json::to_value(timing)?))
        .transpose()?;
    let session_elapsed_seconds = session_timing
        .as_ref()
        .map(|timing| timing.session_elapsed_seconds);
    let usage = indexed.usage.map(|usage| AcpUsageVm {
        used: usage.used,
        size: usage.size,
        cost_amount_usd: usage.cost_amount_usd,
        ..AcpUsageVm::default()
    });
    Ok(AcpEventScan {
        events,
        event_page: AcpEventPageVm {
            generation: indexed.generation,
            covered_revision: indexed.covered_revision,
            newest_revision: indexed.newest_revision,
            loaded_count: indexed.loaded_semantic_blocks,
            total: indexed.total_semantic_blocks,
            oldest_seq: indexed.oldest_seq,
            newest_seq: indexed.newest_seq,
            has_older: indexed.has_older,
            has_newer: indexed.has_newer,
            oldest_cursor: indexed.oldest_seq.map(format_timeline_cursor),
            newest_cursor: indexed.newest_seq.map(format_timeline_cursor),
        },
        timeline_projection,
        event_count: indexed.event_count,
        session_elapsed_seconds,
        session_timing,
        latest_permission_events,
        pending_elicitations,
        available_commands: indexed.available_commands,
        usage,
    })
}

#[cfg(test)]
fn merge_confirmed_usage_observation(
    usage: &mut Option<AcpUsageVm>,
    used: Option<u64>,
    size: Option<u64>,
    cost_amount_usd: Option<f64>,
) {
    let current = usage.get_or_insert_with(AcpUsageVm::default);
    if let Some(used) = used.filter(|used| *used > 0) {
        current.used = Some(used);
    }
    if let Some(size) = size.filter(|size| *size > 0) {
        current.size = Some(size);
    }
    if let Some(cost_amount_usd) = cost_amount_usd {
        current.cost_amount_usd = Some(cost_amount_usd);
    }
}

fn apply_persisted_attempt_token_totals(usage: &mut AcpUsageVm, session: &serde_json::Value) {
    usage.input_tokens = session
        .get("attemptInputTokens")
        .and_then(|value| value.as_u64());
    usage.output_tokens = session
        .get("attemptOutputTokens")
        .and_then(|value| value.as_u64());
    usage.cached_read_tokens = session
        .get("attemptCachedReadTokens")
        .and_then(|value| value.as_u64());
    usage.cached_write_tokens = session
        .get("attemptCachedWriteTokens")
        .and_then(|value| value.as_u64());
    usage.total_tokens = session
        .get("attemptTotalTokens")
        .and_then(|value| value.as_u64());
}

#[cfg(test)]
fn apply_recovered_attempt_token_totals(
    usage: &mut AcpUsageVm,
    recovery: &gold_band::acp::usage::AcpAttemptUsageRecovery,
) {
    if recovery.completed_turns == 0 {
        return;
    }
    usage.input_tokens = recovery.totals.input_tokens;
    usage.output_tokens = recovery.totals.output_tokens;
    usage.cached_read_tokens = recovery.totals.cached_read_tokens;
    usage.cached_write_tokens = recovery.totals.cached_write_tokens;
    usage.total_tokens = recovery.totals.total_tokens;
}

#[cfg(test)]
fn parse_timeline_file(
    path: &camino::Utf8Path,
    session_active: bool,
) -> Result<(
    Vec<AcpUiEventVm>,
    usize,
    Option<u64>,
    HashMap<String, AcpUiEventVm>,
    Option<Vec<serde_json::Value>>,
    Option<AcpUsageVm>,
)> {
    let mut latest_by_item = HashMap::<String, (u64, AcpUiEventVm)>::new();
    let mut available_commands = None;
    let mut usage = None;
    let mut event_count = 0usize;

    if path.exists() {
        let file = fs::File::open(path.as_std_path())?;
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(patch) = serde_json::from_str::<AcpTimelinePatchVm>(&line) {
                if patch.patch_type != "timelinePatch" || patch.op != "upsert" {
                    continue;
                }
                event_count += 1;
                if let Some(raw) = patch.item.raw.as_ref() {
                    if is_session_update(&patch.item, "available_commands_update") {
                        available_commands = raw
                            .get("availableCommands")
                            .and_then(|value| value.as_array())
                            .cloned();
                    } else if is_session_update(&patch.item, "usage_update") {
                        let (used, size, cost_amount) =
                            gold_band::acp::events::extract_usage_fields(raw);
                        merge_confirmed_usage_observation(&mut usage, used, size, cost_amount);
                    }
                }
                let should_replace = latest_by_item
                    .get(&patch.item_id)
                    .map(|(revision, _)| patch.revision >= *revision)
                    .unwrap_or(true);
                if should_replace {
                    let item = latest_by_item
                        .get(&patch.item_id)
                        .map(|(_, existing)| {
                            merge_timeline_item_revision_vm(existing, patch.item.clone())
                        })
                        .unwrap_or(patch.item);
                    latest_by_item.insert(patch.item_id, (patch.revision, item));
                }
                continue;
            }

            let Ok(final_item) = serde_json::from_str::<AcpTimelineItemVm>(&line) else {
                continue;
            };
            event_count += 1;
            if let Some(raw) = final_item.item.raw.as_ref() {
                if is_session_update(&final_item.item, "available_commands_update") {
                    available_commands = raw
                        .get("availableCommands")
                        .and_then(|value| value.as_array())
                        .cloned();
                } else if is_session_update(&final_item.item, "usage_update") {
                    let (used, size, cost_amount) =
                        gold_band::acp::events::extract_usage_fields(raw);
                    merge_confirmed_usage_observation(&mut usage, used, size, cost_amount);
                }
            }
            let item_id = final_item.item.id.clone();
            let should_replace = latest_by_item
                .get(&item_id)
                .map(|(revision, _)| *revision == 0)
                .unwrap_or(true);
            if should_replace {
                let item = latest_by_item
                    .get(&item_id)
                    .map(|(_, existing)| {
                        merge_timeline_item_revision_vm(existing, final_item.item.clone())
                    })
                    .unwrap_or(final_item.item);
                latest_by_item.insert(item_id, (0, item));
            }
        }
    }

    let mut canonical_events = latest_by_item
        .into_values()
        .map(|(_, event)| event)
        .collect::<Vec<_>>();
    for event in &mut canonical_events {
        normalize_turn_file_change_set_position(event);
    }
    canonical_events.sort_by_key(|event| (event.started_seq.unwrap_or(event.seq), event.seq));
    remove_reclassified_local_provider_history_vm(&mut canonical_events);

    let mut session_elapsed = AcpSessionElapsedState::default();
    let mut latest_permission_events = HashMap::<String, AcpUiEventVm>::new();
    for event in &canonical_events {
        session_elapsed.observe_event(event);
        if event.kind == "permissionRequest" {
            insert_latest_permission_event(&mut latest_permission_events, event);
        }
    }

    let all_events = canonical_events
        .into_iter()
        .filter(|event| !is_hidden_from_chat(event) && is_session_timeline_event(event))
        .collect::<Vec<_>>();

    Ok((
        all_events,
        event_count,
        session_elapsed.finish(session_active),
        latest_permission_events,
        available_commands,
        usage,
    ))
}

fn normalize_turn_file_change_set_position(event: &mut AcpUiEventVm) {
    if event.kind != "fileChangeSet" {
        return;
    }
    event.started_seq = Some(event.seq);
    event.started_at = event
        .ended_at
        .clone()
        .or_else(|| Some(event.timestamp.clone()));
}

fn merge_timeline_item_revision_vm(
    existing: &AcpUiEventVm,
    mut incoming: AcpUiEventVm,
) -> AcpUiEventVm {
    if is_provider_history_event_vm(&incoming) && !is_provider_history_event_vm(existing) {
        return existing.clone();
    }
    let existing_start = existing.started_seq.unwrap_or(existing.seq);
    let incoming_start = incoming.started_seq.unwrap_or(incoming.seq);
    if existing_start > incoming_start {
        return incoming;
    }
    if matches!(incoming.kind.as_str(), "toolCall" | "toolCallUpdate") {
        if incoming.title.is_none() {
            incoming.title.clone_from(&existing.title);
        }
        if let Some(existing_raw) = existing.raw.as_ref() {
            let incoming_raw = incoming.raw.get_or_insert_with(|| serde_json::json!({}));
            gold_band::acp::events::merge_tool_revision_raw(incoming_raw, existing_raw);
        }
    }

    let repeated_payload = existing.kind == incoming.kind
        && existing.content == incoming.content
        && existing.title == incoming.title
        && existing.tool_call_id == incoming.tool_call_id
        && existing.status == incoming.status
        && raw_equal_ignoring_history_placement_vm(existing.raw.as_ref(), incoming.raw.as_ref());
    incoming.started_seq = Some(existing_start);
    incoming.started_at = existing
        .started_at
        .clone()
        .or_else(|| Some(existing.timestamp.clone()));
    incoming.timestamp = existing.timestamp.clone();
    if repeated_payload {
        incoming.seq = existing.seq;
        incoming.ended_seq = existing.ended_seq;
        incoming.ended_at = existing.ended_at.clone();
        incoming.timing = existing.timing.clone();
    }
    incoming
}

fn raw_equal_ignoring_history_placement_vm(
    existing: Option<&Value>,
    incoming: Option<&Value>,
) -> bool {
    fn without_placement(raw: Option<&Value>) -> Option<Value> {
        raw.map(|raw| {
            let mut raw = raw.clone();
            if let Some(object) = raw.as_object_mut() {
                object.remove("historyPlacement");
            }
            raw
        })
    }

    without_placement(existing) == without_placement(incoming)
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ProviderHistoryTurnKeyVm {
    session_id: Option<String>,
    provider: String,
    turn_index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ProviderHistoryPlacementKeyVm {
    session_id: Option<String>,
    provider: String,
    after_prompt_id: Option<String>,
    before_prompt_id: Option<String>,
    gap_turn_index: u64,
}

#[derive(Debug)]
struct PlacedProviderHistoryGroupVm {
    slot: usize,
    after_anchor_index: Option<usize>,
    gap_turn_index: u64,
    audit_seq: u64,
    stable_key: String,
    items: Vec<AcpUiEventVm>,
}

#[cfg(test)]
fn remove_reclassified_local_provider_history_vm(items: &mut Vec<AcpUiEventVm>) {
    let mut local_prompts = HashMap::<Option<String>, Vec<String>>::new();
    for item in items
        .iter()
        .filter(|item| is_gold_band_user_prompt_event(item))
    {
        let Some(content) = item.content.as_deref() else {
            continue;
        };
        local_prompts
            .entry(item.session_id.clone())
            .or_default()
            .push(normalize_provider_history_prompt(content));
    }

    let mut cursors = HashMap::<(Option<String>, String), usize>::new();
    let mut stale_turns = HashSet::<ProviderHistoryTurnKeyVm>::new();
    for item in items.iter().filter(|item| {
        item.kind == "userTextDelta"
            && is_provider_history_event_vm(item)
            && !has_provider_history_placement_vm(item.raw.as_ref())
    }) {
        let Some(turn) = provider_history_turn_key_vm(item) else {
            continue;
        };
        let Some(content) = item.content.as_deref() else {
            continue;
        };
        let Some(anchors) = local_prompts.get(&turn.session_id) else {
            continue;
        };
        let cursor_key = (turn.session_id.clone(), turn.provider.clone());
        let cursor = cursors.entry(cursor_key).or_default();
        let normalized = normalize_provider_history_prompt(content);
        let Some(relative_index) = anchors[*cursor..]
            .iter()
            .position(|anchor| anchor == &normalized)
        else {
            continue;
        };
        *cursor = cursor.saturating_add(relative_index).saturating_add(1);
        stale_turns.insert(turn);
    }

    if stale_turns.is_empty() {
        return;
    }
    items.retain(|item| {
        provider_history_turn_key_vm(item).is_none_or(|turn| !stale_turns.contains(&turn))
    });
}

#[cfg(test)]
fn has_provider_history_placement_vm(raw: Option<&Value>) -> bool {
    raw.and_then(|raw| raw.get("historyPlacement"))
        .and_then(Value::as_object)
        .and_then(|placement| placement.get("version"))
        .and_then(Value::as_u64)
        == Some(1)
}

fn is_provider_history_event_vm(event: &AcpUiEventVm) -> bool {
    event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("source"))
        .and_then(|value| value.as_str())
        == Some("providerHistory")
}

#[cfg(test)]
fn provider_history_turn_key_vm(event: &AcpUiEventVm) -> Option<ProviderHistoryTurnKeyVm> {
    let raw = event.raw.as_ref()?;
    if raw.get("source").and_then(|value| value.as_str()) != Some("providerHistory") {
        return None;
    }
    Some(ProviderHistoryTurnKeyVm {
        session_id: event.session_id.clone(),
        provider: raw
            .get("historyProvider")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        turn_index: raw
            .get("historyTurnIndex")
            .and_then(|value| value.as_u64())?,
    })
}

#[cfg(test)]
fn normalize_provider_history_prompt(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
}

fn order_provider_history_by_prompt_anchors_vm(items: &mut Vec<AcpUiEventVm>) {
    let mut base = Vec::with_capacity(items.len());
    let mut grouped = HashMap::<ProviderHistoryPlacementKeyVm, Vec<AcpUiEventVm>>::new();
    for item in items.drain(..) {
        let Some(key) = provider_history_placement_key_vm(&item) else {
            base.push(item);
            continue;
        };
        grouped.entry(key).or_default().push(item);
    }
    if grouped.is_empty() {
        *items = base;
        return;
    }

    let prompt_indexes = base
        .iter()
        .enumerate()
        .filter(|(_, event)| is_gold_band_user_prompt_event(event))
        .map(|(index, event)| {
            (
                (event.session_id.clone(), prompt_anchor_id_vm(event)),
                index,
            )
        })
        .collect::<HashMap<_, _>>();
    let mut groups = grouped
        .into_iter()
        .map(|(key, mut history_items)| {
            history_items.sort_by_key(|event| {
                (
                    provider_history_item_index_vm(event),
                    event.started_seq.unwrap_or(event.seq),
                    event.seq,
                )
            });
            let audit_seq = history_items
                .iter()
                .map(|event| event.started_seq.unwrap_or(event.seq))
                .min()
                .unwrap_or_default();
            let before_anchor_index = key
                .before_prompt_id
                .as_ref()
                .and_then(|prompt_id| {
                    prompt_indexes.get(&(key.session_id.clone(), prompt_id.clone()))
                })
                .copied();
            let after_anchor_index = key
                .after_prompt_id
                .as_ref()
                .and_then(|prompt_id| {
                    prompt_indexes.get(&(key.session_id.clone(), prompt_id.clone()))
                })
                .copied();
            let slot = before_anchor_index
                .or_else(|| {
                    after_anchor_index.map(|after_index| {
                        base.iter()
                            .enumerate()
                            .skip(after_index.saturating_add(1))
                            .find(|(_, event)| is_gold_band_user_prompt_event(event))
                            .map(|(index, _)| index)
                            .unwrap_or(base.len())
                    })
                })
                .unwrap_or_else(|| {
                    base.iter()
                        .take_while(|event| event.started_seq.unwrap_or(event.seq) <= audit_seq)
                        .count()
                });
            let stable_key = format!(
                "{}:{}:{}:{}:{}",
                key.session_id.as_deref().unwrap_or_default(),
                key.provider,
                key.after_prompt_id.as_deref().unwrap_or_default(),
                key.before_prompt_id.as_deref().unwrap_or_default(),
                key.gap_turn_index,
            );
            PlacedProviderHistoryGroupVm {
                slot,
                after_anchor_index,
                gap_turn_index: key.gap_turn_index,
                audit_seq,
                stable_key,
                items: history_items,
            }
        })
        .collect::<Vec<_>>();
    groups.sort_by(|left, right| {
        (
            left.slot,
            left.after_anchor_index,
            left.gap_turn_index,
            left.audit_seq,
            left.stable_key.as_str(),
        )
            .cmp(&(
                right.slot,
                right.after_anchor_index,
                right.gap_turn_index,
                right.audit_seq,
                right.stable_key.as_str(),
            ))
    });

    let mut slots = (0..=base.len())
        .map(|_| Vec::<AcpUiEventVm>::new())
        .collect::<Vec<_>>();
    for group in groups {
        slots[group.slot].extend(group.items);
    }
    let mut ordered = Vec::with_capacity(base.len() + slots.iter().map(Vec::len).sum::<usize>());
    for (index, event) in base.into_iter().enumerate() {
        ordered.append(&mut slots[index]);
        ordered.push(event);
    }
    ordered.append(&mut slots.last_mut().expect("history slots are never empty"));
    *items = ordered;
}

fn provider_history_placement_key_vm(
    event: &AcpUiEventVm,
) -> Option<ProviderHistoryPlacementKeyVm> {
    let raw = event.raw.as_ref()?;
    if raw.get("source").and_then(Value::as_str) != Some("providerHistory") {
        return None;
    }
    let placement = raw.get("historyPlacement")?.as_object()?;
    if placement.get("version").and_then(Value::as_u64) != Some(1) {
        return None;
    }
    Some(ProviderHistoryPlacementKeyVm {
        session_id: event.session_id.clone(),
        provider: raw
            .get("historyProvider")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        after_prompt_id: placement
            .get("afterPromptId")
            .and_then(Value::as_str)
            .map(str::to_string),
        before_prompt_id: placement
            .get("beforePromptId")
            .and_then(Value::as_str)
            .map(str::to_string),
        gap_turn_index: placement
            .get("gapTurnIndex")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
    })
}

fn prompt_anchor_id_vm(event: &AcpUiEventVm) -> String {
    event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("promptId"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(event.id.as_str())
        .to_string()
}

fn provider_history_item_index_vm(event: &AcpUiEventVm) -> u64 {
    event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("historyItemIndex"))
        .and_then(Value::as_u64)
        .unwrap_or_default()
}

#[cfg(test)]
fn paginate_timeline(
    timeline_path: &camino::Utf8Path,
    all_events: &[AcpUiEventVm],
    event_count: usize,
    session_elapsed_seconds: Option<u64>,
    latest_permission_events: &HashMap<String, AcpUiEventVm>,
    available_commands: Option<&Vec<serde_json::Value>>,
    usage: Option<&AcpUsageVm>,
    session_active: bool,
    after_seq: Option<u64>,
    before_seq: Option<u64>,
    limit: usize,
) -> Result<AcpEventScan> {
    let semantic_blocks = conversation_semantic_blocks(all_events);
    let total = semantic_blocks.len();
    let session_timing = latest_session_timing_from_events(all_events);
    let pending_elicitations = pending_elicitation_vms(all_events);
    let timeline_projection =
        build_acp_timeline_projection(all_events, latest_permission_events, session_active);
    let (selected_blocks, after_cursor_has_newer) = if let Some(cursor) = after_seq {
        let mut changed_blocks = semantic_blocks
            .iter()
            .filter(|block| block.newest_seq > cursor)
            .collect::<Vec<_>>();
        changed_blocks
            .sort_by_key(|block| (block.newest_seq, block.oldest_seq, block.start, block.end));
        let selected_count = if changed_blocks.len() > limit {
            let cutoff_revision = changed_blocks[limit - 1].newest_seq;
            changed_blocks.partition_point(|block| block.newest_seq <= cutoff_revision)
        } else {
            changed_blocks.len()
        };
        let has_newer = selected_count < changed_blocks.len();
        let mut selected = changed_blocks[..selected_count]
            .iter()
            .map(|block| (*block).clone())
            .collect::<Vec<_>>();
        selected.sort_by_key(|block| (block.start, block.end));
        (selected, Some(has_newer))
    } else if let Some(cursor) = before_seq {
        let mut page = semantic_blocks
            .iter()
            .filter(|block| block.newest_seq < cursor)
            .cloned()
            .collect::<Vec<_>>();
        if page.len() > limit {
            page = page.split_off(page.len() - limit);
        }
        (page, None)
    } else if total > limit {
        (semantic_blocks[total - limit..].to_vec(), None)
    } else {
        (semantic_blocks.clone(), None)
    };
    let loaded_semantic_count = selected_blocks.len();
    let filtered = compact_selected_semantic_blocks(all_events, &selected_blocks);
    // Compact only the events in the final window (not all events)
    let mut filtered: Vec<_> = filtered
        .into_iter()
        .map(|event| {
            if matches!(event.kind.as_str(), "permissionRequest") {
                event // keep permission events as-is for pending check
            } else {
                compact_event_for_session(event)
            }
        })
        .collect();
    // Pagination cursors describe the requested event window. Structural anchors and
    // pending-interaction snapshots are appended afterwards and must not move cursors.
    let oldest_seq = filtered
        .iter()
        .map(|event| event.started_seq.unwrap_or(event.seq))
        .min();
    let newest_seq = filtered
        .iter()
        .map(|event| event.ended_seq.unwrap_or(event.seq))
        .max();
    let oldest_index = selected_blocks.first().and_then(|selected| {
        semantic_blocks
            .iter()
            .position(|block| block.oldest_seq == selected.oldest_seq)
    });
    let newest_index = selected_blocks.last().and_then(|selected| {
        semantic_blocks
            .iter()
            .rposition(|block| block.newest_seq == selected.newest_seq)
    });
    let event_page = AcpEventPageVm {
        generation: 0,
        covered_revision: newest_seq.unwrap_or_default(),
        newest_revision: newest_seq,
        loaded_count: loaded_semantic_count,
        total,
        oldest_seq,
        newest_seq,
        has_older: oldest_index.is_some_and(|index| index > 0),
        has_newer: after_cursor_has_newer
            .unwrap_or_else(|| newest_index.is_some_and(|index| index + 1 < total)),
        oldest_cursor: oldest_seq.map(format_timeline_cursor),
        newest_cursor: newest_seq.map(format_timeline_cursor),
    };
    include_latest_permission_events(&mut filtered, latest_permission_events);
    order_provider_history_by_prompt_anchors_vm(&mut filtered);
    hydrate_timeline_events(timeline_path, &mut filtered)?;

    Ok(AcpEventScan {
        events: filtered,
        event_page,
        timeline_projection,
        event_count,
        session_elapsed_seconds,
        session_timing,
        latest_permission_events: latest_permission_events.clone(),
        pending_elicitations,
        available_commands: available_commands.cloned(),
        usage: usage.cloned(),
    })
}

fn hydrate_timeline_events(
    timeline_path: &camino::Utf8Path,
    events: &mut [AcpUiEventVm],
) -> Result<()> {
    for event in events {
        if let Some(raw) = event.raw.as_mut() {
            gold_band::acp::timeline::hydrate_timeline_value(timeline_path, raw)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[derive(Debug, Clone)]
struct ConversationSemanticBlockRange {
    start: usize,
    end: usize,
    oldest_seq: u64,
    newest_seq: u64,
}

#[cfg(test)]
fn conversation_semantic_blocks(events: &[AcpUiEventVm]) -> Vec<ConversationSemanticBlockRange> {
    let mut blocks = Vec::<ConversationSemanticBlockRange>::new();
    let mut activity_start: Option<usize> = None;
    let resolved_elicitation_ids = events
        .iter()
        .filter(|event| event.kind == "elicitationResponse")
        .filter_map(elicitation_id_from_event)
        .collect::<HashSet<_>>();
    let flush_activity =
        |end: usize,
         start: &mut Option<usize>,
         blocks: &mut Vec<ConversationSemanticBlockRange>| {
            if let Some(start_index) = start.take() {
                blocks.push(semantic_block_range(events, start_index, end));
            }
        };
    for (index, event) in events.iter().enumerate() {
        if !is_conversation_semantic_event(event, &resolved_elicitation_ids) {
            continue;
        }
        if is_conversation_activity_event(event) {
            activity_start.get_or_insert(index);
            continue;
        }
        if index > 0 {
            flush_activity(index - 1, &mut activity_start, &mut blocks);
        }
        blocks.push(semantic_block_range(events, index, index));
    }
    if !events.is_empty() {
        flush_activity(events.len() - 1, &mut activity_start, &mut blocks);
    }
    blocks
}

const ACTIVITY_DETAIL_INITIAL_LIMIT: usize = 40;

#[cfg(test)]
fn compact_selected_semantic_blocks(
    events: &[AcpUiEventVm],
    blocks: &[ConversationSemanticBlockRange],
) -> Vec<AcpUiEventVm> {
    let mut selected = Vec::new();
    for block in blocks {
        let block_events = &events[block.start..=block.end];
        let is_activity = block_events.iter().any(is_conversation_activity_event);
        if !is_activity {
            selected.extend(block_events.iter().cloned());
            continue;
        }
        let audit_events = block_events
            .iter()
            .filter(|event| is_conversation_activity_event(event))
            .collect::<Vec<_>>();
        if let Some(summary) = activity_summary_event(block, &audit_events) {
            selected.push(summary);
        }
    }
    selected
}

#[cfg(test)]
fn activity_summary_event(
    block: &ConversationSemanticBlockRange,
    audit_events: &[&AcpUiEventVm],
) -> Option<AcpUiEventVm> {
    let latest = audit_events.last()?;
    let mut tool_ids = HashSet::<String>::new();
    let mut thought_count = 0usize;
    let mut error_count = 0usize;
    let mut read_files = std::collections::BTreeSet::<String>::new();
    let mut written_files = std::collections::BTreeSet::<String>::new();
    for event in audit_events {
        match event.kind.as_str() {
            "thoughtDelta" => thought_count += 1,
            "error" => error_count += 1,
            "toolCall" | "toolCallUpdate" => {
                tool_ids.insert(
                    event
                        .tool_call_id
                        .clone()
                        .unwrap_or_else(|| event.id.clone()),
                );
                if !event.status.as_deref().is_some_and(|status| {
                    matches!(
                        status.to_ascii_lowercase().as_str(),
                        "completed" | "success" | "succeeded"
                    )
                }) {
                    continue;
                }
                let tool_name = agent_event_meta_vm(event)
                    .tool_name
                    .or_else(|| {
                        event
                            .title
                            .as_deref()
                            .and_then(|title| title.split_whitespace().next())
                            .map(str::to_string)
                    })
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let paths = structured_tool_paths(event);
                if matches!(tool_name.as_str(), "read" | "get-content" | "read_file") {
                    read_files.extend(paths);
                } else if matches!(
                    tool_name.as_str(),
                    "write" | "edit" | "applypatch" | "apply_patch" | "set-content" | "write_file"
                ) {
                    written_files.extend(paths);
                }
            }
            _ => {}
        }
    }
    Some(AcpUiEventVm {
        id: format!("activity-{}", block.oldest_seq),
        seq: block.oldest_seq,
        timestamp: audit_events
            .first()
            .map(|event| event.timestamp.clone())
            .unwrap_or_else(|| latest.timestamp.clone()),
        kind: "activitySummary".to_string(),
        session_id: latest.session_id.clone(),
        content: None,
        title: latest.title.clone(),
        tool_call_id: None,
        status: latest.status.clone(),
        started_seq: Some(block.oldest_seq),
        ended_seq: Some(block.newest_seq),
        started_at: audit_events
            .first()
            .and_then(|event| event.started_at.clone())
            .or_else(|| audit_events.first().map(|event| event.timestamp.clone())),
        ended_at: latest
            .ended_at
            .clone()
            .or_else(|| Some(latest.timestamp.clone())),
        timing: latest.timing.clone(),
        raw: Some(serde_json::json!({
            "goldBandActivity": {
                "activityStartSeq": block.oldest_seq,
                "activityEndSeq": block.newest_seq,
                "totalEventCount": audit_events.len(),
                "toolCallCount": tool_ids.len(),
                "thoughtCount": thought_count,
                "errorCount": error_count,
                "readFileCount": read_files.len(),
                "writtenFileCount": written_files.len(),
                "detailAvailable": !audit_events.is_empty(),
            }
        })),
    })
}

pub fn acp_activity_detail_vm_for_attempt(
    attempt_dir: &camino::Utf8Path,
    query: AcpActivityDetailQueryInput,
) -> Result<AcpActivityDetailVm> {
    gold_band::acp::branches::validate_conversation_branch_id(&query.branch_id)?;
    gold_band::acp::branches::migrate_legacy_agent_timeline(attempt_dir)?;
    let timeline_path =
        gold_band::acp::branches::branch_timeline_path(attempt_dir, &query.branch_id);
    if query.activity_end_seq < query.activity_start_seq || !timeline_path.exists() {
        return Ok(AcpActivityDetailVm {
            items: Vec::new(),
            has_more_earlier: false,
            earlier_cursor: None,
        });
    }
    let before_seq = query
        .earlier_cursor
        .as_deref()
        .and_then(parse_timeline_cursor);
    let limit = query
        .limit
        .unwrap_or(ACTIVITY_DETAIL_INITIAL_LIMIT)
        .clamp(1, 200);
    let candidates = scan_activity_detail_candidates(
        &timeline_path,
        query.activity_start_seq,
        query.activity_end_seq,
        before_seq,
        limit.saturating_add(1),
    )?;
    let has_more_earlier = candidates.len() > limit;
    let selected_ids = candidates
        .iter()
        .skip(usize::from(has_more_earlier))
        .map(|(item_id, _)| item_id.clone())
        .collect::<HashSet<_>>();
    let audit = load_selected_activity_detail_events(&timeline_path, &selected_ids)?;
    let mut audit = audit;
    hydrate_timeline_events(&timeline_path, &mut audit)?;
    let items = audit
        .into_iter()
        .map(compact_event_for_activity_audit)
        .collect::<Vec<_>>();
    let earlier_cursor = items
        .first()
        .map(|event| format_timeline_cursor(event.started_seq.unwrap_or(event.seq)));
    Ok(AcpActivityDetailVm {
        items,
        has_more_earlier,
        earlier_cursor,
    })
}

fn scan_activity_detail_candidates(
    timeline_path: &camino::Utf8Path,
    activity_start_seq: u64,
    activity_end_seq: u64,
    before_seq: Option<u64>,
    capacity: usize,
) -> Result<Vec<(String, AcpActivityDetailCandidateVm)>> {
    let file = fs::File::open(timeline_path.as_std_path())?;
    let mut candidates = HashMap::<String, AcpActivityDetailCandidateVm>::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        let Ok(header) = serde_json::from_str::<AcpTimelineEntryHeaderVm>(&line) else {
            continue;
        };
        let is_patch = header.patch_type.as_deref() == Some("timelinePatch");
        if is_patch && header.op.as_deref() != Some("upsert") {
            continue;
        }
        let started_seq = header.item.started_seq.unwrap_or(header.item.seq);
        if started_seq < activity_start_seq
            || started_seq > activity_end_seq
            || before_seq.is_some_and(|cursor| started_seq >= cursor)
            || header
                .item
                .raw
                .as_ref()
                .is_some_and(|raw| raw.hidden_from_chat)
            || !matches!(
                header.item.kind.as_str(),
                "thoughtDelta" | "toolCall" | "toolCallUpdate" | "error"
            )
        {
            continue;
        }
        let item_id = header.item_id.unwrap_or(header.item.id);
        let revision = if is_patch {
            header.revision.unwrap_or_default()
        } else {
            0
        };
        match candidates.get_mut(&item_id) {
            Some(current) if is_patch && revision >= current.revision => {
                current.revision = revision;
                current.started_seq = current.started_seq.min(started_seq);
            }
            Some(current) if !is_patch && current.revision == 0 => {
                current.started_seq = current.started_seq.min(started_seq);
            }
            Some(_) => {}
            None => {
                candidates.insert(
                    item_id,
                    AcpActivityDetailCandidateVm {
                        revision,
                        started_seq,
                    },
                );
            }
        }
        if candidates.len() > capacity {
            let oldest = candidates
                .iter()
                .min_by_key(|(item_id, candidate)| (candidate.started_seq, item_id.as_str()))
                .map(|(item_id, _)| item_id.clone());
            if let Some(oldest) = oldest {
                candidates.remove(&oldest);
            }
        }
    }
    let mut candidates = candidates.into_iter().collect::<Vec<_>>();
    candidates.sort_by(|(left_id, left), (right_id, right)| {
        (left.started_seq, left_id.as_str()).cmp(&(right.started_seq, right_id.as_str()))
    });
    Ok(candidates)
}

fn load_selected_activity_detail_events(
    timeline_path: &camino::Utf8Path,
    selected_ids: &HashSet<String>,
) -> Result<Vec<AcpUiEventVm>> {
    if selected_ids.is_empty() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(timeline_path.as_std_path())?;
    let mut latest_by_item = HashMap::<String, (u64, AcpUiEventVm)>::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        let Ok(header) = serde_json::from_str::<AcpTimelineEntryHeaderVm>(&line) else {
            continue;
        };
        let item_id = header.item_id.unwrap_or(header.item.id);
        if !selected_ids.contains(&item_id) {
            continue;
        }
        if header.patch_type.as_deref() == Some("timelinePatch") {
            let Ok(patch) = serde_json::from_str::<AcpTimelinePatchVm>(&line) else {
                continue;
            };
            if patch.op != "upsert" {
                continue;
            }
            let should_replace = latest_by_item
                .get(&patch.item_id)
                .map(|(revision, _)| patch.revision >= *revision)
                .unwrap_or(true);
            if should_replace {
                let item = latest_by_item
                    .get(&patch.item_id)
                    .map(|(_, existing)| {
                        merge_timeline_item_revision_vm(existing, patch.item.clone())
                    })
                    .unwrap_or(patch.item);
                latest_by_item.insert(patch.item_id, (patch.revision, item));
            }
            continue;
        }
        let Ok(final_item) = serde_json::from_str::<AcpTimelineItemVm>(&line) else {
            continue;
        };
        let item_id = final_item.item.id.clone();
        let should_replace = latest_by_item
            .get(&item_id)
            .map(|(revision, _)| *revision == 0)
            .unwrap_or(true);
        if should_replace {
            let item = latest_by_item
                .get(&item_id)
                .map(|(_, existing)| {
                    merge_timeline_item_revision_vm(existing, final_item.item.clone())
                })
                .unwrap_or(final_item.item);
            latest_by_item.insert(item_id, (0, item));
        }
    }
    let mut events = latest_by_item
        .into_values()
        .map(|(_, event)| event)
        .filter(|event| !is_hidden_from_chat(event) && is_conversation_activity_event(event))
        .collect::<Vec<_>>();
    events.sort_by_key(|event| (event.started_seq.unwrap_or(event.seq), event.seq));
    Ok(events)
}

pub fn acp_tool_detail_vm_for_attempt(
    attempt_dir: &camino::Utf8Path,
    query: AcpToolDetailQueryInput,
) -> Result<AcpToolDetailVm> {
    gold_band::acp::branches::validate_conversation_branch_id(&query.branch_id)?;
    gold_band::acp::branches::migrate_legacy_agent_timeline(attempt_dir)?;
    let timeline_path =
        gold_band::acp::branches::branch_timeline_path(attempt_dir, &query.branch_id);
    if !timeline_path.exists() {
        return Ok(AcpToolDetailVm { event: None });
    }
    let needles = [Some(query.event_id.as_str()), query.tool_call_id.as_deref()]
        .into_iter()
        .flatten()
        .filter(|value| !value.is_empty())
        .map(|value| serde_json::to_string(value).unwrap_or_else(|_| format!("\"{value}\"")))
        .collect::<Vec<_>>();
    let file = fs::File::open(timeline_path.as_std_path())?;
    let mut detail: Option<AcpUiEventVm> = None;
    for line in BufReader::new(file).lines() {
        let line = line?;
        if !needles.iter().any(|needle| line.contains(needle)) {
            continue;
        }
        let candidate = if let Ok(patch) = serde_json::from_str::<AcpTimelinePatchVm>(&line) {
            (patch.patch_type == "timelinePatch" && patch.op == "upsert").then_some(patch.item)
        } else {
            serde_json::from_str::<AcpTimelineItemVm>(&line)
                .ok()
                .map(|item| item.item)
        };
        let Some(candidate) = candidate else { continue };
        let identity_matches = candidate.id == query.event_id
            || query.tool_call_id.as_deref().is_some_and(|tool_call_id| {
                candidate.tool_call_id.as_deref() == Some(tool_call_id)
            });
        if !identity_matches || !matches!(candidate.kind.as_str(), "toolCall" | "toolCallUpdate") {
            continue;
        }
        detail = Some(
            detail
                .as_ref()
                .map(|current| merge_timeline_item_revision_vm(current, candidate.clone()))
                .unwrap_or(candidate),
        );
    }
    if let Some(event) = detail.as_mut() {
        hydrate_timeline_events(&timeline_path, std::slice::from_mut(event))?;
    }
    Ok(AcpToolDetailVm { event: detail })
}

#[cfg(test)]
fn semantic_block_range(
    events: &[AcpUiEventVm],
    start: usize,
    end: usize,
) -> ConversationSemanticBlockRange {
    let selected = &events[start..=end];
    ConversationSemanticBlockRange {
        start,
        end,
        oldest_seq: selected
            .iter()
            .map(|event| event.started_seq.unwrap_or(event.seq))
            .min()
            .unwrap_or_default(),
        newest_seq: selected
            .iter()
            .map(|event| event.ended_seq.unwrap_or(event.seq))
            .max()
            .unwrap_or_default(),
    }
}

#[cfg(test)]
fn is_conversation_semantic_event(
    event: &AcpUiEventVm,
    resolved_elicitation_ids: &HashSet<String>,
) -> bool {
    if event.kind == "permissionRequest" {
        return event.status.as_deref() == Some("pending");
    }
    if event.kind == "elicitationRequest" {
        return event.status.as_deref().unwrap_or("pending") == "pending"
            && elicitation_id_from_event(event)
                .is_none_or(|id| !resolved_elicitation_ids.contains(&id));
    }
    matches!(
        event.kind.as_str(),
        "userTextDelta"
            | "textDelta"
            | "thoughtDelta"
            | "toolCall"
            | "toolCallUpdate"
            | "fileChangeSet"
            | "attemptSeparator"
            | "contextCompaction"
            | "error"
    )
}

#[cfg(test)]
fn elicitation_id_from_event(event: &AcpUiEventVm) -> Option<String> {
    event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("elicitationId"))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            let id = event.id.strip_suffix("-response").unwrap_or(&event.id);
            (!id.is_empty()).then(|| id.to_string())
        })
}

fn is_conversation_activity_event(event: &AcpUiEventVm) -> bool {
    if agent_event_meta_vm(event).agent_launch {
        return false;
    }
    matches!(
        event.kind.as_str(),
        "thoughtDelta" | "toolCall" | "toolCallUpdate" | "error"
    )
}

#[derive(Debug, Clone, Default)]
struct AgentEventMetaVm {
    agent_launch: bool,
    #[cfg(test)]
    tool_name: Option<String>,
}

fn agent_event_meta_vm(event: &AcpUiEventVm) -> AgentEventMetaVm {
    let Some(conversation) = event
        .raw
        .as_ref()
        .and_then(|raw| raw.pointer("/_meta/goldBandConversation"))
    else {
        return AgentEventMetaVm::default();
    };
    AgentEventMetaVm {
        agent_launch: conversation
            .get("launchedAgentExecutionId")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| !value.is_empty()),
        #[cfg(test)]
        tool_name: conversation
            .get("toolName")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    }
}

fn conversation_branch_record<'a>(
    records: &'a [gold_band::acp::branches::AgentExecutionRecord],
    branch_id: &str,
) -> Option<&'a gold_band::acp::branches::AgentExecutionRecord> {
    (branch_id != gold_band::acp::branches::ROOT_BRANCH_ID)
        .then(|| {
            records
                .iter()
                .find(|record| record.agent_execution_id == branch_id)
        })
        .flatten()
}

fn conversation_branch_status(
    root_status: &str,
    branch_id: &str,
    branch_record: Option<&gold_band::acp::branches::AgentExecutionRecord>,
) -> String {
    if branch_id == gold_band::acp::branches::ROOT_BRANCH_ID {
        return root_status.to_string();
    }
    branch_record
        .map(|record| record.status.clone())
        .unwrap_or_else(|| "unknown".to_string())
}

fn conversation_branch_elapsed_seconds(
    branch_id: &str,
    branch_record: Option<&gold_band::acp::branches::AgentExecutionRecord>,
    root_elapsed_seconds: Option<u64>,
) -> Option<u64> {
    if branch_id == gold_band::acp::branches::ROOT_BRANCH_ID {
        return root_elapsed_seconds;
    }
    let record = branch_record?;
    let started_at = parse_epoch_timestamp(&record.started_at)?;
    let updated_at = parse_epoch_timestamp(&record.updated_at)?;
    Some(updated_at.saturating_sub(started_at))
}

fn apply_agent_index_projection(
    projection: &mut AcpTimelineProjectionVm,
    records: &[gold_band::acp::branches::AgentExecutionRecord],
    branch_id: &str,
) {
    projection.agents = records
        .iter()
        .filter(|record| {
            if branch_id == gold_band::acp::branches::ROOT_BRANCH_ID {
                record.parent_agent_execution_id.is_none()
            } else {
                record.parent_agent_execution_id.as_deref() == Some(branch_id)
            }
        })
        .map(agent_execution_vm)
        .collect();
    if branch_id == gold_band::acp::branches::ROOT_BRANCH_ID
        && !records.is_empty()
        && projection.todo_ownership
            == Some(gold_band::acp::branches::ConversationPlanOwnership::Unscoped)
    {
        projection.todo_entries.clear();
    }
}

fn agent_execution_vm(
    record: &gold_band::acp::branches::AgentExecutionRecord,
) -> AcpAgentExecutionVm {
    AcpAgentExecutionVm {
        agent_execution_id: record.agent_execution_id.clone(),
        parent_agent_execution_id: record.parent_agent_execution_id.clone(),
        execution_status: record.status.clone(),
        event_count: record.event_count,
        tool_call_count: record.tool_call_count,
        read_file_count: record.read_file_count,
        written_file_count: record.written_file_count,
        has_attention: record.has_attention,
        title: record.title.clone(),
        description: record.description.clone(),
        todo_entries: record.todo_entries.clone(),
    }
}

fn build_acp_timeline_projection(
    all_events: &[AcpUiEventVm],
    _latest_permission_events: &HashMap<String, AcpUiEventVm>,
    _session_active: bool,
) -> AcpTimelineProjectionVm {
    // Plan ownership is fail-closed. Some ACP providers emit a session-wide
    // aggregate plan without a branch relation; that plan remains unscoped and
    // must not be guessed into the root or an Agent from its natural-language
    // content. `apply_agent_index_projection` suppresses such root plans when
    // the session contains Agent executions.
    let latest_plan = all_events
        .iter()
        .filter(|event| event.kind == "plan")
        .max_by_key(|event| event.ended_seq.unwrap_or(event.seq));
    let todo_entries = latest_plan
        .and_then(|event| event.raw.as_ref())
        .and_then(|raw| raw.get("entries").or_else(|| raw.pointer("/plan/entries")))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let todo_ownership = latest_plan.map(|event| {
        let branch_id = event
            .raw
            .as_ref()
            .and_then(|raw| raw.pointer("/_meta/goldBandConversation/branchId"))
            .and_then(Value::as_str)
            .unwrap_or(gold_band::acp::branches::ROOT_BRANCH_ID);
        gold_band::acp::branches::conversation_plan_ownership(event.raw.as_ref(), branch_id)
    });
    AcpTimelineProjectionVm {
        agents: Vec::new(),
        todo_entries,
        todo_ownership,
    }
}

#[cfg(test)]
fn tool_raw_input(event: &AcpUiEventVm) -> Option<&serde_json::Map<String, serde_json::Value>> {
    let raw = event.raw.as_ref()?.as_object()?;
    let container = raw
        .get("toolCall")
        .and_then(serde_json::Value::as_object)
        .or_else(|| raw.get("content").and_then(serde_json::Value::as_object))
        .unwrap_or(raw);
    container
        .get("rawInput")
        .and_then(serde_json::Value::as_object)
        .or_else(|| raw.get("rawInput").and_then(serde_json::Value::as_object))
}

#[cfg(test)]
fn structured_tool_paths(event: &AcpUiEventVm) -> Vec<String> {
    let mut paths = Vec::<String>::new();
    if let Some(input) = tool_raw_input(event) {
        for key in ["file_path", "path"] {
            if let Some(path) = input.get(key).and_then(serde_json::Value::as_str) {
                paths.push(normalize_metric_path(path));
            }
        }
    }
    let raw = event.raw.as_ref();
    let locations = raw
        .and_then(|raw| raw.pointer("/toolCall/locations"))
        .or_else(|| raw.and_then(|raw| raw.get("locations")))
        .and_then(serde_json::Value::as_array);
    if let Some(locations) = locations {
        for location in locations {
            if let Some(path) = location.get("path").and_then(serde_json::Value::as_str) {
                paths.push(normalize_metric_path(path));
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

#[cfg(test)]
fn normalize_metric_path(path: &str) -> String {
    path.trim()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

#[cfg(test)]
fn latest_session_timing_from_events(all_events: &[AcpUiEventVm]) -> Option<AcpSessionTimingVm> {
    all_events.iter().rev().find_map(|event| {
        event.timing.as_ref().map(|timing| AcpSessionTimingVm {
            session_elapsed_seconds: timing.session_elapsed_seconds,
            revision: timing.revision,
            observed_at: timing.observed_at.clone(),
            active_turn_started_at: timing.active_turn_started_at.clone(),
            active_turn_last_activity_at: timing.active_turn_last_activity_at.clone(),
            permission_wait_started_at: timing.permission_wait_started_at.clone(),
            user_wait_started_at: timing.user_wait_started_at.clone(),
            wait_reason: timing.wait_reason.clone(),
            paused: timing.paused,
        })
    })
}

fn acp_session_timing_from_snapshot(session: &serde_json::Value) -> Option<AcpSessionTimingVm> {
    session
        .get("timing")
        .cloned()
        .and_then(|value| serde_json::from_value::<AcpSessionTimingVm>(value).ok())
}

fn resolve_acp_session_timing(
    status: &str,
    snapshot_timing: Option<AcpSessionTimingVm>,
    event_timing: Option<AcpSessionTimingVm>,
    session_elapsed_seconds: Option<u64>,
) -> Option<AcpSessionTimingVm> {
    if is_acp_session_active_status(status) {
        return active_session_timing(event_timing, session_elapsed_seconds).or(snapshot_timing);
    }
    snapshot_timing
        .or(event_timing)
        .or_else(|| legacy_session_timing(session_elapsed_seconds))
}

fn active_session_timing(
    event_timing: Option<AcpSessionTimingVm>,
    session_elapsed_seconds: Option<u64>,
) -> Option<AcpSessionTimingVm> {
    let Some(seconds) = session_elapsed_seconds else {
        return event_timing;
    };
    if let Some(mut timing) = event_timing {
        timing.session_elapsed_seconds = seconds;
        return Some(timing);
    }
    Some(AcpSessionTimingVm {
        session_elapsed_seconds: seconds,
        revision: None,
        observed_at: None,
        active_turn_started_at: None,
        active_turn_last_activity_at: None,
        permission_wait_started_at: None,
        user_wait_started_at: None,
        wait_reason: None,
        paused: false,
    })
}

fn legacy_session_timing(session_elapsed_seconds: Option<u64>) -> Option<AcpSessionTimingVm> {
    session_elapsed_seconds.map(|seconds| AcpSessionTimingVm {
        session_elapsed_seconds: seconds,
        revision: None,
        observed_at: None,
        active_turn_started_at: None,
        active_turn_last_activity_at: None,
        permission_wait_started_at: None,
        user_wait_started_at: None,
        wait_reason: None,
        paused: true,
    })
}

fn include_latest_permission_events(
    events: &mut Vec<AcpUiEventVm>,
    latest_permission_events: &HashMap<String, AcpUiEventVm>,
) {
    if latest_permission_events.is_empty() {
        return;
    }

    let mut changed = false;
    for (request_id, latest) in latest_permission_events {
        if latest.status.as_deref() != Some("pending") {
            continue;
        }
        if let Some(existing) = events.iter_mut().find(|event| {
            event.kind == "permissionRequest"
                && permission_request_id_from_event(event) == *request_id
        }) {
            if latest.seq >= existing.seq {
                *existing = latest.clone();
                changed = true;
            }
            continue;
        }
        events.push(latest.clone());
        changed = true;
    }
    if changed {
        events.sort_by_key(|event| event.started_seq.unwrap_or(event.seq));
    }
}

fn apply_stale_session_completion_fuse(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    node_path: &camino::Utf8Path,
    session: &mut serde_json::Value,
) -> Result<()> {
    let pid_path = app
        .paths
        .provider_pid_file(task_id, run_id, round_id, node_id, attempt_id);
    let raw_path = app
        .paths
        .acp_raw_file(task_id, run_id, round_id, node_id, attempt_id);
    let node_status = if node_path.exists() {
        read_json::<NodeState>(node_path)
            .ok()
            .map(|node| node.status)
    } else {
        None
    };
    let fused = apply_stale_session_completion_fuse_common(
        &pid_path,
        &raw_path,
        session,
        node_status
            .map(|status| status == RunStatus::Completed)
            .unwrap_or(false),
        gold_band::acp::client::prompt_activity(
            &app.paths
                .attempt_dir(task_id, run_id, round_id, node_id, attempt_id),
        )
        .is_some(),
    )?;
    // View models are projections, not lifecycle owners. Keep the fused value
    // in this response only; canonical reconciliation belongs to the runtime
    // writer and must not race a provider terminal commit from a read path.
    let _ = fused;
    Ok(())
}

fn apply_stale_session_completion_fuse_dynamic(
    attempt_dir: &camino::Utf8Path,
    node_path: &camino::Utf8Path,
    session: &mut serde_json::Value,
) -> Result<()> {
    let pid_path = attempt_dir.join("provider.pid");
    let raw_path = attempt_dir.join("acp.raw.jsonl");
    let node_completed = if node_path.exists() {
        read_json::<gold_band::dynamic::DynamicNodeState>(node_path)
            .ok()
            .map(|node| node.status == gold_band::dynamic::DynamicNodeStatus::Completed)
            .unwrap_or(false)
    } else {
        false
    };
    let fused = apply_stale_session_completion_fuse_common(
        &pid_path,
        &raw_path,
        session,
        node_completed,
        gold_band::acp::client::prompt_activity(attempt_dir).is_some(),
    )?;
    let _ = fused;
    Ok(())
}

fn apply_stale_session_completion_fuse_common(
    pid_path: &camino::Utf8Path,
    _raw_path: &camino::Utf8Path,
    session: &mut serde_json::Value,
    node_completed: bool,
    prompt_active: bool,
) -> Result<bool> {
    if prompt_active {
        return Ok(false);
    }
    if pid_path.exists() && !node_completed {
        return Ok(false);
    }
    if !node_completed {
        return Ok(false);
    }
    if !matches!(session_metadata_status(session), "idle" | "unknown") {
        return Ok(false);
    }
    session["latestTurnStatus"] = serde_json::json!("completed");
    if session.get("stopReason").is_none() || session["stopReason"].is_null() {
        session["stopReason"] = serde_json::json!("end_turn");
    }
    session["updatedAt"] = serde_json::json!(current_epoch_timestamp());
    Ok(true)
}

fn parse_epoch_timestamp(value: &str) -> Option<u64> {
    value.trim_end_matches('Z').parse::<u64>().ok()
}

#[cfg(test)]
#[derive(Default)]
struct AcpSessionElapsedState {
    elapsed_seconds: u64,
    active_turn_started_at: Option<u64>,
    active_turn_last_event_at: Option<u64>,
    saw_turn: bool,
    pending_permission_ids: HashSet<String>,
    pending_elicitation_ids: HashSet<String>,
    user_wait_started_at: Option<u64>,
    user_wait_seconds: u64,
}

#[cfg(test)]
impl AcpSessionElapsedState {
    fn observe_event(&mut self, event: &AcpUiEventVm) {
        if is_gold_band_user_prompt_event(event) {
            self.elapsed_seconds = self
                .elapsed_seconds
                .saturating_add(self.finish_current_turn(false, None));
            self.active_turn_started_at = parse_epoch_timestamp(&event.timestamp);
            self.active_turn_last_event_at = None;
            self.pending_permission_ids.clear();
            self.pending_elicitation_ids.clear();
            self.user_wait_started_at = None;
            self.user_wait_seconds = 0;
            self.saw_turn = true;
            return;
        }
        if self.active_turn_started_at.is_none() {
            return;
        }
        let Some(timestamp) = parse_epoch_timestamp(&event.timestamp) else {
            return;
        };
        self.observe_permission_event(event, timestamp);
        self.observe_elicitation_event(event, timestamp);
        if is_session_elapsed_progress_event(event) {
            self.active_turn_last_event_at = Some(timestamp);
        }
    }

    fn finish(&self, session_active: bool) -> Option<u64> {
        self.finish_at(session_active, None)
    }

    fn finish_at(&self, session_active: bool, now: Option<u64>) -> Option<u64> {
        self.saw_turn.then_some(
            self.elapsed_seconds
                .saturating_add(self.finish_current_turn(session_active, now)),
        )
    }

    fn finish_current_turn(&self, session_active: bool, now: Option<u64>) -> u64 {
        let Some(started_at) = self.active_turn_started_at else {
            return 0;
        };
        let end_at = if session_active {
            now.unwrap_or_else(current_epoch_seconds)
        } else {
            self.active_turn_last_event_at.unwrap_or(started_at)
        };
        let base_elapsed = end_at.saturating_sub(started_at);
        base_elapsed.saturating_sub(
            self.user_wait_seconds
                .saturating_add(self.open_user_wait(end_at)),
        )
    }

    fn open_user_wait(&self, end_at: u64) -> u64 {
        self.user_wait_started_at
            .map(|started_at| end_at.saturating_sub(started_at))
            .unwrap_or_default()
    }

    fn observe_permission_event(&mut self, event: &AcpUiEventVm, timestamp: u64) {
        if event.kind != "permissionRequest" {
            return;
        }
        let is_pending = event
            .status
            .as_deref()
            .is_some_and(|status| status.eq_ignore_ascii_case("pending"));
        if is_pending {
            let request_id = permission_request_id_from_event(event);
            let was_waiting = self.is_waiting_for_user();
            if self.pending_permission_ids.insert(request_id) && !was_waiting {
                self.user_wait_started_at = Some(timestamp);
            }
            return;
        }
        let request_id = permission_request_id_from_event(event);
        if !self.pending_permission_ids.remove(&request_id) {
            if let Some(started_at) = compacted_wait_started_at(event, timestamp) {
                self.add_closed_user_wait(started_at, timestamp);
            }
            return;
        }
        self.close_user_wait_if_idle(timestamp);
    }

    fn observe_elicitation_event(&mut self, event: &AcpUiEventVm, timestamp: u64) {
        match event.kind.as_str() {
            "elicitationRequest"
                if event
                    .status
                    .as_deref()
                    .is_some_and(|status| status.eq_ignore_ascii_case("pending")) =>
            {
                let was_waiting = self.is_waiting_for_user();
                if self.pending_elicitation_ids.insert(event.id.clone()) && !was_waiting {
                    self.user_wait_started_at = Some(timestamp);
                }
            }
            "elicitationResponse" => {
                let elicitation_id = event
                    .raw
                    .as_ref()
                    .and_then(|raw| raw.get("elicitationId"))
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| event.id.trim_end_matches("-response").to_string());
                if self.pending_elicitation_ids.remove(&elicitation_id) {
                    self.close_user_wait_if_idle(timestamp);
                } else if let Some(started_at) = compacted_wait_started_at(event, timestamp) {
                    self.add_closed_user_wait(started_at, timestamp);
                }
            }
            "elicitationRequest" => {
                if let Some(started_at) = compacted_wait_started_at(event, timestamp) {
                    self.add_closed_user_wait(started_at, timestamp);
                }
            }
            _ => {}
        }
    }

    fn is_waiting_for_user(&self) -> bool {
        !self.pending_permission_ids.is_empty() || !self.pending_elicitation_ids.is_empty()
    }

    fn close_user_wait_if_idle(&mut self, timestamp: u64) {
        if self.is_waiting_for_user() {
            return;
        }
        if let Some(started_at) = self.user_wait_started_at.take() {
            self.user_wait_seconds = self
                .user_wait_seconds
                .saturating_add(timestamp.saturating_sub(started_at));
        }
    }

    fn add_closed_user_wait(&mut self, started_at: u64, ended_at: u64) {
        if ended_at <= started_at {
            return;
        }
        let effective_end = self
            .user_wait_started_at
            .map(|open_started_at| ended_at.min(open_started_at))
            .unwrap_or(ended_at);
        if effective_end <= started_at {
            return;
        }
        self.user_wait_seconds = self
            .user_wait_seconds
            .saturating_add(effective_end.saturating_sub(started_at));
    }
}

#[cfg(test)]
fn compacted_wait_started_at(event: &AcpUiEventVm, ended_at: u64) -> Option<u64> {
    let started_at = event
        .started_at
        .as_deref()
        .and_then(parse_epoch_timestamp)?;
    (started_at < ended_at).then_some(started_at)
}

fn is_gold_band_user_prompt_event(event: &AcpUiEventVm) -> bool {
    event.kind == "userTextDelta"
        && event
            .raw
            .as_ref()
            .and_then(|raw| raw.get("source"))
            .and_then(|value| value.as_str())
            == Some("goldBandPrompt")
}

#[cfg(test)]
fn is_session_elapsed_progress_event(event: &AcpUiEventVm) -> bool {
    let session_update = event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("sessionUpdate"))
        .and_then(|value| value.as_str());
    !matches!(
        session_update,
        Some("available_commands_update" | "current_mode_update" | "session_info_update")
    )
}

fn current_epoch_timestamp() -> String {
    format!("{}Z", current_epoch_seconds())
}

fn format_timeline_cursor(seq: u64) -> String {
    format!("rev:{seq}")
}

fn parse_timeline_cursor(value: &str) -> Option<u64> {
    value
        .strip_prefix("rev:")
        .and_then(|value| value.parse::<u64>().ok())
}

fn current_epoch_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn is_hidden_from_chat(event: &AcpUiEventVm) -> bool {
    event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("hiddenFromChat"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn is_session_timeline_event(event: &AcpUiEventVm) -> bool {
    if event.kind == "permissionRequest" {
        return event.status.as_deref() == Some("pending");
    }
    if matches!(
        event.kind.as_str(),
        "availableCommands"
            | "usageUpdate"
            | "sessionInfo"
            | "modeUpdate"
            | "configUpdate"
            | "rawDiagnostic"
    ) {
        return false;
    }
    let Some(raw) = event.raw.as_ref() else {
        return true;
    };
    let session_update = raw.get("sessionUpdate").and_then(|value| value.as_str());
    if session_update == Some("user_message_chunk")
        && raw.get("source").and_then(|value| value.as_str()) == Some("providerHistory")
    {
        return true;
    }
    !matches!(
        session_update,
        Some(
            "user_message_chunk"
                | "available_commands_update"
                | "usage_update"
                | "session_info_update"
                | "current_mode_update"
                | "config_option_update"
        )
    )
}

#[cfg(test)]
fn scan_acp_diagnostics(path: &camino::Utf8Path) -> Result<AcpDiagnosticsScan> {
    let mut error_count = 0usize;
    let mut last_error = None;
    let mut last_error_timestamp = None;
    if !path.exists() {
        return Ok(AcpDiagnosticsScan {
            error_count,
            last_error,
            last_error_timestamp,
        });
    }
    let file = fs::File::open(path.as_std_path())?;
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if value.get("level").and_then(|item| item.as_str()) == Some("error") {
            error_count += 1;
            if let Some(message) = value.get("message").and_then(|item| item.as_str()) {
                last_error = Some(message.to_string());
                last_error_timestamp = value
                    .get("timestamp")
                    .and_then(|item| item.as_str())
                    .map(str::to_string);
            }
        }
    }
    Ok(AcpDiagnosticsScan {
        error_count,
        last_error,
        last_error_timestamp,
    })
}

/// Extract system prompt append from the beginning of the raw ACP frame file.
/// Only reads the first 500 lines — system prompt is carried by the first session lifecycle frame.
#[cfg(test)]
fn extract_system_prompt_append(path: &camino::Utf8Path) -> Option<String> {
    if !path.exists() {
        return None;
    }
    let file = fs::File::open(path.as_std_path()).ok()?;
    for line in std::io::BufReader::new(file).lines().take(500) {
        let line = line.ok()?;
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line).ok()?;
        if value.get("direction").and_then(|v| v.as_str()) != Some("outbound") {
            continue;
        }
        let method = value.pointer("/frame/method").and_then(|v| v.as_str());
        if !matches!(
            method,
            Some("session/new" | "session/load" | "session/resume")
        ) {
            continue;
        }
        return value
            .pointer("/frame/params/_meta/systemPrompt/append")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
    }
    None
}

fn compact_event_for_session(mut event: AcpUiEventVm) -> AcpUiEventVm {
    if let Some(raw) = event.raw.as_mut() {
        remove_provider_agent_metadata(raw);
    }
    event.raw = event.raw.map(compact_raw_value);
    event.content = event
        .content
        .map(|content| truncate_string(content, 64_000));
    event.title = event.title.map(|title| truncate_string(title, 2_000));
    event
}

fn compact_event_for_activity_audit(event: AcpUiEventVm) -> AcpUiEventVm {
    let mut event = compact_event_for_session(event);
    if !matches!(event.kind.as_str(), "toolCall" | "toolCallUpdate") {
        return event;
    }
    let Some(raw) = event.raw.as_mut() else {
        return event;
    };
    for path in [
        &["output"][..],
        &["fields", "output"][..],
        &["content", "output"][..],
        &["toolCall", "output"][..],
        &["toolCall", "content"][..],
        &["toolCall", "fields", "output"][..],
        &["_meta", "goldBandConversation", "toolOutput"][..],
    ] {
        remove_nested_json_key(raw, path);
    }
    if raw
        .get("content")
        .is_some_and(|content| !content.is_object())
        && let Some(object) = raw.as_object_mut()
    {
        object.remove("content");
    }
    if !raw.is_object() {
        *raw = serde_json::json!({});
    }
    let raw_object = raw.as_object_mut().expect("activity raw must be an object");
    let meta = raw_object
        .entry("_meta")
        .or_insert_with(|| serde_json::json!({}));
    if !meta.is_object() {
        *meta = serde_json::json!({});
    }
    let meta_object = meta
        .as_object_mut()
        .expect("activity meta must be an object");
    let conversation = meta_object
        .entry("goldBandConversation")
        .or_insert_with(|| serde_json::json!({}));
    if !conversation.is_object() {
        *conversation = serde_json::json!({});
    }
    conversation
        .as_object_mut()
        .expect("conversation meta must be an object")
        .insert(
            "toolDetailAvailable".to_string(),
            serde_json::Value::Bool(true),
        );
    event
}

fn remove_provider_agent_metadata(raw: &mut serde_json::Value) {
    for path in [
        &["_meta", "claudeCode"][..],
        &["_meta", "agentTranscript"][..],
        &["toolCall", "_meta", "claudeCode"][..],
        &["toolCall", "_meta", "agentTranscript"][..],
    ] {
        remove_nested_json_key(raw, path);
    }
}

fn remove_nested_json_key(value: &mut serde_json::Value, path: &[&str]) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut current = value;
    for key in parents {
        let Some(next) = current.get_mut(*key) else {
            return;
        };
        current = next;
    }
    if let Some(object) = current.as_object_mut() {
        object.remove(*last);
    }
}

fn compact_raw_value(value: serde_json::Value) -> serde_json::Value {
    const MAX_RAW_CHARS: usize = 32_000;
    let compacted = truncate_json_value(value, 8_000);
    let Ok(serialized) = serde_json::to_string(&compacted) else {
        return serde_json::json!({ "truncated": true });
    };
    if serialized.chars().count() <= MAX_RAW_CHARS {
        return compacted;
    }
    let mut fallback = serde_json::Map::new();
    for key in [
        "sessionUpdate",
        "title",
        "status",
        "requestId",
        "toolCallId",
        "toolCall",
        "rawInput",
        "locations",
        "entries",
        "_meta",
        "source",
        "synthetic",
        "optimistic",
    ] {
        if let Some(item) = compacted.get(key) {
            fallback.insert(key.to_string(), item.clone());
        }
    }
    fallback.insert("truncated".to_string(), serde_json::Value::Bool(true));
    fallback.insert(
        "summary".to_string(),
        serde_json::Value::String(truncate_string(serialized, MAX_RAW_CHARS)),
    );
    serde_json::Value::Object(fallback)
}

fn truncate_json_value(value: serde_json::Value, max_string_chars: usize) -> serde_json::Value {
    match value {
        serde_json::Value::String(value) => {
            serde_json::Value::String(truncate_string(value, max_string_chars))
        }
        serde_json::Value::Array(values) => serde_json::Value::Array(
            values
                .into_iter()
                .take(100)
                .map(|value| truncate_json_value(value, max_string_chars))
                .collect(),
        ),
        serde_json::Value::Object(values) => serde_json::Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, truncate_json_value(value, max_string_chars)))
                .collect(),
        ),
        value => value,
    }
}

fn truncate_string(value: String, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value;
    }
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    truncated.push_str("…");
    truncated
}

fn is_acp_session_active_status(status: &str) -> bool {
    matches!(
        status
            .trim()
            .to_ascii_lowercase()
            .replace('_', "-")
            .as_str(),
        "pending" | "running" | "in-progress" | "sending" | "cancelling" | "cancel-requested"
    )
}

fn effective_acp_session_status(
    persisted_status: &str,
    prompt_activity: Option<PromptActivity>,
) -> String {
    match prompt_activity {
        Some(PromptActivity::Starting) => "pending".to_string(),
        Some(PromptActivity::Accepted | PromptActivity::Running) => "running".to_string(),
        Some(PromptActivity::CancelRequested) => "cancelling".to_string(),
        None => persisted_status.to_string(),
    }
}

fn session_metadata_status(session: &serde_json::Value) -> &str {
    let availability = session
        .get("availability")
        .and_then(Value::as_str)
        .unwrap_or("unavailable");
    let activity = session
        .get("liveTurnActivity")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if activity.eq_ignore_ascii_case("cancelRequested") {
        return "cancelling";
    }
    let latest = session
        .get("latestTurnStatus")
        .and_then(Value::as_str)
        .unwrap_or("none");
    match latest.to_ascii_lowercase().as_str() {
        "completed" => "completed",
        "cancelled" | "canceled" => "cancelled",
        "failed" => "failed",
        _ if availability.eq_ignore_ascii_case("closing") => "closing",
        _ if matches!(
            availability.to_ascii_lowercase().as_str(),
            "established" | "restorable"
        ) =>
        {
            "idle"
        }
        _ => "unknown",
    }
}

fn canonical_legacy_availability(
    normalized_status: &str,
    session_established: bool,
) -> &'static str {
    match normalized_status {
        "failed" | "failure" | "error" | "killed" if session_established => "restorable",
        _ if session_established => "established",
        _ => "unavailable",
    }
}

fn canonical_legacy_activity(normalized_status: &str) -> &'static str {
    match normalized_status {
        "cancelling" | "cancel-requested" | "closing" => "cancelRequested",
        _ => "idle",
    }
}

fn canonical_legacy_latest_status(normalized_status: &str) -> &'static str {
    match normalized_status {
        "completed" | "complete" => "completed",
        "cancelled" | "canceled" => "cancelled",
        "failed" | "failure" | "error" | "killed" => "failed",
        _ => "none",
    }
}

fn normalize_preloaded_session_metadata(mut session: serde_json::Value) -> serde_json::Value {
    let legacy_status = session
        .get("status")
        .and_then(Value::as_str)
        .map(str::to_string);
    let Some(status) = legacy_status else {
        return session;
    };
    let normalized = status.trim().to_ascii_lowercase().replace('_', "-");
    let session_established = session
        .get("sessionId")
        .or_else(|| session.get("acpSessionId"))
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    session["availability"] = serde_json::json!(canonical_legacy_availability(
        &normalized,
        session_established
    ));
    session["liveTurnActivity"] = serde_json::json!(canonical_legacy_activity(&normalized));
    session["latestTurnStatus"] = serde_json::json!(canonical_legacy_latest_status(&normalized));
    if let Some(object) = session.as_object_mut() {
        object.remove("status");
    }
    session
}

fn load_session_metadata_value(path: &camino::Utf8Path) -> Option<serde_json::Value> {
    gold_band::acp::events::read_session_metadata_value(path, None).ok()
}

fn run_worktree_state_optional(
    app: &App,
    task_id: &str,
    run_id: &str,
) -> Result<Option<gold_band::runtime::RunWorktreeState>> {
    let path = app.paths.run_file(task_id, run_id);
    if !path.exists() {
        return Ok(None);
    }
    Ok(read_json::<RunState>(&path)?.worktree)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionWorktreeProjection {
    pub path: String,
    pub branch: Option<String>,
}

pub(crate) fn session_worktree_projection(
    run_worktree: Option<&gold_band::runtime::RunWorktreeState>,
    dynamic_graph: Option<&DynamicGraphState>,
    dynamic_node_id: Option<&str>,
) -> Option<SessionWorktreeProjection> {
    let (graph, dynamic_node_id) = match (dynamic_graph, dynamic_node_id) {
        (None, None) => {
            return run_worktree.map(|worktree| SessionWorktreeProjection {
                path: worktree.path.to_string(),
                branch: Some(worktree.branch.clone()),
            });
        }
        (Some(graph), Some(dynamic_node_id)) => (graph, dynamic_node_id),
        _ => return None,
    };
    let node = graph.nodes.iter().find(|node| node.id == dynamic_node_id)?;
    workspace_worktree_projection_by_id(run_worktree, &graph.workspaces, &node.workspace_id)
}

fn workspace_worktree_projection_by_id(
    run_worktree: Option<&gold_band::runtime::RunWorktreeState>,
    workspaces: &[gold_band::dynamic::WorkspaceState],
    workspace_id: &str,
) -> Option<SessionWorktreeProjection> {
    let workspace = workspaces
        .iter()
        .find(|workspace| workspace.id == workspace_id)?;
    workspace_worktree_projection(run_worktree, workspace)
}

fn workspace_worktree_projection(
    run_worktree: Option<&gold_band::runtime::RunWorktreeState>,
    workspace: &gold_band::dynamic::WorkspaceState,
) -> Option<SessionWorktreeProjection> {
    match workspace.kind {
        WorkspaceKind::Worktree
            if workspace.status != gold_band::dynamic::WorkspaceStatus::Released =>
        {
            Some(SessionWorktreeProjection {
                path: workspace.path.to_string(),
                branch: workspace.branch.clone(),
            })
        }
        WorkspaceKind::Worktree => None,
        WorkspaceKind::Main => run_worktree
            .filter(|worktree| {
                gold_band::storage::normalize_workspace_path(&worktree.path)
                    == gold_band::storage::normalize_workspace_path(&workspace.path)
            })
            .map(|worktree| SessionWorktreeProjection {
                path: worktree.path.to_string(),
                branch: Some(worktree.branch.clone()),
            }),
    }
}

fn is_acp_session_stopping_status(status: &str) -> bool {
    matches!(
        status
            .trim()
            .to_ascii_lowercase()
            .replace('_', "-")
            .as_str(),
        "cancelling" | "cancel-requested"
    )
}

fn acp_session_config_vm(session: &serde_json::Value) -> Option<AcpSessionConfigVm> {
    let catalog_observed_at = session
        .get("configCatalogObservedAt")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let models = session.get("models").cloned();
    let modes = session.get("modes").cloned();
    let config_options = session.get("configOptions").cloned();
    let model_override_id = session
        .get("modelOverride")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let permission_mode_override_id = session
        .get("permissionModeOverride")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let config_option_overrides: std::collections::BTreeMap<String, String> = session
        .get("configOptionOverrides")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    let current_model_id = config_current_value(config_options.as_ref(), "model").or_else(|| {
        models
            .as_ref()
            .and_then(|value| value.get("currentModelId"))
            .and_then(|value| value.as_str())
            .map(str::to_string)
    });
    let current_mode_id = config_current_value(config_options.as_ref(), "mode").or_else(|| {
        modes
            .as_ref()
            .and_then(|value| value.get("currentModeId"))
            .and_then(|value| value.as_str())
            .map(str::to_string)
    });
    let current_model_name = current_model_id.as_deref().and_then(|model_id| {
        config_option_display_name(config_options.as_ref(), "model", model_id)
            .or_else(|| model_display_name(models.as_ref(), model_id))
    });
    let current_mode_name = current_mode_id.as_deref().and_then(|mode_id| {
        config_option_display_name(config_options.as_ref(), "mode", mode_id)
            .or_else(|| mode_display_name(modes.as_ref(), mode_id))
    });

    if model_override_id.is_none()
        && permission_mode_override_id.is_none()
        && config_option_overrides.is_empty()
        && current_model_id.is_none()
        && current_model_name.is_none()
        && current_mode_id.is_none()
        && current_mode_name.is_none()
        && models.is_none()
        && modes.is_none()
        && config_options.is_none()
    {
        return None;
    }

    Some(AcpSessionConfigVm {
        catalog_observed_at,
        model_override_id,
        permission_mode_override_id,
        config_option_overrides,
        current_model_id,
        current_model_name,
        current_mode_id,
        current_mode_name,
        models,
        modes,
        config_options,
    })
}

fn config_current_value(
    config_options: Option<&serde_json::Value>,
    option_id: &str,
) -> Option<String> {
    find_config_option(config_options, option_id)
        .and_then(|option| option.get("currentValue"))
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

fn config_option_display_name(
    config_options: Option<&serde_json::Value>,
    option_id: &str,
    value: &str,
) -> Option<String> {
    find_config_option(config_options, option_id)
        .and_then(|option| option.get("options"))
        .and_then(|options| options.as_array())
        .and_then(|options| {
            options
                .iter()
                .find(|option| option.get("value").and_then(|item| item.as_str()) == Some(value))
        })
        .and_then(|option| option.get("name"))
        .and_then(|name| name.as_str())
        .map(str::to_string)
}

fn find_config_option<'a>(
    config_options: Option<&'a serde_json::Value>,
    option_id: &str,
) -> Option<&'a serde_json::Value> {
    config_options
        .and_then(|value| value.as_array())
        .and_then(|options| {
            options.iter().find(|option| {
                option.get("id").and_then(|item| item.as_str()) == Some(option_id)
                    || option.get("category").and_then(|item| item.as_str()) == Some(option_id)
            })
        })
}

fn model_display_name(models: Option<&serde_json::Value>, model_id: &str) -> Option<String> {
    models
        .and_then(|value| value.get("availableModels"))
        .and_then(|value| value.as_array())
        .and_then(|models| {
            models
                .iter()
                .find(|model| model.get("modelId").and_then(|item| item.as_str()) == Some(model_id))
        })
        .and_then(|model| model.get("name"))
        .and_then(|name| name.as_str())
        .map(str::to_string)
}

fn mode_display_name(modes: Option<&serde_json::Value>, mode_id: &str) -> Option<String> {
    modes
        .and_then(|value| value.get("availableModes"))
        .and_then(|value| value.as_array())
        .and_then(|modes| {
            modes
                .iter()
                .find(|mode| mode.get("id").and_then(|item| item.as_str()) == Some(mode_id))
        })
        .and_then(|mode| mode.get("name"))
        .and_then(|name| name.as_str())
        .map(str::to_string)
}

#[cfg(test)]
fn is_session_update(event: &AcpUiEventVm, session_update: &str) -> bool {
    event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("sessionUpdate"))
        .and_then(|value| value.as_str())
        == Some(session_update)
}

fn permission_request_id_from_event(event: &AcpUiEventVm) -> String {
    let value = event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("requestId"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&event.id);
    canonical_permission_request_id(value)
}

fn canonical_permission_request_id(value: &str) -> String {
    let mut current = value;
    while let Some(next) = current.strip_prefix("permission-") {
        current = next;
    }
    current.to_string()
}

#[cfg(test)]
fn insert_latest_permission_event(
    latest_permission_events: &mut HashMap<String, AcpUiEventVm>,
    event: &AcpUiEventVm,
) {
    let request_id = permission_request_id_from_event(event);
    let should_replace = latest_permission_events
        .get(&request_id)
        .map(|current| {
            event.seq > current.seq
                || (event.seq == current.seq
                    && parse_epoch_timestamp(&event.timestamp).unwrap_or_default()
                        >= parse_epoch_timestamp(&current.timestamp).unwrap_or_default())
        })
        .unwrap_or(true);
    if should_replace {
        latest_permission_events.insert(request_id, event.clone());
    }
}

fn permission_vm_from_event(event: &AcpUiEventVm) -> AcpPromptInteractionVm {
    let request_id = permission_request_id_from_event(event);
    let mut raw = event
        .raw
        .clone()
        .map(compact_raw_value)
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(object) = raw.as_object_mut() {
        object.insert(
            "requestId".to_string(),
            serde_json::Value::String(request_id.clone()),
        );
    }
    let options = raw
        .get("options")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .map(|option| AcpPermissionOptionVm {
            option_id: option
                .get("optionId")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string(),
            name: option
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string(),
            kind: option
                .get("kind")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string(),
        })
        .collect::<Vec<_>>();
    AcpPromptInteractionVm::Permission {
        interaction_id: request_id,
        turn_id: raw
            .pointer("/_meta/goldBandConversation/turnId")
            .or_else(|| raw.get("turnId"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
        prompt_event_id: raw
            .pointer("/_meta/goldBandConversation/promptEventId")
            .or_else(|| raw.get("promptEventId"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
        title: event
            .title
            .clone()
            .unwrap_or_else(|| "Permission required".to_string()),
        tool_call_id: event.tool_call_id.clone(),
        options,
        raw,
    }
}

#[cfg(test)]
fn pending_elicitation_vms(
    events: &[AcpUiEventVm],
) -> Vec<AcpPromptInteractionVm> {
    let resolved_ids = events
        .iter()
        .filter(|event| event.kind == "elicitationResponse")
        .filter_map(elicitation_id_from_event)
        .collect::<HashSet<_>>();
    let Some(request) = events
        .iter()
        .rev()
        .find(|event| event.kind == "elicitationRequest")
    else {
        return Vec::new();
    };
    let pending = request
        .status
        .as_deref()
        .is_some_and(|status| status.eq_ignore_ascii_case("pending"));
    if !pending || resolved_ids.contains(&request.id) {
        return Vec::new();
    }
    vec![elicitation_vm_from_event(request)]
}

fn elicitation_vm_from_event(event: &AcpUiEventVm) -> AcpPromptInteractionVm {
    let raw = event
        .raw
        .clone()
        .map(compact_raw_value)
        .unwrap_or_else(|| serde_json::json!({}));
    let requested_schema = raw
        .get("requestedSchema")
        .cloned()
        .or_else(|| {
            (raw.get("type").and_then(Value::as_str) == Some("object")).then(|| raw.clone())
        })
        .unwrap_or_else(|| serde_json::json!({ "type": "object", "properties": {} }));
    AcpPromptInteractionVm::Elicitation {
        interaction_id: event.id.clone(),
        turn_id: raw
            .pointer("/_meta/goldBandConversation/turnId")
            .or_else(|| raw.get("turnId"))
            .and_then(Value::as_str)
            .map(str::to_string),
        prompt_event_id: raw
            .pointer("/_meta/goldBandConversation/promptEventId")
            .or_else(|| raw.get("promptEventId"))
            .and_then(Value::as_str)
            .map(str::to_string),
        message: raw
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| event.content.clone())
            .unwrap_or_default(),
        tool_call_id: event.tool_call_id.clone().or_else(|| {
            raw.get("toolCallId")
                .and_then(Value::as_str)
                .map(str::to_string)
        }),
        requested_schema,
        raw,
    }
}

fn asset_item_vm(
    kind: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    name: String,
) -> AssetItemVm {
    AssetItemVm {
        kind: kind.to_string(),
        title: name.clone(),
        preview: name.clone(),
        tone: if kind == "artifact" {
            "accent"
        } else {
            "neutral"
        }
        .to_string(),
        round_id: round_id.to_string(),
        node_id: node_id.to_string(),
        attempt_id: attempt_id.to_string(),
        name,
    }
}

pub fn acp_raw_frame_page_vm(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    query: AcpRawFrameQueryInput,
) -> Result<AcpRawFramePageVm> {
    let path = app
        .paths
        .acp_raw_file(task_id, run_id, round_id, node_id, attempt_id);
    acp_raw_frame_page_vm_for_path(&path, query)
}

pub fn acp_raw_frame_page_vm_for_path(
    path: &camino::Utf8Path,
    query: AcpRawFrameQueryInput,
) -> Result<AcpRawFramePageVm> {
    let page = query.page.unwrap_or(0);
    let page_size = query.page_size.unwrap_or(100).clamp(25, 200);
    let search = normalized_filter(query.search);
    let kind = normalized_filter(query.kind);
    let direction = normalized_filter(query.direction);
    let order = query.order.unwrap_or_default();

    let total = count_matching_raw_frames(
        path,
        search.as_deref(),
        kind.as_deref(),
        direction.as_deref(),
    )?;
    let offset = page.saturating_mul(page_size);
    let bounded_offset = offset.min(total);
    let bounded_end_offset = offset.saturating_add(page_size).min(total);
    let (start, end) = match order {
        AcpRawFrameOrder::Asc => (bounded_offset, bounded_end_offset),
        AcpRawFrameOrder::Desc => (
            total.saturating_sub(bounded_end_offset),
            total.saturating_sub(bounded_offset),
        ),
    };
    let mut items = collect_matching_raw_frames(
        path,
        search.as_deref(),
        kind.as_deref(),
        direction.as_deref(),
        start,
        end,
    )?;
    if order == AcpRawFrameOrder::Desc {
        items.reverse();
    }

    Ok(AcpRawFramePageVm {
        items,
        page,
        page_size,
        total,
        has_previous: page > 0 && total > 0,
        has_next: offset.saturating_add(page_size) < total,
        order,
        search,
        kind,
        direction,
    })
}

fn count_matching_raw_frames(
    path: &camino::Utf8Path,
    search: Option<&str>,
    kind: Option<&str>,
    direction: Option<&str>,
) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }
    let file = fs::File::open(path.as_std_path())?;
    let mut total = 0usize;
    for line in BufReader::new(file)
        .lines()
        .map_while(std::result::Result::ok)
    {
        if raw_frame_matches(&line, search, kind, direction) {
            total += 1;
        }
    }
    Ok(total)
}

fn collect_matching_raw_frames(
    path: &camino::Utf8Path,
    search: Option<&str>,
    kind: Option<&str>,
    direction: Option<&str>,
    start: usize,
    end: usize,
) -> Result<Vec<AcpRawFrameVm>> {
    if !path.exists() || start >= end {
        return Ok(Vec::new());
    }
    let file = fs::File::open(path.as_std_path())?;
    let mut ordinal = 0usize;
    let mut items = Vec::with_capacity(end.saturating_sub(start));
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if !raw_frame_matches(&line, search, kind, direction) {
            continue;
        }
        if ordinal >= start && ordinal < end {
            items.push(raw_frame_vm(index + 1, line));
        }
        ordinal += 1;
        if ordinal >= end {
            break;
        }
    }
    Ok(items)
}

fn raw_frame_matches(
    line: &str,
    search: Option<&str>,
    kind: Option<&str>,
    direction: Option<&str>,
) -> bool {
    if let Some(search) = search {
        if !line.to_lowercase().contains(search) {
            return false;
        }
    }
    if kind.is_none() && direction.is_none() {
        return true;
    }
    let parsed = raw_frame_meta(line);
    if let Some(kind) = kind {
        if !parsed.kind.to_lowercase().contains(kind) {
            return false;
        }
    }
    if let Some(direction) = direction {
        if parsed
            .direction
            .as_deref()
            .map(str::to_lowercase)
            .as_deref()
            != Some(direction)
        {
            return false;
        }
    }
    true
}

fn raw_frame_vm(line_number: usize, content: String) -> AcpRawFrameVm {
    const MAX_CONTENT_CHARS: usize = 200_000;
    let meta = raw_frame_meta(&content);
    let content_truncated = content.chars().count() > MAX_CONTENT_CHARS;
    let content = if content_truncated {
        content.chars().take(MAX_CONTENT_CHARS).collect()
    } else {
        content
    };
    AcpRawFrameVm {
        id: format!("raw-{line_number}"),
        line_number,
        timestamp: meta.timestamp,
        direction: meta.direction,
        kind: meta.kind,
        content,
        content_truncated,
    }
}

struct RawFrameMeta {
    timestamp: Option<String>,
    direction: Option<String>,
    kind: String,
}

fn raw_frame_meta(line: &str) -> RawFrameMeta {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return RawFrameMeta {
            timestamp: None,
            direction: None,
            kind: "parse-error".to_string(),
        };
    };
    let frame = value.get("frame");
    let kind = frame
        .and_then(|frame| frame.pointer("/params/update/sessionUpdate"))
        .and_then(|item| item.as_str())
        .or_else(|| {
            frame
                .and_then(|frame| frame.get("method"))
                .and_then(|item| item.as_str())
        })
        .map(str::to_string)
        .or_else(|| {
            frame
                .and_then(|frame| frame.get("error"))
                .map(|_| "error".to_string())
        })
        .or_else(|| {
            frame
                .and_then(|frame| frame.get("result"))
                .map(|_| "result".to_string())
        })
        .unwrap_or_else(|| "frame".to_string());
    RawFrameMeta {
        timestamp: json_string(&value, "timestamp"),
        direction: json_string(&value, "direction"),
        kind,
    }
}

fn normalized_filter(value: Option<String>) -> Option<String> {
    value
        .map(|item| item.trim().to_lowercase())
        .filter(|item| !item.is_empty())
}

pub fn log_page_vm(app: &App, query: LogQueryInput) -> Result<LogPageVm> {
    let page = query.page.unwrap_or(0);
    let page_size = query.page_size.unwrap_or(50).clamp(10, 200);
    let hot_limit = query.hot_limit.unwrap_or(1000).clamp(page_size, 5000);
    let source = query.source.as_deref().unwrap_or("system");
    let lines = log_lines_for_query(app, &query, source, hot_limit)?;
    let mut items = lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| log_entry_from_line(index, source, &line))
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.id.cmp(&right.id))
    });
    let total = items.len();
    let start = page.saturating_mul(page_size).min(total);
    let end = (start + page_size).min(total);
    let page_items = items[start..end].to_vec();

    Ok(LogPageVm {
        items: page_items,
        page,
        page_size,
        total,
        has_previous: page > 0 && total > 0,
        has_next: end < total,
        tier: "hot".to_string(),
        hot_limit,
        archive_retention_days: app.config.log_retention_days,
    })
}

fn log_lines_for_query(
    app: &App,
    query: &LogQueryInput,
    source: &str,
    hot_limit: usize,
) -> Result<Vec<String>> {
    let scope = &query.scope;
    let path = match source {
        "progress-events" => match (&scope.round_id, &scope.node_id, &scope.attempt_id) {
            (Some(round_id), Some(node_id), Some(attempt_id)) => app.paths.progress_events_file(
                &scope.task_id,
                &scope.run_id,
                round_id,
                node_id,
                attempt_id,
            ),
            _ => return Ok(Vec::new()),
        },
        "raw-stream" => match (&scope.round_id, &scope.node_id, &scope.attempt_id) {
            (Some(round_id), Some(node_id), Some(attempt_id)) => app.paths.raw_stream_file(
                &scope.task_id,
                &scope.run_id,
                round_id,
                node_id,
                attempt_id,
            ),
            _ => return Ok(Vec::new()),
        },
        "run-events" | "system" => app.paths.run_events_file(&scope.task_id, &scope.run_id),
        _ => app.paths.run_events_file(&scope.task_id, &scope.run_id),
    };
    if path.exists() {
        return read_tail_lines(&path, hot_limit);
    }
    if source == "system" {
        return read_tail_lines(&app.paths.runtime_log_file(), hot_limit);
    }
    Ok(Vec::new())
}

fn read_tail_lines(path: &camino::Utf8Path, limit: usize) -> Result<Vec<String>> {
    if !path.exists() || limit == 0 {
        return Ok(Vec::new());
    }
    let mut file = fs::File::open(path.as_std_path())?;
    let file_len = file.metadata()?.len();
    if file_len == 0 {
        return Ok(Vec::new());
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
        newline_count += buffer[..read_len]
            .iter()
            .filter(|&&byte| byte == b'\n')
            .count();
        chunks.push(buffer[..read_len].to_vec());
    }
    chunks.reverse();
    let text = String::from_utf8(chunks.concat())?;
    let normalized = text.strip_suffix('\n').unwrap_or(&text);
    let lines = normalized.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(limit);
    Ok(lines[start..]
        .iter()
        .map(|line| (*line).to_string())
        .collect())
}

fn log_entry_from_line(index: usize, source: &str, line: &str) -> LogEntryVm {
    match serde_json::from_str::<serde_json::Value>(line) {
        Ok(value) => log_entry_from_json(index, source, value),
        Err(_) => LogEntryVm {
            id: format!("{source}-{index}"),
            timestamp: String::new(),
            entry_type: if source == "system" {
                "runtime"
            } else {
                "parse-error"
            }
            .to_string(),
            level: None,
            node_id: None,
            attempt_id: None,
            stage: None,
            summary: preview_text(line, 240),
            source: source.to_string(),
            raw: serde_json::Value::String(line.to_string()),
        },
    }
}

fn log_entry_from_json(index: usize, source: &str, value: serde_json::Value) -> LogEntryVm {
    let data = value.get("data");
    let timestamp = json_string(&value, "timestamp").unwrap_or_default();
    let entry_type = json_string(&value, "type")
        .or_else(|| json_string(&value, "stream"))
        .or_else(|| data.and_then(|data| json_string(data, "rawEventType")))
        .unwrap_or_else(|| source.to_string());
    let node_id = data
        .and_then(|data| json_string(data, "nodeId"))
        .or_else(|| data.and_then(|data| json_string(data, "node_id")));
    let attempt_id = data
        .and_then(|data| json_string(data, "attemptId"))
        .or_else(|| data.and_then(|data| json_string(data, "attempt_id")));
    let stage = data.and_then(|data| json_string(data, "stage"));
    let summary = data
        .and_then(|data| json_string(data, "summary"))
        .or_else(|| data.and_then(|data| json_string(data, "content")))
        .or_else(|| {
            data.and_then(|data| json_string(data, "toolName"))
                .map(|tool| format!("tool: {tool}"))
        })
        .or_else(|| json_string(&value, "content"))
        .unwrap_or_else(|| preview_text(&value.to_string(), 240));

    LogEntryVm {
        id: format!("{source}-{index}"),
        timestamp,
        entry_type,
        level: json_string(&value, "level").or_else(|| json_string(&value, "stream")),
        node_id,
        attempt_id,
        stage,
        summary: preview_text(&summary, 240),
        source: source.to_string(),
        raw: value,
    }
}

fn json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(|value| value.to_string())
}

fn count_task_outputs(app: &App, task_id: &str) -> Result<(usize, usize)> {
    let mut artifacts = 0usize;
    let mut attachments = 0usize;
    for run in app.run_list(task_id)? {
        for round in app.round_list(task_id, &run.id)? {
            let (round_artifacts, round_attachments) =
                count_round_outputs(app, task_id, &run.id, &round.id)?;
            artifacts += round_artifacts;
            attachments += round_attachments;
        }
    }
    Ok((artifacts, attachments))
}

fn count_round_outputs(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
) -> Result<(usize, usize)> {
    let mut artifacts = 0usize;
    let mut attachments = 0usize;
    for node in app.node_list(task_id, run_id, round_id)? {
        for attempt in app.attempt_list(task_id, run_id, round_id, &node.node_id)? {
            artifacts += app
                .artifact_list(
                    task_id,
                    run_id,
                    round_id,
                    &node.node_id,
                    &attempt.attempt_id,
                )?
                .len();
            attachments += app
                .attachment_list(
                    task_id,
                    run_id,
                    round_id,
                    &node.node_id,
                    &attempt.attempt_id,
                )?
                .len();
        }
    }
    Ok((artifacts, attachments))
}

fn workflow_node_labels(app: &App, task_id: &str, run_id: &str) -> HashMap<String, String> {
    read_json::<WorkflowDsl>(&app.paths.workflow_snapshot_file(task_id, run_id))
        .or_else(|_| app.task_workflow(task_id))
        .map(|workflow| {
            workflow
                .nodes
                .iter()
                .map(|node| (node.id().to_string(), node_label(node)))
                .collect()
        })
        .unwrap_or_default()
}

fn node_label(node: &NodeDsl) -> String {
    match node {
        NodeDsl::Worker(node) => node.goal.clone().unwrap_or_else(|| node.id.clone()),
        NodeDsl::AiDynamic(node) => node.id.clone(),
    }
}

fn enum_label<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(label)) => label,
        Ok(value) => value.to_string(),
        Err(_) => "unknown".to_string(),
    }
}

fn empty_graph() -> GraphVm {
    GraphVm {
        nodes: Vec::new(),
        edges: Vec::new(),
    }
}

fn read_optional_text(path: &camino::Utf8Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(fs::read_to_string(path)?))
}

fn preview_text(text: &str, limit: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= limit {
        compact
    } else {
        format!("{}…", compact.chars().take(limit).collect::<String>())
    }
}

fn newest_first<T>(mut items: Vec<T>) -> Vec<T> {
    items.reverse();
    items
}

// ── MCP Server VM ──

pub fn mcp_server_list_vm(
    servers: &[gold_band::config::McpServerConfig],
    health: &std::collections::BTreeMap<String, McpServerState>,
) -> Vec<McpServerVm> {
    servers
        .iter()
        .map(|s| {
            let (transport, command, args, env, url, headers) = match &s.transport {
                gold_band::config::McpTransportConfig::Stdio {
                    command: cmd,
                    args: a,
                    env: e,
                } => (
                    "stdio".to_string(),
                    Some(cmd.clone()),
                    Some(a.clone()),
                    Some(env_to_entries(e)),
                    None,
                    None,
                ),
                gold_band::config::McpTransportConfig::Http {
                    url: u, headers: h, ..
                } => (
                    "http".to_string(),
                    None,
                    None,
                    None,
                    Some(u.clone()),
                    Some(env_to_entries(h)),
                ),
                gold_band::config::McpTransportConfig::Sse { url: u, headers: h } => (
                    "sse".to_string(),
                    None,
                    None,
                    None,
                    Some(u.clone()),
                    Some(env_to_entries(h)),
                ),
            };
            let (health_status, health_message) = match health.get(&s.id) {
                Some(McpServerState::Running { .. }) => (Some("healthy".to_string()), None),
                Some(McpServerState::Error { message }) => {
                    (Some("unhealthy".to_string()), Some(message.clone()))
                }
                Some(McpServerState::AuthRequired { auth_url }) => {
                    (Some("auth_required".to_string()), auth_url.clone())
                }
                Some(McpServerState::Stopped) => (Some("stopped".to_string()), None),
                Some(McpServerState::Starting) => (Some("checking".to_string()), None),
                None => (None, None),
            };
            McpServerVm {
                id: s.id.clone(),
                name: s.name.clone(),
                enabled: s.enabled,
                transport,
                command,
                args,
                env,
                url,
                headers,
                managed: s.managed,
                help_message: s.help_message.clone(),
                health_status,
                health_message,
            }
        })
        .collect()
}

fn env_to_entries(map: &std::collections::BTreeMap<String, String>) -> Vec<AgentEnvEntryVm> {
    map.iter()
        .map(|(k, v)| AgentEnvEntryVm {
            key: k.clone(),
            value: v.clone(),
        })
        .collect()
}

// ── SKILL VM ──

pub fn skill_list_vm(result: &gold_band::skill::SkillListResult) -> SkillListVm {
    SkillListVm {
        global: result.global.iter().map(skill_meta_vm).collect(),
        project: result.project.iter().map(skill_meta_vm).collect(),
    }
}

pub fn skill_content_vm(content: &gold_band::skill::SkillContent) -> SkillContentVm {
    SkillContentVm {
        meta: skill_meta_vm(&content.meta),
        description_source: content.description_source.clone(),
        body: content.body.clone(),
    }
}

pub fn skill_meta_vm(meta: &gold_band::config::SkillMeta) -> SkillMetaVm {
    SkillMetaVm {
        name: meta.name.clone(),
        description: meta.description.clone(),
        source: skill_source_str(meta.source),
        directory_path: meta.directory_path.clone(),
        agent_source: meta.agent_source.clone(),
        load_warnings: meta.load_warnings.clone(),
        synced_agent_types: meta.synced_agent_types.clone(),
    }
}

fn skill_source_str(source: gold_band::config::SkillSource) -> String {
    match source {
        gold_band::config::SkillSource::BuiltIn => "built-in".to_string(),
        gold_band::config::SkillSource::Global => "global".to_string(),
        gold_band::config::SkillSource::Project => "project".to_string(),
    }
}

// ── SKILL Sync Status ──

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatusEntryVm {
    pub agent_type: String,
    pub is_synced: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use gold_band::app::App;
    use gold_band::domain::{PauseReason, RunStatus, VERSION};
    use gold_band::runtime::{RunState, RuntimeExecutionPhase, RuntimeExecutionState};
    use gold_band::storage::write_json;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn app_config_vm_exposes_workspace_layout_contract() {
        let vm = app_config_vm(&RuntimeConfig::default());
        let value = serde_json::to_value(vm).unwrap();

        assert_eq!(value["conversationInlineContentMaxBytes"], 64_000);
        assert_eq!(value["conversationInlineImageMaxBytes"], 4 * 1024 * 1024);
        assert_eq!(value["conversationInlineImageMaxDimension"], 2_560);
        assert_eq!(value["workspaceLayout"]["shellMinWidth"], 480);
        assert_eq!(value["workspaceLayout"]["shellMinHeight"], 680);
        assert_eq!(value["workspaceLayout"]["rightWorkspace"]["minWidth"], 288);
        assert_eq!(
            value["workspaceLayout"]["rightWorkspace"]["defaultWidth"],
            440
        );
        assert_eq!(value["workspaceLayout"]["rightWorkspace"]["maxWidth"], 1440);
        assert_eq!(
            value["workspaceLayout"]["rightWorkspace"]["file"],
            json!({
                "splitMinWidth": 500,
                "treeDefaultWidth": 280,
                "treeMinWidth": 200,
                "treeMaxWidth": 420,
            })
        );
        assert_eq!(
            value["workspaceLayout"]["conversation"]["centerMinWidth"],
            360
        );
        assert_eq!(
            value["workspaceLayout"]["workflowCanvas"]["windowMinWidth"],
            640
        );
    }

    #[test]
    fn run_detail_only_exposes_progress_for_the_authoritative_runtime_revision() {
        let temp = tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        let run = RunState {
            version: VERSION.to_string(),
            id: "run-001".to_string(),
            task_id: "task-001".to_string(),
            task_uuid: None,
            status: RunStatus::Paused,
            outcome: None,
            started_at: "t0".to_string(),
            updated_at: "t1".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: None,
            current_node: None,
            current_attempt: None,
            new_rounds_opened: 0,
            pause_reason: Some(PauseReason::ProcessInterrupted),
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: RuntimeExecutionState {
                revision: 7,
                phase: RuntimeExecutionPhase::Paused,
                locator: None,
                recovery_candidate_token: None,
                updated_at: "t1".to_string(),
            },
        };
        write_json(&app.paths.run_file("task-001", "run-001"), &run).unwrap();
        write_json(
            &app.paths.run_progress_file("task-001", "run-001"),
            &json!({ "runtimeRevision": 6, "status": "running" }),
        )
        .unwrap();

        let stale = run_detail_vm(&app, "task-001", "run-001").unwrap();
        assert!(stale.progress.is_none());

        write_json(
            &app.paths.run_progress_file("task-001", "run-001"),
            &json!({ "runtimeRevision": 7, "status": "paused" }),
        )
        .unwrap();
        let current = run_detail_vm(&app, "task-001", "run-001").unwrap();
        assert_eq!(
            current
                .progress
                .as_ref()
                .and_then(|value| value["status"].as_str()),
            Some("paused")
        );
    }

    fn test_event(kind: &str, content: &str) -> AcpUiEventVm {
        AcpUiEventVm {
            id: format!("{kind}-{content}"),
            seq: 1,
            timestamp: "1778771541Z".to_string(),
            kind: kind.to_string(),
            session_id: Some("session-123".to_string()),
            content: Some(content.to_string()),
            title: None,
            tool_call_id: None,
            status: Some("completed".to_string()),
            started_seq: None,
            ended_seq: None,
            started_at: None,
            ended_at: None,
            timing: None,
            raw: Some(json!({ "source": "goldBandPrompt" })),
        }
    }

    #[test]
    fn session_projection_restores_missing_initial_task_attachments() {
        let mut initial_prompt = test_event("userTextDelta", "hi");
        initial_prompt.raw = Some(json!({
            "source": "goldBandPrompt",
            "attachments": [{
                "name": "image.png",
                "path": "task-inputs/image.png",
                "type": "image/png",
                "size": 81_401
            }]
        }));
        let mut later_prompt = test_event("userTextDelta", "follow up");
        later_prompt.raw = Some(json!({
            "source": "goldBandPrompt",
            "promptId": "prompt-002"
        }));
        let mut events = vec![initial_prompt, later_prompt];

        merge_initial_task_attachment_values(
            &mut events,
            vec![
                json!({
                    "name": "image.png",
                    "path": "task-inputs/image.png",
                    "type": "image/png",
                    "size": 81_401
                }),
                json!({
                    "name": "acp.raw.jsonl",
                    "path": "task-inputs/acp.raw.jsonl",
                    "type": "application/json",
                    "size": 1_672_643
                }),
            ],
        );

        let attachments = events[0]
            .raw
            .as_ref()
            .and_then(|raw| raw.get("attachments"))
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(attachments.len(), 2);
        assert_eq!(attachments[1]["name"], "acp.raw.jsonl");
        assert!(
            events[1]
                .raw
                .as_ref()
                .and_then(|raw| raw.get("attachments"))
                .is_none()
        );
    }

    fn test_agent_record(
        agent_execution_id: &str,
        parent_agent_execution_id: Option<&str>,
        status: &str,
    ) -> gold_band::acp::branches::AgentExecutionRecord {
        gold_band::acp::branches::AgentExecutionRecord {
            agent_execution_id: agent_execution_id.to_string(),
            parent_agent_execution_id: parent_agent_execution_id.map(str::to_string),
            launch_tool_call_id: format!("launch-{agent_execution_id}"),
            session_id: "session-123".to_string(),
            status: status.to_string(),
            title: Some(agent_execution_id.to_string()),
            description: None,
            started_at: "100Z".to_string(),
            updated_at: "120Z".to_string(),
            ended_at: (status == "completed").then(|| "120Z".to_string()),
            event_count: 1,
            tool_call_count: 1,
            read_file_count: 0,
            written_file_count: 0,
            has_attention: false,
            latest_cursor: Some("seq:2".to_string()),
            todo_entries: Vec::new(),
        }
    }

    #[test]
    fn usage_projection_keeps_last_confirmed_value_across_transient_zeroes() {
        let mut usage = None;

        for used in [0, 28_084, 0, 34_791, 0] {
            merge_confirmed_usage_observation(&mut usage, Some(used), Some(1_000_000), None);
        }

        let usage = usage.unwrap();
        assert_eq!(usage.used, Some(34_791));
        assert_eq!(usage.size, Some(1_000_000));
    }

    #[test]
    fn session_view_consumes_only_internal_agent_metadata_for_historical_events() {
        let mut event = test_event("toolCall", "");
        event.tool_call_id = Some("call-child".to_string());
        event.raw = Some(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call-child",
            "_meta": {
                "goldBandConversation": {
                    "branchId": "agent-internal-parent",
                    "launchedAgentExecutionId": "agent-internal-child",
                    "toolName": "Agent"
                },
                "claudeCode": {
                    "toolName": "ConflictingProviderTool",
                    "subagent": true,
                    "parentToolUseId": "provider-parent-must-not-be-consumed"
                }
            }
        }));

        let event = compact_event_for_session(event);
        let conversation = event
            .raw
            .as_ref()
            .and_then(|raw| raw.pointer("/_meta/goldBandConversation"))
            .expect("normalized conversation metadata");

        assert_eq!(conversation["branchId"], "agent-internal-parent");
        assert_eq!(
            conversation["launchedAgentExecutionId"],
            "agent-internal-child"
        );
        assert_eq!(conversation["toolName"], "Agent");
        assert!(
            event
                .raw
                .as_ref()
                .unwrap()
                .pointer("/_meta/claudeCode")
                .is_none()
        );
        assert!(
            event
                .raw
                .as_ref()
                .unwrap()
                .pointer("/_meta/agentTranscript")
                .is_none()
        );
    }

    #[test]
    fn usage_projection_preserves_cost_when_zero_sample_is_ignored() {
        let mut usage = Some(AcpUsageVm {
            used: Some(38_223),
            size: Some(1_000_000),
            ..Default::default()
        });

        merge_confirmed_usage_observation(&mut usage, Some(0), Some(1_000_000), Some(0.315532));

        let usage = usage.unwrap();
        assert_eq!(usage.used, Some(38_223));
        assert_eq!(usage.cost_amount_usd, Some(0.315532));
    }

    #[test]
    fn conversation_usage_projects_cumulative_attempt_totals_not_latest_prompt() {
        let mut usage = AcpUsageVm {
            input_tokens: Some(7_453),
            output_tokens: Some(315),
            cached_read_tokens: Some(16_896),
            total_tokens: Some(24_664),
            ..Default::default()
        };
        let session = json!({
            "inputTokens": 7_453,
            "outputTokens": 315,
            "cachedReadTokens": 16_896,
            "totalTokens": 24_664,
            "attemptInputTokens": 16_510,
            "attemptOutputTokens": 330,
            "attemptCachedReadTokens": 24_576,
            "attemptTotalTokens": 41_416
        });

        apply_persisted_attempt_token_totals(&mut usage, &session);

        assert_eq!(usage.input_tokens, Some(16_510));
        assert_eq!(usage.output_tokens, Some(330));
        assert_eq!(usage.cached_read_tokens, Some(24_576));
        assert_eq!(usage.total_tokens, Some(41_416));
    }

    #[test]
    fn conversation_usage_prefers_recovered_usage_journal_over_stale_snapshot() {
        let mut usage = AcpUsageVm::default();
        let stale_snapshot = json!({
            "attemptInputTokens": 60_852,
            "attemptOutputTokens": 2_673,
            "attemptCachedReadTokens": 370_176,
            "attemptTotalTokens": 433_701
        });
        apply_persisted_attempt_token_totals(&mut usage, &stale_snapshot);
        apply_recovered_attempt_token_totals(
            &mut usage,
            &gold_band::acp::usage::AcpAttemptUsageRecovery {
                totals: gold_band::acp::usage::AcpAttemptTokenTotals {
                    input_tokens: Some(133_877),
                    output_tokens: Some(4_430),
                    cached_read_tokens: Some(523_776),
                    cached_write_tokens: Some(0),
                    total_tokens: Some(662_083),
                },
                latest_prompt: Default::default(),
                completed_turns: 6,
                recovered_turns: 3,
            },
        );

        assert_eq!(usage.input_tokens, Some(133_877));
        assert_eq!(usage.output_tokens, Some(4_430));
        assert_eq!(usage.cached_read_tokens, Some(523_776));
        assert_eq!(usage.total_tokens, Some(662_083));
    }

    fn acp_event_at(
        id: &str,
        kind: &str,
        status: Option<&str>,
        timestamp: u64,
        raw: Option<serde_json::Value>,
    ) -> AcpUiEventVm {
        AcpUiEventVm {
            id: id.to_string(),
            seq: 1,
            timestamp: format!("{timestamp}Z"),
            kind: kind.to_string(),
            session_id: Some("session-123".to_string()),
            content: Some(id.to_string()),
            title: None,
            tool_call_id: None,
            status: status.map(str::to_string),
            started_seq: None,
            ended_seq: None,
            started_at: None,
            ended_at: None,
            timing: None,
            raw,
        }
    }

    fn gold_band_prompt_at(timestamp: u64) -> AcpUiEventVm {
        acp_event_at(
            &format!("prompt-{timestamp}"),
            "userTextDelta",
            Some("completed"),
            timestamp,
            Some(json!({ "source": "goldBandPrompt" })),
        )
    }

    fn text_event_at(timestamp: u64) -> AcpUiEventVm {
        acp_event_at(
            &format!("text-{timestamp}"),
            "textDelta",
            Some("completed"),
            timestamp,
            None,
        )
    }

    fn metadata_update_at(kind: &str, session_update: &str, timestamp: u64) -> AcpUiEventVm {
        acp_event_at(
            &format!("{kind}-{timestamp}"),
            kind,
            None,
            timestamp,
            Some(json!({ "sessionUpdate": session_update })),
        )
    }

    #[test]
    fn raw_frame_page_defaults_to_descending_and_supports_ascending_order() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "gold-band-raw-frame-order-{}-{unique}.jsonl",
            std::process::id()
        ));
        let path = Utf8PathBuf::from_path_buf(path).unwrap();
        let contents = (1..=30)
            .map(|index| {
                json!({
                    "timestamp": format!("2026-07-28T12:00:{index:02}Z"),
                    "direction": "inbound",
                    "frame": {
                        "jsonrpc": "2.0",
                        "method": "session/update",
                        "params": { "index": index },
                    },
                })
                .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(path.as_std_path(), contents).unwrap();

        let default_page = acp_raw_frame_page_vm_for_path(
            &path,
            AcpRawFrameQueryInput {
                page: Some(0),
                page_size: Some(25),
                search: None,
                kind: None,
                direction: None,
                order: None,
            },
        )
        .unwrap();
        assert_eq!(default_page.order, AcpRawFrameOrder::Desc);
        assert_eq!(
            default_page
                .items
                .iter()
                .map(|item| item.line_number)
                .collect::<Vec<_>>(),
            (6..=30).rev().collect::<Vec<_>>()
        );
        assert!(!default_page.has_previous);
        assert!(default_page.has_next);

        let ascending_second_page = acp_raw_frame_page_vm_for_path(
            &path,
            AcpRawFrameQueryInput {
                page: Some(1),
                page_size: Some(25),
                search: None,
                kind: None,
                direction: None,
                order: Some(AcpRawFrameOrder::Asc),
            },
        )
        .unwrap();
        assert_eq!(ascending_second_page.order, AcpRawFrameOrder::Asc);
        assert_eq!(
            ascending_second_page
                .items
                .iter()
                .map(|item| item.line_number)
                .collect::<Vec<_>>(),
            (26..=30).collect::<Vec<_>>()
        );
        assert!(ascending_second_page.has_previous);
        assert!(!ascending_second_page.has_next);

        fs::remove_file(path.as_std_path()).unwrap();
    }

    fn permission_event_at(request_id: &str, status: &str, timestamp: u64) -> AcpUiEventVm {
        acp_event_at(
            request_id,
            "permissionRequest",
            Some(status),
            timestamp,
            Some(json!({
                "requestId": request_id,
                "options": [
                    { "optionId": "allow-once", "name": "Allow once", "kind": "allow_once" },
                    { "optionId": "reject-once", "name": "Reject", "kind": "reject_once" }
                ]
            })),
        )
    }

    fn multi_option_permission_event_at(
        request_id: &str,
        status: &str,
        timestamp: u64,
    ) -> AcpUiEventVm {
        acp_event_at(
            request_id,
            "permissionRequest",
            Some(status),
            timestamp,
            Some(json!({
                "requestId": request_id,
                "options": [
                    { "optionId": "allow-once", "name": "Allow once", "kind": "allow_once" },
                    { "optionId": "reject-once", "name": "Reject", "kind": "reject_once" }
                ]
            })),
        )
    }

    fn elicitation_request_event_at(elicitation_id: &str, timestamp: u64) -> AcpUiEventVm {
        let mut event = acp_event_at(
            elicitation_id,
            "elicitationRequest",
            Some("pending"),
            timestamp,
            Some(json!({
                "message": "Choose a database",
                "toolCallId": "ask-tool-1",
                "requestedSchema": {
                    "type": "object",
                    "properties": {
                        "database": { "type": "string" }
                    }
                }
            })),
        );
        event.content = Some("Choose a database".to_string());
        event.tool_call_id = Some("ask-tool-1".to_string());
        event
    }

    fn elicitation_response_event_at(elicitation_id: &str, timestamp: u64) -> AcpUiEventVm {
        acp_event_at(
            &format!("{elicitation_id}-response"),
            "elicitationResponse",
            Some("completed"),
            timestamp,
            Some(json!({ "elicitationId": elicitation_id, "action": "accept" })),
        )
    }

    fn elapsed_for(
        events: Vec<AcpUiEventVm>,
        session_active: bool,
        now: Option<u64>,
    ) -> Option<u64> {
        let mut state = AcpSessionElapsedState::default();
        for event in events {
            state.observe_event(&event);
        }
        state.finish_at(session_active, now)
    }

    fn seed_dynamic_round_graph_fixture(app: &App) {
        let task_id = "task-dynamic-round-graph";
        let run_id = "run-001";
        let round_id = "round-001";
        let workflow = json!({
            "version": "0.1",
            "id": "dynamic-to-accept",
            "entry": "ai-dynamic1",
            "control": {},
            "nodes": [
                {
                    "type": "ai-dynamic",
                    "id": "ai-dynamic1",
                    "agentStrategy": { "mode": "fixed", "provider": "claude-acp" },
                    "control": {}
                },
                {
                    "type": "worker",
                    "id": "accept",
                    "provider": "claude-acp",
                    "profile": "pf-builtin-accept"
                }
            ],
            "edges": [
                { "from": "ai-dynamic1", "to": "accept", "on": "success" },
                { "from": "accept", "to": "$end", "on": "success" }
            ]
        });
        write_json(
            &app.paths.task_file(task_id),
            &json!({
                "version": "0.1",
                "id": task_id,
                "title": "Dynamic round graph"
            }),
        )
        .unwrap();
        write_json(&app.paths.workflow_file(task_id), &workflow).unwrap();
        write_json(
            &app.paths.workflow_snapshot_file(task_id, run_id),
            &workflow,
        )
        .unwrap();
        write_json(
            &app.paths.run_file(task_id, run_id),
            &json!({
                "version": "0.1",
                "id": run_id,
                "task_id": task_id,
                "status": "completed",
                "outcome": "success",
                "started_at": "2026-06-17T10:00:00Z",
                "updated_at": "2026-06-17T10:03:00Z",
                "workflow_snapshot": "workflow.snapshot.json",
                "current_round": null,
                "current_node": null,
                "current_attempt": null,
                "new_rounds_opened": 0,
                "pause_reason": null
            }),
        )
        .unwrap();
        write_json(
            &app.paths.round_file(task_id, run_id, round_id),
            &json!({
                "version": "0.1",
                "id": round_id,
                "run_id": run_id,
                "index": 1,
                "status": "completed",
                "outcome": "success",
                "trigger": "initial",
                "started_at": "2026-06-17T10:00:00Z",
                "trace": [
                    {
                        "sequence": 1,
                        "node_id": "ai-dynamic1",
                        "attempt_id": "attempt-001",
                        "from_node_id": null,
                        "edge_outcome": null,
                        "entered_at": "2026-06-17T10:00:00Z"
                    },
                    {
                        "sequence": 2,
                        "node_id": "accept",
                        "attempt_id": "attempt-001",
                        "from_node_id": "ai-dynamic1",
                        "edge_outcome": "success",
                        "entered_at": "2026-06-17T10:03:00Z"
                    }
                ]
            }),
        )
        .unwrap();
        write_json(
            &app.paths
                .node_file(task_id, run_id, round_id, "ai-dynamic1", "attempt-001"),
            &json!({
                "version": "0.1",
                "node_id": "ai-dynamic1",
                "node_type": "ai-dynamic",
                "run_id": run_id,
                "round_id": round_id,
                "attempt_id": "attempt-001",
                "status": "completed",
                "outcome": "success",
                "started_at": "2026-06-17T10:00:00Z",
                "finished_at": "2026-06-17T10:02:50Z",
                "manual_check_pending": false,
                "resolved_config": {}
            }),
        )
        .unwrap();
        write_json(
            &app.paths
                .node_file(task_id, run_id, round_id, "accept", "attempt-001"),
            &json!({
                "version": "0.1",
                "node_id": "accept",
                "node_type": "worker",
                "run_id": run_id,
                "round_id": round_id,
                "attempt_id": "attempt-001",
                "status": "completed",
                "outcome": "success",
                "started_at": "2026-06-17T10:03:00Z",
                "finished_at": "2026-06-17T10:03:20Z",
                "manual_check_pending": false,
                "resolved_config": { "provider": "claude-acp" }
            }),
        )
        .unwrap();
        write_json(
            &app.paths
                .dynamic_graph_file(task_id, run_id, round_id, "ai-dynamic1", "attempt-001"),
            &json!({
                "version": gold_band::dynamic_store::CURRENT_DYNAMIC_GRAPH_VERSION,
                "run": {
                    "version": "0.1",
                    "id": "dynamic-run-001",
                    "parentRunId": run_id,
                    "parentRoundId": round_id,
                    "parentNodeId": "ai-dynamic1",
                    "parentAttemptId": "attempt-001",
                    "status": "completed",
                    "outcome": "success",
                    "pauseReason": null,
                    "startedAt": "2026-06-17T10:00:00Z",
                    "updatedAt": "2026-06-17T10:02:50Z",
                    "control": {},
                    "allowedWorkflowSnapshots": [],
                    "currentNodeIds": []
                },
                "nodes": [
                    {
                        "version": "0.1",
                        "id": "bootstrap",
                        "dynamicRunId": "dynamic-run-001",
                        "kind": "worker",
                        "title": "AI-DYNAMIC bootstrap",
                        "task": "Design the first internal dynamic step.",
                        "status": "completed",
                        "outcome": "success",
                        "groupId": null,
                        "chainId": "bootstrap",
                        "depth": 0,
                        "dependsOn": [],
                        "workspaceId": "workspace-main",
                        "provider": "claude-acp",
                        "profile": null,
                        "permissionMode": "bypassPermissions",
                        "model": null,
                        "sessionMode": "new",
                        "continueFromNodeId": null,
                        "workflowId": null,
                        "workflowSnapshotId": null,
                        "childRunId": null,
                        "startedAt": "2026-06-17T10:00:00Z",
                        "finishedAt": "2026-06-17T10:01:00Z"
                    },
                    {
                        "version": "0.1",
                        "id": "create-hello-world-py",
                        "dynamicRunId": "dynamic-run-001",
                        "kind": "worker",
                        "title": "Create hello-world Python class",
                        "task": "Create hello_world.py.",
                        "status": "completed",
                        "outcome": "success",
                        "groupId": null,
                        "chainId": "bootstrap",
                        "depth": 1,
                        "dependsOn": [],
                        "workspaceId": "workspace-main",
                        "provider": "claude-acp",
                        "profile": "pf-builtin-dev",
                        "permissionMode": "bypassPermissions",
                        "model": null,
                        "sessionMode": "new",
                        "continueFromNodeId": null,
                        "workflowId": null,
                        "workflowSnapshotId": null,
                        "childRunId": null,
                        "startedAt": "2026-06-17T10:01:00Z",
                        "finishedAt": "2026-06-17T10:02:50Z"
                    }
                ],
                "groups": [],
                "workspaces": [{
                    "version": "0.1",
                    "id": "workspace-main",
                    "dynamicRunId": "dynamic-run-001",
                    "kind": "main",
                    "ownership": "user",
                    "repoRoot": app.paths.repo_root,
                    "path": app.paths.repo_root,
                    "branch": null,
                    "parentWorkspaceId": null,
                    "createdByGroupId": null,
                    "forkCommit": "test-head",
                    "checkpointCommit": null,
                    "status": "active",
                    "createdAt": "2026-06-17T10:00:00Z",
                    "updatedAt": "2026-06-17T10:00:00Z"
                }],
                "proposals": []
            }),
        )
        .unwrap();
    }

    #[test]
    fn runtime_display_marks_workflow_failure_as_non_blocking() {
        let failure = runtime_display_vm(Some("completed"), Some("failure"), false, None, false);
        let error_blocked =
            runtime_display_vm(Some("paused"), None, true, Some("error-blocked"), true);
        let runtime_abnormal =
            runtime_display_vm(Some("paused"), None, true, Some("runtime-abnormal"), true);
        let killed = runtime_display_vm(Some("completed"), Some("killed"), false, None, false);

        assert_eq!(failure.tone, "danger");
        assert!(!failure.blocking_error);
        assert!(error_blocked.blocking_error);
        assert!(!error_blocked.resumable);
        assert_eq!(runtime_abnormal.code, "runtime-abnormal");
        assert_eq!(runtime_abnormal.tone, "danger");
        assert!(!runtime_abnormal.blocking_error);
        assert!(runtime_abnormal.resumable);
        assert!(killed.blocking_error);
    }

    #[test]
    fn round_graph_connects_ai_dynamic_exit_to_next_workflow_node() {
        let directory = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let app = App::new(repo_root);
        seed_dynamic_round_graph_fixture(&app);

        let detail = round_detail_vm(
            &app,
            "task-dynamic-round-graph",
            "run-001",
            "round-001",
            None,
        )
        .unwrap();

        assert!(detail.graph.edges.iter().any(|edge| {
            edge.from == "ai-dynamic1::attempt-001::create-hello-world-py"
                && edge.to == "accept"
                && edge.label == "success"
        }));
        assert!(!detail.graph.edges.iter().any(|edge| {
            edge.from == "ai-dynamic1::attempt-001::bootstrap"
                && edge.to == "accept"
                && edge.label == "success"
        }));
        let dynamic_exit_sequence = detail
            .graph
            .nodes
            .iter()
            .find(|node| node.id == "ai-dynamic1::attempt-001::create-hello-world-py")
            .and_then(|node| node.sequence)
            .unwrap();
        let accept_sequence = detail
            .graph
            .nodes
            .iter()
            .find(|node| node.id == "accept")
            .and_then(|node| node.sequence)
            .unwrap();
        assert!(
            dynamic_exit_sequence < accept_sequence,
            "AI-DYNAMIC exit should rank before the next workflow node"
        );
    }

    #[test]
    fn stale_session_completion_fuse_projects_completion_without_deleting_pid() {
        let dir = std::env::temp_dir().join(format!(
            "gold-band-completion-fuse-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let pid_path = attempt_dir.join("provider.pid");
        let raw_path = attempt_dir.join("acp.raw.jsonl");
        fs::write(pid_path.as_std_path(), "12345").unwrap();
        let mut session = json!({ "availability": "established", "latestTurnStatus": "none" });

        let fused = apply_stale_session_completion_fuse_common(
            &pid_path,
            &raw_path,
            &mut session,
            true,
            false,
        )
        .unwrap();

        assert!(fused);
        assert_eq!(
            session
                .get("latestTurnStatus")
                .and_then(|value| value.as_str()),
            Some("completed")
        );
        assert!(pid_path.exists());

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stale_session_completion_fuse_never_closes_a_live_follow_up_prompt() {
        let dir = std::env::temp_dir().join(format!(
            "gold-band-live-follow-up-fuse-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let pid_path = attempt_dir.join("provider.pid");
        let raw_path = attempt_dir.join("acp.raw.jsonl");
        let mut session = json!({ "availability": "established", "latestTurnStatus": "none", "stopReason": null });

        let fused = apply_stale_session_completion_fuse_common(
            &pid_path,
            &raw_path,
            &mut session,
            true,
            true,
        )
        .unwrap();

        assert!(!fused);
        assert_eq!(session["latestTurnStatus"], "none");
        assert!(session["stopReason"].is_null());

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn live_prompt_activity_overrides_a_previous_terminal_snapshot() {
        assert_eq!(
            effective_acp_session_status("completed", Some(PromptActivity::Starting)),
            "pending"
        );
        assert_eq!(
            effective_acp_session_status("completed", Some(PromptActivity::Running)),
            "running"
        );
        assert_eq!(
            effective_acp_session_status("completed", Some(PromptActivity::CancelRequested)),
            "cancelling"
        );
        assert_eq!(effective_acp_session_status("completed", None), "completed");
    }

    #[test]
    fn session_metadata_status_prioritizes_turn_terminal_over_legacy_closing() {
        assert_eq!(
            session_metadata_status(&json!({
                "availability": "closing",
                "liveTurnActivity": "idle",
                "latestTurnStatus": "cancelled"
            })),
            "cancelled"
        );
        assert_eq!(
            session_metadata_status(&json!({
                "availability": "established",
                "liveTurnActivity": "cancelRequested",
                "latestTurnStatus": "none"
            })),
            "cancelling"
        );
        assert_eq!(
            session_metadata_status(&json!({
                "availability": "closing",
                "liveTurnActivity": "idle",
                "latestTurnStatus": "none"
            })),
            "closing"
        );
    }

    #[test]
    fn preloaded_legacy_stop_is_migrated_to_turn_cancellation() {
        let normalized = normalize_preloaded_session_metadata(json!({
            "sessionId": "provider-session",
            "status": "closing"
        }));
        assert_eq!(normalized["availability"], "established");
        assert_eq!(normalized["liveTurnActivity"], "cancelRequested");
        assert_eq!(normalized["latestTurnStatus"], "none");
        assert!(normalized.get("status").is_none());
    }

    #[test]
    fn stale_session_completion_fuse_keeps_live_incomplete_node_running() {
        let dir =
            std::env::temp_dir().join(format!("gold-band-live-fuse-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let pid_path = attempt_dir.join("provider.pid");
        let raw_path = attempt_dir.join("acp.raw.jsonl");
        fs::write(pid_path.as_std_path(), "12345").unwrap();
        let mut session = json!({ "availability": "established", "latestTurnStatus": "none" });

        let fused = apply_stale_session_completion_fuse_common(
            &pid_path,
            &raw_path,
            &mut session,
            false,
            false,
        )
        .unwrap();

        assert!(!fused);
        assert_eq!(
            session
                .get("latestTurnStatus")
                .and_then(|value| value.as_str()),
            Some("none")
        );
        assert!(pid_path.exists());

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stale_session_completion_fuse_leaves_terminal_session_unchanged() {
        let dir = std::env::temp_dir().join(format!(
            "gold-band-terminal-fuse-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let pid_path = attempt_dir.join("provider.pid");
        let raw_path = attempt_dir.join("acp.raw.jsonl");
        let mut session = json!({ "availability": "restorable", "latestTurnStatus": "failed" });

        let fused = apply_stale_session_completion_fuse_common(
            &pid_path,
            &raw_path,
            &mut session,
            true,
            false,
        )
        .unwrap();

        assert!(!fused);
        assert_eq!(
            session
                .get("latestTurnStatus")
                .and_then(|value| value.as_str()),
            Some("failed")
        );

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stale_session_completion_fuse_does_not_recover_from_raw_log() {
        let dir = std::env::temp_dir().join(format!(
            "gold-band-raw-close-detection-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let raw_path = attempt_dir.join("acp.raw.jsonl");
        fs::write(
            raw_path.as_std_path(),
            [
                r#"{"direction":"outbound","frame":{"jsonrpc":"2.0","id":5,"method":"session/close","params":{"sessionId":"session-1"}}}"#,
                r#"{"direction":"inbound","frame":{"jsonrpc":"2.0","id":5,"result":{}}}"#,
            ]
            .join("\n"),
        )
        .unwrap();

        fs::write(attempt_dir.join("provider.pid"), "12345").unwrap();
        let mut session = json!({ "availability": "established", "latestTurnStatus": "none" });
        let fused = apply_stale_session_completion_fuse_common(
            &attempt_dir.join("provider.pid"),
            &raw_path,
            &mut session,
            false,
            false,
        )
        .unwrap();
        assert!(!fused);
        assert_eq!(session["latestTurnStatus"], "none");

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn system_prompt_append_is_extracted_from_session_resume() {
        let dir = std::env::temp_dir().join(format!(
            "gold-band-resume-system-prompt-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let raw_path = Utf8PathBuf::from_path_buf(dir.clone())
            .unwrap()
            .join("acp.raw.jsonl");
        fs::write(
            raw_path.as_std_path(),
            r#"{"direction":"outbound","frame":{"jsonrpc":"2.0","id":6,"method":"session/resume","params":{"sessionId":"session-1","cwd":"D:/repo","mcpServers":[],"_meta":{"systemPrompt":{"append":"stable context"}}}}}"#,
        )
        .unwrap();

        assert_eq!(
            extract_system_prompt_append(&raw_path).as_deref(),
            Some("stable context")
        );

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn provider_pid_with_running_session_does_not_force_stopping() {
        assert!(!is_acp_session_stopping_status("running"));
    }

    #[test]
    fn explicit_cancelling_session_is_stopping() {
        assert!(is_acp_session_stopping_status("cancelling"));
    }

    #[test]
    fn active_session_timing_prefers_scanned_elapsed_over_stale_snapshot() {
        let snapshot = AcpSessionTimingVm {
            session_elapsed_seconds: 22,
            revision: Some(220),
            observed_at: Some("1782985079Z".to_string()),
            active_turn_started_at: Some("1782985079Z".to_string()),
            active_turn_last_activity_at: Some("1782985079Z".to_string()),
            permission_wait_started_at: None,
            user_wait_started_at: None,
            wait_reason: None,
            paused: false,
        };
        let event_timing = AcpSessionTimingVm {
            session_elapsed_seconds: 22,
            revision: Some(221),
            observed_at: Some("1782985079Z".to_string()),
            active_turn_started_at: Some("1782985079Z".to_string()),
            active_turn_last_activity_at: Some("1782985079Z".to_string()),
            permission_wait_started_at: None,
            user_wait_started_at: None,
            wait_reason: None,
            paused: false,
        };

        let resolved =
            resolve_acp_session_timing("running", Some(snapshot), Some(event_timing), Some(32))
                .unwrap();

        assert_eq!(resolved.session_elapsed_seconds, 32);
        assert_eq!(
            resolved.active_turn_started_at.as_deref(),
            Some("1782985079Z")
        );
    }

    #[test]
    fn terminal_session_timing_keeps_snapshot_as_persisted_truth() {
        let snapshot = AcpSessionTimingVm {
            session_elapsed_seconds: 37,
            revision: Some(348),
            observed_at: Some("1782985094Z".to_string()),
            active_turn_started_at: None,
            active_turn_last_activity_at: None,
            permission_wait_started_at: None,
            user_wait_started_at: None,
            wait_reason: None,
            paused: true,
        };
        let event_timing = AcpSessionTimingVm {
            session_elapsed_seconds: 32,
            revision: Some(221),
            observed_at: Some("1782985079Z".to_string()),
            active_turn_started_at: None,
            active_turn_last_activity_at: None,
            permission_wait_started_at: None,
            user_wait_started_at: None,
            wait_reason: None,
            paused: false,
        };

        let resolved =
            resolve_acp_session_timing("cancelled", Some(snapshot), Some(event_timing), Some(32))
                .unwrap();

        assert_eq!(resolved.session_elapsed_seconds, 37);
        assert!(resolved.paused);
    }

    #[test]
    fn acp_session_config_preserves_options_without_current_values() {
        let config = acp_session_config_vm(&json!({
            "configOptions": [
                {
                    "id": "model",
                    "category": "model",
                    "type": "select",
                    "options": [
                        { "value": "default", "name": "Default" },
                        { "value": "opus", "name": "Opus" }
                    ]
                },
                {
                    "id": "mode",
                    "category": "mode",
                    "type": "select",
                    "options": [
                        { "value": "default", "name": "Default" },
                        { "value": "acceptEdits", "name": "Accept Edits" }
                    ]
                }
            ]
        }))
        .unwrap();

        assert!(config.model_override_id.is_none());
        assert!(config.permission_mode_override_id.is_none());
        assert!(config.current_model_id.is_none());
        assert!(config.current_mode_id.is_none());
        assert_eq!(
            config
                .config_options
                .as_ref()
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(2)
        );
    }

    #[test]
    fn acp_session_config_prefers_config_options_over_conflicting_legacy_state() {
        let config = acp_session_config_vm(&json!({
            "models": {
                "currentModelId": "gpt-5.6-sol[max]",
                "availableModels": [
                    { "modelId": "gpt-5.6-sol[low]", "name": "GPT-5.6-Sol (low)" },
                    { "modelId": "gpt-5.6-sol[max]", "name": "GPT-5.6-Sol (max)" }
                ]
            },
            "modes": {
                "currentModeId": "legacy-mode",
                "availableModes": [
                    { "id": "legacy-mode", "name": "Legacy mode" }
                ]
            },
            "configOptions": [
                {
                    "id": "model",
                    "category": "model",
                    "type": "select",
                    "currentValue": "gpt-5.6-sol",
                    "options": [
                        { "value": "gpt-5.6-sol", "name": "GPT-5.6-Sol" },
                        { "value": "gpt-5.6-terra", "name": "GPT-5.6-Terra" }
                    ]
                },
                {
                    "id": "mode",
                    "category": "mode",
                    "type": "select",
                    "currentValue": "agent",
                    "options": [
                        { "value": "agent", "name": "Agent" }
                    ]
                }
            ]
        }))
        .unwrap();

        assert_eq!(config.current_model_id.as_deref(), Some("gpt-5.6-sol"));
        assert_eq!(config.current_model_name.as_deref(), Some("GPT-5.6-Sol"));
        assert_eq!(config.current_mode_id.as_deref(), Some("agent"));
        assert_eq!(config.current_mode_name.as_deref(), Some("Agent"));
    }

    #[test]
    fn acp_session_config_separates_gold_band_override_from_agent_current_model() {
        let unspecified = acp_session_config_vm(&json!({
            "models": {
                "currentModelId": "default",
                "availableModels": [
                    { "modelId": "default", "name": "Default (recommended)" },
                    { "modelId": "glm-5.2-hs", "name": "GLM 5.2" }
                ]
            }
        }))
        .unwrap();
        assert!(unspecified.model_override_id.is_none());
        assert_eq!(unspecified.current_model_id.as_deref(), Some("default"));

        let explicit = acp_session_config_vm(&json!({
            "modelOverride": "default",
            "models": {
                "currentModelId": "default",
                "availableModels": [
                    { "modelId": "default", "name": "Default (recommended)" }
                ]
            }
        }))
        .unwrap();
        assert_eq!(explicit.model_override_id.as_deref(), Some("default"));
    }

    #[test]
    fn acp_session_config_separates_gold_band_override_from_agent_current_permission_mode() {
        let unspecified = acp_session_config_vm(&json!({
            "modes": {
                "currentModeId": "default",
                "availableModes": [
                    { "id": "default", "name": "Default" },
                    { "id": "bypassPermissions", "name": "Bypass Permissions" }
                ]
            }
        }))
        .unwrap();
        assert!(unspecified.permission_mode_override_id.is_none());
        assert_eq!(unspecified.current_mode_id.as_deref(), Some("default"));

        let explicit = acp_session_config_vm(&json!({
            "permissionModeOverride": "default",
            "modes": {
                "currentModeId": "default",
                "availableModes": [
                    { "id": "default", "name": "Default" }
                ]
            }
        }))
        .unwrap();
        assert_eq!(
            explicit.permission_mode_override_id.as_deref(),
            Some("default")
        );
    }

    #[test]
    fn diagnostics_file_populates_session_diagnostics() {
        let dir = std::env::temp_dir().join(format!("gold-band-diag-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.join("acp.diagnostics.jsonl")).unwrap();
        fs::write(
            path.as_std_path(),
            r#"{"level":"error","message":"Internal error: API Error: Request rejected (429)","timestamp":"1778771541Z"}
"#,
        )
        .unwrap();

        let diagnostics = scan_acp_diagnostics(&path).unwrap();

        assert_eq!(diagnostics.error_count, 1);
        assert_eq!(
            diagnostics.last_error.as_deref(),
            Some("Internal error: API Error: Request rejected (429)")
        );
        assert_eq!(
            diagnostics.last_error_timestamp.as_deref(),
            Some("1778771541Z")
        );

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn session_elapsed_excludes_selected_permission_wait() {
        let elapsed = elapsed_for(
            vec![
                gold_band_prompt_at(100),
                text_event_at(105),
                permission_event_at("permission-1", "pending", 110),
                permission_event_at("permission-1", "selected", 160),
                text_event_at(190),
            ],
            false,
            None,
        );

        assert_eq!(elapsed, Some(40));
    }

    #[test]
    fn session_elapsed_stops_while_permission_is_pending_for_active_turn() {
        let elapsed = elapsed_for(
            vec![
                gold_band_prompt_at(100),
                text_event_at(105),
                permission_event_at("permission-1", "pending", 110),
            ],
            true,
            Some(200),
        );

        assert_eq!(elapsed, Some(10));
    }

    #[test]
    fn session_elapsed_resumes_after_permission_selected() {
        let elapsed = elapsed_for(
            vec![
                gold_band_prompt_at(100),
                permission_event_at("permission-1", "pending", 110),
                permission_event_at("permission-1", "selected", 160),
                text_event_at(170),
            ],
            false,
            None,
        );

        assert_eq!(elapsed, Some(20));
    }

    #[test]
    fn session_elapsed_reconstructs_compacted_permission_wait() {
        let mut selected = permission_event_at("permission-1", "selected", 170);
        selected.started_at = Some("120Z".to_string());
        selected.ended_at = Some("170Z".to_string());
        let elapsed = elapsed_for(
            vec![
                gold_band_prompt_at(100),
                text_event_at(110),
                selected,
                text_event_at(180),
            ],
            false,
            None,
        );

        assert_eq!(elapsed, Some(30));
    }

    #[test]
    fn session_elapsed_excludes_elicitation_wait() {
        let elapsed = elapsed_for(
            vec![
                gold_band_prompt_at(100),
                text_event_at(105),
                elicitation_request_event_at("elicit-1", 110),
                elicitation_response_event_at("elicit-1", 160),
                text_event_at(190),
            ],
            false,
            None,
        );

        assert_eq!(elapsed, Some(40));
    }

    #[test]
    fn session_elapsed_does_not_double_count_overlapping_permission_waits() {
        let elapsed = elapsed_for(
            vec![
                gold_band_prompt_at(100),
                permission_event_at("permission-1", "pending", 110),
                permission_event_at("permission-2", "pending", 120),
                permission_event_at("permission-1", "selected", 150),
                permission_event_at("permission-2", "selected", 170),
                text_event_at(180),
            ],
            false,
            None,
        );

        assert_eq!(elapsed, Some(20));
    }

    #[test]
    fn session_elapsed_ignores_unmatched_permission_selected() {
        let elapsed = elapsed_for(
            vec![
                gold_band_prompt_at(100),
                permission_event_at("permission-1", "selected", 150),
                text_event_at(160),
            ],
            false,
            None,
        );

        assert_eq!(elapsed, Some(60));
    }

    #[test]
    fn session_elapsed_resets_permission_wait_between_prompt_turns() {
        let elapsed = elapsed_for(
            vec![
                gold_band_prompt_at(100),
                permission_event_at("permission-1", "pending", 110),
                text_event_at(130),
                gold_band_prompt_at(200),
                text_event_at(230),
            ],
            false,
            None,
        );

        assert_eq!(elapsed, Some(40));
    }

    #[test]
    fn session_elapsed_excludes_idle_resume_gaps_between_prompt_turns() {
        let elapsed = elapsed_for(
            vec![
                gold_band_prompt_at(1_782_903_916),
                text_event_at(1_782_903_917),
                gold_band_prompt_at(1_782_904_743),
                text_event_at(1_782_904_746),
                gold_band_prompt_at(1_782_905_348),
                text_event_at(1_782_905_355),
                gold_band_prompt_at(1_782_905_444),
                text_event_at(1_782_905_448),
                metadata_update_at(
                    "availableCommands",
                    "available_commands_update",
                    1_782_906_094,
                ),
                metadata_update_at("modeUpdate", "current_mode_update", 1_782_906_094),
                gold_band_prompt_at(1_782_906_094),
                text_event_at(1_782_906_106),
                gold_band_prompt_at(1_782_906_114),
                text_event_at(1_782_906_115),
                gold_band_prompt_at(1_782_906_120),
                text_event_at(1_782_906_121),
                metadata_update_at(
                    "availableCommands",
                    "available_commands_update",
                    1_782_907_082,
                ),
                metadata_update_at("modeUpdate", "current_mode_update", 1_782_907_082),
                gold_band_prompt_at(1_782_907_082),
                text_event_at(1_782_907_085),
                metadata_update_at(
                    "availableCommands",
                    "available_commands_update",
                    1_782_907_091,
                ),
                metadata_update_at("modeUpdate", "current_mode_update", 1_782_907_091),
                gold_band_prompt_at(1_782_907_091),
                text_event_at(1_782_907_091),
            ],
            false,
            None,
        );

        assert_eq!(elapsed, Some(32));
    }

    #[test]
    fn session_elapsed_excludes_plan_intervention_permission_wait() {
        let elapsed = elapsed_for(
            vec![
                gold_band_prompt_at(100),
                multi_option_permission_event_at("permission-1", "pending", 110),
                multi_option_permission_event_at("permission-1", "selected", 160),
                text_event_at(180),
            ],
            false,
            None,
        );

        assert_eq!(elapsed, Some(30));
    }

    #[test]
    fn permission_vm_uses_raw_request_id_over_timeline_display_id() {
        let event = acp_event_at(
            "permission-0",
            "permissionRequest",
            Some("pending"),
            110,
            Some(json!({
                "requestId": "0",
                "_meta": { "goldBandConversation": {
                    "turnId": "turn-2",
                    "promptEventId": "prompt-2"
                }},
                "options": [
                    { "optionId": "allow", "name": "Allow", "kind": "allow_once" }
                ]
            })),
        );

        let vm = permission_vm_from_event(&event);

        let AcpPromptInteractionVm::Permission {
            interaction_id,
            turn_id,
            prompt_event_id,
            raw,
            ..
        } = vm
        else {
            panic!("expected permission interaction");
        };
        assert_eq!(interaction_id, "0");
        assert_eq!(turn_id.as_deref(), Some("turn-2"));
        assert_eq!(prompt_event_id.as_deref(), Some("prompt-2"));
        assert_eq!(
            raw.get("requestId").and_then(|value| value.as_str()),
            Some("0")
        );
    }

    #[test]
    fn legacy_permission_display_id_falls_back_to_original_request_id() {
        let event = acp_event_at(
            "permission-permission-0",
            "permissionRequest",
            Some("pending"),
            110,
            Some(json!({
                "options": [
                    { "optionId": "allow", "name": "Allow", "kind": "allow_once" }
                ]
            })),
        );

        assert_eq!(permission_request_id_from_event(&event), "0");
        let AcpPromptInteractionVm::Permission { interaction_id, .. } =
            permission_vm_from_event(&event)
        else {
            panic!("expected permission interaction");
        };
        assert_eq!(interaction_id, "0");
    }

    #[test]
    fn timeline_permission_decision_replaces_pending_by_request_id() {
        let dir = std::env::temp_dir().join(format!("gb-tl-permission-id-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let db = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let path = write_timeline_file(
            &db,
            "acp.timeline.jsonl",
            &[
                acp_event_at(
                    "permission-0",
                    "permissionRequest",
                    Some("pending"),
                    110,
                    Some(json!({
                        "requestId": "0",
                        "options": [
                            { "optionId": "allow", "name": "Allow", "kind": "allow_once" }
                        ]
                    })),
                ),
                acp_event_at(
                    "permission-permission-0",
                    "permissionRequest",
                    Some("selected"),
                    160,
                    Some(json!({ "requestId": "permission-0", "optionId": "allow" })),
                ),
            ],
        );

        let (_, _, _, latest_permissions, _, _) = parse_timeline_file(&path, true).unwrap();

        assert_eq!(latest_permissions.len(), 1);
        assert_eq!(
            latest_permissions
                .get("0")
                .and_then(|event| event.status.as_deref()),
            Some("selected")
        );

        fs::remove_dir_all(dir).unwrap();
    }

    // --- timeline / events parse & cache tests ---

    fn write_timeline_file(dir: &Utf8PathBuf, name: &str, events: &[AcpUiEventVm]) -> Utf8PathBuf {
        let path = dir.join(name);
        let mut content = String::new();
        for event in events {
            let item = AcpTimelineItemVm {
                item: event.clone(),
            };
            content.push_str(&serde_json::to_string(&item).unwrap());
            content.push('\n');
        }
        fs::write(path.as_std_path(), &content).unwrap();
        path
    }

    fn write_timeline_patch_file(
        dir: &Utf8PathBuf,
        name: &str,
        patches: &[(u64, &str, AcpUiEventVm)],
    ) -> Utf8PathBuf {
        let path = dir.join(name);
        let mut content = String::new();
        for (revision, item_id, item) in patches {
            content.push_str(
                &serde_json::to_string(&json!({
                    "patchType": "timelinePatch",
                    "itemId": item_id,
                    "revision": revision,
                    "op": "upsert",
                    "item": item,
                }))
                .unwrap(),
            );
            content.push('\n');
        }
        fs::write(path.as_std_path(), &content).unwrap();
        path
    }

    fn event_sequence(count: usize, base_ts: u64) -> Vec<AcpUiEventVm> {
        (0..count)
            .map(|i| AcpUiEventVm {
                id: format!("evt-{i}"),
                seq: i as u64 + 1,
                timestamp: format!("{}Z", base_ts + i as u64),
                kind: "textDelta".to_string(),
                session_id: Some("s1".to_string()),
                content: Some(format!("message {i}")),
                title: None,
                tool_call_id: None,
                status: Some("completed".to_string()),
                started_seq: Some(i as u64 + 1),
                ended_seq: Some(i as u64 + 1),
                started_at: Some(format!("{}Z", base_ts + i as u64)),
                ended_at: Some(format!("{}Z", base_ts + i as u64)),
                timing: None,
                raw: None,
            })
            .collect()
    }

    #[test]
    fn parse_timeline_file_all_events() {
        let dir = std::env::temp_dir().join(format!("gb-tl-parse-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let db = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let events = event_sequence(50, 1000);
        let path = write_timeline_file(&db, "acp.timeline.jsonl", &events);

        let (all_events, count, _, _, _, _) = parse_timeline_file(&path, false).unwrap();

        assert_eq!(all_events.len(), 50);
        assert_eq!(count, 50);
        assert_eq!(all_events[0].content.as_deref(), Some("message 0"));
        assert_eq!(all_events[49].content.as_deref(), Some("message 49"));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn parse_timeline_keeps_persisted_file_change_set_at_turn_end() {
        let dir =
            std::env::temp_dir().join(format!("gb-tl-file-change-position-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let db = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let mut events = event_sequence(3, 1_000);
        events[0].id = "prompt".to_string();
        events[0].kind = "userTextDelta".to_string();
        events[1].id = "tool".to_string();
        events[1].kind = "toolCall".to_string();
        events[2].id = "changes".to_string();
        events[2].kind = "fileChangeSet".to_string();
        events[2].started_seq = Some(events[0].seq);
        events[2].started_at = events[0].started_at.clone();
        let expected_change_seq = events[2].seq;
        let path = write_timeline_file(&db, "acp.timeline.jsonl", &events);

        let (all_events, _, _, _, _, _) = parse_timeline_file(&path, false).unwrap();

        assert_eq!(
            all_events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            vec!["prompt", "tool", "changes"]
        );
        assert_eq!(all_events[2].started_seq, Some(expected_change_seq));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reload_page_keeps_file_change_set_appended_after_final_answer() {
        let dir =
            std::env::temp_dir().join(format!("gb-tl-file-change-page-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let attempt = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let mut events = event_sequence(4, 1_000);
        events[0].id = "prompt".to_string();
        events[0].kind = "userTextDelta".to_string();
        events[1].id = "tool".to_string();
        events[1].kind = "toolCall".to_string();
        events[1].tool_call_id = Some("call-1".to_string());
        events[2].id = "answer".to_string();
        events[3].id = "changes".to_string();
        events[3].kind = "fileChangeSet".to_string();
        events[3].raw = Some(json!({
            "changeSetId": "turn-files-1",
            "summary": {
                "fileCount": 1,
                "addedFiles": 0,
                "modifiedFiles": 1,
                "deletedFiles": 0,
                "addedLines": 1,
                "deletedLines": 1
            }
        }));
        let path = write_timeline_file(&attempt, "acp.timeline.jsonl", &events);

        let page = scan_acp_timeline(&path, None, false, 30).unwrap();

        assert_eq!(page.event_page.total, 4);
        assert_eq!(
            page.events.last().map(|event| event.id.as_str()),
            Some("changes")
        );
        assert_eq!(
            page.events.last().map(|event| event.kind.as_str()),
            Some("fileChangeSet")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn tool_detail_keeps_intermediate_diff_when_terminal_revision_has_only_status() {
        let dir = std::env::temp_dir().join(format!(
            "gb-tool-detail-diff-revision-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        let attempt = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let mut intermediate = event_sequence(1, 1_000).remove(0);
        intermediate.id = "tool-call-call-1".to_string();
        intermediate.kind = "toolCall".to_string();
        intermediate.tool_call_id = Some("call-1".to_string());
        intermediate.status = None;
        intermediate.raw = Some(json!({
            "rawInput": { "path": "report.md" },
            "content": [{
                "type": "diff",
                "path": "report.md",
                "oldText": "before",
                "newText": "after"
            }]
        }));
        let mut terminal = intermediate.clone();
        terminal.seq = 2;
        terminal.ended_seq = Some(2);
        terminal.status = Some("completed".to_string());
        terminal.raw = Some(json!({ "rawOutput": "done" }));
        write_timeline_patch_file(
            &attempt,
            "acp.timeline.jsonl",
            &[
                (1, "tool-call-call-1", intermediate),
                (2, "tool-call-call-1", terminal),
            ],
        );

        let detail = acp_tool_detail_vm_for_attempt(
            &attempt,
            AcpToolDetailQueryInput {
                branch_id: gold_band::acp::branches::ROOT_BRANCH_ID.to_string(),
                event_id: "tool-call-call-1".to_string(),
                tool_call_id: Some("call-1".to_string()),
            },
        )
        .unwrap()
        .event
        .expect("tool detail");

        assert_eq!(detail.status.as_deref(), Some("completed"));
        assert_eq!(detail.raw.as_ref().unwrap()["content"][0]["type"], "diff");
        assert_eq!(detail.raw.as_ref().unwrap()["rawOutput"], "done");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn parse_timeline_file_uses_latest_patch_for_stable_stream_item() {
        let dir = std::env::temp_dir().join(format!("gb-tl-patch-latest-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let db = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let mut first = text_event_at(1_000);
        first.id = "assistant-message-1".to_string();
        first.content = Some("partial".to_string());
        first.started_seq = Some(10);
        first.ended_seq = Some(10);
        let mut latest = first.clone();
        latest.content = Some("partial complete".to_string());
        latest.seq = 20;
        latest.timestamp = "1020Z".to_string();
        latest.ended_seq = Some(20);
        latest.ended_at = Some("1020Z".to_string());
        let path = write_timeline_patch_file(
            &db,
            "acp.timeline.jsonl",
            &[
                (1, "assistant-message-1", first),
                (2, "assistant-message-1", latest),
            ],
        );

        let (events, count, _, _, _, _) = parse_timeline_file(&path, true).unwrap();

        assert_eq!(count, 2);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, "assistant-message-1");
        assert_eq!(events[0].content.as_deref(), Some("partial complete"));
        assert_eq!(events[0].ended_seq, Some(20));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn session_timeline_projection_hides_provider_echoes_and_keeps_replay_position() {
        let dir =
            std::env::temp_dir().join(format!("gb-tl-replay-projection-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let db = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();

        let mut prompt = gold_band_prompt_at(1_000);
        prompt.id = "gold-band-user-prompt-1".to_string();
        prompt.seq = 1;
        prompt.started_seq = Some(1);
        prompt.ended_seq = Some(1);

        let mut provider_echo = acp_event_at(
            "provider-echo",
            "userTextDelta",
            Some("completed"),
            1_001,
            Some(json!({ "sessionUpdate": "user_message_chunk" })),
        );
        provider_echo.content = Some("hi".to_string());
        provider_echo.seq = 2;
        provider_echo.started_seq = Some(2);
        provider_echo.ended_seq = Some(2);

        let mut interrupted = provider_echo.clone();
        interrupted.id = "provider-interrupted".to_string();
        interrupted.content = Some("[Request interrupted by user]".to_string());
        interrupted.seq = 3;
        interrupted.started_seq = Some(3);
        interrupted.ended_seq = Some(3);

        let mut external = provider_echo.clone();
        external.id = "provider-user-external".to_string();
        external.content = Some("external question".to_string());
        external.seq = 4;
        external.started_seq = Some(4);
        external.ended_seq = Some(4);
        external.raw = Some(json!({
            "source": "providerHistory",
            "historyOrigin": "external",
            "sessionUpdate": "user_message_chunk",
            "messageId": "external-user"
        }));

        let mut original = acp_event_at(
            "assistant-message-answer-1",
            "textDelta",
            Some("completed"),
            1_002,
            Some(json!({
                "sessionUpdate": "agent_message_chunk",
                "messageId": "answer-1"
            })),
        );
        original.content = Some("answer".to_string());
        original.seq = 5;
        original.started_seq = Some(5);
        original.ended_seq = Some(5);
        original.started_at = Some("1002Z".to_string());
        original.ended_at = Some("1002Z".to_string());

        let mut replayed = original.clone();
        replayed.seq = 100;
        replayed.timestamp = "1100Z".to_string();
        replayed.started_seq = Some(100);
        replayed.ended_seq = Some(100);
        replayed.started_at = Some("1100Z".to_string());
        replayed.ended_at = Some("1100Z".to_string());

        let path = write_timeline_patch_file(
            &db,
            "acp.timeline.jsonl",
            &[
                (1, "gold-band-user-prompt-1", prompt),
                (2, "provider-echo", provider_echo),
                (3, "provider-interrupted", interrupted),
                (4, "provider-user-external", external),
                (5, "assistant-message-answer-1", original),
                (100, "assistant-message-answer-1", replayed),
            ],
        );

        let scan = scan_acp_timeline(&path, None, false, 30).unwrap();

        assert_eq!(
            scan.events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "gold-band-user-prompt-1",
                "provider-user-external",
                "assistant-message-answer-1"
            ]
        );
        assert_eq!(scan.events[2].seq, 5);
        assert_eq!(scan.events[2].started_seq, Some(5));
        assert_eq!(scan.events[2].timestamp, "1002Z");

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn provider_history_projection_inserts_external_turn_before_prompt_anchor() {
        let mut first_prompt = gold_band_prompt_at(1_000);
        first_prompt.id = "gold-band-user-prompt-1".to_string();
        first_prompt.seq = 1;
        first_prompt.started_seq = Some(1);
        first_prompt.ended_seq = Some(1);
        first_prompt.content = Some("hi".to_string());
        first_prompt.raw = Some(json!({
            "source": "goldBandPrompt",
            "promptId": "prompt-1"
        }));

        let mut first_answer = text_event_at(1_001);
        first_answer.id = "assistant-message-first".to_string();
        first_answer.seq = 2;
        first_answer.started_seq = Some(2);
        first_answer.ended_seq = Some(2);

        let mut ask_prompt = gold_band_prompt_at(1_029);
        ask_prompt.id = "gold-band-user-prompt-2".to_string();
        ask_prompt.seq = 29;
        ask_prompt.started_seq = Some(29);
        ask_prompt.ended_seq = Some(29);
        ask_prompt.content = Some("用askUserQuestion工具随便问几个问题给我".to_string());
        ask_prompt.raw = Some(json!({
            "source": "goldBandPrompt",
            "promptId": "prompt-2"
        }));

        let mut ask_tool = acp_event_at(
            "tool-call-ask",
            "toolCall",
            Some("completed"),
            1_030,
            Some(json!({ "toolCallId": "ask" })),
        );
        ask_tool.seq = 30;
        ask_tool.started_seq = Some(30);
        ask_tool.ended_seq = Some(30);

        let placement = json!({
            "version": 1,
            "afterPromptId": "prompt-1",
            "beforePromptId": "prompt-2",
            "gapTurnIndex": 1
        });
        let mut external_user = acp_event_at(
            "provider-user-external",
            "userTextDelta",
            Some("completed"),
            1_101,
            Some(json!({
                "source": "providerHistory",
                "historyProvider": "claude-acp",
                "historyItemIndex": 1,
                "historyPlacement": placement
            })),
        );
        external_user.seq = 101;
        external_user.started_seq = Some(101);
        external_user.ended_seq = Some(101);
        external_user.content = Some("这是我追加的信息".to_string());

        let mut external_answer = text_event_at(1_102);
        external_answer.id = "assistant-message-external".to_string();
        external_answer.seq = 102;
        external_answer.started_seq = Some(102);
        external_answer.ended_seq = Some(102);
        external_answer.raw = Some(json!({
            "source": "providerHistory",
            "historyProvider": "claude-acp",
            "historyItemIndex": 2,
            "historyPlacement": {
                "version": 1,
                "afterPromptId": "prompt-1",
                "beforePromptId": "prompt-2",
                "gapTurnIndex": 1
            }
        }));

        let mut events = vec![
            first_prompt,
            first_answer,
            ask_prompt,
            ask_tool,
            external_user,
            external_answer,
        ];
        order_provider_history_by_prompt_anchors_vm(&mut events);

        assert_eq!(
            events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "gold-band-user-prompt-1",
                "assistant-message-first",
                "provider-user-external",
                "assistant-message-external",
                "gold-band-user-prompt-2",
                "tool-call-ask",
            ]
        );
    }

    #[test]
    fn session_timeline_projection_repairs_reclassified_local_tool_turn() {
        let dir = std::env::temp_dir().join(format!(
            "gb-tl-reclassified-local-turn-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let db = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();

        let mut prompts = ["hi", "hi", "用askUserQuestion工具随便问几个问题给我"]
            .into_iter()
            .enumerate()
            .map(|(index, content)| {
                let mut prompt = gold_band_prompt_at(1_000 + index as u64);
                prompt.id = format!("gold-band-user-prompt-{index}");
                prompt.seq = index as u64 + 1;
                prompt.started_seq = Some(prompt.seq);
                prompt.ended_seq = Some(prompt.seq);
                prompt.content = Some(content.to_string());
                prompt
            })
            .collect::<Vec<_>>();

        let mut original_tool = acp_event_at(
            "tool-call-ask",
            "toolCall",
            Some("completed"),
            1_004,
            Some(json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "ask",
                "rawInput": { "questions": [{ "question": "Question" }] }
            })),
        );
        original_tool.seq = 4;
        original_tool.started_seq = Some(4);
        original_tool.ended_seq = Some(4);
        original_tool.title = Some("Asking for your input".to_string());
        original_tool.tool_call_id = Some("ask".to_string());

        let mut external = acp_event_at(
            "provider-user-external",
            "userTextDelta",
            Some("completed"),
            1_010,
            Some(json!({
                "source": "providerHistory",
                "historyProvider": "claude-acp",
                "historyTurnIndex": 2,
                "sessionUpdate": "user_message_chunk"
            })),
        );
        external.seq = 10;
        external.started_seq = Some(10);
        external.ended_seq = Some(10);
        external.content = Some("这是我追加的信息".to_string());

        let mut stale_ask = external.clone();
        stale_ask.id = "provider-user-ask".to_string();
        stale_ask.seq = 11;
        stale_ask.started_seq = Some(11);
        stale_ask.ended_seq = Some(11);
        stale_ask.content = Some("用askUserQuestion工具随便问几个问题给我".to_string());
        stale_ask.raw = Some(json!({
            "source": "providerHistory",
            "historyProvider": "claude-acp",
            "historyTurnIndex": 3,
            "sessionUpdate": "user_message_chunk"
        }));

        let mut replayed_tool = original_tool.clone();
        replayed_tool.seq = 12;
        replayed_tool.started_seq = Some(12);
        replayed_tool.ended_seq = Some(12);
        replayed_tool.raw = Some(json!({
            "source": "providerHistory",
            "historyProvider": "claude-acp",
            "historyTurnIndex": 3,
            "sessionUpdate": "tool_call_update",
            "toolCallId": "ask",
            "rawOutput": "answered"
        }));

        let mut stale_answer = text_event_at(1_013);
        stale_answer.id = "provider-answer-ask".to_string();
        stale_answer.seq = 13;
        stale_answer.started_seq = Some(13);
        stale_answer.ended_seq = Some(13);
        stale_answer.content = Some("answered".to_string());
        stale_answer.raw = Some(json!({
            "source": "providerHistory",
            "historyProvider": "claude-acp",
            "historyTurnIndex": 3,
            "sessionUpdate": "agent_message_chunk"
        }));

        let mut patches = prompts
            .drain(..)
            .enumerate()
            .map(|(index, prompt)| (index as u64 + 1, prompt.id.clone(), prompt))
            .collect::<Vec<_>>();
        patches.extend([
            (4, original_tool.id.clone(), original_tool.clone()),
            (10, external.id.clone(), external.clone()),
            (11, stale_ask.id.clone(), stale_ask.clone()),
            (12, replayed_tool.id.clone(), replayed_tool),
            (13, stale_answer.id.clone(), stale_answer.clone()),
        ]);
        let borrowed = patches
            .iter()
            .map(|(revision, id, event)| (*revision, id.as_str(), event.clone()))
            .collect::<Vec<_>>();
        let path = write_timeline_patch_file(&db, "acp.timeline.jsonl", &borrowed);

        let (canonical, _, _, _, _, _) = parse_timeline_file(&path, false).unwrap();
        let scan = scan_acp_timeline(&path, None, false, 30).unwrap();

        assert!(scan.events.iter().any(|event| event.id == external.id));
        assert!(!scan.events.iter().any(|event| event.id == stale_ask.id));
        assert!(!scan.events.iter().any(|event| event.id == stale_answer.id));
        let tool = canonical
            .iter()
            .find(|event| event.id == original_tool.id)
            .unwrap();
        assert_eq!(tool.raw, original_tool.raw);
        assert_eq!(tool.seq, original_tool.seq);

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn parse_timeline_elapsed_ignores_metadata_updates_before_next_prompt() {
        let dir = std::env::temp_dir().join(format!("gb-tl-elapsed-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let db = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let events = [
            gold_band_prompt_at(1_782_903_916),
            text_event_at(1_782_903_917),
            metadata_update_at("modeUpdate", "current_mode_update", 1_782_904_743),
            gold_band_prompt_at(1_782_904_743),
            text_event_at(1_782_904_746),
            metadata_update_at(
                "availableCommands",
                "available_commands_update",
                1_782_905_348,
            ),
            metadata_update_at("modeUpdate", "current_mode_update", 1_782_905_348),
            gold_band_prompt_at(1_782_905_348),
            text_event_at(1_782_905_355),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, mut event)| {
            event.seq = index as u64 + 1;
            event
        })
        .collect::<Vec<_>>();
        let path = write_timeline_file(&db, "acp.timeline.jsonl", &events);

        let (_, _, elapsed, _, _, _) = parse_timeline_file(&path, false).unwrap();

        assert_eq!(elapsed, Some(11));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn session_worktree_projection_uses_the_current_physical_workspace() {
        let outer_path = Utf8PathBuf::from("C:/GoldBand/worktrees/outer");
        let child_path = Utf8PathBuf::from("D:/repo/.gold-band/worktrees/child");
        let run_worktree = gold_band::runtime::RunWorktreeState {
            path: outer_path.clone(),
            branch: "gb-conversation-outer".to_string(),
            fork_commit: "abc123".to_string(),
        };
        let workspace = |id: &str, kind, path: Utf8PathBuf| gold_band::dynamic::WorkspaceState {
            version: VERSION.to_string(),
            id: id.to_string(),
            dynamic_run_id: "dynamic-run-001".to_string(),
            kind,
            ownership: match kind {
                WorkspaceKind::Main => gold_band::dynamic::WorkspaceOwnership::User,
                WorkspaceKind::Worktree => gold_band::dynamic::WorkspaceOwnership::Runtime,
            },
            repo_root: Utf8PathBuf::from("D:/repo"),
            path,
            branch: (kind == WorkspaceKind::Worktree).then(|| "gb-dynamic-child".to_string()),
            parent_workspace_id: (kind == WorkspaceKind::Worktree)
                .then(|| "workspace-main".to_string()),
            created_by_group_id: (kind == WorkspaceKind::Worktree).then(|| "group-001".to_string()),
            fork_commit: "abc123".to_string(),
            checkpoint_commit: None,
            status: gold_band::dynamic::WorkspaceStatus::Active,
            created_at: "t0".to_string(),
            updated_at: "t0".to_string(),
        };

        assert_eq!(
            workspace_worktree_projection(
                Some(&run_worktree),
                &workspace(
                    "workspace-child",
                    WorkspaceKind::Worktree,
                    child_path.clone()
                ),
            ),
            Some(SessionWorktreeProjection {
                path: child_path.to_string(),
                branch: Some("gb-dynamic-child".to_string()),
            }),
        );
        assert_eq!(
            workspace_worktree_projection(
                Some(&run_worktree),
                &workspace("workspace-main", WorkspaceKind::Main, outer_path.clone()),
            ),
            Some(SessionWorktreeProjection {
                path: outer_path.to_string(),
                branch: Some("gb-conversation-outer".to_string()),
            }),
        );
        assert_eq!(
            workspace_worktree_projection(
                Some(&run_worktree),
                &workspace(
                    "workspace-main",
                    WorkspaceKind::Main,
                    Utf8PathBuf::from("D:/repo")
                ),
            ),
            None,
        );
        assert_eq!(
            workspace_worktree_projection(
                None,
                &workspace(
                    "workspace-main",
                    WorkspaceKind::Main,
                    Utf8PathBuf::from("D:/repo")
                ),
            ),
            None,
        );
        assert_eq!(
            workspace_worktree_projection_by_id(
                Some(&run_worktree),
                &[
                    workspace("workspace-main", WorkspaceKind::Main, outer_path.clone()),
                    workspace(
                        "workspace-child",
                        WorkspaceKind::Worktree,
                        child_path.clone()
                    ),
                ],
                "workspace-child",
            ),
            Some(SessionWorktreeProjection {
                path: child_path.to_string(),
                branch: Some("gb-dynamic-child".to_string()),
            }),
        );
        let mut released_child = workspace(
            "workspace-child",
            WorkspaceKind::Worktree,
            child_path.clone(),
        );
        released_child.status = gold_band::dynamic::WorkspaceStatus::Released;
        assert_eq!(
            workspace_worktree_projection(Some(&run_worktree), &released_child),
            None,
        );
        assert_eq!(
            session_worktree_projection(Some(&run_worktree), None, Some("missing-dynamic-node")),
            None,
        );
    }

    #[test]
    fn acp_session_vm_ignores_unavailable_runtime_control_placeholder() {
        let dir = tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap());
        let snapshot_path = app.paths.acp_snapshot_file(
            "task-placeholder",
            "run-001",
            "round-001",
            "direct-agent",
            "attempt-001",
        );
        write_json(
            &app.paths.node_file(
                "task-placeholder",
                "run-001",
                "round-001",
                "direct-agent",
                "attempt-001",
            ),
            &NodeState {
                version: gold_band::domain::VERSION.to_string(),
                acp_storage_schema_version: gold_band::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
                node_id: "direct-agent".to_string(),
                node_type: NodeType::Worker,
                run_id: "run-001".to_string(),
                round_id: "round-001".to_string(),
                attempt_id: "attempt-001".to_string(),
                status: RunStatus::Paused,
                outcome: None,
                started_at: "1787036945Z".to_string(),
                finished_at: None,
                manual_check_pending: false,
                runtime_execution_id: None,
                resolved_config: gold_band::domain::ResolvedConfig::new(),
                uuid: None,
            },
        )
        .unwrap();
        write_json(
            &snapshot_path,
            &json!({
                "availability": "unavailable",
                "latestTurnStatus": "none",
                "restored": false,
                "createdAt": "1787036946Z",
                "runtimeControlTimelineScanComplete": true
            }),
        )
        .unwrap();

        let session = acp_session_vm(
            &app,
            "task-placeholder",
            "run-001",
            "round-001",
            "direct-agent",
            "attempt-001",
            None,
            None,
        )
        .unwrap();

        assert!(session.is_none(), "unexpected ACP session VM: {session:#?}");
    }

    #[test]
    fn acp_session_vm_preserves_newer_prompt_interactions_when_session_status_is_terminal() {
        let dir = tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap());
        let node_path = app.paths.node_file(
            "task-permission",
            "run-001",
            "round-001",
            "direct-agent",
            "attempt-001",
        );
        write_json(
            &node_path,
            &NodeState {
                version: gold_band::domain::VERSION.to_string(),
                acp_storage_schema_version: gold_band::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
                node_id: "direct-agent".to_string(),
                node_type: NodeType::Worker,
                run_id: "run-001".to_string(),
                round_id: "round-001".to_string(),
                attempt_id: "attempt-001".to_string(),
                status: RunStatus::Completed,
                outcome: Some(gold_band::domain::NodeOutcome::Success),
                started_at: "1787036945Z".to_string(),
                finished_at: Some("1787036947Z".to_string()),
                manual_check_pending: false,
                runtime_execution_id: None,
                resolved_config: gold_band::domain::ResolvedConfig::new(),
                uuid: None,
            },
        )
        .unwrap();
        write_json(
            &app.paths.acp_snapshot_file(
                "task-permission",
                "run-001",
                "round-001",
                "direct-agent",
                "attempt-001",
            ),
            &json!({
                "sessionId": "session-1",
                "status": "completed",
                "latestTurnStatus": "completed",
                "restored": false,
                "createdAt": "1787036946Z"
            }),
        )
        .unwrap();
        let attempt_dir = node_path.parent().unwrap().to_path_buf();
        let mut permission = gold_band::acp::events::permission_request_event(
            3,
            "request-turn-2".to_string(),
            json!({
                "sessionId": "session-1",
                "_meta": { "goldBandConversation": {
                    "branchId": "root",
                    "turnId": "turn-2",
                    "promptEventId": "prompt-turn-2"
                }},
                "options": [{ "optionId": "allow", "name": "Allow", "kind": "allow_once" }]
            }),
        );
        permission.id = "permission-request-turn-2".to_string();
        let elicitation_request = serde_json::from_value(json!({
            "mode": "form",
            "sessionId": "session-1",
            "message": "Choose",
            "requestedSchema": { "type": "object", "properties": {} }
        }))
        .unwrap();
        let mut elicitation = gold_band::acp::events::elicitation_request_event(
            4,
            "elicit-turn-2".to_string(),
            &elicitation_request,
        );
        gold_band::acp::branches::annotate_event_branch(&mut elicitation);
        gold_band::acp::interaction::annotate_prompt_interaction_identity(
            &mut elicitation,
            &gold_band::acp::interaction::AcpPromptInteractionIdentity::new(
                "elicit-turn-2",
                gold_band::acp::interaction::AcpPromptInteractionKind::Elicitation,
                "turn-2",
                "prompt-turn-2",
            ),
        );
        gold_band::acp::events::write_timeline_items(
            &gold_band::acp::branches::branch_timeline_path(&attempt_dir, "root"),
            &[permission, elicitation],
        )
        .unwrap();

        let session = acp_session_vm(
            &app,
            "task-permission",
            "run-001",
            "round-001",
            "direct-agent",
            "attempt-001",
            None,
            None,
        )
        .unwrap()
        .unwrap();

        assert_eq!(session.pending_interactions.len(), 2);
        let AcpPromptInteractionVm::Permission {
            interaction_id,
            turn_id,
            prompt_event_id,
            ..
        } = &session.pending_interactions[0]
        else {
            panic!("expected permission interaction");
        };
        assert_eq!(interaction_id, "request-turn-2");
        assert_eq!(turn_id.as_deref(), Some("turn-2"));
        assert_eq!(prompt_event_id.as_deref(), Some("prompt-turn-2"));
        let elicitation = session
            .pending_interactions
            .iter()
            .find(|interaction| matches!(interaction, AcpPromptInteractionVm::Elicitation { .. }))
            .expect("expected elicitation interaction");
        let AcpPromptInteractionVm::Elicitation {
            interaction_id,
            turn_id,
            prompt_event_id,
            ..
        } = elicitation
        else {
            unreachable!();
        };
        assert_eq!(interaction_id, "elicit-turn-2");
        assert_eq!(turn_id.as_deref(), Some("turn-2"));
        assert_eq!(prompt_event_id.as_deref(), Some("prompt-turn-2"));
    }

    #[test]
    fn dynamic_acp_session_vm_keeps_attempt_cwd_separate_from_provider_cwd() {
        let dir =
            std::env::temp_dir().join(format!("gb-dynamic-session-cwd-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(dir.clone()).unwrap());
        let attempt_dir = app.paths.dynamic_node_attempt_dir(
            "task-081",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
            "goodbye-output",
            "attempt-001",
        );
        let workspace_cwd = "D:\\Projects\\code\\ai\\Gold-Band";
        gold_band::storage::write_json(
            &app.paths.dynamic_node_file(
                "task-081",
                "run-001",
                "round-001",
                "ai-dynamic",
                "attempt-001",
                "goodbye-output",
            ),
            &json!({
                "version": gold_band::domain::VERSION,
                "acpStorageSchemaVersion": gold_band::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
                "id": "goodbye-output",
                "dynamicRunId": "dynamic-run-001",
                "kind": "worker",
                "title": "Goodbye output",
                "task": "Return the final output",
                "status": "completed",
                "outcome": "success",
                "chainId": "goodbye-output",
                "depth": 0,
                "dependsOn": [],
                "workspaceId": "workspace-main",
                "provider": "codex-acp",
                "sessionMode": "continue",
                "startedAt": "1778771540Z",
                "finishedAt": "1778771541Z"
            }),
        )
        .unwrap();
        gold_band::storage::write_json(
            &attempt_dir.join("acp.snapshot.json"),
            &json!({
                "adapterId": "npx",
                "adapterDisplayName": "Codex",
                "cwd": attempt_dir.to_string(),
                "status": "completed",
                "restored": true
            }),
        )
        .unwrap();
        gold_band::storage::write_json(
            &attempt_dir.join("worker-ref.json"),
            &json!({
                "version": "0.1",
                "provider": "codex-acp",
                "mode": "continue",
                "supports_open_session": true,
                "supports_continue_session": true,
                "continue_ref": {
                    "acpSessionId": "session-continue-1",
                    "adapterId": "npx",
                    "adapterDisplayName": "Codex",
                    "cwd": workspace_cwd
                },
                "open_command": null
            }),
        )
        .unwrap();

        let session = dynamic_acp_session_vm(
            &app,
            "task-081",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
            "goodbye-output",
            "attempt-001",
            None,
            None,
        )
        .unwrap()
        .unwrap();

        assert_eq!(session.cwd.as_deref(), Some(attempt_dir.as_str()));
        assert_eq!(session.provider_cwd.as_deref(), Some(workspace_cwd));
        assert_eq!(session.round_id, "round-001");
        assert_eq!(session.node_id, "goodbye-output");
        assert_eq!(session.attempt_id, "attempt-001");
        assert_eq!(session.outer_node_id.as_deref(), Some("ai-dynamic"));
        assert_eq!(session.outer_attempt_id.as_deref(), Some("attempt-001"));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn scan_timeline_cache_hit_on_repeat() {
        let dir = std::env::temp_dir().join(format!("gb-tl-hit-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let db = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let path = write_timeline_file(&db, "acp.timeline.jsonl", &event_sequence(20, 2000));

        let r1 = scan_acp_timeline(&path, None, false, 360).unwrap();
        let r2 = scan_acp_timeline(&path, None, false, 360).unwrap();

        assert_eq!(r1.events.len(), 20);
        assert_eq!(r2.events.len(), 20);
        assert_eq!(r2.event_count, r1.event_count);
        assert_eq!(r2.event_page.total, r1.event_page.total);

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn scan_timeline_cache_invalidates_when_file_changes() {
        let dir = std::env::temp_dir().join(format!("gb-tl-stale-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let db = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let path = write_timeline_file(&db, "acp.timeline.jsonl", &event_sequence(5, 2500));

        let r1 = scan_acp_timeline(&path, None, false, 360).unwrap();
        assert_eq!(r1.events.len(), 5);

        let rewritten_path =
            write_timeline_file(&db, "acp.timeline.jsonl", &event_sequence(8, 2500));
        assert_eq!(rewritten_path, path);
        let r2 = scan_acp_timeline(&path, None, false, 360).unwrap();

        assert_eq!(r2.events.len(), 8);
        assert_eq!(r2.event_page.total, 8);
        assert_eq!(r2.events[7].content.as_deref(), Some("message 7"));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn scan_timeline_active_session_bypasses_cache() {
        let dir = std::env::temp_dir().join(format!("gb-tl-active-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let db = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let path = write_timeline_file(&db, "acp.timeline.jsonl", &event_sequence(10, 4000));

        // Active: should parse fresh, not write cache
        let r = scan_acp_timeline(&path, None, true, 360).unwrap();
        assert_eq!(r.events.len(), 10);

        // Completed: first call should be a MISS, then a HIT
        let r2 = scan_acp_timeline(&path, None, false, 360).unwrap();
        assert_eq!(r2.events.len(), 10);

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn paginate_respects_limit() {
        let dir = std::env::temp_dir().join(format!("gb-tl-page-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let db = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let path = write_timeline_file(&db, "acp.timeline.jsonl", &event_sequence(100, 5000));

        let r = scan_acp_timeline(&path, None, false, 30).unwrap();

        assert_eq!(r.events.len(), 30);
        assert_eq!(r.event_page.total, 100);
        assert!(r.event_page.has_older);
        assert!(!r.event_page.has_newer);
        assert_eq!(r.events[0].content.as_deref(), Some("message 70"));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn paginate_after_seq_includes_a_cumulative_block_extended_past_the_cursor() {
        let mut cumulative = text_event_at(1_000);
        cumulative.id = "assistant-message-1".to_string();
        cumulative.seq = 20;
        cumulative.started_seq = Some(2);
        cumulative.ended_seq = Some(20);
        cumulative.content = Some("检查已经完成，准备调用工具".to_string());

        let scan = paginate_timeline(
            camino::Utf8Path::new("acp.timeline.jsonl"),
            std::slice::from_ref(&cumulative),
            20,
            Some(0),
            &HashMap::new(),
            None,
            None,
            true,
            Some(10),
            None,
            30,
        )
        .unwrap();

        assert_eq!(scan.events.len(), 1);
        assert_eq!(scan.events[0].id, "assistant-message-1");
        assert_eq!(scan.events[0].ended_seq, Some(20));
        assert_eq!(scan.event_page.newest_seq, Some(20));
    }

    #[test]
    fn paginate_after_seq_orders_changed_blocks_by_revision_without_skipping() {
        let mut events = event_sequence(4, 1_000);
        events[0].ended_seq = Some(100);
        events[1].ended_seq = Some(20);
        events[2].ended_seq = Some(30);
        events[3].ended_seq = Some(40);

        let first = paginate_timeline(
            camino::Utf8Path::new("acp.timeline.jsonl"),
            &events,
            4,
            Some(0),
            &HashMap::new(),
            None,
            None,
            true,
            Some(10),
            None,
            2,
        )
        .unwrap();
        assert_eq!(
            first
                .events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            vec!["evt-1", "evt-2"]
        );
        assert_eq!(first.event_page.newest_seq, Some(30));
        assert!(first.event_page.has_newer);

        let second = paginate_timeline(
            camino::Utf8Path::new("acp.timeline.jsonl"),
            &events,
            4,
            Some(0),
            &HashMap::new(),
            None,
            None,
            true,
            first.event_page.newest_seq,
            None,
            2,
        )
        .unwrap();
        assert_eq!(
            second
                .events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            vec!["evt-0", "evt-3"]
        );
        assert_eq!(second.event_page.newest_seq, Some(100));
        assert!(!second.event_page.has_newer);
    }

    #[test]
    fn paginate_after_seq_keeps_equal_revision_blocks_in_one_page() {
        let mut events = event_sequence(3, 1_000);
        events[0].ended_seq = Some(20);
        events[1].ended_seq = Some(20);
        events[2].ended_seq = Some(30);

        let first = paginate_timeline(
            camino::Utf8Path::new("acp.timeline.jsonl"),
            &events,
            3,
            Some(0),
            &HashMap::new(),
            None,
            None,
            true,
            Some(10),
            None,
            1,
        )
        .unwrap();

        assert_eq!(first.events.len(), 2);
        assert_eq!(first.event_page.newest_seq, Some(20));
        assert!(first.event_page.has_newer);
    }

    #[test]
    fn paginate_excludes_resolved_permission_from_semantic_page() {
        let mut latest = HashMap::new();
        latest.insert(
            "req-1".to_string(),
            permission_event_at("req-1", "selected", 3000),
        );
        let events = vec![
            permission_event_at("req-1", "pending", 1000),
            text_event_at(1100),
            text_event_at(1200),
        ];

        let scan = paginate_timeline(
            camino::Utf8Path::new("acp.timeline.jsonl"),
            &events,
            events.len(),
            Some(0),
            &latest,
            None,
            None,
            false,
            None,
            None,
            2,
        )
        .unwrap();

        assert!(
            scan.events
                .iter()
                .all(|event| event.kind != "permissionRequest")
        );
        assert_eq!(
            scan.latest_permission_events
                .get("req-1")
                .and_then(|event| event.status.as_deref()),
            Some("selected")
        );
    }

    #[test]
    fn semantic_page_ignores_todo_revisions_and_resolved_elicitation() {
        let mut user = acp_event_at("user", "userTextDelta", Some("completed"), 1_000, None);
        user.content = Some("delegate".to_string());
        let mut plan = acp_event_at(
            "plan-latest",
            "plan",
            Some("completed"),
            1_100,
            Some(json!({ "entries": [{ "content": "inspect", "status": "pending" }] })),
        );
        plan.content = None;
        let request = elicitation_request_event_at("elicit-1", 1_200);
        let response = elicitation_response_event_at("elicit-1", 1_300);
        let mut assistant = acp_event_at("answer", "textDelta", Some("completed"), 1_400, None);
        assistant.content = Some("done".to_string());
        let events = vec![user, plan, request, response, assistant];

        let scan = paginate_timeline(
            camino::Utf8Path::new("acp.timeline.jsonl"),
            &events,
            events.len(),
            Some(0),
            &HashMap::new(),
            None,
            None,
            false,
            None,
            None,
            30,
        )
        .unwrap();

        assert_eq!(scan.event_page.total, 2);
        assert!(!scan.event_page.has_older);
        assert_eq!(scan.events.len(), 2);
        assert!(
            scan.events
                .iter()
                .all(|event| matches!(event.kind.as_str(), "userTextDelta" | "textDelta"))
        );
        assert_eq!(scan.timeline_projection.todo_entries.len(), 1);
    }

    #[test]
    fn pending_elicitation_is_one_current_semantic_block() {
        let request = elicitation_request_event_at("elicit-pending", 1_000);
        let scan = paginate_timeline(
            camino::Utf8Path::new("acp.timeline.jsonl"),
            std::slice::from_ref(&request),
            1,
            Some(0),
            &HashMap::new(),
            None,
            None,
            true,
            None,
            None,
            30,
        )
        .unwrap();

        assert_eq!(scan.event_page.total, 1);
        assert_eq!(scan.events.len(), 1);
        assert_eq!(scan.events[0].kind, "elicitationRequest");
        assert_eq!(scan.pending_elicitations.len(), 1);
        let AcpPromptInteractionVm::Elicitation {
            interaction_id,
            requested_schema,
            ..
        } = &scan.pending_elicitations[0]
        else {
            panic!("expected elicitation interaction");
        };
        assert_eq!(interaction_id, "elicit-pending");
        assert_eq!(
            requested_schema["properties"]["database"]["type"],
            "string"
        );
    }

    #[test]
    fn pending_elicitation_is_authoritative_outside_the_requested_page_and_timing() {
        let request = elicitation_request_event_at("elicit-authoritative", 1_000);
        let later_message =
            acp_event_at("later-message", "textDelta", Some("completed"), 2_000, None);
        let events = vec![request, later_message];
        let scan = paginate_timeline(
            camino::Utf8Path::new("acp.timeline.jsonl"),
            &events,
            events.len(),
            Some(0),
            &HashMap::new(),
            None,
            None,
            true,
            None,
            None,
            1,
        )
        .unwrap();

        assert!(
            scan.events
                .iter()
                .all(|event| event.kind != "elicitationRequest")
        );
        assert_eq!(scan.pending_elicitations.len(), 1);
        let AcpPromptInteractionVm::Elicitation { interaction_id, .. } =
            &scan.pending_elicitations[0]
        else {
            panic!("expected elicitation interaction");
        };
        assert_eq!(interaction_id, "elicit-authoritative");
    }

    #[test]
    fn elicitation_response_settles_pending_but_terminal_status_alone_does_not() {
        let request = elicitation_request_event_at("elicit-resolved", 1_000);
        let response = elicitation_response_event_at("elicit-resolved", 2_000);
        let resolved = pending_elicitation_vms(&[request.clone(), response]);
        let terminal = pending_elicitation_vms(&[request]);

        assert!(resolved.is_empty());
        assert_eq!(terminal.len(), 1);
    }

    #[test]
    fn answered_latest_elicitation_does_not_resurface_an_older_request() {
        let older = elicitation_request_event_at("elicit-old", 1_000);
        let newer = elicitation_request_event_at("elicit-new", 2_000);
        let response = elicitation_response_event_at("elicit-new", 3_000);

        assert!(pending_elicitation_vms(&[older, newer, response]).is_empty());
    }

    #[test]
    fn root_semantic_page_counts_agent_links_but_not_agent_branch_history() {
        let dir =
            std::env::temp_dir().join(format!("gb-agent-semantic-page-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let attempt = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let mut user = test_event("userTextDelta", "inspect agents");
        user.id = "root-user".to_string();
        user.seq = 1;
        user.started_seq = Some(1);
        user.ended_seq = Some(1);
        let launch = |id: &str, tool_call_id: &str, seq: u64| {
            let agent_execution_id =
                gold_band::acp::branches::stable_agent_execution_id("s1", tool_call_id);
            let mut event = acp_event_at(
                id,
                "toolCall",
                Some("completed"),
                1_000 + seq,
                Some(json!({
                    "_meta": { "goldBandConversation": {
                        "branchId": "root",
                        "launchedAgentExecutionId": agent_execution_id,
                        "toolName": "Agent"
                    }, "agentTranscript": {
                        "agentLaunch": true,
                        "toolName": "Agent"
                    } },
                    "rawInput": { "run_in_background": true, "description": id }
                })),
            );
            event.seq = seq;
            event.started_seq = Some(seq);
            event.ended_seq = Some(seq);
            event.tool_call_id = Some(tool_call_id.to_string());
            event
        };
        let root = vec![
            user,
            launch("agent-a", "provider-a", 2),
            launch("agent-b", "provider-b", 3),
        ];
        let root_path = gold_band::acp::branches::branch_timeline_path(
            &attempt,
            gold_band::acp::branches::ROOT_BRANCH_ID,
        );
        write_timeline_file(&attempt, "acp.timeline.jsonl", &root);

        let child_id = gold_band::acp::branches::stable_agent_execution_id("s1", "provider-a");
        let child_events = (0..500)
            .map(|index| {
                let mut event = acp_event_at(
                    &format!("child-tool-{index}"),
                    "toolCall",
                    Some("completed"),
                    2_000 + index,
                    Some(json!({ "rawInput": { "path": format!("file-{index}.rs") } })),
                );
                event.seq = index + 10;
                event.started_seq = Some(index + 10);
                event.ended_seq = Some(index + 10);
                event.tool_call_id = Some(format!("child-call-{index}"));
                event
            })
            .collect::<Vec<_>>();
        let child_path = gold_band::acp::branches::branch_timeline_path(&attempt, &child_id);
        fs::create_dir_all(child_path.parent().unwrap().as_std_path()).unwrap();
        write_timeline_file(
            &attempt,
            &format!("agents/{child_id}/timeline.jsonl"),
            &child_events,
        );

        let scan = scan_acp_timeline(&root_path, None, false, 30).unwrap();
        assert_eq!(scan.event_page.total, 3);
        assert_eq!(scan.event_page.loaded_count, 3);
        assert!(!scan.event_page.has_older);
        assert_eq!(scan.events.len(), 3);
        assert_eq!(
            scan.events
                .iter()
                .filter(|event| {
                    event
                        .raw
                        .as_ref()
                        .and_then(|raw| {
                            raw.pointer("/_meta/goldBandConversation/launchedAgentExecutionId")
                        })
                        .and_then(serde_json::Value::as_str)
                        .is_some()
                })
                .count(),
            2
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn activity_summary_and_detail_use_independent_cursors() {
        let dir = std::env::temp_dir().join(format!("gb-activity-detail-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let attempt = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let mut events = (1..=100)
            .map(|seq| {
                let mut event = acp_event_at(
                    &format!("tool-{seq}"),
                    if seq % 2 == 0 {
                        "thoughtDelta"
                    } else {
                        "toolCall"
                    },
                    Some("completed"),
                    1_000 + seq,
                    Some(json!({ "rawInput": { "path": format!("file-{seq}.rs") } })),
                );
                event.seq = seq;
                event.started_seq = Some(seq);
                event.ended_seq = Some(seq);
                if event.kind == "toolCall" {
                    event.tool_call_id = Some(format!("call-{seq}"));
                    event.raw.as_mut().unwrap()["output"] = json!(format!("tool output {seq}"));
                }
                event
            })
            .collect::<Vec<_>>();
        let mut resolved_permission = permission_event_at("resolved", "selected", 1_050);
        resolved_permission.seq = 50;
        resolved_permission.started_seq = Some(50);
        resolved_permission.ended_seq = Some(50);
        events.push(resolved_permission);
        let mut answer = text_event_at(2_000);
        answer.id = "answer".to_string();
        answer.seq = 101;
        answer.started_seq = Some(101);
        answer.ended_seq = Some(101);
        events.push(answer);
        events.sort_by_key(|event| (event.started_seq.unwrap_or(event.seq), event.seq));
        let path = gold_band::acp::branches::branch_timeline_path(
            &attempt,
            gold_band::acp::branches::ROOT_BRANCH_ID,
        );
        write_timeline_file(&attempt, "acp.timeline.jsonl", &events);

        let page = scan_acp_timeline(&path, None, false, 30).unwrap();
        assert_eq!(page.event_page.total, 2);
        assert!(!page.event_page.has_older);
        assert_eq!(page.events.len(), 2);
        let summary = page
            .events
            .iter()
            .find(|event| event.kind == "activitySummary")
            .unwrap();
        assert_eq!(
            summary.raw.as_ref().unwrap()["goldBandActivity"]["totalEventCount"],
            100
        );

        let detail = acp_activity_detail_vm_for_attempt(
            &attempt,
            AcpActivityDetailQueryInput {
                branch_id: gold_band::acp::branches::ROOT_BRANCH_ID.to_string(),
                activity_start_seq: 1,
                activity_end_seq: 100,
                earlier_cursor: None,
                limit: Some(40),
            },
        )
        .unwrap();
        assert_eq!(detail.items.len(), 40);
        assert!(detail.has_more_earlier);
        assert!(
            detail
                .items
                .iter()
                .all(|event| event.kind != "permissionRequest")
        );
        let recent_tool = detail
            .items
            .iter()
            .find(|event| event.kind == "toolCall")
            .unwrap();
        assert!(recent_tool.raw.as_ref().unwrap().get("output").is_none());
        assert_eq!(
            recent_tool.raw.as_ref().unwrap()["_meta"]["goldBandConversation"]["toolDetailAvailable"],
            true
        );
        assert_eq!(page.event_page.total, 2);

        let tool_detail = acp_tool_detail_vm_for_attempt(
            &attempt,
            AcpToolDetailQueryInput {
                branch_id: gold_band::acp::branches::ROOT_BRANCH_ID.to_string(),
                event_id: "tool-99".to_string(),
                tool_call_id: Some("call-99".to_string()),
            },
        )
        .unwrap()
        .event
        .expect("tool detail");
        assert_eq!(
            tool_detail.raw.as_ref().unwrap()["output"],
            "tool output 99"
        );

        let earlier = acp_activity_detail_vm_for_attempt(
            &attempt,
            AcpActivityDetailQueryInput {
                branch_id: gold_band::acp::branches::ROOT_BRANCH_ID.to_string(),
                activity_start_seq: 1,
                activity_end_seq: 100,
                earlier_cursor: detail.earlier_cursor.clone(),
                limit: Some(40),
            },
        )
        .unwrap();
        assert_eq!(earlier.items.len(), 40);
        assert!(earlier.items.last().unwrap().seq < detail.items.first().unwrap().seq);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn current_pending_permission_is_one_semantic_block_until_resolved() {
        let dir = std::env::temp_dir().join(format!(
            "gb-pending-semantic-block-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        let attempt = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let mut user = test_event("userTextDelta", "request");
        user.seq = 1;
        user.started_seq = Some(1);
        user.ended_seq = Some(1);
        let mut pending = permission_event_at("permission-1", "pending", 2_000);
        pending.seq = 2;
        pending.started_seq = Some(2);
        pending.ended_seq = Some(2);
        let timeline = gold_band::acp::branches::branch_timeline_path(
            &attempt,
            gold_band::acp::branches::ROOT_BRANCH_ID,
        );
        write_timeline_file(&attempt, "acp.timeline.jsonl", &[user.clone(), pending]);
        let pending_page = scan_acp_timeline(&timeline, None, true, 30).unwrap();
        assert_eq!(pending_page.event_page.total, 2);
        assert!(pending_page.events.iter().any(|event| {
            event.kind == "permissionRequest" && event.status.as_deref() == Some("pending")
        }));

        let mut resolved = permission_event_at("permission-1", "selected", 3_000);
        resolved.seq = 3;
        resolved.started_seq = Some(2);
        resolved.ended_seq = Some(3);
        write_timeline_file(&attempt, "acp.timeline.jsonl", &[user, resolved]);
        let resolved_page = scan_acp_timeline(&timeline, None, false, 30).unwrap();
        assert_eq!(resolved_page.event_page.total, 1);
        assert!(
            resolved_page
                .events
                .iter()
                .all(|event| event.kind != "permissionRequest")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn paginated_tool_detail_hydrates_blob_backed_terminal_output() {
        let dir =
            std::env::temp_dir().join(format!("gb-blob-tool-detail-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let attempt = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let timeline_path = gold_band::acp::branches::branch_timeline_path(
            &attempt,
            gold_band::acp::branches::ROOT_BRANCH_ID,
        );
        let large_output = "terminal-output".repeat(8 * 1024);
        let mut event = acp_event_at(
            "tool-large",
            "toolCall",
            Some("completed"),
            1_000,
            Some(json!({ "output": large_output })),
        );
        event.tool_call_id = Some("call-large".to_string());
        let stored_event: gold_band::acp::events::AcpUiEvent =
            serde_json::from_value(serde_json::to_value(event).unwrap()).unwrap();
        gold_band::acp::events::write_timeline_items(&timeline_path, &[stored_event]).unwrap();

        let stored = fs::read_to_string(timeline_path.as_std_path()).unwrap();
        assert!(stored.contains("$goldBandBlob"));
        let detail = acp_tool_detail_vm_for_attempt(
            &attempt,
            AcpToolDetailQueryInput {
                branch_id: gold_band::acp::branches::ROOT_BRANCH_ID.to_string(),
                event_id: "tool-large".to_string(),
                tool_call_id: Some("call-large".to_string()),
            },
        )
        .unwrap()
        .event
        .unwrap();

        assert_eq!(
            detail.raw.unwrap()["output"].as_str(),
            Some(large_output.as_str())
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn agent_execution_vm_does_not_serialize_provider_tool_ids() {
        let vm = AcpAgentExecutionVm {
            agent_execution_id: "agent-internal".to_string(),
            parent_agent_execution_id: Some("agent-parent".to_string()),
            execution_status: "running".to_string(),
            event_count: 1,
            tool_call_count: 1,
            read_file_count: 0,
            written_file_count: 0,
            has_attention: false,
            title: Some("Agent".to_string()),
            description: None,
            todo_entries: Vec::new(),
        };
        let value = serde_json::to_value(vm).unwrap();
        assert_eq!(value["agentExecutionId"], "agent-internal");
        assert!(value.get("toolCallId").is_none());
        assert!(value.get("parentToolCallId").is_none());
        assert!(value.get("launchStatus").is_none());
    }

    #[test]
    fn agent_branch_session_uses_execution_status_instead_of_root_status() {
        let agent = test_agent_record("agent-current", None, "completed");
        let records = vec![agent];
        let record = conversation_branch_record(&records, "agent-current");

        assert_eq!(
            conversation_branch_status("running", "agent-current", record),
            "completed"
        );
        assert_eq!(
            conversation_branch_status("running", gold_band::acp::branches::ROOT_BRANCH_ID, None),
            "running"
        );
        assert_eq!(
            conversation_branch_status("running", "agent-missing", None),
            "unknown"
        );
        assert_eq!(
            conversation_branch_elapsed_seconds("agent-current", record, Some(999)),
            Some(20)
        );
        assert_eq!(
            conversation_branch_elapsed_seconds(
                gold_band::acp::branches::ROOT_BRANCH_ID,
                None,
                Some(999),
            ),
            Some(999)
        );
    }

    #[test]
    fn agent_branch_projection_contains_only_direct_child_agents() {
        let records = vec![
            test_agent_record("agent-current", None, "running"),
            test_agent_record("agent-child", Some("agent-current"), "running"),
            test_agent_record("agent-grandchild", Some("agent-child"), "queued"),
            test_agent_record("agent-sibling", None, "completed"),
        ];
        let mut branch_projection = AcpTimelineProjectionVm::default();
        apply_agent_index_projection(&mut branch_projection, &records, "agent-current");
        assert_eq!(branch_projection.agents.len(), 1);
        assert_eq!(
            branch_projection.agents[0].agent_execution_id,
            "agent-child"
        );

        let mut root_projection = AcpTimelineProjectionVm::default();
        apply_agent_index_projection(
            &mut root_projection,
            &records,
            gold_band::acp::branches::ROOT_BRANCH_ID,
        );
        assert_eq!(root_projection.agents.len(), 2);
        assert_eq!(
            root_projection.agents[0].agent_execution_id,
            "agent-current"
        );
        assert_eq!(
            root_projection.agents[1].agent_execution_id,
            "agent-sibling"
        );
    }

    #[test]
    fn root_projection_excludes_twenty_five_nested_agents() {
        let mut records = vec![
            test_agent_record("agent-top-a", None, "running"),
            test_agent_record("agent-top-b", None, "running"),
        ];
        for index in 0..25 {
            let id = format!("agent-nested-{index}");
            let parent = if index % 2 == 0 {
                "agent-top-a"
            } else {
                "agent-top-b"
            };
            records.push(test_agent_record(&id, Some(parent), "running"));
        }

        let mut projection = AcpTimelineProjectionVm::default();
        apply_agent_index_projection(
            &mut projection,
            &records,
            gold_band::acp::branches::ROOT_BRANCH_ID,
        );

        assert_eq!(projection.agents.len(), 2);
        assert!(
            projection
                .agents
                .iter()
                .all(|agent| agent.parent_agent_execution_id.is_none())
        );
    }

    #[test]
    fn unscoped_session_plan_is_hidden_from_agent_root_without_text_inference() {
        let plan = acp_event_at(
            "session-plan",
            "plan",
            Some("completed"),
            1_000,
            Some(json!({
                "entries": [{
                    "content": "agent-top-a wording must not imply ownership",
                    "status": "pending"
                }],
                "_meta": {
                    "goldBandConversation": {
                        "branchId": "root",
                        "planOwnership": "unscoped"
                    }
                }
            })),
        );
        let mut projection = build_acp_timeline_projection(&[plan], &HashMap::new(), true);
        let records = vec![test_agent_record("agent-top-a", None, "running")];

        apply_agent_index_projection(
            &mut projection,
            &records,
            gold_band::acp::branches::ROOT_BRANCH_ID,
        );

        assert!(projection.todo_entries.is_empty());
    }

    #[test]
    fn ordinary_root_and_explicit_agent_plans_keep_their_todos() {
        let unscoped_root_plan = acp_event_at(
            "root-plan",
            "plan",
            Some("completed"),
            1_000,
            Some(json!({
                "entries": [{ "content": "ordinary root task", "status": "pending" }],
                "_meta": {
                    "goldBandConversation": {
                        "branchId": "root",
                        "planOwnership": "unscoped"
                    }
                }
            })),
        );
        let mut root_projection =
            build_acp_timeline_projection(&[unscoped_root_plan], &HashMap::new(), false);
        apply_agent_index_projection(
            &mut root_projection,
            &[],
            gold_band::acp::branches::ROOT_BRANCH_ID,
        );
        assert_eq!(root_projection.todo_entries.len(), 1);

        let branch_plan = acp_event_at(
            "branch-plan",
            "plan",
            Some("completed"),
            2_000,
            Some(json!({
                "entries": [{ "content": "agent task", "status": "in_progress" }],
                "_meta": {
                    "goldBandConversation": {
                        "branchId": "agent-current",
                        "planOwnership": "branch"
                    }
                }
            })),
        );
        let mut branch_projection =
            build_acp_timeline_projection(&[branch_plan], &HashMap::new(), true);
        apply_agent_index_projection(
            &mut branch_projection,
            &[test_agent_record("agent-current", None, "running")],
            "agent-current",
        );
        assert_eq!(branch_projection.todo_entries.len(), 1);
    }

    #[test]
    fn cache_key_isolated_by_path() {
        let dir = std::env::temp_dir().join(format!("gb-tl-keys-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let db = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let pa = write_timeline_file(&db, "a.jsonl", &event_sequence(5, 6000));
        let pb = write_timeline_file(&db, "b.jsonl", &event_sequence(8, 7000));

        let ra = scan_acp_timeline(&pa, None, false, 360).unwrap();
        let rb = scan_acp_timeline(&pb, None, false, 360).unwrap();
        assert_eq!(ra.events.len(), 5);
        assert_eq!(rb.events.len(), 8);

        // Second call to A still returns 5, not 8
        let ra2 = scan_acp_timeline(&pa, None, false, 360).unwrap();
        assert_eq!(ra2.events.len(), 5);

        fs::remove_dir_all(dir).unwrap();
    }
}
