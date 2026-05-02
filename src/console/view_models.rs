use std::collections::{BTreeMap, HashMap, VecDeque};

use anyhow::{anyhow, Result};
use crate::app::{App, LogSource, TaskSummary};
use crate::dsl::{validate_workflow, EdgeOutcome, WorkflowDsl};
use crate::inspect::render_console_banner;
use crate::runtime::RunState;

use super::state::{
    ConsoleState, DetailLevel, DetailSelection, FocusPane, LayoutMode, Screen, WelcomeAction, WorkspaceSelection,
    WorkspaceState, MIN_VIEWPORT_HEIGHT, MIN_VIEWPORT_WIDTH,
};

#[derive(Clone, Copy)]
pub enum BodyLineKind {
    Normal,
    Muted,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Copy)]
pub enum BodySpanRole {
    Normal,
    Muted,
    Accent,
    Success,
    Warning,
    Error,
    PickerBorder,
    PickerTitle,
    PickerSelection,
    PickerMeta,
    PickerReasonLabel,
}

#[derive(Clone)]
pub struct BodySpan {
    pub text: String,
    pub role: BodySpanRole,
}

pub struct ConsoleViewModel {
    pub header: String,
    pub body_title: String,
    pub body_lines: Vec<String>,
    pub body_line_kinds: Vec<BodyLineKind>,
    pub body_rich_lines: Option<Vec<Vec<BodySpan>>>,
    pub body_scroll: u16,
    pub detail_title: String,
    pub detail_body: String,
    pub detail_scroll: u16,
    pub show_detail: bool,
    pub show_input: bool,
    pub input_title: String,
    pub input: String,
    pub input_hint: String,
    pub footer: String,
    pub overlay_title: Option<String>,
    pub overlay_body: String,
    pub overlay_scroll: u16,
    pub show_overlay: bool,
    pub compact_detail_only: bool,
}

pub fn build_view_model(app: &App, state: &ConsoleState) -> Result<ConsoleViewModel> {
    if state.layout_mode == LayoutMode::TooSmall {
        return Ok(build_too_small_view_model(state));
    }
    match state.screen {
        Screen::Welcome => build_welcome_view_model(state),
        Screen::TaskPicker => build_task_picker_view_model(state),
        Screen::Workspace => build_workspace_view_model(app, state),
    }
}

pub fn build_workspace_state(app: &App, task_summary: TaskSummary) -> Result<WorkspaceState> {
    let workflow_path = app.paths.workflow_file(&task_summary.task.id);
    let workflow = if workflow_path.exists() {
        Some(crate::storage::read_json::<WorkflowDsl>(&workflow_path)?)
    } else {
        None
    };
    let dag_positions = workflow
        .as_ref()
        .and_then(|workflow| validate_workflow(workflow.clone()).ok())
        .map(|validated| dag_columns(&validated.raw))
        .unwrap_or_default();
    let active_run_id = app.find_active_or_resumable_run_id(&task_summary.task.id)?;
    let live_selection = active_run_id
        .as_ref()
        .and_then(|run_id| app.current_attempt_selection(&task_summary.task.id, run_id).ok().flatten());
    let selection = live_selection
        .as_ref()
        .map(|(_, node_id, _)| WorkspaceSelection::Node { node_id: node_id.clone() })
        .or_else(|| {
            dag_positions
                .first()
                .and_then(|column| column.first())
                .map(|node_id| WorkspaceSelection::Node { node_id: node_id.clone() })
        })
        .unwrap_or(WorkspaceSelection::TaskOverview);
    let selected_round_id = live_selection
        .as_ref()
        .map(|(round_id, _, _)| round_id.clone())
        .or_else(|| {
            active_run_id
                .as_ref()
                .and_then(|run_id| app.run_status(&task_summary.task.id, run_id).ok())
                .and_then(|run| run.current_round)
        });
    let run_progress_summary = active_run_id
        .as_ref()
        .and_then(|run_id| app.run_progress(&task_summary.task.id, run_id).ok().flatten())
        .map(|value| summarize_run_progress(&value));
    let run_events_tail = active_run_id
        .as_ref()
        .and_then(|run_id| app.run_events(&task_summary.task.id, run_id).ok().flatten())
        .map(|events| tail_lines(&events, 6));
    let mut workspace = WorkspaceState {
        task_id: task_summary.task.id.clone(),
        task_summary,
        active_run_id,
        selected_round_id,
        run_progress_summary,
        run_events_tail,
        selection,
        dag_positions,
        dag_column: 0,
        dag_row: 0,
        detail_level: DetailLevel::NodeHome,
        detail_items: Vec::new(),
        detail_index: 0,
        detail_scroll: 0,
        log_source: LogSource::RawStream,
        log_scroll: 0,
    };
    if let Some((round_id, node_id, attempt_id)) = live_selection {
        workspace.selection = WorkspaceSelection::Node { node_id: node_id.clone() };
        workspace.selected_round_id = Some(round_id.clone());
        if let Some((column, row)) = locate_dag_node(&workspace.dag_positions, &node_id) {
            workspace.dag_column = column;
            workspace.dag_row = row;
        }
        if let Some(run_id) = workspace.active_run_id.clone() {
            workspace.log_source = default_log_source(app, &workspace.task_id, &run_id, &round_id, &node_id, &attempt_id);
            workspace.detail_level = DetailLevel::AttemptItems {
                attempt_id,
                follow_live: true,
            };
        }
    }
    sync_workspace_detail(app, &mut workspace)?;
    Ok(workspace)
}

