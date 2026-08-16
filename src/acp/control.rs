use std::hash::{Hash, Hasher};
use std::sync::{LazyLock, Mutex};

use anyhow::{Result, anyhow};
use camino::Utf8Path;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::acp::events::{current_timestamp, load_session_metadata_value, load_timeline_items};
use crate::domain::{TurnControlMode, TurnControlTransitionCause};
use crate::storage::{ensure_parent_dir, write_json};

const SNAPSHOT_FILE: &str = "acp.snapshot.json";
const SESSION_FILE: &str = "acp.session.json";
const TIMELINE_FILE: &str = "acp.timeline.jsonl";
const TIMELINE_SCAN_COMPLETE_FIELD: &str = "runtimeControlTimelineScanComplete";

const CURSOR_LOCK_STRIPES: usize = 64;
static CURSOR_LOCKS: LazyLock<[Mutex<()>; CURSOR_LOCK_STRIPES]> =
    LazyLock::new(|| std::array::from_fn(|_| Mutex::new(())));

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpRuntimeControlCursor {
    pub current_mode: TurnControlMode,
    pub transition_id: String,
    pub transition_cause: TurnControlTransitionCause,
    pub changed_at: String,
}

fn cursor_lock(attempt_dir: &Utf8Path) -> &'static Mutex<()> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    attempt_dir.hash(&mut hasher);
    &CURSOR_LOCKS[(hasher.finish() as usize) % CURSOR_LOCK_STRIPES]
}

pub fn mark_runtime_interrupted(attempt_dir: &Utf8Path) -> Result<AcpRuntimeControlCursor> {
    let lock = cursor_lock(attempt_dir);
    let _guard = lock
        .lock()
        .map_err(|_| anyhow!("runtime control cursor lock poisoned"))?;
    if let Some(cursor) = load_persisted_cursor_unlocked(attempt_dir)?.0
        && cursor.current_mode == TurnControlMode::NonRuntimeControlled
    {
        return Ok(cursor);
    }
    write_transition_unlocked(
        attempt_dir,
        TurnControlMode::NonRuntimeControlled,
        TurnControlTransitionCause::RuntimeInterrupted,
    )
}

pub fn prepare_manual_follow_up(
    attempt_dir: &Utf8Path,
) -> Result<Option<(Option<String>, String)>> {
    let lock = cursor_lock(attempt_dir);
    let _guard = lock
        .lock()
        .map_err(|_| anyhow!("runtime control cursor lock poisoned"))?;
    let cursor = load_runtime_control_cursor_unlocked(attempt_dir)?;
    if cursor
        .as_ref()
        .is_some_and(|cursor| cursor.current_mode == TurnControlMode::NonRuntimeControlled)
    {
        return Ok(None);
    }
    Ok(Some((
        cursor.map(|cursor| cursor.transition_id),
        format!("runtime-control-{}", Uuid::new_v4().simple()),
    )))
}

pub fn commit_manual_follow_up(
    attempt_dir: &Utf8Path,
    source_transition_id: Option<&str>,
    transition_id: &str,
) -> Result<bool> {
    let lock = cursor_lock(attempt_dir);
    let _guard = lock
        .lock()
        .map_err(|_| anyhow!("runtime control cursor lock poisoned"))?;
    let cursor = load_runtime_control_cursor_unlocked(attempt_dir)?;
    let source_matches = match (source_transition_id, cursor.as_ref()) {
        (None, None) => true,
        (Some(source_transition_id), Some(cursor)) => {
            cursor.current_mode == TurnControlMode::RuntimeControlled
                && cursor.transition_id == source_transition_id
        }
        _ => false,
    };
    if !source_matches {
        return Ok(false);
    }
    persist_cursor_unlocked(
        attempt_dir,
        &AcpRuntimeControlCursor {
            current_mode: TurnControlMode::NonRuntimeControlled,
            transition_id: transition_id.to_string(),
            transition_cause: TurnControlTransitionCause::ManualFollowUp,
            changed_at: current_timestamp(),
        },
    )?;
    Ok(true)
}

