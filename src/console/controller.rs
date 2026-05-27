use anyhow::{Result, anyhow};
use serde_json::to_string_pretty;
use std::thread;

use crate::app::{App, LogSource};
use crate::command::execute::execute_command;
use crate::command::{Command, CommandResult, RunCommand};
use crate::config::ConsoleThemeName;
use crate::domain::RunStatus;
use crate::inspect::{render_console_help, render_run_help};

use super::commands::{
    ConsoleLocalCommand, ParsedConsoleCommand, parse_console_command, suggest_console_commands,
};
use super::state::{
    BackgroundTaskState, CommandViewKind, ConsoleState, DetailLevel, DetailSelection, FocusPane,
    OverlayState, Screen, WelcomeAction, WorkspaceSelection,
};
use super::view_models::{build_workspace_state, sync_workspace_detail};

pub fn submit_input(app: &App, state: &mut ConsoleState) -> Result<()> {
    let input = state.input.trim().to_string();
    let parsed = parse_console_command(&input)?;
    state.history.push(input.clone());
    state.command_suggestions.clear();
    state.message = None;

    match parsed {
        ParsedConsoleCommand::Local(command) => apply_local_command(app, state, command),
        ParsedConsoleCommand::Runtime(command) => apply_runtime_command(app, state, command),
    }
}

pub fn refresh_command_suggestions(state: &mut ConsoleState) {
    state.command_suggestions = suggest_console_commands(&state.input);
}

pub fn start_command_input(state: &mut ConsoleState) {
    if matches!(state.screen, Screen::Welcome) {
        return;
    }
    state.message = None;
    if state.overlay.is_some() {
        return;
    }
    if state.input.is_empty() {
        state.input.push('/');
    }
    state.focus = FocusPane::Input;
    refresh_command_suggestions(state);
}

pub fn show_help_overlay(app: &App, state: &mut ConsoleState) -> Result<()> {
    if matches!(state.screen, Screen::Welcome) {
        state.message = Some(render_console_help());
        return Ok(());
    }
    show_overlay(app, state, CommandViewKind::Help, render_help_body(state))
}

pub fn cycle_focus(state: &mut ConsoleState) {
    state.focus = match state.screen {
        Screen::Welcome => FocusPane::Welcome,
        Screen::TaskPicker => match state.focus {
            FocusPane::TaskPicker => FocusPane::Input,
            _ => FocusPane::TaskPicker,
        },
        Screen::Workspace => {
            if state.overlay.is_some() {
                FocusPane::Overlay
            } else {
                match (state.layout_mode, state.focus) {
                    (_, FocusPane::Overlay) => FocusPane::Dag,
                    (_, FocusPane::Input) => FocusPane::Dag,
                    (super::state::LayoutMode::Compact, FocusPane::Dag) => FocusPane::Detail,
                    (super::state::LayoutMode::Compact, FocusPane::Detail) => FocusPane::Input,
                    (super::state::LayoutMode::Compact, _) => FocusPane::Dag,
                    (_, FocusPane::Dag) => FocusPane::Detail,
                    (_, FocusPane::Detail) => FocusPane::Input,
                    _ => FocusPane::Dag,
                }
            }
        }
    };
}

pub fn move_up(state: &mut ConsoleState) {
    match state.screen {
        Screen::Welcome => {
            state.welcome_action = WelcomeAction::AddTask;
        }
        Screen::TaskPicker => match state.focus {
            FocusPane::Overlay => {
                if let Some(overlay) = state.overlay.as_mut() {
                    overlay.scroll = overlay.scroll.saturating_sub(1);
                }
            }
            _ => {
                if state.task_index > 0 {
                    state.task_index -= 1;
                }
            }
        },
        Screen::Workspace => match state.focus {
            FocusPane::Dag => {
                if let Some(workspace) = state.workspace.as_mut() {
                    if workspace.dag_row > 0 {
                        workspace.dag_row -= 1;
                        sync_dag_selection(workspace);
                    }
                }
            }
            FocusPane::Detail => {
                if let Some(workspace) = state.workspace.as_mut() {
                    if matches!(
                        workspace.detail_level,
                        DetailLevel::AttemptItems { .. } | DetailLevel::Content
                    ) {
                        workspace.log_scroll = workspace.log_scroll.saturating_sub(1);
                    } else {
                        workspace.detail_index = workspace.detail_index.saturating_sub(1);
                    }
                }
            }
            FocusPane::Overlay => {
                if let Some(overlay) = state.overlay.as_mut() {
                    overlay.scroll = overlay.scroll.saturating_sub(1);
                }
            }
            _ => {}
        },
    }
}