pub fn sync_workspace_detail(app: &App, workspace: &mut WorkspaceState) -> Result<()> {
    workspace.detail_items = match (&workspace.selection, &workspace.detail_level) {
        (WorkspaceSelection::Node { .. }, DetailLevel::NodeHome) => build_node_home_items(app, workspace)?,
        (WorkspaceSelection::Node { .. }, DetailLevel::AttemptItems { attempt_id, .. }) => {
            let effective_attempt_id = effective_attempt_id(app, workspace, attempt_id)?;
            build_attempt_items(app, workspace, &effective_attempt_id)?
        }
        _ => Vec::new(),
    };
    if workspace.detail_index >= workspace.detail_items.len() {
        workspace.detail_index = workspace.detail_items.len().saturating_sub(1);
    }
    Ok(())
}

fn command_hint(state: &ConsoleState, fallback: &str) -> String {
    if state.command_suggestions.is_empty() {
        fallback.to_string()
    } else {
        state.command_suggestions.iter().take(6).cloned().collect::<Vec<_>>().join("   ")
    }
}

fn build_too_small_view_model(state: &ConsoleState) -> ConsoleViewModel {
    ConsoleViewModel {
        header: "Gold Band Console".to_string(),
        body_title: "Viewport".to_string(),
        body_lines: vec![
            "Terminal too small for console workspace.".to_string(),
            format!("Current size: {}x{}", state.viewport.width, state.viewport.height),
            format!("Minimum supported size: {}x{}", MIN_VIEWPORT_WIDTH, MIN_VIEWPORT_HEIGHT),
            "Resize the terminal, then continue.".to_string(),
        ],
        body_line_kinds: vec![BodyLineKind::Warning, BodyLineKind::Muted, BodyLineKind::Muted, BodyLineKind::Normal],
        body_rich_lines: None,
        body_scroll: 0,
        detail_title: String::new(),
        detail_body: String::new(),
        detail_scroll: 0,
        show_detail: false,
        show_input: false,
        input_title: String::new(),
        input: String::new(),
        input_hint: String::new(),
        footer: "Resize terminal   ? help   Esc quit".to_string(),
        overlay_title: state.overlay.as_ref().map(|overlay| overlay.kind.title().to_string()),
        overlay_body: state.overlay.as_ref().map(|overlay| overlay.body.clone()).unwrap_or_default(),
        overlay_scroll: state.overlay.as_ref().map(|overlay| overlay.scroll).unwrap_or(0),
        show_overlay: state.overlay.is_some(),
        compact_detail_only: false,
    }
}

fn build_welcome_view_model(state: &ConsoleState) -> Result<ConsoleViewModel> {
    let mut body_lines = vec![
        "".to_string(),
        "  ─────────────────────────────────────────────".to_string(),
    ];
    body_lines.extend(render_console_banner().lines().map(|line| line.to_string()));
    body_lines.push("  ─────────────────────────────────────────────".to_string());
    body_lines.push(String::new());
    body_lines.push("  workflow-first runtime console".to_string());
    body_lines.push(String::new());
    body_lines.push(format!("  {}", welcome_line(state, WelcomeAction::AddTask, "新增 task（本期占位）")));
    body_lines.push(format!("  {}", welcome_line(state, WelcomeAction::SelectTask, "选择现有 task")));
    let body_line_kinds = body_lines
        .iter()
        .map(|line| {
            if line.contains("选择现有 task") || line.contains("新增 task") {
                BodyLineKind::Normal
            } else {
                BodyLineKind::Muted
            }
        })
        .collect::<Vec<_>>();
    Ok(ConsoleViewModel {
        header: "Gold Band Console".to_string(),
        body_title: pane_title("Welcome", state.focus == FocusPane::Welcome),
        body_lines,
        body_line_kinds,
        body_rich_lines: None,
        body_scroll: 0,
        detail_title: String::new(),
        detail_body: String::new(),
        detail_scroll: 0,
        show_detail: false,
        show_input: false,
        input_title: String::new(),
        input: String::new(),
        input_hint: String::new(),
        footer: "↑↓ move   Enter select   ? help   Esc quit".to_string(),
        overlay_title: state.overlay.as_ref().map(|overlay| overlay.kind.title().to_string()),
        overlay_body: state.overlay.as_ref().map(|overlay| overlay.body.clone()).unwrap_or_default(),
        overlay_scroll: state.overlay.as_ref().map(|overlay| overlay.scroll).unwrap_or(0),
        show_overlay: state.overlay.is_some(),
        compact_detail_only: false,
    })
}