pub fn prepare_workflow_continued(attempt_dir: &Utf8Path) -> Result<Option<(String, String)>> {
    let lock = cursor_lock(attempt_dir);
    let _guard = lock
        .lock()
        .map_err(|_| anyhow!("runtime control cursor lock poisoned"))?;
    let Some(cursor) = load_runtime_control_cursor_unlocked(attempt_dir)? else {
        return Ok(None);
    };
    if cursor.current_mode != TurnControlMode::NonRuntimeControlled {
        return Ok(None);
    }
    Ok(Some((
        cursor.transition_id,
        format!("runtime-control-{}", Uuid::new_v4().simple()),
    )))
}

pub fn commit_workflow_continued(
    attempt_dir: &Utf8Path,
    source_transition_id: &str,
    transition_id: &str,
) -> Result<bool> {
    let lock = cursor_lock(attempt_dir);
    let _guard = lock
        .lock()
        .map_err(|_| anyhow!("runtime control cursor lock poisoned"))?;
    let Some(cursor) = load_runtime_control_cursor_unlocked(attempt_dir)? else {
        return Ok(false);
    };
    if cursor.current_mode != TurnControlMode::NonRuntimeControlled
        || cursor.transition_id != source_transition_id
    {
        return Ok(false);
    }
    persist_cursor_unlocked(
        attempt_dir,
        &AcpRuntimeControlCursor {
            current_mode: TurnControlMode::RuntimeControlled,
            transition_id: transition_id.to_string(),
            transition_cause: TurnControlTransitionCause::WorkflowContinued,
            changed_at: current_timestamp(),
        },
    )?;
    Ok(true)
}

pub fn load_runtime_control_cursor(
    attempt_dir: &Utf8Path,
) -> Result<Option<AcpRuntimeControlCursor>> {
    let lock = cursor_lock(attempt_dir);
    let _guard = lock
        .lock()
        .map_err(|_| anyhow!("runtime control cursor lock poisoned"))?;
    load_runtime_control_cursor_unlocked(attempt_dir)
}

fn load_runtime_control_cursor_unlocked(
    attempt_dir: &Utf8Path,
) -> Result<Option<AcpRuntimeControlCursor>> {
    let (cursor, timeline_scan_complete) = load_persisted_cursor_unlocked(attempt_dir)?;
    if cursor.is_some() {
        return Ok(cursor);
    }
    if timeline_scan_complete {
        return Ok(None);
    }
    reconstruct_cursor_from_timeline_unlocked(attempt_dir)
}

fn load_persisted_cursor_unlocked(
    attempt_dir: &Utf8Path,
) -> Result<(Option<AcpRuntimeControlCursor>, bool)> {
    let mut candidates: Vec<AcpRuntimeControlCursor> = Vec::with_capacity(2);
    let mut timeline_scan_complete = false;
    for name in [SNAPSHOT_FILE, SESSION_FILE] {
        let path = attempt_dir.join(name);
        let Ok(value) = load_session_metadata_value(&path, None) else {
            continue;
        };
        timeline_scan_complete |= value
            .get(TIMELINE_SCAN_COMPLETE_FIELD)
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if let Some(cursor) = value
            .get("runtimeControl")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
        {
            candidates.push(cursor);
        }
    }
    candidates.sort_by(|left, right| left.changed_at.cmp(&right.changed_at));
    Ok((candidates.pop(), timeline_scan_complete))
}

fn write_transition_unlocked(
    attempt_dir: &Utf8Path,
    current_mode: TurnControlMode,
    transition_cause: TurnControlTransitionCause,
) -> Result<AcpRuntimeControlCursor> {
    let cursor = AcpRuntimeControlCursor {
        current_mode,
        transition_id: format!("runtime-control-{}", Uuid::new_v4().simple()),
        transition_cause,
        changed_at: current_timestamp(),
    };
    persist_cursor_unlocked(attempt_dir, &cursor)?;
    Ok(cursor)
}

fn persist_cursor_unlocked(attempt_dir: &Utf8Path, cursor: &AcpRuntimeControlCursor) -> Result<()> {
    for name in [SNAPSHOT_FILE, SESSION_FILE] {
        let path = attempt_dir.join(name);
        let mut session = session_value(&path)?;
        session["runtimeControl"] = serde_json::to_value(cursor)?;
        session[TIMELINE_SCAN_COMPLETE_FIELD] = Value::Bool(true);
        ensure_parent_dir(&path)?;
        write_json(&path, &session)?;
    }
    Ok(())
}

