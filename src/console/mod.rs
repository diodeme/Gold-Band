pub mod commands;
pub mod controller;
#[allow(dead_code)]
mod events;
pub mod state;
mod theme;
pub mod view_models;

use std::io::{self, Stdout};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Terminal;

use crate::app::App;

use self::controller::{activate_current, cycle_focus, escape, move_down, move_left, move_right, move_up, refresh_command_suggestions, refresh_tick, show_help_overlay, start_command_input, start_selected_task, toggle_log_source};
use self::state::{ConsoleState, FocusPane, LayoutMode, Viewport};
use self::theme::ConsoleTheme;
use self::view_models::{build_view_model, render_overlay_body, BodyLineKind, BodySpan};

pub fn run_console(app: &App) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_console_loop(app, &mut terminal);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run_console_loop(app: &App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let mut state = ConsoleState::default();
    state.console_theme = app.config.console_theme;

    loop {
        let size = terminal.size()?;
        state.viewport = Viewport {
            width: size.width,
            height: size.height,
        };
        state.layout_mode = state.viewport.layout_mode();
        let theme = ConsoleTheme::from_name(state.console_theme);
        let vm = build_view_model(app, &state)?;
        terminal.draw(|frame| {
            let areas = layout_areas(frame.area(), vm.show_detail, vm.show_input);

            frame.render_widget(
                Paragraph::new(vm.header.clone())
                    .wrap(Wrap { trim: false })
                    .block(panel_block("Gold Band / Runtime Console", false, theme))
                    .style(theme.header_style()),
                areas[0],
            );
            frame.render_widget(
                Paragraph::new(render_body_content(vm.body_rich_lines.as_ref(), &vm.body_lines, &vm.body_line_kinds, theme))
                    .scroll((vm.body_scroll, 0))
                    .wrap(Wrap { trim: false })
                    .block(panel_block(&vm.body_title, matches!(state.focus, FocusPane::Welcome | FocusPane::TaskPicker | FocusPane::Dag | FocusPane::Detail) && !vm.show_overlay, theme))
                    .style(if vm.compact_detail_only { theme.detail_style() } else { theme.body_style() }),
                areas[1],
            );
            if vm.show_detail {
                frame.render_widget(
                    Paragraph::new(vm.detail_body.clone())
                        .scroll((vm.detail_scroll, 0))
                        .wrap(Wrap { trim: false })
                        .block(panel_block(&vm.detail_title, state.focus == FocusPane::Detail, theme))
                        .style(theme.detail_style()),
                    areas[2],
                );
            }
            let footer_area = areas[areas.len() - 1];
            if vm.show_input {
                let input_area = areas[areas.len() - 2];
                let input_text = if vm.input.is_empty() {
                    vm.input_hint.clone()
                } else {
                    format!("{}\n{}", vm.input, vm.input_hint)
                };
                frame.render_widget(
                    Paragraph::new(input_text)
                        .block(panel_block(&vm.input_title, state.focus == FocusPane::Input, theme))
                        .style(if vm.input.is_empty() { theme.input_placeholder_style() } else { theme.input_value_style() })
                        .wrap(Wrap { trim: false }),
                    input_area,
                );
            }
            frame.render_widget(Paragraph::new(vm.footer.clone()).style(theme.footer_style()), footer_area);

            if vm.show_overlay {
                let overlay_area = centered_rect(frame.area(), state.layout_mode);
                frame.render_widget(Clear, overlay_area);
                let title = vm.overlay_title.as_deref().unwrap_or("Overlay");
                frame.render_widget(
                    Paragraph::new(render_overlay_body(title, &vm.overlay_body, overlay_area.width).join("\n"))
                        .scroll((vm.overlay_scroll, 0))
                        .wrap(Wrap { trim: false })
                        .block(panel_block(title, state.focus == FocusPane::Overlay, theme))
                        .style(theme.overlay_style()),
                    overlay_area,
                );
            }
        })?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Esc => {
                        if escape(app, &mut state)? {
                            break;
                        }
                    }
                    KeyCode::Char('?') => show_help_overlay(app, &mut state)?,
                    KeyCode::Char('s') if state.focus != FocusPane::Input => start_selected_task(app, &mut state)?,
                    KeyCode::Char('l') if state.focus == FocusPane::Detail => toggle_log_source(&mut state),
                    KeyCode::Char('/') if state.focus != FocusPane::Input => start_command_input(&mut state),
                    KeyCode::Tab if state.layout_mode != LayoutMode::TooSmall => cycle_focus(&mut state),
                    KeyCode::Up if state.focus != FocusPane::Input => move_up(&mut state),
                    KeyCode::Down if state.focus != FocusPane::Input => move_down(&mut state),
                    KeyCode::Left if state.focus != FocusPane::Overlay => move_left(&mut state),
                    KeyCode::Right if state.focus != FocusPane::Overlay => move_right(&mut state),
                    KeyCode::Enter if state.layout_mode != LayoutMode::TooSmall => activate_current(app, &mut state)?,
                    KeyCode::Backspace if state.focus == FocusPane::Input => {
                        state.input.pop();
                        refresh_command_suggestions(&mut state);
                    }
                    KeyCode::Char(c) if state.focus == FocusPane::Input => {
                        state.input.push(c);
                        refresh_command_suggestions(&mut state);
                    }
                    _ => {}
                }
            }
        } else {
            refresh_tick(app, &mut state)?;
        }
    }

    Ok(())
}

