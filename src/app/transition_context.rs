use anyhow::Result;
use camino::Utf8PathBuf;

use crate::domain::SessionMode;
use crate::runtime::NodeState;

use super::App;
use super::ids::latest_attempt_id;

pub(crate) fn find_latest_worker_ref_for_transition(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    _previous_node: &NodeState,
    target_node_id: &str,
    session_mode: SessionMode,
) -> Result<Option<Utf8PathBuf>> {
    if session_mode != SessionMode::Continue {
        return Ok(None);
    }
    let Some(attempt_id) =
        latest_attempt_id(
            &app.paths
                .node_dir(task_id, run_id, round_id, target_node_id),
        )?
    else {
        return Ok(None);
    };
    let path = app
        .paths
        .worker_ref_file(task_id, run_id, round_id, target_node_id, &attempt_id);
    if path.exists() {
        Ok(Some(path))
    } else {
        Ok(None)
    }
}