fn reconstruct_cursor_from_timeline_unlocked(
    attempt_dir: &Utf8Path,
) -> Result<Option<AcpRuntimeControlCursor>> {
    let timeline_path = attempt_dir.join(TIMELINE_FILE);
    let cursor = if timeline_path.exists() {
        load_timeline_items(&timeline_path)?
            .into_iter()
            .rev()
            .find_map(|event| {
                event
                    .raw
                    .as_ref()
                    .and_then(|raw| raw.get("runtimeControl"))
                    .cloned()
                    .and_then(|value| serde_json::from_value(value).ok())
            })
    } else {
        None
    };
    if let Some(cursor) = cursor.as_ref() {
        persist_cursor_unlocked(attempt_dir, cursor)?;
    } else {
        persist_timeline_scan_complete_unlocked(attempt_dir)?;
    }
    Ok(cursor)
}

fn persist_timeline_scan_complete_unlocked(attempt_dir: &Utf8Path) -> Result<()> {
    for name in [SNAPSHOT_FILE, SESSION_FILE] {
        let path = attempt_dir.join(name);
        let mut session = session_value(&path)?;
        session[TIMELINE_SCAN_COMPLETE_FIELD] = Value::Bool(true);
        ensure_parent_dir(&path)?;
        write_json(&path, &session)?;
    }
    Ok(())
}

