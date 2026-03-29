use crate::app::App;
use anyhow::Result;
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "gold-band")]
#[command(about = "Gold Band CLI MVP")]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Task { #[command(subcommand)] command: TaskCommands },
    Run { #[command(subcommand)] command: RunCommands },
    Artifact { #[command(subcommand)] command: ArtifactCommands },
}

#[derive(Debug, Subcommand)]
enum TaskCommands {
    Show { task_id: String },
}

#[derive(Debug, Subcommand)]
enum RunCommands {
    Start { task_id: String, #[arg(long)] workflow: Option<Utf8PathBuf> },
    Status { task_id: String, run_id: String },
    Continue { task_id: String, run_id: String },
    Retry { task_id: String, run_id: String },
    Kill { task_id: String, run_id: String },
    OpenSession { task_id: String, run_id: String, #[arg(long)] round: String, #[arg(long)] node: String, #[arg(long)] attempt: String },
}

#[derive(Debug, Subcommand)]
enum ArtifactCommands {
    List { task_id: String, run_id: String, #[arg(long)] round: String, #[arg(long)] node: String, #[arg(long)] attempt: String },
    Show { task_id: String, run_id: String, #[arg(long)] round: String, #[arg(long)] node: String, #[arg(long)] attempt: String, #[arg(long)] name: String },
    ShowPath { path: Utf8PathBuf },
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?;
    let repo_root = Utf8PathBuf::from_path_buf(cwd).map_err(|_| anyhow::anyhow!("working directory is not valid UTF-8"))?;
    let app = App::new(repo_root);

    match cli.command {
        Commands::Task { command } => match command {
            TaskCommands::Show { task_id } => {
                let task = app.task_show(&task_id)?;
                println!("{}", serde_json::to_string_pretty(&task)?);
            }
        },
        Commands::Run { command } => match command {
            RunCommands::Start { task_id, workflow } => {
                let run = app.run_start(&task_id, workflow.as_deref())?;
                println!("{}", serde_json::to_string_pretty(&run)?);
            }
            RunCommands::Status { task_id, run_id } => {
                let run = app.run_status(&task_id, &run_id)?;
                println!("{}", serde_json::to_string_pretty(&run)?);
            }
            RunCommands::Continue { task_id, run_id } => {
                let run = app.run_continue(&task_id, &run_id)?;
                println!("{}", serde_json::to_string_pretty(&run)?);
            }
            RunCommands::Retry { task_id, run_id } => {
                let run = app.run_retry(&task_id, &run_id)?;
                println!("{}", serde_json::to_string_pretty(&run)?);
            }
            RunCommands::Kill { task_id, run_id } => {
                let run = app.run_kill(&task_id, &run_id)?;
                println!("{}", serde_json::to_string_pretty(&run)?);
            }
            RunCommands::OpenSession { task_id, run_id, round, node, attempt } => {
                let command = app.run_open_session(&task_id, &run_id, &round, &node, &attempt)?;
                println!("{command}");
            }
        },
        Commands::Artifact { command } => match command {
            ArtifactCommands::List { task_id, run_id, round, node, attempt } => {
                let artifacts = app.artifact_list(&task_id, &run_id, &round, &node, &attempt)?;
                println!("{}", serde_json::to_string_pretty(&artifacts)?);
            }
            ArtifactCommands::Show { task_id, run_id, round, node, attempt, name } => {
                let artifact = app.artifact_show(&task_id, &run_id, &round, &node, &attempt, &name)?;
                println!("{artifact}");
            }
            ArtifactCommands::ShowPath { path } => {
                let artifact = app.artifact_show_path(&path)?;
                println!("{artifact}");
            }
        },
    }

    Ok(())
}