fn build_task_picker_view_model(state: &ConsoleState) -> Result<ConsoleViewModel> {
    let show_overlay = state.overlay.is_some();
    let overlay_title = state.overlay.as_ref().map(|overlay| overlay.kind.title().to_string());
    let overlay_body = state.overlay.as_ref().map(|overlay| overlay.body.clone()).unwrap_or_default();
    let overlay_scroll = state.overlay.as_ref().map(|overlay| overlay.scroll).unwrap_or(0);
    let (body_lines, body_line_kinds, body_rich_lines) = if let Some(message) = state.message.as_ref() {
        let lines = message.lines().map(|line| line.to_string()).collect::<Vec<_>>();
        let kinds = vec![BodyLineKind::Warning; lines.len()];
        (lines, kinds, None)
    } else if state.task_list.is_empty() {
        (
            vec!["No task-* directories found under .gold-band/tasks".to_string()],
            vec![BodyLineKind::Muted],
            None,
        )
    } else {
        let mut lines = Vec::new();
        let mut kinds = Vec::new();
        let mut rich_lines = Vec::new();
        let width = state.viewport.width.saturating_sub(10) as usize;
        let width = width.clamp(32, 88);
        for (index, summary) in state.task_list.iter().enumerate() {
            let selected = index == state.task_index;
            let marker = if selected { '▶' } else { '·' };
            let desc = summary.task.description.as_deref().unwrap_or("");
            let run_hint = summary
                .suggested_run_id
                .as_ref()
                .map(|run_id| format!("run {run_id}"))
                .unwrap_or_else(|| "run none".to_string());
            let shell = if selected { ("┏", "┓", "┗", "┛", "━") } else { ("╭", "╮", "╰", "╯", "─") };
            let border_top = format!("{}{}", shell.0, shell.4.to_string().repeat(width));
            lines.push(border_top.clone());
            kinds.push(BodyLineKind::Normal);
            rich_lines.push(vec![BodySpan { text: border_top, role: BodySpanRole::PickerBorder }]);

            let title_line = format!("│ {} {}{}", marker, summary.task.id, if selected { "   [selected]" } else { "" });
            lines.push(title_line);
            kinds.push(BodyLineKind::Normal);
            let mut title_spans = vec![
                BodySpan { text: "│ ".to_string(), role: BodySpanRole::PickerBorder },
                BodySpan { text: format!("{} ", marker), role: BodySpanRole::PickerSelection },
                BodySpan { text: summary.task.id.clone(), role: BodySpanRole::PickerTitle },
            ];
            if selected {
                title_spans.push(BodySpan { text: "   [selected]".to_string(), role: BodySpanRole::Accent });
            }
            rich_lines.push(title_spans);

            let desc_line = format!("│   {}", desc);
            lines.push(desc_line.clone());
            kinds.push(BodyLineKind::Muted);
            rich_lines.push(vec![
                BodySpan { text: "│   ".to_string(), role: BodySpanRole::PickerBorder },
                BodySpan { text: desc_line.trim_start_matches("│   ").to_string(), role: BodySpanRole::Muted },
            ]);

            if summary.workflow_valid {
                let workflow_line = format!("│   workflow valid   {}", run_hint);
                lines.push(workflow_line);
                kinds.push(BodyLineKind::Success);
                rich_lines.push(vec![
                    BodySpan { text: "│   ".to_string(), role: BodySpanRole::PickerBorder },
                    BodySpan { text: "workflow valid".to_string(), role: BodySpanRole::Success },
                    BodySpan { text: "   ".to_string(), role: BodySpanRole::Normal },
                    BodySpan { text: run_hint, role: BodySpanRole::PickerMeta },
                ]);
            } else if summary.workflow_exists {
                lines.push("│   workflow invalid".to_string());
                kinds.push(BodyLineKind::Error);
                rich_lines.push(vec![
                    BodySpan { text: "│   ".to_string(), role: BodySpanRole::PickerBorder },
                    BodySpan { text: "workflow invalid".to_string(), role: BodySpanRole::Error },
                ]);
                let reason = summary.workflow_error.as_deref().unwrap_or("unknown").to_string();
                lines.push(format!("│   reason: {}", reason));
                kinds.push(BodyLineKind::Warning);
                rich_lines.push(vec![
                    BodySpan { text: "│   ".to_string(), role: BodySpanRole::PickerBorder },
                    BodySpan { text: "reason: ".to_string(), role: BodySpanRole::PickerReasonLabel },
                    BodySpan { text: reason, role: BodySpanRole::Warning },
                ]);
                lines.push(format!("│   {}", run_hint));
                kinds.push(BodyLineKind::Muted);
                rich_lines.push(vec![
                    BodySpan { text: "│   ".to_string(), role: BodySpanRole::PickerBorder },
                    BodySpan { text: run_hint, role: BodySpanRole::PickerMeta },
                ]);
            } else {
                lines.push("│   workflow missing".to_string());
                kinds.push(BodyLineKind::Warning);
                rich_lines.push(vec![
                    BodySpan { text: "│   ".to_string(), role: BodySpanRole::PickerBorder },
                    BodySpan { text: "workflow missing".to_string(), role: BodySpanRole::Warning },
                ]);
                lines.push("│   reason: missing authoring/workflow.json".to_string());
                kinds.push(BodyLineKind::Warning);
                rich_lines.push(vec![
                    BodySpan { text: "│   ".to_string(), role: BodySpanRole::PickerBorder },
                    BodySpan { text: "reason: ".to_string(), role: BodySpanRole::PickerReasonLabel },
                    BodySpan { text: "missing authoring/workflow.json".to_string(), role: BodySpanRole::Warning },
                ]);
                lines.push(format!("│   {}", run_hint));
                kinds.push(BodyLineKind::Muted);
                rich_lines.push(vec![
                    BodySpan { text: "│   ".to_string(), role: BodySpanRole::PickerBorder },
                    BodySpan { text: run_hint, role: BodySpanRole::PickerMeta },
                ]);
            }

            let border_bottom = format!("{}{}", shell.2, shell.4.to_string().repeat(width));
            lines.push(border_bottom.clone());
            kinds.push(BodyLineKind::Normal);
            rich_lines.push(vec![BodySpan { text: border_bottom, role: BodySpanRole::PickerBorder }]);
            if index + 1 < state.task_list.len() {
                lines.push(String::new());
                kinds.push(BodyLineKind::Normal);
                rich_lines.push(vec![BodySpan { text: String::new(), role: BodySpanRole::Normal }]);
            }
        }
        (lines, kinds, Some(rich_lines))
    };
    Ok(ConsoleViewModel {
        header: if show_overlay {
            "Gold Band Console • overlay mode".to_string()
        } else {
            format!("Gold Band Console • tasks discovered: {}", state.task_list.len())
        },
        body_title: pane_title("Task Picker", state.focus == FocusPane::TaskPicker && !show_overlay),
        body_lines,
        body_line_kinds,
        body_rich_lines,
        body_scroll: 0,
        detail_title: String::new(),
        detail_body: String::new(),
        detail_scroll: 0,
        show_detail: false,
        show_input: !show_overlay,
        input_title: pane_title("Command Bar", state.focus == FocusPane::Input),
        input: state.input.clone(),
        input_hint: command_hint(state, "/task   /log   /config   /theme   /help"),
        footer: if show_overlay {
            "↑↓ scroll   Esc close".to_string()
        } else {
            "↑↓ move   Enter open   s start   / command   ? help   Esc back".to_string()
        },
        overlay_title,
        overlay_body,
        overlay_scroll,
        show_overlay,
        compact_detail_only: false,
    })
}