fn session_value(path: &Utf8Path) -> Result<Value> {
    if path.exists() {
        return load_session_metadata_value(path, None);
    }
    Ok(serde_json::json!({
        "availability": "established",
        "latestTurnStatus": "none",
        "restored": false,
        "createdAt": current_timestamp(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::events::{AcpUiEvent, write_timeline_items};

    #[test]
    fn runtime_control_cursor_does_not_invent_a_terminal_turn_status() {
        let dir = tempfile::tempdir().unwrap();
        let attempt_dir = Utf8Path::from_path(dir.path()).unwrap();

        mark_runtime_interrupted(attempt_dir).unwrap();

        for name in [SNAPSHOT_FILE, SESSION_FILE] {
            let metadata = load_session_metadata_value(&attempt_dir.join(name), None).unwrap();
            assert_eq!(metadata["latestTurnStatus"], "none");
            assert_eq!(
                metadata["runtimeControl"]["currentMode"],
                "non-runtime-controlled"
            );
        }
    }

    #[test]
    fn runtime_control_cursor_preserves_an_existing_turn_status() {
        let dir = tempfile::tempdir().unwrap();
        let attempt_dir = Utf8Path::from_path(dir.path()).unwrap();
        write_json(
            &attempt_dir.join(SNAPSHOT_FILE),
            &serde_json::json!({
                "sessionId": "session-existing",
                "availability": "established",
                "latestTurnStatus": "completed",
                "restored": false,
                "createdAt": current_timestamp(),
            }),
        )
        .unwrap();

        mark_runtime_interrupted(attempt_dir).unwrap();

        let metadata = load_session_metadata_value(&attempt_dir.join(SNAPSHOT_FILE), None).unwrap();
        assert_eq!(metadata["latestTurnStatus"], "completed");
        assert_eq!(metadata["sessionId"], "session-existing");
    }

    #[test]
    fn non_runtime_stop_does_not_create_another_transition() {
        let dir = tempfile::tempdir().unwrap();
        let attempt_dir = Utf8Path::from_path(dir.path()).unwrap();
        let first = mark_runtime_interrupted(attempt_dir).unwrap();
        let repeated = mark_runtime_interrupted(attempt_dir).unwrap();
        assert_eq!(repeated.transition_id, first.transition_id);
        assert_eq!(
            load_runtime_control_cursor(attempt_dir)
                .unwrap()
                .unwrap()
                .transition_id,
            first.transition_id
        );
    }

    #[test]
    fn accepted_manual_follow_up_persists_non_runtime_control_to_both_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let attempt_dir = Utf8Path::from_path(dir.path()).unwrap();
        let (source_id, transition_id) = prepare_manual_follow_up(attempt_dir).unwrap().unwrap();

        assert!(source_id.is_none());
        assert!(load_runtime_control_cursor(attempt_dir).unwrap().is_none());
        assert!(
            commit_manual_follow_up(attempt_dir, source_id.as_deref(), &transition_id).unwrap()
        );

        for name in [SNAPSHOT_FILE, SESSION_FILE] {
            let metadata = load_session_metadata_value(&attempt_dir.join(name), None).unwrap();
            assert_eq!(
                metadata["runtimeControl"]["currentMode"],
                "non-runtime-controlled"
            );
            assert_eq!(
                metadata["runtimeControl"]["transitionCause"],
                "manual-follow-up"
            );
            assert_eq!(metadata["runtimeControl"]["transitionId"], transition_id);
        }
    }

    #[test]
    fn repeated_manual_follow_up_keeps_existing_non_runtime_transition() {
        let dir = tempfile::tempdir().unwrap();
        let attempt_dir = Utf8Path::from_path(dir.path()).unwrap();
        let (_, transition_id) = prepare_manual_follow_up(attempt_dir).unwrap().unwrap();
        assert!(commit_manual_follow_up(attempt_dir, None, &transition_id).unwrap());

        assert!(prepare_manual_follow_up(attempt_dir).unwrap().is_none());
        assert_eq!(
            load_runtime_control_cursor(attempt_dir)
                .unwrap()
                .unwrap()
                .transition_id,
            transition_id
        );
    }

    #[test]
    fn stale_manual_follow_up_cannot_overwrite_a_new_control_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let attempt_dir = Utf8Path::from_path(dir.path()).unwrap();
        let (_, stale_transition_id) = prepare_manual_follow_up(attempt_dir).unwrap().unwrap();
        let interrupted = mark_runtime_interrupted(attempt_dir).unwrap();

        assert!(!commit_manual_follow_up(attempt_dir, None, &stale_transition_id).unwrap());
        assert_eq!(
            load_runtime_control_cursor(attempt_dir)
                .unwrap()
                .unwrap()
                .transition_id,
            interrupted.transition_id
        );
    }

    #[test]
    fn resumed_runtime_commits_only_after_acceptance() {
        let dir = tempfile::tempdir().unwrap();
        let attempt_dir = Utf8Path::from_path(dir.path()).unwrap();
        let first = mark_runtime_interrupted(attempt_dir).unwrap();
        let (source_id, resumed_id) = prepare_workflow_continued(attempt_dir).unwrap().unwrap();
        assert_eq!(source_id, first.transition_id);
        assert_eq!(
            load_runtime_control_cursor(attempt_dir)
                .unwrap()
                .unwrap()
                .current_mode,
            TurnControlMode::NonRuntimeControlled
        );

        assert!(commit_workflow_continued(attempt_dir, &source_id, &resumed_id).unwrap());
        let resumed = load_runtime_control_cursor(attempt_dir).unwrap().unwrap();
        assert_eq!(resumed.current_mode, TurnControlMode::RuntimeControlled);
        assert_eq!(resumed.transition_id, resumed_id);
    }

    #[test]
    fn manual_follow_up_after_resume_can_return_to_runtime_control() {
        let dir = tempfile::tempdir().unwrap();
        let attempt_dir = Utf8Path::from_path(dir.path()).unwrap();
        let interrupted = mark_runtime_interrupted(attempt_dir).unwrap();
        let (interrupted_id, resumed_id) =
            prepare_workflow_continued(attempt_dir).unwrap().unwrap();
        assert_eq!(interrupted_id, interrupted.transition_id);
        assert!(commit_workflow_continued(attempt_dir, &interrupted_id, &resumed_id).unwrap());

        let (source_id, manual_id) = prepare_manual_follow_up(attempt_dir).unwrap().unwrap();
        assert_eq!(source_id.as_deref(), Some(resumed_id.as_str()));
        assert!(commit_manual_follow_up(attempt_dir, source_id.as_deref(), &manual_id).unwrap());
        let (manual_source_id, resumed_again_id) =
            prepare_workflow_continued(attempt_dir).unwrap().unwrap();
        assert_eq!(manual_source_id, manual_id);
        assert!(
            commit_workflow_continued(attempt_dir, &manual_source_id, &resumed_again_id).unwrap()
        );

        let cursor = load_runtime_control_cursor(attempt_dir).unwrap().unwrap();
        assert_eq!(cursor.current_mode, TurnControlMode::RuntimeControlled);
        assert_eq!(cursor.transition_id, resumed_again_id);
    }

    #[test]
    fn stale_resume_cannot_overwrite_a_new_stop_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let attempt_dir = Utf8Path::from_path(dir.path()).unwrap();
        mark_runtime_interrupted(attempt_dir).unwrap();
        let (source_id, stale_resumed_id) =
            prepare_workflow_continued(attempt_dir).unwrap().unwrap();
        let (_, accepted_resumed_id) = prepare_workflow_continued(attempt_dir).unwrap().unwrap();
        assert!(commit_workflow_continued(attempt_dir, &source_id, &accepted_resumed_id).unwrap());
        let newer_stop = mark_runtime_interrupted(attempt_dir).unwrap();

        assert!(!commit_workflow_continued(attempt_dir, &source_id, &stale_resumed_id).unwrap());
        assert_eq!(
            load_runtime_control_cursor(attempt_dir)
                .unwrap()
                .unwrap()
                .transition_id,
            newer_stop.transition_id
        );
    }

    #[test]
    fn missing_legacy_cursor_scans_timeline_only_once() {
        let dir = tempfile::tempdir().unwrap();
        let attempt_dir = Utf8Path::from_path(dir.path()).unwrap();
        assert!(load_runtime_control_cursor(attempt_dir).unwrap().is_none());

        let cursor = AcpRuntimeControlCursor {
            current_mode: TurnControlMode::NonRuntimeControlled,
            transition_id: "late-timeline-transition".to_string(),
            transition_cause: TurnControlTransitionCause::RuntimeInterrupted,
            changed_at: current_timestamp(),
        };
        write_timeline_items(
            &attempt_dir.join(TIMELINE_FILE),
            &[AcpUiEvent {
                id: "late-event".to_string(),
                seq: 1,
                timestamp: current_timestamp(),
                kind: "userPrompt".to_string(),
                session_id: None,
                content: Some("late".to_string()),
                title: None,
                tool_call_id: None,
                status: Some("completed".to_string()),
                started_seq: None,
                ended_seq: None,
                started_at: None,
                ended_at: None,
                timing: None,
                raw: Some(serde_json::json!({ "runtimeControl": cursor })),
            }],
        )
        .unwrap();

        assert!(load_runtime_control_cursor(attempt_dir).unwrap().is_none());
    }

    #[test]
    fn stop_transition_never_reconstructs_legacy_timeline() {
        let dir = tempfile::tempdir().unwrap();
        let attempt_dir = Utf8Path::from_path(dir.path()).unwrap();
        let legacy_cursor = AcpRuntimeControlCursor {
            current_mode: TurnControlMode::NonRuntimeControlled,
            transition_id: "legacy-timeline-transition".to_string(),
            transition_cause: TurnControlTransitionCause::RuntimeInterrupted,
            changed_at: current_timestamp(),
        };
        write_timeline_items(
            &attempt_dir.join(TIMELINE_FILE),
            &[AcpUiEvent {
                id: "legacy-event".to_string(),
                seq: 1,
                timestamp: current_timestamp(),
                kind: "userPrompt".to_string(),
                session_id: None,
                content: Some("legacy".to_string()),
                title: None,
                tool_call_id: None,
                status: Some("completed".to_string()),
                started_seq: None,
                ended_seq: None,
                started_at: None,
                ended_at: None,
                timing: None,
                raw: Some(serde_json::json!({ "runtimeControl": legacy_cursor })),
            }],
        )
        .unwrap();

        let interrupted = mark_runtime_interrupted(attempt_dir).unwrap();

        assert_ne!(interrupted.transition_id, "legacy-timeline-transition");
        assert_eq!(
            load_runtime_control_cursor(attempt_dir)
                .unwrap()
                .unwrap()
                .transition_id,
            interrupted.transition_id
        );
    }
}