pub fn toggle_log_source(state: &mut ConsoleState) {
    if state.screen != Screen::Workspace || state.focus != FocusPane::Detail {
        return;
    }
    let Some(workspace) = state.workspace.as_mut() else {
        return;
    };
    if !matches!(workspace.detail_level, DetailLevel::AttemptItems { .. }) {
        return;
    }
    workspace.log_source = match workspace.log_source {
        LogSource::ProgressEvents => LogSource::RawStream,
        LogSource::RawStream => LogSource::ProgressEvents,
    };
    workspace.log_scroll = 0;
}

pub fn move_down(state: &mut ConsoleState) {
    match state.screen {
        Screen::Welcome => {
            state.welcome_action = WelcomeAction::SelectTask;
        }
        Screen::TaskPicker => match state.focus {
            FocusPane::Overlay => {
                if let Some(overlay) = state.overlay.as_mut() {
                    overlay.scroll = overlay.scroll.saturating_add(1);
                }
            }
            _ => {
                if state.task_index + 1 < state.task_list.len() {
                    state.task_index += 1;
                }
            }
        },
        Screen::Workspace => match state.focus {
            FocusPane::Dag => {
                if let Some(workspace) = state.workspace.as_mut() {
                    if let Some(column) = workspace.dag_positions.get(workspace.dag_column) {
                        if workspace.dag_row + 1 < column.len() {
                            workspace.dag_row += 1;
                            sync_dag_selection(workspace);
                        }
                    }
                }
            }
            FocusPane::Detail => {
                if let Some(workspace) = state.workspace.as_mut() {
                    if matches!(
                        workspace.detail_level,
                        DetailLevel::AttemptItems { .. } | DetailLevel::Content
                    ) {
                        workspace.log_scroll = workspace.log_scroll.saturating_add(1);
                    } else if workspace.detail_index + 1 < workspace.detail_items.len() {
                        workspace.detail_index += 1;
                    }
                }
            }
            FocusPane::Overlay => {
                if let Some(overlay) = state.overlay.as_mut() {
                    overlay.scroll = overlay.scroll.saturating_add(1);
                }
            }
            _ => {}
        },
    }
}

pub fn move_left(state: &mut ConsoleState) {
    if state.screen != Screen::Workspace || state.focus != FocusPane::Dag {
        return;
    }
    if let Some(workspace) = state.workspace.as_mut() {
        if workspace.dag_column > 0 {
            workspace.dag_column -= 1;
            if let Some(column) = workspace.dag_positions.get(workspace.dag_column) {
                workspace.dag_row = workspace.dag_row.min(column.len().saturating_sub(1));
            }
            sync_dag_selection(workspace);
        }
    }
}

pub fn move_right(state: &mut ConsoleState) {
    if state.screen != Screen::Workspace || state.focus != FocusPane::Dag {
        return;
    }
    if let Some(workspace) = state.workspace.as_mut() {
        if workspace.dag_column + 1 < workspace.dag_positions.len() {
            workspace.dag_column += 1;
            if let Some(column) = workspace.dag_positions.get(workspace.dag_column) {
                workspace.dag_row = workspace.dag_row.min(column.len().saturating_sub(1));
            }
            sync_dag_selection(workspace);
        }
    }
}