fn build_workspace_view_model(app: &App, state: &ConsoleState) -> Result<ConsoleViewModel> {
    let workspace = state.workspace.as_ref().ok_or_else(|| anyhow!("workspace missing"))?;
    let detail_body = render_detail_panel(app, workspace, &state.command_suggestions)?;
    let compact_detail_only = state.layout_mode == LayoutMode::Compact && state.focus == FocusPane::Detail;
    let show_overlay = state.overlay.is_some();
    let overlay_title = state.overlay.as_ref().map(|overlay| overlay.kind.title().to_string());
    let overlay_body = state.overlay.as_ref().map(|overlay| overlay.body.clone()).unwrap_or_default();
    let overlay_scroll = state.overlay.as_ref().map(|overlay| overlay.scroll).unwrap_or(0);
    let body_lines = if compact_detail_only {
        detail_body.lines().map(|line| line.to_string()).collect()
    } else {
        render_workspace_dag(app, workspace)?
    };
    let body_line_kinds = body_lines.iter().map(|_| BodyLineKind::Normal).collect();
    Ok(ConsoleViewModel {
        header: if show_overlay {
            "Gold Band Console • overlay mode".to_string()
        } else {
            let mut header = render_workspace_header(workspace);
            if let Some(background_task) = state.background_task.as_ref() {
                if background_task.task_id == workspace.task_id {
                    if let Some(error) = background_task.error.as_ref() {
                        header.push_str(&format!("\n[background: {} failed] {}", background_task.kind, error));
                    } else {
                        header.push_str(&format!("\n[background: {} pending]", background_task.kind));
                    }
                }
            }
            header
        },
        body_title: if compact_detail_only {
            pane_title("Details", state.focus == FocusPane::Detail)
        } else {
            pane_title("Workflow", state.focus == FocusPane::Dag)
        },
        body_lines,
        body_line_kinds,
        body_rich_lines: None,
        body_scroll: if compact_detail_only { workspace.log_scroll } else { 0 },
        detail_title: if state.layout_mode == LayoutMode::Full { pane_title("Details", state.focus == FocusPane::Detail) } else { String::new() },
        detail_body: if state.layout_mode == LayoutMode::Full { detail_body } else { String::new() },
        detail_scroll: if state.layout_mode == LayoutMode::Full { workspace.log_scroll } else { 0 },
        show_detail: state.layout_mode == LayoutMode::Full,
        show_input: !show_overlay,
        input_title: pane_title("Command Bar", state.focus == FocusPane::Input),
        input: state.input.clone(),
        input_hint: command_hint(state, "/task   /log   /config   /theme   /continue   /help"),
        footer: if show_overlay {
            "↑↓ scroll   Esc close".to_string()
        } else if state.layout_mode == LayoutMode::Compact {
            "←→↑↓ move   Enter open   l toggle log   Tab swap pane   / command   ? help   Esc back".to_string()
        } else {
            "←→↑↓ move   Enter open   l toggle log   Tab focus   / command   ? help   Esc back".to_string()
        },
        overlay_title,
        overlay_body,
        overlay_scroll,
        show_overlay,
        compact_detail_only,
    })
}

pub fn render_overlay_body(title: &str, body: &str, width: u16) -> Vec<String> {
    let width = width.saturating_sub(10) as usize;
    let width = width.clamp(24, 96);
    let rule = "─".repeat(width);
    let mut lines = vec![format!("  {}", title), format!("  {}", rule), String::new()];
    lines.extend(body.lines().map(|line| line.to_string()));
    lines
}