fn layout_areas(area: Rect, show_detail: bool, show_input: bool) -> Vec<Rect> {
    match (show_detail, show_input) {
        (true, true) => Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4),
                Constraint::Min(8),
                Constraint::Length(10),
                Constraint::Length(4),
                Constraint::Length(1),
            ])
            .split(area)
            .to_vec(),
        (false, true) => Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4),
                Constraint::Min(18),
                Constraint::Length(4),
                Constraint::Length(1),
            ])
            .split(area)
            .to_vec(),
        (false, false) => Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(4), Constraint::Min(18), Constraint::Length(1)])
            .split(area)
            .to_vec(),
        (true, false) => Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4),
                Constraint::Min(8),
                Constraint::Length(10),
                Constraint::Length(1),
            ])
            .split(area)
            .to_vec(),
    }
}

fn centered_rect(area: Rect, layout_mode: LayoutMode) -> Rect {
    let horizontal = match layout_mode {
        LayoutMode::Compact => [Constraint::Percentage(8), Constraint::Percentage(84), Constraint::Percentage(8)],
        _ => [Constraint::Percentage(15), Constraint::Percentage(70), Constraint::Percentage(15)],
    };
    let vertical = match layout_mode {
        LayoutMode::Compact => [Constraint::Percentage(10), Constraint::Percentage(80), Constraint::Percentage(10)],
        _ => [Constraint::Percentage(12), Constraint::Percentage(76), Constraint::Percentage(12)],
    };
    let vertical_layout = Layout::default().direction(Direction::Vertical).constraints(vertical).split(area);
    let horizontal_layout = Layout::default().direction(Direction::Horizontal).constraints(horizontal).split(vertical_layout[1]);
    horizontal_layout[1]
}

fn panel_block(title: &str, focused: bool, theme: ConsoleTheme) -> Block<'static> {
    let title = title.to_string();
    let border_style = if focused { theme.focused_border_style() } else { theme.unfocused_border_style() };
    let title_style = if focused { theme.title_style() } else { theme.unfocused_border_style() };
    Block::default()
        .borders(Borders::ALL)
        .border_type(if focused { theme.focused_border_type() } else { theme.unfocused_border_type() })
        .border_style(border_style)
        .title(Span::styled(title, title_style))
}

fn render_body_content(
    rich_lines: Option<&Vec<Vec<BodySpan>>>,
    lines: &[String],
    kinds: &[BodyLineKind],
    theme: ConsoleTheme,
) -> Vec<Line<'static>> {
    if let Some(rich_lines) = rich_lines {
        return rich_lines
            .iter()
            .map(|line| {
                Line::from(
                    line.iter()
                        .map(|span| Span::styled(span.text.clone(), theme.span_style(span.role)))
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
    }

    lines.iter()
        .enumerate()
        .map(|(index, line)| Line::from(Span::styled(line.clone(), theme.line_style(kinds.get(index).copied().unwrap_or(BodyLineKind::Normal)))))
        .collect()
}
