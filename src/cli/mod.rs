//! Headless automation entry point.
//!
//! `gold-band run` executes one AUTO conversation unattended in the current
//! Git repository: it creates the task through the same core service as the
//! desktop app, drives the run in the foreground until it completes or
//! pauses, prints the canonical `RunState` JSON to stdout and maps the run
//! lifecycle onto [`CliExitStatus`].

use std::process::ExitCode;

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};

use crate::app::{
    App, CONVERSATION_SOURCE_CLI, ConversationRunLaunch, CreateConversationTaskCommand,
};
use crate::config::{
    ConversationAutoConfig, ConversationRunMode, InteractionMode, RuntimeConfig, RuntimeLogLevel,
    StateConfig,
};
use crate::domain::{RunOutcome, RunStatus};
use crate::observability::{init_tracing, touch_log_file_best_effort};
use crate::runtime::RunState;
use crate::storage::{GoldBandPaths, load_settings_file, read_json};

#[derive(Debug, Parser)]
#[command(name = "gold-band")]
#[command(about = "Gold Band headless runner")]
pub struct Cli {
    #[arg(long, default_value = "info", global = true)]
    log_level: RuntimeLogLevel,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run one AUTO conversation in the current Git repository.
    Run(RunArgs),
}

#[derive(Debug, clap::Args)]
struct RunArgs {
    /// Markdown file whose content is the conversation requirement.
    #[arg(long)]
    requirement_file: Utf8PathBuf,
    /// JSON file deserialized as the AUTO configuration (`ConversationAutoConfig`).
    #[arg(long)]
    auto_config: Utf8PathBuf,
}

/// Process exit status. Only the category is encoded here; the precise
/// status, outcome and pause reason are in the printed `RunState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliExitStatus {
    Success,
    RunFailed,
    RunPaused,
    CommandFailed,
    RunNotSettled,
}

impl CliExitStatus {
    pub const fn code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::RunFailed => 1,
            Self::RunPaused => 2,
            Self::CommandFailed => 3,
            Self::RunNotSettled => 4,
        }
    }

    pub fn from_run(run: &RunState) -> Self {
        Self::from_lifecycle(run.status, run.outcome)
    }

    pub fn from_lifecycle(status: RunStatus, outcome: Option<RunOutcome>) -> Self {
        match (status, outcome) {
            (RunStatus::Completed, Some(RunOutcome::Success)) => Self::Success,
            (RunStatus::Completed, _) => Self::RunFailed,
            (RunStatus::Paused, _) => Self::RunPaused,
            (RunStatus::Running, _) => Self::RunNotSettled,
        }
    }
}

impl From<CliExitStatus> for ExitCode {
    fn from(status: CliExitStatus) -> Self {
        ExitCode::from(status.code())
    }
}

pub async fn run() -> ExitCode {
    let cli = Cli::parse();
    match execute(cli) {
        Ok(status) => status.into(),
        Err(error) => {
            eprintln!("{error:#}");
            CliExitStatus::CommandFailed.into()
        }
    }
}

fn execute(cli: Cli) -> Result<CliExitStatus> {
    let cwd = std::env::current_dir()?;
    let repo_root = Utf8PathBuf::from_path_buf(cwd)
        .map_err(|_| anyhow::anyhow!("working directory is not valid UTF-8"))?;
    let paths = GoldBandPaths::new(repo_root.clone());
    let settings = load_settings_file(&paths.user_settings_file()).unwrap_or_default();
    let state: StateConfig = read_json(&paths.user_state_file()).unwrap_or_default();
    let mut config = RuntimeConfig::default()
        .apply_settings(&settings)
        .apply_state(&state);
    config.log_level = cli.log_level;
    // The headless runner has no responder for prompt interactions.
    config.interaction_mode = InteractionMode::Unattended;
    let app = App::with_config(repo_root, config);
    let _runtime_log_guard = init_tracing(&app.paths, &app.config, true);
    touch_log_file_best_effort(&app.paths);

    match cli.command {
        Commands::Run(args) => {
            let run = run_auto(&app, &args)?;
            println!("{}", serde_json::to_string_pretty(&run)?);
            Ok(CliExitStatus::from_run(&run))
        }
    }
}

fn run_auto(app: &App, args: &RunArgs) -> Result<RunState> {
    let requirement = std::fs::read_to_string(&args.requirement_file)
        .with_context(|| format!("failed to read `{}`", args.requirement_file))?;
    let auto_config: ConversationAutoConfig = read_json(&args.auto_config)
        .with_context(|| format!("failed to parse `{}`", args.auto_config))?;
    let mut command = CreateConversationTaskCommand::new(
        CONVERSATION_SOURCE_CLI,
        requirement,
        ConversationRunMode::Auto,
    );
    command.auto_config = Some(auto_config);
    Ok(app
        .create_conversation_run(&command, ConversationRunLaunch::Foreground)?
        .run)
}

#[cfg(test)]
mod tests {
    use super::{Cli, CliExitStatus, Commands};
    use crate::domain::{RunOutcome, RunStatus};
    use clap::Parser;

    #[test]
    fn run_command_parses_requirement_and_auto_config() {
        let cli = Cli::parse_from([
            "gold-band",
            "run",
            "--requirement-file",
            "task.md",
            "--auto-config",
            "auto.json",
        ]);
        let Commands::Run(args) = cli.command;
        assert_eq!(args.requirement_file, "task.md");
        assert_eq!(args.auto_config, "auto.json");
    }

    #[test]
    fn run_command_requires_auto_config() {
        assert!(
            Cli::try_parse_from(["gold-band", "run", "--requirement-file", "task.md"]).is_err()
        );
    }

    #[test]
    fn exit_status_follows_run_lifecycle() {
        let cases = [
            (
                RunStatus::Completed,
                Some(RunOutcome::Success),
                CliExitStatus::Success,
                0,
            ),
            (
                RunStatus::Completed,
                Some(RunOutcome::Failure),
                CliExitStatus::RunFailed,
                1,
            ),
            (
                RunStatus::Completed,
                Some(RunOutcome::Killed),
                CliExitStatus::RunFailed,
                1,
            ),
            (RunStatus::Paused, None, CliExitStatus::RunPaused, 2),
            (RunStatus::Running, None, CliExitStatus::RunNotSettled, 4),
        ];
        for (status, outcome, expected, code) in cases {
            let actual = CliExitStatus::from_lifecycle(status, outcome);
            assert_eq!(actual, expected);
            assert_eq!(actual.code(), code);
        }
        assert_eq!(CliExitStatus::CommandFailed.code(), 3);
    }
}
