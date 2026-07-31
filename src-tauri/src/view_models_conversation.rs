use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::view_models::{
    AssetItemVm, GraphVm, RuntimeDisplayVm, acp_session_status, acp_session_vm,
    dynamic_acp_session_status, dynamic_acp_session_vm, dynamic_runtime_graph_vm,
    latest_control_failure_vm, round_detail_vm, runtime_display_vm, workflow_graph_vm,
};
use gold_band::acp::client::{PromptActivity, prompt_activity};
use gold_band::app::{App, CreateTaskInput, DEFAULT_WORKFLOW_TEMPLATE_ID, is_run_continuable};
use gold_band::config::StateConfig;
use gold_band::config::{ConversationRunMode, ManagedAgentId, managed_agent_preset};
use gold_band::domain::NodeType;
use gold_band::domain::RunStatus;
use gold_band::dsl::{
    AiDynamicAgentStrategy, AiDynamicNode, DynamicAgentRef, DynamicControlDsl, END_NODE, EdgeDsl,
    EdgeOutcome, NodeDsl, PromptEnvelopeMode, WorkerNode, WorkflowDsl,
};
use gold_band::dynamic::DynamicGraphState;
use gold_band::runtime::RunState;
use gold_band::storage::{read_json, write_json};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTaskVm {
    pub id: String,
    pub project_id: String,
    pub workspace_name: String,
    pub title: String,
    pub enabled: bool,
    pub mode: String,
    pub session_policy: String,
    pub schedule: String,
    pub schedule_label: String,
    pub timezone_label: String,
    pub next_at: Option<String>,
    pub status: String,
    pub last_trigger_at: Option<String>,
    pub last_trigger_status: Option<String>,
    pub last_trigger_label: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateScheduledTaskInputVm {
    pub project_id: String,
    pub content: String,
    pub run_mode: String,
    pub workflow_template_id: Option<String>,
    pub include_interview: Option<bool>,
    pub direct_config: Option<ConversationDirectConfigVm>,
    pub auto_config: Option<ConversationAutoConfigVm>,
    pub attachment_paths: Option<Vec<String>>,
    pub schedule: gold_band::scheduler::ScheduleSpec,
    pub overlap_policy: gold_band::scheduler::OverlapPolicy,
    pub session_policy: Option<gold_band::scheduler::SessionPolicy>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTaskEditVm {
    pub scheduled_task_id: String,
    pub project_id: String,
    pub content: String,
    pub attachment_names: Vec<String>,
    pub run_mode: String,
    pub workflow_template_id: Option<String>,
    pub include_interview: Option<bool>,
    pub direct_config: Option<ConversationDirectConfigVm>,
    pub auto_config: Option<ConversationAutoConfigVm>,
    pub schedule: gold_band::scheduler::ScheduleSpec,
    pub overlap_policy: gold_band::scheduler::OverlapPolicy,
    pub session_policy: gold_band::scheduler::SessionPolicy,
    pub direct_agent_type: Option<String>,
    pub expected_updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateScheduledTaskInputVm {
    pub scheduled_task_id: String,
    pub project_id: String,
    pub expected_updated_at: String,
    pub content: String,
    pub run_mode: String,
    pub workflow_template_id: Option<String>,
    pub include_interview: Option<bool>,
    pub direct_config: Option<ConversationDirectConfigVm>,
    pub auto_config: Option<ConversationAutoConfigVm>,
    pub attachment_paths: Option<Vec<String>>,
    pub schedule: gold_band::scheduler::ScheduleSpec,
    pub overlap_policy: gold_band::scheduler::OverlapPolicy,
    pub session_policy: gold_band::scheduler::SessionPolicy,
}

impl ScheduledTaskVm {
    pub fn from_definition(definition: &gold_band::scheduler::ScheduledTaskDefinition) -> Self {
        Self::from_definition_in_workspace(definition, &definition.project_id)
    }

    pub fn from_definition_in_workspace(
        definition: &gold_band::scheduler::ScheduledTaskDefinition,
        workspace_name: &str,
    ) -> Self {
        Self {
            id: definition.id.clone(),
            project_id: definition.project_id.clone(),
            workspace_name: workspace_name.to_string(),
            title: scheduled_task_title(&definition.instruction),
            enabled: definition.enabled,
            mode: serde_json::to_value(definition.mode)
                .ok()
                .and_then(|value| value.as_str().map(ToOwned::to_owned))
                .unwrap_or_default(),
            session_policy: serde_json::to_value(definition.session_policy)
                .ok()
                .and_then(|value| value.as_str().map(ToOwned::to_owned))
                .unwrap_or_default(),
            schedule: definition.display_schedule(),
            schedule_label: scheduled_task_schedule_label(definition),
            timezone_label: scheduled_task_timezone_label(definition),
            next_at: definition
                .schedule
                .next_occurrence_after(chrono::Utc::now())
                .map(|value| scheduled_task_datetime_label(definition, value)),
            status: scheduled_task_status(definition),
            last_trigger_at: definition.last_trigger_at.map(|value| value.to_rfc3339()),
            last_trigger_status: definition.last_trigger_status.clone(),
            last_trigger_label: scheduled_task_last_trigger_label(definition),
            created_at: definition.created_at.to_rfc3339(),
            updated_at: definition.updated_at.to_rfc3339(),
        }
    }
}

impl ScheduledTaskEditVm {
    pub fn from_definition(definition: &gold_band::scheduler::ScheduledTaskDefinition) -> Self {
        let run_mode = definition
            .execution_config
            .get("runMode")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| match definition.mode {
                gold_band::scheduler::ScheduledMode::Direct => "direct".to_string(),
                gold_band::scheduler::ScheduledMode::Workflow => "workflow".to_string(),
                gold_band::scheduler::ScheduledMode::Auto => "auto".to_string(),
            });
        let workflow_template_id = definition
            .execution_config
            .get("workflowTemplateId")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned);
        let include_interview = definition
            .execution_config
            .get("includeInterview")
            .and_then(serde_json::Value::as_bool);
        let direct_config = definition
            .execution_config
            .get("directConfig")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok());
        let auto_config = definition
            .execution_config
            .get("autoConfig")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok());
        Self {
            scheduled_task_id: definition.id.clone(),
            project_id: definition.project_id.clone(),
            content: definition.instruction.clone(),
            attachment_names: definition.attachment_names.clone(),
            run_mode,
            workflow_template_id,
            include_interview,
            direct_config,
            auto_config,
            schedule: definition.schedule.clone(),
            overlap_policy: definition.overlap_policy,
            session_policy: definition.session_policy,
            direct_agent_type: definition.content_snapshot.direct_agent_id.clone(),
            expected_updated_at: definition.updated_at.to_rfc3339(),
        }
    }
}

pub fn scheduled_task_vms_from_sources(
    sources: &[ConversationWorkspaceSource],
    project_id: Option<&str>,
) -> anyhow::Result<Vec<ScheduledTaskVm>> {
    let mut tasks = Vec::new();
    for source in sources {
        if project_id.is_some_and(|value| value != source.workspace.project_id) {
            continue;
        }
        let store = gold_band::scheduler::store::ScheduledTaskStore::new(source.app.paths.clone());
        tasks.extend(store.list()?.iter().map(|definition| {
            ScheduledTaskVm::from_definition_in_workspace(definition, &source.workspace.name)
        }));
    }
    tasks.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    Ok(tasks)
}

fn scheduled_task_last_trigger_label(
    definition: &gold_band::scheduler::ScheduledTaskDefinition,
) -> String {
    let Some(status) = definition.last_trigger_status.as_deref() else {
        return "尚未运行".to_string();
    };
    let time = definition
        .last_trigger_at
        .map(|value| scheduled_task_datetime_label(definition, value));
    let label = match status {
        "completed" => "成功",
        "running" => "运行中",
        "retrying" => "等待重试",
        "skipped" if definition.last_error.as_deref() == Some("queue-busy") => "已跳过",
        "skipped" => "已跳过",
        "failed" => "执行失败",
        _ => status,
    };
    match time {
        Some(time) => format!("{label} {time}"),
        None => label.to_string(),
    }
}

pub fn scheduled_task_title(instruction: &str) -> String {
    let first_line = instruction
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let normalized = first_line.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut title = normalized.chars().take(48).collect::<String>();
    if normalized.chars().count() > 48 {
        title.push('…');
    }
    if title.is_empty() {
        "未命名定时任务".to_string()
    } else {
        title
    }
}

fn scheduled_task_timezone(definition: &gold_band::scheduler::ScheduledTaskDefinition) -> &str {
    definition.schedule.timezone().unwrap_or("Asia/Shanghai")
}

pub fn scheduled_task_timezone_label(
    definition: &gold_band::scheduler::ScheduledTaskDefinition,
) -> String {
    match scheduled_task_timezone(definition) {
        "Asia/Shanghai" => "中国（上海）".to_string(),
        "Asia/Tokyo" => "日本（东京）".to_string(),
        "Europe/London" => "英国（伦敦）".to_string(),
        "America/New_York" => "美国（纽约）".to_string(),
        timezone => timezone.to_string(),
    }
}

