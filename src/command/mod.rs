pub mod execute;

use camino::Utf8PathBuf;

#[derive(Debug, Clone)]
pub enum Command {
    Task(TaskCommand),
    Run(RunCommand),
    Artifact(ArtifactCommand),
}

#[derive(Debug, Clone)]
pub enum TaskCommand {
    Show { task_id: String },
}

#[derive(Debug, Clone)]
pub enum RunCommand {
    Start {
        task_id: String,
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
        round: String,
        node: String,
        attempt: String,
    },
}

#[derive(Debug, Clone)]
pub enum ArtifactCommand {
    List {
        task_id: String,
        run_id: String,
        round: String,
        node: String,
        attempt: String,
    },
    Show {
        task_id: String,
        run_id: String,
        round: String,
        node: String,
        attempt: String,
        name: String,
    },
    ShowPath {
        path: Utf8PathBuf,
    },
}

#[derive(Debug, Clone)]
pub enum CommandResult {
    Json(serde_json::Value),
    Text(String),
}