pub fn activate_current(app: &App, state: &mut ConsoleState) -> Result<()> {
    match state.screen {
        Screen::Welcome => match state.welcome_action {
            WelcomeAction::AddTask => {
                state.message = Some("新增 task 本期暂未实现".to_string());
            }
            WelcomeAction::SelectTask => open_task_picker(app, state)?,
        },
        Screen::TaskPicker => match state.focus {
            FocusPane::Input => {
                if !state.input.trim().is_empty() {
                    match submit_input(app, state) {
                        Ok(()) => state.input.clear(),
                        Err(err) => state.message = Some(err.to_string()),
                    }
                }
            }
            _ => open_selected_task(app, state)?,
        },
        Screen::Workspace => match state.focus {
            FocusPane::Dag => open_selected_node(app, state)?,
            FocusPane::Detail => open_detail_selection(app, state)?,
            FocusPane::Input => {
                if !state.input.trim().is_empty() {
                    match submit_input(app, state) {
                        Ok(()) => state.input.clear(),
                        Err(err) => state.message = Some(err.to_string()),
                    }
                }
            }
            FocusPane::Overlay => {}
            _ => {}
        },
    }
    Ok(())
}

pub fn escape(app: &App, state: &mut ConsoleState) -> Result<bool> {
    match state.screen {
        Screen::Welcome => Ok(true),
        Screen::TaskPicker => {
            if let Some(overlay) = state.overlay.take() {
                state.focus = overlay.return_focus;
                return Ok(false);
            }
            state.screen = Screen::Welcome;
            state.focus = FocusPane::Welcome;
            state.command_suggestions.clear();
            Ok(false)
        }
        Screen::Workspace => {
            if let Some(overlay) = state.overlay.take() {
                state.focus = overlay.return_focus;
                return Ok(false);
            }
            if let Some(workspace) = state.workspace.as_mut() {
                match workspace.detail_level {
                    DetailLevel::Content => {
                        if let Some(
                            DetailSelection::Artifact { attempt_id, .. }
                            | DetailSelection::Attachment { attempt_id, .. },
                        ) = workspace.detail_items.get(workspace.detail_index).cloned()
                        {
                            workspace.detail_level = DetailLevel::AttemptItems {
                                attempt_id,
                                follow_live: false,
                            };
                            sync_workspace_detail(app, workspace)?;
                        }
                        return Ok(false);
                    }
                    DetailLevel::AttemptItems { .. } => {
                        workspace.detail_level = DetailLevel::NodeHome;
                        sync_workspace_detail(app, workspace)?;
                        return Ok(false);
                    }
                    DetailLevel::NodeHome => {
                        if state.focus != FocusPane::Dag {
                            state.focus = FocusPane::Dag;
                            return Ok(false);
                        }
                    }
                }
            }
            state.screen = Screen::TaskPicker;
            state.focus = FocusPane::TaskPicker;
            state.workspace = None;
            state.task_list = app.task_summaries()?;
            Ok(false)
        }
    }
}

pub fn refresh_tick(app: &App, state: &mut ConsoleState) -> Result<()> {
    if !state.auto_refresh_enabled {
        return Ok(());
    }
    match state.screen {
        Screen::TaskPicker => {
            state.task_list = app.task_summaries()?;
            if state.task_index >= state.task_list.len() {
                state.task_index = state.task_list.len().saturating_sub(1);
            }
        }
        Screen::Workspace => refresh_workspace(app, state)?,
        Screen::Welcome => {}
    }
    state.last_refresh_label = Some("auto".to_string());
    Ok(())
}

fn apply_local_command(
    app: &App,
    state: &mut ConsoleState,
    command: ConsoleLocalCommand,
) -> Result<()> {
    match command {
        ConsoleLocalCommand::Help => {
            show_overlay(app, state, CommandViewKind::Help, render_help_body(state))
        }
        ConsoleLocalCommand::Task => open_task_picker(app, state),
        ConsoleLocalCommand::Log => {
            let body = app
                .runtime_log_tail_show(500)?
                .unwrap_or_else(|| "runtime log not found".to_string());
            show_overlay(app, state, CommandViewKind::Log, body)
        }
        ConsoleLocalCommand::Config => {
            let persisted_user_theme = app.load_settings()?.console_theme;
            let body = format!(
                "{}\n\nConsole Session\n  startup_theme: {}\n  persisted_user_theme: {}\n  effective_theme: {}\n  source: {}",
                to_string_pretty(&app.config)?,
                theme_name(app.config.console_theme),
                persisted_user_theme.map(theme_name).unwrap_or("<unset>"),
                theme_name(state.console_theme),
                if state.console_theme == app.config.console_theme {
                    "startup config"
                } else {
                    "console command"
                }
            );
            show_overlay(app, state, CommandViewKind::Config, body)
        }
        ConsoleLocalCommand::ThemeShow => show_overlay(
            app,
            state,
            CommandViewKind::Notice,
            render_theme_help(app, state),
        ),
        ConsoleLocalCommand::ThemeSet(theme) => {
            let persisted = app.set_user_console_theme(theme)?;
            state.console_theme = theme;
            show_overlay(
                app,
                state,
                CommandViewKind::Notice,
                format!(
                    "Console theme switched to {}.\n\nPersisted user theme: {}\nStartup theme: {}\nEffective session theme: {}",
                    theme_name(theme),
                    persisted.console_theme.map(theme_name).unwrap_or("<unset>"),
                    theme_name(app.config.console_theme),
                    theme_name(state.console_theme)
                ),
            )
        }
        ConsoleLocalCommand::Continue => continue_workspace_run(app, state),
    }
}

