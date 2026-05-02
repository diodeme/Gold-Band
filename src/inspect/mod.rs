use anyhow::Result;
use figlet_rs::FIGlet;

use crate::app::App;

pub fn render_run_status(summary: &str) -> String {
    summary.to_string()
}

pub fn render_console_banner() -> String {
    let rendered = FIGlet::slant()
        .or_else(|_| FIGlet::standard())
        .ok()
        .and_then(|font| font.convert("GOLD BAND").map(|figure| figure.to_string()));

    rendered
        .map(|banner| {
            banner
                .lines()
                .map(|line| line.trim_end().to_string())
                .filter(|line| !line.trim().is_empty())
                .map(|line| format!("  {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|banner| !banner.trim().is_empty())
        .unwrap_or_else(|| "  GOLD BAND".to_string())
}

pub fn render_console_help() -> String {
    [
        "Keyboard:",
        "  ?                open help",
        "  /                focus command bar",
        "  Tab              switch focus / pane",
        "  Enter            open selection",
        "  s                start selected task (Task Picker)",
        "  l                toggle log source (Attempt detail)",
        "  Esc              back / close overlay / quit from Welcome",
        "  Arrow keys       move selection; in attempt detail they scroll history",
        "",
        "Local commands:",
        "  /help",
        "  /task",
        "  /log",
        "  /config",
        "  /theme [gold-band|nord|dracula|cyber|onyx|mist|high-contrast]",
        "  /continue",
        "",
        "Runtime passthrough:",
        "  /run start <task-id>",
        "  /run status <task-id> <run-id>",
        "  /run continue <task-id> <run-id>",
        "  /run retry <task-id> <run-id>",
        "  /artifact show ...",
    ]
    .join("\n")
}

pub fn render_run_help() -> String {
    [
        "Run Commands",
        "  /run start <task-id> [--workflow <path>]",
        "  /run status <task-id> <run-id>",
        "  /run continue <task-id> <run-id>",
        "  /run retry <task-id> <run-id>",
        "  /run kill <task-id> <run-id>",
        "  /run open-session <task-id> <run-id> --round <round> --node <node> --attempt <attempt>",
    ]
    .join("\n")
}

pub fn render_artifact_help() -> String {
    [
        "Artifact Commands",
        "  /artifact list <task-id> <run-id> --round <round> --node <node> --attempt <attempt>",
        "  /artifact show <task-id> <run-id> --round <round> --node <node> --attempt <attempt> --name <name>",
        "",
        "In workspace mode, prefer Enter/Esc drill-down from node details.",
    ]
    .join("\n")
}

pub fn render_console_summary(app: &App) -> Result<String> {
    let task_count = app.task_list()?.len();
    Ok(format!(
        "Workspace: {}\nTasks discovered: {}\nMode: console bootstrap",
        app.paths.repo_root,
        task_count
    ))
}