fn render_workspace_header(workspace: &WorkspaceState) -> String {
    let task = &workspace.task_summary.task;
    let desc = task.description.as_deref().unwrap_or("");
    let workflow = if workspace.task_summary.workflow_valid {
        "workflow=ok".to_string()
    } else if workspace.task_summary.workflow_exists {
        format!("workflow=invalid({})", workspace.task_summary.workflow_error.as_deref().unwrap_or("unknown"))
    } else {
        "workflow=missing".to_string()
    };
    let run = workspace.active_run_id.as_deref().unwrap_or("none");
    let restore = workspace.task_summary.resumable_run_id.as_deref().unwrap_or("none");
    let progress = workspace.run_progress_summary.as_deref().unwrap_or("progress unavailable");
    format!(
        "Task: {}\n{}\n[run: {}] [resumable: {}] [{}]\n{}",
        task.id, desc, run, restore, workflow, progress
    )
}

fn render_workspace_dag(app: &App, workspace: &WorkspaceState) -> Result<Vec<String>> {
    if workspace.dag_positions.is_empty() {
        return Ok(vec!["No valid workflow graph available".to_string()]);
    }

    let workflow: WorkflowDsl = crate::storage::read_json(&app.paths.workflow_file(&workspace.task_id))?;
    let active_run = workspace
        .active_run_id
        .as_ref()
        .and_then(|run_id| app.run_status(&workspace.task_id, run_id).ok());

    let max_rows = workspace.dag_positions.iter().map(|column| column.len()).max().unwrap_or(0);
    let cell_width = 20usize;
    let column_gap = 6usize;
    let row_height = 4usize;
    let total_rows = max_rows.saturating_mul(row_height).max(row_height);
    let total_cols = workspace.dag_positions.len().saturating_mul(cell_width + column_gap).saturating_sub(column_gap).max(cell_width);
    let mut canvas = vec![vec![' '; total_cols]; total_rows];
    let mut anchors = HashMap::<String, (usize, usize, usize, usize)>::new();

    for (column_index, column) in workspace.dag_positions.iter().enumerate() {
        for (row_index, node_id) in column.iter().enumerate() {
            let selected = matches!(&workspace.selection, WorkspaceSelection::Node { node_id: selected } if selected == node_id);
            let x = column_index * (cell_width + column_gap);
            let y = row_index * row_height;
            let status = node_status_label(active_run.as_ref(), node_id);
            let title = if selected {
                format!("◆ {}", node_id)
            } else {
                format!("· {}", node_id)
            };
            let status_line = if selected {
                format!("active:{}", status)
            } else {
                status.to_string()
            };
            draw_box(&mut canvas, x, y, cell_width, 3, &title, &status_line, selected);
            anchors.insert(node_id.clone(), (x, y, x + cell_width - 1, y + 1));
        }
    }

    for edge in &workflow.edges {
        if edge.to == crate::dsl::END_NODE {
            continue;
        }
        let Some((from_left, _from_top, from_right, from_mid_y)) = anchors.get(&edge.from).copied() else {
            continue;
        };
        let Some((to_left, _to_top, _to_right, to_mid_y)) = anchors.get(&edge.to).copied() else {
            continue;
        };

        let start_x = from_right + 1;
        let end_x = to_left.saturating_sub(2);
        let bridge_y = from_mid_y;
        if bridge_y < canvas.len() {
            for x in start_x..=end_x.min(total_cols.saturating_sub(1)) {
                canvas[bridge_y][x] = '─';
            }
        }

        let target_y = to_mid_y;
        if bridge_y != target_y {
            let min_y = bridge_y.min(target_y);
            let max_y = bridge_y.max(target_y);
            let vertical_x = end_x.min(total_cols.saturating_sub(1));
            for y in min_y..=max_y.min(total_rows.saturating_sub(1)) {
                canvas[y][vertical_x] = '│';
            }
            canvas[bridge_y][vertical_x] = if target_y > bridge_y { '╮' } else { '╯' };
            canvas[target_y][vertical_x] = if target_y > bridge_y { '╰' } else { '╭' };
        }

        let arrow_x = to_left.saturating_sub(1).min(total_cols.saturating_sub(1));
        if target_y < canvas.len() {
            canvas[target_y][arrow_x] = '▶';
        }

        let label = edge_symbol(edge.on);
        let label_x = ((start_x + end_x) / 2).min(arrow_x.saturating_sub(1));
        if bridge_y < canvas.len() && label_x < total_cols {
            for (offset, ch) in label.chars().enumerate() {
                if label_x + offset < arrow_x {
                    canvas[bridge_y][label_x + offset] = ch;
                }
            }
        }

        let _ = from_left;
    }

    Ok(canvas
        .into_iter()
        .map(|row| row.into_iter().collect::<String>().trim_end().to_string())
        .collect())
}

fn render_detail_panel(app: &App, workspace: &WorkspaceState, command_suggestions: &[String]) -> Result<String> {
    let body = match (&workspace.selection, &workspace.detail_level) {
        (WorkspaceSelection::TaskOverview, _) => Ok(render_task_summary(&workspace.task_summary)),
        (WorkspaceSelection::Node { node_id }, DetailLevel::NodeHome) => render_node_home(app, workspace, node_id),
        (WorkspaceSelection::Node { node_id }, DetailLevel::AttemptItems { attempt_id, .. }) => {
            render_attempt_items(app, workspace, node_id, attempt_id)
        }
        (WorkspaceSelection::Node { node_id }, DetailLevel::Content) => render_content_view(app, workspace, node_id),
    }?;

    if command_suggestions.is_empty() {
        Ok(body)
    } else {
        Ok(format!("{}\n\nCommands\n{}", body, command_suggestions.iter().map(|item| format!("- {}", item)).collect::<Vec<_>>().join("\n")))
    }
}