fn apply_runtime_command(app: &App, state: &mut ConsoleState, command: Command) -> Result<()> {
    let task_to_open = match &command {
        Command::Run(RunCommand::Start { task_id, .. }) => Some(task_id.clone()),
        _ => None,
    };
    if let Some(task_id) = task_to_open {
        spawn_run_start(app, &task_id);
        state.background_task = Some(BackgroundTaskState {
            task_id: task_id.clone(),
            kind: "start",
            error: None,
        });
        state.last_refresh_label = Some("starting run".to_string());
        enter_task_workspace(app, state, &task_id)?;
        return focus_workspace_runtime_detail(app, state);
    }
    let result = execute_command(app, command)?;
    let body = match result {
        CommandResult::Json(value) => to_string_pretty(&value)?,
        CommandResult::Text(text) => text,
    };
    if state.workspace.is_some() {
        show_overlay(app, state, CommandViewKind::RuntimeCommand, body)?;
    } else {
        state.message = Some(body);
    }
    Ok(())
}

fn open_task_picker(app: &App, state: &mut ConsoleState) -> Result<()> {
    state.task_list = app.task_summaries()?;
    state.task_index = 0;
    state.message = None;
    state.screen = Screen::TaskPicker;
    state.focus = FocusPane::TaskPicker;
    state.command_suggestions.clear();
    Ok(())
}

fn open_selected_task(app: &App, state: &mut ConsoleState) -> Result<()> {
    let Some(summary) = state.task_list.get(state.task_index).cloned() else {
        return Ok(());
    };
    open_task_workspace(app, state, summary)?;
    focus_workspace_runtime_detail(app, state)
}

pub fn start_selected_task(app: &App, state: &mut ConsoleState) -> Result<()> {
    if state.screen != Screen::TaskPicker
        || state.focus != FocusPane::TaskPicker
        || state.overlay.is_some()
    {
        return Ok(());
    }
    let Some(summary) = state.task_list.get(state.task_index).cloned() else {
        return Ok(());
    };
    if !summary.workflow_valid {
        let reason = summary
            .workflow_error
            .as_deref()
            .unwrap_or("workflow invalid");
        return show_overlay(
            app,
            state,
            CommandViewKind::Notice,
            format!(
                "Task {} cannot start yet.\n\nReason\n{}",
                summary.task.id, reason
            ),
        );
    }
    spawn_run_start(app, &summary.task.id);
    state.background_task = Some(BackgroundTaskState {
        task_id: summary.task.id.clone(),
        kind: "start",
        error: None,
    });
    state.last_refresh_label = Some("starting run".to_string());
    enter_task_workspace(app, state, &summary.task.id)?;
    focus_workspace_runtime_detail(app, state)
}

fn spawn_run_start(app: &App, task_id: &str) {
    let repo_root = app.paths.repo_root.clone();
    let config = app.config.clone();
    let task_id = task_id.to_string();
    let task_id_for_thread = task_id.clone();
    thread::spawn(move || {
        let app = App::with_config(repo_root, config);
        if let Err(err) = execute_command(
            &app,
            Command::Run(RunCommand::Start {
                task_id: task_id_for_thread.clone(),
                workflow: None,
            }),
        ) {
            let _ = std::fs::create_dir_all(app.paths.runs_dir(&task_id_for_thread).as_std_path());
            let _ = std::fs::write(
                app.paths
                    .runs_dir(&task_id_for_thread)
                    .join("console-start-error.txt")
                    .as_std_path(),
                err.to_string(),
            );
        }
    });
}

