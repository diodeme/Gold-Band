use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    sync::{LazyLock, Mutex},
    time::SystemTime,
};

use anyhow::Result;
use gold_band::app::{App, LogSource, TaskSummary, is_run_continuable};
use gold_band::config::{
    DesktopAvailableUpdate, DesktopFontPreference, DesktopLanguage, DesktopThemePreference,
    DesktopUpdateBadgeState, ManagedAgentConfig, ManagedAgentType, RuntimeConfig, RuntimeLogLevel,
};
use gold_band::domain::{NodeType, RunOutcome, RunStatus, SessionMode};
use gold_band::dsl::{NodeDsl, WorkflowDsl, WorkflowValidationError};
use gold_band::dynamic::DynamicGraphState;
use gold_band::provider::{supported_models_from_capabilities, supported_modes_from_capabilities};
use gold_band::runtime::{NodeState, RoundState, RoundTraceStep, RunState, WorkerRefState};

use crate::channel::current_channel_config;
use crate::i18n::Translator;
use crate::metrics::{MetricsSettingsVm, metrics_settings};
use crate::state::AgentDiagnosticState;
use crate::updater::{UpdateInfoVm, UpdateStatusVm, UpdaterSettingsVm, updater_settings};
use gold_band::storage::{read_json, write_json};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferencesVm {
    pub theme: DesktopThemePreference,
    pub language: DesktopLanguage,
    pub font: DesktopFontPreference,
    pub use_local_claude: bool,
    pub verbose_logging: bool,
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
    pub app_info: AppInfoVm,
    pub app_config: AppConfigVm,
    pub needs_workspace: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfigVm {
    pub acp_session_title_refresh_enabled: bool,
    pub acp_chat_event_page_size: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoVm {
    pub channel: String,
    pub app_name: String,
    pub app_key: String,
    pub config_dir_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRegistryVm {
    pub agents: Vec<ManagedAgentVm>,
    pub supported_types: Vec<SupportedAgentTypeVm>,
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
    pub skills_dir_name: String,
    pub supported: bool,
    pub diagnostic: Option<ManagedAgentDiagnosticVm>,
    pub supported_modes: Option<Vec<AcpModeVm>>,
    pub supported_models: Option<Vec<AcpModeVm>>,
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
pub struct SupportedAgentTypeVm {
    pub agent_type: String,
    pub label: String,
    pub icon_key: String,
    pub skills_dir_name: String,
    pub supported: bool,
    pub configured: bool,
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
            Some("running") | Some("in-progress") | Some("in_progress") | Some("active") => {
                ("running", "running", "dot", false)
            }
            Some("paused") if current && reason_code.as_deref() == Some("error-blocked") => {
                ("error-blocked", "danger", "error", false)
            }
            Some("paused") => ("paused", "warning", "pause", false),
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
        resumable: (code == "paused" || code == "error-blocked") && resumable,
        reason_code,
        blocking_error,
    }
}

fn normalize_status_code(value: &str) -> String {
    match value.trim().to_ascii_lowercase().replace('_', "-").as_str() {
        "errorblocked" => "error-blocked".to_string(),
        "processinterrupted" => "process-interrupted".to_string(),
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
    pub session_id: Option<String>,
    pub title: Option<String>,
    pub provider: String,
    pub adapter_id: Option<String>,
    pub adapter_display_name: Option<String>,
    pub cwd: Option<String>,
    pub status: String,
    pub session_started_at: Option<String>,
    pub session_updated_at: Option<String>,
    pub session_elapsed_seconds: Option<u64>,
    pub restored: bool,
    pub stop_reason: Option<String>,
    pub system_prompt_append: Option<String>,
    pub config: Option<AcpSessionConfigVm>,
    pub events: Vec<AcpUiEventVm>,
    pub event_page: AcpEventPageVm,
    pub pending_permissions: Vec<AcpPermissionRequestVm>,
    pub available_commands: Option<Vec<serde_json::Value>>,
    pub usage: Option<AcpUsageVm>,
    pub diagnostics: AcpDiagnosticsVm,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpSessionQueryInput {
    pub before_seq: Option<u64>,
    pub after_seq: Option<u64>,
    pub before_cursor: Option<String>,
    pub after_cursor: Option<String>,
    pub event_limit: Option<usize>,
    pub page_size: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpEventPageVm {
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
    pub raw: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpPermissionRequestVm {
    pub request_id: String,
    pub title: String,
    pub tool_call_id: Option<String>,
    pub options: Vec<AcpPermissionOptionVm>,
    pub raw: serde_json::Value,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpRawFrameQueryInput {
    pub page: Option<usize>,
    pub page_size: Option<usize>,
    pub search: Option<String>,
    pub kind: Option<String>,
    pub direction: Option<String>,
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
    pub order: String,
    pub search: Option<String>,
    pub kind: Option<String>,
    pub direction: Option<String>,
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
    theme: DesktopThemePreference,
    language: DesktopLanguage,
    font: DesktopFontPreference,
    use_local_claude: bool,
    log_level: RuntimeLogLevel,
) -> PreferencesVm {
    PreferencesVm {
        theme,
        language,
        font,
        use_local_claude,
        verbose_logging: matches!(log_level, RuntimeLogLevel::Debug | RuntimeLogLevel::Trace),
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
    AppConfigVm {
        acp_session_title_refresh_enabled: config.acp_session_title_refresh_enabled,
        acp_chat_event_page_size: config.acp_chat_event_page_size,
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
            app.config.desktop_theme,
            app.config.desktop_language,
            app.config.desktop_font.clone(),
            app.config.use_local_claude,
            app.config.log_level,
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
        app_info: AppInfoVm {
            channel: channel_config.channel.to_string(),
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
    diagnostics: &std::collections::BTreeMap<ManagedAgentType, AgentDiagnosticState>,
) -> AgentRegistryVm {
    let agents = app
        .managed_agents()
        .iter()
        .map(|(agent_type, config)| {
            managed_agent_vm(*agent_type, config, diagnostics.get(agent_type))
        })
        .collect::<Vec<_>>();
    let supported_types = ManagedAgentType::ALL
        .into_iter()
        .map(|agent_type| {
            let default_config = agent_type.default_adapter_config();
            SupportedAgentTypeVm {
                agent_type: agent_type.as_str().to_string(),
                label: supported_agent_label(agent_type).to_string(),
                icon_key: agent_icon_key(agent_type).to_string(),
                skills_dir_name: app
                    .managed_agents()
                    .get(&agent_type)
                    .map(|config| config.skills_dir_name(agent_type).to_string())
                    .unwrap_or_else(|| agent_type.skills_dir_name().to_string()),
                supported: agent_type.is_supported(),
                configured: app.managed_agents().contains_key(&agent_type),
                default_display_name: default_config.display_name,
                default_command: default_config.command,
                default_args: default_config.args,
                default_env: default_config
                    .env
                    .into_iter()
                    .map(|(key, value)| AgentEnvEntryVm { key, value })
                    .collect(),
            }
        })
        .collect();
    AgentRegistryVm {
        agents,
        supported_types,
    }
}

fn managed_agent_vm(
    agent_type: ManagedAgentType,
    config: &ManagedAgentConfig,
    diagnostic: Option<&AgentDiagnosticState>,
) -> ManagedAgentVm {
    ManagedAgentVm {
        agent_type: agent_type.as_str().to_string(),
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
        icon_key: agent_icon_key(agent_type).to_string(),
        skills_dir_name: config.skills_dir_name(agent_type).to_string(),
        supported: agent_type.is_supported(),
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
    }
}

fn agent_icon_key(agent_type: ManagedAgentType) -> &'static str {
    match agent_type {
        ManagedAgentType::ClaudeAcp => "claude",
        ManagedAgentType::CodexAcp => "codex",
        ManagedAgentType::Cursor => "cursor",
        ManagedAgentType::Gemini => "gemini",
        ManagedAgentType::OpenCode => "opencode",
    }
}

fn provider_icon_key(provider: &str) -> Option<String> {
    match provider {
        "claude-acp" => Some("claude".to_string()),
        "codex-acp" => Some("codex".to_string()),
        "cursor" => Some("cursor".to_string()),
        "gemini" => Some("gemini".to_string()),
        "opencode" => Some("opencode".to_string()),
        _ => None,
    }
}

fn supported_agent_label(agent_type: ManagedAgentType) -> &'static str {
    match agent_type {
        ManagedAgentType::ClaudeAcp => "Claude",
        ManagedAgentType::CodexAcp => "Codex",
        ManagedAgentType::Cursor => "Cursor",
        ManagedAgentType::Gemini => "Gemini",
        ManagedAgentType::OpenCode => "OpenCode",
    }
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
    let workflow_json = read_optional_text(&app.paths.workflow_file(task_id))?;
    let workflow = read_json::<WorkflowDsl>(&app.paths.workflow_file(task_id)).ok();
    let graph = workflow
        .as_ref()
        .map(workflow_graph_vm)
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
    })
}

pub fn run_detail_vm(app: &App, task_id: &str, run_id: &str) -> Result<RunDetailVm> {
    let run = app.run_status(task_id, run_id)?;
    let rounds = app
        .round_list(task_id, run_id)?
        .into_iter()
        .map(|round| round_summary_vm(app, task_id, &run, round))
        .collect::<Result<Vec<_>>>()?;
    Ok(RunDetailVm {
        run: run_summary_vm(run),
        rounds,
        events: app.run_events(task_id, run_id)?,
        progress: app.run_progress(task_id, run_id)?,
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

fn latest_control_failure_vm(
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
    if latest.is_none() {
        if let Some(progress) = app.run_progress(task_id, run_id)? {
            if let Some(summary) = progress.get("summary").and_then(|value| value.as_str()) {
                latest = control_failure_from_summary(summary, &progress, &serde_json::Value::Null);
            }
        }
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

pub fn workflow_graph_vm(workflow: &WorkflowDsl) -> GraphVm {
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
                icon_key: node.provider().and_then(provider_icon_key),
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
                    .and_then(provider_icon_key)
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
        .then(|| read_json::<DynamicGraphState>(&path).ok())
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
        icon_key: node.provider.as_deref().and_then(provider_icon_key),
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
            let pause_reason = graph.run.pause_reason.as_ref().map(enum_label);
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
            .and_then(provider_icon_key),
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
    let mut session = if let Some(json) = preloaded_session_json {
        json
    } else if snapshot_path.exists() {
        read_json::<serde_json::Value>(&snapshot_path).unwrap_or_else(|_| serde_json::json!({}))
    } else if session_path.exists() {
        read_json::<serde_json::Value>(&session_path).unwrap_or_else(|_| serde_json::json!({}))
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
    let node_path = app.paths.dynamic_node_file(
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        node_id,
    );
    let worker_ref = if worker_ref_path.exists() {
        read_json::<WorkerRefState>(&worker_ref_path).ok()
    } else {
        None
    };
    let continue_ref = worker_ref
        .as_ref()
        .and_then(|state| state.continue_ref.as_ref());
    let diagnostics = scan_acp_diagnostics(&diagnostics_path)?;
    let system_prompt_append = session
        .get("systemPromptAppend")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| extract_system_prompt_append(&raw_path));
    apply_stale_session_completion_fuse_dynamic(&attempt_dir, &node_path, &mut session)?;
    let config = acp_session_config_vm(&session);
    let metadata_status = session
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let stopping = is_acp_session_stopping_status(metadata_status);
    let status = metadata_status.to_string();
    let default_event_limit = app.config.acp_chat_event_page_size;
    let event_scan = if timeline_path.exists() {
        scan_acp_timeline(
            &timeline_path,
            query.clone(),
            is_acp_session_active_status(&status),
            default_event_limit,
        )?
    } else {
        scan_acp_events(
            &events_path,
            query,
            is_acp_session_active_status(&status),
            default_event_limit,
        )?
    };
    let pending_permissions = if stopping {
        Vec::new()
    } else {
        event_scan
            .latest_permission_events
            .into_values()
            .filter(|event| event.status.as_deref() == Some("pending"))
            .map(|event| permission_vm_from_event(&event))
            .collect::<Vec<_>>()
    };
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
    let result = AcpSessionVm {
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
        provider,
        adapter_id: continue_ref
            .and_then(|value| value.get("adapterId"))
            .and_then(|value| value.as_str())
            .or_else(|| session.get("adapterId").and_then(|value| value.as_str()))
            .map(str::to_string),
        adapter_display_name,
        cwd: continue_ref
            .and_then(|value| value.get("cwd"))
            .and_then(|value| value.as_str())
            .or_else(|| session.get("cwd").and_then(|value| value.as_str()))
            .map(str::to_string),
        status,
        session_started_at: session
            .get("createdAt")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        session_updated_at: session
            .get("updatedAt")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        session_elapsed_seconds: event_scan.session_elapsed_seconds,
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
        pending_permissions,
        available_commands: event_scan.available_commands,
        usage: {
            let mut u = event_scan.usage.unwrap_or_default();
            if u.used.is_none() {
                u.used = session.get("usedTokens").and_then(|v| v.as_u64());
            }
            if u.size.is_none() {
                u.size = session.get("contextWindowSize").and_then(|v| v.as_u64());
            }
            if u.cost_amount_usd.is_none() {
                u.cost_amount_usd = session.get("totalCostUsd").and_then(|v| v.as_f64());
            }
            if u.input_tokens.is_none() {
                u.input_tokens = session.get("inputTokens").and_then(|v| v.as_u64());
            }
            if u.output_tokens.is_none() {
                u.output_tokens = session.get("outputTokens").and_then(|v| v.as_u64());
            }
            if u.cached_read_tokens.is_none() {
                u.cached_read_tokens = session.get("cachedReadTokens").and_then(|v| v.as_u64());
            }
            if u.cached_write_tokens.is_none() {
                u.cached_write_tokens = session.get("cachedWriteTokens").and_then(|v| v.as_u64());
            }
            if u.total_tokens.is_none() {
                u.total_tokens = session.get("totalTokens").and_then(|v| v.as_u64());
            }
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

    let mut session = if let Some(json) = preloaded_session_json {
        json
    } else if snapshot_path.exists() {
        read_json::<serde_json::Value>(&snapshot_path).unwrap_or_else(|_| serde_json::json!({}))
    } else if session_path.exists() {
        read_json::<serde_json::Value>(&session_path).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
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
    let continue_ref = worker_ref
        .as_ref()
        .and_then(|state| state.continue_ref.as_ref());
    let node_path = app
        .paths
        .node_file(task_id, run_id, round_id, node_id, attempt_id);
    let diagnostics = scan_acp_diagnostics(&diagnostics_path)?;
    let system_prompt_append = session
        .get("systemPromptAppend")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| extract_system_prompt_append(&raw_path));
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
    let config = acp_session_config_vm(&session);
    let metadata_status = session
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let stopping = is_acp_session_stopping_status(metadata_status);
    let status = metadata_status.to_string();
    let default_event_limit = app.config.acp_chat_event_page_size;
    let event_scan = if timeline_path.exists() {
        scan_acp_timeline(
            &timeline_path,
            query.clone(),
            is_acp_session_active_status(&status),
            default_event_limit,
        )?
    } else {
        scan_acp_events(
            &events_path,
            query,
            is_acp_session_active_status(&status),
            default_event_limit,
        )?
    };
    let pending_permissions = if stopping {
        Vec::new()
    } else {
        event_scan
            .latest_permission_events
            .into_values()
            .filter(|event| event.status.as_deref() == Some("pending"))
            .map(|event| permission_vm_from_event(&event))
            .collect::<Vec<_>>()
    };

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

    let result = AcpSessionVm {
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
        provider,
        adapter_id: continue_ref
            .and_then(|value| value.get("adapterId"))
            .and_then(|value| value.as_str())
            .or_else(|| session.get("adapterId").and_then(|value| value.as_str()))
            .map(str::to_string),
        adapter_display_name,
        cwd: continue_ref
            .and_then(|value| value.get("cwd"))
            .and_then(|value| value.as_str())
            .or_else(|| session.get("cwd").and_then(|value| value.as_str()))
            .map(str::to_string),
        status,
        session_started_at: session
            .get("createdAt")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        session_updated_at: session
            .get("updatedAt")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        session_elapsed_seconds: event_scan.session_elapsed_seconds,
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
        usage: {
            let mut u = event_scan.usage.unwrap_or_default();
            // Merge persisted session usage as fallback for restored sessions
            // where events may not contain a usage_update yet.
            if u.used.is_none() {
                u.used = session.get("usedTokens").and_then(|v| v.as_u64());
            }
            if u.size.is_none() {
                u.size = session.get("contextWindowSize").and_then(|v| v.as_u64());
            }
            if u.cost_amount_usd.is_none() {
                u.cost_amount_usd = session.get("totalCostUsd").and_then(|v| v.as_f64());
            }
            // Merge session-end breakdown (input/output/cache/total) from session metadata.
            // These fields are only available after the prompt completes.
            if u.input_tokens.is_none() {
                u.input_tokens = session.get("inputTokens").and_then(|v| v.as_u64());
            }
            if u.output_tokens.is_none() {
                u.output_tokens = session.get("outputTokens").and_then(|v| v.as_u64());
            }
            if u.cached_read_tokens.is_none() {
                u.cached_read_tokens = session.get("cachedReadTokens").and_then(|v| v.as_u64());
            }
            if u.cached_write_tokens.is_none() {
                u.cached_write_tokens = session.get("cachedWriteTokens").and_then(|v| v.as_u64());
            }
            if u.total_tokens.is_none() {
                u.total_tokens = session.get("totalTokens").and_then(|v| v.as_u64());
            }
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
        pending_permissions,
    };
    Ok(Some(result))
}

struct AcpEventScan {
    events: Vec<AcpUiEventVm>,
    event_page: AcpEventPageVm,
    event_count: usize,
    session_elapsed_seconds: Option<u64>,
    latest_permission_events: HashMap<String, AcpUiEventVm>,
    available_commands: Option<Vec<serde_json::Value>>,
    usage: Option<AcpUsageVm>,
}

struct AcpDiagnosticsScan {
    error_count: usize,
    last_error: Option<String>,
    last_error_timestamp: Option<String>,
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

// --- timeline scan cache ---

const TIMELINE_CACHE_MAX_ENTRIES: usize = 16;

struct CachedTimeline {
    file_signature: Option<TimelineFileSignature>,
    all_events: Vec<AcpUiEventVm>,
    event_count: usize,
    session_elapsed_seconds: Option<u64>,
    latest_permission_events: HashMap<String, AcpUiEventVm>,
    available_commands: Option<Vec<serde_json::Value>>,
    usage: Option<AcpUsageVm>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TimelineFileSignature {
    len: u64,
    modified: Option<SystemTime>,
}

struct TimelineCache {
    entries: HashMap<String, CachedTimeline>,
    order: VecDeque<String>,
}

static TIMELINE_CACHE: LazyLock<Mutex<TimelineCache>> = LazyLock::new(|| {
    Mutex::new(TimelineCache {
        entries: HashMap::new(),
        order: VecDeque::new(),
    })
});

fn timeline_cache_key(path: &camino::Utf8Path) -> String {
    path.as_str().to_string()
}

fn timeline_file_signature(path: &camino::Utf8Path) -> Result<Option<TimelineFileSignature>> {
    if !path.exists() {
        return Ok(None);
    }
    let metadata = fs::metadata(path.as_std_path())?;
    Ok(Some(TimelineFileSignature {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    }))
}

fn touch_timeline_cache(cache: &mut TimelineCache, key: &str) {
    if let Some(pos) = cache.order.iter().position(|k| k == key) {
        cache.order.remove(pos);
    }
    cache.order.push_back(key.to_string());
}

fn evict_timeline_cache(cache: &mut TimelineCache) {
    while cache.order.len() > TIMELINE_CACHE_MAX_ENTRIES {
        if let Some(oldest) = cache.order.pop_front() {
            cache.entries.remove(&oldest);
        }
    }
}

// --- end timeline scan cache ---

fn scan_acp_timeline(
    path: &camino::Utf8Path,
    query: Option<AcpSessionQueryInput>,
    session_active: bool,
    default_event_limit: usize,
) -> Result<AcpEventScan> {
    const MIN_EVENT_LIMIT: usize = 1;
    const MAX_EVENT_LIMIT: usize = 1000;

    let query = query.unwrap_or(AcpSessionQueryInput {
        before_seq: None,
        after_seq: None,
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
    let file_signature = timeline_file_signature(path)?;

    // Completed sessions may still receive a final timeline flush shortly after
    // the snapshot flips terminal, so cache only while the file signature matches.
    let cache_key = timeline_cache_key(path);
    let (
        all_events,
        event_count,
        session_elapsed_seconds,
        latest_permission_events,
        available_commands,
        usage,
    ) = if !session_active {
        let mut cache = TIMELINE_CACHE.lock().unwrap();
        if let Some(cached) = cache
            .entries
            .get(&cache_key)
            .filter(|cached| cached.file_signature == file_signature)
        {
            let all_events = cached.all_events.clone();
            let event_count = cached.event_count;
            let session_elapsed_seconds = cached.session_elapsed_seconds;
            let latest_permission_events = cached.latest_permission_events.clone();
            let available_commands = cached.available_commands.clone();
            let usage = cached.usage.clone();
            touch_timeline_cache(&mut cache, &cache_key);
            return paginate_timeline(
                &all_events,
                event_count,
                session_elapsed_seconds,
                &latest_permission_events,
                available_commands.as_ref(),
                usage.as_ref(),
                after_seq,
                before_seq,
                limit,
            );
        }
        drop(cache);
        let result = parse_timeline_file(path, session_active)?;
        let mut cache = TIMELINE_CACHE.lock().unwrap();
        cache.entries.insert(
            cache_key.clone(),
            CachedTimeline {
                file_signature,
                all_events: result.0.clone(),
                event_count: result.1,
                session_elapsed_seconds: result.2,
                latest_permission_events: result.3.clone(),
                available_commands: result.4.clone(),
                usage: result.5.clone(),
            },
        );
        touch_timeline_cache(&mut cache, &cache_key);
        evict_timeline_cache(&mut cache);
        result
    } else {
        parse_timeline_file(path, session_active)?
    };

    paginate_timeline(
        &all_events,
        event_count,
        session_elapsed_seconds,
        &latest_permission_events,
        available_commands.as_ref(),
        usage.as_ref(),
        after_seq,
        before_seq,
        limit,
    )
}

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
    let mut latest_permission_events = HashMap::<String, AcpUiEventVm>::new();
    let mut available_commands = None;
    let mut usage = None;
    let mut session_elapsed = AcpSessionElapsedState::default();
    let mut event_count = 0usize;
    let mut final_items = Vec::<AcpUiEventVm>::new();
    let mut saw_legacy_patch = false;

    if path.exists() {
        let file = fs::File::open(path.as_std_path())?;
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(mut patch) = serde_json::from_str::<AcpTimelinePatchVm>(&line) {
                if patch.patch_type != "timelinePatch" || patch.op != "upsert" {
                    continue;
                }
                saw_legacy_patch = true;
                event_count += 1;
                patch.item.seq = patch.item.ended_seq.unwrap_or(patch.revision);
                session_elapsed.observe_event(&patch.item);
                if patch.item.kind == "permissionRequest" {
                    insert_latest_permission_event(&mut latest_permission_events, &patch.item);
                }
                if let Some(raw) = patch.item.raw.as_ref() {
                    if is_session_update(&patch.item, "available_commands_update") {
                        available_commands = raw
                            .get("availableCommands")
                            .and_then(|value| value.as_array())
                            .cloned();
                    } else if is_session_update(&patch.item, "usage_update") {
                        let (used, size, cost_amount) =
                            gold_band::acp::events::extract_usage_fields(raw);
                        usage = Some(AcpUsageVm {
                            used,
                            size,
                            cost_amount_usd: cost_amount,
                            ..Default::default()
                        });
                    }
                }
                if is_hidden_from_chat(&patch.item) || !is_session_timeline_event(&patch.item) {
                    continue;
                }
                let should_replace = latest_by_item
                    .get(&patch.item_id)
                    .map(|(revision, _)| patch.revision >= *revision)
                    .unwrap_or(true);
                if should_replace {
                    latest_by_item.insert(patch.item_id, (patch.revision, patch.item));
                }
                continue;
            }

            let Ok(mut final_item) = serde_json::from_str::<AcpTimelineItemVm>(&line) else {
                continue;
            };
            event_count += 1;
            final_item.item.seq = final_item
                .item
                .ended_seq
                .or(final_item.item.started_seq)
                .unwrap_or(final_item.item.seq);
            session_elapsed.observe_event(&final_item.item);
            if final_item.item.kind == "permissionRequest" {
                insert_latest_permission_event(&mut latest_permission_events, &final_item.item);
            }
            if let Some(raw) = final_item.item.raw.as_ref() {
                if is_session_update(&final_item.item, "available_commands_update") {
                    available_commands = raw
                        .get("availableCommands")
                        .and_then(|value| value.as_array())
                        .cloned();
                } else if is_session_update(&final_item.item, "usage_update") {
                    let (used, size, cost_amount) =
                        gold_band::acp::events::extract_usage_fields(raw);
                    usage = Some(AcpUsageVm {
                        used,
                        size,
                        cost_amount_usd: cost_amount,
                        ..Default::default()
                    });
                }
            }
            if is_hidden_from_chat(&final_item.item) || !is_session_timeline_event(&final_item.item)
            {
                continue;
            }
            latest_by_item.insert(final_item.item.id.clone(), (0, final_item.item.clone()));
            final_items.push(final_item.item);
        }
    }

    let mut all_events = if saw_legacy_patch {
        let mut merged = latest_by_item;
        for item in final_items {
            merged.entry(item.id.clone()).or_insert((0, item));
        }
        merged
            .into_values()
            .map(|(_, event)| event)
            .collect::<Vec<_>>()
    } else {
        final_items
    };
    all_events.sort_by_key(|event| event.started_seq.unwrap_or(event.seq));

    Ok((
        all_events,
        event_count,
        session_elapsed.finish(session_active),
        latest_permission_events,
        available_commands,
        usage,
    ))
}

fn paginate_timeline(
    all_events: &[AcpUiEventVm],
    event_count: usize,
    session_elapsed_seconds: Option<u64>,
    latest_permission_events: &HashMap<String, AcpUiEventVm>,
    available_commands: Option<&Vec<serde_json::Value>>,
    usage: Option<&AcpUsageVm>,
    after_seq: Option<u64>,
    before_seq: Option<u64>,
    limit: usize,
) -> Result<AcpEventScan> {
    let total = all_events.len();
    let filtered = if let Some(cursor) = after_seq {
        all_events
            .iter()
            .filter(|event| event.started_seq.unwrap_or(event.seq) > cursor)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
    } else if let Some(cursor) = before_seq {
        let mut page = all_events
            .iter()
            .filter(|event| event.started_seq.unwrap_or(event.seq) < cursor)
            .cloned()
            .collect::<Vec<_>>();
        if page.len() > limit {
            page = page.split_off(page.len() - limit);
        }
        page
    } else if total > limit {
        all_events[total - limit..].to_vec()
    } else {
        all_events.to_vec()
    };
    // Compact only the events in the final window (not all events)
    let filtered: Vec<_> = filtered
        .into_iter()
        .map(|event| {
            if matches!(event.kind.as_str(), "permissionRequest") {
                event // keep permission events as-is for pending check
            } else {
                compact_event_for_session(event)
            }
        })
        .collect();
    let oldest_seq = filtered
        .first()
        .map(|event| event.started_seq.unwrap_or(event.seq));
    let newest_seq = filtered
        .last()
        .map(|event| event.ended_seq.unwrap_or(event.seq));
    let oldest_index = oldest_seq.and_then(|seq| {
        all_events
            .iter()
            .position(|event| event.started_seq.unwrap_or(event.seq) == seq)
    });
    let newest_index = newest_seq.and_then(|seq| {
        all_events
            .iter()
            .rposition(|event| event.ended_seq.unwrap_or(event.seq) == seq)
    });
    let event_page = AcpEventPageVm {
        loaded_count: filtered.len(),
        total,
        oldest_seq,
        newest_seq,
        has_older: oldest_index.is_some_and(|index| index > 0),
        has_newer: newest_index.is_some_and(|index| index + 1 < total),
        oldest_cursor: oldest_seq.map(format_timeline_cursor),
        newest_cursor: newest_seq.map(format_timeline_cursor),
    };

    Ok(AcpEventScan {
        events: filtered,
        event_page,
        event_count,
        session_elapsed_seconds,
        latest_permission_events: latest_permission_events.clone(),
        available_commands: available_commands.cloned(),
        usage: usage.cloned(),
    })
}

fn scan_acp_events(
    path: &camino::Utf8Path,
    query: Option<AcpSessionQueryInput>,
    session_active: bool,
    default_event_limit: usize,
) -> Result<AcpEventScan> {
    const MIN_EVENT_LIMIT: usize = 1;
    const MAX_EVENT_LIMIT: usize = 1000;

    let query = query.unwrap_or(AcpSessionQueryInput {
        before_seq: None,
        after_seq: None,
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
    let file_signature = timeline_file_signature(path)?;

    // Cache via the same pool as scan_acp_timeline and invalidate on file writes.
    let cache_key = timeline_cache_key(path);
    let (
        all_events,
        raw_event_count,
        session_elapsed_seconds,
        latest_permission_events,
        available_commands,
        usage,
    ) = if !session_active {
        let mut cache = TIMELINE_CACHE.lock().unwrap();
        if let Some(cached) = cache
            .entries
            .get(&cache_key)
            .filter(|cached| cached.file_signature == file_signature)
        {
            let all_events = cached.all_events.clone();
            let raw_event_count = cached.event_count;
            let session_elapsed_seconds = cached.session_elapsed_seconds;
            let latest_permission_events = cached.latest_permission_events.clone();
            let available_commands = cached.available_commands.clone();
            let usage = cached.usage.clone();
            touch_timeline_cache(&mut cache, &cache_key);
            return paginate_timeline(
                &all_events,
                raw_event_count,
                session_elapsed_seconds,
                &latest_permission_events,
                available_commands.as_ref(),
                usage.as_ref(),
                after_seq,
                before_seq,
                limit,
            );
        }
        drop(cache);
        let result = parse_events_file(path, session_active)?;
        let mut cache = TIMELINE_CACHE.lock().unwrap();
        cache.entries.insert(
            cache_key.clone(),
            CachedTimeline {
                file_signature,
                all_events: result.0.clone(),
                event_count: result.1,
                session_elapsed_seconds: result.2,
                latest_permission_events: result.3.clone(),
                available_commands: result.4.clone(),
                usage: result.5.clone(),
            },
        );
        touch_timeline_cache(&mut cache, &cache_key);
        evict_timeline_cache(&mut cache);
        result
    } else {
        parse_events_file(path, session_active)?
    };

    paginate_timeline(
        &all_events,
        raw_event_count,
        session_elapsed_seconds,
        &latest_permission_events,
        available_commands.as_ref(),
        usage.as_ref(),
        after_seq,
        before_seq,
        limit,
    )
}

fn parse_events_file(
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
    let mut raw_event_count = 0usize;
    let mut latest_permission_events = HashMap::<String, AcpUiEventVm>::new();
    let mut available_commands = None;
    let mut usage = None;
    let mut session_elapsed = AcpSessionElapsedState::default();
    let mut all_events = Vec::<AcpUiEventVm>::new();
    let mut pending_delta: Option<AcpUiEventVm> = None;

    if path.exists() {
        let file = fs::File::open(path.as_std_path())?;
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let Ok(mut event) = serde_json::from_str::<AcpUiEventVm>(&line) else {
                continue;
            };
            raw_event_count += 1;
            event.seq = raw_event_count as u64;
            session_elapsed.observe_event(&event);
            if event.kind == "permissionRequest" {
                insert_latest_permission_event(&mut latest_permission_events, &event);
            }
            if let Some(raw) = event.raw.as_ref() {
                if is_session_update(&event, "available_commands_update") {
                    available_commands = raw
                        .get("availableCommands")
                        .and_then(|value| value.as_array())
                        .cloned();
                } else if is_session_update(&event, "usage_update") {
                    let (used, size, cost_amount) =
                        gold_band::acp::events::extract_usage_fields(raw);
                    usage = Some(AcpUsageVm {
                        used,
                        size,
                        cost_amount_usd: cost_amount,
                        ..Default::default()
                    });
                }
            }
            if is_hidden_from_chat(&event) {
                flush_pending_delta(&mut pending_delta, &mut all_events);
                continue;
            }
            if !is_session_timeline_event(&event) {
                continue;
            }
            if merge_pending_delta(&mut pending_delta, &event) {
                continue;
            }
            flush_pending_delta(&mut pending_delta, &mut all_events);
            if is_delta_event(&event) {
                pending_delta = Some(event);
            } else {
                all_events.push(event);
            }
        }
    }

    flush_pending_delta(&mut pending_delta, &mut all_events);

    Ok((
        all_events,
        raw_event_count,
        session_elapsed.finish(session_active),
        latest_permission_events,
        available_commands,
        usage,
    ))
}

fn flush_pending_delta(pending: &mut Option<AcpUiEventVm>, events: &mut Vec<AcpUiEventVm>) {
    if let Some(event) = pending.take() {
        events.push(event);
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
    let node_status = if node_path.exists() {
        read_json::<NodeState>(node_path)
            .ok()
            .map(|node| node.status)
    } else {
        None
    };
    let fused = apply_stale_session_completion_fuse_common(
        &pid_path,
        session,
        node_status
            .map(|status| status == RunStatus::Completed)
            .unwrap_or(false),
    )?;
    if fused {
        let snapshot_path = app
            .paths
            .acp_snapshot_file(task_id, run_id, round_id, node_id, attempt_id);
        let _ = write_json(&snapshot_path, &*session);
    }
    Ok(())
}

fn apply_stale_session_completion_fuse_dynamic(
    attempt_dir: &camino::Utf8Path,
    node_path: &camino::Utf8Path,
    session: &mut serde_json::Value,
) -> Result<()> {
    let pid_path = attempt_dir.join("provider.pid");
    let node_completed = if node_path.exists() {
        read_json::<gold_band::dynamic::DynamicNodeState>(node_path)
            .ok()
            .map(|node| node.status == gold_band::dynamic::DynamicNodeStatus::Completed)
            .unwrap_or(false)
    } else {
        false
    };
    let fused = apply_stale_session_completion_fuse_common(&pid_path, session, node_completed)?;
    if fused {
        let snapshot_path = attempt_dir.join("acp.snapshot.json");
        let _ = write_json(&snapshot_path, &*session);
    }
    Ok(())
}

fn apply_stale_session_completion_fuse_common(
    pid_path: &camino::Utf8Path,
    session: &mut serde_json::Value,
    node_completed: bool,
) -> Result<bool> {
    let metadata_status = session
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    if !is_acp_session_active_status(metadata_status) {
        return Ok(false);
    }
    if pid_path.exists() && !node_completed {
        return Ok(false);
    }
    if !node_completed {
        return Ok(false);
    }
    if node_completed && pid_path.exists() {
        let _ = fs::remove_file(pid_path.as_std_path());
    }
    session["status"] = serde_json::json!("completed");
    if session.get("stopReason").is_none() || session["stopReason"].is_null() {
        session["stopReason"] = serde_json::json!("end_turn");
    }
    session["updatedAt"] = serde_json::json!(current_epoch_timestamp());
    Ok(true)
}

fn parse_epoch_timestamp(value: &str) -> Option<u64> {
    value.trim_end_matches('Z').parse::<u64>().ok()
}

#[derive(Default)]
struct AcpSessionElapsedState {
    elapsed_seconds: u64,
    active_turn_started_at: Option<u64>,
    active_turn_last_event_at: Option<u64>,
    saw_turn: bool,
    pending_permission_ids: HashSet<String>,
    permission_wait_started_at: Option<u64>,
    permission_wait_seconds: u64,
}

impl AcpSessionElapsedState {
    fn observe_event(&mut self, event: &AcpUiEventVm) {
        if is_gold_band_user_prompt_event(event) {
            self.elapsed_seconds = self
                .elapsed_seconds
                .saturating_add(self.finish_current_turn(false, None));
            self.active_turn_started_at = parse_epoch_timestamp(&event.timestamp);
            self.active_turn_last_event_at = None;
            self.pending_permission_ids.clear();
            self.permission_wait_started_at = None;
            self.permission_wait_seconds = 0;
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
        self.active_turn_last_event_at = Some(timestamp);
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
            self.permission_wait_seconds
                .saturating_add(self.open_permission_wait(end_at)),
        )
    }

    fn open_permission_wait(&self, end_at: u64) -> u64 {
        self.permission_wait_started_at
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
            let was_empty = self.pending_permission_ids.is_empty();
            if self.pending_permission_ids.insert(request_id) && was_empty {
                self.permission_wait_started_at = Some(timestamp);
            }
            return;
        }
        let request_id = permission_request_id_from_event(event);
        if !self.pending_permission_ids.remove(&request_id) {
            return;
        }
        if self.pending_permission_ids.is_empty() {
            if let Some(started_at) = self.permission_wait_started_at.take() {
                self.permission_wait_seconds = self
                    .permission_wait_seconds
                    .saturating_add(timestamp.saturating_sub(started_at));
            }
        }
    }
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

fn merge_pending_delta(pending: &mut Option<AcpUiEventVm>, event: &AcpUiEventVm) -> bool {
    let Some(previous) = pending.as_mut() else {
        return false;
    };
    if !is_delta_event(event) || previous.kind != event.kind {
        return false;
    }
    previous.content = Some(format!(
        "{}{}",
        previous.content.as_deref().unwrap_or_default(),
        event.content.as_deref().unwrap_or_default()
    ));
    previous.seq = event.seq;
    previous.timestamp = event.timestamp.clone();
    previous.status = event.status.clone().or_else(|| previous.status.clone());
    previous.raw = event
        .raw
        .clone()
        .or_else(|| previous.raw.clone())
        .map(compact_raw_value);
    true
}

fn is_delta_event(event: &AcpUiEventVm) -> bool {
    matches!(event.kind.as_str(), "textDelta" | "thoughtDelta")
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
    if matches!(
        event.kind.as_str(),
        "availableCommands"
            | "usageUpdate"
            | "sessionInfo"
            | "modeUpdate"
            | "configUpdate"
            | "permissionRequest"
            | "rawDiagnostic"
    ) {
        return false;
    }
    let Some(raw) = event.raw.as_ref() else {
        return true;
    };
    let session_update = raw.get("sessionUpdate").and_then(|value| value.as_str());
    !matches!(
        session_update,
        Some(
            "available_commands_update"
                | "usage_update"
                | "session_info_update"
                | "current_mode_update"
                | "config_option_update"
        )
    )
}

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
/// Only reads the first ~200 lines — system prompt is always in session/new or session/load frame at the start.
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
        if !matches!(method, Some("session/new" | "session/load")) {
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
    event.raw = event.raw.map(compact_raw_value);
    event.content = event
        .content
        .map(|content| truncate_string(content, 64_000));
    event.title = event.title.map(|title| truncate_string(title, 2_000));
    event
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
    let models = session.get("models").cloned();
    let modes = session.get("modes").cloned();
    let config_options = session.get("configOptions").cloned();
    let current_model_id = models
        .as_ref()
        .and_then(|value| value.get("currentModelId"))
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .or_else(|| config_current_value(config_options.as_ref(), "model"));
    let current_mode_id = modes
        .as_ref()
        .and_then(|value| value.get("currentModeId"))
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .or_else(|| config_current_value(config_options.as_ref(), "mode"));
    let current_model_name = current_model_id.as_deref().and_then(|model_id| {
        model_display_name(models.as_ref(), model_id)
            .or_else(|| config_option_display_name(config_options.as_ref(), "model", model_id))
    });
    let current_mode_name = current_mode_id.as_deref().and_then(|mode_id| {
        mode_display_name(modes.as_ref(), mode_id)
            .or_else(|| config_option_display_name(config_options.as_ref(), "mode", mode_id))
    });

    if current_model_id.is_none()
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

fn insert_latest_permission_event(
    latest_permission_events: &mut HashMap<String, AcpUiEventVm>,
    event: &AcpUiEventVm,
) {
    let request_id = permission_request_id_from_event(event);
    let should_replace = latest_permission_events
        .get(&request_id)
        .map(|current| event.seq >= current.seq)
        .unwrap_or(true);
    if should_replace {
        latest_permission_events.insert(request_id, event.clone());
    }
}

fn permission_vm_from_event(event: &AcpUiEventVm) -> AcpPermissionRequestVm {
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
    AcpPermissionRequestVm {
        request_id,
        title: event
            .title
            .clone()
            .unwrap_or_else(|| "Permission required".to_string()),
        tool_call_id: event.tool_call_id.clone(),
        options,
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

    let total = count_matching_raw_frames(
        path,
        search.as_deref(),
        kind.as_deref(),
        direction.as_deref(),
    )?;
    let end = total.saturating_sub(page.saturating_mul(page_size));
    let start = total.saturating_sub((page + 1).saturating_mul(page_size));
    let items = collect_matching_raw_frames(
        path,
        search.as_deref(),
        kind.as_deref(),
        direction.as_deref(),
        start,
        end,
    )?;

    Ok(AcpRawFramePageVm {
        items,
        page,
        page_size,
        total,
        has_previous: page > 0 && total > 0,
        has_next: start > 0,
        order: "latest".to_string(),
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
        .or_else(|_| read_json::<WorkflowDsl>(&app.paths.workflow_file(task_id)))
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

pub fn mcp_server_list_vm(servers: &[gold_band::config::McpServerConfig]) -> Vec<McpServerVm> {
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
                health_status: None,
                health_message: None,
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
    use serde_json::json;

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
            raw: Some(json!({ "source": "goldBandPrompt" })),
        }
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

    fn plan_permission_event_at(request_id: &str, status: &str, timestamp: u64) -> AcpUiEventVm {
        acp_event_at(
            request_id,
            "permissionRequest",
            Some(status),
            timestamp,
            Some(json!({
                "requestId": request_id,
                "options": [
                    { "optionId": "keep-planning", "name": "继续规划", "kind": "keep_planning" },
                    { "optionId": "accept-plan", "name": "Accept plan", "kind": "accept" }
                ]
            })),
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
                "version": "0.1",
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
                        "workspace": { "mode": "readonly" },
                        "workspacePath": null,
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
                        "workspace": { "mode": "main" },
                        "workspacePath": null,
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
        let killed = runtime_display_vm(Some("completed"), Some("killed"), false, None, false);

        assert_eq!(failure.tone, "danger");
        assert!(!failure.blocking_error);
        assert!(error_blocked.blocking_error);
        assert!(killed.blocking_error);
    }

    #[test]
    fn round_graph_connects_ai_dynamic_exit_to_next_workflow_node() {
        let dir = std::env::temp_dir().join(format!(
            "gold-band-dynamic-round-graph-test-{}",
            std::process::id()
        ));
        let repo_root = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
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

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stale_session_completion_fuse_ignores_pid_when_node_completed() {
        let dir = std::env::temp_dir().join(format!(
            "gold-band-completion-fuse-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let pid_path = attempt_dir.join("provider.pid");
        fs::write(pid_path.as_std_path(), "12345").unwrap();
        let mut session = json!({ "status": "running" });

        let fused =
            apply_stale_session_completion_fuse_common(&pid_path, &mut session, true).unwrap();

        assert!(fused);
        assert_eq!(
            session.get("status").and_then(|value| value.as_str()),
            Some("completed")
        );
        assert!(!pid_path.exists());

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stale_session_completion_fuse_keeps_live_incomplete_node_running() {
        let dir =
            std::env::temp_dir().join(format!("gold-band-live-fuse-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let pid_path = attempt_dir.join("provider.pid");
        fs::write(pid_path.as_std_path(), "12345").unwrap();
        let mut session = json!({ "status": "running" });

        let fused =
            apply_stale_session_completion_fuse_common(&pid_path, &mut session, false).unwrap();

        assert!(!fused);
        assert_eq!(
            session.get("status").and_then(|value| value.as_str()),
            Some("running")
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
        let mut session = json!({ "status": "failed" });

        let fused =
            apply_stale_session_completion_fuse_common(&pid_path, &mut session, true).unwrap();

        assert!(!fused);
        assert_eq!(
            session.get("status").and_then(|value| value.as_str()),
            Some("failed")
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
    fn text_delta_still_merges_in_scan_window() {
        let mut pending = Some(test_event("textDelta", "输出你的"));
        let next = test_event("textDelta", "工具列表");

        assert!(merge_pending_delta(&mut pending, &next));
        assert_eq!(
            pending.and_then(|event| event.content),
            Some("输出你的工具列表".to_string())
        );
    }

    #[test]
    fn user_text_delta_no_longer_merges_across_prompts() {
        let mut pending = Some(test_event("userTextDelta", "输出你的工具列表"));
        let next = test_event("userTextDelta", "给我一首古诗");

        assert!(!merge_pending_delta(&mut pending, &next));
        assert_eq!(
            pending.and_then(|event| event.content),
            Some("输出你的工具列表".to_string())
        );
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
    fn session_elapsed_excludes_plan_intervention_permission_wait() {
        let elapsed = elapsed_for(
            vec![
                gold_band_prompt_at(100),
                plan_permission_event_at("plan-permission-1", "pending", 110),
                plan_permission_event_at("plan-permission-1", "selected", 160),
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
                "options": [
                    { "optionId": "allow", "name": "Allow", "kind": "allow_once" }
                ]
            })),
        );

        let vm = permission_vm_from_event(&event);

        assert_eq!(vm.request_id, "0");
        assert_eq!(
            vm.raw.get("requestId").and_then(|value| value.as_str()),
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
        assert_eq!(permission_vm_from_event(&event).request_id, "0");
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

    fn write_events_file(dir: &Utf8PathBuf, name: &str, events: &[AcpUiEventVm]) -> Utf8PathBuf {
        let path = dir.join(name);
        let mut content = String::new();
        for event in events {
            content.push_str(&serde_json::to_string(event).unwrap());
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
                raw: None,
            })
            .collect()
    }

    fn tool_event_sequence(count: usize, base_ts: u64) -> Vec<AcpUiEventVm> {
        (0..count)
            .map(|i| AcpUiEventVm {
                id: format!("tool-{i}"),
                seq: i as u64 + 1,
                timestamp: format!("{}Z", base_ts + i as u64),
                kind: "toolCall".to_string(),
                session_id: Some("s1".to_string()),
                content: Some(format!("tool {i}")),
                title: Some(format!("Tool {i}")),
                tool_call_id: Some(format!("call-{i}")),
                status: Some("completed".to_string()),
                started_seq: Some(i as u64 + 1),
                ended_seq: Some(i as u64 + 1),
                started_at: Some(format!("{}Z", base_ts + i as u64)),
                ended_at: Some(format!("{}Z", base_ts + i as u64)),
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
    fn parse_events_file_delta_merging() {
        let dir = std::env::temp_dir().join(format!("gb-ev-merge-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let db = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();

        let raw = vec![
            AcpUiEventVm {
                id: "d1".into(),
                seq: 0,
                timestamp: "100Z".into(),
                kind: "textDelta".into(),
                session_id: Some("s1".into()),
                content: Some("Hello ".into()),
                title: None,
                tool_call_id: None,
                status: Some("completed".into()),
                started_seq: None,
                ended_seq: None,
                started_at: None,
                ended_at: None,
                raw: None,
            },
            AcpUiEventVm {
                id: "d2".into(),
                seq: 0,
                timestamp: "101Z".into(),
                kind: "textDelta".into(),
                session_id: Some("s1".into()),
                content: Some("World".into()),
                title: None,
                tool_call_id: None,
                status: Some("completed".into()),
                started_seq: None,
                ended_seq: None,
                started_at: None,
                ended_at: None,
                raw: None,
            },
        ];
        let path = write_events_file(&db, "acp.events.jsonl", &raw);

        let (all_events, raw_count, _, _, _, _) = parse_events_file(&path, false).unwrap();

        assert_eq!(raw_count, 2);
        assert_eq!(all_events.len(), 1);
        assert_eq!(all_events[0].content.as_deref(), Some("Hello World"));

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
    fn scan_events_cache_hit_on_repeat() {
        let dir = std::env::temp_dir().join(format!("gb-ev-hit-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let db = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let path = write_events_file(&db, "acp.events.jsonl", &tool_event_sequence(15, 3000));

        let r1 = scan_acp_events(&path, None, false, 360).unwrap();
        let r2 = scan_acp_events(&path, None, false, 360).unwrap();

        assert_eq!(r1.events.len(), 15);
        assert_eq!(r2.events.len(), 15);
        assert_eq!(r2.event_count, r1.event_count);

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn scan_events_cache_invalidates_when_file_changes() {
        let dir = std::env::temp_dir().join(format!("gb-ev-stale-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let db = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let path = write_events_file(&db, "acp.events.jsonl", &tool_event_sequence(4, 3500));

        let r1 = scan_acp_events(&path, None, false, 360).unwrap();
        assert_eq!(r1.events.len(), 4);

        let rewritten_path =
            write_events_file(&db, "acp.events.jsonl", &tool_event_sequence(7, 3500));
        assert_eq!(rewritten_path, path);
        let r2 = scan_acp_events(&path, None, false, 360).unwrap();

        assert_eq!(r2.events.len(), 7);
        assert_eq!(r2.event_page.total, 7);
        assert_eq!(r2.events[6].content.as_deref(), Some("tool 6"));

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