fn render_task_summary(summary: &TaskSummary) -> String {
    let task = &summary.task;
    let title = task.title.as_deref().unwrap_or(task.id.as_str());
    let description = task.description.as_deref().unwrap_or("(no description)");
    let workflow = if summary.workflow_valid {
        "valid".to_string()
    } else if summary.workflow_exists {
        format!("invalid: {}", summary.workflow_error.as_deref().unwrap_or("unknown"))
    } else {
        "missing authoring/workflow.json".to_string()
    };
    let latest_run = summary
        .latest_run
        .as_ref()
        .map(|run| format!("{} ({:?})", run.id, run.status))
        .unwrap_or_else(|| "none".to_string());
    let resumable = summary.resumable_run_id.as_deref().unwrap_or("none");
    format!(
        "Task: {}\nTitle: {}\nDescription: {}\nWorkflow: {}\nLatest run: {}\nResumable run: {}",
        task.id, title, description, workflow, latest_run, resumable
    )
}

fn render_node_home(app: &App, workspace: &WorkspaceState, node_id: &str) -> Result<String> {
    let Some(run_id) = workspace.active_run_id.as_ref() else {
        return Ok(format!("Node: {}\nNo active run", node_id));
    };
    let Some(round_id) = workspace.selected_round_id.as_ref() else {
        return Ok(format!("Node: {}\nNo active round", node_id));
    };
    let workflow: WorkflowDsl = crate::storage::read_json(&app.paths.workflow_file(&workspace.task_id))?;
    let summary = app.node_runtime_summary(&workspace.task_id, run_id, round_id, &workflow, node_id)?;
    let mut lines = vec![format!("Node: {}", node_id)];
    if let Some(progress) = workspace.run_progress_summary.as_ref() {
        lines.push(format!("Run: {}", progress));
    }
    if let Some(run_events) = workspace.run_events_tail.as_ref() {
        lines.push(String::new());
        lines.push("Recent events".to_string());
        lines.extend(run_events.lines().map(|line| format!("- {}", line)));
    }
    lines.push(String::new());
    lines.push("Attempts".to_string());
    for (index, item) in workspace.detail_items.iter().enumerate() {
        match item {
            DetailSelection::RetryAction => {
                let marker = if workspace.detail_index == index { ">" } else { " " };
                lines.push(format!("{} retry current node", marker));
            }
            DetailSelection::Attempt { attempt_id } => {
                let marker = if workspace.detail_index == index { ">" } else { " " };
                let status = summary
                    .attempts
                    .iter()
                    .find(|attempt| &attempt.attempt_id == attempt_id)
                    .map(|attempt| format!("{:?}", attempt.status))
                    .unwrap_or_else(|| "unknown".to_string());
                lines.push(format!("{} {} [{}]", marker, attempt_id, status));
            }
            _ => {}
        }
    }
    if !summary.outgoing_edges.is_empty() {
        lines.push(String::new());
        lines.push("Outgoing".to_string());
        for edge in summary.outgoing_edges {
            lines.push(format!("- {} {}", edge_symbol(edge.on), edge.to));
        }
    }
    Ok(lines.join("\n"))
}

fn default_log_source(app: &App, task_id: &str, run_id: &str, round_id: &str, node_id: &str, attempt_id: &str) -> LogSource {
    if app.attempt_log_exists(task_id, run_id, round_id, node_id, attempt_id, LogSource::ProgressEvents) {
        LogSource::ProgressEvents
    } else {
        LogSource::RawStream
    }
}

fn locate_dag_node(dag_positions: &[Vec<String>], node_id: &str) -> Option<(usize, usize)> {
    for (column_index, column) in dag_positions.iter().enumerate() {
        if let Some(row_index) = column.iter().position(|candidate| candidate == node_id) {
            return Some((column_index, row_index));
        }
    }
    None
}

fn effective_attempt_id(app: &App, workspace: &WorkspaceState, attempt_id: &str) -> Result<String> {
    let WorkspaceSelection::Node { node_id } = &workspace.selection else {
        return Ok(attempt_id.to_string());
    };
    let Some(run_id) = workspace.active_run_id.as_ref() else {
        return Ok(attempt_id.to_string());
    };
    let Some(round_id) = workspace.selected_round_id.as_ref() else {
        return Ok(attempt_id.to_string());
    };
    Ok(match &workspace.detail_level {
        DetailLevel::AttemptItems { follow_live: true, .. } => app
            .current_attempt_selection(&workspace.task_id, run_id)?
            .and_then(|(active_round_id, active_node_id, active_attempt_id)| {
                (active_round_id == *round_id && active_node_id == *node_id).then_some(active_attempt_id)
            })
            .unwrap_or_else(|| attempt_id.to_string()),
        _ => attempt_id.to_string(),
    })
}

