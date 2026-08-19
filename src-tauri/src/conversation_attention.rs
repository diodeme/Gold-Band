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
