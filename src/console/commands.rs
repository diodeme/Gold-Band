use anyhow::{Result, anyhow, bail};

use crate::command::{ArtifactCommand, Command, RunCommand};
use crate::config::ConsoleThemeName;

const TOP_LEVEL_COMMANDS: &[&str] = &[
    "/help",
    "/task",
    "/log",
    "/config",
    "/theme",
    "/continue",
    "/run",
    "/artifact",
];
const RUN_SUBCOMMANDS: &[&str] = &[
    "start",
    "status",
    "continue",
    "retry",
    "kill",
    "open-session",
];
const ARTIFACT_SUBCOMMANDS: &[&str] = &["list", "show"];
const THEME_COMMANDS: &[&str] = &[
    "gold-band",
    "nord",
    "dracula",
    "cyber",
    "onyx",
    "mist",
    "high-contrast",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsoleLocalCommand {
    Help,
    Task,
    Log,
    Config,
    ThemeShow,
    ThemeSet(ConsoleThemeName),
    Continue,
}

#[derive(Debug, Clone)]
pub enum ParsedConsoleCommand {
    Runtime(Command),
    Local(ConsoleLocalCommand),
}

pub fn parse_console_command(input: &str) -> Result<ParsedConsoleCommand> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        bail!("empty command");
    }
    if !trimmed.starts_with('/') {
        bail!("console expects slash commands")
    }

    let parts = trimmed.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        ["/help"] => Ok(ParsedConsoleCommand::Local(ConsoleLocalCommand::Help)),
        ["/task"] => Ok(ParsedConsoleCommand::Local(ConsoleLocalCommand::Task)),
        ["/log"] => Ok(ParsedConsoleCommand::Local(ConsoleLocalCommand::Log)),
        ["/config"] => Ok(ParsedConsoleCommand::Local(ConsoleLocalCommand::Config)),
        ["/theme"] => Ok(ParsedConsoleCommand::Local(ConsoleLocalCommand::ThemeShow)),
        ["/theme", name] => Ok(ParsedConsoleCommand::Local(ConsoleLocalCommand::ThemeSet(
            name.parse()?,
        ))),
        ["/continue"] => Ok(ParsedConsoleCommand::Local(ConsoleLocalCommand::Continue)),
        ["/run", "start", task_id] => Ok(ParsedConsoleCommand::Runtime(Command::Run(
            RunCommand::Start {
                task_id: (*task_id).to_string(),
                workflow: None,
            },
        ))),
        ["/run", "status", task_id, run_id] => Ok(ParsedConsoleCommand::Runtime(Command::Run(
            RunCommand::Status {
                task_id: (*task_id).to_string(),
                run_id: (*run_id).to_string(),
            },
        ))),
        ["/run", "continue", task_id, run_id] => Ok(ParsedConsoleCommand::Runtime(Command::Run(
            RunCommand::Continue {
                task_id: (*task_id).to_string(),
                run_id: (*run_id).to_string(),
            },
        ))),
        ["/run", "retry", task_id, run_id] => Ok(ParsedConsoleCommand::Runtime(Command::Run(
            RunCommand::Retry {
                task_id: (*task_id).to_string(),
                run_id: (*run_id).to_string(),
            },
        ))),
        [
            "/run",
            "open-session",
            task_id,
            run_id,
            "--round",
            round,
            "--node",
            node,
            "--attempt",
            attempt,
        ] => Ok(ParsedConsoleCommand::Runtime(Command::Run(
            RunCommand::OpenSession {
                task_id: (*task_id).to_string(),
                run_id: (*run_id).to_string(),
                round: (*round).to_string(),
                node: (*node).to_string(),
                attempt: (*attempt).to_string(),
            },
        ))),
        [
            "/artifact",
            "list",
            task_id,
            run_id,
            "--round",
            round,
            "--node",
            node,
            "--attempt",
            attempt,
        ] => Ok(ParsedConsoleCommand::Runtime(Command::Artifact(
            ArtifactCommand::List {
                task_id: (*task_id).to_string(),
                run_id: (*run_id).to_string(),
                round: (*round).to_string(),
                node: (*node).to_string(),
                attempt: (*attempt).to_string(),
            },
        ))),
        [
            "/artifact",
            "show",
            task_id,
            run_id,
            "--round",
            round,
            "--node",
            node,
            "--attempt",
            attempt,
            "--name",
            name,
        ] => Ok(ParsedConsoleCommand::Runtime(Command::Artifact(
            ArtifactCommand::Show {
                task_id: (*task_id).to_string(),
                run_id: (*run_id).to_string(),
                round: (*round).to_string(),
                node: (*node).to_string(),
                attempt: (*attempt).to_string(),
                name: (*name).to_string(),
            },
        ))),
        ["/run", "--help"] | ["/artifact", "--help"] | ["/provider", "--help"] => {
            Ok(ParsedConsoleCommand::Local(ConsoleLocalCommand::Help))
        }
        _ => Err(anyhow!("unsupported console command: {trimmed}")),
    }
}

pub fn suggest_console_commands(input: &str) -> Vec<String> {
    let is_space_terminated = input.ends_with(' ');
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return TOP_LEVEL_COMMANDS
            .iter()
            .map(|item| (*item).to_string())
            .collect();
    }
    if !trimmed.starts_with('/') {
        return Vec::new();
    }

    let parts = trimmed.split_whitespace().collect::<Vec<_>>();
    if parts.len() <= 1 && !is_space_terminated {
        return TOP_LEVEL_COMMANDS
            .iter()
            .filter(|item| item.starts_with(trimmed))
            .map(|item| (*item).to_string())
            .collect();
    }

    match parts.first().copied() {
        Some("/run") => RUN_SUBCOMMANDS
            .iter()
            .filter(|item| {
                if is_space_terminated || parts.len() == 1 {
                    true
                } else {
                    item.starts_with(parts.get(1).copied().unwrap_or_default())
                }
            })
            .map(|item| format!("/run {item}"))
            .collect(),
        Some("/artifact") => ARTIFACT_SUBCOMMANDS
            .iter()
            .filter(|item| {
                if is_space_terminated || parts.len() == 1 {
                    true
                } else {
                    item.starts_with(parts.get(1).copied().unwrap_or_default())
                }
            })
            .map(|item| format!("/artifact {item}"))
            .collect(),
        Some("/theme") => THEME_COMMANDS
            .iter()
            .filter(|item| {
                if is_space_terminated || parts.len() == 1 {
                    true
                } else {
                    item.starts_with(parts.get(1).copied().unwrap_or_default())
                }
            })
            .map(|item| format!("/theme {item}"))
            .collect(),
        _ => Vec::new(),
    }
}
