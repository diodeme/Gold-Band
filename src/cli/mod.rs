use crate::app::App;
use crate::command::execute::execute_command;
use crate::command::{ArtifactCommand, Command, CommandResult, RunCommand, TaskCommand};
use crate::config::{
    ConsoleThemeName, RuntimeConfig, RuntimeLogLevel, SettingsConfig, StateConfig,
};
use crate::console::run_console;
use crate::observability::{init_tracing, touch_log_file_best_effort};
use crate::storage::{GoldBandPaths, read_json};
use anyhow::Result;
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "gold-band")]
#[command(about = "Gold Band CLI MVP")]
pub struct Cli {
    #[arg(long, default_value = "debug")]
    log_level: RuntimeLogLevel,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Task {
        #[command(subcommand)]
        command: TaskCommands,
    },
    Run {
        #[command(subcommand)]
        command: RunCommands,
    },
    Artifact {
        #[command(subcommand)]
        command: ArtifactCommands,
    },
    Console {
        #[arg(long)]
        theme: Option<ConsoleThemeName>,
    },
}

#[derive(Debug, Subcommand)]
enum TaskCommands {
    Show { task_id: String },
}

#[derive(Debug, Subcommand)]
enum RunCommands {
    Start {
        task_id: String,
        #[arg(long)]
        workflow: Option<Utf8PathBuf>,
    },
    Status {
        task_id: String,
        run_id: String,
    },
    Continue {
        task_id: String,
        run_id: String,
    },
    Retry {
        task_id: String,
        run_id: String,
    },
    OpenSession {
        task_id: String,
        run_id: String,
        #[arg(long)]
        round: String,
        #[arg(long)]
        node: String,
        #[arg(long)]
        attempt: String,
    },
}

#[derive(Debug, Subcommand)]
enum ArtifactCommands {
    List {
        task_id: String,
        run_id: String,
        #[arg(long)]
        round: String,
        #[arg(long)]
        node: String,
        #[arg(long)]
        attempt: String,
    },
    Show {
        task_id: String,
        run_id: String,
        #[arg(long)]
        round: String,
        #[arg(long)]
        node: String,
        #[arg(long)]
        attempt: String,
        #[arg(long)]
        name: String,
    },
    ShowPath {
        path: Utf8PathBuf,
    },
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?;
    let repo_root = Utf8PathBuf::from_path_buf(cwd)
        .map_err(|_| anyhow::anyhow!("working directory is not valid UTF-8"))?;
    let paths = GoldBandPaths::new(repo_root.clone());
    let settings: SettingsConfig = read_json(&paths.user_settings_file()).unwrap_or_default();
    let state: StateConfig = read_json(&paths.user_state_file()).unwrap_or_default();
    let enable_stderr_progress = !matches!(cli.command, Commands::Console { .. });
    let config = resolve_runtime_config(&cli, &settings, &state);
    let app = App::with_config(repo_root, config);
    init_tracing(&app.paths, &app.config, enable_stderr_progress);
    touch_log_file_best_effort(&app.paths);

    match cli.command {
        Commands::Console { .. } => run_console(&app),
        Commands::Task { command } => print_result(execute_command(
            &app,
            Command::Task(map_task_command(command)?),
        )?),
        Commands::Run { command } => print_result(execute_command(
            &app,
            Command::Run(map_run_command(command)?),
        )?),
        Commands::Artifact { command } => print_result(execute_command(
            &app,
            Command::Artifact(map_artifact_command(command)?),
        )?),
    }
}

fn resolve_runtime_config(
    cli: &Cli,
    settings: &SettingsConfig,
    state: &StateConfig,
) -> RuntimeConfig {
    let mut config = RuntimeConfig::default()
        .apply_settings(settings)
        .apply_state(state);
    config.log_level = cli.log_level;
    if let Commands::Console { theme: Some(theme) } = &cli.command {
        config.console_theme = *theme;
    }
    config
}

fn map_task_command(command: TaskCommands) -> Result<TaskCommand> {
    Ok(match command {
        TaskCommands::Show { task_id } => TaskCommand::Show { task_id },
    })
}

fn map_run_command(command: RunCommands) -> Result<RunCommand> {
    Ok(match command {
        RunCommands::Start { task_id, workflow } => RunCommand::Start { task_id, workflow },
        RunCommands::Status { task_id, run_id } => RunCommand::Status { task_id, run_id },
        RunCommands::Continue { task_id, run_id } => RunCommand::Continue { task_id, run_id },
        RunCommands::Retry { task_id, run_id } => RunCommand::Retry { task_id, run_id },
        RunCommands::OpenSession {
            task_id,
            run_id,
            round,
            node,
            attempt,
        } => RunCommand::OpenSession {
            task_id,
            run_id,
            round,
            node,
            attempt,
        },
    })
}

fn map_artifact_command(command: ArtifactCommands) -> Result<ArtifactCommand> {
    Ok(match command {
        ArtifactCommands::List {
            task_id,
            run_id,
            round,
            node,
            attempt,
        } => ArtifactCommand::List {
            task_id,
            run_id,
            round,
            node,
            attempt,
        },
        ArtifactCommands::Show {
            task_id,
            run_id,
            round,
            node,
            attempt,
            name,
        } => ArtifactCommand::Show {
            task_id,
            run_id,
            round,
            node,
            attempt,
            name,
        },
        ArtifactCommands::ShowPath { path } => ArtifactCommand::ShowPath { path },
    })
}

fn print_result(result: CommandResult) -> Result<()> {
    match result {
        CommandResult::Json(value) => println!("{}", serde_json::to_string_pretty(&value)?),
        CommandResult::Text(text) => println!("{text}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Cli, Commands, resolve_runtime_config};

    fn stderr_progress_enabled(cli: &Cli) -> bool {
        !matches!(cli.command, Commands::Console { .. })
    }
    use crate::config::{ConsoleThemeName, RuntimeLogLevel, SettingsConfig, StateConfig};
    use clap::Parser;

    #[test]
    fn console_disables_stderr_progress() {
        let cli = Cli::parse_from(["gold-band", "console"]);
        assert!(!stderr_progress_enabled(&cli));
    }

    #[test]
    fn non_console_commands_keep_stderr_progress() {
        let cli = Cli::parse_from(["gold-band", "run", "status", "task-001", "run-001"]);
        assert!(stderr_progress_enabled(&cli));
    }

    #[test]
    fn console_without_theme_uses_user_config_theme() {
        let cli = Cli::parse_from(["gold-band", "console"]);
        let config = resolve_runtime_config(
            &cli,
            &SettingsConfig {
                console_theme: Some(ConsoleThemeName::Nord),
                ..SettingsConfig::default()
            },
            &StateConfig::default(),
        );

        assert_eq!(config.console_theme, ConsoleThemeName::Nord);
        assert!(matches!(cli.command, Commands::Console { theme: None }));
    }

    #[test]
    fn console_theme_flag_overrides_user_config_theme() {
        let cli = Cli::parse_from(["gold-band", "console", "--theme", "cyber"]);
        let config = resolve_runtime_config(
            &cli,
            &SettingsConfig {
                console_theme: Some(ConsoleThemeName::Nord),
                ..SettingsConfig::default()
            },
            &StateConfig::default(),
        );

        assert_eq!(config.console_theme, ConsoleThemeName::Cyber);
        assert!(matches!(
            cli.command,
            Commands::Console {
                theme: Some(ConsoleThemeName::Cyber)
            }
        ));
        assert!(matches!(config.log_level, RuntimeLogLevel::Debug));
    }
}