fn render_attempt_items(app: &App, workspace: &WorkspaceState, node_id: &str, attempt_id: &str) -> Result<String> {
    let Some(run_id) = workspace.active_run_id.as_ref() else {
        return Ok("No active run".to_string());
    };
    let Some(round_id) = workspace.selected_round_id.as_ref() else {
        return Ok("No active round".to_string());
    };
    let effective_attempt_id = effective_attempt_id(app, workspace, attempt_id)?;
    let maybe_attempt = app
        .attempt_list(&workspace.task_id, run_id, round_id, node_id)?
        .into_iter()
        .find(|attempt| attempt.attempt_id == effective_attempt_id);
    let log_title = match workspace.log_source {
        LogSource::ProgressEvents => "Provider input snapshot (progress.events.jsonl)",
        LogSource::RawStream => "Provider output (raw.stream.jsonl)",
    };
    let provider_output = app.attempt_log_tail(
        &workspace.task_id,
        run_id,
        round_id,
        node_id,
        &effective_attempt_id,
        workspace.log_source,
        10,
    )?;
    let mut lines = vec![
        format!("Attempt: {}", effective_attempt_id),
        format!(
            "Status: {}",
            maybe_attempt
                .as_ref()
                .map(|attempt| format!("{:?}", attempt.status))
                .unwrap_or_else(|| "pending-persist".to_string())
        ),
        format!(
            "Started: {}",
            maybe_attempt
                .as_ref()
                .map(|attempt| attempt.started_at.clone())
                .unwrap_or_else(|| "not persisted yet".to_string())
        ),
        format!(
            "Finished: {}",
            maybe_attempt
                .as_ref()
                .and_then(|attempt| attempt.finished_at.clone())
                .unwrap_or_else(|| "not finished".to_string())
        ),
        format!("Follow live: {}", matches!(workspace.detail_level, DetailLevel::AttemptItems { follow_live: true, .. })),
    ];
    if maybe_attempt.is_none() {
        lines.push("Attempt state file is not available yet. Waiting for runtime persistence...".to_string());
    }
    if app.attempt_log_exists(&workspace.task_id, run_id, round_id, node_id, &effective_attempt_id, workspace.log_source) {
        if let Some(output) = provider_output {
            lines.push(String::new());
            lines.push(log_title.to_string());
            lines.extend(output.lines().map(|line| line.to_string()));
        }
    } else {
        lines.push(String::new());
        lines.push(log_title.to_string());
        lines.push("Selected log source not available. Press l to switch source.".to_string());
    }
    lines.push(String::new());
    lines.push("Items".to_string());
    for (index, item) in workspace.detail_items.iter().enumerate() {
        let marker = if workspace.detail_index == index { ">" } else { " " };
        match item {
            DetailSelection::Artifact { name, .. } => lines.push(format!("{} artifact {}", marker, name)),
            DetailSelection::Attachment { name, .. } => lines.push(format!("{} attachment {}", marker, name)),
            _ => {}
        }
    }
    Ok(lines.join("\n"))
}

fn render_content_view(app: &App, workspace: &WorkspaceState, node_id: &str) -> Result<String> {
    let Some(run_id) = workspace.active_run_id.as_ref() else {
        return Ok("No active run".to_string());
    };
    let Some(round_id) = workspace.selected_round_id.as_ref() else {
        return Ok("No active round".to_string());
    };
    let Some(item) = workspace.detail_items.get(workspace.detail_index) else {
        return Ok("No content selected".to_string());
    };
    match item {
        DetailSelection::Artifact { attempt_id, name } => app.artifact_show(&workspace.task_id, run_id, round_id, node_id, attempt_id, name),
        DetailSelection::Attachment { attempt_id, name } => app.attachment_show(&workspace.task_id, run_id, round_id, node_id, attempt_id, name),
        _ => Ok("No content selected".to_string()),
    }
}

fn build_node_home_items(app: &App, workspace: &WorkspaceState) -> Result<Vec<DetailSelection>> {
    let WorkspaceSelection::Node { node_id } = &workspace.selection else {
        return Ok(Vec::new());
    };
    let Some(run_id) = workspace.active_run_id.as_ref() else {
        return Ok(Vec::new());
    };
    let Some(round_id) = workspace.selected_round_id.as_ref() else {
        return Ok(Vec::new());
    };
    let attempts = app.attempt_list(&workspace.task_id, run_id, round_id, node_id)?;
    let mut items = vec![DetailSelection::RetryAction];
    items.extend(attempts.into_iter().rev().map(|attempt| DetailSelection::Attempt {
        attempt_id: attempt.attempt_id,
    }));
    Ok(items)
}

fn build_attempt_items(app: &App, workspace: &WorkspaceState, attempt_id: &str) -> Result<Vec<DetailSelection>> {
    let WorkspaceSelection::Node { node_id } = &workspace.selection else {
        return Ok(Vec::new());
    };
    let Some(run_id) = workspace.active_run_id.as_ref() else {
        return Ok(Vec::new());
    };
    let Some(round_id) = workspace.selected_round_id.as_ref() else {
        return Ok(Vec::new());
    };
    let mut items = app
        .artifact_list(&workspace.task_id, run_id, round_id, node_id, attempt_id)?
        .into_iter()
        .map(|name| DetailSelection::Artifact {
            attempt_id: attempt_id.to_string(),
            name: name.trim_end_matches(".json").to_string(),
        })
        .collect::<Vec<_>>();
    items.extend(
        app.attachment_list(&workspace.task_id, run_id, round_id, node_id, attempt_id)?
            .into_iter()
            .map(|name| DetailSelection::Attachment {
                attempt_id: attempt_id.to_string(),
                name,
            }),
    );
    Ok(items)
}