fn focus_workspace_runtime_detail(app: &App, state: &mut ConsoleState) -> Result<()> {
    let Some(workspace) = state.workspace.as_mut() else {
        return Ok(());
    };
    workspace.detail_level = DetailLevel::NodeHome;
    workspace.detail_index = 0;
    state.overlay = None;
    sync_workspace_detail(app, workspace)?;

    if let Some(run_id) = workspace.active_run_id.clone() {
        if let Some((round_id, node_id, attempt_id)) =
            app.current_attempt_selection(&workspace.task_id, &run_id)?
        {
            if workspace.selected_round_id.as_deref() == Some(round_id.as_str()) {
                workspace.selection = WorkspaceSelection::Node {
                    node_id: node_id.clone(),
                };
                workspace.log_source = if app.attempt_log_exists(
                    &workspace.task_id,
                    &run_id,
                    &round_id,
                    &node_id,
                    &attempt_id,
                    LogSource::ProgressEvents,
                ) {
                    LogSource::ProgressEvents
                } else {
                    LogSource::RawStream
                };
                workspace.detail_level = DetailLevel::AttemptItems {
                    attempt_id,
                    follow_live: true,
                };
                workspace.detail_index = 0;
                sync_workspace_detail(app, workspace)?;
                state.focus = FocusPane::Detail;
                return Ok(());
            }
        }
    }

    if let Some(attempt_index) = workspace
        .detail_items
        .iter()
        .position(|item| matches!(item, DetailSelection::Attempt { .. }))
    {
        workspace.detail_index = attempt_index;
        if let Some(DetailSelection::Attempt { attempt_id }) =
            workspace.detail_items.get(attempt_index).cloned()
        {
            workspace.log_source =
                if let (Some(run_id), Some(round_id), WorkspaceSelection::Node { node_id }) = (
                    workspace.active_run_id.as_ref(),
                    workspace.selected_round_id.as_ref(),
                    &workspace.selection,
                ) {
                    if app.attempt_log_exists(
                        &workspace.task_id,
                        run_id,
                        round_id,
                        node_id,
                        &attempt_id,
                        LogSource::ProgressEvents,
                    ) {
                        LogSource::ProgressEvents
                    } else {
                        LogSource::RawStream
                    }
                } else {
                    LogSource::RawStream
                };
            workspace.detail_level = DetailLevel::AttemptItems {
                attempt_id,
                follow_live: false,
            };
            workspace.detail_index = 0;
            sync_workspace_detail(app, workspace)?;
        }
    }
    state.focus = FocusPane::Detail;
    Ok(())
}

fn open_selected_node(app: &App, state: &mut ConsoleState) -> Result<()> {
    let Some(workspace) = state.workspace.as_mut() else {
        return Ok(());
    };
    workspace.detail_level = DetailLevel::NodeHome;
    state.overlay = None;
    sync_workspace_detail(app, workspace)?;
    state.focus = FocusPane::Detail;
    Ok(())
}

fn open_detail_selection(app: &App, state: &mut ConsoleState) -> Result<()> {
    let Some(workspace) = state.workspace.as_mut() else {
        return Ok(());
    };
    let Some(item) = workspace.detail_items.get(workspace.detail_index).cloned() else {
        return Ok(());
    };
    match item {
        DetailSelection::RetryAction => retry_selected_node(app, state),
        DetailSelection::Attempt { attempt_id } => {
            workspace.log_source =
                if let (Some(run_id), Some(round_id), WorkspaceSelection::Node { node_id }) = (
                    workspace.active_run_id.as_ref(),
                    workspace.selected_round_id.as_ref(),
                    &workspace.selection,
                ) {
                    if app.attempt_log_exists(
                        &workspace.task_id,
                        run_id,
                        round_id,
                        node_id,
                        &attempt_id,
                        LogSource::ProgressEvents,
                    ) {
                        LogSource::ProgressEvents
                    } else {
                        LogSource::RawStream
                    }
                } else {
                    LogSource::RawStream
                };
            workspace.detail_level = DetailLevel::AttemptItems {
                attempt_id,
                follow_live: false,
            };
            workspace.detail_index = 0;
            sync_workspace_detail(app, workspace)
        }
        DetailSelection::Artifact { .. } | DetailSelection::Attachment { .. } => {
            workspace.detail_level = DetailLevel::Content;
            Ok(())
        }
    }
}

