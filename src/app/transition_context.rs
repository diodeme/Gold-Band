use anyhow::Result;
use camino::Utf8PathBuf;

use super::App;
use crate::domain::SessionMode;

pub(crate) fn worker_ref_for_transition(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    target_node_id: &str,
    committed_attempt_id: Option<&str>,
    session_mode: SessionMode,
) -> Result<Option<Utf8PathBuf>> {
    if session_mode != SessionMode::Continue {
        return Ok(None);
    }
    let Some(attempt_id) = committed_attempt_id else {
        return Ok(None);
    };
    let path = app
        .paths
        .worker_ref_file(task_id, run_id, round_id, target_node_id, attempt_id);
    if path.exists() {
        Ok(Some(path))
    } else {
        Ok(None)
    }
}