fn node_status_label(active_run: Option<&RunState>, node_id: &str) -> &'static str {
    let Some(run) = active_run else {
        return "idle";
    };
    if run.current_node.as_deref() == Some(node_id) {
        return "current";
    }
    match run.status {
        crate::domain::RunStatus::Completed => "done",
        crate::domain::RunStatus::Paused => "paused",
        crate::domain::RunStatus::Running => "seen",
    }
}

fn dag_columns(workflow: &WorkflowDsl) -> Vec<Vec<String>> {
    let mut adjacency = BTreeMap::<String, Vec<String>>::new();
    let mut indegree = BTreeMap::<String, usize>::new();
    for node in &workflow.nodes {
        adjacency.entry(node.id().to_string()).or_default();
        indegree.entry(node.id().to_string()).or_insert(0);
    }
    for edge in &workflow.edges {
        if edge.to == crate::dsl::END_NODE {
            continue;
        }
        adjacency.entry(edge.from.clone()).or_default().push(edge.to.clone());
        *indegree.entry(edge.to.clone()).or_insert(0) += 1;
    }
    let mut queue = VecDeque::new();
    queue.push_back(workflow.entry.clone());
    let mut depth = BTreeMap::<String, usize>::new();
    depth.insert(workflow.entry.clone(), 0);
    while let Some(node_id) = queue.pop_front() {
        let current_depth = depth.get(&node_id).copied().unwrap_or(0);
        if let Some(targets) = adjacency.get(&node_id) {
            for target in targets {
                let next_depth = current_depth + 1;
                let entry = depth.entry(target.clone()).or_insert(next_depth);
                if next_depth > *entry {
                    *entry = next_depth;
                }
                queue.push_back(target.clone());
            }
        }
    }
    let mut columns = BTreeMap::<usize, Vec<String>>::new();
    for node in &workflow.nodes {
        let column = depth.get(node.id()).copied().unwrap_or(0);
        columns.entry(column).or_default().push(node.id().to_string());
    }
    columns.into_values().collect()
}

fn welcome_line(state: &ConsoleState, action: WelcomeAction, label: &str) -> String {
    let marker = if state.welcome_action == action { '◆' } else { '·' };
    format!("{} {}", marker, label)
}

fn draw_box(canvas: &mut [Vec<char>], x: usize, y: usize, width: usize, height: usize, title: &str, body: &str, selected: bool) {
    if canvas.is_empty() || width < 6 || height < 3 {
        return;
    }
    let max_y = (y + height - 1).min(canvas.len() - 1);
    let max_x = (x + width - 1).min(canvas[0].len() - 1);
    let inner_start = x + 1;
    let inner_end = max_x.saturating_sub(1);

    let (top_left, top_right, bottom_left, bottom_right, stem) = if selected {
        ('┏', '┓', '┗', '┛', '┃')
    } else {
        ('╭', '╮', '╰', '╯', '╵')
    };
    let horizontal = if selected { '━' } else { '─' };

    canvas[y][x] = top_left;
    canvas[y][max_x] = top_right;
    canvas[max_y][x] = bottom_left;
    canvas[max_y][max_x] = bottom_right;
    for col in x + 1..max_x {
        canvas[y][col] = horizontal;
        canvas[max_y][col] = horizontal;
    }

    if y + 1 <= max_y.saturating_sub(1) {
        if x < canvas[0].len() {
            canvas[y + 1][x] = stem;
        }
        if max_x < canvas[0].len() {
            canvas[y + 1][max_x] = stem;
        }
    }

    let title_limit = inner_end.saturating_sub(inner_start).saturating_add(1);
    for (offset, ch) in title.chars().take(title_limit).enumerate() {
        let col = inner_start + offset;
        if col <= inner_end {
            canvas[y][col] = ch;
        }
    }

    if y + 1 < canvas.len() {
        let status = format!("[{}]", body);
        for (offset, ch) in status.chars().take(title_limit).enumerate() {
            let col = inner_start + offset;
            if col <= inner_end {
                canvas[y + 1][col] = ch;
            }
        }
    }
}

fn summarize_run_progress(value: &serde_json::Value) -> String {
    let status = value.get("status").and_then(|v| v.as_str()).unwrap_or("unknown");
    let node = value.get("current_node").and_then(|v| v.as_str()).unwrap_or("none");
    let attempt = value.get("current_attempt").and_then(|v| v.as_str()).unwrap_or("none");
    let round = value.get("current_round").and_then(|v| v.as_str()).unwrap_or("none");
    format!("status={} round={} node={} attempt={}", status, round, node, attempt)
}

fn tail_lines(text: &str, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }
    let trimmed = text.trim_end_matches('\n');
    if trimmed.is_empty() {
        return String::new();
    }
    let lines = trimmed.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(limit);
    lines[start..].join("\n")
}

fn edge_symbol(outcome: EdgeOutcome) -> &'static str {
    match outcome {
        EdgeOutcome::Success => "✔",
        EdgeOutcome::Failure => "✘",
        EdgeOutcome::Invalid => "?",
    }
}

fn pane_title(title: &str, focused: bool) -> String {
    if focused {
        format!("{} *", title)
    } else {
        title.to_string()
    }
}
