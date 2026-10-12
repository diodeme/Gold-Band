use anyhow::Result;
use gold_band::app::App;
use gold_band::storage::{read_json, write_json};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const CONVERSATION_ATTENTION_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConversationTerminalResultKind {
    Completed,
    Stopped,
    Failed,
    NewMessage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTerminalResultVm {
    pub event_id: String,
    pub run_id: String,
    pub kind: ConversationTerminalResultKind,
    pub occurred_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTerminalResultAcknowledgementVm {
    pub acknowledged: bool,
    pub unread_terminal_result: Option<ConversationTerminalResultVm>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationTerminalResultRecord {
    pub changed: bool,
    pub unread_terminal_result: Option<ConversationTerminalResultVm>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConversationAttentionState {
    version: u32,
    tasks: HashMap<String, ConversationTaskAttentionState>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConversationTaskAttentionState {
    latest_terminal_result: Option<ConversationTerminalResultVm>,
    seen_terminal_event_id: Option<String>,
    #[serde(default)]
    background_message_cursor: Option<(String, u64)>,
}

impl Default for ConversationAttentionState {
    fn default() -> Self {
        Self {
            version: CONVERSATION_ATTENTION_VERSION,
            tasks: HashMap::new(),
        }
    }
}

impl ConversationTaskAttentionState {
    fn unread_terminal_result(&self) -> Option<ConversationTerminalResultVm> {
        self.latest_terminal_result
            .as_ref()
            .filter(|result| self.seen_terminal_event_id.as_deref() != Some(&result.event_id))
            .cloned()
    }
}

impl ConversationAttentionState {
    fn unread_terminal_result(&self, task_id: &str) -> Option<ConversationTerminalResultVm> {
        self.tasks
            .get(task_id)
            .and_then(ConversationTaskAttentionState::unread_terminal_result)
    }

    fn unread_terminal_results(&self) -> HashMap<String, ConversationTerminalResultVm> {
        self.tasks
            .iter()
            .filter_map(|(task_id, state)| {
                state
                    .unread_terminal_result()
                    .map(|result| (task_id.clone(), result))
            })
            .collect()
    }
}

fn load_state(app: &App) -> Result<ConversationAttentionState> {
    let path = app.paths.conversation_attention_file();
    if !path.exists() {
        return Ok(ConversationAttentionState::default());
    }
    read_json(&path)
}

fn save_state(app: &App, state: &ConversationAttentionState) -> Result<()> {
    write_json(&app.paths.conversation_attention_file(), state)
}

pub fn unread_terminal_results(app: &App) -> Result<HashMap<String, ConversationTerminalResultVm>> {
    Ok(load_state(app)?.unread_terminal_results())
}

pub fn unread_terminal_result(
    app: &App,
    task_id: &str,
) -> Result<Option<ConversationTerminalResultVm>> {
    Ok(load_state(app)?.unread_terminal_result(task_id))
}

pub fn record_terminal_result(
    app: &App,
    task_id: &str,
    result: ConversationTerminalResultVm,
) -> Result<ConversationTerminalResultRecord> {
    let mut state = load_state(app)?;
    let task = state.tasks.entry(task_id.to_string()).or_default();
    if task
        .latest_terminal_result
        .as_ref()
        .is_some_and(|latest| latest.event_id == result.event_id)
    {
        return Ok(ConversationTerminalResultRecord {
            changed: false,
            unread_terminal_result: task.unread_terminal_result(),
        });
    }
    task.latest_terminal_result = Some(result);
    let unread_terminal_result = task.unread_terminal_result();
    state.version = CONVERSATION_ATTENTION_VERSION;
    save_state(app, &state)?;
    Ok(ConversationTerminalResultRecord {
        changed: true,
        unread_terminal_result,
    })
}

pub(crate) fn background_message_event_id(stream_key: &str, started_seq: u64) -> String {
    format!("background:{stream_key}:{started_seq}")
}

/// Called under the existing attention write lock. One background unread episode owns one
/// notification; acknowledging a streaming message does not re-arm its chunks.
pub fn record_background_message(
    app: &App,
    task_id: &str,
    stream_key: &str,
    started_seq: u64,
    mut result: ConversationTerminalResultVm,
) -> Result<(ConversationTerminalResultRecord, bool)> {
    let mut state = load_state(app)?;
    let task = state.tasks.entry(task_id.to_string()).or_default();
    if task.background_message_cursor.as_ref().is_some_and(|(stream, seq)| stream == stream_key && *seq >= started_seq) {
        return Ok((ConversationTerminalResultRecord { changed: false, unread_terminal_result: task.unread_terminal_result() }, false));
    }
    // Prompt completion has its own notification. Only an unread reply matching
    // the existing background cursor can coalesce this background notification.
    let notify = !task.background_message_cursor.as_ref().is_some_and(|(stream, seq)| {
        task.unread_terminal_result().is_some_and(|previous| {
            previous.event_id == background_message_event_id(stream, *seq)
        })
    });
    task.background_message_cursor = Some((stream_key.to_owned(), started_seq));
    // Preserve unread errors/stops; advance the ACK token so a delayed read of
    // the previous reply cannot acknowledge a newer reply.
    if let Some(previous) = task.unread_terminal_result()
        && matches!(previous.kind, ConversationTerminalResultKind::Failed | ConversationTerminalResultKind::Stopped) {
        result.kind = previous.kind;
    }
    task.latest_terminal_result = Some(result);
    let record = ConversationTerminalResultRecord { changed: true, unread_terminal_result: task.unread_terminal_result() };
    save_state(app, &state)?;
    Ok((record, notify))
}

pub fn acknowledge_terminal_result(
    app: &App,
    task_id: &str,
    event_id: &str,
) -> Result<ConversationTerminalResultAcknowledgementVm> {
    let mut state = load_state(app)?;
    let Some(task) = state.tasks.get_mut(task_id) else {
        return Ok(ConversationTerminalResultAcknowledgementVm {
            acknowledged: false,
            unread_terminal_result: None,
        });
    };
    let matches_latest = task
        .latest_terminal_result
        .as_ref()
        .is_some_and(|latest| latest.event_id == event_id);
    let already_seen = task.seen_terminal_event_id.as_deref() == Some(event_id);
    if matches_latest && !already_seen {
        task.seen_terminal_event_id = Some(event_id.to_string());
        state.version = CONVERSATION_ATTENTION_VERSION;
        save_state(app, &state)?;
        tracing::debug!(project_id = %app.paths.project_id, task_id, event_id, "conversation attention acknowledged");
    }
    Ok(ConversationTerminalResultAcknowledgementVm {
        acknowledged: matches_latest,
        unread_terminal_result: state.unread_terminal_result(task_id),
    })
}

pub fn remove_task_attention(app: &App, task_id: &str) -> Result<bool> {
    let mut state = load_state(app)?;
    if state.tasks.remove(task_id).is_none() {
        return Ok(false);
    }
    state.version = CONVERSATION_ATTENTION_VERSION;
    save_state(app, &state)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    #[test]
    fn unread_prompt_result_does_not_suppress_first_background_reply() {
        for kind in [ConversationTerminalResultKind::Completed, ConversationTerminalResultKind::Failed, ConversationTerminalResultKind::Stopped] {
            let (_root, app) = app();
            record_terminal_result(&app, "task-001", result("prompt-done", kind)).unwrap();
            let (record, notify) = record_background_message(&app, "task-001", "run/session", 135, result("background:run/session:135", ConversationTerminalResultKind::NewMessage)).unwrap();
            assert!(notify, "an unread {kind:?} prompt result must not suppress background replies");
            assert_eq!(record.unread_terminal_result.unwrap().kind, if kind == ConversationTerminalResultKind::Completed { ConversationTerminalResultKind::NewMessage } else { kind });
            assert!(!record_background_message(&app, "task-001", "run/session", 140, result("background:run/session:140", ConversationTerminalResultKind::NewMessage)).unwrap().1);
        }
    }

    #[test]
    fn read_prompt_completion_allows_a_new_background_notification() {
        let (_root, app) = app();
        record_terminal_result(&app, "task-001", result("prompt-done", ConversationTerminalResultKind::Completed)).unwrap();
        assert!(acknowledge_terminal_result(&app, "task-001", "prompt-done").unwrap().acknowledged);
        let (record, notify) = record_background_message(&app, "task-001", "run/session", 135, result("background-reply", ConversationTerminalResultKind::NewMessage)).unwrap();
        assert!(notify);
        assert_eq!(record.unread_terminal_result.unwrap().event_id, "background-reply");
        assert!(!record_background_message(&app, "task-001", "run/session", 135, result("background-reply", ConversationTerminalResultKind::NewMessage)).unwrap().1);
    }

    #[test]
    fn background_notifications_follow_unread_episodes_not_chunks_or_prompt_completions() {
        let (_root, app) = app();
        let submit = |seq| record_background_message(&app, "task-001", "run/session", seq, result(&background_message_event_id("run/session", seq), ConversationTerminalResultKind::NewMessage)).unwrap();
        assert!(submit(10).1);
        assert!(!submit(10).1);
        assert!(!submit(20).1);
        assert!(!acknowledge_terminal_result(&app, "task-001", &background_message_event_id("run/session", 10)).unwrap().acknowledged);
        assert!(acknowledge_terminal_result(&app, "task-001", &background_message_event_id("run/session", 20)).unwrap().acknowledged);
        assert!(!submit(20).1);
        assert!(submit(30).1);
        record_terminal_result(&app, "task-001", result("prompt-done", ConversationTerminalResultKind::Completed)).unwrap();
        assert!(submit(40).1);
        assert_eq!(unread_terminal_result(&app, "task-001").unwrap().unwrap().event_id, background_message_event_id("run/session", 40));
    }

    fn app() -> (tempfile::TempDir, App) {
        let root = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
        (root, App::new(path))
    }

    fn result(
        event_id: &str,
        kind: ConversationTerminalResultKind,
    ) -> ConversationTerminalResultVm {
        ConversationTerminalResultVm {
            event_id: event_id.to_string(),
            run_id: "run-001".to_string(),
            kind,
            occurred_at: "2026-08-18T10:00:00Z".to_string(),
        }
    }

    #[test]
    fn terminal_result_remains_unread_until_matching_acknowledgement() {
        let (_root, app) = app();
        let recorded = record_terminal_result(
            &app,
            "task-001",
            result("event-001", ConversationTerminalResultKind::Completed),
        )
        .unwrap();
        assert!(recorded.changed);
        assert_eq!(
            recorded
                .unread_terminal_result
                .as_ref()
                .map(|value| value.event_id.as_str()),
            Some("event-001")
        );

        let acknowledged = acknowledge_terminal_result(&app, "task-001", "event-001").unwrap();
        assert!(acknowledged.acknowledged);
        assert!(acknowledged.unread_terminal_result.is_none());
        assert!(unread_terminal_result(&app, "task-001").unwrap().is_none());
    }

    #[test]
    fn stale_acknowledgement_cannot_clear_a_newer_terminal_result() {
        let (_root, app) = app();
        record_terminal_result(
            &app,
            "task-001",
            result("event-001", ConversationTerminalResultKind::Completed),
        )
        .unwrap();
        record_terminal_result(
            &app,
            "task-001",
            result("event-002", ConversationTerminalResultKind::Failed),
        )
        .unwrap();

        let stale = acknowledge_terminal_result(&app, "task-001", "event-001").unwrap();
        assert!(!stale.acknowledged);
        assert_eq!(
            stale
                .unread_terminal_result
                .as_ref()
                .map(|value| value.event_id.as_str()),
            Some("event-002")
        );
        assert_eq!(
            unread_terminal_result(&app, "task-001")
                .unwrap()
                .map(|value| value.kind),
            Some(ConversationTerminalResultKind::Failed)
        );
    }

    #[test]
    fn replaying_an_acknowledged_event_does_not_make_it_unread_again() {
        let (_root, app) = app();
        let terminal = result("event-001", ConversationTerminalResultKind::Stopped);
        record_terminal_result(&app, "task-001", terminal.clone()).unwrap();
        acknowledge_terminal_result(&app, "task-001", "event-001").unwrap();

        let replayed = record_terminal_result(&app, "task-001", terminal).unwrap();
        assert!(!replayed.changed);
        assert!(replayed.unread_terminal_result.is_none());
    }

    #[test]
    fn workspace_attention_keeps_tasks_isolated_and_prunes_deleted_tasks() {
        let (_root, app) = app();
        record_terminal_result(
            &app,
            "task-001",
            result("event-001", ConversationTerminalResultKind::Completed),
        )
        .unwrap();
        record_terminal_result(
            &app,
            "task-002",
            result("event-002", ConversationTerminalResultKind::Failed),
        )
        .unwrap();

        let unread = unread_terminal_results(&app).unwrap();
        assert_eq!(unread.len(), 2);
        assert_eq!(unread["task-001"].event_id, "event-001");
        assert_eq!(unread["task-002"].event_id, "event-002");

        assert!(remove_task_attention(&app, "task-001").unwrap());
        assert!(!remove_task_attention(&app, "task-001").unwrap());
        let unread = unread_terminal_results(&app).unwrap();
        assert!(!unread.contains_key("task-001"));
        assert_eq!(unread["task-002"].event_id, "event-002");
    }
}
