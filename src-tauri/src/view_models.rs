use std::fs;

use anyhow::Result;
use gold_band::app::{App, TaskSummary};
use gold_band::config::{DesktopLanguage, DesktopThemePreference};
use gold_band::domain::{NodeOutcome, RunOutcome, RunStatus};
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
    pub stream: Vec<StreamItemVm>,
    pub detail: ContentVm,
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
pub struct StreamItemVm {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub tone: String,
    pub content: String,
    pub node_id: Option<String>,
    pub attempt_id: Option<String>,
    pub name: Option<String>,
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
    Round,
    Requirement {
        node_id: Option<String>,
    },
    Node {
        node_id: String,
    },
    Artifact {
        node_id: String,
        attempt_id: String,
        name: String,
    },
    Attachment {
        node_id: String,
        attempt_id: String,
        name: String,
    },
    WorkerRef {
        node_id: String,
        attempt_id: String,
    },
    Event {
        id: String,
        node_id: Option<String>,
        attempt_id: Option<String>,
    },
    Log {
        id: String,
        node_id: Option<String>,
        attempt_id: Option<String>,
    },
}

pub fn preferences_vm(theme: DesktopThemePreference, language: DesktopLanguage) -> PreferencesVm {
    PreferencesVm { theme, language }
}

pub fn bootstrap_vm(app: &App, recent_workspaces: Vec<String>) -> AppBootstrapVm {
    AppBootstrapVm {
        repo_root: app.paths.repo_root.to_string(),
        recent_workspaces,
        preferences: preferences_vm(app.config.desktop_theme, app.config.desktop_language),
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
    let selection = selection.unwrap_or(RoundSelectionInput::Round);
    let stream = round_stream_vm(
        app, task_id, run_id, round_id, &run, &round, &nodes, &selection,
    )?;
    let detail = detail_vm(
        app, task_id, run_id, round_id, &run, &round, &nodes, &selection,
    )?;

    Ok(RoundDetailVm {
        run: run_summary_vm(run.clone()),
        round: round_summary_vm(app, task_id, &run, round)?,
        graph,
        stream,
        detail,
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
    if !round.trace.is_empty() {
        return round_trace_graph_vm(app, task_id, run, round, nodes);
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
        .map(|(index, node)| round_node_graph_vm(app, task_id, run, round, node, index as u32 + 1))
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
) -> Result<GraphVm> {
    let mut steps = round.trace.clone();
    steps.sort_by_key(|step| step.sequence);
    let graph_nodes = steps
        .iter()
        .map(|step| {
            let node = nodes.iter().find(|node| {
                node.node_id == step.node_id && node.attempt_id == step.attempt_id
            });
            trace_step_graph_vm(app, task_id, run, round, step, node)
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
        label: step.node_id.clone(),
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
        label: node.node_id.clone(),
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

fn round_stream_vm(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    run: &RunState,
    round: &RoundState,
    nodes: &[NodeState],
    selection: &RoundSelectionInput,
) -> Result<Vec<StreamItemVm>> {
    let labels = Translator::new(app.config.desktop_language);
    let mut items = Vec::new();
    let requirement = read_optional_text(&app.paths.requirement_file(task_id))?.unwrap_or_default();
    items.push(StreamItemVm {
        id: "requirement".to_string(),
        title: labels.tr("stream.requirement"),
        kind: "requirement".to_string(),
        tone: "neutral".to_string(),
        content: preview_text(&requirement, 600),
        node_id: None,
        attempt_id: None,
        name: None,
    });
    items.push(StreamItemVm {
        id: "round-summary".to_string(),
        title: labels.format("stream.round", &round.id),
        kind: "round".to_string(),
        tone: tone_for_status(round.status, round.outcome),
        content: round_summary_text(&labels, run, round),
        node_id: None,
        attempt_id: None,
        name: None,
    });

    if let Some(events) = app.run_events(task_id, run_id)? {
        items.push(StreamItemVm {
            id: "run-events".to_string(),
            title: labels.tr("stream.runEvents"),
            kind: "event".to_string(),
            tone: "muted".to_string(),
            content: tail_text(&events, 80),
            node_id: None,
            attempt_id: None,
            name: None,
        });
    }

    if let Some(node_id) = selected_node_id(selection) {
        append_node_stream_items(app, &labels, task_id, run, round_id, nodes, node_id, &mut items)?;
    }

    Ok(items)
}

fn round_summary_text(labels: &Translator, run: &RunState, round: &RoundState) -> String {
    [
        labels.format_pair("stream.field.status", &enum_label(&round.status)),
        labels.format_pair("stream.field.outcome", &round.outcome.map(|outcome| enum_label(&outcome)).unwrap_or_else(|| "-".to_string())),
        labels.format_pair("stream.field.trigger", &enum_label(&round.trigger)),
        labels.format_pair("stream.field.repairLoops", &round.repair_loops_used.to_string()),
        labels.format_pair("stream.field.currentNode", run.current_node.as_deref().unwrap_or("-")),
    ]
    .join("\n")
}

fn node_summary_text(labels: &Translator, node: &NodeState) -> String {
    [
        labels.format_pair("stream.field.status", &enum_label(&node.status)),
        labels.format_pair("stream.field.outcome", &node.outcome.map(|outcome| enum_label(&outcome)).unwrap_or_else(|| "-".to_string())),
        labels.format_pair("stream.field.attempt", &node.attempt_id),
        labels.format_pair("stream.field.startedAt", &node.started_at),
        labels.format_pair("stream.field.finishedAt", node.finished_at.as_deref().unwrap_or("-")),
    ]
    .join("\n")
}

fn selected_node_id(selection: &RoundSelectionInput) -> Option<&str> {
    match selection {
        RoundSelectionInput::Node { node_id }
        | RoundSelectionInput::Artifact { node_id, .. }
        | RoundSelectionInput::Attachment { node_id, .. }
        | RoundSelectionInput::WorkerRef { node_id, .. } => Some(node_id),
        RoundSelectionInput::Log { node_id: Some(node_id), .. } => Some(node_id),
        RoundSelectionInput::Event { node_id: Some(node_id), .. } => Some(node_id),
        RoundSelectionInput::Requirement { node_id: Some(node_id) } => Some(node_id),
        RoundSelectionInput::Round | RoundSelectionInput::Requirement { .. } | RoundSelectionInput::Event { .. } | RoundSelectionInput::Log { .. } => None,
    }
}

fn append_node_stream_items(
    app: &App,
    labels: &Translator,
    task_id: &str,
    run: &RunState,
    round_id: &str,
    nodes: &[NodeState],
    node_id: &str,
    items: &mut Vec<StreamItemVm>,
) -> Result<()> {
    if let Some(node) = nodes.iter().find(|node| node.node_id == node_id) {
        items.push(StreamItemVm {
            id: format!("node-{node_id}"),
            title: labels.format("stream.node", node_id),
            kind: "node".to_string(),
            tone: tone_for_node(node.status, node.outcome),
            content: node_summary_text(&labels, node),
            node_id: Some(node_id.to_string()),
            attempt_id: Some(node.attempt_id.clone()),
            name: None,
        });
        for name in app.artifact_list(task_id, &run.id, round_id, node_id, &node.attempt_id)? {
            items.push(StreamItemVm {
                id: format!("artifact-{node_id}-{}-{name}", node.attempt_id),
                title: labels.format("stream.artifact", &name),
                kind: "artifact".to_string(),
                tone: "accent".to_string(),
                content: name.clone(),
                node_id: Some(node_id.to_string()),
                attempt_id: Some(node.attempt_id.clone()),
                name: Some(name),
            });
        }
        for name in app.attachment_list(task_id, &run.id, round_id, node_id, &node.attempt_id)? {
            items.push(StreamItemVm {
                id: format!("attachment-{node_id}-{}-{name}", node.attempt_id),
                title: labels.format("stream.attachment", &name),
                kind: "attachment".to_string(),
                tone: "neutral".to_string(),
                content: name.clone(),
                node_id: Some(node_id.to_string()),
                attempt_id: Some(node.attempt_id.clone()),
                name: Some(name),
            });
        }
        if let Some(progress) =
            app.attempt_progress_events(task_id, &run.id, round_id, node_id, &node.attempt_id)?
        {
            items.push(StreamItemVm {
                id: format!("log-{node_id}-{}", node.attempt_id),
                title: labels.tr("stream.progressEvents"),
                kind: "log".to_string(),
                tone: "muted".to_string(),
                content: tail_text(&progress, 80),
                node_id: Some(node_id.to_string()),
                attempt_id: Some(node.attempt_id.clone()),
                name: None,
            });
        }
    }
    Ok(())
}

fn detail_vm(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    run: &RunState,
    round: &RoundState,
    nodes: &[NodeState],
    selection: &RoundSelectionInput,
) -> Result<ContentVm> {
    let labels = Translator::new(app.config.desktop_language);
    match selection {
        RoundSelectionInput::Round => Ok(ContentVm {
            title: labels.format("detail.round", &round.id),
            kind: "round".to_string(),
            content: serde_json::to_string_pretty(round)?,
            metadata: serde_json::json!({ "runId": run.id, "source": "canonical-state" }),
        }),
        RoundSelectionInput::Requirement { node_id } => Ok(ContentVm {
            title: labels.tr("detail.requirement"),
            kind: "requirement".to_string(),
            content: read_optional_text(&app.paths.requirement_file(task_id))?
                .unwrap_or_else(|| labels.tr("fallback.missingRequirement")),
            metadata: serde_json::json!({ "source": "task-authoring", "nodeId": node_id }),
        }),
        RoundSelectionInput::Node { node_id } => {
            let node = nodes
                .iter()
                .find(|node| node.node_id == *node_id)
                .ok_or_else(|| anyhow::anyhow!("node not found: {node_id}"))?;
            Ok(ContentVm {
                title: labels.format("detail.node", node_id),
                kind: "node".to_string(),
                content: serde_json::to_string_pretty(node)?,
                metadata: serde_json::json!({ "attemptId": node.attempt_id, "source": "canonical-state" }),
            })
        }
        RoundSelectionInput::Artifact {
            node_id,
            attempt_id,
            name,
        } => Ok(ContentVm {
            title: labels.format("detail.artifact", name),
            kind: "artifact".to_string(),
            content: app.artifact_show(task_id, run_id, round_id, node_id, attempt_id, name)?,
            metadata: serde_json::json!({ "nodeId": node_id, "attemptId": attempt_id }),
        }),
        RoundSelectionInput::Attachment {
            node_id,
            attempt_id,
            name,
        } => Ok(ContentVm {
            title: labels.format("detail.attachment", name),
            kind: "attachment".to_string(),
            content: app.attachment_show(task_id, run_id, round_id, node_id, attempt_id, name)?,
            metadata: serde_json::json!({ "nodeId": node_id, "attemptId": attempt_id }),
        }),
        RoundSelectionInput::WorkerRef {
            node_id,
            attempt_id,
        } => Ok(ContentVm {
            title: labels.format("detail.workerRef", node_id),
            kind: "worker-ref".to_string(),
            content: app
                .worker_ref_show(task_id, run_id, round_id, node_id, attempt_id)?
                .unwrap_or_else(|| labels.tr("fallback.missingWorkerRef")),
            metadata: serde_json::json!({ "nodeId": node_id, "attemptId": attempt_id }),
        }),
        RoundSelectionInput::Event { id, node_id, attempt_id } => Ok(ContentVm {
            title: labels.tr("detail.runEvents"),
            kind: "event".to_string(),
            content: app
                .run_events(task_id, run_id)?
                .unwrap_or_else(|| labels.tr("fallback.missingEvents")),
            metadata: serde_json::json!({ "id": id, "nodeId": node_id, "attemptId": attempt_id }),
        }),
        RoundSelectionInput::Log { id, node_id, attempt_id } => {
            let content = if let (Some(node_id), Some(attempt_id)) = (node_id.as_deref(), attempt_id.as_deref()) {
                app.attempt_progress_events(task_id, run_id, round_id, node_id, attempt_id)?
                    .unwrap_or_else(|| labels.tr("fallback.missingEvents"))
            } else {
                app.runtime_log_tail_show(200)?
                    .unwrap_or_else(|| labels.tr("fallback.missingRuntimeLog"))
            };
            Ok(ContentVm {
                title: labels.tr("detail.runtimeLog"),
                kind: "log".to_string(),
                content,
                metadata: serde_json::json!({ "id": id, "nodeId": node_id, "attemptId": attempt_id }),
            })
        },
    }
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

fn tone_for_status(status: RunStatus, outcome: Option<RunOutcome>) -> String {
    match (status, outcome) {
        (RunStatus::Running, _) => "accent".to_string(),
        (RunStatus::Paused, _) => "warning".to_string(),
        (RunStatus::Completed, Some(RunOutcome::Success)) => "success".to_string(),
        (RunStatus::Completed, Some(RunOutcome::Failure | RunOutcome::Killed)) => {
            "danger".to_string()
        }
        _ => "neutral".to_string(),
    }
}

fn tone_for_node(status: RunStatus, outcome: Option<NodeOutcome>) -> String {
    match (status, outcome) {
        (RunStatus::Running, _) => "accent".to_string(),
        (RunStatus::Paused, _) => "warning".to_string(),
        (RunStatus::Completed, Some(NodeOutcome::Success)) => "success".to_string(),
        (
            RunStatus::Completed,
            Some(NodeOutcome::Failure | NodeOutcome::Invalid | NodeOutcome::Killed),
        ) => "danger".to_string(),
        _ => "neutral".to_string(),
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

fn tail_text(text: &str, limit: usize) -> String {
    let normalized = text.strip_suffix('\n').unwrap_or(text);
    let lines = normalized.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(limit);
    lines[start..].join("\n")
}

fn newest_first<T>(mut items: Vec<T>) -> Vec<T> {
    items.reverse();
    items
}