fn scheduled_task_datetime_label(
    definition: &gold_band::scheduler::ScheduledTaskDefinition,
    value: chrono::DateTime<chrono::Utc>,
) -> String {
    value
        .with_timezone(
            &scheduled_task_timezone(definition)
                .parse::<chrono_tz::Tz>()
                .unwrap_or(chrono_tz::Asia::Shanghai),
        )
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

pub fn scheduled_task_schedule_label(
    definition: &gold_band::scheduler::ScheduledTaskDefinition,
) -> String {
    use gold_band::scheduler::{EveryUnit, RepeatPreset, ScheduleKind};
    match &definition.schedule.kind {
        ScheduleKind::At { at, .. } => {
            format!("单次 {}", scheduled_task_datetime_label(definition, *at))
        }
        ScheduleKind::Every { every, .. } => format!(
            "每隔 {} {}",
            every.value,
            match every.unit {
                EveryUnit::Minutes => "分钟",
                EveryUnit::Hours => "小时",
            }
        ),
        ScheduleKind::Repeat {
            preset,
            hour,
            minute,
            ..
        } => match preset {
            RepeatPreset::Hourly => "每小时".to_string(),
            RepeatPreset::Daily => format!("每天 {:02}:{:02}", hour, minute),
            RepeatPreset::Weekdays => format!("工作日 {:02}:{:02}", hour, minute),
            RepeatPreset::Weekly { weekdays } => format!(
                "每周 {} {:02}:{:02}",
                weekdays
                    .iter()
                    .map(|day| match day {
                        chrono::Weekday::Mon => "周一",
                        chrono::Weekday::Tue => "周二",
                        chrono::Weekday::Wed => "周三",
                        chrono::Weekday::Thu => "周四",
                        chrono::Weekday::Fri => "周五",
                        chrono::Weekday::Sat => "周六",
                        chrono::Weekday::Sun => "周日",
                    })
                    .collect::<Vec<_>>()
                    .join("、"),
                hour,
                minute
            ),
        },
        ScheduleKind::Cron { expression, .. } => format!("Cron {}", expression),
    }
}

fn scheduled_task_status(definition: &gold_band::scheduler::ScheduledTaskDefinition) -> String {
    if !definition.enabled {
        return "paused".to_string();
    }
    if definition.last_trigger_status.as_deref() == Some("failed") {
        return "failed".to_string();
    }
    if definition
        .schedule
        .next_occurrence_after(chrono::Utc::now())
        .is_none()
    {
        return "completed".to_string();
    }
    "enabled".to_string()
}

// ── Conversation View Models ──

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationWorkspaceVm {
    pub project_id: String,
    pub workspace_path: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSidebarVm {
    pub workspaces: Vec<ConversationWorkspaceVm>,
    pub pinned_tasks: Vec<ConversationTaskRowVm>,
    pub tasks_by_workspace: std::collections::HashMap<String, Vec<ConversationTaskRowVm>>,
    pub last_active_workspace_id: Option<String>,
    pub preferences: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTaskRowVm {
    pub project_id: String,
    pub task_id: String,
    pub title: String,
    pub auto_title: bool,
    pub run_mode: String,
    pub workflow_template_id: Option<String>,
    pub agent_identity: Option<ConversationAgentIdentityVm>,
    pub last_activity_at: Option<String>,
    pub latest_run: Option<ConversationRunSummaryVm>,
    pub runs: Vec<ConversationRunSummaryVm>,
    pub pinned: bool,
    pub pinned_order: Option<usize>,
    pub scheduled_task_id: Option<String>,
}

pub struct ConversationWorkspaceSource {
    pub workspace: ConversationWorkspaceVm,
    pub app: App,
}

pub fn conversation_workspace_vms(state: &StateConfig) -> Vec<ConversationWorkspaceVm> {
    let mut workspaces = state
        .conversation_workspaces
        .iter()
        .map(|workspace| ConversationWorkspaceVm {
            project_id: workspace.project_id.clone(),
            workspace_path: workspace.workspace_path.clone(),
            name: workspace.name.clone(),
        })
        .collect::<Vec<_>>();
    if let Some(last_workspace) = &state.last_conversation_workspace {
        workspaces.sort_by_key(|workspace| usize::from(workspace.project_id != *last_workspace));
    }
    workspaces
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRunSummaryVm {
    pub run_id: String,
    pub status: String,
    pub outcome: Option<String>,
    pub started_at: String,
    pub updated_at: String,
    pub current_round: Option<String>,
    pub current_node: Option<String>,
    pub resumable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRunVm {
    pub project_id: String,
    pub task_id: String,
    pub task_uuid: Option<String>,
    pub run_id: String,
    pub title: String,
    pub auto_title: bool,
    pub run_mode: String,
    pub workflow_template_id: Option<String>,
    pub direct_config: Option<ConversationDirectConfigVm>,
    pub agent_identity: Option<ConversationAgentIdentityVm>,
    pub last_activity_at: Option<String>,
    pub run_status: String,
    pub run_outcome: Option<String>,
    pub session_tree: ConversationSessionTreeVm,
    pub selected_session: Option<crate::view_models::AcpSessionVm>,
    pub active_sessions: Vec<ConversationActiveSessionVm>,
    pub artifacts: Vec<crate::view_models::AssetItemVm>,
    pub attachments: Vec<crate::view_models::AssetItemVm>,
    pub input_attachments: Vec<crate::view_models::AssetItemVm>,
    pub workflow_status: String,
    pub workflow_valid: bool,
    pub workflow_error: Option<crate::view_models::WorkflowErrorVm>,
    pub workflow_json: Option<String>,
    pub workflow_graph: GraphVm,
    pub resumable: bool,
    pub pause_reason: Option<String>,
    pub runtime_error_message: Option<String>,
    pub scheduled_task_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSessionSwitchVm {
    pub selected_session: Option<crate::view_models::AcpSessionVm>,
    pub artifacts: Vec<crate::view_models::AssetItemVm>,
    pub attachments: Vec<crate::view_models::AssetItemVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSessionTreeVm {
    pub rounds: Vec<ConversationRoundNodeVm>,
    pub selected_session_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRoundNodeVm {
    pub round_id: String,
    pub index: u32,
    pub label: String,
    pub status: String,
    pub runtime_display: RuntimeDisplayVm,
    pub nodes: Vec<ConversationTreeNodeVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTreeNodeVm {
    pub node_id: String,
    pub label: String,
    pub node_type: String,
    pub status: String,
    pub runtime_display: RuntimeDisplayVm,
    pub attempts: Vec<ConversationSessionLeafVm>,
    pub outer_nodes: Option<Vec<ConversationTreeNodeVm>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSessionLeafVm {
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
    pub outer_node_id: Option<String>,
    pub outer_attempt_id: Option<String>,
    pub path_label: String,
    pub status: String,
    pub outcome: Option<String>,
    pub runtime_display: RuntimeDisplayVm,
    pub lifecycle: ConversationAttemptLifecycleVm,
    pub current: bool,
    pub manual_check_pending: bool,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub session_id: Option<String>,
    pub artifact_count: usize,
    pub attachment_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationAttemptLifecycleVm {
    pub runtime: ConversationRuntimeFacetVm,
    pub acp: ConversationAcpFacetVm,
    pub display_status: String,
    pub runtime_display: RuntimeDisplayVm,
    pub continue_kind: Option<String>,
    pub composer: ConversationComposerVm,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRuntimeFacetVm {
    pub status: String,
    pub outcome: Option<String>,
    pub pause_reason: Option<String>,
    pub resumable: bool,
    pub current: bool,
    pub active: bool,
    pub continuable: bool,
    pub phase: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationAcpFacetVm {
    pub status: Option<String>,
    pub phase: Option<String>,
    pub active: bool,
    pub stopping: bool,
    pub terminal: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationComposerVm {
    pub mode: String,
    pub submit_target: String,
    pub processing_kind: String,
    pub status_key: Option<String>,
    pub can_stop: bool,
    pub lock_input: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationActiveSessionVm {
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
    pub outer_node_id: Option<String>,
    pub outer_attempt_id: Option<String>,
    pub path_label: String,
    pub status: String,
    pub runtime_display: RuntimeDisplayVm,
    pub lifecycle: ConversationAttemptLifecycleVm,
    pub manual_check_pending: bool,
    pub session_id: Option<String>,
    pub started_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRunModeVm {
    pub mode: String,
    pub workflow_template_id: Option<String>,
    pub include_interview: Option<bool>,
    pub direct_config: Option<ConversationDirectConfigVm>,
    pub direct_preferences: HashMap<String, ConversationDirectConfigVm>,
    pub auto_config: Option<ConversationAutoConfigVm>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDirectConfigVm {
    pub agent_type: String,
    pub model_id: Option<String>,
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config_options: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationAgentIdentityVm {
    pub agent_type: String,
    pub display_name: String,
    pub icon_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationAutoConfigVm {
    pub agent_strategy: Option<String>,
    pub agent_type: String,
    pub bootstrap_agent_type: Option<String>,
    pub bootstrap_model_id: Option<String>,
    pub acceptance_model_id: Option<String>,
    pub model_id: Option<String>,
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config_options: BTreeMap<String, String>,
    pub available_agents: Option<Vec<ConversationDynamicAgentRefVm>>,
    pub routing_prompt: Option<String>,
    pub allowed_workflows: Option<Vec<ConversationAllowedWorkflowRefVm>>,
    pub allowed_profiles: Option<Vec<String>>,
    pub global_goal: Option<String>,
    pub control: Option<ConversationDynamicControlVm>,
    pub active_template_id: Option<String>,
    pub active_template_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDynamicAgentRefVm {
    pub provider: String,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationAllowedWorkflowRefVm {
    pub workflow_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDynamicControlVm {
    pub max_dynamic_nodes: u32,
    pub max_fanout: u32,
    pub max_depth: u32,
    pub max_parallel: u32,
    pub max_group_depth: u32,
    pub max_workflow_invocations: u32,
    pub allow_nested_dynamic: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationCreateInputVm {
    pub project_id: String,
    pub content: String,
    pub run_mode: String,
    pub workflow_template_id: Option<String>,
    pub include_interview: Option<bool>,
    pub direct_config: Option<ConversationDirectConfigVm>,
    pub auto_config: Option<ConversationAutoConfigVm>,
    pub attachment_paths: Option<Vec<String>>,
    #[serde(default)]
    pub scheduled_task_id: Option<String>,
    #[serde(default)]
    pub scheduled_content_fingerprint: Option<String>,
}

pub fn scheduled_content_snapshot(
    app: &App,
    input: &ConversationCreateInputVm,
) -> anyhow::Result<gold_band::scheduler::ScheduledTaskContentSnapshot> {
    use gold_band::scheduler::{AutoAuthoringIdentity, ScheduledMode};

    let mode = match input.run_mode.as_str() {
        "direct" => ScheduledMode::Direct,
        "workflow" => ScheduledMode::Workflow,
        "auto" => ScheduledMode::Auto,
        other => anyhow::bail!("unsupported scheduled task mode: {other}"),
    };
    let attachment_hashes = input
        .attachment_paths
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|path| gold_band::scheduler::fingerprint::attachment_file_hash(Path::new(path)))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let mut snapshot = gold_band::scheduler::ScheduledTaskContentInput::new(
        mode,
        input.content.clone(),
        attachment_hashes,
        input.project_id.clone(),
    );

    match mode {
        ScheduledMode::Direct => {
            snapshot.direct_agent_id = Some(
                input
                    .direct_config
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("direct config is required"))?
                    .agent_type
                    .trim()
                    .to_string(),
            );
        }
        ScheduledMode::Workflow => {
            let store = app.workflow_templates()?;
            let template_id = input
                .workflow_template_id
                .as_deref()
                .unwrap_or(DEFAULT_WORKFLOW_TEMPLATE_ID);
            let mut workflow = store
                .templates
                .iter()
                .find(|template| template.id == template_id)
                .map(|template| template.workflow.clone())
                .ok_or_else(|| anyhow::anyhow!("workflow template not found: {template_id}"))?;
            apply_workflow_interview_preference(
                template_id,
                input.include_interview,
                &mut workflow,
            )?;
            snapshot.workflow_authoring = Some(serde_json::to_value(workflow)?);
        }
        ScheduledMode::Auto => {
            let config = input
                .auto_config
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("auto config is required"))?;
            let available_agent_types = config
                .available_agents
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|agent| agent.provider.clone());
            let allowed_workflow_ids = config
                .allowed_workflows
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|workflow| workflow.workflow_id.clone());
            snapshot.auto_authoring = Some(AutoAuthoringIdentity::new(
                config
                    .agent_strategy
                    .clone()
                    .unwrap_or_else(|| "fixed".to_string()),
                config.agent_type.clone(),
                config.bootstrap_agent_type.clone(),
                available_agent_types,
                config.global_goal.clone(),
                allowed_workflow_ids,
            ));
        }
    }

    Ok(snapshot)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationValidationResultVm {
    pub valid: bool,
    pub missing_items: Vec<ConversationMissingItemVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMissingItemVm {
    pub code: String,
    pub label: String,
    pub recovery_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSearchResultVm {
    pub project_id: String,
    pub workspace_path: String,
    pub workspace_name: String,
    pub task_id: String,
    pub title: String,
    pub description: Option<String>,
    pub requirement_preview: String,
    pub match_preview: String,
    pub latest_run: Option<ConversationRunSummaryVm>,
    pub run_mode: String,
    pub agent_identity: Option<ConversationAgentIdentityVm>,
    pub last_activity_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConversationMetadata {
    pub(crate) version: String,
    pub(crate) source: String,
    pub(crate) run_mode: String,
    pub(crate) workflow_template_id: Option<String>,
    pub(crate) include_interview: Option<bool>,
    pub(crate) direct_config: Option<ConversationDirectConfigVm>,
    pub(crate) agent_identity: Option<ConversationAgentIdentityVm>,
    pub(crate) title_auto_generated: bool,
    pub(crate) initial_attachment_names: Option<Vec<String>>,
    pub(crate) created_at: String,
    pub(crate) last_activity_at: Option<String>,
    #[serde(default)]
    pub(crate) scheduled_task_id: Option<String>,
    #[serde(default)]
    pub(crate) scheduled_content_fingerprint: Option<String>,
}

fn read_conversation_metadata(app: &App, task_id: &str) -> Option<ConversationMetadata> {
    read_json::<ConversationMetadata>(
        &app.paths
            .task_dir(task_id)
            .join("authoring")
            .join("conversation.json"),
    )
    .ok()
}

pub(crate) fn scheduled_content_fingerprint_for_task(app: &App, task_id: &str) -> Option<String> {
    read_conversation_metadata(app, task_id)
        .and_then(|metadata| metadata.scheduled_content_fingerprint)
}

fn direct_agent_identity(app: &App, agent_type: &str) -> Option<ConversationAgentIdentityVm> {
    let agent_id = ManagedAgentId::from_str(agent_type).ok()?;
    let (_, config) = app.managed_agent(agent_type).ok()?;
    let icon_key = managed_agent_preset(&agent_id)?.icon_key;
    Some(ConversationAgentIdentityVm {
        agent_type: agent_type.to_string(),
        display_name: config.adapter.display_name.clone(),
        icon_key: icon_key.to_string(),
    })
}

pub fn touch_conversation_activity(app: &App, task_id: &str) -> anyhow::Result<()> {
    let Some(mut metadata) = read_conversation_metadata(app, task_id) else {
        return Ok(());
    };
    metadata.last_activity_at = Some(chrono::Utc::now().to_rfc3339());
    write_json(
        &app.paths
            .task_dir(task_id)
            .join("authoring")
            .join("conversation.json"),
        &metadata,
    )
}

fn conversation_timestamp_millis(value: &str) -> Option<i64> {
    let trimmed = value.trim();
    let epoch = trimmed.strip_suffix('Z').unwrap_or(trimmed);
    if let Ok(seconds) = epoch.parse::<f64>() {
        return Some((seconds * 1_000.0) as i64);
    }
    if let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        return Some(timestamp.timestamp_millis());
    }
    chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|timestamp| timestamp.and_utc().timestamp_millis())
}

fn compare_conversation_timestamps(left: &str, right: &str) -> Ordering {
    match (
        conversation_timestamp_millis(left),
        conversation_timestamp_millis(right),
    ) {
        (Some(left_millis), Some(right_millis)) => {
            left_millis.cmp(&right_millis).then_with(|| left.cmp(right))
        }
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => left.cmp(right),
    }
}

fn latest_conversation_activity_at(
    metadata: Option<&ConversationMetadata>,
    latest_run: Option<&ConversationRunSummaryVm>,
) -> Option<String> {
    [
        metadata.and_then(|metadata| metadata.last_activity_at.as_deref()),
        metadata.map(|metadata| metadata.created_at.as_str()),
        latest_run.map(|run| run.updated_at.as_str()),
    ]
    .into_iter()
    .flatten()
    .max_by(|left, right| compare_conversation_timestamps(left, right))
    .map(str::to_owned)
}

// ── Builder functions (stubs — full implementation in later phases) ──

pub fn conversation_sidebar_vm_from_sources(
    state: &StateConfig,
    sources: &[ConversationWorkspaceSource],
) -> ConversationSidebarVm {
    let mut workspaces = sources
        .iter()
        .map(|source| source.workspace.clone())
        .collect::<Vec<_>>();
    if let Some(last_workspace) = &state.last_conversation_workspace {
        workspaces.sort_by_key(|workspace| usize::from(workspace.project_id != *last_workspace));
    }
    let mut pinned_tasks: Vec<ConversationTaskRowVm> = Vec::new();
    let mut tasks_by_workspace: HashMap<String, Vec<ConversationTaskRowVm>> = HashMap::new();
    let pinned_set: std::collections::HashSet<(String, String)> = state
        .conversation_pins
        .iter()
        .map(|p| (p.project_id.clone(), p.task_id.clone()))
        .collect();

    for ws in &workspaces {
        tasks_by_workspace.entry(ws.project_id.clone()).or_default();
    }

    for source in sources {
        if let Ok(tasks) = source.app.task_list() {
            for task in tasks {
                let task_id = &task.id;
                let project_id = &source.workspace.project_id;
                let pinned = pinned_set.contains(&(project_id.clone(), task_id.clone()));
                let pin_order = state
                    .conversation_pins
                    .iter()
                    .find(|p| p.project_id == *project_id && p.task_id == *task_id)
                    .map(|p| p.order);

                let metadata = read_conversation_metadata(&source.app, task_id);
                let run_mode = metadata
                    .as_ref()
                    .map(|metadata| metadata.run_mode.clone())
                    .unwrap_or_else(|| "workflow".to_string());

                let run_list = source.app.run_list(task_id).unwrap_or_default();
                let mut runs: Vec<ConversationRunSummaryVm> =
                    run_list.iter().map(conversation_run_summary_vm).collect();
                runs.sort_by(|left, right| {
                    compare_conversation_timestamps(&right.updated_at, &left.updated_at)
                        .then_with(|| {
                            compare_conversation_timestamps(&right.started_at, &left.started_at)
                        })
                        .then_with(|| right.run_id.cmp(&left.run_id))
                });
                let latest_run = runs.first().cloned();
                let last_activity_at =
                    latest_conversation_activity_at(metadata.as_ref(), latest_run.as_ref());

                let row = ConversationTaskRowVm {
                    project_id: project_id.clone(),
                    task_id: task_id.clone(),
                    title: task.title.clone().unwrap_or_else(|| task_id.clone()),
                    auto_title: metadata
                        .as_ref()
                        .is_some_and(|metadata| metadata.title_auto_generated),
                    run_mode,
                    workflow_template_id: None,
                    agent_identity: metadata
                        .as_ref()
                        .and_then(|metadata| metadata.agent_identity.clone()),
                    last_activity_at,
                    latest_run,
                    runs,
                    pinned,
                    pinned_order: pin_order,
                    scheduled_task_id: metadata
                        .as_ref()
                        .and_then(|metadata| metadata.scheduled_task_id.clone()),
                };

                if pinned {
                    pinned_tasks.push(row.clone());
                }
                tasks_by_workspace
                    .entry(project_id.clone())
                    .or_default()
                    .push(row);
            }
        }
    }

    pinned_tasks.sort_by_key(|t| t.pinned_order.unwrap_or(usize::MAX));
    for tasks in tasks_by_workspace.values_mut() {
        tasks.sort_by(|a, b| {
            match (a.last_activity_at.as_deref(), b.last_activity_at.as_deref()) {
                (Some(a_time), Some(b_time)) => compare_conversation_timestamps(b_time, a_time),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            }
            .then_with(|| b.task_id.cmp(&a.task_id))
        });
    }

    let last_active_workspace_id = state
        .last_conversation_workspace
        .clone()
        .or_else(|| workspaces.first().map(|w| w.project_id.clone()));

    ConversationSidebarVm {
        workspaces,
        pinned_tasks,
        tasks_by_workspace,
        last_active_workspace_id,
        preferences: state.preferences.clone(),
    }
}

fn enum_label<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(label)) => label,
        Ok(value) => value.to_string(),
        Err(_) => "unknown".to_string(),
    }
}

pub(crate) fn conversation_run_summary_vm(run: &RunState) -> ConversationRunSummaryVm {
    ConversationRunSummaryVm {
        run_id: run.id.clone(),
        status: enum_label(&run.status),
        outcome: run.outcome.map(|outcome| enum_label(&outcome)),
        started_at: run.started_at.clone(),
        updated_at: run.updated_at.clone(),
        current_round: run.current_round.clone(),
        current_node: run.current_node.clone(),
        resumable: is_run_continuable(run),
    }
}

fn display_pause_reason_for_attempt(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    run_pause_reason: Option<&str>,
) -> Option<String> {
    if run_pause_reason.is_some_and(|reason| {
        normalize_lifecycle_code(reason) == "error-blocked"
            || is_runtime_continue_pause_reason(Some(reason))
    }) {
        return run_pause_reason.map(str::to_string);
    }
    let snapshot_path = app
        .paths
        .acp_snapshot_file(task_id, run_id, round_id, node_id, attempt_id);
    let session_path = app
        .paths
        .acp_session_file(task_id, run_id, round_id, node_id, attempt_id);
    if acp_session_file_is_cancelled(&snapshot_path) || acp_session_file_is_cancelled(&session_path)
    {
        return Some("process-interrupted".to_string());
    }
    run_pause_reason.map(str::to_string)
}

fn display_pause_reason_for_dynamic_attempt(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    node_id: &str,
    attempt_id: &str,
    dynamic_node: &gold_band::dynamic::DynamicNodeState,
    run_pause_reason: Option<&str>,
) -> Option<String> {
    if let Some(pause_reason) = dynamic_node.pause_reason.as_ref() {
        return Some(enum_label(pause_reason));
    }
    if run_pause_reason.is_some_and(|reason| {
        normalize_lifecycle_code(reason) == "error-blocked"
            || is_runtime_continue_pause_reason(Some(reason))
    }) {
        return run_pause_reason.map(str::to_string);
    }
    let attempt_dir = app.paths.dynamic_node_attempt_dir(
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        node_id,
        attempt_id,
    );
    if acp_session_file_is_cancelled(&attempt_dir.join("acp.snapshot.json"))
        || acp_session_file_is_cancelled(&attempt_dir.join("acp.session.json"))
    {
        return Some("process-interrupted".to_string());
    }
    run_pause_reason.map(str::to_string)
}

fn acp_session_file_is_cancelled(path: &camino::Utf8Path) -> bool {
    read_json::<serde_json::Value>(path)
        .ok()
        .and_then(|session| {
            let status = session
                .get("status")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let stop_reason = session
                .get("stopReason")
                .or_else(|| session.get("stop_reason"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            (status.eq_ignore_ascii_case("cancelled")
                || status.eq_ignore_ascii_case("canceled")
                || stop_reason.eq_ignore_ascii_case("cancelled")
                || stop_reason.eq_ignore_ascii_case("canceled"))
            .then_some(())
        })
        .is_some()
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

fn default_dynamic_attempt_id() -> String {
    "attempt-001".to_string()
}

fn list_file_names_from_dir(
    dir: &camino::Utf8Path,
    logical_json_name: bool,
) -> anyhow::Result<Vec<String>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut names = std::fs::read_dir(dir.as_std_path())?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|ty| ty.is_file()).unwrap_or(false))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .map(|name| {
            if logical_json_name {
                name.strip_suffix(".json").unwrap_or(&name).to_string()
            } else {
                name
            }
        })
        .collect::<Vec<_>>();
    names.sort();
    Ok(names)
}

fn conversation_session_assets(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<&str>,
    outer_attempt_id: Option<&str>,
) -> anyhow::Result<(Vec<AssetItemVm>, Vec<AssetItemVm>)> {
    let (artifact_names, attachment_names) =
        if let (Some(outer_node_id), Some(outer_attempt_id)) = (outer_node_id, outer_attempt_id) {
            let artifacts_dir = app.paths.dynamic_node_artifacts_dir(
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
                node_id,
                attempt_id,
            );
            let attachments_dir = app.paths.dynamic_node_attachments_dir(
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
                node_id,
                attempt_id,
            );
            (
                list_file_names_from_dir(&artifacts_dir, true)?,
                list_file_names_from_dir(&attachments_dir, false)?,
            )
        } else {
            (
                app.artifact_list(task_id, run_id, round_id, node_id, attempt_id)?,
                app.attachment_list(task_id, run_id, round_id, node_id, attempt_id)?,
            )
        };

    let artifacts = artifact_names
        .into_iter()
        .map(|name| asset_item_vm("artifact", round_id, node_id, attempt_id, name))
        .collect::<Vec<_>>();
    let attachments = attachment_names
        .into_iter()
        .map(|name| asset_item_vm("attachment", round_id, node_id, attempt_id, name))
        .collect::<Vec<_>>();
    Ok((artifacts, attachments))
}

fn find_leaf_by_key(
    rounds: &[ConversationRoundNodeVm],
    key: &str,
) -> Option<ConversationSessionLeafVm> {
    for round in rounds {
        for node in &round.nodes {
            // Check top-level attempts
            for leaf in &node.attempts {
                if format!("{}/{}/{}", leaf.round_id, leaf.node_id, leaf.attempt_id) == key {
                    return Some(leaf.clone());
                }
                if leaf.outer_node_id.is_some() {
                    let outer_key = format!(
                        "{}/{}/{}/{}/{}",
                        leaf.round_id,
                        leaf.outer_node_id.as_deref().unwrap_or(""),
                        leaf.outer_attempt_id.as_deref().unwrap_or(""),
                        leaf.node_id,
                        leaf.attempt_id,
                    );
                    if outer_key == key {
                        return Some(leaf.clone());
                    }
                }
            }
            // Check dynamic child nodes
            if let Some(ref outer_nodes) = node.outer_nodes {
                for on in outer_nodes {
                    for leaf in &on.attempts {
                        if let (Some(outer_id), Some(outer_attempt)) = (
                            leaf.outer_node_id.as_deref(),
                            leaf.outer_attempt_id.as_deref(),
                        ) {
                            let dyn_key = format!(
                                "{}/{}/{}/{}/{}",
                                leaf.round_id,
                                outer_id,
                                outer_attempt,
                                leaf.node_id,
                                leaf.attempt_id,
                            );
                            if dyn_key == key {
                                return Some(leaf.clone());
                            }
                        }
                        if format!("{}/{}/{}", leaf.round_id, leaf.node_id, leaf.attempt_id) == key
                        {
                            return Some(leaf.clone());
                        }
                    }
                }
            }
        }
    }
    None
}

fn latest_session_leaf(rounds: &[ConversationRoundNodeVm]) -> Option<ConversationSessionLeafVm> {
    let mut latest: Option<ConversationSessionLeafVm> = None;
    for round in rounds {
        for node in &round.nodes {
            for leaf in &node.attempts {
                if is_leaf_newer(leaf, latest.as_ref()) {
                    latest = Some(leaf.clone());
                }
            }
            if let Some(ref outer_nodes) = node.outer_nodes {
                for outer_node in outer_nodes {
                    for leaf in &outer_node.attempts {
                        if is_leaf_newer(leaf, latest.as_ref()) {
                            latest = Some(leaf.clone());
                        }
                    }
                }
            }
        }
    }
    latest
}

fn current_session_leaf(rounds: &[ConversationRoundNodeVm]) -> Option<ConversationSessionLeafVm> {
    for round in rounds {
        for node in &round.nodes {
            for leaf in &node.attempts {
                if leaf.current {
                    return Some(leaf.clone());
                }
            }
            if let Some(ref outer_nodes) = node.outer_nodes {
                for outer_node in outer_nodes {
                    for leaf in &outer_node.attempts {
                        if leaf.current {
                            return Some(leaf.clone());
                        }
                    }
                }
            }
        }
    }
    None
}

fn active_session_leaf(rounds: &[ConversationRoundNodeVm]) -> Option<ConversationSessionLeafVm> {
    for round in rounds {
        for node in &round.nodes {
            for leaf in &node.attempts {
                if is_active_session_status(&leaf.status) {
                    return Some(leaf.clone());
                }
            }
            if let Some(ref outer_nodes) = node.outer_nodes {
                for outer_node in outer_nodes {
                    for leaf in &outer_node.attempts {
                        if is_active_session_status(&leaf.status) {
                            return Some(leaf.clone());
                        }
                    }
                }
            }
        }
    }
    None
}

fn default_session_leaf(rounds: &[ConversationRoundNodeVm]) -> Option<ConversationSessionLeafVm> {
    if let Some(leaf) = current_session_leaf(rounds) {
        return Some(leaf);
    }
    if let Some(leaf) = active_session_leaf(rounds) {
        return Some(leaf);
    }
    latest_session_leaf(rounds)
}

fn normalize_lifecycle_code(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('_', "-")
}

fn runtime_error_message(
    app: &App,
    task_id: &str,
    run_id: &str,
    pause_reason: Option<&str>,
    run_outcome: Option<&str>,
) -> Option<String> {
    if pause_reason.map(normalize_lifecycle_code).as_deref() == Some("error-blocked") {
        let progress = app.run_progress(task_id, run_id).ok().flatten()?;
        return runtime_error_message_from_summary(progress.get("summary")?.as_str()?);
    }

    if !matches!(
        run_outcome.map(normalize_lifecycle_code).as_deref(),
        Some("failure" | "failed" | "error")
    ) {
        return None;
    }

    latest_control_failure_vm(app, task_id, run_id)
        .ok()
        .flatten()
        .map(|failure| {
            if failure.title.trim().is_empty() || failure.message.trim().is_empty() {
                failure.message
            } else {
                format!("{}：{}", failure.title, failure.message)
            }
        })
}

fn dynamic_leaf_runtime_error_message(
    app: &App,
    task_id: &str,
    run_id: &str,
    leaf: &ConversationSessionLeafVm,
) -> Option<String> {
    let (outer_node_id, outer_attempt_id) = leaf
        .outer_node_id
        .as_deref()
        .zip(leaf.outer_attempt_id.as_deref())?;
    let graph_path = app.paths.dynamic_graph_file(
        task_id,
        run_id,
        &leaf.round_id,
        outer_node_id,
        outer_attempt_id,
    );
    let graph = read_json::<DynamicGraphState>(&graph_path).ok()?;
    let diagnostic = graph
        .nodes
        .iter()
        .find(|node| node.id == leaf.node_id)?
        .runtime_error
        .as_ref()?
        .diagnostic
        .trim();
    (!diagnostic.is_empty()).then(|| format_runtime_error_reason(diagnostic))
}

fn runtime_error_message_from_summary(summary: &str) -> Option<String> {
    let summary = summary.trim();
    if summary.is_empty() {
        return None;
    }
    let reason = summary
        .split_once(" blocked at ")
        .and_then(|(_, blocked)| blocked.split_once(": ").map(|(_, reason)| reason.trim()))
        .filter(|reason| !reason.is_empty())
        .unwrap_or(summary);
    Some(format_runtime_error_reason(reason))
}

fn format_runtime_error_reason(reason: &str) -> String {
    let reason = reason.trim();
    let Some((json_start, error_value)) = find_embedded_json_object(reason) else {
        return reason.to_string();
    };
    let formatted = format_json_error_payload(&error_value);
    if json_start == 0 {
        return formatted;
    }
    let prefix = reason[..json_start].trim_end();
    if prefix.ends_with(':') {
        format!("{prefix} {formatted}")
    } else {
        format!("{prefix}: {formatted}")
    }
}

fn find_embedded_json_object(text: &str) -> Option<(usize, serde_json::Value)> {
    text.char_indices()
        .filter(|(_, ch)| *ch == '{')
        .find_map(|(index, _)| {
            serde_json::from_str(&text[index..])
                .ok()
                .map(|value| (index, value))
        })
}

fn format_json_error_payload(value: &serde_json::Value) -> String {
    let details = value
        .pointer("/data/details")
        .and_then(serde_json::Value::as_str)
        .or_else(|| value.get("details").and_then(serde_json::Value::as_str));
    let message = value.get("message").and_then(serde_json::Value::as_str);
    let code = value.get("code").map(|code| match code {
        serde_json::Value::String(code) => code.clone(),
        other => other.to_string(),
    });

    match (details, message, code.as_deref()) {
        (Some(details), Some(message), _) if details != message => format!("{details} ({message})"),
        (Some(details), _, _) => details.to_string(),
        (None, Some(message), Some(code)) => format!("{message} ({code})"),
        (None, Some(message), None) => message.to_string(),
        _ => value.to_string(),
    }
}

fn is_active_session_status(status: &str) -> bool {
    matches!(
        normalize_lifecycle_code(status).as_str(),
        "pending"
            | "ready"
            | "running"
            | "in-progress"
            | "active"
            | "sending"
            | "cancelling"
            | "cancel-requested"
    )
}

fn is_stopping_session_status(status: &str) -> bool {
    matches!(
        normalize_lifecycle_code(status).as_str(),
        "cancelling" | "cancel-requested"
    )
}

fn is_terminal_session_status(status: &str) -> bool {
    matches!(
        normalize_lifecycle_code(status).as_str(),
        "completed"
            | "complete"
            | "cancelled"
            | "canceled"
            | "failed"
            | "failure"
            | "error"
            | "killed"
    )
}

fn is_runtime_continue_pause_reason(pause_reason: Option<&str>) -> bool {
    matches!(
        pause_reason.map(normalize_lifecycle_code).as_deref(),
        Some("process-interrupted" | "runtime-abnormal")
    )
}

fn runtime_continue_kind(
    runtime_status: &str,
    pause_reason: Option<&str>,
    runtime_resumable: bool,
    manual_check_pending: bool,
) -> Option<String> {
    if manual_check_pending
        || !runtime_resumable
        || normalize_lifecycle_code(runtime_status) != "paused"
    {
        return None;
    }
    match pause_reason.map(normalize_lifecycle_code).as_deref() {
        Some("process-interrupted" | "runtime-abnormal") => Some("input".to_string()),
        _ => None,
    }
}

fn runtime_phase_for_lifecycle(
    runtime_status: &str,
    runtime_active: bool,
    runtime_terminal: bool,
    acp_status: Option<&str>,
    acp_active: bool,
    acp_stopping: bool,
    acp_terminal: bool,
) -> String {
    if runtime_terminal && (acp_active || acp_stopping) {
        let session_status = acp_status.map(normalize_lifecycle_code);
        return if matches!(session_status.as_deref(), Some("pending" | "starting")) {
            "launching-session".to_string()
        } else {
            "provider-running".to_string()
        };
    }
    if runtime_terminal && !acp_active && !acp_stopping {
        return "terminal".to_string();
    }
    if normalize_lifecycle_code(runtime_status) == "paused" {
        return "paused".to_string();
    }
    if runtime_active && acp_terminal {
        let session_status = acp_status.map(normalize_lifecycle_code);
        return if matches!(session_status.as_deref(), Some("completed" | "complete")) {
            "launching-next-node".to_string()
        } else {
            "provider-running".to_string()
        };
    }
    if runtime_active {
        let session_status = acp_status.map(normalize_lifecycle_code);
        if !acp_active || matches!(session_status.as_deref(), Some("pending")) {
            return "launching-session".to_string();
        }
        return "provider-running".to_string();
    }
    "idle".to_string()
}

fn composer_for_lifecycle(
    runtime_phase: &str,
    runtime_active: bool,
    acp_active: bool,
    acp_stopping: bool,
    continue_kind: Option<&str>,
    runtime_display: &RuntimeDisplayVm,
) -> ConversationComposerVm {
    let mode = if acp_stopping {
        "stopping"
    } else if runtime_active || acp_active {
        "runtime-active"
    } else if continue_kind == Some("input") {
        "interrupted-input"
    } else if runtime_display.blocking_error {
        "runtime-error"
    } else {
        "normal"
    };
    let submit_target = match mode {
        "interrupted-input" => "runtime-continue",
        "normal" => "acp-prompt",
        _ => "none",
    };
    let processing_kind = match mode {
        "stopping" => "stopping",
        "runtime-active" if runtime_phase == "launching-next-node" => "launching-next-node",
        "runtime-active" if runtime_phase == "launching-session" => "launching",
        "runtime-active" => "processing",
        _ => "processing",
    };
    let status_key = match mode {
        "stopping" => Some("acp.stopping"),
        "runtime-active" if runtime_phase == "launching-next-node" => {
            Some("conversation.runtime.launchingNextNode")
        }
        "runtime-active" => Some("conversation.runtime.runtimeActive"),
        _ => None,
    };

    ConversationComposerVm {
        mode: mode.to_string(),
        submit_target: submit_target.to_string(),
        processing_kind: processing_kind.to_string(),
        status_key: status_key.map(str::to_string),
        can_stop: runtime_active || acp_active || acp_stopping,
        lock_input: !matches!(mode, "normal" | "interrupted-input"),
    }
}

fn derive_conversation_attempt_lifecycle(
    session_status: Option<&str>,
    prompt_activity: Option<PromptActivity>,
    runtime_status: &str,
    runtime_outcome: Option<&str>,
    current: bool,
    pause_reason: Option<&str>,
    runtime_resumable: bool,
    manual_check_pending: bool,
) -> ConversationAttemptLifecycleVm {
    let session_status = session_status
        .map(str::trim)
        .filter(|status| !status.is_empty() && !status.eq_ignore_ascii_case("unknown"))
        .map(str::to_string);
    let normalized_runtime_status = normalize_lifecycle_code(runtime_status);
    let runtime_paused = normalized_runtime_status == "paused";
    let unexplained_provider_failure = runtime_paused
        && runtime_outcome.is_none()
        && pause_reason.is_none()
        && matches!(
            session_status
                .as_deref()
                .map(normalize_lifecycle_code)
                .as_deref(),
            Some("failed" | "failure" | "error")
        );
    let runtime_active = is_active_session_status(runtime_status) || unexplained_provider_failure;
    let live_phase = prompt_activity.map(|activity| match activity {
        PromptActivity::Starting => "starting",
        PromptActivity::Accepted => "accepted",
        PromptActivity::Running => "running",
        PromptActivity::CancelRequested => "cancel-requested",
    });
    let live_active = matches!(
        prompt_activity,
        Some(PromptActivity::Starting | PromptActivity::Accepted | PromptActivity::Running)
    );
    let acp_stopping = matches!(prompt_activity, Some(PromptActivity::CancelRequested))
        || session_status
            .as_deref()
            .is_some_and(is_stopping_session_status);
    let runtime_terminal = !runtime_active
        && !acp_stopping
        && matches!(
            normalized_runtime_status.as_str(),
            "completed"
                | "complete"
                | "failed"
                | "failure"
                | "error"
                | "killed"
                | "cancelled"
                | "canceled"
        );
    let suppress_stale_acp_active =
        runtime_terminal && !runtime_resumable && prompt_activity.is_none();
    let acp_active = live_active
        || (!suppress_stale_acp_active
            && session_status
                .as_deref()
                .is_some_and(is_active_session_status));
    let acp_terminal = prompt_activity.is_none()
        && (suppress_stale_acp_active
            || session_status
                .as_deref()
                .is_some_and(is_terminal_session_status));
    let runtime_pause_overrides_session = runtime_paused
        && runtime_outcome.is_none()
        && (pause_reason.is_none()
            || manual_check_pending
            || matches!(
                pause_reason
                    .as_deref()
                    .map(normalize_lifecycle_code)
                    .as_deref(),
                Some("error-blocked")
            )
            || (runtime_resumable && is_runtime_continue_pause_reason(pause_reason)));

    let display_status = if matches!(prompt_activity, Some(PromptActivity::CancelRequested)) {
        "cancelling".to_string()
    } else if matches!(prompt_activity, Some(PromptActivity::Starting)) && !runtime_active {
        "starting".to_string()
    } else if matches!(
        prompt_activity,
        Some(PromptActivity::Accepted | PromptActivity::Running)
    ) && !runtime_active
    {
        "running".to_string()
    } else if acp_stopping {
        session_status
            .clone()
            .unwrap_or_else(|| "cancelling".to_string())
    } else if runtime_active || suppress_stale_acp_active || runtime_pause_overrides_session {
        runtime_status.to_string()
    } else if acp_active {
        session_status
            .clone()
            .unwrap_or_else(|| runtime_status.to_string())
    } else {
        session_status
            .clone()
            .unwrap_or_else(|| runtime_status.to_string())
    };
    let runtime_display = runtime_display_vm(
        Some(&display_status),
        runtime_outcome,
        current,
        pause_reason,
        runtime_resumable,
    );
    let continue_kind = runtime_continue_kind(
        runtime_status,
        pause_reason,
        runtime_resumable,
        manual_check_pending,
    );
    let runtime_phase = runtime_phase_for_lifecycle(
        runtime_status,
        runtime_active,
        runtime_terminal,
        live_phase.or(session_status.as_deref()),
        acp_active,
        acp_stopping,
        acp_terminal,
    );
    let composer = composer_for_lifecycle(
        &runtime_phase,
        runtime_active,
        acp_active,
        acp_stopping,
        continue_kind.as_deref(),
        &runtime_display,
    );

    ConversationAttemptLifecycleVm {
        runtime: ConversationRuntimeFacetVm {
            status: runtime_status.to_string(),
            outcome: runtime_outcome.map(str::to_string),
            pause_reason: pause_reason.map(str::to_string),
            resumable: runtime_resumable,
            current,
            active: runtime_active,
            continuable: continue_kind.is_some(),
            phase: runtime_phase,
        },
        acp: ConversationAcpFacetVm {
            status: session_status,
            phase: live_phase.map(str::to_string),
            active: acp_active,
            stopping: acp_stopping,
            terminal: acp_terminal,
        },
        display_status,
        runtime_display,
        continue_kind,
        composer,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn conversation_attempt_lifecycle_vm(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<&str>,
    outer_attempt_id: Option<&str>,
) -> anyhow::Result<ConversationAttemptLifecycleVm> {
    let run = app.run_status(task_id, run_id)?;
    let run_pause_reason = run.pause_reason.as_ref().map(enum_label);
    let runtime_resumable = is_run_continuable(&run);

    if let (Some(outer_node_id), Some(outer_attempt_id)) = (outer_node_id, outer_attempt_id) {
        let session_vm = dynamic_acp_session_vm(
            app,
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            node_id,
            attempt_id,
            None,
            None,
        )?;
        let dynamic_path = app.paths.dynamic_graph_file(
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
        );
        let dynamic_graph = read_json::<DynamicGraphState>(&dynamic_path)?;
        let dynamic_node = dynamic_graph
            .nodes
            .iter()
            .find(|node| node.id == node_id)
            .ok_or_else(|| anyhow::anyhow!("dynamic node `{}` not found", node_id))?;
        let raw_runtime_status = enum_label(&dynamic_node.status);
        let pause_reason = display_pause_reason_for_dynamic_attempt(
            app,
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            node_id,
            attempt_id,
            dynamic_node,
            run_pause_reason.as_deref(),
        );
        let runtime_status = if run.status == RunStatus::Paused
            && raw_runtime_status == "running"
            && dynamic_node.outcome.is_none()
            && is_runtime_continue_pause_reason(pause_reason.as_deref())
        {
            "paused".to_string()
        } else {
            raw_runtime_status
        };
        let outcome = dynamic_node.outcome.as_ref().map(enum_label);
        let run_paused_for_current_leaf = run_pause_reason.as_deref().is_some_and(|reason| {
            normalize_lifecycle_code(reason) == "error-blocked"
                || is_runtime_continue_pause_reason(Some(reason))
        });
        let current = run.current_round.as_deref() == Some(round_id)
            && run.current_node.as_deref() == Some(outer_node_id)
            && run.current_attempt.as_deref() == Some(outer_attempt_id)
            && (dynamic_graph
                .run
                .current_node_ids
                .iter()
                .any(|id| id == node_id)
                || (run_paused_for_current_leaf
                    && dynamic_node.status == gold_band::dynamic::DynamicNodeStatus::Paused
                    && dynamic_node.outcome.is_none()));
        let leaf_resumable = runtime_status == "paused"
            && outcome.is_none()
            && is_runtime_continue_pause_reason(pause_reason.as_deref());
        let attempt_dir = app.paths.dynamic_node_attempt_dir(
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            node_id,
            attempt_id,
        );
        return Ok(derive_conversation_attempt_lifecycle(
            session_vm.as_ref().map(|session| session.status.as_str()),
            prompt_activity(&attempt_dir),
            &runtime_status,
            outcome.as_deref(),
            current,
            pause_reason.as_deref(),
            leaf_resumable,
            false,
        ));
    }

    let session_vm = acp_session_vm(
        app, task_id, run_id, round_id, node_id, attempt_id, None, None,
    )?;
    let node_path = app
        .paths
        .node_file(task_id, run_id, round_id, node_id, attempt_id);
    let node = read_json::<gold_band::runtime::NodeState>(&node_path)?;
    let runtime_status = enum_label(&node.status);
    let outcome = node.outcome.as_ref().map(enum_label);
    let current = run.current_round.as_deref() == Some(round_id)
        && run.current_node.as_deref() == Some(node_id)
        && run.current_attempt.as_deref() == Some(attempt_id);
    let pause_reason = display_pause_reason_for_attempt(
        app,
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        run_pause_reason.as_deref(),
    );
    let attempt_dir = app
        .paths
        .attempt_dir(task_id, run_id, round_id, node_id, attempt_id);
    Ok(derive_conversation_attempt_lifecycle(
        session_vm.as_ref().map(|session| session.status.as_str()),
        prompt_activity(&attempt_dir),
        &runtime_status,
        outcome.as_deref(),
        current,
        pause_reason.as_deref(),
        runtime_resumable,
        node.manual_check_pending,
    ))
}

fn conversation_status_from_session(
    session_status: Option<&str>,
    runtime_status: &str,
    run_pause_reason: Option<&str>,
    runtime_resumable: bool,
) -> String {
    derive_conversation_attempt_lifecycle(
        session_status,
        None,
        runtime_status,
        None,
        false,
        run_pause_reason,
        runtime_resumable,
        false,
    )
    .display_status
}

fn lifecycle_is_active(
    lifecycle: &ConversationAttemptLifecycleVm,
    manual_check_pending: bool,
) -> bool {
    manual_check_pending
        || lifecycle.runtime.active
        || lifecycle.acp.active
        || lifecycle.acp.stopping
}

fn is_leaf_newer(
    candidate: &ConversationSessionLeafVm,
    current: Option<&ConversationSessionLeafVm>,
) -> bool {
    let Some(current) = current else {
        return true;
    };
    leaf_order_key(candidate) > leaf_order_key(current)
}

fn leaf_order_key(leaf: &ConversationSessionLeafVm) -> (&str, &str, &str, &str, &str) {
    (
        leaf.started_at
            .as_deref()
            .or(leaf.finished_at.as_deref())
            .unwrap_or(""),
        leaf.round_id.as_str(),
        leaf.outer_node_id.as_deref().unwrap_or(""),
        leaf.node_id.as_str(),
        leaf.attempt_id.as_str(),
    )
}

fn conversation_leaf_key(leaf: &ConversationSessionLeafVm) -> String {
    if leaf.outer_node_id.is_some() {
        format!(
            "{}/{}/{}/{}/{}",
            leaf.round_id,
            leaf.outer_node_id.as_deref().unwrap_or(""),
            leaf.outer_attempt_id.as_deref().unwrap_or(""),
            leaf.node_id,
            leaf.attempt_id
        )
    } else {
        format!("{}/{}/{}", leaf.round_id, leaf.node_id, leaf.attempt_id)
    }
}

pub fn conversation_run_vm(
    app: &App,
    project_id: &str,
    task_id: &str,
    run_id: &str,
    selected_session_key: Option<&str>,
) -> anyhow::Result<ConversationRunVm> {
    // Read the run state from disk
    let run = match app.run_status(task_id, run_id) {
        Ok(r) => r,
        Err(e) => {
            return Err(anyhow::anyhow!("run not found: {task_id}/{run_id}: {e}"));
        }
    };

    // Read the task state for title
    let task_state = app
        .task_show(task_id)
        .map_err(|e| anyhow::anyhow!("task not found: {task_id}: {e}"))?;
    let task_uuid = task_state
        .uuid
        .clone()
        .or_else(|| Some(task_id.to_string()));
    let title = task_state.title.unwrap_or_else(|| task_id.to_string());

    // Read conversation metadata if exists
    let conversation_metadata = read_conversation_metadata(app, task_id);
    let (run_mode, auto_title) = conversation_metadata
        .as_ref()
        .map(|metadata| (metadata.run_mode.clone(), metadata.title_auto_generated))
        .unwrap_or_else(|| ("workflow".to_string(), false));

    // Build the session tree from rounds/nodes/attempts
    // Read workflow snapshot once for node order + validity + raw JSON
    let workflow_snapshot: Option<WorkflowDsl> = gold_band::storage::read_json::<WorkflowDsl>(
        &app.paths.workflow_snapshot_file(task_id, run_id),
    )
    .ok();
    let workflow_node_order: HashMap<String, usize> = workflow_snapshot
        .as_ref()
        .map(|dsl| {
            dsl.nodes
                .iter()
                .enumerate()
                .map(|(i, n)| (n.id().to_string(), i))
                .collect()
        })
        .unwrap_or_default();

    let rounds = app.round_list(task_id, run_id)?;
    let mut tree_rounds: Vec<ConversationRoundNodeVm> = Vec::new();
    let mut active_sessions: Vec<ConversationActiveSessionVm> = Vec::new();
    let run_pause_reason = run.pause_reason.as_ref().map(enum_label);
    let runtime_resumable = is_run_continuable(&run);

    for round in &rounds {
        // List all nodes for this round (latest attempt per node)
        let mut nodes = app.node_list(task_id, run_id, &round.id)?;
        let trace_node_order: HashMap<String, usize> = {
            let mut order = HashMap::new();
            let mut trace = round.trace.clone();
            trace.sort_by_key(|step| step.sequence);
            for (index, step) in trace.iter().enumerate() {
                order.entry(step.node_id.clone()).or_insert(index);
            }
            order
        };
        // Prefer the actual per-round execution trace. Workflow DSL order is only a fallback for
        // legacy or synthetic nodes that do not have trace entries.
        nodes.sort_by_key(|n| {
            (
                trace_node_order
                    .get(&n.node_id)
                    .copied()
                    .unwrap_or(usize::MAX),
                workflow_node_order
                    .get(&n.node_id)
                    .copied()
                    .unwrap_or(usize::MAX),
            )
        });
        let mut tree_nodes: Vec<ConversationTreeNodeVm> = Vec::new();

        for node in &nodes {
            let is_ai_dynamic = node.node_type == NodeType::AiDynamic;
            let all_attempts = app.attempt_list(task_id, run_id, &round.id, &node.node_id)?;

            // Build child nodes for AI-DYNAMIC
            let mut outer_nodes: Option<Vec<ConversationTreeNodeVm>> = None;
            if is_ai_dynamic {
                if let Some(latest_attempt) = all_attempts.last() {
                    let dynamic_path = app.paths.dynamic_graph_file(
                        task_id,
                        run_id,
                        &round.id,
                        &node.node_id,
                        &latest_attempt.attempt_id,
                    );
                    if let Ok(dynamic_graph) = read_json::<DynamicGraphState>(&dynamic_path) {
                        let mut dynamic_tree_nodes: Vec<ConversationTreeNodeVm> = Vec::new();
                        for dyn_node in &dynamic_graph.nodes {
                            // Find the latest attempt for this dynamic child node
                            let dyn_node_dir = app.paths.dynamic_node_dir(
                                task_id,
                                run_id,
                                &round.id,
                                &node.node_id,
                                &latest_attempt.attempt_id,
                                &dyn_node.id,
                            );
                            let mut dyn_attempt_ids = std::fs::read_dir(dyn_node_dir.as_std_path())
                                .map(|entries| {
                                    entries
                                        .filter_map(|e| e.ok())
                                        .filter(|e| {
                                            e.file_type().map(|t| t.is_dir()).unwrap_or(false)
                                        })
                                        .filter_map(|e| e.file_name().into_string().ok())
                                        .filter(|n| n.starts_with("attempt-"))
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            dyn_attempt_ids.sort();
                            let dyn_runtime_status = enum_label(&dyn_node.status);
                            if dyn_attempt_ids.is_empty()
                                && is_active_session_status(&dyn_runtime_status)
                            {
                                dyn_attempt_ids.push(default_dynamic_attempt_id());
                            }

                            let mut dyn_leafs: Vec<ConversationSessionLeafVm> = Vec::new();
                            let dyn_outcome = dyn_node.outcome.as_ref().map(enum_label);
                            let run_paused_for_dyn_leaf =
                                run_pause_reason.as_deref().is_some_and(|reason| {
                                    normalize_lifecycle_code(reason) == "error-blocked"
                                        || is_runtime_continue_pause_reason(Some(reason))
                                });
                            let dyn_current = run.current_round.as_deref() == Some(&round.id)
                                && run.current_node.as_deref() == Some(&node.node_id)
                                && run.current_attempt.as_deref()
                                    == Some(&latest_attempt.attempt_id)
                                && (dynamic_graph
                                    .run
                                    .current_node_ids
                                    .iter()
                                    .any(|id| id == &dyn_node.id)
                                    || (run_paused_for_dyn_leaf
                                        && dyn_node.status
                                            == gold_band::dynamic::DynamicNodeStatus::Paused
                                        && dyn_node.outcome.is_none()));
                            let dyn_base_status = if run.status == RunStatus::Paused
                                && dyn_runtime_status == "running"
                                && dyn_node.outcome.is_none()
                                && is_runtime_continue_pause_reason(run_pause_reason.as_deref())
                            {
                                "paused".to_string()
                            } else {
                                dyn_runtime_status.clone()
                            };
                            for dyn_attempt_id in &dyn_attempt_ids {
                                let dyn_session_status = dynamic_acp_session_status(
                                    app,
                                    task_id,
                                    run_id,
                                    &round.id,
                                    &node.node_id,
                                    &latest_attempt.attempt_id,
                                    &dyn_node.id,
                                    dyn_attempt_id,
                                )?;
                                let dyn_pause_reason = display_pause_reason_for_dynamic_attempt(
                                    app,
                                    task_id,
                                    run_id,
                                    &round.id,
                                    &node.node_id,
                                    &latest_attempt.attempt_id,
                                    &dyn_node.id,
                                    dyn_attempt_id,
                                    dyn_node,
                                    run_pause_reason.as_deref(),
                                );
                                let dyn_status = if run.status == RunStatus::Paused
                                    && dyn_runtime_status == "running"
                                    && dyn_node.outcome.is_none()
                                    && is_runtime_continue_pause_reason(dyn_pause_reason.as_deref())
                                {
                                    "paused".to_string()
                                } else {
                                    dyn_runtime_status.clone()
                                };
                                let dyn_leaf_resumable = dyn_status == "paused"
                                    && dyn_outcome.is_none()
                                    && is_runtime_continue_pause_reason(
                                        dyn_pause_reason.as_deref(),
                                    );
                                let dyn_attempt_dir = app.paths.dynamic_node_attempt_dir(
                                    task_id,
                                    run_id,
                                    &round.id,
                                    &node.node_id,
                                    &latest_attempt.attempt_id,
                                    &dyn_node.id,
                                    dyn_attempt_id,
                                );
                                let lifecycle = derive_conversation_attempt_lifecycle(
                                    dyn_session_status.as_deref(),
                                    prompt_activity(&dyn_attempt_dir),
                                    &dyn_status,
                                    dyn_outcome.as_deref(),
                                    dyn_current,
                                    dyn_pause_reason.as_deref(),
                                    dyn_leaf_resumable,
                                    false,
                                );
                                let dyn_status = lifecycle.display_status.clone();
                                let dyn_runtime_display = lifecycle.runtime_display.clone();
                                let is_active = lifecycle_is_active(&lifecycle, false);
                                let (artifacts, attachments) = conversation_session_assets(
                                    app,
                                    task_id,
                                    run_id,
                                    &round.id,
                                    &dyn_node.id,
                                    dyn_attempt_id,
                                    Some(&node.node_id),
                                    Some(&latest_attempt.attempt_id),
                                )?;

                                dyn_leafs.push(ConversationSessionLeafVm {
                                    round_id: round.id.clone(),
                                    node_id: dyn_node.id.clone(),
                                    attempt_id: dyn_attempt_id.clone(),
                                    outer_node_id: Some(node.node_id.clone()),
                                    outer_attempt_id: Some(latest_attempt.attempt_id.clone()),
                                    path_label: format!("{}/{}", dyn_node.id, dyn_attempt_id),
                                    status: dyn_status.clone(),
                                    outcome: dyn_outcome.clone(),
                                    runtime_display: dyn_runtime_display.clone(),
                                    lifecycle: lifecycle.clone(),
                                    current: dyn_current,
                                    manual_check_pending: false,
                                    started_at: dyn_node.started_at.clone(),
                                    finished_at: dyn_node.finished_at.clone(),
                                    session_id: None,
                                    artifact_count: artifacts.len(),
                                    attachment_count: attachments.len(),
                                });

                                if is_active {
                                    active_sessions.push(ConversationActiveSessionVm {
                                        round_id: round.id.clone(),
                                        node_id: dyn_node.id.clone(),
                                        attempt_id: dyn_attempt_id.clone(),
                                        outer_node_id: Some(node.node_id.clone()),
                                        outer_attempt_id: Some(latest_attempt.attempt_id.clone()),
                                        path_label: format!("{}/{}", dyn_node.id, dyn_attempt_id),
                                        status: dyn_status.clone(),
                                        runtime_display: dyn_runtime_display.clone(),
                                        lifecycle: lifecycle.clone(),
                                        manual_check_pending: false,
                                        session_id: None,
                                        started_at: None,
                                    });
                                }
                            }

                            let dyn_node_status = dyn_leafs
                                .last()
                                .map(|l| l.status.clone())
                                .unwrap_or_else(|| dyn_base_status.clone());
                            let dyn_node_runtime_display = dyn_leafs
                                .last()
                                .map(|l| l.runtime_display.clone())
                                .unwrap_or_else(|| {
                                    runtime_display_vm(
                                        Some(&dyn_base_status),
                                        dyn_outcome.as_deref(),
                                        dyn_current,
                                        run_pause_reason.as_deref(),
                                        runtime_resumable,
                                    )
                                });

                            dynamic_tree_nodes.push(ConversationTreeNodeVm {
                                node_id: dyn_node.id.clone(),
                                label: dyn_node.title.clone(),
                                node_type: format!("dynamic-{}", enum_label(&dyn_node.kind)),
                                status: dyn_node_status,
                                runtime_display: dyn_node_runtime_display,
                                attempts: dyn_leafs,
                                outer_nodes: None,
                            });
                        }
                        outer_nodes = Some(dynamic_tree_nodes);
                    }
                }
            }

            // Build leafs for the top-level node itself.
            // AI-DYNAMIC nodes are containers — their real sessions live in outer_nodes.
            let mut leafs: Vec<ConversationSessionLeafVm> = Vec::new();
            if !is_ai_dynamic {
                for attempt in &all_attempts {
                    let session_status = acp_session_status(
                        app,
                        task_id,
                        run_id,
                        &round.id,
                        &node.node_id,
                        &attempt.attempt_id,
                    )?;
                    let runtime_status = enum_label(&attempt.status);
                    let display_pause_reason = display_pause_reason_for_attempt(
                        app,
                        task_id,
                        run_id,
                        &round.id,
                        &node.node_id,
                        &attempt.attempt_id,
                        run_pause_reason.as_deref(),
                    );
                    let outcome = attempt.outcome.as_ref().map(enum_label);
                    let current = run.current_round.as_deref() == Some(&round.id)
                        && run.current_node.as_deref() == Some(&node.node_id)
                        && run.current_attempt.as_deref() == Some(&attempt.attempt_id);
                    let manual_check_pending = attempt.manual_check_pending;
                    let attempt_dir = app.paths.attempt_dir(
                        task_id,
                        run_id,
                        &round.id,
                        &node.node_id,
                        &attempt.attempt_id,
                    );
                    let lifecycle = derive_conversation_attempt_lifecycle(
                        session_status.as_deref(),
                        prompt_activity(&attempt_dir),
                        &runtime_status,
                        outcome.as_deref(),
                        current,
                        display_pause_reason.as_deref(),
                        runtime_resumable,
                        manual_check_pending,
                    );
                    let status = lifecycle.display_status.clone();
                    let runtime_display = lifecycle.runtime_display.clone();
                    let is_active = lifecycle_is_active(&lifecycle, manual_check_pending);
                    let (artifacts, attachments) = conversation_session_assets(
                        app,
                        task_id,
                        run_id,
                        &round.id,
                        &node.node_id,
                        &attempt.attempt_id,
                        None,
                        None,
                    )?;
                    leafs.push(ConversationSessionLeafVm {
                        round_id: round.id.clone(),
                        node_id: node.node_id.clone(),
                        attempt_id: attempt.attempt_id.clone(),
                        outer_node_id: None,
                        outer_attempt_id: None,
                        path_label: format!("{}/{}", node.node_id, attempt.attempt_id),
                        status: status.clone(),
                        outcome,
                        runtime_display: runtime_display.clone(),
                        lifecycle: lifecycle.clone(),
                        current,
                        manual_check_pending,
                        started_at: Some(attempt.started_at.clone()),
                        finished_at: attempt.finished_at.clone(),
                        session_id: None,
                        artifact_count: artifacts.len(),
                        attachment_count: attachments.len(),
                    });

                    if is_active {
                        active_sessions.push(ConversationActiveSessionVm {
                            round_id: round.id.clone(),
                            node_id: node.node_id.clone(),
                            attempt_id: attempt.attempt_id.clone(),
                            outer_node_id: None,
                            outer_attempt_id: None,
                            path_label: format!("{}/{}", node.node_id, attempt.attempt_id),
                            status,
                            runtime_display: runtime_display.clone(),
                            lifecycle: lifecycle.clone(),
                            manual_check_pending,
                            session_id: None,
                            started_at: Some(attempt.started_at.clone()),
                        });
                    }
                }
            }

            let node_status = if is_ai_dynamic {
                // Derive status from dynamic child nodes
                outer_nodes
                    .as_ref()
                    .and_then(|ons| ons.last())
                    .map(|on| on.status.clone())
                    .unwrap_or_else(|| "completed".to_string())
            } else {
                all_attempts
                    .last()
                    .map(|a| enum_label(&a.status))
                    .unwrap_or_else(|| "pending".to_string())
            };
            let node_runtime_display = if is_ai_dynamic {
                outer_nodes
                    .as_ref()
                    .and_then(|ons| ons.last())
                    .map(|on| on.runtime_display.clone())
                    .unwrap_or_else(|| {
                        runtime_display_vm(
                            Some(&node_status),
                            None,
                            false,
                            run_pause_reason.as_deref(),
                            runtime_resumable,
                        )
                    })
            } else {
                leafs
                    .last()
                    .map(|leaf| leaf.runtime_display.clone())
                    .unwrap_or_else(|| {
                        runtime_display_vm(
                            Some(&node_status),
                            None,
                            false,
                            run_pause_reason.as_deref(),
                            runtime_resumable,
                        )
                    })
            };

            tree_nodes.push(ConversationTreeNodeVm {
                node_id: node.node_id.clone(),
                label: node.node_id.clone(),
                node_type: enum_label(&node.node_type),
                status: node_status,
                runtime_display: node_runtime_display,
                attempts: leafs,
                outer_nodes,
            });
        }

        let round_status = enum_label(&round.status);
        let round_outcome = round.outcome.as_ref().map(enum_label);
        tree_rounds.push(ConversationRoundNodeVm {
            round_id: round.id.clone(),
            index: round.index,
            label: format!("round-{:03}", round.index),
            status: round_status.clone(),
            runtime_display: runtime_display_vm(
                Some(&round_status),
                round_outcome.as_deref(),
                run.current_round.as_deref() == Some(&round.id),
                run_pause_reason.as_deref(),
                runtime_resumable,
            ),
            nodes: tree_nodes,
        });
    }

    // Determine which session leaf to load.
    let selected_leaf: Option<ConversationSessionLeafVm> = if let Some(key) = selected_session_key {
        // Find the leaf matching the key by searching the tree.
        find_leaf_by_key(&tree_rounds, key)
    } else {
        // Runtime-owned sessions need a stable UI anchor as soon as the run starts.
        // Prefer the current/running attempt, then fall back to the newest conversation.
        default_session_leaf(&tree_rounds)
    };

    let effective_key: Option<String> = selected_leaf.as_ref().map(conversation_leaf_key);

    // Load the selected ACP session
    let selected_session = if let Some(ref leaf) = selected_leaf {
        if let (Some(outer_id), Some(outer_attempt)) = (
            leaf.outer_node_id.as_deref(),
            leaf.outer_attempt_id.as_deref(),
        ) {
            crate::view_models::dynamic_acp_session_vm(
                app,
                task_id,
                run_id,
                &leaf.round_id,
                outer_id,
                outer_attempt,
                &leaf.node_id,
                &leaf.attempt_id,
                None,
                None,
            )
            .ok()
            .flatten()
        } else {
            crate::view_models::acp_session_vm(
                app,
                task_id,
                run_id,
                &leaf.round_id,
                &leaf.node_id,
                &leaf.attempt_id,
                None,
                None,
            )
            .ok()
            .flatten()
        }
    } else {
        None
    };

    let (artifacts, attachments) = if let Some(ref leaf) = selected_leaf {
        conversation_session_assets(
            app,
            task_id,
            run_id,
            &leaf.round_id,
            &leaf.node_id,
            &leaf.attempt_id,
            leaf.outer_node_id.as_deref(),
            leaf.outer_attempt_id.as_deref(),
        )?
    } else {
        (Vec::new(), Vec::new())
    };

    let input_attachments = input_attachments_vm(app, task_id);

    let run_outcome = run.outcome.map(|o| enum_label(&o));
    let resumable = gold_band::app::is_run_continuable(&run);
    let run_status = enum_label(&run.status);
    let runtime_error_message = selected_leaf
        .as_ref()
        .and_then(|leaf| dynamic_leaf_runtime_error_message(app, task_id, run_id, leaf))
        .or_else(|| {
            runtime_error_message(
                app,
                task_id,
                run_id,
                run_pause_reason.as_deref(),
                run_outcome.as_deref(),
            )
        });

    let (workflow_valid, workflow_json) = if let Some(ref dsl) = workflow_snapshot {
        (true, Some(serde_json::to_string(dsl).unwrap_or_default()))
    } else {
        (true, None)
    };

    // Build workflow graph from the selected session's runtime locator so the
    // conversation view keeps AI-DYNAMIC internal graphs even after terminal refreshes.
    let workflow_graph = selected_leaf
        .as_ref()
        .and_then(|leaf| {
            leaf.outer_node_id
                .as_deref()
                .zip(leaf.outer_attempt_id.as_deref())
                .and_then(|(outer_node_id, outer_attempt_id)| {
                    dynamic_runtime_graph_vm(
                        app,
                        task_id,
                        run_id,
                        &leaf.round_id,
                        outer_node_id,
                        outer_attempt_id,
                    )
                })
                .or_else(|| {
                    round_detail_vm(app, task_id, run_id, &leaf.round_id, None)
                        .ok()
                        .map(|detail| detail.graph)
                })
        })
        .or_else(|| workflow_snapshot.as_ref().map(|dsl| workflow_graph_vm(dsl)))
        .unwrap_or_else(|| GraphVm {
            nodes: Vec::new(),
            edges: Vec::new(),
        });

    Ok(ConversationRunVm {
        workflow_graph,
        project_id: project_id.to_string(),
        task_id: task_id.to_string(),
        task_uuid,
        run_id: run_id.to_string(),
        title,
        auto_title,
        run_mode,
        workflow_template_id: None,
        direct_config: conversation_metadata
            .as_ref()
            .and_then(|metadata| metadata.direct_config.clone()),
        agent_identity: conversation_metadata
            .as_ref()
            .and_then(|metadata| metadata.agent_identity.clone()),
        last_activity_at: conversation_metadata.as_ref().and_then(|metadata| {
            metadata
                .last_activity_at
                .clone()
                .or_else(|| Some(metadata.created_at.clone()))
        }),
        run_status,
        run_outcome,
        session_tree: ConversationSessionTreeVm {
            rounds: tree_rounds,
            selected_session_key: effective_key,
        },
        selected_session,
        active_sessions,
        artifacts,
        attachments,
        input_attachments,
        workflow_status: "valid".to_string(),
        workflow_valid,
        workflow_error: None,
        workflow_json,
        resumable,
        pause_reason: run.pause_reason.map(|r| enum_label(&r)),
        runtime_error_message,
        scheduled_task_id: conversation_metadata
            .as_ref()
            .and_then(|metadata| metadata.scheduled_task_id.clone()),
    })
}

// ── Attachment validation helpers ──

pub(crate) const MAX_ATTACHMENT_COUNT: usize = 10;
pub(crate) const MAX_ATTACHMENT_PER_FILE: u64 = 25 * 1024 * 1024; // 25 MB
pub(crate) const MAX_ATTACHMENT_TOTAL: u64 = 100 * 1024 * 1024; // 100 MB

pub(crate) fn allowed_attachment_ext(ext: &str) -> bool {
    gold_band::provider::supported_attachment_extensions()
        .into_iter()
        .any(|supported| supported == ext)
}

pub(crate) fn validate_attachment_paths(paths: &[String]) -> Vec<String> {
    let mut errors: Vec<String> = Vec::new();
    if paths.len() > MAX_ATTACHMENT_COUNT {
        errors.push("conversation.attachment-count-exceeded".to_string());
        return errors;
    }
    let mut total_size: u64 = 0;
    let mut seen = std::collections::HashSet::new();
    for p in paths {
        if !seen.insert(p) {
            continue;
        }
        let path = Path::new(p);
        if !path.exists() {
            errors.push("conversation.attachment-not-found".to_string());
            continue;
        }
        if path.is_dir() {
            errors.push("conversation.attachment-unsupported-type".to_string());
            continue;
        }
        let meta = match path.metadata() {
            Ok(m) => m,
            Err(_) => {
                errors.push("conversation.attachment-unreadable".to_string());
                continue;
            }
        };
        if meta.len() == 0 {
            errors.push("conversation.attachment-unreadable".to_string());
            continue;
        }
        if meta.len() > MAX_ATTACHMENT_PER_FILE {
            errors.push("conversation.attachment-too-large".to_string());
            continue;
        }
        total_size += meta.len();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !allowed_attachment_ext(ext.to_lowercase().as_str()) {
            errors.push("conversation.attachment-unsupported-type".to_string());
        }
    }
    if total_size > MAX_ATTACHMENT_TOTAL {
        errors.push("conversation.attachment-total-too-large".to_string());
    }
    errors
}

fn missing_item(code: &str, label: &str, recovery_path: &str) -> ConversationMissingItemVm {
    ConversationMissingItemVm {
        code: code.to_string(),
        label: label.to_string(),
        recovery_path: recovery_path.to_string(),
    }
}

// ── Input attachments (task-level authoring) ──

fn input_attachments_vm(app: &App, task_id: &str) -> Vec<AssetItemVm> {
    let dir = app.paths.task_dir(task_id).join("authoring").join("inputs");
    if !dir.exists() {
        return Vec::new();
    }
    let mut files: Vec<AssetItemVm> = std::fs::read_dir(dir.as_std_path())
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().map(|t| t.is_file()).unwrap_or(false))
                .filter_map(|entry| {
                    let name = entry.file_name().into_string().ok()?;
                    let size = entry.metadata().ok()?.len();
                    Some(AssetItemVm {
                        kind: "input-attachment".to_string(),
                        title: format!("{} ({} KB)", name, size / 1024),
                        preview: name.clone(),
                        tone: "info".to_string(),
                        round_id: String::new(),
                        node_id: String::new(),
                        attempt_id: String::new(),
                        name,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort_by(|a, b| a.name.cmp(&b.name));
    files
}

// ── Validated create ──

pub fn validate_conversation_create_vm(
    app: &App,
    input: &ConversationCreateInputVm,
) -> anyhow::Result<ConversationValidationResultVm> {
    let mut missing: Vec<ConversationMissingItemVm> = Vec::new();

    if input.content.trim().is_empty() {
        missing.push(missing_item(
            "content.required",
            "Content is required",
            "/chat",
        ));
    }

    if input.run_mode == ConversationRunMode::Direct.as_str() {
        let config = input.direct_config.as_ref();
        let agent_type = config
            .map(|config| config.agent_type.trim())
            .unwrap_or_default();
        if agent_type.is_empty() {
            missing.push(missing_item(
                "direct.agent.required",
                "Agent is required for Direct mode",
                "/chat/agents",
            ));
        } else if app.managed_agent(agent_type).is_err() {
            missing.push(missing_item(
                "direct.agent.not-found",
                "Selected Agent is not configured",
                "/chat/agents",
            ));
        }
    } else if input.run_mode == ConversationRunMode::Auto.as_str() {
        let config = input.auto_config.as_ref();
        let strategy = config
            .and_then(|c| c.agent_strategy.as_deref())
            .unwrap_or("fixed");
        if strategy == "dynamic" {
            if config
                .and_then(|c| c.bootstrap_agent_type.as_deref())
                .or_else(|| config.map(|c| c.agent_type.as_str()))
                .map(|agent| agent.trim().is_empty())
                .unwrap_or(true)
            {
                missing.push(missing_item(
                    "agent.required",
                    "Agent is required for AUTO mode",
                    "/chat/agents",
                ));
            }
            if config
                .and_then(|c| c.available_agents.as_ref())
                .map(|agents| agents.iter().all(|agent| agent.provider.trim().is_empty()))
                .unwrap_or(true)
            {
                missing.push(missing_item(
                    "agent.required",
                    "Agent is required for AUTO mode",
                    "/chat/agents",
                ));
            }
        } else if config
            .map(|c| c.agent_type.trim().is_empty())
            .unwrap_or(true)
        {
            missing.push(missing_item(
                "agent.required",
                "Agent is required for AUTO mode",
                "/chat/agents",
            ));
        }
    } else if input.run_mode == ConversationRunMode::Workflow.as_str() {
        if input
            .workflow_template_id
            .as_ref()
            .map(|t| t.trim().is_empty())
            .unwrap_or(true)
        {
            missing.push(missing_item(
                "workflow.required",
                "Workflow template is required",
                "/chat/run-modes",
            ));
        } else if let Some(ref tid) = input.workflow_template_id {
            let store = app.workflow_templates().ok();
            let found = store
                .as_ref()
                .and_then(|s| s.templates.iter().find(|t| t.id == *tid));
            if found.is_none() {
                missing.push(missing_item(
                    "workflow.not-found",
                    "Selected workflow template not found",
                    "/chat/run-modes",
                ));
            }
        }
    }

    // Validate attachments
    if let Some(ref paths) = input.attachment_paths {
        let errors = validate_attachment_paths(paths);
        for code in &errors {
            missing.push(missing_item(code, code, "/chat"));
        }
    }

    Ok(ConversationValidationResultVm {
        valid: missing.is_empty(),
        missing_items: missing,
    })
}

// ── Real create ──

fn conversation_auto_title(content: &str, max_chars: usize) -> String {
    if content.is_empty() {
        "New Task".to_string()
    } else {
        content
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(max_chars.max(1))
            .collect()
    }
}

fn dynamic_control_from_vm(control: Option<&ConversationDynamicControlVm>) -> DynamicControlDsl {
    control
        .map(|control| DynamicControlDsl {
            max_dynamic_nodes: control.max_dynamic_nodes,
            max_fanout: control.max_fanout,
            max_depth: control.max_depth,
            max_parallel: control.max_parallel,
            max_group_depth: control.max_group_depth,
            max_workflow_invocations: control.max_workflow_invocations,
            allow_nested_dynamic: control.allow_nested_dynamic,
        })
        .unwrap_or_default()
}

fn build_auto_workflow(config: Option<&ConversationAutoConfigVm>) -> WorkflowDsl {
    let agent_type = config.map(|c| c.agent_type.as_str()).unwrap_or("");
    let model_id = config
        .and_then(|c| c.model_id.as_deref())
        .filter(|v| !v.trim().is_empty());
    let bootstrap_model_id = config
        .and_then(|c| c.bootstrap_model_id.as_deref())
        .filter(|v| !v.trim().is_empty());
    let acceptance_model_id = config
        .and_then(|c| c.acceptance_model_id.as_deref())
        .filter(|v| !v.trim().is_empty());
    let permission_mode = config
        .and_then(|c| c.permission_mode.as_deref())
        .filter(|v| !v.trim().is_empty());
    let global_goal = config
        .and_then(|c| c.global_goal.as_deref())
        .filter(|v| !v.trim().is_empty());
    let agent_strategy_mode = config
        .and_then(|c| c.agent_strategy.as_deref())
        .unwrap_or("fixed");

    let agent_strategy = if agent_strategy_mode == "dynamic" {
        let bootstrap_provider = config
            .and_then(|c| c.bootstrap_agent_type.as_deref())
            .filter(|v| !v.trim().is_empty())
            .unwrap_or(agent_type)
            .to_string();
        let available_agents = config
            .and_then(|c| c.available_agents.as_ref())
            .map(|agents| {
                agents
                    .iter()
                    .filter_map(|agent| {
                        let provider = agent.provider.trim();
                        if provider.is_empty() {
                            return None;
                        }
                        Some(DynamicAgentRef {
                            provider: provider.to_string(),
                            model: agent
                                .model
                                .as_deref()
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(str::to_string),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|agents| !agents.is_empty())
            .unwrap_or_else(|| {
                vec![DynamicAgentRef {
                    provider: bootstrap_provider.clone(),
                    model: model_id.map(str::to_string),
                }]
            });
        AiDynamicAgentStrategy::Dynamic {
            bootstrap_provider,
            bootstrap_model: bootstrap_model_id.map(str::to_string),
            acceptance_model: acceptance_model_id.map(str::to_string),
            routing_prompt: config
                .and_then(|c| c.routing_prompt.as_deref())
                .map(str::trim)
                .unwrap_or("")
                .to_string(),
            available_agents,
        }
    } else {
        AiDynamicAgentStrategy::Fixed {
            provider: agent_type.to_string(),
            model: model_id.map(str::to_string),
        }
    };

    WorkflowDsl {
        version: "0.1".to_string(),
        id: "auto-workflow".to_string(),
        entry: "ai-dynamic".to_string(),
        control: Default::default(),
        nodes: vec![NodeDsl::AiDynamic(AiDynamicNode {
            id: "ai-dynamic".to_string(),
            agent_strategy,
            permission_mode: permission_mode.map(|s| s.to_string()),
            config_options: config
                .map(|config| config.config_options.clone())
                .unwrap_or_default(),
            allowed_profiles: config
                .and_then(|c| c.allowed_profiles.clone())
                .unwrap_or_default(),
            global_goal: global_goal.map(|s| s.to_string()),
            control: dynamic_control_from_vm(config.and_then(|c| c.control.as_ref())),
            allowed_workflows: config
                .and_then(|c| c.allowed_workflows.as_ref())
                .map(|workflows| {
                    workflows
                        .iter()
                        .filter_map(|workflow| {
                            let workflow_id = workflow.workflow_id.trim();
                            (!workflow_id.is_empty()).then(|| {
                                gold_band::dsl::AllowedWorkflowRefDsl {
                                    workflow_id: workflow_id.to_string(),
                                }
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })],
        edges: vec![EdgeDsl {
            from: "ai-dynamic".to_string(),
            to: END_NODE.to_string(),
            on: EdgeOutcome::Success,
            session: None,
            new_round_entry: None,
        }],
    }
}

fn build_direct_workflow(config: &ConversationDirectConfigVm) -> WorkflowDsl {
    WorkflowDsl {
        version: "0.1".to_string(),
        id: "direct-agent".to_string(),
        entry: "direct-agent".to_string(),
        control: Default::default(),
        nodes: vec![NodeDsl::Worker(WorkerNode {
            id: "direct-agent".to_string(),
            provider: Some(config.agent_type.clone()),
            model: config.model_id.clone(),
            profile: None,
            goal: None,
            output: None,
            success_condition: None,
            permission_mode: config.permission_mode.clone(),
            config_options: config.config_options.clone(),
            manual_check: Some(false),
            prompt_envelope: PromptEnvelopeMode::RawAgent,
        })],
        edges: vec![EdgeDsl {
            from: "direct-agent".to_string(),
            to: END_NODE.to_string(),
            on: EdgeOutcome::Success,
            session: None,
            new_round_entry: None,
        }],
    }
}

pub fn create_conversation_task_vm(
    app: &App,
    input: &ConversationCreateInputVm,
) -> anyhow::Result<(String, Option<String>, String)> {
    let title =
        conversation_auto_title(&input.content, app.config.conversation_auto_title_max_chars);

    // Build workflow
    let (workflow, effective_include_interview) =
        if input.run_mode == ConversationRunMode::Direct.as_str() {
            let config = input
                .direct_config
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("direct config is required"))?;
            (build_direct_workflow(config), None)
        } else if input.run_mode == ConversationRunMode::Auto.as_str() {
            (build_auto_workflow(input.auto_config.as_ref()), None)
        } else {
            // Load from template
            let store = app.workflow_templates()?;
            let template_id = input
                .workflow_template_id
                .as_deref()
                .unwrap_or(DEFAULT_WORKFLOW_TEMPLATE_ID);
            let mut workflow = store
                .templates
                .iter()
                .find(|t| t.id == template_id)
                .map(|t| t.workflow.clone())
                .ok_or_else(|| anyhow::anyhow!("workflow template not found: {template_id}"))?;
            let include_interview = apply_workflow_interview_preference(
                template_id,
                input.include_interview,
                &mut workflow,
            )?;
            (workflow, include_interview)
        };

    // Create task
    let summary = app.create_task_from_requirement(CreateTaskInput {
        title: Some(title.clone()),
        description: None,
        requirement_file_name: None,
        requirement_content: input.content.clone(),
        workflow,
        workflow_template_id: input.workflow_template_id.clone(),
    })?;

    let task_id = summary.task.id.clone();
    let task_uuid = summary.task.uuid.clone().or_else(|| Some(task_id.clone()));

    // Save conversation metadata
    let authoring_dir = app.paths.task_dir(&task_id).join("authoring");
    fs::create_dir_all(authoring_dir.as_std_path())?;

    let created_at = chrono::Utc::now().to_rfc3339();
    let agent_identity = input
        .direct_config
        .as_ref()
        .and_then(|config| direct_agent_identity(app, &config.agent_type));
    let meta = ConversationMetadata {
        version: "2".to_string(),
        source: "conversation-ui".to_string(),
        run_mode: input.run_mode.clone(),
        workflow_template_id: input.workflow_template_id.clone(),
        include_interview: effective_include_interview,
        direct_config: input.direct_config.clone(),
        agent_identity,
        title_auto_generated: true,
        initial_attachment_names: Some(
            input
                .attachment_paths
                .as_ref()
                .map(|paths| {
                    paths
                        .iter()
                        .map(|path| {
                            Path::new(path)
                                .file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or("unknown")
                                .to_string()
                        })
                        .collect()
                })
                .unwrap_or_default(),
        ),
        created_at: created_at.clone(),
        last_activity_at: Some(created_at),
        scheduled_task_id: input.scheduled_task_id.clone(),
        scheduled_content_fingerprint: input.scheduled_content_fingerprint.clone(),
    };
    write_json(&authoring_dir.join("conversation.json"), &meta)?;

    // Copy attachments to authoring dir
    if let Some(ref paths) = input.attachment_paths {
        let attach_dir = authoring_dir.join("inputs");
        fs::create_dir_all(attach_dir.as_std_path())?;
        for src in paths {
            let src_path = Path::new(src);
            if let Some(name) = src_path.file_name().and_then(|n| n.to_str()) {
                let dest = attach_dir.join(name);
                let _ = fs::copy(src_path, &dest);
            }
        }
    }

    Ok((task_id, task_uuid, title))
}

pub fn create_conversation_run_vm(
    app: &App,
    input: &ConversationCreateInputVm,
) -> anyhow::Result<ConversationRunVm> {
    let (task_id, task_uuid, title) = create_conversation_task_vm(app, input)?;

    // Start the workflow in the background so the conversation surface can
    // display the session as soon as the first ACP events arrive.
    let run = app.run_start_background(&task_id, None)?;

    // Return early VM from the run
    conversation_run_vm(app, &input.project_id, &task_id, &run.id, None).or_else(|_| {
        Ok(ConversationRunVm {
            project_id: input.project_id.clone(),
            task_id: task_id.clone(),
            task_uuid: task_uuid.clone(),
            run_id: run.id,
            title,
            auto_title: true,
            run_mode: input.run_mode.clone(),
            workflow_template_id: input.workflow_template_id.clone(),
            direct_config: input.direct_config.clone(),
            agent_identity: input
                .direct_config
                .as_ref()
                .and_then(|config| direct_agent_identity(app, &config.agent_type)),
            last_activity_at: Some(chrono::Utc::now().to_rfc3339()),
            run_status: enum_label(&run.status),
            run_outcome: None,
            session_tree: ConversationSessionTreeVm {
                rounds: Vec::new(),
                selected_session_key: None,
            },
            selected_session: None,
            active_sessions: Vec::new(),
            artifacts: Vec::new(),
            attachments: Vec::new(),
            input_attachments: Vec::new(),
            workflow_status: "valid".to_string(),
            workflow_valid: true,
            workflow_error: None,
            workflow_json: None,
            workflow_graph: GraphVm {
                nodes: Vec::new(),
                edges: Vec::new(),
            },
            resumable: false,
            pause_reason: None,
            runtime_error_message: None,
            scheduled_task_id: input.scheduled_task_id.clone(),
        })
    })
}

fn remove_default_workflow_interview_stage(workflow: &mut WorkflowDsl) -> anyhow::Result<()> {
    const INTERVIEW_NODE_ID: &str = "interview";

    if workflow.entry != INTERVIEW_NODE_ID {
        anyhow::bail!("default workflow interview stage must be the entry node");
    }
    if !workflow
        .nodes
        .iter()
        .any(|node| node.id() == INTERVIEW_NODE_ID)
    {
        anyhow::bail!("default workflow interview stage is missing");
    }

    let mut successors = workflow
        .edges
        .iter()
        .filter(|edge| edge.from == INTERVIEW_NODE_ID && edge.on == EdgeOutcome::Success)
        .map(|edge| edge.to.as_str());
    let next_entry = successors
        .next()
        .ok_or_else(|| anyhow::anyhow!("default workflow interview stage has no success edge"))?;
    if successors.next().is_some() {
        anyhow::bail!("default workflow interview stage has multiple success edges");
    }
    let next_entry = next_entry.to_string();

    workflow.nodes.retain(|node| node.id() != INTERVIEW_NODE_ID);
    workflow
        .edges
        .retain(|edge| edge.from != INTERVIEW_NODE_ID && edge.to != INTERVIEW_NODE_ID);
    workflow.entry = next_entry;
    Ok(())
}

fn apply_workflow_interview_preference(
    template_id: &str,
    include_interview: Option<bool>,
    workflow: &mut WorkflowDsl,
) -> anyhow::Result<Option<bool>> {
    if template_id != DEFAULT_WORKFLOW_TEMPLATE_ID {
        return Ok(None);
    }

    let include_interview = include_interview.unwrap_or(true);
    if !include_interview {
        remove_default_workflow_interview_stage(workflow)?;
    }
    Ok(Some(include_interview))
}

pub fn rerun_conversation_task_vm(
    app: &App,
    project_id: &str,
    task_id: &str,
) -> anyhow::Result<ConversationRunVm> {
    // Pause running run if any
    if let Ok(summaries) = app.task_summaries() {
        if let Some(ts) = summaries.iter().find(|s| s.task.id == task_id) {
            if let Some(ref latest) = ts.latest_run {
                if latest.status == RunStatus::Running {
                    let _ = app.run_pause(
                        task_id,
                        &latest.id,
                        gold_band::domain::PauseReason::ProcessInterrupted,
                    );
                }
            }
        }
    }
    // Start new run in the background; live ACP events drive the UI refresh.
    let run = app.run_start_background(task_id, None)?;
    conversation_run_vm(app, project_id, task_id, &run.id, None).or_else(|_| {
        Ok(ConversationRunVm {
            project_id: project_id.to_string(),
            task_id: task_id.to_string(),
            task_uuid: app
                .task_show(task_id)
                .ok()
                .and_then(|task| task.uuid)
                .or_else(|| Some(task_id.to_string())),
            run_id: run.id,
            title: String::new(),
            auto_title: false,
            run_mode: "workflow".to_string(),
            workflow_template_id: None,
            direct_config: read_conversation_metadata(app, task_id)
                .and_then(|metadata| metadata.direct_config),
            agent_identity: read_conversation_metadata(app, task_id)
                .and_then(|metadata| metadata.agent_identity),
            last_activity_at: read_conversation_metadata(app, task_id).and_then(|metadata| {
                metadata
                    .last_activity_at
                    .or_else(|| Some(metadata.created_at))
            }),
            run_status: enum_label(&run.status),
            run_outcome: None,
            session_tree: ConversationSessionTreeVm {
                rounds: Vec::new(),
                selected_session_key: None,
            },
            selected_session: None,
            active_sessions: Vec::new(),
            artifacts: Vec::new(),
            attachments: Vec::new(),
            input_attachments: Vec::new(),
            workflow_status: "valid".to_string(),
            workflow_valid: true,
            workflow_error: None,
            workflow_json: None,
            workflow_graph: GraphVm {
                nodes: Vec::new(),
                edges: Vec::new(),
            },
            resumable: false,
            pause_reason: None,
            runtime_error_message: None,
            scheduled_task_id: None,
        })
    })
}

pub fn switch_conversation_session_vm(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<&str>,
    outer_attempt_id: Option<&str>,
) -> anyhow::Result<ConversationSessionSwitchVm> {
    let selected_session =
        if let (Some(outer_id), Some(outer_attempt)) = (outer_node_id, outer_attempt_id) {
            crate::view_models::dynamic_acp_session_vm(
                app,
                task_id,
                run_id,
                round_id,
                outer_id,
                outer_attempt,
                node_id,
                attempt_id,
                None,
                None,
            )
            .ok()
            .flatten()
        } else {
            crate::view_models::acp_session_vm(
                app, task_id, run_id, round_id, node_id, attempt_id, None, None,
            )
            .ok()
            .flatten()
        };

    let (artifacts, attachments) = conversation_session_assets(
        app,
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    )?;

    let result = ConversationSessionSwitchVm {
        selected_session,
        artifacts,
        attachments,
    };
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{
        ConversationAutoConfigVm, ConversationCreateInputVm, ConversationDirectConfigVm,
        ConversationDynamicAgentRefVm, ConversationWorkspaceSource, ConversationWorkspaceVm,
        PromptActivity, apply_workflow_interview_preference, build_auto_workflow,
        build_direct_workflow, conversation_attempt_lifecycle_vm, conversation_auto_title,
        conversation_run_vm, conversation_sidebar_vm_from_sources,
        conversation_status_from_session, conversation_workspace_vms, create_conversation_task_vm,
        derive_conversation_attempt_lifecycle, lifecycle_is_active, scheduled_task_schedule_label,
        scheduled_task_timezone_label, scheduled_task_vms_from_sources,
        switch_conversation_session_vm,
    };
    use camino::Utf8PathBuf;
    use chrono::TimeZone;
    use gold_band::app::{App, CreateTaskInput};
    use gold_band::config::ConversationRunMode;
    use gold_band::dsl::{AiDynamicAgentStrategy, NodeDsl, PromptEnvelopeMode};
    use serde_json::json;

    fn workflow_with_interview() -> gold_band::dsl::WorkflowDsl {
        serde_json::from_value(json!({
            "version": "0.1",
            "id": "workflow-with-interview",
            "entry": "interview",
            "control": {},
            "nodes": [
                { "type": "worker", "id": "interview", "provider": "claude-acp" },
                { "type": "worker", "id": "plan", "provider": "claude-acp" }
            ],
            "edges": [
                { "from": "interview", "to": "plan", "on": "success" },
                { "from": "plan", "to": "$end", "on": "success" }
            ]
        }))
        .unwrap()
    }

    #[test]
    fn interview_preference_only_changes_the_builtin_default_workflow() {
        let mut custom = workflow_with_interview();
        let result =
            apply_workflow_interview_preference("custom", Some(false), &mut custom).unwrap();
        assert_eq!(result, None);
        assert_eq!(custom.entry, "interview");
        assert!(custom.nodes.iter().any(|node| node.id() == "interview"));

        let mut default = workflow_with_interview();
        let result =
            apply_workflow_interview_preference("default", Some(false), &mut default).unwrap();
        assert_eq!(result, Some(false));
        assert_eq!(default.entry, "plan");
        assert!(!default.nodes.iter().any(|node| node.id() == "interview"));
    }

    #[test]
    fn default_workflow_interview_is_enabled_when_the_preference_is_missing() {
        let mut workflow = workflow_with_interview();
        let result = apply_workflow_interview_preference("default", None, &mut workflow).unwrap();
        assert_eq!(result, Some(true));
        assert_eq!(workflow.entry, "interview");
    }

    #[test]
    fn conversation_auto_title_uses_configured_character_limit() {
        let content = "在.claude下输出两个python类，一个输出hello，一个输出good bye";

        assert_eq!(conversation_auto_title(content, 12), "在.claude下输出两");
        assert_eq!(
            conversation_auto_title(content, 20),
            "在.claude下输出两个python类"
        );
        assert_eq!(conversation_auto_title("", 20), "New Task");
    }

    #[test]
    fn paused_runtime_keeps_paused_status_after_process_interrupt() {
        let status = conversation_status_from_session(
            Some("cancelled"),
            "paused",
            Some("process-interrupted"),
            true,
        );

        assert_eq!(status, "paused");
    }

    #[test]
    fn running_runtime_overrides_stale_session_cancelled_status_without_launching_next_node() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            Some("cancelled"),
            None,
            "running",
            None,
            true,
            None,
            false,
            false,
        );

        assert_eq!(lifecycle.display_status, "running");
        assert!(lifecycle.runtime.active);
        assert_eq!(lifecycle.runtime.phase, "provider-running");
        assert!(!lifecycle.acp.active);
        assert!(lifecycle_is_active(&lifecycle, false));
        assert_eq!(lifecycle.composer.mode, "runtime-active");
        assert_eq!(lifecycle.composer.processing_kind, "processing");
        assert_eq!(
            lifecycle.composer.status_key.as_deref(),
            Some("conversation.runtime.runtimeActive")
        );
    }

    #[test]
    fn running_runtime_with_completed_session_launches_next_node() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            Some("completed"),
            None,
            "running",
            None,
            true,
            None,
            false,
            false,
        );

        assert_eq!(lifecycle.runtime.phase, "launching-next-node");
        assert_eq!(lifecycle.composer.processing_kind, "launching-next-node");
        assert_eq!(
            lifecycle.composer.status_key.as_deref(),
            Some("conversation.runtime.launchingNextNode")
        );
    }

    #[test]
    fn non_resumable_runtime_still_uses_session_terminal_status() {
        let status = conversation_status_from_session(
            Some("cancelled"),
            "paused",
            Some("process-interrupted"),
            false,
        );

        assert_eq!(status, "cancelled");
    }

    #[test]
    fn acp_cancelling_keeps_leaf_active_and_stopping() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            Some("cancelling"),
            None,
            "paused",
            None,
            true,
            Some("process-interrupted"),
            true,
            false,
        );

        assert_eq!(lifecycle.display_status, "cancelling");
        assert!(lifecycle.acp.active);
        assert!(lifecycle.acp.stopping);
        assert!(lifecycle_is_active(&lifecycle, false));
        assert_eq!(lifecycle.composer.mode, "stopping");
        assert_eq!(lifecycle.composer.submit_target, "none");
        assert_eq!(lifecycle.composer.processing_kind, "stopping");
        assert!(lifecycle.composer.lock_input);
    }

    #[test]
    fn completed_runtime_suppresses_stale_acp_running() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            Some("running"),
            None,
            "completed",
            Some("success"),
            false,
            None,
            false,
            false,
        );

        assert_eq!(lifecycle.display_status, "completed");
        assert!(!lifecycle.runtime.active);
        assert!(!lifecycle.acp.active);
        assert!(lifecycle.acp.terminal);
        assert!(!lifecycle_is_active(&lifecycle, false));
    }

    #[test]
    fn completed_runtime_keeps_live_follow_up_running() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            Some("running"),
            Some(PromptActivity::Running),
            "completed",
            Some("success"),
            false,
            None,
            false,
            false,
        );

        assert_eq!(lifecycle.display_status, "running");
        assert_eq!(lifecycle.acp.phase.as_deref(), Some("running"));
        assert!(lifecycle.acp.active);
        assert!(!lifecycle.acp.terminal);
        assert_eq!(lifecycle.runtime.phase, "provider-running");
        assert_eq!(lifecycle.composer.mode, "runtime-active");
        assert!(lifecycle.composer.can_stop);
        assert!(lifecycle.composer.lock_input);
    }

    #[test]
    fn completed_runtime_exposes_starting_follow_up() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            Some("completed"),
            Some(PromptActivity::Starting),
            "completed",
            Some("success"),
            false,
            None,
            false,
            false,
        );

        assert_eq!(lifecycle.display_status, "starting");
        assert_eq!(lifecycle.runtime.phase, "launching-session");
        assert_eq!(lifecycle.composer.processing_kind, "launching");
        assert!(lifecycle.acp.active);
    }

    #[test]
    fn completed_runtime_exposes_accepted_follow_up_as_processing() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            Some("running"),
            Some(PromptActivity::Accepted),
            "completed",
            Some("success"),
            false,
            None,
            false,
            false,
        );

        assert_eq!(lifecycle.display_status, "running");
        assert_eq!(lifecycle.acp.phase.as_deref(), Some("accepted"));
        assert!(lifecycle.acp.active);
        assert_eq!(lifecycle.composer.processing_kind, "processing");
    }

    #[test]
    fn completed_runtime_exposes_follow_up_cancel_request() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            Some("running"),
            Some(PromptActivity::CancelRequested),
            "completed",
            Some("success"),
            false,
            None,
            false,
            false,
        );

        assert_eq!(lifecycle.display_status, "cancelling");
        assert_eq!(lifecycle.acp.phase.as_deref(), Some("cancel-requested"));
        assert!(lifecycle.acp.stopping);
        assert_eq!(lifecycle.composer.mode, "stopping");
    }

    #[test]
    fn workflow_failure_runtime_suppresses_stale_acp_running() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            Some("running"),
            None,
            "completed",
            Some("failure"),
            false,
            None,
            false,
            false,
        );

        assert_eq!(lifecycle.display_status, "completed");
        assert_eq!(lifecycle.runtime_display.tone, "danger");
        assert!(!lifecycle.runtime_display.blocking_error);
        assert!(!lifecycle.acp.active);
        assert!(!lifecycle_is_active(&lifecycle, false));
    }

    #[test]
    fn interrupted_runtime_pause_is_input_continue() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            Some("cancelled"),
            None,
            "paused",
            None,
            true,
            Some("process-interrupted"),
            true,
            false,
        );

        assert_eq!(lifecycle.display_status, "paused");
        assert_eq!(lifecycle.continue_kind.as_deref(), Some("input"));
        assert!(lifecycle.runtime.continuable);
        assert_eq!(lifecycle.runtime.phase, "paused");
        assert_eq!(lifecycle.composer.mode, "interrupted-input");
        assert_eq!(lifecycle.composer.submit_target, "runtime-continue");
        assert!(!lifecycle.composer.lock_input);
    }

    #[test]
    fn runtime_abnormal_pause_is_input_continue_even_when_acp_failed() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            Some("failed"),
            None,
            "paused",
            None,
            true,
            Some("runtime-abnormal"),
            true,
            false,
        );

        assert_eq!(lifecycle.display_status, "paused");
        assert_eq!(lifecycle.continue_kind.as_deref(), Some("input"));
        assert!(lifecycle.runtime.continuable);
        assert_eq!(
            lifecycle.runtime.pause_reason.as_deref(),
            Some("runtime-abnormal")
        );
        assert_eq!(lifecycle.composer.mode, "interrupted-input");
        assert_eq!(lifecycle.composer.submit_target, "runtime-continue");
        assert!(!lifecycle.composer.lock_input);
    }

    #[test]
    fn current_unexplained_paused_provider_failure_stays_runtime_active() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            Some("failed"),
            None,
            "paused",
            None,
            true,
            None,
            false,
            false,
        );

        assert_eq!(lifecycle.display_status, "paused");
        assert!(lifecycle.runtime.active);
        assert!(!lifecycle.runtime_display.blocking_error);
        assert_eq!(lifecycle.composer.mode, "runtime-active");
        assert_eq!(lifecycle.composer.submit_target, "none");
        assert!(lifecycle.composer.lock_input);
    }

    #[test]
    fn unexplained_paused_provider_failure_stays_runtime_active_without_current_marker() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            Some("failed"),
            None,
            "paused",
            None,
            false,
            None,
            false,
            false,
        );

        assert_eq!(lifecycle.display_status, "paused");
        assert!(lifecycle.runtime.active);
        assert!(!lifecycle.runtime_display.blocking_error);
        assert_eq!(lifecycle.composer.mode, "runtime-active");
        assert_eq!(lifecycle.composer.submit_target, "none");
        assert!(lifecycle.composer.lock_input);
    }

    #[test]
    fn manual_check_waiting_for_user_input_keeps_acp_prompt_available() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            None,
            None,
            "paused",
            None,
            true,
            Some("waiting-for-user-input"),
            true,
            true,
        );

        assert_eq!(lifecycle.display_status, "paused");
        assert_eq!(lifecycle.continue_kind, None);
        assert!(!lifecycle.runtime.continuable);
        assert_eq!(lifecycle.composer.mode, "normal");
        assert_eq!(lifecycle.composer.submit_target, "acp-prompt");
        assert!(!lifecycle.composer.lock_input);
        assert!(lifecycle_is_active(&lifecycle, true));
    }

    #[test]
    fn error_blocked_runtime_pause_is_runtime_error() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            Some("failed"),
            None,
            "paused",
            None,
            true,
            Some("error-blocked"),
            true,
            false,
        );

        assert_eq!(lifecycle.display_status, "paused");
        assert_eq!(lifecycle.continue_kind, None);
        assert_eq!(lifecycle.composer.mode, "runtime-error");
        assert_eq!(lifecycle.composer.submit_target, "none");
    }

    #[test]
    fn runtime_error_summary_keeps_json_error_details() {
        let message = super::runtime_error_message_from_summary(
            r#"run run-021 blocked at round-001/ai-dynamic/attempt-001: provider `claude-acp` failed to run `good-morning`: ACP `session/set_config_option` failed: {"code":-32603,"data":{"details":"Invalid value for config option model: claude-sonnet-4-6"},"message":"Internal error"}"#,
        );

        assert_eq!(
            message.as_deref(),
            Some(
                "provider `claude-acp` failed to run `good-morning`: ACP `session/set_config_option` failed: Invalid value for config option model: claude-sonnet-4-6 (Internal error)"
            )
        );
    }

    #[test]
    fn running_parent_dynamic_leaf_pause_is_runtime_continue() {
        let repo_root = temp_repo_root();
        let app = App::new(repo_root);
        write_dynamic_lifecycle_fixture(&app, "running", json!(null), "paused", vec!["good-night"]);

        let lifecycle = conversation_attempt_lifecycle_vm(
            &app,
            "task-dyn",
            "run-dyn",
            "round-001",
            "good-morning",
            "attempt-001",
            Some("ai-dynamic"),
            Some("attempt-001"),
        )
        .unwrap();

        assert_eq!(lifecycle.runtime.status, "paused");
        assert_eq!(
            lifecycle.runtime.pause_reason.as_deref(),
            Some("process-interrupted")
        );
        assert_eq!(lifecycle.continue_kind.as_deref(), Some("input"));
        assert_eq!(lifecycle.composer.mode, "interrupted-input");
        assert_eq!(lifecycle.composer.submit_target, "runtime-continue");
    }

    #[test]
    fn paused_parent_stale_cancelled_dynamic_leaf_is_runtime_continue() {
        let repo_root = temp_repo_root();
        let app = App::new(repo_root);
        write_dynamic_lifecycle_fixture_with_cancelled_session(
            &app,
            "paused",
            json!("process-interrupted"),
            "running",
            Vec::new(),
            true,
        );

        let lifecycle = conversation_attempt_lifecycle_vm(
            &app,
            "task-dyn",
            "run-dyn",
            "round-001",
            "good-morning",
            "attempt-001",
            Some("ai-dynamic"),
            Some("attempt-001"),
        )
        .unwrap();

        assert_eq!(lifecycle.runtime.status, "paused");
        assert_eq!(
            lifecycle.runtime.pause_reason.as_deref(),
            Some("process-interrupted")
        );
        assert_eq!(lifecycle.continue_kind.as_deref(), Some("input"));
        assert_eq!(lifecycle.composer.submit_target, "runtime-continue");
    }

    #[test]
    fn paused_parent_suppresses_stale_dynamic_leaf_launching_state() {
        let repo_root = temp_repo_root();
        let app = App::new(repo_root);
        write_dynamic_lifecycle_fixture(
            &app,
            "paused",
            json!("process-interrupted"),
            "running",
            Vec::new(),
        );

        let lifecycle = conversation_attempt_lifecycle_vm(
            &app,
            "task-dyn",
            "run-dyn",
            "round-001",
            "good-morning",
            "attempt-001",
            Some("ai-dynamic"),
            Some("attempt-001"),
        )
        .unwrap();

        assert_eq!(lifecycle.runtime.status, "paused");
        assert_eq!(lifecycle.runtime.phase, "paused");
        assert_eq!(lifecycle.composer.processing_kind, "processing");
        assert_eq!(lifecycle.continue_kind.as_deref(), Some("input"));
    }

    #[test]
    fn paused_parent_runtime_abnormal_dynamic_leaf_is_runtime_continue() {
        let repo_root = temp_repo_root();
        let app = App::new(repo_root);
        write_dynamic_lifecycle_fixture(
            &app,
            "paused",
            json!("runtime-abnormal"),
            "paused",
            Vec::new(),
        );

        let lifecycle = conversation_attempt_lifecycle_vm(
            &app,
            "task-dyn",
            "run-dyn",
            "round-001",
            "good-morning",
            "attempt-001",
            Some("ai-dynamic"),
            Some("attempt-001"),
        )
        .unwrap();

        assert_eq!(lifecycle.display_status, "paused");
        assert_eq!(lifecycle.runtime.status, "paused");
        assert_eq!(
            lifecycle.runtime.pause_reason.as_deref(),
            Some("runtime-abnormal")
        );
        assert_eq!(lifecycle.continue_kind.as_deref(), Some("input"));
        assert_eq!(lifecycle.composer.mode, "interrupted-input");
        assert_eq!(lifecycle.composer.submit_target, "runtime-continue");
        assert!(!lifecycle.composer.lock_input);
    }

    #[test]
    fn dynamic_leaf_pause_reason_overrides_legacy_graph_reason() {
        let repo_root = temp_repo_root();
        let app = App::new(repo_root);
        write_dynamic_lifecycle_fixture(
            &app,
            "paused",
            json!("process-interrupted"),
            "paused",
            Vec::new(),
        );
        write_dynamic_node_pause_details(
            &app,
            "runtime-abnormal",
            Some("session/set_config_option: failed to persist config.toml"),
        );

        let lifecycle = conversation_attempt_lifecycle_vm(
            &app,
            "task-dyn",
            "run-dyn",
            "round-001",
            "good-morning",
            "attempt-001",
            Some("ai-dynamic"),
            Some("attempt-001"),
        )
        .unwrap();

        assert_eq!(
            lifecycle.runtime.pause_reason.as_deref(),
            Some("runtime-abnormal")
        );
        assert_eq!(lifecycle.composer.mode, "interrupted-input");
    }

    #[test]
    fn selected_dynamic_leaf_runtime_error_overrides_run_fallback() {
        let repo_root = temp_repo_root();
        let app = App::new(repo_root);
        write_dynamic_lifecycle_fixture(
            &app,
            "paused",
            json!("process-interrupted"),
            "paused",
            Vec::new(),
        );
        write_dynamic_node_pause_details(
            &app,
            "runtime-abnormal",
            Some("provider `codex-acp`: session/set_config_option: failed to persist config.toml"),
        );

        let vm = conversation_run_vm(&app, "default", "task-dyn", "run-dyn", None).unwrap();

        assert_eq!(
            vm.runtime_error_message.as_deref(),
            Some("provider `codex-acp`: session/set_config_option: failed to persist config.toml")
        );
    }

    #[test]
    fn dynamic_leaf_provider_failure_does_not_flash_runtime_error_before_pause_reason() {
        let repo_root = temp_repo_root();
        let app = App::new(repo_root);
        write_dynamic_lifecycle_fixture_with_cancelled_session(
            &app,
            "running",
            json!(null),
            "paused",
            vec!["good-morning"],
            false,
        );
        gold_band::storage::write_json(
            &app.paths
                .dynamic_node_attempt_dir(
                    "task-dyn",
                    "run-dyn",
                    "round-001",
                    "ai-dynamic",
                    "attempt-001",
                    "good-morning",
                    "attempt-001",
                )
                .join("acp.session.json"),
            &json!({
                "status": "failed",
                "stopReason": "error",
                "sessionId": "session-good-morning",
                "messages": []
            }),
        )
        .unwrap();

        let lifecycle = conversation_attempt_lifecycle_vm(
            &app,
            "task-dyn",
            "run-dyn",
            "round-001",
            "good-morning",
            "attempt-001",
            Some("ai-dynamic"),
            Some("attempt-001"),
        )
        .unwrap();

        assert_eq!(lifecycle.display_status, "paused");
        assert!(lifecycle.runtime.active);
        assert!(!lifecycle.runtime_display.blocking_error);
        assert_eq!(lifecycle.composer.mode, "runtime-active");
        assert_eq!(lifecycle.composer.submit_target, "none");
    }

    #[test]
    fn running_dynamic_leaf_with_terminal_acp_still_launches_next_node() {
        let repo_root = temp_repo_root();
        let app = App::new(repo_root);
        write_dynamic_lifecycle_fixture(
            &app,
            "running",
            json!(null),
            "running",
            vec!["good-morning"],
        );
        gold_band::storage::write_json(
            &app.paths
                .dynamic_node_attempt_dir(
                    "task-dyn",
                    "run-dyn",
                    "round-001",
                    "ai-dynamic",
                    "attempt-001",
                    "good-morning",
                    "attempt-001",
                )
                .join("acp.session.json"),
            &json!({
                "status": "completed",
                "sessionId": "session-good-morning",
                "messages": []
            }),
        )
        .unwrap();

        let lifecycle = conversation_attempt_lifecycle_vm(
            &app,
            "task-dyn",
            "run-dyn",
            "round-001",
            "good-morning",
            "attempt-001",
            Some("ai-dynamic"),
            Some("attempt-001"),
        )
        .unwrap();

        assert_eq!(lifecycle.runtime.status, "running");
        assert_eq!(lifecycle.runtime.phase, "launching-next-node");
        assert_eq!(lifecycle.composer.processing_kind, "launching-next-node");
        assert_eq!(lifecycle.continue_kind, None);
    }

    #[test]
    fn error_blocked_dynamic_leaf_is_selected_runtime_error() {
        let repo_root = temp_repo_root();
        let app = App::new(repo_root);
        write_dynamic_lifecycle_fixture(
            &app,
            "paused",
            json!("error-blocked"),
            "paused",
            Vec::new(),
        );
        gold_band::storage::write_json(
            &app.paths.run_progress_file("task-dyn", "run-dyn"),
            &json!({
                "version": gold_band::domain::VERSION,
                "status": "paused",
                "currentRoundId": "round-001",
                "currentNodeId": "ai-dynamic",
                "currentAttemptId": "attempt-001",
                "currentStage": "blocked",
                "summary": "run run-dyn blocked at round-001/ai-dynamic/attempt-001: ACP prompt cancelled",
                "updatedAt": "2026-06-15T00:00:02Z"
            }),
        )
        .unwrap();

        let vm = conversation_run_vm(&app, "default", "task-dyn", "run-dyn", None).unwrap();
        let leaf = vm.session_tree.rounds[0].nodes[0]
            .outer_nodes
            .as_ref()
            .unwrap()[0]
            .attempts[0]
            .clone();

        assert_eq!(
            vm.session_tree.selected_session_key.as_deref(),
            Some("round-001/ai-dynamic/attempt-001/good-morning/attempt-001")
        );
        assert!(leaf.current);
        assert_eq!(
            leaf.lifecycle.runtime.pause_reason.as_deref(),
            Some("error-blocked")
        );
        assert_eq!(leaf.runtime_display.code, "error-blocked");
        assert!(leaf.runtime_display.blocking_error);
        assert_eq!(leaf.lifecycle.composer.mode, "runtime-error");
        assert_eq!(
            vm.runtime_error_message.as_deref(),
            Some("ACP prompt cancelled")
        );
    }

    #[test]
    fn ready_dynamic_child_without_attempt_is_active_launching_leaf() {
        let repo_root = temp_repo_root();
        let app = App::new(repo_root);
        write_dynamic_lifecycle_fixture(
            &app,
            "running",
            json!(null),
            "ready",
            vec!["good-morning"],
        );

        let vm = conversation_run_vm(&app, "default", "task-dyn", "run-dyn", None).unwrap();
        let child = vm.session_tree.rounds[0].nodes[0]
            .outer_nodes
            .as_ref()
            .unwrap()[0]
            .clone();
        assert_eq!(child.attempts.len(), 1);
        assert_eq!(child.attempts[0].attempt_id, "attempt-001");
        assert_eq!(child.attempts[0].lifecycle.runtime.status, "ready");
        assert_eq!(
            child.attempts[0].lifecycle.runtime.phase,
            "launching-session"
        );
        assert!(vm.active_sessions.iter().any(|session| {
            session.node_id == "good-morning" && session.attempt_id == "attempt-001"
        }));
    }

    #[test]
    fn build_auto_workflow_preserves_dynamic_acceptance_model() {
        let workflow = build_auto_workflow(Some(&ConversationAutoConfigVm {
            agent_strategy: Some("dynamic".to_string()),
            agent_type: "claude-acp".to_string(),
            bootstrap_agent_type: Some("claude-acp".to_string()),
            bootstrap_model_id: Some("bootstrap-model".to_string()),
            acceptance_model_id: Some("accept-model".to_string()),
            model_id: None,
            permission_mode: None,
            config_options: Default::default(),
            available_agents: Some(vec![ConversationDynamicAgentRefVm {
                provider: "claude-acp".to_string(),
                model: Some("worker-model".to_string()),
            }]),
            routing_prompt: Some("Pick worker models explicitly".to_string()),
            allowed_workflows: None,
            allowed_profiles: None,
            global_goal: None,
            control: None,
            active_template_id: None,
            active_template_name: None,
        }));

        let NodeDsl::AiDynamic(node) = &workflow.nodes[0] else {
            panic!("expected ai-dynamic node");
        };
        match &node.agent_strategy {
            AiDynamicAgentStrategy::Dynamic {
                bootstrap_model,
                acceptance_model,
                available_agents,
                ..
            } => {
                assert_eq!(bootstrap_model.as_deref(), Some("bootstrap-model"));
                assert_eq!(acceptance_model.as_deref(), Some("accept-model"));
                assert_eq!(available_agents[0].model.as_deref(), Some("worker-model"));
            }
            other => panic!("expected dynamic strategy, got {other:?}"),
        }
    }

    #[test]
    fn build_direct_workflow_uses_one_raw_agent_worker() {
        let workflow = build_direct_workflow(&ConversationDirectConfigVm {
            agent_type: "codex-acp".to_string(),
            model_id: Some("gpt-direct".to_string()),
            permission_mode: Some("ask".to_string()),
            config_options: Default::default(),
        });

        assert_eq!(workflow.entry, "direct-agent");
        assert_eq!(workflow.nodes.len(), 1);
        assert_eq!(workflow.edges.len(), 1);
        let NodeDsl::Worker(worker) = &workflow.nodes[0] else {
            panic!("expected worker node");
        };
        assert_eq!(worker.provider.as_deref(), Some("codex-acp"));
        assert_eq!(worker.model.as_deref(), Some("gpt-direct"));
        assert_eq!(worker.permission_mode.as_deref(), Some("ask"));
        assert_eq!(worker.prompt_envelope, PromptEnvelopeMode::RawAgent);
        assert!(worker.profile.is_none());
        assert!(worker.goal.is_none());
        assert!(worker.output.is_none());
    }

    #[test]
    fn direct_task_creation_does_not_require_role_metadata() {
        let app = App::new(temp_repo_root());
        let workflow = build_direct_workflow(&ConversationDirectConfigVm {
            agent_type: "claude-acp".to_string(),
            model_id: None,
            permission_mode: None,
            config_options: Default::default(),
        });

        let created = app.create_task_from_requirement(CreateTaskInput {
            title: Some("Direct task".to_string()),
            description: None,
            requirement_file_name: None,
            requirement_content: "hello".to_string(),
            workflow,
            workflow_template_id: None,
        });

        assert!(created.is_ok(), "{created:?}");
    }

    #[test]
    fn scheduled_task_materialization_creates_task_without_starting_run() {
        let app = App::new(temp_repo_root());
        let input = ConversationCreateInputVm {
            project_id: app.paths.project_id.clone(),
            content: "run this later".to_string(),
            run_mode: ConversationRunMode::Direct.as_str().to_string(),
            workflow_template_id: None,
            include_interview: None,
            direct_config: Some(ConversationDirectConfigVm {
                agent_type: "claude-acp".to_string(),
                model_id: None,
                permission_mode: None,
                config_options: Default::default(),
            }),
            auto_config: None,
            attachment_paths: None,
            scheduled_task_id: None,
            scheduled_content_fingerprint: None,
        };

        let (task_id, _, _) = create_conversation_task_vm(&app, &input).unwrap();
        assert!(app.paths.task_file(&task_id).exists());
        assert!(!app.paths.runs_dir(&task_id).exists());
    }

    #[test]
    fn conversation_sidebar_vm_groups_tasks_by_workspace_source() {
        let repo_a = temp_repo_root();
        let repo_b = temp_repo_root();
        let app_a = App::new(repo_a.clone());
        let app_b = App::new(repo_b.clone());
        write_sidebar_task_fixture(&app_a, "task-a", "Task A", "run-a", "2026-06-15T00:00:00Z");
        write_sidebar_task_fixture(&app_b, "task-b", "Task B", "run-b", "2026-06-15T00:01:00Z");

        let state = gold_band::config::StateConfig::default();
        let sources = vec![
            ConversationWorkspaceSource {
                workspace: ConversationWorkspaceVm {
                    project_id: "workspace-a".to_string(),
                    workspace_path: repo_a.to_string(),
                    name: "Workspace A".to_string(),
                },
                app: app_a.clone_for_background(),
            },
            ConversationWorkspaceSource {
                workspace: ConversationWorkspaceVm {
                    project_id: "workspace-b".to_string(),
                    workspace_path: repo_b.to_string(),
                    name: "Workspace B".to_string(),
                },
                app: app_b.clone_for_background(),
            },
        ];

        let vm = conversation_sidebar_vm_from_sources(&state, &sources);

        assert_eq!(vm.workspaces.len(), 2);
        assert_eq!(vm.tasks_by_workspace["workspace-a"][0].task_id, "task-a");
        assert_eq!(
            vm.tasks_by_workspace["workspace-a"][0].project_id,
            "workspace-a"
        );
        assert_eq!(vm.tasks_by_workspace["workspace-b"][0].task_id, "task-b");
        assert_eq!(
            vm.tasks_by_workspace["workspace-b"][0].project_id,
            "workspace-b"
        );
    }

    #[test]
    fn conversation_sidebar_sorts_all_task_modes_by_normalized_last_activity() {
        let repo = temp_repo_root();
        let app = App::new(repo.clone());
        write_sidebar_task_fixture_with_updated_at(
            &app,
            "task-workflow",
            "Workflow task",
            "run-001",
            "1000000000Z",
            "2000000000Z",
        );
        write_sidebar_task_fixture_with_updated_at(
            &app,
            "task-direct",
            "Direct task",
            "run-001",
            "2026-07-24T00:00:00Z",
            "2026-07-24T00:00:00Z",
        );
        write_sidebar_conversation_metadata_fixture(
            &app,
            "task-direct",
            "direct",
            "2026-07-24T00:00:00Z",
        );
        let state = gold_band::config::StateConfig::default();
        let sources = vec![ConversationWorkspaceSource {
            workspace: ConversationWorkspaceVm {
                project_id: "workspace-a".to_string(),
                workspace_path: repo.to_string(),
                name: "Workspace A".to_string(),
            },
            app: app.clone_for_background(),
        }];

        let vm = conversation_sidebar_vm_from_sources(&state, &sources);
        let tasks = &vm.tasks_by_workspace["workspace-a"];

        assert_eq!(tasks[0].task_id, "task-workflow");
        assert_eq!(tasks[0].last_activity_at.as_deref(), Some("2000000000Z"));
        assert_eq!(tasks[1].task_id, "task-direct");
    }

    #[test]
    fn conversation_sidebar_orders_runs_and_latest_run_by_updated_at() {
        let repo = temp_repo_root();
        let app = App::new(repo.clone());
        write_sidebar_task_fixture_with_updated_at(
            &app,
            "task-a",
            "Task A",
            "run-001",
            "1000000000Z",
            "3000000000Z",
        );
        write_sidebar_task_fixture_with_updated_at(
            &app,
            "task-a",
            "Task A",
            "run-002",
            "2000000000Z",
            "2500000000Z",
        );
        let state = gold_band::config::StateConfig::default();
        let sources = vec![ConversationWorkspaceSource {
            workspace: ConversationWorkspaceVm {
                project_id: "workspace-a".to_string(),
                workspace_path: repo.to_string(),
                name: "Workspace A".to_string(),
            },
            app: app.clone_for_background(),
        }];

        let vm = conversation_sidebar_vm_from_sources(&state, &sources);
        let task = &vm.tasks_by_workspace["workspace-a"][0];

        assert_eq!(
            task.latest_run.as_ref().map(|run| run.run_id.as_str()),
            Some("run-001")
        );
        assert_eq!(
            task.runs
                .iter()
                .map(|run| run.run_id.as_str())
                .collect::<Vec<_>>(),
            vec!["run-001", "run-002"]
        );
        assert_eq!(task.last_activity_at.as_deref(), Some("3000000000Z"));
    }

    #[test]
    fn conversation_sidebar_vm_prioritizes_last_workspace() {
        let repo_a = temp_repo_root();
        let repo_b = temp_repo_root();
        let app_a = App::new(repo_a.clone());
        let app_b = App::new(repo_b.clone());
        let mut state = gold_band::config::StateConfig::default();
        state.last_conversation_workspace = Some("workspace-b".to_string());
        let sources = vec![
            ConversationWorkspaceSource {
                workspace: ConversationWorkspaceVm {
                    project_id: "workspace-a".to_string(),
                    workspace_path: repo_a.to_string(),
                    name: "Workspace A".to_string(),
                },
                app: app_a.clone_for_background(),
            },
            ConversationWorkspaceSource {
                workspace: ConversationWorkspaceVm {
                    project_id: "workspace-b".to_string(),
                    workspace_path: repo_b.to_string(),
                    name: "Workspace B".to_string(),
                },
                app: app_b.clone_for_background(),
            },
        ];

        let vm = conversation_sidebar_vm_from_sources(&state, &sources);

        assert_eq!(vm.last_active_workspace_id.as_deref(), Some("workspace-b"));
        assert_eq!(vm.workspaces[0].project_id, "workspace-b");
        assert_eq!(vm.workspaces[1].project_id, "workspace-a");
    }

    #[test]
    fn conversation_workspace_vms_returns_only_workspace_metadata() {
        let mut state = gold_band::config::StateConfig::default();
        state.conversation_workspaces = vec![
            gold_band::config::ConversationWorkspaceEntry {
                project_id: "workspace-a".to_string(),
                workspace_path: "D:/Workspace/A".to_string(),
                name: "Workspace A".to_string(),
                added_at: "2026-07-26T00:00:00Z".to_string(),
            },
            gold_band::config::ConversationWorkspaceEntry {
                project_id: "workspace-b".to_string(),
                workspace_path: "D:/Workspace/B".to_string(),
                name: "Workspace B".to_string(),
                added_at: "2026-07-26T00:01:00Z".to_string(),
            },
        ];
        state.last_conversation_workspace = Some("workspace-b".to_string());

        let workspaces = conversation_workspace_vms(&state);

        assert_eq!(workspaces.len(), 2);
        assert_eq!(workspaces[0].project_id, "workspace-b");
        assert_eq!(workspaces[1].project_id, "workspace-a");
    }

    #[test]
    fn conversation_sidebar_keeps_task_when_run_history_is_unreadable() {
        let repo = temp_repo_root();
        let app = App::new(repo.clone());
        write_sidebar_task_fixture(
            &app,
            "task-broken-run",
            "Broken run history",
            "run-001",
            "2026-07-26T00:00:00Z",
        );
        std::fs::write(
            app.paths
                .run_file("task-broken-run", "run-001")
                .as_std_path(),
            "{invalid-json",
        )
        .unwrap();
        let state = gold_band::config::StateConfig::default();
        let sources = vec![ConversationWorkspaceSource {
            workspace: ConversationWorkspaceVm {
                project_id: "workspace-a".to_string(),
                workspace_path: repo.to_string(),
                name: "Workspace A".to_string(),
            },
            app: app.clone_for_background(),
        }];

        let vm = conversation_sidebar_vm_from_sources(&state, &sources);

        let task = &vm.tasks_by_workspace["workspace-a"][0];
        assert_eq!(task.task_id, "task-broken-run");
        assert!(task.latest_run.is_none());
        assert!(task.runs.is_empty());
    }

    #[test]
    fn conversation_run_vm_exposes_selected_session_assets_and_leaf_counts() {
        let repo_root = temp_repo_root();
        let app = App::new(repo_root);
        write_conversation_assets_fixture(&app);

        let vm = conversation_run_vm(
            &app,
            "project-001",
            "task-046",
            "run-060",
            Some("round-001/测试/attempt-002"),
        )
        .unwrap();

        assert_eq!(vm.artifacts.len(), 1);
        assert_eq!(vm.artifacts[0].name, "测试-result");
        assert_eq!(vm.attachments.len(), 1);
        assert_eq!(vm.attachments[0].name, "test-report.md");
        assert_eq!(vm.task_uuid.as_deref(), Some("task-046-fixture-uuid"));

        let leaf = vm.session_tree.rounds[0].nodes[0]
            .attempts
            .iter()
            .find(|leaf| leaf.attempt_id == "attempt-002")
            .unwrap();
        assert_eq!(leaf.artifact_count, 1);
        assert_eq!(leaf.attachment_count, 1);
    }

    #[test]
    fn conversation_run_vm_ignores_unreadable_timeline_for_non_selected_session() {
        let repo_root = temp_repo_root();
        let app = App::new(repo_root);
        write_conversation_assets_fixture(&app);
        let attempt_dir =
            app.paths
                .attempt_dir("task-046", "run-060", "round-001", "测试", "attempt-001");
        std::fs::create_dir_all(attempt_dir.as_std_path()).unwrap();
        gold_band::storage::write_json(
            &app.paths
                .node_file("task-046", "run-060", "round-001", "测试", "attempt-001"),
            &json!({
                "version": gold_band::domain::VERSION,
                "node_id": "测试",
                "node_type": "worker",
                "run_id": "run-060",
                "round_id": "round-001",
                "attempt_id": "attempt-001",
                "status": "completed",
                "outcome": "success",
                "started_at": "2026-06-15T00:00:00Z",
                "finished_at": "2026-06-15T00:00:01Z",
                "manual_check_pending": false,
                "resolved_config": {}
            }),
        )
        .unwrap();
        std::fs::create_dir_all(attempt_dir.join("acp.timeline.jsonl").as_std_path()).unwrap();

        let vm = conversation_run_vm(
            &app,
            "project-001",
            "task-046",
            "run-060",
            Some("round-001/测试/attempt-002"),
        )
        .unwrap();

        assert_eq!(vm.session_tree.rounds[0].nodes[0].attempts.len(), 2);
        assert_eq!(
            vm.session_tree.selected_session_key.as_deref(),
            Some("round-001/测试/attempt-002")
        );
    }

    #[test]
    fn conversation_session_tree_orders_nodes_by_round_trace() {
        let repo_root = temp_repo_root();
        let app = App::new(repo_root);
        write_trace_order_fixture(&app);

        let vm = conversation_run_vm(&app, "project-001", "task-trace", "run-001", None).unwrap();

        let node_ids = vm.session_tree.rounds[0]
            .nodes
            .iter()
            .map(|node| node.node_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(node_ids, vec!["方案", "开发", "验收"]);
    }

    #[test]
    fn conversation_run_vm_exposes_terminal_control_failure_message() {
        let repo_root = temp_repo_root();
        let app = App::new(repo_root);
        write_trace_order_fixture(&app);
        std::fs::write(
            app.paths
                .run_events_file("task-trace", "run-001")
                .as_std_path(),
            r#"{"version":"0.1","type":"workflow_control_limit_exceeded","timestamp":"2026-07-08T00:00:03Z","data":{"taskId":"task-trace","runId":"run-001","roundId":"round-001","nodeId":"验收","attemptId":"attempt-001","stage":"completed","status":"completed","summary":"max rounds exceeded for $new-round: 2 > 1","pauseReason":null,"controlFailure":{"limit":1,"message":"max rounds exceeded for $new-round: 2 > 1","proposedCount":2,"reasonKind":"max_rounds_exceeded","target":"$new-round"}}}"#,
        )
        .unwrap();

        let vm = conversation_run_vm(&app, "project-001", "task-trace", "run-001", None).unwrap();

        assert_eq!(
            vm.runtime_error_message.as_deref(),
            Some("Round 数已达上限：max rounds exceeded for $new-round: 2 > 1")
        );
    }

    #[test]
    fn conversation_run_vm_restores_manual_check_pending_from_node_state() {
        let repo_root = temp_repo_root();
        let app = App::new(repo_root);
        write_conversation_assets_fixture(&app);
        write_manual_check_pause_overrides(&app);

        let vm = conversation_run_vm(
            &app,
            "project-001",
            "task-046",
            "run-060",
            Some("round-001/测试/attempt-002"),
        )
        .unwrap();

        let leaf = vm.session_tree.rounds[0].nodes[0]
            .attempts
            .iter()
            .find(|leaf| leaf.attempt_id == "attempt-002")
            .unwrap();
        assert!(leaf.manual_check_pending);
        assert_eq!(leaf.lifecycle.continue_kind, None);
        assert_eq!(leaf.lifecycle.composer.mode, "normal");
        assert_eq!(leaf.lifecycle.composer.submit_target, "acp-prompt");
        assert!(!leaf.lifecycle.composer.lock_input);
        assert!(
            vm.active_sessions
                .iter()
                .any(|session| session.node_id == "测试" && session.manual_check_pending)
        );
    }

    #[test]
    fn switch_conversation_session_vm_uses_same_asset_contract() {
        let repo_root = temp_repo_root();
        let app = App::new(repo_root);
        write_conversation_assets_fixture(&app);

        let switched = switch_conversation_session_vm(
            &app,
            "task-046",
            "run-060",
            "round-001",
            "测试",
            "attempt-002",
            None,
            None,
        )
        .unwrap();

        assert_eq!(switched.artifacts.len(), 1);
        assert_eq!(switched.artifacts[0].name, "测试-result");
        assert_eq!(switched.attachments.len(), 1);
        assert_eq!(switched.attachments[0].name, "test-report.md");
    }

    fn temp_repo_root() -> Utf8PathBuf {
        let mut root = std::env::temp_dir();
        root.push(format!(
            "gold-band-conversation-assets-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        Utf8PathBuf::from_path_buf(root).unwrap()
    }

    fn write_sidebar_task_fixture(
        app: &App,
        task_id: &str,
        title: &str,
        run_id: &str,
        started_at: &str,
    ) {
        write_sidebar_task_fixture_with_updated_at(
            app, task_id, title, run_id, started_at, started_at,
        );
    }

    fn write_sidebar_task_fixture_with_updated_at(
        app: &App,
        task_id: &str,
        title: &str,
        run_id: &str,
        started_at: &str,
        updated_at: &str,
    ) {
        std::fs::create_dir_all(app.paths.task_dir(task_id).as_std_path()).unwrap();
        gold_band::storage::write_json(
            &app.paths.task_file(task_id),
            &json!({
                "version": gold_band::domain::VERSION,
                "id": task_id,
                "title": title,
                "description": null
            }),
        )
        .unwrap();
        gold_band::storage::write_json(
            &app.paths.run_file(task_id, run_id),
            &json!({
                "version": gold_band::domain::VERSION,
                "id": run_id,
                "task_id": task_id,
                "status": "completed",
                "outcome": "success",
                "started_at": started_at,
                "updated_at": updated_at,
                "workflow_snapshot": "workflow.snapshot.json",
                "current_round": null,
                "current_node": null,
                "current_attempt": null,
                "new_rounds_opened": 0,
                "pause_reason": null
            }),
        )
        .unwrap();
        let authoring_dir = app.paths.task_dir(task_id).join("authoring");
        std::fs::create_dir_all(authoring_dir.as_std_path()).unwrap();
        gold_band::storage::write_json(
            &authoring_dir.join("conversation.json"),
            &json!({
                "version": "1",
                "source": "conversation-ui",
                "runMode": "auto"
            }),
        )
        .unwrap();
    }

    fn write_sidebar_conversation_metadata_fixture(
        app: &App,
        task_id: &str,
        run_mode: &str,
        last_activity_at: &str,
    ) {
        let authoring_dir = app.paths.task_dir(task_id).join("authoring");
        std::fs::create_dir_all(authoring_dir.as_std_path()).unwrap();
        gold_band::storage::write_json(
            &authoring_dir.join("conversation.json"),
            &json!({
                "version": "1",
                "source": "conversation-ui",
                "runMode": run_mode,
                "workflowTemplateId": null,
                "includeInterview": null,
                "directConfig": null,
                "agentIdentity": null,
                "titleAutoGenerated": false,
                "initialAttachmentNames": null,
                "createdAt": last_activity_at,
                "lastActivityAt": last_activity_at
            }),
        )
        .unwrap();
    }

    fn write_dynamic_lifecycle_fixture(
        app: &App,
        run_status: &str,
        run_pause_reason: serde_json::Value,
        dynamic_node_status: &str,
        current_dynamic_node_ids: Vec<&str>,
    ) {
        write_dynamic_lifecycle_fixture_with_cancelled_session(
            app,
            run_status,
            run_pause_reason,
            dynamic_node_status,
            current_dynamic_node_ids,
            dynamic_node_status == "paused",
        );
    }

    fn write_dynamic_lifecycle_fixture_with_cancelled_session(
        app: &App,
        run_status: &str,
        run_pause_reason: serde_json::Value,
        dynamic_node_status: &str,
        current_dynamic_node_ids: Vec<&str>,
        cancelled_session: bool,
    ) {
        let task_id = "task-dyn";
        let run_id = "run-dyn";
        let round_id = "round-001";
        let outer_node_id = "ai-dynamic";
        let outer_attempt_id = "attempt-001";
        let dynamic_node_id = "good-morning";
        gold_band::storage::write_json(
            &app.paths.task_file(task_id),
            &json!({
                "version": gold_band::domain::VERSION,
                "id": task_id,
                "title": "Dynamic lifecycle",
                "description": null
            }),
        )
        .unwrap();
        gold_band::storage::write_json(
            &app.paths.run_file(task_id, run_id),
            &json!({
                "version": gold_band::domain::VERSION,
                "id": run_id,
                "task_id": task_id,
                "status": run_status,
                "outcome": null,
                "started_at": "2026-06-15T00:00:00Z",
                "updated_at": "2026-06-15T00:00:02Z",
                "workflow_snapshot": "workflow.snapshot.json",
                "current_round": round_id,
                "current_node": outer_node_id,
                "current_attempt": outer_attempt_id,
                "new_rounds_opened": 0,
                "pause_reason": run_pause_reason.clone()
            }),
        )
        .unwrap();
        gold_band::storage::write_json(
            &app.paths.round_file(task_id, run_id, round_id),
            &json!({
                "version": gold_band::domain::VERSION,
                "id": round_id,
                "run_id": run_id,
                "index": 1,
                "status": run_status,
                "outcome": null,
                "trigger": "initial",
                "started_at": "2026-06-15T00:00:00Z",
                "trace": []
            }),
        )
        .unwrap();
        gold_band::storage::write_json(
            &app.paths
                .node_file(task_id, run_id, round_id, outer_node_id, outer_attempt_id),
            &json!({
                "version": gold_band::domain::VERSION,
                "node_id": outer_node_id,
                "node_type": "ai-dynamic",
                "run_id": run_id,
                "round_id": round_id,
                "attempt_id": outer_attempt_id,
                "status": run_status,
                "outcome": null,
                "started_at": "2026-06-15T00:00:00Z",
                "finished_at": null,
                "manual_check_pending": false,
                "resolved_config": {}
            }),
        )
        .unwrap();
        let dynamic_run = json!({
            "version": gold_band::domain::VERSION,
            "id": "dynamic-run-001",
            "parentRunId": run_id,
            "parentRoundId": round_id,
            "parentNodeId": outer_node_id,
            "parentAttemptId": outer_attempt_id,
            "status": run_status,
            "outcome": null,
            "pauseReason": run_pause_reason.clone(),
            "startedAt": "2026-06-15T00:00:00Z",
            "updatedAt": "2026-06-15T00:00:02Z",
            "control": {},
            "allowedWorkflowSnapshots": [],
            "currentNodeIds": current_dynamic_node_ids
        });
        let dynamic_node = json!({
            "version": gold_band::domain::VERSION,
            "id": dynamic_node_id,
            "dynamicRunId": "dynamic-run-001",
            "kind": "worker",
            "title": "Good morning",
            "task": "Say good morning",
            "status": dynamic_node_status,
            "outcome": null,
            "groupId": null,
            "chainId": dynamic_node_id,
            "depth": 1,
            "dependsOn": [],
            "workspace": { "mode": "worktree" },
            "workspacePath": null,
            "provider": "claude-acp",
            "profile": null,
            "permissionMode": null,
            "model": null,
            "sessionMode": "new",
            "continueFromNodeId": null,
            "workflowId": null,
            "workflowSnapshotId": null,
            "childRunId": null,
            "startedAt": "2026-06-15T00:00:00Z",
            "finishedAt": if dynamic_node_status == "paused" { json!("2026-06-15T00:00:02Z") } else { json!(null) }
        });
        gold_band::storage::write_json(
            &app.paths.dynamic_graph_file(
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
            ),
            &json!({
                "version": gold_band::domain::VERSION,
                "run": dynamic_run,
                "nodes": [dynamic_node],
                "groups": [],
                "proposals": []
            }),
        )
        .unwrap();
        gold_band::storage::write_json(
            &app.paths
                .dynamic_run_file(task_id, run_id, round_id, outer_node_id, outer_attempt_id),
            &dynamic_run,
        )
        .unwrap();
        gold_band::storage::write_json(
            &app.paths.dynamic_node_file(
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
                dynamic_node_id,
            ),
            &dynamic_node,
        )
        .unwrap();
        if cancelled_session {
            gold_band::storage::write_json(
                &app.paths
                    .dynamic_node_attempt_dir(
                        task_id,
                        run_id,
                        round_id,
                        outer_node_id,
                        outer_attempt_id,
                        dynamic_node_id,
                        "attempt-001",
                    )
                    .join("acp.session.json"),
                &json!({
                    "status": "cancelled",
                    "stopReason": "cancelled",
                    "sessionId": "session-good-morning",
                    "messages": []
                }),
            )
            .unwrap();
        }
    }

    fn write_dynamic_node_pause_details(app: &App, pause_reason: &str, diagnostic: Option<&str>) {
        let graph_path = app.paths.dynamic_graph_file(
            "task-dyn",
            "run-dyn",
            "round-001",
            "ai-dynamic",
            "attempt-001",
        );
        let mut graph: serde_json::Value = gold_band::storage::read_json(&graph_path).unwrap();
        graph["nodes"][0]["pauseReason"] = json!(pause_reason);
        graph["nodes"][0]["runtimeError"] = diagnostic.map_or(json!(null), |diagnostic| {
            json!({
                "code": { "domain": "provider", "code": "provider.acp-error" },
                "domain": "provider",
                "recovery": "manual",
                "retryPolicy": null,
                "params": { "method": "session/set_config_option" },
                "diagnostic": diagnostic,
                "raw": null
            })
        });
        gold_band::storage::write_json(&graph_path, &graph).unwrap();
    }

    fn write_conversation_assets_fixture(app: &App) {
        let task_id = "task-046";
        let run_id = "run-060";
        let round_id = "round-001";
        let node_id = "测试";
        let attempt_id = "attempt-002";

        std::fs::create_dir_all(app.paths.task_dir(task_id).as_std_path()).unwrap();
        gold_band::storage::write_json(
            &app.paths.task_file(task_id),
            &json!({
                "version": gold_band::domain::VERSION,
                "id": task_id,
                "uuid": "task-046-fixture-uuid",
                "title": "中文节点资源回归",
                "description": null
            }),
        )
        .unwrap();
        gold_band::storage::write_json(
            &app.paths.run_file(task_id, run_id),
            &json!({
                "version": gold_band::domain::VERSION,
                "id": run_id,
                "task_id": task_id,
                "status": "completed",
                "outcome": "success",
                "started_at": "2026-06-15T00:00:00Z",
                "updated_at": "2026-06-15T00:00:02Z",
                "workflow_snapshot": "workflow.snapshot.json",
                "current_round": round_id,
                "current_node": node_id,
                "current_attempt": attempt_id,
                "new_rounds_opened": 0,
                "pause_reason": null
            }),
        )
        .unwrap();
        gold_band::storage::write_json(
            &app.paths.round_file(task_id, run_id, round_id),
            &json!({
                "version": gold_band::domain::VERSION,
                "id": round_id,
                "run_id": run_id,
                "index": 1,
                "status": "completed",
                "outcome": "success",
                "trigger": "initial",
                "started_at": "2026-06-15T00:00:00Z",
                "trace": [
                    {
                        "sequence": 1,
                        "node_id": node_id,
                        "attempt_id": attempt_id,
                        "from_node_id": null,
                        "edge_outcome": null,
                        "entered_at": "2026-06-15T00:00:00Z"
                    }
                ]
            }),
        )
        .unwrap();
        gold_band::storage::write_json(
            &app.paths
                .node_file(task_id, run_id, round_id, node_id, attempt_id),
            &json!({
                "version": gold_band::domain::VERSION,
                "node_id": node_id,
                "node_type": "worker",
                "run_id": run_id,
                "round_id": round_id,
                "attempt_id": attempt_id,
                "status": "completed",
                "outcome": "success",
                "started_at": "2026-06-15T00:00:00Z",
                "finished_at": "2026-06-15T00:00:02Z",
                "manual_check_pending": false,
                "resolved_config": {}
            }),
        )
        .unwrap();

        let artifacts_dir = app
            .paths
            .artifacts_dir(task_id, run_id, round_id, node_id, attempt_id);
        std::fs::create_dir_all(artifacts_dir.as_std_path()).unwrap();
        std::fs::write(
            artifacts_dir.join("测试-result.json").as_std_path(),
            r#"{"result":true}"#,
        )
        .unwrap();

        let attachments_dir = app
            .paths
            .attachments_dir(task_id, run_id, round_id, node_id, attempt_id);
        std::fs::create_dir_all(attachments_dir.as_std_path()).unwrap();
        std::fs::write(attachments_dir.join("test-report.md").as_std_path(), "ok").unwrap();
    }

    fn write_trace_order_fixture(app: &App) {
        let task_id = "task-trace";
        let run_id = "run-001";
        let round_id = "round-001";
        std::fs::create_dir_all(app.paths.task_dir(task_id).as_std_path()).unwrap();
        gold_band::storage::write_json(
            &app.paths.task_file(task_id),
            &json!({
                "version": gold_band::domain::VERSION,
                "id": task_id,
                "title": "Trace order",
                "description": null
            }),
        )
        .unwrap();
        gold_band::storage::write_json(
            &app.paths.run_file(task_id, run_id),
            &json!({
                "version": gold_band::domain::VERSION,
                "id": run_id,
                "task_id": task_id,
                "status": "completed",
                "outcome": "failure",
                "started_at": "2026-07-08T00:00:00Z",
                "updated_at": "2026-07-08T00:00:03Z",
                "workflow_snapshot": "workflow.snapshot.json",
                "current_round": round_id,
                "current_node": "验收",
                "current_attempt": "attempt-001",
                "new_rounds_opened": 0,
                "pause_reason": null
            }),
        )
        .unwrap();
        gold_band::storage::write_json(
            &app.paths.workflow_snapshot_file(task_id, run_id),
            &json!({
                "version": "0.1",
                "id": "workflow-trace-order",
                "entry": "方案",
                "nodes": [
                    { "type": "worker", "id": "开发", "provider": "claude-acp" },
                    { "type": "worker", "id": "验收", "provider": "claude-acp" },
                    { "type": "worker", "id": "方案", "provider": "claude-acp" }
                ],
                "edges": [
                    { "from": "方案", "to": "开发", "on": "success" },
                    { "from": "开发", "to": "验收", "on": "success" },
                    { "from": "验收", "to": "$end", "on": "success" }
                ]
            }),
        )
        .unwrap();
        gold_band::storage::write_json(
            &app.paths.round_file(task_id, run_id, round_id),
            &json!({
                "version": gold_band::domain::VERSION,
                "id": round_id,
                "run_id": run_id,
                "index": 1,
                "status": "completed",
                "outcome": "failure",
                "trigger": "initial",
                "started_at": "2026-07-08T00:00:00Z",
                "trace": [
                    { "sequence": 1, "node_id": "方案", "attempt_id": "attempt-001", "from_node_id": null, "edge_outcome": null, "entered_at": "2026-07-08T00:00:00Z" },
                    { "sequence": 2, "node_id": "开发", "attempt_id": "attempt-001", "from_node_id": "方案", "edge_outcome": "success", "entered_at": "2026-07-08T00:00:01Z" },
                    { "sequence": 3, "node_id": "验收", "attempt_id": "attempt-001", "from_node_id": "开发", "edge_outcome": "success", "entered_at": "2026-07-08T00:00:02Z" }
                ]
            }),
        )
        .unwrap();
        for (node_id, status, outcome) in [
            ("方案", "completed", "success"),
            ("开发", "completed", "success"),
            ("验收", "completed", "failure"),
        ] {
            gold_band::storage::write_json(
                &app.paths
                    .node_file(task_id, run_id, round_id, node_id, "attempt-001"),
                &json!({
                    "version": gold_band::domain::VERSION,
                    "node_id": node_id,
                    "node_type": "worker",
                    "run_id": run_id,
                    "round_id": round_id,
                    "attempt_id": "attempt-001",
                    "status": status,
                    "outcome": outcome,
                    "started_at": "2026-07-08T00:00:00Z",
                    "finished_at": "2026-07-08T00:00:01Z",
                    "manual_check_pending": false,
                    "resolved_config": {}
                }),
            )
            .unwrap();
        }
    }

    fn write_manual_check_pause_overrides(app: &App) {
        let task_id = "task-046";
        let run_id = "run-060";
        let round_id = "round-001";
        let node_id = "测试";
        let attempt_id = "attempt-002";
        gold_band::storage::write_json(
            &app.paths.run_file(task_id, run_id),
            &json!({
                "version": gold_band::domain::VERSION,
                "id": run_id,
                "task_id": task_id,
                "status": "paused",
                "outcome": null,
                "started_at": "2026-06-15T00:00:00Z",
                "updated_at": "2026-06-15T00:00:02Z",
                "workflow_snapshot": "workflow.snapshot.json",
                "current_round": round_id,
                "current_node": node_id,
                "current_attempt": attempt_id,
                "new_rounds_opened": 0,
                "pause_reason": "waiting-for-user-input"
            }),
        )
        .unwrap();
        gold_band::storage::write_json(
            &app.paths.round_file(task_id, run_id, round_id),
            &json!({
                "version": gold_band::domain::VERSION,
                "id": round_id,
                "run_id": run_id,
                "index": 1,
                "status": "paused",
                "outcome": null,
                "trigger": "initial",
                "started_at": "2026-06-15T00:00:00Z",
                "trace": [
                    {
                        "sequence": 1,
                        "node_id": node_id,
                        "attempt_id": attempt_id,
                        "from_node_id": null,
                        "edge_outcome": null,
                        "entered_at": "2026-06-15T00:00:00Z"
                    }
                ]
            }),
        )
        .unwrap();
        gold_band::storage::write_json(
            &app.paths
                .node_file(task_id, run_id, round_id, node_id, attempt_id),
            &json!({
                "version": gold_band::domain::VERSION,
                "node_id": node_id,
                "node_type": "worker",
                "run_id": run_id,
                "round_id": round_id,
                "attempt_id": attempt_id,
                "status": "paused",
                "outcome": null,
                "started_at": "2026-06-15T00:00:00Z",
                "finished_at": "2026-06-15T00:00:02Z",
                "manual_check_pending": true,
                "resolved_config": {}
            }),
        )
        .unwrap();
    }

    #[test]
    fn scheduled_task_title_uses_first_instruction_line_and_truncates() {
        assert_eq!(
            super::scheduled_task_title("  每天整理日报  \n补充说明"),
            "每天整理日报"
        );
        assert_eq!(super::scheduled_task_title(""), "未命名定时任务");
        assert_eq!(
            super::scheduled_task_title(&"x".repeat(60)).chars().count(),
            49
        );
    }

    #[test]
    fn scheduled_task_management_labels_are_localized_and_timezone_aware() {
        let definition = gold_band::scheduler::ScheduledTaskDefinition::new(
            "project-a",
            "scheduled-1",
            "direct",
            gold_band::scheduler::ScheduleSpec::repeat(
                gold_band::scheduler::RepeatPreset::Daily,
                9,
                0,
                "Asia/Shanghai",
            )
            .unwrap(),
            gold_band::scheduler::OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        assert_eq!(scheduled_task_schedule_label(&definition), "每天 09:00");
        assert_eq!(scheduled_task_timezone_label(&definition), "中国（上海）");
    }

    #[test]
    fn scheduled_task_management_aggregates_all_workspaces_and_can_filter_one() {
        let app_a = App::new(temp_repo_root());
        let app_b = App::new(temp_repo_root());
        let definition_a = gold_band::scheduler::ScheduledTaskDefinition::new(
            "workspace-a",
            "scheduled-a",
            "direct",
            gold_band::scheduler::ScheduleSpec::at(
                chrono::Utc.with_ymd_and_hms(2026, 8, 1, 1, 0, 0).unwrap(),
            ),
            gold_band::scheduler::OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        let definition_b = gold_band::scheduler::ScheduledTaskDefinition::new(
            "workspace-b",
            "scheduled-b",
            "workflow",
            gold_band::scheduler::ScheduleSpec::at(
                chrono::Utc.with_ymd_and_hms(2026, 8, 2, 1, 0, 0).unwrap(),
            ),
            gold_band::scheduler::OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        gold_band::scheduler::store::ScheduledTaskStore::new(app_a.paths.clone())
            .save(&definition_a)
            .unwrap();
        gold_band::scheduler::store::ScheduledTaskStore::new(app_b.paths.clone())
            .save(&definition_b)
            .unwrap();
        let sources = vec![
            ConversationWorkspaceSource {
                workspace: ConversationWorkspaceVm {
                    project_id: "workspace-a".to_string(),
                    workspace_path: app_a.paths.repo_root.to_string(),
                    name: "Workspace A".to_string(),
                },
                app: app_a,
            },
            ConversationWorkspaceSource {
                workspace: ConversationWorkspaceVm {
                    project_id: "workspace-b".to_string(),
                    workspace_path: app_b.paths.repo_root.to_string(),
                    name: "Workspace B".to_string(),
                },
                app: app_b,
            },
        ];

        let all = scheduled_task_vms_from_sources(&sources, None).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].workspace_name, "Workspace A");
        assert_eq!(all[1].workspace_name, "Workspace B");

        let filtered = scheduled_task_vms_from_sources(&sources, Some("workspace-b")).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "scheduled-b");
    }
}

pub fn update_task_metadata_vm(
    app: &App,
    _project_id: &str,
    task_id: &str,
    title: &str,
    description: Option<&str>,
) -> anyhow::Result<()> {
    app.update_task_metadata(task_id, title, description)
}
