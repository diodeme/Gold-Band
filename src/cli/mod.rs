//! Headless automation entry point.
//!
//! `gold-band run` executes one AUTO or workflow conversation unattended in
//! the current Git repository: it creates the task through the same core
//! service as the desktop app, drives the run in the foreground until it
//! completes or pauses, prints the canonical `RunState` JSON to stdout and
//! maps the run lifecycle onto [`CliExitStatus`]. `gold-band continue` resumes
//! a paused run the same way and reports it with the same contract.

use std::collections::{BTreeMap, BTreeSet};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use clap::{Parser, Subcommand};

use crate::acp::session_config::doctor_authoring_capabilities;
use crate::app::{
    App, CONVERSATION_SOURCE_CLI, ConversationRunLaunch, CreateConversationTaskCommand,
};
use crate::config::{
    ConversationAutoConfig, ConversationRunMode, InteractionMode, ProviderDiagnosticSnapshot,
    RuntimeConfig, RuntimeLogLevel, StateConfig,
};
use crate::domain::{RunOutcome, RunStatus};
use crate::observability::{init_tracing, touch_log_file_best_effort};
use crate::runtime::RunState;
use crate::storage::{GoldBandPaths, load_settings_file, read_json};
use crate::workflow_model_binding::{
    TaskAuthoringWorkflow, TaskAuthoringWorkflowCompat, migrate_authoring_workflow,
};

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
    /// Run one AUTO or workflow conversation in the current Git repository.
    Run(RunArgs),
    /// Continue a paused run of the current Git repository in the foreground.
    Continue(ContinueArgs),
}

#[derive(Debug, clap::Args)]
struct ContinueArgs {
    /// `task_id` of the printed `RunState`.
    #[arg(long)]
    task_id: String,
    /// `id` of the printed `RunState`.
    #[arg(long)]
    run_id: String,
}

#[derive(Debug, clap::Args)]
struct RunArgs {
    /// Markdown file whose content is the conversation requirement.
    #[arg(long)]
    requirement_file: Utf8PathBuf,
    #[command(flatten)]
    mode: RunModeArgs,
}

#[derive(Debug, clap::Args)]
#[group(required = true, multiple = false)]
struct RunModeArgs {
    /// JSON file deserialized as the AUTO configuration (`ConversationAutoConfig`).
    #[arg(long)]
    auto_config: Option<Utf8PathBuf>,
    /// Workflow JSON in the desktop authoring format: `{ workflow, modelBindings }`
    /// or a bare workflow whose worker nodes carry their agent settings.
    #[arg(long)]
    workflow: Option<Utf8PathBuf>,
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

    let run = match cli.command {
        Commands::Run(args) => {
            let requirement = std::fs::read_to_string(&args.requirement_file)
                .with_context(|| format!("failed to read `{}`", args.requirement_file))?;
            match (&args.mode.auto_config, &args.mode.workflow) {
                (Some(path), _) => run_auto(&app, requirement, path)?,
                (_, Some(path)) => run_workflow(app, requirement, path)?,
                (None, None) => unreachable!("clap requires one run mode"),
            }
        }
        Commands::Continue(args) => app.run_continue_foreground(&args.task_id, &args.run_id)?,
    };
    println!("{}", serde_json::to_string_pretty(&run)?);
    Ok(CliExitStatus::from_run(&run))
}

