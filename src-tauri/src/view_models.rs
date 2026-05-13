use std::{collections::{HashMap, VecDeque}, fs, io::{BufRead, BufReader, Read, Seek, SeekFrom}};

use anyhow::Result;
use gold_band::app::{App, LogSource, TaskSummary};
use gold_band::config::{DesktopFontPreference, DesktopLanguage, DesktopThemePreference};
use gold_band::domain::{RunOutcome, RunStatus};
use gold_band::dsl::{NodeDsl, WorkflowDsl};
use gold_band::runtime::{NodeState, RoundState, RoundTraceStep, RunState};

use crate::i18n::Translator;
use gold_band::storage::read_json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferencesVm {
    pub theme: DesktopThemePreference,
    pub language: DesktopLanguage,
    pub font: DesktopFontPreference,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBootstrapVm {
    pub repo_root: String,
    pub recent_workspaces: Vec<String>,
    pub preferences: PreferencesVm,
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
    pub workflow_error: Option<String>,
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
pub struct WorkflowControlVm {
    pub max_repair_loops: u32,
    pub max_acceptance_loops: u32,
    pub on_acceptance_failure: String,
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
    pub repair_loops_used: u32,
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
pub struct GraphNodeVm {
    pub id: String,
    pub node_id: Option<String>,
    pub sequence: Option<u32>,
    pub label: String,
    pub node_type: String,
    pub status: Option<String>,
    pub outcome: Option<String>,
    pub attempt_id: Option<String>,
    pub artifact_count: usize,
    pub attachment_count: usize,
    pub current: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdgeVm {
    pub from: String,
    pub to: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDetailVm {
    pub id: String,
    pub node_id: String,
    pub sequence: Option<u32>,
    pub label: String,
    pub node_type: String,
    pub status: String,
    pub outcome: Option<String>,
    pub attempt_id: String,
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
    pub acp_session: Option<AcpSessionVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpSessionVm {
    pub session_id: Option<String>,
    pub provider: String,
    pub adapter_id: Option<String>,
    pub adapter_display_name: Option<String>,
    pub cwd: Option<String>,
    pub status: String,
    pub restored: bool,
    pub stop_reason: Option<String>,
    pub config: Option<AcpSessionConfigVm>,
    pub events: Vec<AcpUiEventVm>,
    pub event_page: AcpEventPageVm,
    pub pending_permissions: Vec<AcpPermissionRequestVm>,
    pub available_commands: Option<Vec<serde_json::Value>>,
    pub usage: Option<serde_json::Value>,
    pub diagnostics: AcpDiagnosticsVm,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpSessionQueryInput {
    pub before_seq: Option<u64>,
    pub after_seq: Option<u64>,
    pub event_limit: Option<usize>,
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetItemVm {
    pub kind: String,
    pub name: String,
    pub title: String,
    pub tone: String,
    pub preview: String,
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
    },
    Artifact {
        node_id: String,
    },
    Attachment {
        node_id: String,
    },
    WorkerRef {
        node_id: String,
    },
    Event {
        node_id: Option<String>,
        context_node_id: Option<String>,
    },
    Log {
        node_id: Option<String>,
        context_node_id: Option<String>,
    },
}

pub fn preferences_vm(
    theme: DesktopThemePreference,
    language: DesktopLanguage,
    font: DesktopFontPreference,
) -> PreferencesVm {
    PreferencesVm {
        theme,
        language,
        font,
    }
}

pub fn bootstrap_vm(app: &App, recent_workspaces: Vec<String>) -> AppBootstrapVm {
    AppBootstrapVm {
        repo_root: app.paths.repo_root.to_string(),
        recent_workspaces,
        preferences: preferences_vm(
            app.config.desktop_theme,
            app.config.desktop_language,
            app.config.desktop_font.clone(),
        ),
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
    let nodes = app.node_list(task_id, run_id, round_id)?;
    let graph = round_graph_vm(app, task_id, &run, &round, &nodes)?;
    let selection = selection.unwrap_or(RoundSelectionInput::Round { context_node_id: None });
    let requirement = read_optional_text(&app.paths.requirement_file(task_id))?.unwrap_or_default();
    let selected_node_detail = selected_node_detail_vm(app, task_id, run_id, round_id, &run, &round, &nodes, &graph, &selection)?;

    Ok(RoundDetailVm {
        run: run_summary_vm(run.clone()),
        round: round_summary_vm(app, task_id, &run, round)?,
        graph,
        requirement,
        selected_node_detail,
    })
}

pub fn run_summary_vm(run: RunState) -> RunSummaryVm {
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
        resumable: run.status == RunStatus::Paused,
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
        workflow_error: summary.workflow_error.clone(),
        latest_run: summary.latest_run.clone().map(run_summary_vm),
        resumable_run_id: summary.resumable_run_id.clone(),
        artifact_count,
        attachment_count,
    })
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
        repair_loops_used: round.repair_loops_used,
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
        max_repair_loops: workflow.control.max_repair_loops,
        max_acceptance_loops: workflow.control.max_acceptance_loops,
        on_acceptance_failure: enum_label(&workflow.control.on_acceptance_failure),
    }
}

fn workflow_graph_vm(workflow: &WorkflowDsl) -> GraphVm {
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
                attempt_id: None,
                artifact_count: 0,
                attachment_count: 0,
                current: false,
            })
            .collect(),
        edges: workflow
            .edges
            .iter()
            .map(|edge| GraphEdgeVm {
                from: edge.from.clone(),
                to: edge.to.clone(),
                label: enum_label(&edge.on),
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
) -> Result<GraphVm> {
    let node_labels = workflow_node_labels(app, task_id, &run.id);
    if !round.trace.is_empty() {
        return round_trace_graph_vm(app, task_id, run, round, nodes, &node_labels);
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
        .map(|(index, node)| round_node_graph_vm(app, task_id, run, round, node, index as u32 + 1, &node_labels))
        .collect::<Result<Vec<_>>>()?;
    let edges = graph_nodes
        .windows(2)
        .map(|pair| GraphEdgeVm {
            from: pair[0].id.clone(),
            to: pair[1].id.clone(),
            label: "observed".to_string(),
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
) -> Result<GraphVm> {
    let mut steps = round.trace.clone();
    steps.sort_by_key(|step| step.sequence);
    let graph_nodes = steps
        .iter()
        .map(|step| {
            let node = nodes.iter().find(|node| {
                node.node_id == step.node_id && node.attempt_id == step.attempt_id
            });
            trace_step_graph_vm(app, task_id, run, round, step, node, node_labels)
        })
        .collect::<Result<Vec<_>>>()?;
    let edges = graph_nodes
        .windows(2)
        .enumerate()
        .map(|(index, pair)| GraphEdgeVm {
            from: pair[0].id.clone(),
            to: pair[1].id.clone(),
            label: steps[index + 1].edge_outcome.clone().unwrap_or_default(),
        })
        .collect();
    Ok(GraphVm {
        nodes: graph_nodes,
        edges,
    })
}

fn trace_step_graph_vm(
    app: &App,
    task_id: &str,
    run: &RunState,
    round: &RoundState,
    step: &RoundTraceStep,
    node: Option<&NodeState>,
    node_labels: &HashMap<String, String>,
) -> Result<GraphNodeVm> {
    let artifacts = app
        .artifact_list(task_id, &run.id, &round.id, &step.node_id, &step.attempt_id)?
        .len();
    let attachments = app
        .attachment_list(task_id, &run.id, &round.id, &step.node_id, &step.attempt_id)?
        .len();
    Ok(GraphNodeVm {
        id: format!("{}:{}:{}", step.sequence, step.node_id, step.attempt_id),
        node_id: Some(step.node_id.clone()),
        sequence: Some(step.sequence),
        label: node_labels.get(&step.node_id).cloned().unwrap_or_else(|| step.node_id.clone()),
        node_type: node.map(|node| enum_label(&node.node_type)).unwrap_or_else(|| "unknown".to_string()),
        status: node.map(|node| enum_label(&node.status)),
        outcome: node.and_then(|node| node.outcome.map(|outcome| enum_label(&outcome))),
        attempt_id: Some(step.attempt_id.clone()),
        artifact_count: artifacts,
        attachment_count: attachments,
        current: run.current_round.as_deref() == Some(&round.id)
            && run.current_node.as_deref() == Some(&step.node_id)
            && run.current_attempt.as_deref() == Some(&step.attempt_id),
    })
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
    Ok(GraphNodeVm {
        id: format!("{}:{}:{}", sequence, node.node_id, node.attempt_id),
        node_id: Some(node.node_id.clone()),
        sequence: Some(sequence),
        label: node_labels.get(&node.node_id).cloned().unwrap_or_else(|| node.node_id.clone()),
        node_type: enum_label(&node.node_type),
        status: Some(enum_label(&node.status)),
        outcome: node.outcome.map(|outcome| enum_label(&outcome)),
        attempt_id: Some(node.attempt_id.clone()),
        artifact_count: artifacts,
        attachment_count: attachments,
        current: run.current_round.as_deref() == Some(&round.id)
            && run.current_node.as_deref() == Some(&node.node_id),
    })
}

fn selected_node_id(selection: &RoundSelectionInput) -> Option<&str> {
    match selection {
        RoundSelectionInput::Node { node_id, .. }
        | RoundSelectionInput::Artifact { node_id, .. }
        | RoundSelectionInput::Attachment { node_id, .. }
        | RoundSelectionInput::WorkerRef { node_id, .. } => Some(node_id),
        RoundSelectionInput::Log { node_id: Some(node_id), .. } => Some(node_id),
        RoundSelectionInput::Event { node_id: Some(node_id), .. } => Some(node_id),
        RoundSelectionInput::Round { context_node_id }
        | RoundSelectionInput::Requirement { context_node_id }
        | RoundSelectionInput::Event { context_node_id, .. }
        | RoundSelectionInput::Log { context_node_id, .. } => context_node_id.as_deref(),
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
    let Some(node) = nodes.iter().find(|node| node.node_id == node_id) else {
        return Ok(None);
    };
    let graph_node = graph
        .nodes
        .iter()
        .find(|item| item.node_id.as_deref() == Some(node_id) || item.id == node_id);
    let artifacts = app
        .artifact_list(task_id, run_id, round_id, node_id, &node.attempt_id)?
        .into_iter()
        .map(|name| asset_item_vm("artifact", node_id, &node.attempt_id, name))
        .collect::<Vec<_>>();
    let attachments = app
        .attachment_list(task_id, run_id, round_id, node_id, &node.attempt_id)?
        .into_iter()
        .map(|name| asset_item_vm("attachment", node_id, &node.attempt_id, name))
        .collect::<Vec<_>>();
    let worker_ref_exists = app
        .paths
        .worker_ref_file(task_id, run_id, round_id, node_id, &node.attempt_id)
        .exists();

    Ok(Some(NodeDetailVm {
        id: graph_node.map(|node| node.id.clone()).unwrap_or_else(|| node_id.to_string()),
        node_id: node_id.to_string(),
        sequence: graph_node.and_then(|node| node.sequence),
        label: graph_node
            .map(|node| node.label.clone())
            .unwrap_or_else(|| node_id.to_string()),
        node_type: enum_label(&node.node_type),
        status: enum_label(&node.status),
        outcome: node.outcome.map(|outcome| enum_label(&outcome)),
        attempt_id: node.attempt_id.clone(),
        current: run.current_round.as_deref() == Some(&round.id)
            && run.current_node.as_deref() == Some(node_id)
            && run.current_attempt.as_deref() == Some(&node.attempt_id),
        started_at: node.started_at.clone(),
        finished_at: node.finished_at.clone(),
        artifact_count: artifacts.len(),
        attachment_count: attachments.len(),
        artifacts,
        attachments,
        has_progress_events: app.attempt_log_exists(task_id, run_id, round_id, node_id, &node.attempt_id, LogSource::ProgressEvents),
        has_raw_stream: app.attempt_log_exists(task_id, run_id, round_id, node_id, &node.attempt_id, LogSource::RawStream),
        has_worker_ref: worker_ref_exists,
        acp_session: acp_session_vm(app, task_id, run_id, round_id, node_id, &node.attempt_id, None)?,
    }))
}

pub fn acp_session_vm(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    query: Option<AcpSessionQueryInput>,
) -> Result<Option<AcpSessionVm>> {
    let session_path = app.paths.acp_session_file(task_id, run_id, round_id, node_id, attempt_id);
    let events_path = app.paths.acp_events_file(task_id, run_id, round_id, node_id, attempt_id);
    let raw_path = app.paths.acp_raw_file(task_id, run_id, round_id, node_id, attempt_id);
    let diagnostics_path = app.paths.acp_diagnostics_file(task_id, run_id, round_id, node_id, attempt_id);
    if !session_path.exists() && !events_path.exists() && !raw_path.exists() && !diagnostics_path.exists() {
        return Ok(None);
    }

    let session = if session_path.exists() {
        read_json::<serde_json::Value>(&session_path).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    let event_scan = scan_acp_events(&events_path, query)?;
    let raw_frame_count = count_jsonl_lines(&raw_path)?;
    let diagnostics = scan_acp_diagnostics(&diagnostics_path)?;
    let config = acp_session_config_vm(&session);
    let pending_permissions = event_scan
        .latest_permission_events
        .into_values()
        .filter(|event| event.status.as_deref() == Some("pending"))
        .map(|event| permission_vm_from_event(&event))
        .collect::<Vec<_>>();

    Ok(Some(AcpSessionVm {
        session_id: session.get("sessionId").and_then(|value| value.as_str()).map(str::to_string),
        provider: gold_band::domain::DEFAULT_PROVIDER.to_string(),
        adapter_id: session.get("adapterId").and_then(|value| value.as_str()).map(str::to_string),
        adapter_display_name: session.get("adapterDisplayName").and_then(|value| value.as_str()).map(str::to_string),
        cwd: session.get("cwd").and_then(|value| value.as_str()).map(str::to_string),
        status: session.get("status").and_then(|value| value.as_str()).unwrap_or("unknown").to_string(),
        restored: session.get("restored").and_then(|value| value.as_bool()).unwrap_or(false),
        stop_reason: session.get("stopReason").and_then(|value| value.as_str()).map(str::to_string),
        config,
        available_commands: event_scan.available_commands,
        usage: event_scan.usage,
        diagnostics: AcpDiagnosticsVm {
            raw_frame_count,
            event_count: event_scan.event_count,
            error_count: diagnostics.error_count,
            last_error: diagnostics.last_error,
        },
        events: event_scan.events,
        event_page: event_scan.event_page,
        pending_permissions,
    }))
}

struct AcpEventScan {
    events: Vec<AcpUiEventVm>,
    event_page: AcpEventPageVm,
    event_count: usize,
    latest_permission_events: HashMap<String, AcpUiEventVm>,
    available_commands: Option<Vec<serde_json::Value>>,
    usage: Option<serde_json::Value>,
}

struct AcpDiagnosticsScan {
    error_count: usize,
    last_error: Option<String>,
}

fn scan_acp_events(path: &camino::Utf8Path, query: Option<AcpSessionQueryInput>) -> Result<AcpEventScan> {
    const DEFAULT_EVENT_LIMIT: usize = 30;
    const MIN_EVENT_LIMIT: usize = 10;
    const MAX_EVENT_LIMIT: usize = 100;

    let query = query.unwrap_or(AcpSessionQueryInput {
        before_seq: None,
        after_seq: None,
        event_limit: None,
    });
    let limit = query
        .event_limit
        .unwrap_or(DEFAULT_EVENT_LIMIT)
        .clamp(MIN_EVENT_LIMIT, MAX_EVENT_LIMIT);
    let before_seq = query.before_seq;
    let after_seq = query.after_seq;
    let mut window = VecDeque::<AcpUiEventVm>::with_capacity(limit + 1);
    let mut after_window = Vec::<AcpUiEventVm>::with_capacity(limit);
    let mut raw_event_count = 0usize;
    let mut normalized_event_count = 0usize;
    let mut latest_permission_events = HashMap::<String, AcpUiEventVm>::new();
    let mut available_commands = None;
    let mut usage = None;
    let mut first_seq = None;
    let mut last_seq = None;
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
            if event.kind == "permissionRequest" {
                latest_permission_events.insert(event.id.clone(), event.clone());
            }
            if let Some(raw) = event.raw.as_ref() {
                if is_session_update(&event, "available_commands_update") {
                    available_commands = raw.get("availableCommands").and_then(|value| value.as_array()).cloned();
                } else if is_session_update(&event, "usage_update") {
                    usage = Some(compact_raw_value(raw.clone()));
                }
            }
            if !is_session_timeline_event(&event) {
                continue;
            }
            if merge_pending_delta(&mut pending_delta, &event) {
                continue;
            }
            flush_normalized_event(
                pending_delta.take(),
                before_seq,
                after_seq,
                limit,
                &mut window,
                &mut after_window,
                &mut normalized_event_count,
                &mut first_seq,
                &mut last_seq,
            );
            if is_delta_event(&event) {
                pending_delta = Some(compact_event_for_session(event));
            } else {
                flush_normalized_event(
                    Some(compact_event_for_session(event)),
                    before_seq,
                    after_seq,
                    limit,
                    &mut window,
                    &mut after_window,
                    &mut normalized_event_count,
                    &mut first_seq,
                    &mut last_seq,
                );
            }
        }
    }

    flush_normalized_event(
        pending_delta.take(),
        before_seq,
        after_seq,
        limit,
        &mut window,
        &mut after_window,
        &mut normalized_event_count,
        &mut first_seq,
        &mut last_seq,
    );

    let events = if after_seq.is_some() {
        after_window
    } else {
        window.into_iter().collect::<Vec<_>>()
    };
    let oldest_seq = events.first().map(|event| event.seq);
    let newest_seq = events.last().map(|event| event.seq);
    let has_older = oldest_seq.zip(first_seq).is_some_and(|(oldest, first)| oldest > first);
    let has_newer = newest_seq.zip(last_seq).is_some_and(|(newest, last)| newest < last);
    let event_page = AcpEventPageVm {
        loaded_count: events.len(),
        total: normalized_event_count,
        oldest_seq,
        newest_seq,
        has_older,
        has_newer,
    };

    Ok(AcpEventScan {
        events,
        event_page,
        event_count: raw_event_count,
        latest_permission_events,
        available_commands,
        usage,
    })
}

fn flush_normalized_event(
    event: Option<AcpUiEventVm>,
    before_seq: Option<u64>,
    after_seq: Option<u64>,
    limit: usize,
    window: &mut VecDeque<AcpUiEventVm>,
    after_window: &mut Vec<AcpUiEventVm>,
    normalized_event_count: &mut usize,
    first_seq: &mut Option<u64>,
    last_seq: &mut Option<u64>,
) {
    let Some(event) = event else {
        return;
    };
    *normalized_event_count += 1;
    first_seq.get_or_insert(event.seq);
    *last_seq = Some(event.seq);
    if let Some(cursor) = after_seq {
        if event.seq > cursor && after_window.len() < limit {
            after_window.push(event);
        }
        return;
    }
    if before_seq.is_some_and(|cursor| event.seq >= cursor) {
        return;
    }
    window.push_back(event);
    if window.len() > limit {
        window.pop_front();
    }
}

fn merge_pending_delta(pending: &mut Option<AcpUiEventVm>, event: &AcpUiEventVm) -> bool {
    let Some(previous) = pending.as_mut() else {
        return false;
    };
    if !is_delta_event(event) || previous.kind != event.kind {
        return false;
    }
    previous.content = Some(format!("{}{}", previous.content.as_deref().unwrap_or_default(), event.content.as_deref().unwrap_or_default()));
    previous.seq = event.seq;
    previous.timestamp = event.timestamp.clone();
    previous.status = event.status.clone().or_else(|| previous.status.clone());
    previous.raw = event.raw.clone().or_else(|| previous.raw.clone()).map(compact_raw_value);
    true
}

fn is_delta_event(event: &AcpUiEventVm) -> bool {
    matches!(event.kind.as_str(), "textDelta" | "userTextDelta" | "thoughtDelta")
}

fn is_session_timeline_event(event: &AcpUiEventVm) -> bool {
    if matches!(
        event.kind.as_str(),
        "availableCommands" | "usageUpdate" | "sessionInfo" | "modeUpdate" | "configUpdate" | "permissionRequest" | "rawDiagnostic"
    ) {
        return false;
    }
    let Some(raw) = event.raw.as_ref() else {
        return true;
    };
    let session_update = raw.get("sessionUpdate").and_then(|value| value.as_str());
    !matches!(
        session_update,
        Some("available_commands_update" | "usage_update" | "session_info_update" | "current_mode_update" | "config_option_update")
    )
}

fn scan_acp_diagnostics(path: &camino::Utf8Path) -> Result<AcpDiagnosticsScan> {
    let mut error_count = 0usize;
    let mut last_error = None;
    if !path.exists() {
        return Ok(AcpDiagnosticsScan { error_count, last_error });
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
            }
        }
    }
    Ok(AcpDiagnosticsScan { error_count, last_error })
}

fn compact_event_for_session(mut event: AcpUiEventVm) -> AcpUiEventVm {
    event.raw = event.raw.map(compact_raw_value);
    event.content = event.content.map(|content| truncate_string(content, 64_000));
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
    for key in ["sessionUpdate", "title", "status", "requestId", "toolCallId", "toolCall", "rawInput", "locations", "entries", "source", "synthetic", "optimistic"] {
        if let Some(item) = compacted.get(key) {
            fallback.insert(key.to_string(), item.clone());
        }
    }
    fallback.insert("truncated".to_string(), serde_json::Value::Bool(true));
    fallback.insert("summary".to_string(), serde_json::Value::String(truncate_string(serialized, MAX_RAW_CHARS)));
    serde_json::Value::Object(fallback)
}

fn truncate_json_value(value: serde_json::Value, max_string_chars: usize) -> serde_json::Value {
    match value {
        serde_json::Value::String(value) => serde_json::Value::String(truncate_string(value, max_string_chars)),
        serde_json::Value::Array(values) => serde_json::Value::Array(values.into_iter().take(100).map(|value| truncate_json_value(value, max_string_chars)).collect()),
        serde_json::Value::Object(values) => serde_json::Value::Object(values.into_iter().map(|(key, value)| (key, truncate_json_value(value, max_string_chars))).collect()),
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
    let current_model_name = current_model_id
        .as_deref()
        .and_then(|model_id| model_display_name(models.as_ref(), model_id).or_else(|| config_option_display_name(config_options.as_ref(), "model", model_id)));
    let current_mode_name = current_mode_id
        .as_deref()
        .and_then(|mode_id| mode_display_name(modes.as_ref(), mode_id).or_else(|| config_option_display_name(config_options.as_ref(), "mode", mode_id)));

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

fn config_current_value(config_options: Option<&serde_json::Value>, option_id: &str) -> Option<String> {
    find_config_option(config_options, option_id)
        .and_then(|option| option.get("currentValue"))
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

fn config_option_display_name(config_options: Option<&serde_json::Value>, option_id: &str, value: &str) -> Option<String> {
    find_config_option(config_options, option_id)
        .and_then(|option| option.get("options"))
        .and_then(|options| options.as_array())
        .and_then(|options| options.iter().find(|option| option.get("value").and_then(|item| item.as_str()) == Some(value)))
        .and_then(|option| option.get("name"))
        .and_then(|name| name.as_str())
        .map(str::to_string)
}

fn find_config_option<'a>(config_options: Option<&'a serde_json::Value>, option_id: &str) -> Option<&'a serde_json::Value> {
    config_options
        .and_then(|value| value.as_array())
        .and_then(|options| options.iter().find(|option| option.get("id").and_then(|item| item.as_str()) == Some(option_id)))
}

fn model_display_name(models: Option<&serde_json::Value>, model_id: &str) -> Option<String> {
    models
        .and_then(|value| value.get("availableModels"))
        .and_then(|value| value.as_array())
        .and_then(|models| models.iter().find(|model| model.get("modelId").and_then(|item| item.as_str()) == Some(model_id)))
        .and_then(|model| model.get("name"))
        .and_then(|name| name.as_str())
        .map(str::to_string)
}

fn mode_display_name(modes: Option<&serde_json::Value>, mode_id: &str) -> Option<String> {
    modes
        .and_then(|value| value.get("availableModes"))
        .and_then(|value| value.as_array())
        .and_then(|modes| modes.iter().find(|mode| mode.get("id").and_then(|item| item.as_str()) == Some(mode_id)))
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

fn permission_vm_from_event(event: &AcpUiEventVm) -> AcpPermissionRequestVm {
    let raw = event.raw.clone().map(compact_raw_value).unwrap_or_else(|| serde_json::json!({}));
    let options = raw
        .get("options")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .map(|option| AcpPermissionOptionVm {
            option_id: option.get("optionId").and_then(|value| value.as_str()).unwrap_or_default().to_string(),
            name: option.get("name").and_then(|value| value.as_str()).unwrap_or_default().to_string(),
            kind: option.get("kind").and_then(|value| value.as_str()).unwrap_or_default().to_string(),
        })
        .collect::<Vec<_>>();
    AcpPermissionRequestVm {
        request_id: event.id.clone(),
        title: event.title.clone().unwrap_or_else(|| "Permission required".to_string()),
        tool_call_id: event.tool_call_id.clone(),
        options,
        raw,
    }
}

fn count_jsonl_lines(path: &camino::Utf8Path) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }
    let file = fs::File::open(path.as_std_path())?;
    Ok(BufReader::new(file)
        .lines()
        .map_while(std::result::Result::ok)
        .filter(|line| !line.trim().is_empty())
        .count())
}

fn asset_item_vm(kind: &str, node_id: &str, attempt_id: &str, name: String) -> AssetItemVm {
    AssetItemVm {
        kind: kind.to_string(),
        title: name.clone(),
        preview: name.clone(),
        tone: if kind == "artifact" { "accent" } else { "neutral" }.to_string(),
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
    let page = query.page.unwrap_or(0);
    let page_size = query.page_size.unwrap_or(100).clamp(25, 200);
    let search = normalized_filter(query.search);
    let kind = normalized_filter(query.kind);
    let direction = normalized_filter(query.direction);
    let path = app.paths.acp_raw_file(task_id, run_id, round_id, node_id, attempt_id);

    let total = count_matching_raw_frames(&path, search.as_deref(), kind.as_deref(), direction.as_deref())?;
    let end = total.saturating_sub(page.saturating_mul(page_size));
    let start = total.saturating_sub((page + 1).saturating_mul(page_size));
    let items = collect_matching_raw_frames(&path, search.as_deref(), kind.as_deref(), direction.as_deref(), start, end)?;

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

fn count_matching_raw_frames(path: &camino::Utf8Path, search: Option<&str>, kind: Option<&str>, direction: Option<&str>) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }
    let file = fs::File::open(path.as_std_path())?;
    let mut total = 0usize;
    for line in BufReader::new(file).lines().map_while(std::result::Result::ok) {
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

fn raw_frame_matches(line: &str, search: Option<&str>, kind: Option<&str>, direction: Option<&str>) -> bool {
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
        if parsed.direction.as_deref().map(str::to_lowercase).as_deref() != Some(direction) {
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
        .or_else(|| frame.and_then(|frame| frame.get("method")).and_then(|item| item.as_str()))
        .map(str::to_string)
        .or_else(|| frame.and_then(|frame| frame.get("error")).map(|_| "error".to_string()))
        .or_else(|| frame.and_then(|frame| frame.get("result")).map(|_| "result".to_string()))
        .unwrap_or_else(|| "frame".to_string());
    RawFrameMeta {
        timestamp: json_string(&value, "timestamp"),
        direction: json_string(&value, "direction"),
        kind,
    }
}

fn normalized_filter(value: Option<String>) -> Option<String> {
    value.map(|item| item.trim().to_lowercase()).filter(|item| !item.is_empty())
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
    items.sort_by(|left, right| left.timestamp.cmp(&right.timestamp).then_with(|| left.id.cmp(&right.id)));
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

fn log_lines_for_query(app: &App, query: &LogQueryInput, source: &str, hot_limit: usize) -> Result<Vec<String>> {
    let scope = &query.scope;
    let path = match source {
        "progress-events" => match (&scope.round_id, &scope.node_id, &scope.attempt_id) {
            (Some(round_id), Some(node_id), Some(attempt_id)) => app.paths.progress_events_file(&scope.task_id, &scope.run_id, round_id, node_id, attempt_id),
            _ => return Ok(Vec::new()),
        },
        "raw-stream" => match (&scope.round_id, &scope.node_id, &scope.attempt_id) {
            (Some(round_id), Some(node_id), Some(attempt_id)) => app.paths.raw_stream_file(&scope.task_id, &scope.run_id, round_id, node_id, attempt_id),
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
        newline_count += buffer[..read_len].iter().filter(|&&byte| byte == b'\n').count();
        chunks.push(buffer[..read_len].to_vec());
    }
    chunks.reverse();
    let text = String::from_utf8(chunks.concat())?;
    let normalized = text.strip_suffix('\n').unwrap_or(&text);
    let lines = normalized.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(limit);
    Ok(lines[start..].iter().map(|line| (*line).to_string()).collect())
}

fn log_entry_from_line(index: usize, source: &str, line: &str) -> LogEntryVm {
    match serde_json::from_str::<serde_json::Value>(line) {
        Ok(value) => log_entry_from_json(index, source, value),
        Err(_) => LogEntryVm {
            id: format!("{source}-{index}"),
            timestamp: String::new(),
            entry_type: if source == "system" { "runtime" } else { "parse-error" }.to_string(),
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
        .or_else(|| data.and_then(|data| json_string(data, "toolName")).map(|tool| format!("tool: {tool}")))
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
        NodeDsl::Exec(node) => format!("exec from {}", node.plan_from),
        NodeDsl::Verify(node) => node.id.clone(),
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