fn retry_selected_node(app: &App, state: &mut ConsoleState) -> Result<()> {
    let (task_id, run_id) = match state.workspace.as_ref() {
        Some(workspace) => (
            workspace.task_id.clone(),
            workspace
                .active_run_id
                .clone()
                .ok_or_else(|| anyhow!("no active run to retry"))?,
        ),
        None => return Ok(()),
    };
    let result = execute_command(app, Command::Run(RunCommand::Retry { task_id, run_id }))?;
    let body = match result {
        CommandResult::Json(value) => to_string_pretty(&value)?,
        CommandResult::Text(text) => text,
    };
    show_overlay(app, state, CommandViewKind::RuntimeCommand, body)
}

fn open_task_workspace(
    app: &App,
    state: &mut ConsoleState,
    summary: crate::app::TaskSummary,
) -> Result<()> {
    if !summary.workflow_valid {
        let reason = summary
            .workflow_error
            .as_deref()
            .unwrap_or("workflow invalid");
        return show_overlay(
            app,
            state,
            CommandViewKind::Notice,
            format!(
                "Task {} is not enterable yet.\n\nReason\n{}",
                summary.task.id, reason
            ),
        );
    }
    let workspace = build_workspace_state(app, summary)?;
    state.workspace = Some(workspace);
    state.message = None;
    state.screen = Screen::Workspace;
    state.focus = FocusPane::Dag;
    state.command_suggestions.clear();
    Ok(())
}

fn enter_task_workspace(app: &App, state: &mut ConsoleState, task_id: &str) -> Result<()> {
    let summary = app.task_summary(task_id)?;
    open_task_workspace(app, state, summary)
}

fn refresh_workspace(app: &App, state: &mut ConsoleState) -> Result<()> {
    let mut background_task_attached = false;
    if let Some(background_task) = state.background_task.clone() {
        let runs_dir = app.paths.runs_dir(&background_task.task_id);
        let error_path = runs_dir.join("console-start-error.txt");
        if error_path.exists() {
            let error = std::fs::read_to_string(error_path.as_std_path())?
                .trim()
                .to_string();
            let _ = std::fs::remove_file(error_path.as_std_path());
            state.background_task = None;
            state.last_refresh_label = Some(format!("{} failed", background_task.kind));
            state.message = Some(error);
        } else {
            let summary = app.task_summary(&background_task.task_id)?;
            if summary.latest_run.is_some() {
                state.background_task = None;
                state.last_refresh_label = Some(format!("{} attached", background_task.kind));
                background_task_attached = true;
            }
        }
    }
    let Some(current) = state.workspace.as_ref() else {
        return Ok(());
    };
    let summary = app.task_summary(&current.task_id)?;
    let previous_focus = state.focus;
    let previous_overlay = state.overlay.clone();
    let previous_level = current.detail_level.clone();
    let previous_selection = current.selection.clone();
    let previous_detail_index = current.detail_index;
    let previous_detail_scroll = current.detail_scroll;
    let previous_log_source = current.log_source;
    let previous_log_scroll = current.log_scroll;
    let previous_column = current.dag_column;
    let previous_row = current.dag_row;
    let preserve_live_runtime_attach = matches!(
        previous_level,
        DetailLevel::AttemptItems {
            follow_live: true,
            ..
        }
    );
    let mut workspace = build_workspace_state(app, summary)?;
    let run_changed = current.active_run_id != workspace.active_run_id;
    if !preserve_live_runtime_attach {
        workspace.selection = previous_selection;
        workspace.detail_level = previous_level;
        workspace.detail_index = previous_detail_index;
        workspace.detail_scroll = previous_detail_scroll;
        workspace.dag_column = previous_column.min(workspace.dag_positions.len().saturating_sub(1));
        if let Some(column) = workspace.dag_positions.get(workspace.dag_column) {
            workspace.dag_row = previous_row.min(column.len().saturating_sub(1));
        } else {
            workspace.dag_row = 0;
        }
    }
    workspace.log_source = previous_log_source;
    workspace.log_scroll = previous_log_scroll;
    if run_changed {
        workspace.detail_level = DetailLevel::NodeHome;
        workspace.detail_index = 0;
        workspace.detail_scroll = 0;
        workspace.log_scroll = 0;
    }
    sync_workspace_detail(app, &mut workspace)?;
    state.workspace = Some(workspace);
    state.focus = previous_focus;
    state.overlay = previous_overlay;
    if background_task_attached {
        focus_workspace_runtime_detail(app, state)?;
    }
    Ok(())
}