fn run_auto(app: &App, requirement: String, auto_config: &Utf8Path) -> Result<RunState> {
    let auto_config: ConversationAutoConfig =
        read_json(auto_config).with_context(|| format!("failed to parse `{auto_config}`"))?;
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

fn run_workflow(app: App, requirement: String, workflow: &Utf8Path) -> Result<RunState> {
    let authoring = read_workflow_authoring(workflow)?;
    // Binding validation needs agent diagnostics, which the desktop caches.
    // Headless runs probe the bound agents once instead.
    let diagnostics = probe_agent_diagnostics(&app, &authoring)?;
    let app = app.with_provider_diagnostics_source(Arc::new(move || Ok(diagnostics.clone())));
    let mut command = CreateConversationTaskCommand::new(
        CONVERSATION_SOURCE_CLI,
        requirement,
        ConversationRunMode::Workflow,
    );
    command.workflow_authoring = Some(authoring);
    Ok(app
        .create_conversation_run(&command, ConversationRunLaunch::Foreground)?
        .run)
}

/// Reads a workflow in the desktop authoring format and materializes worker
/// agent settings into model bindings.
fn read_workflow_authoring(path: &Utf8Path) -> Result<TaskAuthoringWorkflow> {
    let compat: TaskAuthoringWorkflowCompat =
        read_json(path).with_context(|| format!("failed to parse `{path}`"))?;
    let (mut authoring, _) = compat.into_current();
    migrate_authoring_workflow(&mut authoring.workflow, &mut authoring.model_bindings, None)?;
    Ok(authoring)
}

fn probe_agent_diagnostics(
    app: &App,
    authoring: &TaskAuthoringWorkflow,
) -> Result<BTreeMap<String, ProviderDiagnosticSnapshot>> {
    let agent_ids = authoring
        .model_bindings
        .bindings
        .iter()
        .map(|binding| binding.agent_id.as_str())
        .collect::<BTreeSet<_>>();
    agent_ids
        .into_iter()
        .map(|agent_id| {
            let doctor = app.provider_doctor(agent_id)?;
            let capabilities = doctor
                .capabilities
                .map(doctor_authoring_capabilities);
            Ok((
                agent_id.to_string(),
                ProviderDiagnosticSnapshot {
                    available: doctor.available,
                    error: doctor.error,
                    checked_at: chrono::Utc::now().to_rfc3339(),
                    capabilities,
                },
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Cli, CliExitStatus, Commands, read_workflow_authoring};
    use crate::domain::{RunOutcome, RunStatus};
    use crate::dsl::NodeDsl;
    use camino::Utf8PathBuf;
    use clap::Parser;

    fn parse_run(mode_args: &[&str]) -> Result<Cli, clap::Error> {
        let mut argv = vec!["gold-band", "run", "--requirement-file", "task.md"];
        argv.extend_from_slice(mode_args);
        Cli::try_parse_from(argv)
    }

    #[test]
    fn run_command_parses_requirement_and_auto_config() {
        let Commands::Run(args) = parse_run(&["--auto-config", "auto.json"]).unwrap().command
        else {
            panic!("expected run");
        };
        assert_eq!(args.requirement_file, "task.md");
        assert_eq!(args.mode.auto_config.as_deref(), Some("auto.json".into()));
        assert!(args.mode.workflow.is_none());
    }

    #[test]
    fn run_command_parses_workflow() {
        let Commands::Run(args) = parse_run(&["--workflow", "flow.json"]).unwrap().command else {
            panic!("expected run");
        };
        assert_eq!(args.mode.workflow.as_deref(), Some("flow.json".into()));
        assert!(args.mode.auto_config.is_none());
    }

    #[test]
    fn run_command_requires_exactly_one_mode() {
        assert!(parse_run(&[]).is_err());
        assert!(parse_run(&["--auto-config", "auto.json", "--workflow", "flow.json"]).is_err());
    }

    #[test]
    fn continue_command_parses_run_identity() {
        let cli = Cli::try_parse_from([
            "gold-band",
            "continue",
            "--task-id",
            "task-001",
            "--run-id",
            "run-001",
        ])
        .unwrap();
        let Commands::Continue(args) = cli.command else {
            panic!("expected continue");
        };
        assert_eq!(
            (args.task_id.as_str(), args.run_id.as_str()),
            ("task-001", "run-001")
        );
    }

    #[test]
    fn continue_command_requires_run_identity() {
        assert!(Cli::try_parse_from(["gold-band", "continue", "--task-id", "task-001"]).is_err());
        assert!(Cli::try_parse_from(["gold-band", "continue", "--run-id", "run-001"]).is_err());
    }

    fn write_workflow(dir: &tempfile::TempDir, value: serde_json::Value) -> Utf8PathBuf {
        let path = Utf8PathBuf::from_path_buf(dir.path().join("flow.json")).unwrap();
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        path
    }

    fn worker_json(id: &str) -> serde_json::Value {
        serde_json::json!({
            "type": "worker", "id": id, "provider": "claude-acp", "profile": "pf-builtin-dev-test",
            "goal": "Implement it.", "output": null, "success_condition": null,
            "permission_mode": "bypassPermissions", "auto_accept": true,
            "config_options": { "effort": "max" }
        })
    }

    #[test]
    fn bare_workflow_moves_worker_agent_settings_into_bindings() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_workflow(
            &dir,
            serde_json::json!({
                "version": "0.1", "id": "flow", "entry": "dev-test",
                "nodes": [worker_json("dev-test")],
                "edges": [{ "from": "dev-test", "to": "$end", "on": "success", "session": null }]
            }),
        );

        let authoring = read_workflow_authoring(&path).unwrap();

        let NodeDsl::Worker(worker) = &authoring.workflow.nodes[0] else {
            panic!("expected worker");
        };
        let slot = worker.execution_slot_id.clone().expect("slot assigned");
        assert!(worker.provider.is_none() && worker.permission_mode.is_none());
        assert!(!worker.auto_accept && worker.config_options.is_empty());
        let [binding] = authoring.model_bindings.bindings.as_slice() else {
            panic!("expected one binding");
        };
        assert_eq!(binding.execution_slot_id, slot);
        assert_eq!(binding.agent_id, "claude-acp");
        assert_eq!(
            binding.permission_mode_id.as_deref(),
            Some("bypassPermissions")
        );
        assert!(binding.auto_accept);
        assert_eq!(
            binding.config_options.get("effort").map(String::as_str),
            Some("max")
        );
    }

    #[test]
    fn authoring_workflow_keeps_explicit_bindings() {
        let dir = tempfile::tempdir().unwrap();
        let mut worker = worker_json("dev-test");
        let object = worker.as_object_mut().unwrap();
        for key in [
            "provider",
            "permission_mode",
            "auto_accept",
            "config_options",
        ] {
            object.remove(key);
        }
        object.insert("executionSlotId".into(), "slot-dev".into());
        let path = write_workflow(
            &dir,
            serde_json::json!({
                "workflow": {
                    "version": "0.1", "id": "flow", "entry": "dev-test", "nodes": [worker],
                    "edges": [{ "from": "dev-test", "to": "$end", "on": "success", "session": null }]
                },
                "modelBindings": { "bindings": [
                    { "executionSlotId": "slot-dev", "agentId": "claude-acp", "autoAccept": true },
                    { "executionSlotId": "slot-removed", "agentId": "claude-acp" }
                ] }
            }),
        );

        let authoring = read_workflow_authoring(&path).unwrap();

        let slots = authoring
            .model_bindings
            .bindings
            .iter()
            .map(|binding| binding.execution_slot_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(slots, ["slot-dev"]);
        assert!(authoring.model_bindings.bindings[0].auto_accept);
    }

    #[test]
    fn unreadable_workflow_is_a_command_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_workflow(&dir, serde_json::json!({ "nodes": "invalid" }));
        let error = read_workflow_authoring(&path).unwrap_err();
        assert!(format!("{error:#}").contains("failed to parse"));
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