fn continue_workspace_run(app: &App, state: &mut ConsoleState) -> Result<()> {
    let Some(workspace) = state.workspace.as_ref() else {
        state.message = Some("No active workspace".to_string());
        return Ok(());
    };
    let Some(run_id) = workspace.active_run_id.clone() else {
        show_overlay(
            app,
            state,
            CommandViewKind::ContinueResult,
            "No resumable run".to_string(),
        )?;
        return Ok(());
    };
    let task_id = workspace.task_id.clone();
    let result = execute_command(
        app,
        Command::Run(RunCommand::Continue {
            task_id: task_id.clone(),
            run_id,
        }),
    )?;
    enter_task_workspace(app, state, &task_id)?;
    if let Some(workspace) = state.workspace.as_ref() {
        if let Some(active_run_id) = workspace.active_run_id.as_ref() {
            if let Ok(run) = app.run_status(&task_id, active_run_id) {
                if run.status == RunStatus::Paused {
                    let body = match result {
                        CommandResult::Json(value) => to_string_pretty(&value)?,
                        CommandResult::Text(text) => text,
                    };
                    show_overlay(app, state, CommandViewKind::ContinueResult, body)?;
                }
            }
        }
    }
    Ok(())
}

fn show_overlay(
    _app: &App,
    state: &mut ConsoleState,
    kind: CommandViewKind,
    body: String,
) -> Result<()> {
    let return_focus = match state.focus {
        FocusPane::Overlay => match state.screen {
            Screen::TaskPicker => FocusPane::TaskPicker,
            Screen::Workspace => FocusPane::Dag,
            Screen::Welcome => FocusPane::Welcome,
        },
        other => other,
    };
    state.overlay = Some(OverlayState {
        kind,
        body,
        scroll: 0,
        return_focus,
    });
    state.focus = FocusPane::Overlay;
    Ok(())
}

fn render_help_body(state: &ConsoleState) -> String {
    if state.input.trim() == "/run --help" {
        render_run_help()
    } else {
        render_console_help()
    }
}

fn render_theme_help(app: &App, state: &ConsoleState) -> String {
    let persisted_user_theme = app
        .load_settings()
        .ok()
        .and_then(|settings| settings.console_theme);
    format!(
        "Console Themes\n  startup: {}\n  persisted: {}\n  effective: {}\n\nAvailable\n  gold-band\n  nord\n  dracula\n  cyber\n  onyx\n  mist\n  high-contrast\n\nUsage\n  /theme\n  /theme cyber",
        theme_name(app.config.console_theme),
        persisted_user_theme.map(theme_name).unwrap_or("<unset>"),
        theme_name(state.console_theme),
    )
}

fn theme_name(theme: ConsoleThemeName) -> &'static str {
    match theme {
        ConsoleThemeName::GoldBand => "gold-band",
        ConsoleThemeName::Nord => "nord",
        ConsoleThemeName::Dracula => "dracula",
        ConsoleThemeName::Cyber => "cyber",
        ConsoleThemeName::Onyx => "onyx",
        ConsoleThemeName::Mist => "mist",
        ConsoleThemeName::HighContrast => "high-contrast",
    }
}

fn sync_dag_selection(workspace: &mut super::state::WorkspaceState) {
    if let Some(node_id) = workspace
        .dag_positions
        .get(workspace.dag_column)
        .and_then(|column| column.get(workspace.dag_row))
        .cloned()
    {
        workspace.selection = WorkspaceSelection::Node { node_id };
    }
}
